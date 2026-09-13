// src-tauri/src/derive/audio.rs
//! Audio derivations (P3, §3.6). 音频派生（P3，§3.6）。
//!
//! `audio_cover`：用 `lofty`（纯 Rust，符合轻量）提取内嵌封面 → 复用缩略图编码器写入缩略图缓存，
//! 流水线再把 `thumb_status/thumb_path/thumbhash` 回填到 `media_items`，`MediaThumb` 零改动即可显示
//! （与视频封面同路径）。无内嵌封面则返回错误 → 派生状态置 3（不重试），网格回落音符占位。
//!
//! `audio_meta`（标签/歌词）不走派生流水线：它由 enricher（补全阶段）回填 `audio_meta`
//! （与视频元数据同处理），歌词文本由 `get_audio_detail` 懒加载。故 `AudioMeta` 的 `is_implemented`
//! 保持 false（不入队），`run_meta` 仅为框架占位。

use crate::derive::kind::{not_implemented, DerivationContext, DerivationKind, DerivationOutput};
use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};
use crate::thumbnail::generator::{encode_media_step_with_snapshot, snap_to_tier, ThumbConfig};

/// 提取内嵌专辑封面并写为封面缩略图（P3，§3.6）。
pub fn run_cover(ctx: &DerivationContext) -> Result<DerivationOutput> {
    let (bytes, _ext) = crate::audio::read_cover(&ctx.abs_path)?
        .ok_or_else(|| AppError::AudioMetadata("no embedded cover art | 无内嵌封面".into()))?;

    // Decode the embedded picture (jpeg/png/…) → RGBA, then reuse the thumbnail encoder:
    // resize → WebP → write to thumb cache (by cache_key) → thumbhash. Identical to video cover.
    // 解码内嵌图片（jpeg/png/…）→ RGBA，再复用缩略图编码器：缩放 → WebP → 按 cache_key 写缓存 → thumbhash。
    // 与视频封面完全一致。
    let dynimg = image::load_from_memory(&bytes)
        .map_err(|e| AppError::AudioMetadata(format!("cover decode failed | 封面解码失败: {e}")))?;
    let rgba = dynimg.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let decoded = DecodedImage {
        pixels: rgba.into_raw(),
        width: w,
        height: h,
        icc: None, // 内嵌专辑封面暂不解析 ICC,按 sRGB 假定
    };

    let cfg = ThumbConfig {
        cache_dir: ctx.cache_dir.clone(),
        size: snap_to_tier(ctx.thumb_size),
        skip_max_bytes: 0,
        strategy: String::new(),
        gpu_engine: String::new(),
        ai_hq_cache: false, // 音频封面非 CLIP 分析对象，不产 AI 缓存
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

/// Audio tags/lyrics are handled by the enricher (§3.6), not the derivation pipeline — see the
/// module docs. This stub is never reached because `AudioMeta::is_implemented` is false.
/// 音频标签/歌词由 enricher 处理（§3.6），不走派生流水线 —— 见模块文档。由于
/// `AudioMeta::is_implemented` 为 false，本桩永不被触达。
pub fn run_meta(_ctx: &DerivationContext) -> Result<DerivationOutput> {
    Err(not_implemented(DerivationKind::AudioMeta))
}
