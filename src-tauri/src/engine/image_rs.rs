// src-tauri/src/engine/image_rs.rs
//! `ImageRsEngine`：使用 `image` crate 解码阶段 1 的格式。
//! 支持的格式：jpg, jpeg, png, webp, bmp, gif, tif, tiff

use image::{ImageDecoder, ImageReader};
use std::path::Path;

use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::AppError;
use crate::scanner::metadata::read_jpeg_orientation;

pub struct ImageRsEngine;

impl ImageEngine for ImageRsEngine {
    fn name(&self) -> &str {
        "image-rs"
    }

    fn supported_formats(&self) -> &[&str] {
        &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"]
    }

    fn decode(
        &self,
        file_path: &Path,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError> {
        // 便捷路径 `ImageReader::decode()` 会丢弃 ICC(`DynamicImage` 不携带该信息)。
        // 改走 `into_decoder()` 先取 `icc_profile()` 再 `DynamicImage::from_decoder()`,
        // 与 `editing/metadata.rs::read_source_metadata` 同款先例,行为对齐。
        let reader = ImageReader::open(file_path)
            .map_err(AppError::Io)?
            .with_guessed_format()
            .map_err(AppError::Io)?;
        let mut decoder = reader.into_decoder().map_err(AppError::Engine)?;
        // 恢复 ImageReader::decode 内建的 max_alloc 守卫,超大图落稳定错误而非裸分配 abort
        let mut limits = image::Limits::default();
        limits
            .reserve(image::ImageDecoder::total_bytes(&decoder))
            .map_err(AppError::Engine)?;
        decoder.set_limits(limits).map_err(AppError::Engine)?;
        let icc = image::ImageDecoder::icc_profile(&mut decoder)
            .ok()
            .flatten();
        let img = image::DynamicImage::from_decoder(decoder).map_err(AppError::Engine)?;

        // 为 JPEG 应用 EXIF 方向
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let img = if matches!(ext.as_str(), "jpg" | "jpeg") {
            apply_exif_orientation(img, file_path)
        } else {
            img
        };

        // 如果有缩放请求则进行缩放
        let img = if let Some(hint) = resize {
            let (w, h) = (img.width(), img.height());
            match hint {
                ResizeHint::LongEdge(target) => {
                    if w > target || h > target {
                        let (nw, nh) = if w >= h {
                            (target, (h as f32 * target as f32 / w as f32).round() as u32)
                        } else {
                            ((w as f32 * target as f32 / h as f32).round() as u32, target)
                        };
                        img.resize_exact(
                            nw.max(1),
                            nh.max(1),
                            image::imageops::FilterType::CatmullRom,
                        )
                    } else {
                        img
                    }
                }
                ResizeHint::ShortEdge(target) => {
                    // 只下采样(2026-07-06 审查 R2):与 wic_engine 同规,契约见 ai_cache
                    // 「分析只下采样…绝不上采样」;short<=target 时原尺寸解码。
                    let short = w.min(h);
                    if short > target {
                        let scale = target as f32 / short as f32;
                        let nw = (w as f32 * scale).round() as u32;
                        let nh = (h as f32 * scale).round() as u32;
                        img.resize_exact(
                            nw.max(1),
                            nh.max(1),
                            image::imageops::FilterType::CatmullRom,
                        )
                    } else {
                        img
                    }
                }
            }
        } else {
            img
        };

        let rgba = img.to_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        Ok(DecodedImage {
            pixels: rgba.into_raw(),
            width,
            height,
            icc,
        })
    }

    /// 尝试从 EXIF 中提取嵌入的 JPEG 缩略图（快速路径）。
    fn extract_embedded_thumb(&self, file_path: &Path) -> Result<Option<Vec<u8>>, AppError> {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if !matches!(ext.as_str(), "jpg" | "jpeg") {
            return Ok(None);
        }

        let file = std::fs::File::open(file_path).map_err(AppError::from)?;
        let mut reader = std::io::BufReader::new(file);
        let exif = exif::Reader::new().read_from_container(&mut reader).ok();

        let Some(exif) = exif else { return Ok(None) };

        // 查找 IFD1（缩略图）JPEG 数据
        if let Some(field) = exif.get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL) {
            if let exif::Value::Long(ref offsets) = field.value {
                if let Some(&offset) = offsets.first() {
                    // 获取缩略图长度
                    if let Some(len_field) =
                        exif.get_field(exif::Tag::JPEGInterchangeFormatLength, exif::In::THUMBNAIL)
                    {
                        if let exif::Value::Long(ref lengths) = len_field.value {
                            if let Some(&length) = lengths.first() {
                                if length > 0 && length < 1_000_000 {
                                    // 偏移基准修正(2026-07-06 审查 P1-2):EXIF 规范中
                                    // JPEGInterchangeFormat 是**相对 TIFF 头起点**的偏移,
                                    // kamadak-exif 原样返回。旧实现按文件绝对偏移 seek,
                                    // 通常提前约 12 字节(SOI+APP1 头+"Exif\0\0"),FFD8 魔数
                                    // 必失败 → 快路径恒 miss 静默回退全解码。正确基准是
                                    // 已解析的 EXIF 缓冲(exif.buf() 即 TIFF 数据)内切片。
                                    let tiff = exif.buf();
                                    let start = offset as usize;
                                    if let Some(end) = start.checked_add(length as usize) {
                                        if end <= tiff.len() {
                                            let thumb = &tiff[start..end];
                                            // Validate JPEG magic | 验证 JPEG 魔数
                                            if thumb.starts_with(&[0xFF, 0xD8]) {
                                                tracing::debug!(
                                                    "[ExifThumb] IFD1 hit: {} bytes from {}",
                                                    thumb.len(),
                                                    file_path.display()
                                                );
                                                return Ok(Some(thumb.to_vec()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

/// 将 EXIF 方向校正应用于解码后的图像。
fn apply_exif_orientation(img: image::DynamicImage, path: &Path) -> image::DynamicImage {
    let orientation = read_jpeg_orientation(path);
    match orientation {
        1 => img,
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::traits::ImageEngine;

    const GOLDEN_ICC: &[u8] = b"golden-icc-profile-opaque-bytes-not-parsed";

    /// 合成一张带 ICC profile 的最小 PNG(手法照抄 `editing/metadata.rs` 的 golden 测试)。
    fn synthetic_png_with_icc() -> Vec<u8> {
        use image::codecs::png::PngEncoder;
        use image::{ImageEncoder, RgbImage};
        let img = RgbImage::from_fn(2, 2, |x, y| image::Rgb([x as u8 * 60, y as u8 * 60, 128]));
        let mut out = Vec::new();
        let mut enc = PngEncoder::new(&mut out);
        enc.set_icc_profile(GOLDEN_ICC.to_vec()).unwrap();
        enc.write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        out
    }

    /// 回归钉(D-410 地基):`ImageRsEngine::decode()` 此前走 `ImageReader::decode()` 便捷路径
    /// 丢弃 ICC;重构为 `into_decoder()` 路径后,`DecodedImage.icc` 必须等于源文件嵌入的字节。
    #[test]
    fn decode_preserves_embedded_icc_profile() {
        let dir = std::env::temp_dir().join(format!("scrollery_icc_decode_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("icc.png");
        std::fs::write(&path, synthetic_png_with_icc()).unwrap();

        let engine = ImageRsEngine;
        let decoded = engine.decode(&path, None).expect("decode must succeed");
        assert_eq!(decoded.icc.as_deref(), Some(GOLDEN_ICC));
        assert_eq!((decoded.width, decoded.height), (2, 2));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 无 ICC 的源文件解码后 `icc` 必须为 `None`(而非空 Vec 或 panic)。
    #[test]
    fn decode_without_icc_yields_none() {
        use image::{ImageBuffer, Rgb, RgbImage};
        let img: RgbImage = ImageBuffer::from_pixel(2, 2, Rgb([10, 20, 30]));
        let dir = std::env::temp_dir().join(format!("scrollery_icc_none_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plain.png");
        img.save(&path).unwrap();

        let engine = ImageRsEngine;
        let decoded = engine.decode(&path, None).expect("decode must succeed");
        assert_eq!(decoded.icc, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 手工构造最小 EXIF JPEG:SOI + APP1(Exif\0\0 + TIFF) + EOI。
    /// TIFF(LE) 布局(偏移相对 TIFF 头):
    ///   0  II 2A00 + IFD0 偏移=8
    ///   8  IFD0: 1 条(Orientation=1),next-IFD 指针 → 26
    ///   26 IFD1: 0x0201(缩略图偏移=56) + 0x0202(长度=4),next=0
    ///   56 缩略图字节 FFD8FFD9
    /// 文件中 TIFF 头前有 12 字节(SOI2+APP1标记长4+"Exif\0\0"6)——旧实现按文件绝对偏移
    /// seek(56) 会读到错误字节,新实现从 exif.buf()[56..60] 切片命中(2026-07-06 审查 P1-2)。
    fn synthetic_exif_jpeg() -> Vec<u8> {
        let mut tiff: Vec<u8> = Vec::new();
        // TIFF header
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        // IFD0 at 8
        tiff.extend_from_slice(&8u32.to_le_bytes());
        // IFD0: 1 entry (Orientation SHORT=1), next IFD -> 26
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&[1, 0, 0, 0]); // value=1 (前 2 字节)
        tiff.extend_from_slice(&26u32.to_le_bytes()); // next IFD = IFD1
        assert_eq!(tiff.len(), 26);
        // IFD1: 2 entries
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&0x0201u16.to_le_bytes()); // JPEGInterchangeFormat
        tiff.extend_from_slice(&4u16.to_le_bytes()); // LONG
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&56u32.to_le_bytes()); // thumb at TIFF+56
        tiff.extend_from_slice(&0x0202u16.to_le_bytes()); // JPEGInterchangeFormatLength
        tiff.extend_from_slice(&4u16.to_le_bytes()); // LONG
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&4u32.to_le_bytes()); // 4 bytes
        tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none
        assert_eq!(tiff.len(), 56);
        // thumbnail bytes (minimal JPEG magic + EOI)
        tiff.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);

        let mut jpeg: Vec<u8> = Vec::new();
        jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
        let payload_len = 2 + 6 + tiff.len(); // len 字段自含
        jpeg.extend_from_slice(&[0xFF, 0xE1]); // APP1
        jpeg.extend_from_slice(&(payload_len as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI
        jpeg
    }

    #[test]
    fn exif_thumb_offset_is_tiff_relative_not_file_absolute() {
        let dir = std::env::temp_dir().join(format!("scrollery_exifthumb_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ifd1.jpg");
        std::fs::write(&path, synthetic_exif_jpeg()).unwrap();

        let engine = ImageRsEngine;
        let thumb = engine
            .extract_embedded_thumb(&path)
            .expect("no io error")
            .expect("IFD1 thumbnail must hit — offset base is TIFF header, not file start");
        assert_eq!(thumb, vec![0xFF, 0xD8, 0xFF, 0xD9]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exif_thumb_out_of_bounds_offset_returns_none() {
        // 越界偏移(损坏 EXIF)不得 panic,应安全返回 None 走全解码回退。
        let mut bytes = synthetic_exif_jpeg();
        // 把 0x0201 的 value(TIFF+56 处声明的偏移)改成越界值 0xFFFF:
        // 该 4 字节位于 TIFF 内偏移 36..40,文件内 12+36=48。
        bytes[48..52].copy_from_slice(&0xFFFFu32.to_le_bytes());
        let dir = std::env::temp_dir().join(format!("scrollery_exifoob_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("oob.jpg");
        std::fs::write(&path, bytes).unwrap();

        let engine = ImageRsEngine;
        assert!(engine.extract_embedded_thumb(&path).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
