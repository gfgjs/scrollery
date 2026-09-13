//! E0 编辑预览：orientation 烤入 → ICC 转 sRGB → 长边限制 → 编码 → raw IPC packet。

use fast_image_resize::pixels::PixelType;
use fast_image_resize::{images::Image as FirImage, FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, RgbaImage};

use crate::editing::color;
use crate::error::AppError;

pub const PREVIEW_LONG_EDGE: u32 = 2048;
const PACKET_MAGIC: &[u8; 4] = b"SEP2";
const PACKET_VERSION: u8 = 1;
const FORMAT_JPEG: u8 = 1;
const FORMAT_PNG: u8 = 2;
const HEADER_LEN: usize = 28;

pub struct EditPreview {
    pub source_width: u32,
    pub source_height: u32,
    pub preview_width: u32,
    pub preview_height: u32,
    pub mime_type: &'static str,
    pub encoded: Vec<u8>,
}

impl EditPreview {
    /// raw IPC 包头为固定 28 字节小端结构，避免二进制主体经 JSON 数组膨胀：
    /// magic[4], version u8, format u8, reserved u16, source/preview w/h u32, body_len u32。
    pub fn into_packet(self) -> Vec<u8> {
        let format = if self.mime_type == "image/png" {
            FORMAT_PNG
        } else {
            FORMAT_JPEG
        };
        let body_len = u32::try_from(self.encoded.len()).unwrap_or(u32::MAX);
        let mut packet = Vec::with_capacity(HEADER_LEN + self.encoded.len());
        packet.extend_from_slice(PACKET_MAGIC);
        packet.push(PACKET_VERSION);
        packet.push(format);
        packet.extend_from_slice(&0u16.to_le_bytes());
        for value in [
            self.source_width,
            self.source_height,
            self.preview_width,
            self.preview_height,
            body_len,
        ] {
            packet.extend_from_slice(&value.to_le_bytes());
        }
        packet.extend_from_slice(&self.encoded);
        packet
    }
}

pub fn render_edit_preview(
    mut image: DynamicImage,
    orientation: image::metadata::Orientation,
    icc_profile: Option<&[u8]>,
) -> Result<EditPreview, AppError> {
    let has_alpha = image.color().has_alpha();
    image.apply_orientation(orientation);
    let (source_width, source_height) = (image.width(), image.height());
    let rgba = color::to_srgb_rgba8(&image, icc_profile)?;
    let resized = resize_rgba(&rgba, PREVIEW_LONG_EDGE)?;
    let (preview_width, preview_height) = resized.dimensions();

    let (mime_type, encoded) = if has_alpha {
        let mut encoded = Vec::new();
        DynamicImage::ImageRgba8(resized)
            .write_with_encoder(PngEncoder::new(&mut encoded))
            .map_err(|_| encode_error())?;
        ("image/png", encoded)
    } else {
        let rgb = DynamicImage::ImageRgba8(resized).to_rgb8();
        let mut encoded = Vec::new();
        DynamicImage::ImageRgb8(rgb)
            .write_with_encoder(JpegEncoder::new_with_quality(&mut encoded, 90))
            .map_err(|_| encode_error())?;
        ("image/jpeg", encoded)
    };

    Ok(EditPreview {
        source_width,
        source_height,
        preview_width,
        preview_height,
        mime_type,
        encoded,
    })
}

fn resize_rgba(source: &RgbaImage, long_edge: u32) -> Result<RgbaImage, AppError> {
    let (width, height) = source.dimensions();
    if width <= long_edge && height <= long_edge {
        return Ok(source.clone());
    }
    let (new_width, new_height) = capped_dimensions(width, height, long_edge);
    let mut pixels = source.as_raw().clone();
    let source = FirImage::from_slice_u8(width.max(1), height.max(1), &mut pixels, PixelType::U8x4)
        .map_err(|_| resize_error())?;
    let mut target = FirImage::new(new_width, new_height, PixelType::U8x4);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
    Resizer::new()
        .resize(&source, &mut target, &options)
        .map_err(|_| resize_error())?;
    RgbaImage::from_raw(new_width, new_height, target.into_vec()).ok_or_else(resize_error)
}

pub fn capped_dimensions(width: u32, height: u32, long_edge: u32) -> (u32, u32) {
    if width <= long_edge && height <= long_edge {
        return (width.max(1), height.max(1));
    }
    if width >= height {
        let scaled =
            (u64::from(height) * u64::from(long_edge) + u64::from(width) / 2) / u64::from(width);
        (long_edge, u32::try_from(scaled).unwrap_or(1).max(1))
    } else {
        let scaled =
            (u64::from(width) * u64::from(long_edge) + u64::from(height) / 2) / u64::from(height);
        (u32::try_from(scaled).unwrap_or(1).max(1), long_edge)
    }
}

fn resize_error() -> AppError {
    AppError::Edit {
        code: "edit_decode_failed",
        message: "编辑预览缩放失败 | edit preview resize failed".into(),
    }
}

fn encode_error() -> AppError {
    AppError::Edit {
        code: "edit_encode_failed",
        message: "编辑预览编码失败 | edit preview encode failed".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{metadata::Orientation, Rgba};

    #[test]
    fn cap_preserves_aspect_and_never_exceeds_long_edge() {
        assert_eq!(capped_dimensions(4000, 3000, 2048), (2048, 1536));
        assert_eq!(capped_dimensions(3000, 4000, 2048), (1536, 2048));
        assert_eq!(capped_dimensions(1000, 500, 2048), (1000, 500));
        assert_eq!(capped_dimensions(10_000, 1, 2048), (2048, 1));
    }

    #[test]
    fn orientation_is_baked_before_source_and_preview_dimensions() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(3000, 4000, Rgba([1, 2, 3, 4])));
        let preview = render_edit_preview(image, Orientation::Rotate90, None).expect("preview");
        assert_eq!((preview.source_width, preview.source_height), (4000, 3000));
        assert_eq!(
            (preview.preview_width, preview.preview_height),
            (2048, 1536)
        );
        assert_eq!(preview.mime_type, "image/png");
    }

    #[test]
    fn packet_header_describes_body_exactly() {
        let preview = EditPreview {
            source_width: 4000,
            source_height: 3000,
            preview_width: 2048,
            preview_height: 1536,
            mime_type: "image/jpeg",
            encoded: vec![1, 2, 3],
        };
        let packet = preview.into_packet();
        assert_eq!(&packet[..4], b"SEP2");
        assert_eq!(packet[4], 1);
        assert_eq!(packet[5], FORMAT_JPEG);
        assert_eq!(packet.len(), HEADER_LEN + 3);
        assert_eq!(&packet[HEADER_LEN..], &[1, 2, 3]);
    }
}
