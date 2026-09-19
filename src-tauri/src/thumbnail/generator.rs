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
};
use crate::thumbnail::thumbhash::generate_thumbhash;

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
    pub gpu_engine: String,
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
}

// 各变体仅作解码分派的单实例消息穿过 channel/单值传递、不批量收集进 Vec，
// 变体间尺寸差带来的内存浪费可忽略，Box 化反增解引用成本与噪声。
#[allow(clippy::large_enum_variant)]
pub enum DecodeResult {
    Ready(ThumbResult),
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

fn process_deferred_cpu_inner(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    let mut snapped_config = config.clone();
    snapped_config.size = snap_to_tier(config.size);
    let config = &snapped_config;

    match try_cpu_decode(item, abs_path, arena, config)? {
        DecodeResult::Ready(res) => Ok(res),
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
) -> Result<DecodeResult> {
    panic_guard("decode_media_step", || {
        decode_media_step_inner(item, abs_path, arena, config)
    })
}

fn decode_media_step_inner(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    arena: &EngineArena,
    config: &ThumbConfig,
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

    // ── 1. Cache hit ──────────────────────────────────────────────────────
    if item.thumb_status == 1 {
        if let Some(ref tp) = item.thumb_path {
            let full = config.cache_dir.join("thumbnails").join(tp);
            if full.exists() {
                debug!(target: "scrollery::pipeline::thumb", item_id, path = %tp, "CACHE_HIT");
                return Ok(DecodeResult::Ready(ThumbResult {
                    item_id,
                    thumb_status: 1,
                    thumb_path: item.thumb_path.clone(),
                    thumbhash: item.thumbhash.clone(),
                    source_revision: item.source_revision,
                    cache_key: item.cache_key,
                }));
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
    let web_safe_formats = ["jpg", "jpeg", "png", "webp", "gif", "svg", "avif"];
    let is_web_safe = web_safe_formats.contains(&item.file_format.to_lowercase().as_str());

    let mut is_direct = false;
    let mut direct_reason = "";
    if config.strategy == "direct" && is_web_safe && item.media_type == "image" {
        is_direct = true;
        direct_reason = "strategy=direct";
    } else if is_web_safe
        && item.file_size as u64 <= config.skip_max_bytes
        && item.media_type == "image"
    {
        is_direct = true;
        direct_reason = "file_size<=skip_max_bytes";
    }

    if is_direct {
        info!(
            target: "scrollery::pipeline::thumb",
            item_id,
            reason = direct_reason,
            format = %item.file_format,
            size = item.file_size,
            skip_max_bytes = config.skip_max_bytes,
            "DIRECT_DISPLAY: skip generation, use source file directly"
        );
        // 占位图（thumbhash）是体验底线，不应因 strategy=="direct" 而丢失（Part3 Q14 / §3.1.3）：
        // 去掉原 `strategy != "direct"` 短路，两条 direct 路径均在文件 ≤500KB 时生成占位
        // （500KB 守卫保留：避免仅为占位图去全解码大文件）。
        let mut hash = None;
        if item.file_size <= 500 * 1024 {
            if let Some(engine) = arena.engine_for(&item.file_format) {
                if let Ok(decoded) = engine.decode(abs_path, None) {
                    hash = generate_thumbhash(&decoded).ok();
                }
            }
        }

        let abs_path_str = abs_path.to_string_lossy().replace('\\', "/");
        return Ok(DecodeResult::Ready(ThumbResult {
            item_id,
            thumb_status: 3,
            thumb_path: Some(abs_path_str),
            thumbhash: hash,
            source_revision: item.source_revision,
            cache_key: item.cache_key,
        }));
    }

    // ── 3. Dispatch by media_type ─────────────────────────────────────────
    match item.media_type.as_str() {
        "image" => {
            if config.strategy == "gpu" {
                info!(
                    target: "scrollery::pipeline::thumb",
                    item_id, format = %item.file_format, size = item.file_size,
                    "GPU_DECODE"
                );
                match try_gpu_decode(item, abs_path, config) {
                    Ok(res) => {
                        info!(target: "scrollery::pipeline::thumb", item_id, "GPU_DECODE_OK");
                        Ok(res)
                    }
                    Err(e) => {
                        warn!(
                            target: "scrollery::pipeline::thumb",
                            item_id, error = %e,
                            "GPU_DECODE_FAIL: deferring to CPU"
                        );
                        Ok(DecodeResult::DeferredToCpu {
                            item: item.clone(),
                            abs_path: abs_path.to_path_buf(),
                        })
                    }
                }
            } else {
                info!(
                    target: "scrollery::pipeline::thumb",
                    item_id, format = %item.file_format, size = item.file_size,
                    "CPU_DECODE"
                );
                try_cpu_decode(item, abs_path, arena, config)
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

fn try_gpu_decode(
    item: &crate::db::models::MediaItem,
    abs_path: &Path,
    config: &ThumbConfig,
) -> Result<DecodeResult> {
    let gpu_engine = crate::engine::gpu::get_gpu_engine(&config.gpu_engine)
        .ok_or_else(|| AppError::Internal(format!("Unknown GPU engine: {}", config.gpu_engine)))?;

    if !gpu_engine.can_handle(&item.file_format) {
        return Err(AppError::UnsupportedFormat(item.file_format.clone()));
    }

    let decoded = gpu_engine.decode(
        abs_path,
        Some(crate::engine::traits::ResizeHint::LongEdge(
            decode_long_edge(config, item),
        )),
    )?;
    Ok(DecodeResult::ToEncode {
        item_id: item.id,
        source_revision: item.source_revision,
        cache_key: item.cache_key,
        decoded,
    })
}

/// 用于（GPU）源解码的 LongEdge 目标。常态即 `config.size`。但当 AI 高清缓存开启且图像为「宽幅」
/// （其缩略图短边会低于 AI 缓存短边）时，把解码长边略放大，使同一缓冲既能产出缩略图（降采样到
/// `size`）又能产出 AI 缓存（降采样到短边 `AI_CACHE_SHORT_EDGE`），免去 `ai_thumb` 派生再做一次
/// 全分辨率源解码。绝不上采样（WIC LongEdge 仅下采样）。
fn decode_long_edge(config: &ThumbConfig, item: &crate::db::models::MediaItem) -> u32 {
    let (w, h) = (item.width as u32, item.height as u32);
    let (long, short) = (w.max(h), w.min(h));
    if !config.ai_hq_cache || long == 0 || short == 0 {
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
    ) {
        ensure_thumb_dir(&config.cache_dir, config.size, item.cache_key).map_err(AppError::Io)?;
        let disk_path = thumb_path(&config.cache_dir, config.size, item.cache_key);
        write_atomic(&disk_path, &webp).map_err(AppError::from)?;

        let db_path = thumb_db_path(config.size, item.cache_key);
        return Ok(DecodeResult::Ready(ThumbResult {
            item_id: item.id,
            thumb_status: 1,
            thumb_path: Some(db_path),
            thumbhash: hash,
            source_revision: item.source_revision,
            cache_key: item.cache_key,
        }));
    }

    // Full decode fallback —— 解码期即降采样到目标档位（Part3 Q1 / §3.1.1）。
    // 复用 GPU 路径同一 `decode_long_edge`（而非裸 `snap_to_tier`）：AI 高清缓存开启且宽幅图时
    // 解码略大，使 encode 阶段同一缓冲既出缩略图又出 AI 缓存（一次解码两份产物），否则即 config.size。
    // image crate 的 LongEdge 仅下采样不上采样（image_rs.rs:57）→ 小图不被放大；常见情形下
    // `resize_to_rgba` 因 w/h<=target 短路返回，省掉二次缩放（与 GPU 路径输出语义一致，同 CatmullRom）。
    let decoded = engine.decode(
        abs_path,
        Some(crate::engine::traits::ResizeHint::LongEdge(
            decode_long_edge(config, item),
        )),
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

fn encode_media_step_inner(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    mut decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    let t0 = std::time::Instant::now();

    // 一次解码两份产物:缩略图消费缓冲前,顺手从同一份解码结果产出 AI 分析缓存(尽力而为,
    // 失败不影响缩略图)。仅对缩略图短边 < AI 短边的宽幅图生效(方图缩略图已满足分析需求,
    // 再产 AI 缓存只是浪费磁盘)。
    if config.ai_hq_cache {
        if let Err(e) = maybe_write_ai_cache(cache_key, &decoded, config) {
            warn!(
                target: "scrollery::pipeline::thumb",
                item_id, error = %e,
                "AI cache emit failed (best-effort, does not fail the thumbnail)"
            );
        }
    }

    let rgba_img = resize_to_rgba(
        &mut decoded.pixels,
        decoded.width,
        decoded.height,
        config.size,
    )?;

    // ICC→sRGB 投影(D-410):在小图(resize 之后)上做,成本最小;与 resize 先后顺序
    // 造成的色差与现状量级相当,可接受。无 ICC / 解析失败 / 非 RGB 空间一律原样放行。
    let rgba_img = crate::editing::color::project_rgba8_to_srgb(rgba_img, decoded.icc.as_deref());

    let decoded_for_hash = crate::engine::traits::DecodedImage {
        pixels: rgba_img.as_raw().clone(),
        width: rgba_img.width(),
        height: rgba_img.height(),
        icc: None, // 仅用于 ThumbHash 计算,ICC 与此无关
    };
    let final_hash = generate_thumbhash(&decoded_for_hash).ok();

    let webp = crate::thumbnail::exif_thumb::encode_as_webp(&rgba_img, config.webp_quality)
        .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&rgba_img))
        .map_err(|_| AppError::Internal("WebP encode failed".into()))?;

    ensure_thumb_dir(&config.cache_dir, config.size, cache_key).map_err(AppError::Io)?;
    let disk_path = thumb_path(&config.cache_dir, config.size, cache_key);
    write_atomic(&disk_path, &webp).map_err(AppError::from)?;

    let db_path = thumb_db_path(config.size, cache_key);
    info!(
        target: "scrollery::pipeline::thumb",
        item_id, cache_key,
        disk_path = %disk_path.display(),
        db_path = %db_path,
        size_bytes = webp.len() as u64,
        elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0,
        "ENCODE_OK"
    );

    Ok(ThumbResult {
        item_id,
        thumb_status: 1,
        thumb_path: Some(db_path),
        thumbhash: final_hash,
        source_revision,
        cache_key,
    })
}

fn resize_to_rgba(pixels: &mut [u8], w: u32, h: u32, target: u32) -> Result<image::RgbaImage> {
    if w <= target && h <= target {
        return image::RgbaImage::from_raw(w, h, pixels.to_vec())
            .ok_or_else(|| AppError::Internal("resize buffer mismatch".into()));
    }

    use fast_image_resize::pixels::PixelType;
    use fast_image_resize::{images::Image as FirImage, ResizeOptions, Resizer};

    let (new_w, new_h) = if w >= h {
        let r = target as f32 / w as f32;
        (target, (h as f32 * r).round() as u32)
    } else {
        let r = target as f32 / h as f32;
        ((w as f32 * r).round() as u32, target)
    };

    let src = FirImage::from_slice_u8(w.max(1), h.max(1), pixels, PixelType::U8x4)
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
fn maybe_write_ai_cache(
    cache_key: i64,
    decoded: &crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<()> {
    let (w, h) = (decoded.width, decoded.height);
    let (long, short) = (w.max(h), w.min(h));
    if long == 0 || short == 0 {
        return Ok(());
    }
    // 仅当：缓冲短边已 ≥ AI 缓存短边（够大、无需上采样）且缩略图短边 < AI 缓存短边（较方图不必建）。
    let thumb_short = (short as f32 * config.size as f32 / long as f32).round() as u32;
    if short < config.ai_cache_short_edge || thumb_short >= config.ai_cache_short_edge {
        return Ok(());
    }
    let disk = ai_cache_path(&config.cache_dir, cache_key);
    if disk.exists() {
        return Ok(()); // 已存在 → 跳过缩放/编码/写盘
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
    ensure_ai_cache_dir(&config.cache_dir, cache_key).map_err(AppError::Io)?;
    write_atomic(&disk, &webp).map_err(AppError::from)?;
    Ok(())
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
mod tests {
    use super::*;

    /// 短边缩放契约:短边降到目标且保比例;短边已≤目标则原样返回(绝不放大)。
    #[test]
    fn resize_short_edge_downscales_and_never_upscales() {
        let (w, h) = (100u32, 50u32);
        let pixels = vec![128u8; (w * h * 4) as usize];

        let out = resize_short_edge_rgba(&pixels, w, h, 25).unwrap();
        assert_eq!((out.width(), out.height()), (50, 25));

        // 短边 50 ≤ 目标 50:原样返回,不触碰缩放路径。
        let out = resize_short_edge_rgba(&pixels, w, h, 50).unwrap();
        assert_eq!((out.width(), out.height()), (100, 50));
    }

    /// R0-4:成功路径——最终文件内容完整、目录内不残留任何 .tmp。
    #[test]
    fn write_atomic_leaves_complete_file_and_no_tmp() {
        let dir = std::env::temp_dir().join(format!("scrollery_wa_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("a1b2c3.webp");

        write_atomic(&target, b"hello-webp").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello-webp");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no .tmp may remain after success");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R0-4:覆盖路径——目标已存在(含模拟的「半截旧文件」)时 rename 原子替换为新完整内容。
    /// 这正是崩溃恢复场景:上一次直写留下的截断文件,重生成后必须被完整文件顶掉。
    #[test]
    fn write_atomic_replaces_existing_truncated_file() {
        let dir = std::env::temp_dir().join(format!("scrollery_wa_rep_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("d4e5f6.webp");

        std::fs::write(&target, b"trunc").unwrap(); // 模拟半截旧缓存
        write_atomic(&target, b"full-new-content").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"full-new-content");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// snap_to_tier 回归:就近取整到 5 档 [64,128,256,512,1024]。
    #[test]
    fn snap_to_tier_picks_nearest() {
        assert_eq!(snap_to_tier(64), 64); // 恰在档位
        assert_eq!(snap_to_tier(100), 128); // |100-64|=36 > |100-128|=28
        assert_eq!(snap_to_tier(300), 256); // |300-256|=44 < |300-512|=212
        assert_eq!(snap_to_tier(400), 512); // |400-512|=112 < |400-256|=144
        assert_eq!(snap_to_tier(5000), 1024); // 超界取末档
    }

    #[test]
    fn encoded_result_keeps_production_snapshot() {
        let cache_dir = std::env::temp_dir().join(format!(
            "scrollery_encode_snapshot_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config = ThumbConfig {
            cache_dir: cache_dir.clone(),
            size: 64,
            skip_max_bytes: 0,
            strategy: String::new(),
            gpu_engine: String::new(),
            ai_hq_cache: false,
            webp_quality: 80,
            ai_cache_short_edge: 336,
        };
        let decoded = crate::engine::traits::DecodedImage {
            pixels: vec![255, 0, 0, 255],
            width: 1,
            height: 1,
            icc: None,
        };

        let result = encode_media_step_with_snapshot(7, 9, 123, decoded, &config).unwrap();

        assert_eq!(result.item_id, 7);
        assert_eq!(result.source_revision, 9);
        assert_eq!(result.cache_key, 123);
        assert_eq!(result.thumb_status, 1);
        let _ = std::fs::remove_dir_all(cache_dir);
    }
}
