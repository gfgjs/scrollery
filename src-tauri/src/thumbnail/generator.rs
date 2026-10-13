//! 图片编码与占位图；普通图片由执行服务调度，非图片派生复用编码写入入口。

use std::path::Path;
use tracing::warn;

use crate::db::models::ThumbResult;

use crate::error::{AppError, Result};
use crate::thumbnail::cache::thumb_db_path;

use super::benchmark::{stage_started, Stage};
pub(crate) use super::benchmark::{ItemMetrics, MeasuredOutcome};

/// 有效缩略图尺寸档位，是全后端唯一事实源(cache.rs 断言 / file_ops 重定位 /
/// scan 清理 / snap_to_tier 均引用此常量,避免多处硬编码档位漂移致 thumb_path 断言失败)。
/// 必须与前端 `src/constants/defaults.ts` 的 `THUMB_SIZE_TIERS` 保持一致。
/// 改档位会使既有缓存失效并需重产；当前 5 档为经验值。
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
/// (见派生缓存命中路径),若直写最终路径,崩溃/断电会把半截文件留在
/// 正式路径上 → 之后每次都命中「缓存」→ UI 永久裂图。先写同目录 tmp 再 rename,保证正式
/// 路径上只可能出现完整文件。tmp 名 = pid+纳秒+进程内序号(与 exotic/sink.rs
/// `unique_tmp_path` 同范式):序号保证进程内并发批次各写各的 tmp、rename 后到者胜;
/// pid+纳秒防跨进程碰撞——应用无 single-instance 守卫,双实例可共开同库(SQLite WAL 容许),
/// 两进程序号各自从 0 起,同 cache_key 时会交错写同名 tmp、把半截文件 rename 上位,
/// 而命中判定只查 exists(),裂图会存活到下轮自愈。rename 失败即清 tmp;进程中途崩溃遗留的
/// 孤儿 tmp 不影响正确性(命中判定不认 .tmp),随缓存清理/GC(Part3 缓存治理)一并回收。
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

pub use super::image_payload::ThumbConfig;

/// 带生产时源快照的编码入口，供缩略图 worker 将结果交给条件写 API。
pub fn encode_media_step_with_snapshot(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
) -> Result<ThumbResult> {
    encode_media_step_with_snapshot_guarded(
        item_id,
        source_revision,
        cache_key,
        decoded,
        config,
        None,
    )
}

/// encode_media_step_with_snapshot 的带守护版本:沿用同一 panic_guard 标签与算法,
/// 供普通入口与基准入口共用，仅是否携带 metrics 不同。
fn encode_media_step_with_snapshot_guarded(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
    metrics: Option<&mut ItemMetrics>,
) -> Result<ThumbResult> {
    panic_guard("encode_media_step_with_snapshot", move || {
        encode_media_step_inner(
            item_id,
            source_revision,
            cache_key,
            decoded,
            config,
            metrics,
        )
    })
}

pub(crate) use super::image_payload::EncodedImage;

/// 保留宿主原有的编码度量，实际像素处理与 worker 使用同一份实现。
pub(crate) fn encode_image_payload(
    decoded: crate::engine::traits::DecodedImage,
    thumbhash_input: Option<crate::engine::traits::DecodedImage>,
    config: &ThumbConfig,
    emit_display: bool,
    emit_ai: bool,
    mut metrics: Option<&mut ItemMetrics>,
) -> Result<EncodedImage> {
    let _timer = stage_started(&mut metrics, Stage::Encode);
    super::image_payload::encode_image_payload(
        decoded,
        thumbhash_input,
        config,
        emit_display,
        emit_ai,
    )
}

fn encode_media_step_inner(
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    decoded: crate::engine::traits::DecodedImage,
    config: &ThumbConfig,
    mut metrics: Option<&mut ItemMetrics>,
) -> Result<ThumbResult> {
    let ai_relative = super::cache::ai_cache_db_path(cache_key);
    let emit_ai = config.ai_hq_cache && !config.cache_dir.join(&ai_relative).is_file();
    let encoded =
        encode_image_payload(decoded, None, config, true, emit_ai, metrics.as_deref_mut())?;
    let db_path = thumb_db_path(config.size, cache_key);
    let timer = stage_started(&mut metrics, Stage::Write);
    write_cached_artifact(
        &config.cache_dir,
        &format!("thumbnails/{db_path}"),
        &encoded.display,
    )?;
    if let Some(ai) = &encoded.ai {
        write_cached_artifact(&config.cache_dir, &ai_relative, ai)?;
    }
    drop(timer);
    if let Some(metrics) = metrics {
        metrics.outcome = Some(MeasuredOutcome::Generated);
        metrics.ai_written += u64::from(encoded.ai.is_some());
    }
    Ok(ThumbResult {
        item_id,
        thumb_status: 1,
        thumb_path: Some(db_path),
        thumbhash: encoded.thumbhash,
        source_revision,
        cache_key,
    })
}

pub(crate) use super::artifact_io::write_cached_artifact;
