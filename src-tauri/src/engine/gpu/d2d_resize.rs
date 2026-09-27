//! WIC 解码源经硬件适配器上的 Direct2D 缩至目标尺寸，再只回读小图。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Seek;
use std::os::windows::io::AsRawHandle;
use std::sync::{Arc, Mutex, Weak};

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateDevice, ID2D1Device, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_CREATION_PROPERTIES,
    D2D1_DEBUG_LEVEL_NONE, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_MAP_OPTIONS_READ,
    D2D1_THREADING_MODE_MULTI_THREADED,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, DXGI_ADAPTER_DESC1,
};
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};

use crate::engine::native::wic_engine::with_wic_factory;
use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};
use crate::scanner::metadata::read_jpeg_orientation_file;

const MAX_GPU_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
static GPU_SLOTS: GpuSlots = GpuSlots(Mutex::new(Vec::new()));

/// 图像硬件缩放的结果；未执行时保留明确的 CPU 回退原因。
pub enum GpuImageOutcome {
    /// D2D 已完成变换并回读为 RGBA 小图。
    Scaled(DecodedImage, GpuAdapter),
    /// 源有色彩配置，交给保留 ICC 的 CPU 解码路径。
    NeedsIccCpu,
    /// 无适用设备、额度或尺寸；正常 CPU 回退。
    Unavailable,
}

/// 完成变换的实际硬件适配器标识。
#[derive(Clone, Copy)]
pub struct GpuAdapter {
    pub vendor_id: u32,
    pub device_id: u32,
    pub luid_high: i32,
    pub luid_low: u32,
}

// 每进程最多两张源图，每设备一张；忙碌设备让后续适配器接手独立任务。
struct GpuSlots(Mutex<Vec<(i32, u32)>>);

impl GpuSlots {
    fn acquire(&self, luid: (i32, u32)) -> Option<GpuSlot<'_>> {
        let mut active = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if active.len() >= 2 || active.contains(&luid) {
            return None;
        }
        active.push(luid);
        Some(GpuSlot { slots: self, luid })
    }
}

pub(crate) struct GpuSlot<'a> {
    slots: &'a GpuSlots,
    luid: (i32, u32),
}

#[cfg(feature = "native-vpl")]
pub(crate) fn acquire_image_slot(adapter: GpuAdapter) -> Option<GpuSlot<'static>> {
    GPU_SLOTS.acquire((adapter.luid_high, adapter.luid_low))
}

impl Drop for GpuSlot<'_> {
    fn drop(&mut self) {
        let mut active = self.slots.0.lock().unwrap_or_else(|e| e.into_inner());
        active.retain(|luid| *luid != self.luid);
    }
}

struct AdapterSession {
    _d3d: ID3D11Device,
    _d2d: ID2D1Device,
    context: ID2D1DeviceContext,
    desc: DXGI_ADAPTER_DESC1,
    healthy: bool,
}

// SAFETY: D2D factory 显式使用多线程模式；完整绘制/回读仅通过 Mutex 访问。
// D3D device 未使用 SINGLETHREADED 标志，context 不向锁外暴露。
unsafe impl Send for AdapterSession {}

type SharedSession = Arc<Mutex<AdapterSession>>;
type SessionRegistry = BTreeMap<(i32, u32), Weak<Mutex<AdapterSession>>>;
static SHARED_SESSIONS: Mutex<SessionRegistry> = Mutex::new(BTreeMap::new());

thread_local! {
    static SESSIONS: RefCell<Option<Vec<SharedSession>>> = const { RefCell::new(None) };
}

fn shared_session(
    adapter: &IDXGIAdapter1,
    desc: DXGI_ADAPTER_DESC1,
) -> windows::core::Result<SharedSession> {
    let luid = (desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart);
    let mut registry = SHARED_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(session) = registry.get(&luid).and_then(Weak::upgrade) {
        return Ok(session);
    }
    // 注册表只保留弱引用；最后一个图像线程退出或切 CPU 后实际释放设备。
    registry.retain(|_, session| session.strong_count() > 0);
    let session = Arc::new(Mutex::new(unsafe { AdapterSession::new(adapter, desc)? }));
    registry.insert(luid, Arc::downgrade(&session));
    Ok(session)
}

pub(crate) fn clear_thread_sessions() {
    drop(SESSIONS.take());
}

fn has_healthy_session() -> bool {
    SESSIONS.with_borrow_mut(|sessions| {
        if sessions.is_none() {
            let adapters = match crate::video::d3d::hardware_adapters() {
                Ok(adapters) => adapters,
                Err(error) => {
                    tracing::warn!(target: "scrollery::thumb_perf", ?error, "image GPU adapter enumeration failed");
                    Vec::new()
                }
            };
            *sessions = Some(
                adapters
                    .into_iter()
                    .filter_map(|(adapter, desc)| match shared_session(&adapter, desc) {
                        Ok(session) => Some(session),
                        Err(error) => {
                            tracing::debug!(?error, vendor_id = desc.VendorId, device_id = desc.DeviceId, "image GPU adapter unavailable");
                            None
                        }
                    })
                    .collect(),
            );
        }
        sessions.as_ref().is_some_and(|sessions| sessions.iter().any(|session| session.try_lock().is_ok_and(|session| session.healthy)))
    })
}

impl AdapterSession {
    unsafe fn new(
        adapter: &IDXGIAdapter1,
        desc: DXGI_ADAPTER_DESC1,
    ) -> windows::core::Result<Self> {
        let mut device: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            Some(&**adapter as &IDXGIAdapter),
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
        let device = device.ok_or_else(windows::core::Error::from_win32)?;
        let dxgi: IDXGIDevice = device.cast()?;
        let properties = D2D1_CREATION_PROPERTIES {
            threadingMode: D2D1_THREADING_MODE_MULTI_THREADED,
            debugLevel: D2D1_DEBUG_LEVEL_NONE,
            options: D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
        };
        let d2d = D2D1CreateDevice(&dxgi, Some(&properties))?;
        let context = d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        Ok(Self {
            _d3d: device,
            _d2d: d2d,
            context,
            desc,
            healthy: true,
        })
    }

    unsafe fn resize(&self, source: &IWICBitmapSource, width: u32, height: u32) -> Result<Vec<u8>> {
        let pixel_format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        let props = |options| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: pixel_format,
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: options,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let source_bitmap = self
            .context
            .CreateBitmapFromWicBitmap(source, Some(&props(D2D1_BITMAP_OPTIONS_NONE)))
            .map_err(|e| AppError::os("D2D 图像上传失败 | D2D upload failed", e))?;
        let target = self
            .context
            .CreateBitmap(
                D2D_SIZE_U { width, height },
                None,
                0,
                &props(D2D1_BITMAP_OPTIONS_TARGET),
            )
            .map_err(|e| AppError::os("D2D 目标创建失败 | D2D target failed", e))?;
        self.context.SetTarget(&target);
        self.context.BeginDraw();
        self.context.Clear(None);
        self.context.DrawBitmap(
            &source_bitmap,
            Some(&D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: width as f32,
                bottom: height as f32,
            }),
            1.0,
            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
            None,
            None,
        );
        let draw_result = self.context.EndDraw(None, None);
        // 复用 context 时及时解绑，失败路径也不让上张目标纹理常驻。
        self.context.SetTarget(None);
        draw_result.map_err(|e| AppError::os("D2D 图像缩放失败 | D2D resize failed", e))?;

        let readback_options =
            D2D1_BITMAP_OPTIONS(D2D1_BITMAP_OPTIONS_CPU_READ.0 | D2D1_BITMAP_OPTIONS_CANNOT_DRAW.0);
        let readback = self
            .context
            .CreateBitmap(
                D2D_SIZE_U { width, height },
                None,
                0,
                &props(readback_options),
            )
            .map_err(|e| AppError::os("D2D 回读创建失败 | D2D readback failed", e))?;
        readback
            .CopyFromBitmap(None, &target, None)
            .map_err(|e| AppError::os("D2D 小图复制失败 | D2D copy failed", e))?;
        let mapped = readback
            .Map(D2D1_MAP_OPTIONS_READ)
            .map_err(|e| AppError::os("D2D 小图回读失败 | D2D map failed", e))?;
        let stride = width as usize * 4;
        if mapped.bits.is_null() || (mapped.pitch as usize) < stride {
            let _ = readback.Unmap();
            return Err(AppError::Internal("D2D mapped row is invalid".into()));
        }
        let mut pixels = vec![0u8; stride * height as usize];
        for row in 0..height as usize {
            let from =
                std::slice::from_raw_parts(mapped.bits.add(row * mapped.pitch as usize), stride);
            pixels[row * stride..(row + 1) * stride].copy_from_slice(from);
        }
        readback
            .Unmap()
            .map_err(|e| AppError::os("D2D 回读释放失败 | D2D unmap failed", e))?;
        unpremultiply_bgra(&mut pixels);
        Ok(pixels)
    }
}

fn unpremultiply_bgra(pixels: &mut [u8]) {
    // WIC/D2D 以预乘 BGRA 绘制；小图上还原现有直通 RGBA 编码契约。
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u32;
        let (blue, green, red) = (pixel[0] as u32, pixel[1] as u32, pixel[2] as u32);
        let straight = |value: u32| {
            (value * 255 + alpha / 2)
                .checked_div(alpha)
                .unwrap_or(0)
                .min(255) as u8
        };
        pixel[0] = straight(red);
        pixel[1] = straight(green);
        pixel[2] = straight(blue);
    }
}

#[cfg(test)]
mod tests {
    use super::unpremultiply_bgra;

    #[test]
    #[ignore = "requires a real Windows GPU for transparent PNG resize/readback"]
    fn hardware_resize_preserves_transparent_edges() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("alpha.png");
        // 透明区故意存蓝色；正确的预乘缩放不应把不可见颜色混进红色边缘。
        let source = image::RgbaImage::from_fn(64, 32, |x, _| {
            image::Rgba(if x < 32 {
                [255, 0, 0, 128]
            } else {
                [0, 0, 255, 0]
            })
        });
        source.save(&path).unwrap();
        crate::engine::native::wic_engine::with_image_worker_session(|| {
            let outcome = super::decode_open_file(
                std::fs::File::open(&path).unwrap(),
                "png",
                16,
                64 * 32 * 4,
            )
            .unwrap();
            let super::GpuImageOutcome::Scaled(decoded, adapter) = outcome else {
                panic!("real GPU resize required; CPU fallback is not evidence");
            };
            assert_eq!((decoded.width, decoded.height), (16, 8));
            assert_ne!(adapter.vendor_id, 0);
            let pixels =
                image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.pixels).unwrap();
            assert_eq!(pixels.get_pixel(2, 4).0, [255, 0, 0, 128]);
            assert_eq!(pixels.get_pixel(13, 4).0, [0, 0, 0, 0]);
            assert!(
                pixels.pixels().any(|p| p[3] > 0 && p[3] < 128),
                "scaled edge must exercise partial coverage"
            );
            for pixel in pixels.pixels().filter(|p| p[3] > 0) {
                assert!(
                    pixel[0] >= 250 && pixel[1] <= 1 && pixel[2] <= 1,
                    "transparent RGB contaminated edge: {pixel:?}"
                );
            }
            eprintln!(
                "D2D transparent edge verified: adapter={:04x}:{:04x}, output=16x8",
                adapter.vendor_id, adapter.device_id
            );
        });
    }

    #[test]
    fn gpu_readback_restores_rgba_and_transparent_alpha() {
        let mut pixels = [0, 0, 128, 128, 30, 20, 10, 255, 90, 70, 50, 0];
        unpremultiply_bgra(&mut pixels);
        assert_eq!(pixels, [255, 0, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }
}

/// 尝试同一硬件适配器上的图像缩放。无适用设备/额度时由调用方走原生 CPU 路径。
pub fn decode_open_file(
    mut file: File,
    format: &str,
    long_edge: u32,
    max_pixel_bytes: u64,
) -> Result<GpuImageOutcome> {
    use crate::thumbnail::route_diagnostics::{Attempt, Reason, Stage};
    let mut attempt = Attempt::new(Stage::D2d);
    if long_edge == 0 || long_edge > 2048 {
        attempt.reason = Reason::TargetLimit;
        return Ok(GpuImageOutcome::Unavailable);
    }
    let orientation = if matches!(format, "jpg" | "jpeg" | "heic" | "heif") {
        read_jpeg_orientation_file(&mut file)
    } else {
        1
    };
    file.rewind()?;
    with_wic_factory(|factory| unsafe {
        let decoder = factory
            .CreateDecoderFromFileHandle(
                file.as_raw_handle() as usize,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|e| AppError::os("WIC 图像处理失败 | WIC decode failed", e))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| AppError::os("WIC 图像处理失败 | WIC frame failed", e))?;
        let (mut width, mut height) = (0, 0);
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|e| AppError::os("WIC 图像处理失败 | WIC size failed", e))?;
        let source_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .unwrap_or(u64::MAX);
        if width == 0
            || height == 0
            || source_bytes > MAX_GPU_SOURCE_BYTES.min(max_pixel_bytes)
            || width.max(height) > 8192
            || width.max(height) <= long_edge
        {
            attempt.reason = Reason::SourceLimit;
            return Ok(GpuImageOutcome::Unavailable);
        }
        let mut color_contexts = 0u32;
        // 仅查询数量时传真正的空指针，与 WIC 读取路径保持相同的 codec 调用契约。
        let color_query = (frame.vtable().GetColorContexts)(
            frame.as_raw(),
            0,
            std::ptr::null_mut(),
            &mut color_contexts,
        );
        if color_contexts > 0 || color_query.is_err() {
            attempt.reason = Reason::Icc;
            return Ok(GpuImageOutcome::NeedsIccCpu);
        }
        if !has_healthy_session() {
            attempt.reason = Reason::NoSession;
            return Ok(GpuImageOutcome::Unavailable);
        }
        let (target_width, target_height) = if width >= height {
            (
                long_edge,
                ((u64::from(height) * u64::from(long_edge) + u64::from(width) / 2)
                    / u64::from(width))
                .max(1) as u32,
            )
        } else {
            (
                ((u64::from(width) * u64::from(long_edge) + u64::from(height) / 2)
                    / u64::from(height))
                .max(1) as u32,
                long_edge,
            )
        };
        let converter = factory
            .CreateFormatConverter()
            .map_err(|e| AppError::os("WIC 图像处理失败 | WIC converter failed", e))?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| AppError::os("WIC 图像处理失败 | WIC format failed", e))?;
        let source: IWICBitmapSource = converter
            .cast()
            .map_err(|e| AppError::os("WIC 图像处理失败 | WIC source failed", e))?;

        let mut device_failed = false;
        let scaled = SESSIONS.with_borrow_mut(|sessions| -> Result<Option<(Vec<u8>, GpuAdapter)>> {
            for shared in sessions.as_ref().expect("sessions initialized") {
                // 忙碌设备继续尝试后续 adapter，不阻塞另一个图像处理线程。
                let Ok(mut session) = shared.try_lock() else { continue; };
                if !session.healthy { continue; }
                let luid = (session.desc.AdapterLuid.HighPart, session.desc.AdapterLuid.LowPart);
                let Some(_slot) = GPU_SLOTS.acquire(luid) else { continue; };
                match session.resize(&source, target_width, target_height) {
                    Ok(pixels) => {
                        return Ok(Some((pixels, GpuAdapter {
                            vendor_id: session.desc.VendorId,
                            device_id: session.desc.DeviceId,
                            luid_high: session.desc.AdapterLuid.HighPart,
                            luid_low: session.desc.AdapterLuid.LowPart,
                        })));
                    }
                    Err(error) => {
                        device_failed = true;
                        session.healthy = false;
                        tracing::warn!(target: "scrollery::thumb_perf", vendor_id = session.desc.VendorId, device_id = session.desc.DeviceId, %error, "image GPU adapter disabled for this worker");
                    }
                }
            }
            Ok(None)
        })?;
        let Some((pixels, adapter)) = scaled else {
            attempt.reason = if device_failed {
                Reason::DeviceLost
            } else {
                Reason::Busy
            };
            return Ok(GpuImageOutcome::Unavailable);
        };
        let rgba = image::RgbaImage::from_raw(target_width, target_height, pixels)
            .ok_or_else(|| AppError::Internal("D2D RGBA buffer mismatch".into()))?;
        let image = image::DynamicImage::ImageRgba8(rgba);
        let image = match orientation {
            2 => image.fliph(),
            3 => image.rotate180(),
            4 => image.flipv(),
            5 => image.rotate90().fliph(),
            6 => image.rotate90(),
            7 => image.rotate270().fliph(),
            8 => image.rotate270(),
            _ => image,
        };
        attempt.reason = Reason::Success;
        Ok(GpuImageOutcome::Scaled(
            DecodedImage {
                width: image.width(),
                height: image.height(),
                pixels: image.into_rgba8().into_raw(),
                icc: None,
            },
            adapter,
        ))
    })
}
