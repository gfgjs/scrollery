//! 用 OS 原生编解码器解码 + `IWICBitmapScaler` 缩放，**全程 CPU 软件路径**——WIC 静态图解码/缩放
//! 不走 GPU/NPP/CUDA。唯一可能沾硬件的是 HEIC/HEVC：解码交给系统 HEVC 解码器，装了硬件解码器的
//! 机器上该步或由 GPU 承担；JPEG/PNG/… 一律 CPU 软解。故 strategy="gpu" 命中本引擎时实为
//! 「OS 原生 CPU 解码」而非 GPU 图像解码。

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Seek;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::core::Interface;
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, WINCODEC_ERR_UNSUPPORTEDOPERATION};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::engine::image_rs::{apply_exif_orientation, pixel_limit, resize_dimensions};
use crate::engine::orientation::read_jpeg_orientation_file;
use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::AppError;

pub struct WicEngine;

/// 保留 codec 错误类别供隔离进程选择回退；系统故障和内存不足不得当成格式失败。
pub(crate) fn wic_error(context: &str, error: windows::core::Error) -> AppError {
    use windows::Win32::Foundation::{
        E_OUTOFMEMORY, WINCODEC_ERR_BADHEADER, WINCODEC_ERR_BADIMAGE,
        WINCODEC_ERR_BADMETADATAHEADER, WINCODEC_ERR_BADSTREAMDATA, WINCODEC_ERR_COMPONENTNOTFOUND,
        WINCODEC_ERR_FRAMEMISSING, WINCODEC_ERR_IMAGESIZEOUTOFRANGE, WINCODEC_ERR_TOOMUCHMETADATA,
        WINCODEC_ERR_UNKNOWNIMAGEFORMAT, WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT,
        WINCODEC_ERR_UNSUPPORTEDVERSION,
    };
    match error.code() {
        E_OUTOFMEMORY | WINCODEC_ERR_IMAGESIZEOUTOFRANGE | WINCODEC_ERR_TOOMUCHMETADATA => {
            pixel_limit()
        }
        WINCODEC_ERR_COMPONENTNOTFOUND
        | WINCODEC_ERR_UNKNOWNIMAGEFORMAT
        | WINCODEC_ERR_UNSUPPORTEDVERSION
        | WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT
        | WINCODEC_ERR_UNSUPPORTEDOPERATION => AppError::UnsupportedFormat(context.into()),
        WINCODEC_ERR_BADHEADER
        | WINCODEC_ERR_BADIMAGE
        | WINCODEC_ERR_BADMETADATAHEADER
        | WINCODEC_ERR_BADSTREAMDATA
        | WINCODEC_ERR_FRAMEMISSING => AppError::Engine(image::ImageError::Decoding(
            image::error::DecodingError::new(image::error::ImageFormatHint::Unknown, error),
        )),
        _ => AppError::os(context, error),
    }
}

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

/// 在持久 worker 循环内复用图像资源，并在返回或 panic 展开时按顺序清理。
pub fn with_image_worker_session<T>(operation: impl FnOnce() -> T) -> T {
    assert!(
        !WORKER_SESSION.replace(true),
        "image worker session must not nest"
    );
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            // 在线程函数返回前释放工厂，再平衡本线程的 COM 初始化引用。
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
        file: &mut File,
        format: &str,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError> {
        Self::decode_open_file_bounded(file, format, resize, 512 * 1024 * 1024)
    }

    /// 按调用方工作集上限检查输出 RGBA 大小；codec 内部缓存须由进程配额约束。
    pub fn decode_open_file_bounded(
        file: &mut File,
        format: &str,
        resize: Option<ResizeHint>,
        max_output_bytes: u64,
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
        with_wic_factory(|factory| unsafe {
            let decoder = factory
                .CreateDecoderFromFileHandle(
                    file.as_raw_handle() as usize,
                    std::ptr::null(),
                    WICDecodeMetadataCacheOnDemand,
                )
                .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;
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
    let frame = decoder
        .GetFrame(0)
        .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;
    let icc = read_frame_icc(factory, &frame)?;

    decode_frame(factory, &frame, icc, orientation, resize, max_output_bytes)
}

/// 已打开 frame 的共用解码核心；隔离 runtime 先按实际 ICC/目标像素取得许可再进入。
pub(crate) unsafe fn decode_frame(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
    icc: Option<Vec<u8>>,
    orientation: u32,
    resize: Option<ResizeHint>,
    max_output_bytes: u64,
) -> Result<DecodedImage, AppError> {
    let mut width = 0;
    let mut height = 0;
    frame
        .GetSize(&mut width, &mut height)
        .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;
    if width == 0 || height == 0 {
        return Err(AppError::Internal("WIC image has empty dimensions".into()));
    }

    let converter = factory
        .CreateFormatConverter()
        .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;

    // 尺寸来自实际 frame；只在 CopyPixels 前缩小，不构造原图 RGBA 缓冲。
    let (scaled_width, scaled_height) = resize_dimensions(width, height, resize)?;
    let needs_resize = (scaled_width, scaled_height) != (width, height);
    let (stride, buffer_size) = rgba_layout(scaled_width, scaled_height, max_output_bytes)?;

    let source: IWICBitmapSource = if needs_resize {
        let scaler = factory
            .CreateBitmapScaler()
            .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;
        scaler
            .Initialize(
                frame,
                scaled_width,
                scaled_height,
                WICBitmapInterpolationModeCubic,
            )
            .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;

        scaler
            .cast()
            .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?
    } else {
        frame
            .cast()
            .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?
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
        .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;

    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(buffer_size)
        .map_err(|_| AppError::Internal("WIC pixel allocation failed".into()))?;
    pixels.resize(buffer_size, 0);

    converter
        .CopyPixels(std::ptr::null(), stride, &mut pixels)
        .map_err(|e| wic_error("WIC 图像处理失败 | WIC operation failed", e))?;

    // WIC 不保证应用容器方向；只在目标小图上转正。
    if orientation > 1 {
        let rgba = image::RgbaImage::from_raw(scaled_width, scaled_height, pixels)
            .ok_or_else(|| AppError::Internal("WIC RGBA buffer mismatch".into()))?;
        let img = image::DynamicImage::ImageRgba8(rgba);
        let img = apply_exif_orientation(img, orientation);
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
    let Some((context, mut size)) = frame_icc_context(factory, frame)? else {
        return Ok(None);
    };
    let mut profile = vec![0; size as usize];
    context
        .GetProfileBytes(&mut profile, &mut size)
        .map_err(|e| wic_error("WIC color profile read failed", e))?;
    if size as usize > profile.len() {
        return Err(pixel_limit());
    }
    profile.truncate(size as usize);
    Ok(Some(profile))
}

/// 只查询 profile 长度；读取 profile 字节前由调用方取得实际大小的额度。
pub(crate) unsafe fn frame_icc_context(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<Option<(IWICColorContext, u32)>, AppError> {
    let error = |e| wic_error("WIC 色彩配置读取失败 | WIC color profile read failed", e);
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
        return Err(pixel_limit());
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
            return Err(pixel_limit());
        }
        // 格式转换/缩放不做 CMS；沿用下游在小图上统一转到 sRGB 的出口。
        return Ok(Some((context, size)));
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
        _ => Err(pixel_limit()),
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
        Self::decode_open_file(&mut file, format, resize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_scaler_bounds_output_and_keeps_authorized_object() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        image::RgbaImage::from_pixel(2048, 1, image::Rgba([10, 20, 30, 128]))
            .save(&path)
            .unwrap();
        let mut file = File::open(&path).unwrap();
        std::fs::rename(&path, dir.path().join("original.png")).unwrap();
        std::fs::write(&path, b"replacement is not an image").unwrap();
        file.seek(std::io::SeekFrom::End(0)).unwrap();
        let hint = Some(ResizeHint::LongEdge(8));
        assert!(WicEngine::decode_open_file_bounded(&mut file, "png", hint, 31).is_err());
        let decoded = WicEngine::decode_open_file_bounded(&mut file, "png", hint, 32).unwrap();
        assert_eq!((decoded.width, decoded.height), (8, 1));
        assert_eq!(decoded.pixels.len(), 32);
        for pixel in decoded.pixels.as_chunks::<4>().0 {
            // WIC cubic 的整数舍入允许 RGB 一阶误差；alpha 必须原样保留。
            for (actual, expected) in pixel[..3].iter().zip([10u8, 20, 30]) {
                assert!(actual.abs_diff(expected) <= 1);
            }
            assert_eq!(pixel[3], 128);
        }
        assert!(WicEngine::decode_open_file_bounded(
            &mut file,
            "png",
            Some(ResizeHint::LongEdge(0)),
            32
        )
        .is_err());
        assert!(rgba_layout(u32::MAX, 2, u64::MAX).is_err());
        assert!(rgba_layout(0, 1, 32).is_err());
    }
}
