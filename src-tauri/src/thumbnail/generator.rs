// src-tauri/src/thumbnail/generator.rs
//! 统一的缩略图生成入口点（§ 8.1）。
//!
//! 管道：
//!   1. Cache hit check
//!   2. Small file direct display (thumb_status = 3)
//!   3. Dispatch by media_type
//!   4. ThumbHash generation
//!   5. Write to disk + DB update

use std::path::Path;
use tracing::{debug, info, trace, warn};

use crate::db::models::ThumbResult;
use crate::engine::EngineArena;
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{
    ai_cache_path, ensure_ai_cache_dir, ensure_thumb_dir, thumb_db_path, thumb_path,
    thumb_variant_db_path, thumb_variant_path,
};
use crate::thumbnail::scheduler::OutputFingerprint;
use crate::thumbnail::thumbhash::generate_thumbhash_rgba;

/// Valid thumbnail size tiers — 全后端唯一档位事实源(cache.rs 断言 / file_ops 重定位 /
/// scan 清理 / snap_to_tier 均引用此常量,避免多处硬编码档位漂移致 thumb_path 断言失败)。
/// 必须与前端 `src/constants/defaults.ts` 的 `THUMB_SIZE_TIERS` 保持一致。
/// ⚠️ **未经多设备/多库规模实测对比**——改档位会使全库已生成缓存作废需重产,属高代价变更;
/// 当前 5 档是经验值,开发期若要调应在发版前(避免缓存大面积失效)。
pub(crate) const THUMB_TIERS: [u32; 5] = [64, 128, 256, 512, 1024];

/// 将任意缩略图尺寸就近取整到最近的有效档位。
pub fn snap_to_tier(size: u32) -> u32 {
    THUMB_TIERS
        .iter()
        .copied()
        .min_by_key(|&t| (t as i64 - size as i64).unsigned_abs())
        .unwrap_or(256)
}

/// 在 `catch_unwind` 下运行解码/编码步骤，使第三方图像编解码器在处理
/// 损坏/畸形文件时的 panic 仅令该项失败，而非中止整个进程。
/// 需要 `panic = "unwind"`（见 Cargo.toml）。
/// 此处用 `AssertUnwindSafe` 是健全的:解码中途 panic 不会让任何共享可变状态失去一致性
/// ——只丢失在途这一张图并报为失败。
/// (2026-07-06 审查 P1-1 起 pub(crate):derive::pipeline 的 kind::run 共用——派生管线的
/// epub/lofty/MF/WIC 解码原先裸跑,单个畸形文件 panic 经 rayon scope 传播会中止整条流水线,
/// 且在途任务复位后同一毒文件反复触发。)
pub(crate) fn panic_guard<T>(label: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            warn!("[ThumbGen] PANIC caught in {label} — item failed, process kept alive | 已捕获 panic，单项失败但进程存活");
            Err(AppError::Internal(format!("panic during {label}")))
        }
    }
}

/// 原子落盘(审查 R0-4 / CLAUDE.md「派生产物一律原子落盘」):缓存命中判定只查 `exists()`
/// (见 decode_media_step 的 CACHE_HIT 分支),若直写最终路径,崩溃/断电会把半截文件留在
/// 正式路径上 → 之后每次都命中「缓存」→ UI 永久裂图。先写同目录 tmp 再 rename,保证正式
/// 路径上只可能出现完整文件。tmp 名 = pid+纳秒+进程内序号(与 exotic/sink.rs
/// `unique_tmp_path` 同范式):序号保证进程内并发批次各写各的 tmp、rename 后到者胜;
/// pid+纳秒防跨进程碰撞——应用无 single-instance 守卫,双实例可共开同库(SQLite WAL 容许),
/// 两进程序号各自从 0 起,同 cache_key 时会交错写同名 tmp、把半截文件 rename 上位,
/// 而命中判定只查 exists(),裂图会存活到下轮自愈。rename 失败即清 tmp;进程中途崩溃遗留的
/// 孤儿 tmp 不影响正确性(命中判定不认 .tmp),随缓存清理/GC(Part3 缓存治理)一并回收。
/// (T18 起 pub(crate):derive::image 的 ai_cache 生成共用,消除其直写红线违例。)
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!("{file_name}.{pid}.{nanos}.{seq}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[derive(Clone)]
pub struct ThumbConfig {
    pub cache_dir: std::path::PathBuf,
    pub size: u32,
    pub skip_max_bytes: u64,
    pub strategy: String,
    /// 为 true 时，图像缩略图路径**用同一次源解码**顺带产出 AI 分析缓存（短边≥336 的 WebP）——
    /// 一次解码两份产物 —— 使新生成缩略图的图片几乎免费获得 AI 缓存。封面派生（视频/文档/音频）为 false。
    pub ai_hq_cache: bool,
    /// 显示缩略图的 WebP 编码质量(用户设置 `thumb_webp_quality`):1..=99 有损、**100 无损**。
    /// 仅作用于显示缩略图/封面;AI·人脸分析缓存与雪碧图恒用 DEFAULT_WEBP_QUALITY。
    pub webp_quality: u8,
    /// AI 分析缓存短边(px,设置键 `ai_cache_short_edge`,批次C接线)。仅当 `ai_hq_cache` 为真时
    /// 生效(见 `decode_long_edge`/`maybe_write_ai_cache`);封面派生（视频/文档/音频）恒 `ai_hq_cache
    /// = false`,该字段对它们无效果,构造处填编译期默认常量 `cache::AI_CACHE_SHORT_EDGE` 即可。
    pub ai_cache_short_edge: u32,
    /// Coordinator 的新产物按配置摘要隔离；旧派生与既有调用保留原路径。
    pub output_fingerprint: Option<OutputFingerprint>,
}

fn output_paths(config: &ThumbConfig, cache_key: i64) -> (std::path::PathBuf, String) {
    match config.output_fingerprint {
        Some(fp) => (
            thumb_variant_path(&config.cache_dir, config.size, cache_key, fp),
            thumb_variant_db_path(config.size, cache_key, fp),
        ),
        None => (
            thumb_path(&config.cache_dir, config.size, cache_key),
            thumb_db_path(config.size, cache_key),
        ),
    }
}

fn ensure_output_dir(config: &ThumbConfig, cache_key: i64) -> Result<()> {
    if config.output_fingerprint.is_none() {
        return ensure_thumb_dir(&config.cache_dir, config.size, cache_key).map_err(AppError::Io);
    }
    let (disk_path, _) = output_paths(config, cache_key);
    let parent = disk_path
        .parent()
        .ok_or_else(|| AppError::Internal("thumbnail path has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(AppError::Io)
}

// 各变体仅作解码分派的单实例消息穿过 channel/单值传递、不批量收集进 Vec，
// 变体间尺寸差带来的内存浪费可忽略，Box 化反增解引用成本与噪声。
#[allow(clippy::large_enum_variant)]
pub enum DecodeResult {
    Ready(ThumbResult),
    Encoded(EncodedThumbPayload),
    ToEncode {
        item_id: i64,
        /// 解码时读取到的源代次，编码完成后必须原样带到条件写。
        source_revision: i64,
        /// 解码时读取到的缓存键，编码完成后必须原样带到条件写。
        cache_key: i64,
        decoded: crate::engine::traits::DecodedImage,
    },
    DeferredToCpu {
        item: crate::db::models::MediaItem,
        abs_path: std::path::PathBuf,
    },
}

pub fn process_deferred_cpu(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    panic_guard("process_deferred_cpu", || {
        process_deferred_cpu_inner(item, abs_path, arena, config)
    })
}

/// Coordinator 的 CPU 回退只执行解码；编码与写盘交给有界后段。
pub fn decode_deferred_cpu(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
    max_pixel_bytes: u64,
) -> Result<DecodeResult> {
    panic_guard("decode_deferred_cpu", || {
        let mut snapped_config = config.clone();
        snapped_config.size = snap_to_tier(config.size);
        try_cpu_decode(item, abs_path, arena, &snapped_config, max_pixel_bytes)
    })
}

fn process_deferred_cpu_inner(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    let mut snapped_config = config.clone();
    snapped_config.size = snap_to_tier(config.size);
    let config = &snapped_config;

    match try_cpu_decode(item, abs_path, arena, config, 512 * 1024 * 1024)? {
        DecodeResult::Ready(res) => Ok(res),
        DecodeResult::Encoded(payload) => write_encoded_media_payload(
            item.id,
            item.source_revision,
            item.cache_key,
            payload,
            config,
        ),
        DecodeResult::ToEncode {
            item_id,
            source_revision,
            cache_key,
            decoded,
        } => encode_media_step_with_snapshot(item_id, source_revision, cache_key, decoded, config),
        DecodeResult::DeferredToCpu { .. } => unreachable!("CPU decode cannot return Deferred"),
    }
}

pub fn decode_media_step(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
    max_pixel_bytes: u64,
) -> Result<DecodeResult> {
    panic_guard("decode_media_step", || {
        decode_media_step_inner(item, abs_path, arena, config, max_pixel_bytes)
    })
}

fn decode_media_step_inner(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
    max_pixel_bytes: u64,
) -> Result<DecodeResult> {
    let item_id = item.id;
    // target 归一(方案 §3.4/S2):per-item 高频调用点统一挂 scrollery::pipeline::thumb,配合默认
    // directive `scrollery::pipeline=warn` 在诊断态(debug/trace)也不被全库规模线性刷屏;
    // path/文件名一律结构化字段(§9.1 护栏 #6),不再插值进 message。
    trace!(
        target: "scrollery::pipeline::thumb",
        item_id,
        status = item.thumb_status,
        format = %item.file_format,
        size = item.file_size,
        media_type = %item.media_type,
        path = %abs_path.file_name().unwrap_or_default().to_string_lossy(),
        strategy = %config.strategy,
        skip_max_bytes = config.skip_max_bytes,
        "decode_media_step"
    );

    if let Some(result) = preflight_media_step(item, abs_path, config) {
        return Ok(DecodeResult::Ready(result));
    }

    // ── 3. Dispatch by media_type ─────────────────────────────────────────
    match item.media_type.as_str() {
        "image" => {
            if config.strategy == "gpu" {
                if let Some(engine) = arena.engine_for(&item.file_format) {
                    if let Some((webp, thumbhash)) = crate::thumbnail::exif_thumb::try_exif_thumb(
                        engine.as_ref(),
                        abs_path,
                        config.size,
                        config.webp_quality,
                        max_pixel_bytes,
                    ) {
                        return Ok(DecodeResult::Encoded(EncodedThumbPayload {
                            webp,
                            thumbhash,
                            ai_cache: None,
                        }));
                    }
                }
                match try_native_decode(item, abs_path, config, max_pixel_bytes) {
                    Some(Ok(res)) => Ok(res),
                    Some(Err(e)) => {
                        warn!(
                            target: "scrollery::pipeline::thumb",
                            item_id, error = %e,
                            "NATIVE_IMAGE_DECODE_FAIL: deferring to CPU"
                        );
                        Ok(DecodeResult::DeferredToCpu {
                            item: item.clone(),
                            abs_path: abs_path.to_path_buf(),
                        })
                    }
                    None => try_cpu_decode(item, abs_path, arena, config, max_pixel_bytes),
                }
            } else {
                info!(
                    target: "scrollery::pipeline::thumb",
                    item_id, format = %item.file_format, size = item.file_size,
                    "CPU_DECODE"
                );
                try_cpu_decode(item, abs_path, arena, config, max_pixel_bytes)
            }
        }
        _ => {
            debug!(
                target: "scrollery::pipeline::thumb",
                item_id, media_type = %item.media_type,
                "UNSUPPORTED_TYPE: skip"
            );
            // 阶段 2：视频/音频/文档
            Ok(DecodeResult::Ready(ThumbResult {
                item_id,
                thumb_status: 2,
                thumb_path: None,
                thumbhash: None,
                source_revision: item.source_revision,
                cache_key: item.cache_key,
            }))
        }
    }
}

/// 不进入像素执行域的缓存命中与原图直显结果，普通入口和隔离入口共用。
pub(crate) fn preflight_media_step(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    config: &ThumbConfig,
) -> Option<ThumbResult> {
    let item_id = item.id;
    // ── 1. Cache hit ──────────────────────────────────────────────────────
    if item.thumb_status == 1 {
        if let Some(ref tp) = item.thumb_path {
            let full = config.cache_dir.join("thumbnails").join(tp);
            let matches_output = config.output_fingerprint.is_none_or(|_| {
                let (_, expected) = output_paths(config, item.cache_key);
                *tp == expected
            });
            if matches_output && full.exists() {
                debug!(target: "scrollery::pipeline::thumb", item_id, path = %tp, "CACHE_HIT");
                return Some(ThumbResult {
                    item_id,
                    thumb_status: 1,
                    thumb_path: item.thumb_path.clone(),
                    thumbhash: item.thumbhash.clone(),
                    source_revision: item.source_revision,
                    cache_key: item.cache_key,
                });
            } else {
                debug!(
                    target: "scrollery::pipeline::thumb",
                    item_id, thumb_path = %tp,
                    "CACHE_MISS: file does not exist on disk"
                );
            }
        } else {
            debug!(
                target: "scrollery::pipeline::thumb",
                item_id,
                "CACHE_MISS: thumb_status=1 but thumb_path is NULL"
            );
        }
    }

    // ── 2. Small file direct display ─────────────────────────────────────
    if let Some(direct_reason) = direct_display_reason(item, config) {
        info!(
            target: "scrollery::pipeline::thumb",
            item_id,
            reason = direct_reason,
            format = %item.file_format,
            size = item.file_size,
            skip_max_bytes = config.skip_max_bytes,
            "DIRECT_DISPLAY: skip generation, use source file directly"
        );
        // 直显只复用已提交的占位数据；为补 hash 解码会把 Q0 重新变成像素任务。
        let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
        return Some(ThumbResult {
            item_id,
            thumb_status: 3,
            thumb_path: Some(abs_path_str),
            thumbhash: item.thumbhash.clone(),
            source_revision: item.source_revision,
            cache_key: item.cache_key,
        });
    }
    None
}

/// 调度器也使用同一判定，使直显项不占用像素工作集预算。
pub(crate) fn direct_display_reason(
    item: &crate::db::models::MediaItem,
    config: &ThumbConfig,
) -> Option<&'static str> {
    let web_safe = matches!(
        item.file_format.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "svg" | "avif"
    );
    if !web_safe || item.media_type != "image" {
        return None;
    }
    if config.strategy == "direct" {
        return Some("strategy=direct");
    }
    (config.skip_max_bytes > 0
        && item.file_size >= 0
        && item.file_size as u64 <= config.skip_max_bytes)
        .then_some("file_size<=skip_max_bytes")
}

fn try_native_decode(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    config: &ThumbConfig,
    max_pixel_bytes: u64,
) -> Option<Result<DecodeResult>> {
    let native = crate::engine::native::get_native_image_engine("wic")?;
    if !native.can_handle(&item.file_format) {
        return None;
    }
    let decoded = native.decode_bounded(
        abs_path,
        Some(crate::engine::traits::ResizeHint::LongEdge(
            decode_long_edge(config, item, needs_ai_cache(config, item.cache_key)),
        )),
        max_pixel_bytes,
    );
    Some(decoded.map(|decoded| DecodeResult::ToEncode {
        item_id: item.id,
        source_revision: item.source_revision,
        cache_key: item.cache_key,
        decoded,
    }))
}

/// 用于源解码的 LongEdge 目标。常态即 `config.size`。但当 AI 高清缓存开启且图像为「宽幅」
/// （其缩略图短边会低于 AI 缓存短边）时，把解码长边略放大，使同一缓冲既能产出缩略图（降采样到
/// `size`）又能产出 AI 缓存（降采样到短边 `AI_CACHE_SHORT_EDGE`），免去 `ai_thumb` 派生再做一次
/// 全分辨率源解码。绝不上采样（WIC LongEdge 仅下采样）。
pub(crate) fn needs_ai_cache(config: &ThumbConfig, cache_key: i64) -> bool {
    config.ai_hq_cache && !ai_cache_path(&config.cache_dir, cache_key).exists()
}

pub(crate) fn decode_long_edge(
    config: &ThumbConfig,
    item: &crate::db::models::MediaItem,
    need_ai_cache: bool,
) -> u32 {
    let (w, h) = (item.width as u32, item.height as u32);
    let (long, short) = (w.max(h), w.min(h));
    if !need_ai_cache || long == 0 || short == 0 {
        return config.size;
    }
    let thumb_short = (short as f32 * config.size as f32 / long as f32).round() as u32;
    if thumb_short >= config.ai_cache_short_edge {
        config.size // 缩略图短边已≥AI 缓存短边，无需为 AI 缓存放大解码
    } else {
        ((config.ai_cache_short_edge as f32 * long as f32 / short as f32).ceil() as u32)
            .max(config.size)
    }
}

fn try_cpu_decode(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
    max_pixel_bytes: u64,
) -> Result<DecodeResult> {
    let engine = arena
        .engine_for(&item.file_format)
        .ok_or_else(|| AppError::UnsupportedFormat(item.file_format.clone()))?;

    // 优先走快速 EXIF 路径
    if let Some((webp, hash)) = crate::thumbnail::exif_thumb::try_exif_thumb(
        engine.as_ref(),
        abs_path,
        config.size,
        config.webp_quality,
        max_pixel_bytes,
    ) {
        return Ok(DecodeResult::Encoded(EncodedThumbPayload {
            webp,
            thumbhash: hash,
            ai_cache: None,
        }));
    }

    // 完整源解码后缩到目标档位；具体后端可提供解码期降采样。
    // 复用 GPU 路径同一 `decode_long_edge`（而非裸 `snap_to_tier`）：AI 高清缓存开启且宽幅图时
    // 解码略大，使 encode 阶段同一缓冲既出缩略图又出 AI 缓存（一次解码两份产物），否则即 config.size。
    // image crate 的 LongEdge 仅下采样不上采样（image_rs.rs:57）→ 小图不被放大；常见情形下
    // `resize_to_rgba` 因 w/h<=target 短路返回，省掉二次缩放（与 GPU 路径输出语义一致，同 CatmullRom）。
    let decoded = engine.decode_bounded(
        abs_path,
        Some(crate::engine::traits::ResizeHint::LongEdge(
            decode_long_edge(config, item, needs_ai_cache(config, item.cache_key)),
        )),
        max_pixel_bytes,
    )?;
    Ok(DecodeResult::ToEncode {
        item_id: item.id,
        source_revision: item.source_revision,
        cache_key: item.cache_key,
        decoded,
    })
}

/// 旧的无源快照编码入口，供视频/音频/文档等非 worker 派生调用点继续使用。
///
/// 该入口无法证明生产时的 `source_revision`，返回结果使用 `0` 作为未知标记；调用方
/// 必须继续使用不带源快照条件的 [`crate::db::queries::update_thumb_result`]。异步 worker
/// 结果必须改用 [`encode_media_step_with_snapshot`]，再走条件写 API。
pub fn encode_media_step(
    item_id: i64,
    cache_key: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    panic_guard("encode_media_step", move || {
        encode_media_step_inner(item_id, 0, cache_key, decoded, config)
    })
}

/// 带生产时源快照的编码入口，供缩略图 worker 将结果交给条件写 API。
pub fn encode_media_step_with_snapshot(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    panic_guard("encode_media_step_with_snapshot", move || {
        encode_media_step_inner(item_id, source_revision, cache_key, decoded, config)
    })
}

/// 仅含目标小图的编码结果；原生隔离域可返回此结构的字节，文件路径由宿主确定。
pub struct EncodedThumbPayload {
    pub webp: Vec<u8>,
    pub thumbhash: Option<Vec<u8>>,
    pub ai_cache: Option<Vec<u8>>,
}

/// 隔离进程回包和宿主有界提交队列共用的单产物上限。
pub const MAX_ENCODED_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

/// CPU 后处理分项数；顺序为 AI 缓存、缩放、色彩、ThumbHash、图片编码。
pub const ENCODE_STAGE_COUNT: usize = 5;

/// 编码目标缩略图和可选 AI 小图，不执行文件操作。
pub fn encode_media_payload(
    item_id: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
    emit_ai_cache: bool,
) -> Result<EncodedThumbPayload> {
    encode_media_payload_with_timings(item_id, decoded, config, emit_ai_cache)
        .map(|(payload, _)| payload)
}

/// 返回相同编码产物及 CPU 后处理各阶段的微秒耗时，顺序见 ENCODE_STAGE_COUNT。
/// AI 阶段包含按需生成的检查；图片编码包含既有失败回退。失败时不返回计时。
pub fn encode_media_payload_with_timings(
    item_id: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
    emit_ai_cache: bool,
) -> Result<(EncodedThumbPayload, [u64; ENCODE_STAGE_COUNT])> {
    panic_guard("encode_media_payload", move || {
        encode_media_payload_inner(item_id, decoded, config, emit_ai_cache)
    })
}

fn encode_media_payload_inner(
    item_id: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
    emit_ai_cache: bool,
) -> Result<(EncodedThumbPayload, [u64; ENCODE_STAGE_COUNT])> {
    let mut timings = [0; ENCODE_STAGE_COUNT];
    let micros =
        |start: std::time::Instant| u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
    let started = std::time::Instant::now();
    // 次要产物失败不影响封面；文件存在性由宿主决定，编码域只消费像素。
    let mut ai_cache = if emit_ai_cache {
        match maybe_encode_ai_cache(&decoded, config) {
            Ok(bytes) => bytes,
            Err(error) => {
                warn!(target: "scrollery::pipeline::thumb", item_id, %error,
                    "AI cache encode failed (best-effort)");
                None
            }
        }
    } else {
        None
    };
    if emit_ai_cache {
        timings[0] = micros(started);
    }
    let started = std::time::Instant::now();
    let rgba_img = resize_to_rgba(decoded.pixels, decoded.width, decoded.height, config.size)?;
    timings[1] = micros(started);
    let started = std::time::Instant::now();
    let rgba_img = crate::editing::color::project_rgba8_to_srgb(rgba_img, decoded.icc.as_deref());
    timings[2] = micros(started);
    let started = std::time::Instant::now();
    let thumbhash =
        generate_thumbhash_rgba(rgba_img.as_raw(), rgba_img.width(), rgba_img.height()).ok();
    timings[3] = micros(started);
    let started = std::time::Instant::now();
    let webp = crate::thumbnail::exif_thumb::encode_as_webp(&rgba_img, config.webp_quality)
        .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&rgba_img))
        .map_err(|_| AppError::Internal("WebP encode failed".into()))?;
    timings[4] = micros(started);
    if webp.len() > MAX_ENCODED_ARTIFACT_BYTES {
        return Err(AppError::Internal(
            "encoded thumbnail exceeds result limit".into(),
        ));
    }
    if ai_cache
        .as_ref()
        .is_some_and(|bytes| bytes.len() > MAX_ENCODED_ARTIFACT_BYTES)
    {
        warn!(target: "scrollery::pipeline::thumb", item_id,
            "AI cache exceeds result limit (best-effort)");
        ai_cache = None;
    }
    Ok((
        EncodedThumbPayload {
            webp,
            thumbhash,
            ai_cache,
        },
        timings,
    ))
}

/// 宿主按可信配置确定路径，并将编码结果原子写入缓存。
pub fn write_encoded_media_payload(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    mut payload: EncodedThumbPayload,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    let t0 = std::time::Instant::now();
    if payload.webp.len() > MAX_ENCODED_ARTIFACT_BYTES {
        return Err(AppError::Internal(
            "encoded thumbnail exceeds result limit".into(),
        ));
    }
    if payload
        .ai_cache
        .as_ref()
        .is_some_and(|bytes| bytes.len() > MAX_ENCODED_ARTIFACT_BYTES)
    {
        payload.ai_cache = None;
    }
    if let Some(bytes) = payload.ai_cache {
        let disk = ai_cache_path(&config.cache_dir, cache_key);
        if !disk.exists() {
            let result = ensure_ai_cache_dir(&config.cache_dir, cache_key)
                .and_then(|_| write_atomic(&disk, &bytes));
            if let Err(error) = result {
                warn!(target: "scrollery::pipeline::thumb", item_id, %error,
                    "AI cache write failed (best-effort)");
            }
        }
    }
    ensure_output_dir(config, cache_key)?;
    let (disk_path, db_path) = output_paths(config, cache_key);
    write_atomic(&disk_path, &payload.webp).map_err(AppError::from)?;
    info!(
        target: "scrollery::pipeline::thumb",
        item_id, cache_key,
        disk_path = %disk_path.display(),
        db_path = %db_path,
        size_bytes = payload.webp.len() as u64,
        elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0,
        "ENCODE_OK"
    );
    Ok(ThumbResult {
        item_id,
        thumb_status: 1,
        thumb_path: Some(db_path),
        thumbhash: payload.thumbhash,
        source_revision,
        cache_key,
    })
}

fn encode_media_step_inner(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    let emit_ai_cache = needs_ai_cache(config, cache_key);
    let (payload, _) = encode_media_payload_inner(item_id, decoded, config, emit_ai_cache)?;
    write_encoded_media_payload(item_id, source_revision, cache_key, payload, config)
}

fn resize_to_rgba(pixels: Vec<u8>, w: u32, h: u32, target: u32) -> Result<image::RgbaImage> {
    if w <= target && h <= target {
        return image::RgbaImage::from_raw(w, h, pixels)
            .ok_or_else(|| AppError::Internal("resize buffer mismatch".into()));
    }

    use fast_image_resize::pixels::PixelType;
    use fast_image_resize::{
        images::{Image as FirImage, ImageRef},
        ResizeOptions, Resizer,
    };

    let (new_w, new_h) = if w >= h {
        let r = target as f32 / w as f32;
        (target, (h as f32 * r).round() as u32)
    } else {
        let r = target as f32 / h as f32;
        ((w as f32 * r).round() as u32, target)
    };

    let src = ImageRef::new(w.max(1), h.max(1), &pixels, PixelType::U8x4)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut dst = FirImage::new(new_w.max(1), new_h.max(1), PixelType::U8x4);

    use fast_image_resize::{FilterType, ResizeAlg};
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));

    let mut resizer = Resizer::new();
    resizer
        .resize(&src, &mut dst, &options)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    image::RgbaImage::from_raw(new_w.max(1), new_h.max(1), dst.into_vec())
        .ok_or_else(|| AppError::Internal("resize buffer mismatch".into()))
}

/// 一次解码两份产物:从**已解码**图产出 AI 分析缓存,无需额外源解码。仅对缩略图短边会落到
/// `AI_CACHE_SHORT_EDGE` 以下的宽幅图生效(方图缩略图已满足分析需求 → 再产会浪费磁盘)。
/// 要求解码缓冲短边 ≥ 目标(不放大/不上采样)。AI 流水线按 `cache_key` 发现此文件
/// (见 `db::queries::PendingAiItem`)。尽力而为。
fn maybe_encode_ai_cache(
    decoded: &crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<Option<Vec<u8>>> {
    let (w, h) = (decoded.width, decoded.height);
    let (long, short) = (w.max(h), w.min(h));
    if long == 0 || short == 0 {
        return Ok(None);
    }
    // 仅当：缓冲短边已 ≥ AI 缓存短边（够大、无需上采样）且缩略图短边 < AI 缓存短边（较方图不必建）。
    let thumb_short = (short as f32 * config.size as f32 / long as f32).round() as u32;
    if short < config.ai_cache_short_edge || thumb_short >= config.ai_cache_short_edge {
        return Ok(None);
    }

    let rgba = resize_short_edge_rgba(&decoded.pixels, w, h, config.ai_cache_short_edge)?;
    // 同缩略图路径,AI 分析缓存也消费同一 DecodedImage(带 icc),编码前投影到 sRGB。
    let rgba = crate::editing::color::project_rgba8_to_srgb(rgba, decoded.icc.as_deref());
    // AI 分析缓存质量与显示设置解耦(分析精度不随显示质量起落),恒用默认值。
    let webp = crate::thumbnail::exif_thumb::encode_as_webp(
        &rgba,
        crate::thumbnail::exif_thumb::DEFAULT_WEBP_QUALITY,
    )
    .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&rgba))
    .map_err(|_| AppError::Internal("AI cache WebP encode failed".into()))?;
    Ok(Some(webp))
}

/// 缩放 RGBA 像素使短边变为 `target_short`(保持比例)。绝不放大——调用方保证短边 ≥ 目标。
/// fast_image_resize 双线性(与 `resize_to_rgba` 一致)。
fn resize_short_edge_rgba(
    pixels: &[u8],
    w: u32,
    h: u32,
    target_short: u32,
) -> Result<image::RgbaImage> {
    if w.min(h) <= target_short {
        return image::RgbaImage::from_raw(w, h, pixels.to_vec())
            .ok_or_else(|| AppError::Internal("ai cache buffer mismatch".into()));
    }

    use fast_image_resize::pixels::PixelType;
    use fast_image_resize::{
        images::{Image as FirImage, ImageRef},
        FilterType, ResizeAlg, ResizeOptions, Resizer,
    };

    let scale = target_short as f32 / w.min(h) as f32;
    let new_w = ((w as f32 * scale).round() as u32).max(1);
    let new_h = ((h as f32 * scale).round() as u32).max(1);

    // `ImageRef` 只读源视图直接借用解码缓冲(fir 4 已支持),免去此前 `to_vec()` 整幅拷贝
    // (缓冲经 decode_long_edge 有界,短边≈336,量级 1~15MB,宽幅图每次派生都走此路径)。
    let src = ImageRef::new(w.max(1), h.max(1), pixels, PixelType::U8x4)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let mut dst = FirImage::new(new_w, new_h, PixelType::U8x4);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
    Resizer::new()
        .resize(&src, &mut dst, &options)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    image::RgbaImage::from_raw(new_w, new_h, dst.into_vec())
        .ok_or_else(|| AppError::Internal("ai cache buffer mismatch".into()))
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use fast_image_resize::{
        images::{Image as FirImage, ImageRef},
        pixels::PixelType,
        ResizeOptions, Resizer,
    };

    #[test]
    fn image_variants_keep_task_identity_and_serve_smaller_output() {
        // 同一缩略图产物流程锁住占位图比例、颜色和透明度；参考为原先的Lanczos3降采样。
        let rgba = image::RgbaImage::from_fn(512, 256, |x, y| {
            image::Rgba([
                (x / 2) as u8,
                y as u8,
                ((x + y) / 3) as u8,
                if x < 128 { 0 } else { 255 },
            ])
        });
        let source = ImageRef::new(512, 256, rgba.as_raw(), PixelType::U8x4).unwrap();
        let mut reference = FirImage::new(100, 50, PixelType::U8x4);
        let started = std::time::Instant::now();
        Resizer::new()
            .resize(&source, &mut reference, &ResizeOptions::default())
            .unwrap();
        let resize_us = started.elapsed().as_micros();
        let started = std::time::Instant::now();
        let reference_hash = thumbhash::rgba_to_thumb_hash(100, 50, reference.buffer());
        let hash_us = started.elapsed().as_micros();
        let started = std::time::Instant::now();
        let actual_hash = generate_thumbhash_rgba(rgba.as_raw(), 512, 256).unwrap();
        eprintln!(
            "synthetic_thumbhash legacy_resize_us={resize_us} hash_us={hash_us} actual_us={}",
            started.elapsed().as_micros()
        );
        assert_eq!(actual_hash, reference_hash, "透明输入保持原占位图字节");
        let mut opaque = rgba.clone();
        for pixel in opaque.pixels_mut() {
            pixel.0[3] = 255;
        }
        let opaque_source = ImageRef::new(512, 256, opaque.as_raw(), PixelType::U8x4).unwrap();
        let started = std::time::Instant::now();
        Resizer::new()
            .resize(&opaque_source, &mut reference, &ResizeOptions::default())
            .unwrap();
        let opaque_reference = thumbhash::rgba_to_thumb_hash(100, 50, reference.buffer());
        let legacy_us = started.elapsed().as_micros();
        let started = std::time::Instant::now();
        let opaque_hash = generate_thumbhash_rgba(opaque.as_raw(), 512, 256).unwrap();
        eprintln!(
            "opaque_thumbhash legacy_us={legacy_us} actual_us={}",
            started.elapsed().as_micros()
        );
        assert_eq!(
            opaque_hash, opaque_reference,
            "跳过恒等alpha乘除不改变占位图字节"
        );
        let average = thumbhash::thumb_hash_to_average_rgba(&actual_hash).unwrap();
        assert!((0.70..0.80).contains(&average.3));
        assert!(average.0 > 0.55 && average.1 > 0.4 && average.2 > 0.4);
        assert!(thumbhash::thumb_hash_to_approximate_aspect_ratio(&actual_hash).unwrap() > 1.5);
        let narrow = image::RgbaImage::from_pixel(1, 512, image::Rgba([0, 80, 200, 255]));
        let narrow_hash = generate_thumbhash_rgba(narrow.as_raw(), 1, 512).unwrap();
        assert!(thumbhash::thumb_hash_to_approximate_aspect_ratio(&narrow_hash).unwrap() < 0.2);
        let directory = tempfile::tempdir().unwrap();
        let mut config = ThumbConfig {
            cache_dir: directory.path().to_path_buf(),
            size: 512,
            skip_max_bytes: 0,
            strategy: "cpu".into(),
            ai_hq_cache: false,
            webp_quality: 80,
            ai_cache_short_edge: 336,
            output_fingerprint: None,
        };
        config.output_fingerprint = Some(OutputFingerprint::for_image(&config));
        let large_fp = config.output_fingerprint;
        let (_, large) = output_paths(&config, 91);
        config.size = 256;
        config.output_fingerprint = Some(OutputFingerprint::for_image(&config));
        let (_, small) = output_paths(&config, 91);
        assert_ne!(large_fp, config.output_fingerprint);
        let write = |config: &ThumbConfig| {
            write_encoded_media_payload(
                1,
                1,
                91,
                EncodedThumbPayload {
                    webp: vec![1, 2, 3],
                    thumbhash: None,
                    ai_cache: None,
                },
                config,
            )
            .unwrap()
        };
        assert_eq!(write(&config).thumb_path.as_deref(), Some(small.as_str()));
        let mut large_config = config.clone();
        large_config.size = 512;
        large_config.output_fingerprint = large_fp;
        assert_eq!(
            write(&large_config).thumb_path.as_deref(),
            Some(large.as_str())
        );
        let probes = crate::thumbnail::serve::ThumbProbeCache::new(std::time::Duration::ZERO);
        let serve = crate::thumbnail::serve::ThumbServe::prepare_with(
            &crate::thumbnail::serve::RealServeIo,
            &probes,
            directory.path(),
            1.0,
            &[crate::thumbnail::serve::ServeRequest {
                cache_key: 91,
                need_px: 128,
                db_path: large.clone(),
            }],
            std::time::Instant::now(),
        );
        assert_eq!(serve.rewrite_path(&large, 128.0, 128.0, 91), Some(small));
        config.webp_quality = 90;
        config.output_fingerprint = Some(OutputFingerprint::for_image(&config));
        let (_, other) = output_paths(&config, 91);
        assert_eq!(serve.rewrite_path(&other, 128.0, 128.0, 91), None);
        assert_eq!(
            OutputFingerprint::from_hex(&large_fp.unwrap().hex()),
            large_fp
        );
        config.webp_quality = 80;
        config.size = 512;
        config.output_fingerprint = Some(OutputFingerprint::for_native_video_cover(&config));
        let video_large = write(&config).thumb_path.unwrap();
        config.size = 256;
        config.output_fingerprint = Some(OutputFingerprint::for_native_video_cover(&config));
        let video_small = write(&config).thumb_path.unwrap();
        let serve = crate::thumbnail::serve::ThumbServe::prepare_with(
            &crate::thumbnail::serve::RealServeIo,
            &probes,
            directory.path(),
            1.0,
            &[crate::thumbnail::serve::ServeRequest {
                cache_key: 91,
                need_px: 128,
                db_path: video_large.clone(),
            }],
            std::time::Instant::now(),
        );
        assert_eq!(
            serve.rewrite_path(&video_large, 128.0, 128.0, 91),
            Some(video_small)
        );
        let legacy = crate::thumbnail::cache::thumb_db_path(512, 91);
        assert_eq!(serve.rewrite_path(&legacy, 128.0, 128.0, 91), None);
    }
}
