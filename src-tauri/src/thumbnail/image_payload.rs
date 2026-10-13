//! 宿主与隔离进程共用的纯像素编码；不依赖数据库、Tauri 或产物路径。

use super::thumbhash::generate_thumbhash;
use crate::error::{AppError, Result};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    /// 生效；封面派生（视频/文档/音频）恒 `ai_hq_cache
    /// = false`,该字段对它们无效果,构造处填编译期默认常量 `cache::AI_CACHE_SHORT_EDGE` 即可。
    pub ai_cache_short_edge: u32,
}

/// 两个编码产物的独占所有权；worker 只回传字节，输出路径由宿主决定。
pub(crate) struct EncodedImage {
    pub display: Vec<u8>,
    pub thumbhash: Option<Vec<u8>>,
    pub ai: Option<Vec<u8>>,
}

/// 显示图和缺失的 AI 缓存共享同一解码结果，不在编码线程执行磁盘 IO。
/// `thumbhash_input` 由 GPU 从最终方向、sRGB 的显示纹理生成，仅替代 Hash 输入。
pub(crate) fn encode_image_payload(
    mut decoded: crate::engine::traits::DecodedImage,
    thumbhash_input: Option<crate::engine::traits::DecodedImage>,
    config: &ThumbConfig,
    emit_display: bool,
    emit_ai: bool,
) -> Result<EncodedImage> {
    let ai = if emit_ai {
        let rgba = resize_short_edge_rgba(
            &decoded.pixels,
            decoded.width,
            decoded.height,
            config.ai_cache_short_edge,
        )?;
        let rgba = crate::editing::color::project_rgba8_to_srgb(rgba, decoded.icc.as_deref());
        Some(
            crate::thumbnail::exif_thumb::encode_as_webp(
                &rgba,
                crate::thumbnail::exif_thumb::DEFAULT_WEBP_QUALITY,
            )
            .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&rgba))?,
        )
    } else {
        None
    };
    let rgba = resize_to_rgba(
        &mut decoded.pixels,
        decoded.width,
        decoded.height,
        config.size,
    )?;
    let rgba = crate::editing::color::project_rgba8_to_srgb(rgba, decoded.icc.as_deref());
    let (width, height) = rgba.dimensions();
    let display = if emit_display {
        crate::thumbnail::exif_thumb::encode_as_webp(&rgba, config.webp_quality)
            .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&rgba))?
    } else {
        Vec::new()
    };
    // GPU 小图已从最终显示纹理生成；直接供 Hash 使用，避免在编码池再次缩小。
    let hash_image = thumbhash_input.unwrap_or_else(|| crate::engine::traits::DecodedImage {
        pixels: rgba.into_raw(),
        width,
        height,
        icc: None,
    });
    let thumbhash = generate_thumbhash(&hash_image).ok();
    Ok(EncodedImage {
        display,
        thumbhash,
        ai,
    })
}

fn resize_to_rgba(pixels: &mut Vec<u8>, w: u32, h: u32, target: u32) -> Result<image::RgbaImage> {
    if w <= target && h <= target {
        return image::RgbaImage::from_raw(w, h, std::mem::take(pixels))
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
