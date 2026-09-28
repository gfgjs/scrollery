// src-tauri/src/thumbnail/thumbhash.rs
//! 从解码后的像素生成 ThumbHash。
//!
//! 输入：RGBA 像素缓冲区（任何尺寸）。
//! 输出：约 28 字节的 ThumbHash，作为 BLOB 存储在数据库中。
//! 前端接收其为 `number[]` → `Uint8Array` → 渲染为 32×32 占位符。

use std::borrow::Cow;

use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};

/// 在散列之前缩放图像的最大尺寸（ThumbHash 在 100×100 或更小的尺寸下效果很好）。
const HASH_MAX_DIM: u32 = 100;

/// 为解码后的图像生成 ThumbHash。
pub fn generate_thumbhash(decoded: &DecodedImage) -> Result<Vec<u8>> {
    generate_thumbhash_rgba(&decoded.pixels, decoded.width, decoded.height)
}

/// 从已有 RGBA 缓冲生成 ThumbHash；小图直接借用输入，大图只分配降采样结果。
pub fn generate_thumbhash_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    // 如果需要则缩小
    let (pixels, width, height) = if width > HASH_MAX_DIM || height > HASH_MAX_DIM {
        let ratio = (HASH_MAX_DIM as f32) / (width.max(height) as f32);
        let new_w = (((width as f32) * ratio).round() as u32).max(1);
        let new_h = (((height as f32) * ratio).round() as u32).max(1);

        // 使用 fast_image_resize v4 进行降采样（缩小）
        use fast_image_resize::pixels::PixelType;
        use fast_image_resize::{
            images::{Image as FirImage, ImageRef},
            ResizeOptions, Resizer,
        };

        let src = ImageRef::new(width.max(1), height.max(1), rgba, PixelType::U8x4)
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let mut dst = FirImage::new(new_w, new_h, PixelType::U8x4);

        let mut resizer = Resizer::new();
        // JPEG等不透明输入无需乘除alpha，也无需整幅中间缓冲；保留原Lanczos3滤镜。
        let has_transparency = rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 255);
        let options = ResizeOptions::new().use_alpha(has_transparency);
        resizer
            .resize(&src, &mut dst, &options)
            .map_err(|e| AppError::Internal(e.to_string()))?;

        (Cow::Owned(dst.into_vec()), new_w, new_h)
    } else {
        (Cow::Borrowed(rgba), width, height)
    };

    let hash = thumbhash::rgba_to_thumb_hash(width as usize, height as usize, &pixels);
    Ok(hash)
}

/// 由 thumbhash 字节算占位平均色,返回 CSS `#rrggbb`(不足/畸形 → None)。
///
/// 前端 `thumbhashToAverageColor` 的后端对偶:改由布局 hydrate 时算一次(仅可视区 ~10² 项),
/// 前端直接用——**移出渲染热路径**(原 canvas draw 每帧对每个可视格现算均色 + 每项过桥 ~28 字节
/// thumbhash 数组作 JSON,快滚段落地时是白屏与反序列化爆发的一处源)。用 thumbhash crate 的
/// `thumb_hash_to_average_rgba`(golden 生成器同一函数),与前端 TS 移植版在 ±2/通道内一致
/// (前端 thumbhash.spec.ts 已锁此容差),差异不可辨。
pub fn average_color_hex(hash: &[u8]) -> Option<String> {
    let (r, g, b, _a) = thumbhash::thumb_hash_to_average_rgba(hash).ok()?;
    let to255 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!("#{:02x}{:02x}{:02x}", to255(r), to255(g), to255(b)))
}
