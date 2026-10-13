use crate::error::{AppError, Result};

/// 缩略图 WebP 编码质量默认值。q80 是照片类缩略图的通行甜点:480px 档位下肉眼几乎无差、
/// 体积约为无损 WebP 的 1/5~1/10。派生产物非档案(源文件才是),默认不追求无损。
/// 显示缩略图的实际质量由用户设置 `thumb_webp_quality` 驱动(ThumbConfig::webp_quality);
/// 非显示产物(AI/人脸分析缓存、关键帧雪碧图)恒用本默认值,不随显示设置起落。
pub const DEFAULT_WEBP_QUALITY: u8 = 80;

/// 经 libwebp 将 RGBA 像素编码为 WebP。`quality` 1..=99 → 按该质量有损;**100 → 无损(VP8L)**
/// ——两条路径同用 `webp` crate(libwebp 原生支持无损,压缩率优于 image crate 纯 Rust 实现)。
/// libwebp 失败时回退 `image` crate 无损编码(如超出 16383px 边长上限——雪碧图理论可触);
/// 调用方另有 JPEG 兜底。
///
/// 为何不用 `image` crate 直接编码:其 0.25 起 WebP 编码器仅支持无损,照片内容无损体积
/// 约为有损 q80 的 5~10 倍,直接放大磁盘占用与缓存 LRU 驱逐频率(→404 自愈 churn)。
pub fn encode_as_webp(rgba: &image::RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let q = quality.clamp(1, 100);
    let (w, h) = (rgba.width(), rgba.height());
    let encoder = webp::Encoder::from_rgba(rgba.as_raw(), w, h);
    // 质量 100 = 无损语义(用户设置项契约);无损模式下 quality 参数控制的是压缩耗时/率
    // 的权衡(0-100,越高越小越慢),75 取平衡。
    let result = if q >= 100 {
        encoder.encode_simple(true, 75.0)
    } else {
        encoder.encode_simple(false, q as f32)
    };
    if let Ok(mem) = result {
        return Ok(mem.to_vec());
    }
    // 回退:无损(image crate)。极端输入下仍能出图,仅体积劣化。
    let mut buf = Vec::new();
    rgba.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::WebP,
    )
    .map_err(|e| AppError::Internal(format!("WebP encode failed: {e}")))?;
    Ok(buf)
}

/// 作为 JPEG 回退进行编码（质量 85）。
pub fn encode_as_jpeg(rgba: &image::RgbaImage) -> Result<Vec<u8>> {
    let rgb = image::DynamicImage::ImageRgba8(rgba.clone()).to_rgb8();
    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
    encoder
        .encode_image(&image::DynamicImage::ImageRgb8(rgb))
        .map_err(|e| AppError::Internal(format!("JPEG encode failed: {e}")))?;
    Ok(buf)
}
