// src-tauri/src/engine/gpu/wic_engine.rs
//! 用 OS 原生编解码器解码 + `IWICBitmapScaler` 缩放，**全程 CPU 软件路径**——WIC 静态图解码/缩放
//! 不走 GPU/NPP/CUDA。唯一可能沾硬件的是 HEIC/HEVC：解码交给系统 HEVC 解码器，装了硬件解码器的
//! 机器上该步或由 GPU 承担；JPEG/PNG/… 一律 CPU 软解。故 strategy="gpu" 命中本引擎时实为
//! 「OS 原生 CPU 解码」而非 GPU 图像解码（真 GPU 引擎 nvjpeg/dxva 尚未实现，见 engine/gpu/mod.rs）。

use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::AppError;
use crate::scanner::metadata::read_jpeg_orientation;

pub struct WicEngine;

impl ImageEngine for WicEngine {
    fn name(&self) -> &str {
        "wic"
    }

    fn supported_formats(&self) -> &[&str] {
        &[
            "jpg", "jpeg", "png", "bmp", "tif", "tiff", "heic", "heif", "avif", "webp", "gif",
            "ico",
        ]
    }

    fn decode(
        &self,
        file_path: &Path,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError> {
        // Normalize path for Windows WIC API (it dislikes forward slashes in some cases)
        let normalized_path_str = file_path.to_string_lossy().replace('/', "\\");

        unsafe {
            // Initialize COM. It might already be initialized by Tauri or another thread, so we ignore errors.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let factory: IWICImagingFactory = windows::Win32::System::Com::CoCreateInstance(
                &CLSID_WICImagingFactory,
                None,
                windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
            )
            .map_err(|e| AppError::os("WIC 初始化失败 | WIC initialization failed", e))?;

            // Create Decoder
            let decoder = factory
                .CreateDecoderFromFilename(
                    &HSTRING::from(&normalized_path_str),
                    None,
                    GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

            // Get first frame
            let frame = decoder
                .GetFrame(0)
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

            // Get dimensions
            let mut width = 0;
            let mut height = 0;
            frame
                .GetSize(&mut width, &mut height)
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

            // Convert to 32bppRGBA
            let converter = factory
                .CreateFormatConverter()
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

            // 根据 ResizeHint 计算目标尺寸
            let (mut scaled_width, mut scaled_height) = (width, height);
            let needs_resize = match resize {
                Some(ResizeHint::LongEdge(target)) if width > target || height > target => {
                    if width >= height {
                        scaled_width = target;
                        scaled_height =
                            (height as f32 * target as f32 / width as f32).round() as u32;
                    } else {
                        scaled_height = target;
                        scaled_width =
                            (width as f32 * target as f32 / height as f32).round() as u32;
                    }
                    true
                }
                Some(ResizeHint::ShortEdge(target)) => {
                    // 只下采样(2026-07-06 审查 R2):ShortEdge 唯一消费方是 AI/face 缓存
                    // (derive::image::decode_short_edge),契约「分析只下采样…绝不上采样」——
                    // 小图放大只会浪费磁盘并给 CLIP/YuNet 喂插值像素。short<target 时原尺寸解码。
                    let short = width.min(height);
                    if short > target {
                        let scale = target as f32 / short as f32;
                        scaled_width = (width as f32 * scale).round() as u32;
                        scaled_height = (height as f32 * scale).round() as u32;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };

            let source: IWICBitmapSource = if needs_resize {
                let scaler = factory
                    .CreateBitmapScaler()
                    .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;
                scaler
                    .Initialize(
                        &frame,
                        scaled_width,
                        scaled_height,
                        WICBitmapInterpolationModeCubic,
                    )
                    .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

                scaler
                    .cast()
                    .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?
            } else {
                frame
                    .cast()
                    .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?
            };

            converter
                .Initialize(
                    &source,
                    &GUID_WICPixelFormat32bppRGBA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

            // Copy pixels
            let stride = scaled_width * 4;
            let buffer_size = stride * scaled_height;
            let mut pixels = vec![0u8; buffer_size as usize];

            converter
                .CopyPixels(std::ptr::null(), stride, &mut pixels)
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

            // Apply EXIF orientation if needed (since WIC sometimes doesn't automatically apply it based on codec)
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if matches!(ext.as_str(), "jpg" | "jpeg" | "heic" | "heif") {
                let orientation = read_jpeg_orientation(file_path);
                if orientation > 1 {
                    // Fallback to image crate for orientation rotation logic, or we could use fast_image_resize/wgpu in the future.
                    // For now, doing it via `image` crate RgbaImage to keep it simple.
                    if let Some(rgba) =
                        image::RgbaImage::from_raw(scaled_width, scaled_height, pixels.clone())
                    {
                        let img = image::DynamicImage::ImageRgba8(rgba);
                        let img = match orientation {
                            2 => img.fliph(),
                            3 => img.rotate180(),
                            4 => img.flipv(),
                            5 => img.rotate90().fliph(),
                            6 => img.rotate90(),
                            7 => img.rotate270().fliph(),
                            8 => img.rotate270(),
                            _ => img,
                        };
                        let rgba = img.to_rgba8();
                        return Ok(DecodedImage {
                            pixels: rgba.into_raw(),
                            width: img.width(),
                            height: img.height(),
                            icc: None, // D-417:WIC v1 不出 ICC,heic/avif 色偏留 A2 后续
                        });
                    }
                }
            }

            Ok(DecodedImage {
                pixels,
                width: scaled_width,
                height: scaled_height,
                icc: None, // D-417:WIC v1 不出 ICC,heic/avif 色偏留 A2 后续
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R0-2 端到端解码实证(env 门控,同 exotic 真实进程测试的既有模式):
    /// `PICASA_HEIC_SAMPLE=<path.heic> cargo test wic_heic` —— 在装有 HEIF 扩展的
    /// Windows 上用真实样张走完整 WIC 解码链,断言出非零尺寸 RGBA 像素。
    /// CI/无样张环境自动跳过(不设 env 即 return),不制造环境敏感红灯。
    #[test]
    fn wic_heic_decodes_real_sample_when_env_set() {
        let Ok(sample) = std::env::var("PICASA_HEIC_SAMPLE") else {
            eprintln!("PICASA_HEIC_SAMPLE not set — skipping real HEIC decode test");
            return;
        };
        let decoded = WicEngine
            .decode(Path::new(&sample), Some(ResizeHint::LongEdge(480)))
            .expect("WIC must decode the HEIC sample (HEIF extension installed?)");
        assert!(decoded.width > 0 && decoded.height > 0);
        assert!(
            decoded.width.max(decoded.height) <= 480,
            "long edge must be capped at 480"
        );
        assert_eq!(
            decoded.pixels.len(),
            (decoded.width * decoded.height * 4) as usize,
            "RGBA buffer must match dimensions"
        );
    }
}
