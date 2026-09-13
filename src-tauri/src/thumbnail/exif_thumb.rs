use std::path::Path;

use crate::engine::traits::{DecodedImage, ImageEngine};
use crate::error::{AppError, Result};

use crate::scanner::metadata::read_jpeg_orientation;
use crate::thumbnail::thumbhash::generate_thumbhash;

/// 尝试 EXIF 快速路径。返回编码后的 WebP 字节和可选的 ThumbHash，如果需要回退则返回 `None`。
pub fn try_exif_thumb(
    engine: &dyn ImageEngine,
    path: &Path,
    target_size: u32,
    webp_quality: u8,
) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let embedded = engine.extract_embedded_thumb(path).ok()??;

    // 解码内嵌的 JPEG
    let mut img = image::load_from_memory(&embedded).ok()?;

    // The embedded EXIF thumbnail usually shares the physical orientation of the main image.
    // We must apply the EXIF orientation rotation before saving it as WebP,
    // because WebP won't carry the EXIF metadata to the browser.
    // 内嵌的 EXIF 缩略图通常与主图共享物理方向。
    // 在将其保存为 WebP 之前，我们必须应用 EXIF 方向旋转，
    // 因为 WebP 不会将 EXIF 元数据带到浏览器。
    let orientation = read_jpeg_orientation(path);
    img = match orientation {
        1 => img,
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    };

    let (w, h) = (img.width(), img.height());

    // 质量守门（Part3 Q4 / §3.1.2）：内嵌图不足目标档位时是否采用，按档位分级——
    // 大档位（480/960）严格拒绝不足档位的内嵌图、回退全解码（避免 160→960 的数倍上采样劣化）；
    // 小档位（120/240）维持宽通道（标准 160×120/256×160 内嵌图通常已够，选片速度优先、容轻度放大）。
    if !embedded_thumb_acceptable(w.max(h), target_size) {
        return None;
    }

    // 调整大小并编码
    let (new_w, new_h) = if w >= h {
        let ratio = target_size as f32 / w as f32;
        (target_size, (h as f32 * ratio).round() as u32)
    } else {
        let ratio = target_size as f32 / h as f32;
        ((w as f32 * ratio).round() as u32, target_size)
    };

    let resized = img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
    let rgba = resized.to_rgba8();

    let decoded_for_hash = DecodedImage {
        pixels: rgba.clone().into_raw(),
        width: new_w,
        height: new_h,
        icc: None, // 仅用于 ThumbHash 计算,ICC 与此无关(D-416:EXIF 快速路径不做 CMS)
    };
    let hash = generate_thumbhash(&decoded_for_hash).ok();

    let webp_bytes = encode_as_webp(&rgba, webp_quality).ok()?;

    Some((webp_bytes, hash))
}

/// 内嵌 EXIF 缩略图是否够格直接采用（否则 `try_exif_thumb` 返回 None → 回退全解码）。
///
/// 按目标档位分级（Part3 Q4 / §3.1.2）：
/// - **大档位（512/1024）**：严格——`max_edge` 不足档位即拒，避免把 160×120 上采样到 1024（数倍劣化）。
/// - **小档位（64/128/256）**：宽松——`max_edge >= 128` 即可（标准内嵌图通常已够，选片速度优先，容轻度放大）。
///
/// `target_size` 经 `snap_to_tier` 归一到 [64,128,256,512,1024] 再判级（幂等：已是档位则不变）。
fn embedded_thumb_acceptable(max_edge: u32, target_size: u32) -> bool {
    let tier = crate::thumbnail::generator::snap_to_tier(target_size);
    // 小档位取 128px 下限（≤2× 上采样容忍，随最大小档位 256 而定）；大档位要求内嵌图至少达档位（不放大）。
    let min_edge = if tier >= 512 { tier } else { 128 };
    max_edge >= min_edge
}

/// 缩略图 WebP 编码质量默认值。q80 是照片类缩略图的通行甜点:480px 档位下肉眼几乎无差、
/// 体积约为无损 WebP 的 1/5~1/10。派生产物非档案(源文件才是),默认不追求无损。
/// 显示缩略图的实际质量由用户设置 `thumb_webp_quality` 驱动(ThumbConfig::webp_quality);
/// 非显示产物(AI/人脸分析缓存、关键帧雪碧图)恒用本默认值,不随显示设置起落。
pub const DEFAULT_WEBP_QUALITY: u8 = 80;

/// Encode RGBA pixel data as WebP via libwebp. `quality` 1..=99 → lossy at that quality;
/// **100 → lossless (VP8L)** — both paths are the same `webp` crate (libwebp supports
/// lossless natively and compresses better than the `image` crate's pure-Rust encoder).
/// Falls back to the `image` crate's lossless encoder on libwebp failure (e.g. dimensions
/// beyond libwebp's 16383px limit, which the keyframe sprite could theoretically hit);
/// callers additionally fall back to JPEG.
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

#[cfg(test)]
mod tests {
    use super::embedded_thumb_acceptable;

    /// 渐变纹理模拟照片内容(纯色图上有损/无损差异不显著)。
    fn gradient_img() -> image::RgbaImage {
        image::RgbaImage::from_fn(256, 160, |x, y| {
            image::Rgba([
                (x % 256) as u8,
                ((y * 13) % 256) as u8,
                ((x + y) % 256) as u8,
                255,
            ])
        })
    }

    /// 质量 <100 必须是有损 WebP(RIFF 容器内 VP8/VP8X 而非 VP8L 无损 chunk),
    /// 且产物可被 image crate 解码(前端 Webview 同理可显)。
    /// 回归钉:image crate 0.25 的 WebP 编码器仅支持无损,若误退回该路径,
    /// 照片类缩略图体积膨胀 5~10 倍。
    #[test]
    fn encode_as_webp_lossy_below_100_and_decodable() {
        let bytes = super::encode_as_webp(&gradient_img(), super::DEFAULT_WEBP_QUALITY).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        assert_ne!(&bytes[12..16], b"VP8L", "质量 <100 不得落到无损 VP8L");

        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 160));
    }

    /// 用户设置契约:质量 100 = 无损(VP8L chunk),且往返解码逐像素一致。
    /// 超界值(>100)钳到 100 同为无损。
    #[test]
    fn encode_as_webp_quality_100_is_lossless() {
        let img = gradient_img();
        let bytes = super::encode_as_webp(&img, 100).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[12..16], b"VP8L", "质量 100 必须走无损 VP8L");

        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), img.as_raw(), "无损须逐像素还原");

        let clamped = super::encode_as_webp(&img, 255).unwrap();
        assert_eq!(&clamped[12..16], b"VP8L", ">100 钳到 100 同为无损");
    }

    /// 大档位（512/1024）严格：标准内嵌图（160×120/256×160，max_edge≤256）不足档位 → 拒绝回退全解码。
    #[test]
    fn large_tier_rejects_undersized_embedded() {
        // tier=512：256<512 拒；恰达/超档位采用。
        assert!(!embedded_thumb_acceptable(256, 512));
        assert!(embedded_thumb_acceptable(512, 512));
        assert!(embedded_thumb_acceptable(600, 512));
        // tier=1024：800<1024 拒；≥1024 采用。
        assert!(!embedded_thumb_acceptable(800, 1024));
        assert!(embedded_thumb_acceptable(1024, 1024));
        assert!(embedded_thumb_acceptable(1200, 1024));
    }

    /// 小档位（64/128/256）宽松：max_edge≥128 即采用（容轻度放大），仅 <128 才回退。
    #[test]
    fn small_tier_lenient_above_128_floor() {
        // tier=64：160 采用；127 拒（128px 下限）。
        assert!(embedded_thumb_acceptable(160, 64));
        assert!(!embedded_thumb_acceptable(127, 64));
        // tier=256：160<256 仍采用（宽通道、≤2× 放大）；127 拒。
        assert!(embedded_thumb_acceptable(160, 256));
        assert!(!embedded_thumb_acceptable(127, 256));
    }

    /// 非档位 target_size 经 snap_to_tier 归一后判级（500→512 严格；300→256 宽松）。
    #[test]
    fn non_tier_target_snaps_before_grading() {
        assert!(!embedded_thumb_acceptable(256, 500)); // snap→512 严格，256<512 拒
        assert!(embedded_thumb_acceptable(160, 300)); // snap→256 宽松，160≥128 采用
    }
}
