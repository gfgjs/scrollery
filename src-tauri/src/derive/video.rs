// src-tauri/src/derive/video.rs
//! Video derivations: poster cover (§3.2) and keyframe sprite (§3.3), driven by the
//! `VideoBackend` (Media Foundation on Windows). Each is a pure `run(ctx) -> Result<Output>`;
//! the pipeline handles scheduling / resume / yield / orphan recovery.
//!
//! 视频派生：封面帧（§3.2）与关键帧雪碧图（§3.3），由 `VideoBackend`（Windows 下为 Media
//! Foundation）驱动。每个都是纯函数 `run(ctx) -> Result<Output>`；流水线负责调度/续传/让步/孤儿恢复。

use crate::derive::kind::{DerivationContext, DerivationOutput};
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{ensure_sprite_dir, keyframe_sprite_db_path, keyframe_sprite_path};
use crate::thumbnail::generator::{encode_media_step_with_snapshot, snap_to_tier, ThumbConfig};
use crate::video::backend_for;

/// Fallback default when `video_keyframe_count` is absent from config.toml (schema.rs
/// `SETTING_DEFS` default is the authoritative source for the actually-configured value;
/// this constant only backs the `pipeline.rs` `.unwrap_or` parse fallback). **Not** the live
/// value any more (批次C) — since 小批C2 (2026-07-22) the frontend reads this key too
/// (`ui.videoKeyframeCount` via `get_startup_config`, see `useHoverPreview.ts`), so both sides
/// track the same config source; this constant only guards the parse-fallback path here.
/// 关键帧 scrub 帧数的回退默认值(config.toml 缺省时用;真实生效值以 `schema.rs::SETTING_DEFS`
/// 为准)。批次C起不再是恒定生效值——小批C2(2026-07-22)起前端也经 `get_startup_config` 下发
/// 读取本键(`ui.videoKeyframeCount`,见 `useHoverPreview.ts`),两侧同源,本常量只兜解析失败。
pub const KEYFRAME_COUNT: usize = 10;

/// Fallback default when `sprite_cell_height` is absent from config.toml (mirrors
/// `KEYFRAME_COUNT`'s role above). Was `video::media_foundation::SPRITE_CELL_H` pre-批次C.
/// `sprite_cell_height` 缺省时的回退默认值(角色同上方 `KEYFRAME_COUNT`)。批次C前是
/// `video::media_foundation::SPRITE_CELL_H` 常量。
pub const DEFAULT_SPRITE_CELL_H: u32 = 200;

/// Extract a poster frame and write it as a WebP cover (§3.2). The cover is written to the exact
/// thumbnail cache path/key, so the pipeline can mirror `thumb_status=1 / thumb_path` onto
/// `media_items` and `MediaThumb` shows it with zero frontend changes.
/// 抽取封面帧并写为 WebP 封面（§3.2）。封面写入精确的缩略图缓存路径/键，使流水线能把
/// `thumb_status=1 / thumb_path` 回填到 `media_items`，`MediaThumb` 零改动即可显示。
pub fn run_cover(ctx: &DerivationContext) -> Result<DerivationOutput> {
    let backend = backend_for(&ctx.file_format)
        .ok_or_else(|| AppError::UnsupportedFormat(ctx.file_format.clone()))?;

    // 时间戳选择（min(1s, 时长 10%)，黑帧规避）已内聚到后端的同一解码会话内 —— 此处不再
    // 先 probe(旧实现为选时间戳独立 probe,每视频多开一次 reader)。长边直接按缩略图 tier
    // 请求,MF 侧由 XVP 缩好,encode_media_step 的 resize 成为直通。
    let decoded = backend.cover(&ctx.abs_path, snap_to_tier(ctx.thumb_size))?;

    // 复用缩略图编码器：缩放 → WebP → 写入缩略图缓存（按 cache_key）→ thumbhash。
    let cfg = ThumbConfig {
        cache_dir: ctx.cache_dir.clone(),
        size: snap_to_tier(ctx.thumb_size),
        skip_max_bytes: 0,
        strategy: String::new(),
        gpu_engine: String::new(),
        ai_hq_cache: false, // 视频封面非 CLIP 分析对象，不产 AI 缓存
        webp_quality: ctx.webp_quality,
        ai_cache_short_edge: crate::thumbnail::cache::AI_CACHE_SHORT_EDGE, // 未用(ai_hq_cache=false)
    };
    let res = encode_media_step_with_snapshot(
        ctx.item_id,
        ctx.source_revision,
        ctx.cache_key,
        decoded,
        &cfg,
    )?;

    Ok(DerivationOutput {
        payload_path: res.thumb_path,
        thumbhash: res.thumbhash,
        page_count: None,
    })
}

/// 采样 N 帧并拼为一张水平关键帧雪碧图（§3.3），用于悬停/进度条 scrub。
pub fn run_keyframes(ctx: &DerivationContext) -> Result<DerivationOutput> {
    let backend = backend_for(&ctx.file_format)
        .ok_or_else(|| AppError::UnsupportedFormat(ctx.file_format.clone()))?;

    let frames = backend.keyframes(&ctx.abs_path, ctx.keyframe_count, ctx.sprite_cell_height)?;
    if frames.is_empty() {
        return Err(AppError::Internal("no keyframes | 无关键帧".into()));
    }

    // 所有帧共享一个格尺寸（统一比例）。从左到右拼成单条带。
    let cell_w = frames[0].width;
    let cell_h = frames[0].height;
    let cols = frames.len() as u32;
    let sprite_w = cell_w * cols;
    let sprite_h = cell_h;

    let mut sprite = image::RgbaImage::new(sprite_w, sprite_h);
    for (i, f) in frames.iter().enumerate() {
        // 跳过尺寸漂移的帧（防御性 —— 后端按统一尺寸缩放）。
        if f.width != cell_w || f.height != cell_h {
            continue;
        }
        let Some(frame_img) = image::RgbaImage::from_raw(f.width, f.height, f.pixels.clone())
        else {
            continue;
        };
        let x0 = i as u32 * cell_w;
        image::imageops::overlay(&mut sprite, &frame_img, x0 as i64, 0);
    }

    // 雪碧图是悬停 scrub 预览、非显示缩略图,质量与用户设置解耦恒用默认值
    // (10 帧横拼,无损/高质量下体积放大 10 倍不值)。
    let webp = crate::thumbnail::exif_thumb::encode_as_webp(
        &sprite,
        crate::thumbnail::exif_thumb::DEFAULT_WEBP_QUALITY,
    )
    .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&sprite))
    .map_err(|_| AppError::Internal("sprite WebP encode failed | 雪碧图编码失败".into()))?;

    ensure_sprite_dir(&ctx.cache_dir, ctx.cache_key).map_err(AppError::Io)?;
    let disk_path = keyframe_sprite_path(&ctx.cache_dir, ctx.cache_key);
    // 原子落盘(2026-07-06 审查 P1-3):与 run_cover/derive::image 对齐,消除重生成覆盖窗口内
    // 并发读到半截条带的可能。
    crate::thumbnail::generator::write_atomic(&disk_path, &webp).map_err(AppError::from)?;

    Ok(DerivationOutput {
        payload_path: Some(keyframe_sprite_db_path(ctx.cache_key)),
        thumbhash: None,
        page_count: None,
    })
}
