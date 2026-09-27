// src-tauri/src/engine/native/wic_engine.rs
//! 用 OS 原生编解码器解码 + `IWICBitmapScaler` 缩放，**全程 CPU 软件路径**——WIC 静态图解码/缩放
//! 不走 GPU/NPP/CUDA。唯一可能沾硬件的是 HEIC/HEVC：解码交给系统 HEVC 解码器，装了硬件解码器的
//! 机器上该步或由 GPU 承担；JPEG/PNG/… 一律 CPU 软解。故 strategy="gpu" 命中本引擎时实为
//! 「OS 原生 CPU 解码」而非 GPU 图像解码。

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Seek;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::{
    GENERIC_READ, RPC_E_CHANGED_MODE, WINCODEC_ERR_UNSUPPORTEDOPERATION,
};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::AppError;
use crate::scanner::metadata::{read_jpeg_orientation, read_jpeg_orientation_file};

pub struct WicEngine;

struct ComApartment(bool);

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct ThreadFactory {
    // 字段按声明顺序释放，工厂须先于当前线程的 COM 初始化引用释放。
    factory: IWICImagingFactory,
    _apartment: ComApartment,
}

thread_local! {
    static WIC_FACTORY: RefCell<Option<ThreadFactory>> = const { RefCell::new(None) };
    static WORKER_SESSION: Cell<bool> = const { Cell::new(false) };
}

/// 释放当前图像线程不用的 GPU 会话，保留 WIC 工厂及 COM 生命周期。
/// CPU 策略切换后调用，避免旧设备缓存继续占用共享 Job 的内存配额。
pub fn release_image_gpu_sessions() {
    #[cfg(feature = "native-vpl")]
    crate::engine::gpu::vpl_jpeg::clear_thread_sessions();
    crate::engine::gpu::d2d_resize::clear_thread_sessions();
}

/// 在持久 worker 循环内复用图像资源，并在返回或 panic 展开时按顺序清理。
pub fn with_image_worker_session<T>(operation: impl FnOnce() -> T) -> T {
    assert!(
        !WORKER_SESSION.replace(true),
        "image worker session must not nest"
    );
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            release_image_gpu_sessions();
            // 先释放 GPU COM 对象，再释放 WIC/COM 引用，均发生在线程函数返回前。
            drop(WIC_FACTORY.take());
            WORKER_SESSION.set(false);
        }
    }
    let _cleanup = Cleanup;
    operation()
}

fn create_thread_factory() -> Result<ThreadFactory, AppError> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized != RPC_E_CHANGED_MODE {
        initialized
            .ok()
            .map_err(|e| AppError::os("COM 初始化失败 | COM initialization failed", e))?;
    }
    let apartment = ComApartment(initialized.is_ok());
    let factory = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| AppError::os("WIC 初始化失败 | WIC initialization failed", e))?;
    Ok(ThreadFactory {
        factory,
        _apartment: apartment,
    })
}

/// worker 范围内复用工厂；其它调用在本次操作结束前释放全部 COM 引用。
pub(crate) fn with_wic_factory<T>(
    operation: impl FnOnce(&IWICImagingFactory) -> Result<T, AppError>,
) -> Result<T, AppError> {
    if !WORKER_SESSION.get() {
        let local = create_thread_factory()?;
        return operation(&local.factory);
    }
    WIC_FACTORY.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(create_thread_factory()?);
        }
        operation(&slot.as_ref().expect("WIC factory initialized").factory)
    })
}

impl WicEngine {
    /// 从调用方已授权打开的文件句柄解码，WIC 不重新解析源路径。
    pub fn decode_open_file(
        file: File,
        format: &str,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError> {
        Self::decode_open_file_bounded(file, format, resize, 512 * 1024 * 1024)
    }

    /// 按调用方工作集上限检查输出 RGBA 大小；codec 内部缓存须由进程配额约束。
    pub fn decode_open_file_bounded(
        mut file: File,
        format: &str,
        resize: Option<ResizeHint>,
        max_output_bytes: u64,
    ) -> Result<DecodedImage, AppError> {
        let orientation = if matches!(
            format.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "heic" | "heif"
        ) {
            read_jpeg_orientation_file(&mut file)
        } else {
            1
        };
        file.rewind().map_err(AppError::Io)?;
        with_wic_factory(|factory| unsafe {
            let decoder = factory
                .CreateDecoderFromFileHandle(
                    file.as_raw_handle() as usize,
                    std::ptr::null(),
                    WICDecodeMetadataCacheOnDemand,
                )
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;
            decode_decoder(factory, &decoder, orientation, resize, max_output_bytes)
        })
    }
}

unsafe fn decode_decoder(
    factory: &IWICImagingFactory,
    decoder: &IWICBitmapDecoder,
    orientation: u32,
    resize: Option<ResizeHint>,
    max_output_bytes: u64,
) -> Result<DecodedImage, AppError> {
    // Get first frame
    let frame = decoder
        .GetFrame(0)
        .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;
    let icc = read_frame_icc(factory, &frame)?;

    // Get dimensions
    let mut width = 0;
    let mut height = 0;
    frame
        .GetSize(&mut width, &mut height)
        .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;
    if width == 0 || height == 0 {
        return Err(AppError::Internal("WIC image has empty dimensions".into()));
    }

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
                scaled_height = (height as f32 * target as f32 / width as f32).round() as u32;
            } else {
                scaled_height = target;
                scaled_width = (width as f32 * target as f32 / height as f32).round() as u32;
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
    // 极窄长图的缩小边至少一像素；拒绝零目标而非交给 codec 使用不确定行为。
    if matches!(
        resize,
        Some(ResizeHint::LongEdge(0) | ResizeHint::ShortEdge(0))
    ) {
        return Err(AppError::Internal("WIC resize target is zero".into()));
    }
    scaled_width = scaled_width.max(1);
    scaled_height = scaled_height.max(1);
    let (stride, buffer_size) = rgba_layout(scaled_width, scaled_height, max_output_bytes)?;

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
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(buffer_size)
        .map_err(|_| AppError::Internal("WIC pixel allocation failed".into()))?;
    pixels.resize(buffer_size, 0);

    converter
        .CopyPixels(std::ptr::null(), stride, &mut pixels)
        .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;

    // WIC 不保证应用容器方向；只在目标小图上转正。
    if orientation > 1 {
        let rgba = image::RgbaImage::from_raw(scaled_width, scaled_height, pixels)
            .ok_or_else(|| AppError::Internal("WIC RGBA buffer mismatch".into()))?;
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
        let (width, height) = (img.width(), img.height());
        let rgba = img.into_rgba8();
        return Ok(DecodedImage {
            pixels: rgba.into_raw(),
            width,
            height,
            icc,
        });
    }

    Ok(DecodedImage {
        pixels,
        width: scaled_width,
        height: scaled_height,
        icc,
    })
}

unsafe fn read_frame_icc(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<Option<Vec<u8>>, AppError> {
    let error = |e| AppError::os("WIC 色彩配置读取失败 | WIC color profile read failed", e);
    let mut count = 0;
    // 查询长度时传真正的空指针，避免 codec 将空 slice 的非空地址当作输出缓冲。
    let result =
        (frame.vtable().GetColorContexts)(frame.as_raw(), 0, std::ptr::null_mut(), &mut count);
    if result == WINCODEC_ERR_UNSUPPORTEDOPERATION {
        return Ok(None);
    }
    result.ok().map_err(error)?;
    // 不让源文件声明的 context 数或 ICC 长度触发无界分配。
    if count > 16 {
        return Err(AppError::Internal(
            "WIC color context count exceeds budget".into(),
        ));
    }
    if count == 0 {
        return Ok(None);
    }
    let mut contexts = (0..count)
        .map(|_| factory.CreateColorContext().map(Some))
        .collect::<windows::core::Result<Vec<_>>>()
        .map_err(error)?;
    frame
        .GetColorContexts(&mut contexts, &mut count)
        .map_err(error)?;
    if count as usize > contexts.len() {
        return Err(AppError::Internal("WIC color context count changed".into()));
    }
    for context in contexts.into_iter().take(count as usize).flatten() {
        if context.GetType().map_err(error)? != WICColorContextProfile {
            continue;
        }
        let mut size = 0;
        (context.vtable().GetProfileBytes)(context.as_raw(), 0, std::ptr::null_mut(), &mut size)
            .ok()
            .map_err(error)?;
        if size == 0 {
            continue;
        }
        if size > 16 * 1024 * 1024 {
            return Err(AppError::Internal("WIC ICC profile exceeds budget".into()));
        }
        let mut profile = vec![0; size as usize];
        context
            .GetProfileBytes(&mut profile, &mut size)
            .map_err(error)?;
        if size as usize > profile.len() {
            return Err(AppError::Internal("WIC ICC profile size changed".into()));
        }
        profile.truncate(size as usize);
        // 格式转换/缩放不做 CMS；沿用下游在小图上统一转到 sRGB 的出口。
        return Ok(Some(profile));
    }
    Ok(None)
}

fn rgba_layout(width: u32, height: u32, limit: u64) -> Result<(u32, usize), AppError> {
    let stride = width.checked_mul(4);
    let bytes = stride.and_then(|stride| stride.checked_mul(height));
    match (stride, bytes) {
        (Some(stride), Some(bytes)) if bytes > 0 && u64::from(bytes) <= limit => {
            Ok((stride, bytes as usize))
        }
        _ => Err(AppError::Internal("WIC pixel output exceeds budget".into())),
    }
}

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

    fn decode_bounded(
        &self,
        file_path: &Path,
        resize: Option<ResizeHint>,
        max_pixel_bytes: u64,
    ) -> Result<DecodedImage, AppError> {
        let normalized_path_str = file_path.to_string_lossy().replace('/', "\\");
        with_wic_factory(|factory| unsafe {
            let decoder = factory
                .CreateDecoderFromFilename(
                    &HSTRING::from(&normalized_path_str),
                    None,
                    GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )
                .map_err(|e| AppError::os("WIC 图像处理失败 | WIC operation failed", e))?;
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let orientation = if matches!(
                ext.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "heic" | "heif"
            ) {
                read_jpeg_orientation(file_path)
            } else {
                1
            };
            decode_decoder(factory, &decoder, orientation, resize, max_pixel_bytes)
        })
    }
}
