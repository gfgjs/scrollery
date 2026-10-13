// src-tauri/src/derive/video.rs
//! 视频封面帧与关键帧雪碧图派生，由 `VideoBackend` 驱动；Windows 使用 Media Foundation。
//! 流水线负责调度、续传和取消，派生函数负责读取与写入产物。

use crate::derive::kind::{DerivationContext, DerivationOutput};
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{ensure_sprite_dir, keyframe_sprite_db_path, keyframe_sprite_path};
use crate::thumbnail::generator::{encode_media_step_with_snapshot, snap_to_tier, ThumbConfig};
use crate::video::backend_for;

/// 关键帧数量的解析回退值；配置默认值由 `schema.rs::SETTING_DEFS` 定义。
pub const KEYFRAME_COUNT: usize = 10;

/// `sprite_cell_height` 的解析回退值。
pub const DEFAULT_SPRITE_CELL_H: u32 = 200;

/// 抽取封面帧并写入缩略图缓存路径，供流水线回填 `media_items` 的封面状态。
pub fn run_cover(ctx: &DerivationContext) -> Result<DerivationOutput> {
    let backend = backend_for(&ctx.file_format)
        .ok_or_else(|| AppError::UnsupportedFormat(ctx.file_format.clone()))?;

    // 时间戳选择（min(1s, 时长 10%)，黑帧规避）已内聚到后端的同一解码会话内 —— 此处不再
    // 先 probe(旧实现为选时间戳独立 probe,每视频多开一次 reader)。长边直接按缩略图 tier
    // 请求,MF 侧由 XVP 缩好,encode_media_step_with_snapshot 的 resize 成为直通。
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
