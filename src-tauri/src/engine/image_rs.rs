// src-tauri/src/engine/image_rs.rs
//! `ImageRsEngine`：使用 `image` crate 解码阶段 1 的格式。
//! 支持的格式：jpg, jpeg, png, webp, bmp, gif, tif, tiff

use image::{ImageDecoder, ImageReader};
use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::Path;

use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::AppError;
use crate::scanner::metadata::read_jpeg_orientation_file;

pub struct ImageRsEngine;

/// 在物化内嵌图片前按真实头信息限制源格式和 RGBA 字节，供共享工作集内的封面派生使用。
pub(crate) fn decode_image_bytes_bounded(
    bytes: &[u8],
    max_pixel_bytes: u64,
) -> Result<image::DynamicImage, AppError> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(AppError::Io)?;
    decode_image_reader_bounded(reader, max_pixel_bytes)
}

/// 已明确编码格式的产物也复用同一分配前额度检查。
pub(crate) fn decode_image_reader_bounded<R: std::io::BufRead + Seek>(
    reader: ImageReader<R>,
    max_pixel_bytes: u64,
) -> Result<image::DynamicImage, AppError> {
    let mut decoder = reader.into_decoder().map_err(AppError::Engine)?;
    let (width, height) = decoder.dimensions();
    let rgba_bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    if rgba_bytes > max_pixel_bytes {
        return Err(AppError::Internal(
            "embedded RGBA exceeds pixel limit".into(),
        ));
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(max_pixel_bytes);
    limits
        .reserve(decoder.total_bytes())
        .map_err(AppError::Engine)?;
    decoder.set_limits(limits).map_err(AppError::Engine)?;
    image::DynamicImage::from_decoder(decoder).map_err(AppError::Engine)
}

impl ImageRsEngine {
    /// 从已打开 JPEG 提取嵌入图，供受控句柄的子进程快速路径复用。
    pub fn extract_embedded_thumb_file(file: &mut File) -> Result<Option<Vec<u8>>, AppError> {
        file.rewind().map_err(AppError::Io)?;
        let mut reader = std::io::BufReader::new(file);
        let exif = exif::Reader::new().read_from_container(&mut reader).ok();
        let Some(exif) = exif else { return Ok(None) };
        if let Some(field) = exif.get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL) {
            if let exif::Value::Long(ref offsets) = field.value {
                if let Some(&offset) = offsets.first() {
                    if let Some(len_field) =
                        exif.get_field(exif::Tag::JPEGInterchangeFormatLength, exif::In::THUMBNAIL)
                    {
                        if let exif::Value::Long(ref lengths) = len_field.value {
                            if let Some(&length) = lengths.first() {
                                if length > 0 && length < 1_000_000 {
                                    // EXIF 偏移以 TIFF 头为基准；必须在解析后的 TIFF 缓冲切片。
                                    let tiff = exif.buf();
                                    let start = offset as usize;
                                    if let Some(end) = start.checked_add(length as usize) {
                                        if end <= tiff.len() {
                                            let thumb = &tiff[start..end];
                                            if thumb.starts_with(&[0xFF, 0xD8]) {
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

    /// 从已经打开且由调用方授权的文件句柄解码；不会按路径重新打开源。
    pub fn decode_open_file(
        file: File,
        format: &str,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError> {
        Self::decode_open_file_bounded(file, format, resize, 512 * 1024 * 1024)
    }

    /// 在解码像素前检查调用方提供的字节上限；保留 image crate 的内部解码限制。
    pub fn decode_open_file_bounded(
        mut file: File,
        format: &str,
        resize: Option<ResizeHint>,
        max_decoded_bytes: u64,
    ) -> Result<DecodedImage, AppError> {
        let orientation =
            if format.eq_ignore_ascii_case("jpg") || format.eq_ignore_ascii_case("jpeg") {
                read_jpeg_orientation_file(&mut file)
            } else {
                1
            };
        file.rewind().map_err(AppError::Io)?;
        // 便捷路径 `ImageReader::decode()` 会丢弃 ICC(`DynamicImage` 不携带该信息)。
        // 改走 `into_decoder()` 先取 `icc_profile()` 再 `DynamicImage::from_decoder()`,
        // 与 `editing/metadata.rs::read_source_metadata` 同款先例,行为对齐。
        let reader = ImageReader::new(BufReader::new(file))
            .with_guessed_format()
            .map_err(AppError::Io)?;
        let mut decoder = reader.into_decoder().map_err(AppError::Engine)?;
        // 恢复 ImageReader::decode 内建的 max_alloc 守卫,超大图落稳定错误而非裸分配 abort
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(max_decoded_bytes);
        limits
            .reserve(image::ImageDecoder::total_bytes(&decoder))
            .map_err(AppError::Engine)?;
        decoder.set_limits(limits).map_err(AppError::Engine)?;
        let icc = image::ImageDecoder::icc_profile(&mut decoder)
            .ok()
            .flatten();
        let img = image::DynamicImage::from_decoder(decoder).map_err(AppError::Engine)?;

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

        // 灰度/RGB 的解码字节数小于 RGBA；转换前按实际小图尺寸再次检查，不能只限源格式。
        let rgba_bytes = u64::from(img.width())
            .checked_mul(u64::from(img.height()))
            .and_then(|pixels| pixels.checked_mul(4))
            .unwrap_or(u64::MAX);
        if rgba_bytes > max_decoded_bytes {
            return Err(AppError::Internal(
                "decoded RGBA exceeds pixel limit".into(),
            ));
        }
        // 长/短边目标不依赖方向；先缩小再转正，避免为旋转大图再分配一份全尺寸像素。
        let img = apply_exif_orientation(img, orientation);
        let rgba = img.into_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        Ok(DecodedImage {
            pixels: rgba.into_raw(),
            width,
            height,
            icc,
        })
    }
}

impl ImageEngine for ImageRsEngine {
    fn name(&self) -> &str {
        "image-rs"
    }

    fn supported_formats(&self) -> &[&str] {
        &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"]
    }

    fn decode_bounded(
        &self,
        file_path: &Path,
        resize: Option<ResizeHint>,
        max_pixel_bytes: u64,
    ) -> Result<DecodedImage, AppError> {
        let file = File::open(file_path).map_err(AppError::Io)?;
        let format = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        Self::decode_open_file_bounded(file, format, resize, max_pixel_bytes)
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

        let mut file = std::fs::File::open(file_path).map_err(AppError::from)?;
        Self::extract_embedded_thumb_file(&mut file)
    }
}

/// 将 EXIF 方向校正应用于解码后的图像。
pub(crate) fn apply_exif_orientation(
    img: image::DynamicImage,
    orientation: u32,
) -> image::DynamicImage {
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
