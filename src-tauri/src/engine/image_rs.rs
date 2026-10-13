//! `ImageRsEngine`：使用 `image` crate 解码常见静态图格式。
//! 支持的格式：jpg, jpeg, png, webp, bmp, gif, tif, tiff

use image::{ImageDecoder, ImageReader};
use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::Path;

use crate::engine::orientation::read_jpeg_orientation_file;
use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::AppError;

pub struct ImageRsEngine;

/// 从实际 decoder 取得的尺寸和位深；不包含像素分配，也不信任数据库尺寸。
// Windows 的预算预估由独立 worker 使用；宿主复用本文件的其他解码入口。
#[cfg_attr(windows, allow(dead_code))]
pub(crate) struct ImageWorkingSet {
    pub width: u32,
    pub height: u32,
    source_bytes: u64,
    bytes_per_pixel: u64,
}

#[cfg_attr(windows, allow(dead_code))]
impl ImageWorkingSet {
    /// 包含源格式、缩放浮点中间图和最坏方向变换的像素峰值。
    pub(crate) fn bytes_for_resize(&self, resize: Option<ResizeHint>) -> Result<u64, AppError> {
        let target = resize_dimensions(self.width, self.height, resize)?;
        Ok(pixel_peak(
            self.width,
            self.height,
            target,
            self.bytes_per_pixel,
            self.source_bytes,
            6,
        ))
    }
}

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
        let mut file = File::open(file_path).map_err(AppError::Io)?;
        let format = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        // 普通 trait 调用保留 ImageReader 原有 codec 额度，不额外限制缩放/方向的整段峰值。
        Self::decode_open_file_inner(
            &mut file,
            format,
            resize,
            u64::MAX,
            image::Limits::default().max_alloc,
        )
    }
}

impl ImageRsEngine {
    /// 探测真实 decoder 元数据。调用方须先预留压缩输入及元数据额度；JPEG 构造器
    /// 会读入压缩流，`max_probe_bytes` 不能替代调用方的输入文件字节账本。
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn working_set_bytes(
        file: &mut File,
        max_probe_bytes: u64,
    ) -> Result<ImageWorkingSet, AppError> {
        file.rewind().map_err(AppError::Io)?;
        let mut reader = ImageReader::new(BufReader::new(file))
            .with_guessed_format()
            .map_err(AppError::Io)?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(max_probe_bytes);
        reader.limits(limits);
        let decoder = reader.into_decoder().map_err(AppError::Engine)?;
        let (width, height) = decoder.dimensions();
        resize_dimensions(width, height, None)?;
        Ok(ImageWorkingSet {
            width,
            height,
            source_bytes: decoder.total_bytes(),
            bytes_per_pixel: u64::from(decoder.color_type().bytes_per_pixel()),
        })
    }

    /// 从已授权句柄解码，不重新按路径打开；入口自行 rewind，失败后可复用同一句柄。
    /// `max_decoded_bytes` 限制真实源位深、缩放中间缓冲及输出的已知像素峰值。
    /// codec 私有分配仍需执行域资源限制；本额度不能由数据库尺寸直接推导。
    pub fn decode_open_file_bounded(
        file: &mut File,
        format: &str,
        resize: Option<ResizeHint>,
        max_decoded_bytes: u64,
    ) -> Result<DecodedImage, AppError> {
        Self::decode_open_file_inner(
            file,
            format,
            resize,
            max_decoded_bytes,
            Some(max_decoded_bytes),
        )
    }

    fn decode_open_file_inner(
        file: &mut File,
        format: &str,
        resize: Option<ResizeHint>,
        max_peak: u64,
        codec_limit: Option<u64>,
    ) -> Result<DecodedImage, AppError> {
        let orientation = if matches!(
            format.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "tif" | "tiff" | "webp" | "heic" | "heif" | "avif"
        ) {
            read_jpeg_orientation_file(file)
        } else {
            1
        };
        file.rewind().map_err(AppError::Io)?;
        let mut reader = ImageReader::new(BufReader::new(file))
            .with_guessed_format()
            .map_err(AppError::Io)?;
        let mut limits = image::Limits::default();
        limits.max_alloc = codec_limit;
        // PNG 在构建 decoder 时读取元数据，必须在 into_decoder 前传入额度。
        reader.limits(limits.clone());
        let mut decoder = reader.into_decoder().map_err(AppError::Engine)?;
        let (width, height) = decoder.dimensions();
        let target = resize_dimensions(width, height, resize)?;
        let source_bytes = decoder.total_bytes();
        let bytes_per_pixel = u64::from(decoder.color_type().bytes_per_pixel());
        let peak = pixel_peak(
            width,
            height,
            target,
            bytes_per_pixel,
            source_bytes,
            orientation,
        );
        if peak > max_peak {
            return Err(pixel_limit());
        }
        // reserve 恢复 ImageReader::decode 的源缓冲扣额；限制继续交给 codec 自己执行。
        limits.reserve(source_bytes).map_err(AppError::Engine)?;
        decoder.set_limits(limits).map_err(AppError::Engine)?;
        let icc = decoder.icc_profile().map_err(AppError::Engine)?;
        if icc
            .as_ref()
            .is_some_and(|profile| profile.len() > 16 * 1024 * 1024)
        {
            return Err(pixel_limit());
        }
        let img = image::DynamicImage::from_decoder(decoder).map_err(AppError::Engine)?;
        // 长/短边与方向无关：先缩小，再旋转，避免完整原图旋转副本。
        let img = if target != (width, height) {
            img.resize_exact(target.0, target.1, image::imageops::FilterType::CatmullRom)
        } else {
            img
        };
        let rgba = apply_exif_orientation(img, orientation).into_rgba8();
        Ok(DecodedImage {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
            icc,
        })
    }
}

/// 根据真实源尺寸计算只下采样的目标；整数比例避免超长边的浮点舍入溢出。
pub(crate) fn resize_dimensions(
    width: u32,
    height: u32,
    resize: Option<ResizeHint>,
) -> Result<(u32, u32), AppError> {
    if width == 0 || height == 0 {
        return Err(pixel_limit());
    }
    let (target, edge) = match resize {
        Some(ResizeHint::LongEdge(target)) => (target, width.max(height)),
        Some(ResizeHint::ShortEdge(target)) => (target, width.min(height)),
        None => return Ok((width, height)),
    };
    if target == 0 {
        return Err(pixel_limit());
    }
    if edge <= target {
        return Ok((width, height));
    }
    let scale = |value: u32| {
        ((u64::from(value) * u64::from(target) + u64::from(edge) / 2) / u64::from(edge)).max(1)
            as u32
    };
    Ok((scale(width), scale(height)))
}

// CatmullRom 先做纵向 RGBA32F 中间图（源宽 × 目标高），再写目标格式。
// 以实际位深计算各阶段同时存活的缓冲，不能仅用目标 RGBA 或 DB 宽高作准入。
fn pixel_peak(
    width: u32,
    height: u32,
    target: (u32, u32),
    bytes_per_pixel: u64,
    source_bytes: u64,
    orientation: u32,
) -> u64 {
    let target_pixels = u64::from(target.0) * u64::from(target.1);
    let native_output = target_pixels.saturating_mul(bytes_per_pixel);
    let rgba_output = target_pixels.saturating_mul(4);
    // 解码器可能保留一份源格式缓冲，同时向 DynamicImage 写入另一份。
    let mut peak = source_bytes.saturating_mul(2);
    if target != (width, height) {
        let scratch = u64::from(width)
            .saturating_mul(u64::from(target.1))
            .saturating_mul(16);
        // 采样权重 Vec 按源长边保守计入容量增长；不把 f32 中间图误计成 u8。
        let weights = u64::from(width.max(height)).saturating_mul(8);
        peak = peak.max(
            source_bytes
                .saturating_add(scratch)
                .saturating_add(native_output)
                .saturating_add(weights),
        );
    }
    if (5..=8).contains(&orientation) {
        peak = peak.max(native_output.saturating_mul(2));
    }
    peak.max(native_output.saturating_add(rgba_output))
}

pub(crate) fn pixel_limit() -> AppError {
    AppError::Engine(image::ImageError::Limits(
        image::error::LimitError::from_kind(image::error::LimitErrorKind::InsufficientMemory),
    ))
}

/// 将 EXIF 方向应用于小图；镜像/180° 原地操作，90°/270° 最多保留两份小图。
pub(crate) fn apply_exif_orientation(
    mut img: image::DynamicImage,
    orientation: u32,
) -> image::DynamicImage {
    if let Some(orientation) = image::metadata::Orientation::from_exif(orientation as u8) {
        img.apply_orientation(orientation);
    }
    img
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn rejects_real_source_bit_depth_peak_before_small_output_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("16bit.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            64,
            32,
            image::Rgba([30000, 500, 600, 65535]),
        )
        .save(&path)
        .unwrap();
        let mut file = File::open(&path).unwrap();
        assert!(ImageRsEngine::decode_open_file_bounded(
            &mut file,
            "png",
            Some(ResizeHint::LongEdge(2)),
            64 * 32 * 8
        )
        .is_err());
        let decoded = ImageRsEngine::decode_open_file_bounded(
            &mut file,
            "png",
            Some(ResizeHint::LongEdge(2)),
            256 * 1024,
        )
        .unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
    }
}
