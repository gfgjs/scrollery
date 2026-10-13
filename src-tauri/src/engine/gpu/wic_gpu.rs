//! Windows JPEG 平面交接及单线程、双槽 D3D11 / D2D 缩略图原型。

#[path = "wic_gpu/geometry.rs"]
mod geometry;

use self::geometry::{
    copy_rgba_rows, fits_budget, orientation_matrix, oriented_size, row_extent, supported_chroma,
    PlaneLayout, MAX_TEXTURE,
};
use crate::engine::traits::DecodedImage;
use crate::thumbnail::thumbhash::thumbhash_resize_target;
use std::{
    collections::VecDeque, ffi::c_void, marker::PhantomData, mem::size_of, rc::Rc, time::Instant,
};
use windows::{
    core::{Interface, HRESULT},
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::{BOOL, D2DERR_INTERNAL_ERROR, D2DERR_RECREATE_TARGET, HMODULE},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER,
                    D2D1_PIXEL_FORMAT,
                },
                *,
            },
            Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::*,
            Dxgi::{Common::*, *},
            Imaging::*,
        },
    },
};

/// 只暴露受控分类；底层设备错误不参与用户可见的错误消息。
#[derive(Debug, thiserror::Error)]
pub(crate) enum GpuError {
    #[error("gpu_unsupported")]
    Unsupported,
    #[error("gpu_budget")]
    Resource,
    #[error("gpu_api:{operation}")]
    Api {
        operation: &'static str,
        #[source]
        source: windows::core::Error,
    },
}

impl From<windows::core::Error> for GpuError {
    fn from(error: windows::core::Error) -> Self {
        Self::api("d3d11", error)
    }
}

impl GpuError {
    fn api(operation: &'static str, source: windows::core::Error) -> Self {
        Self::Api { operation, source }
    }

    /// 只有设备或绘制上下文已失效时，才停用本 worker 的 GPU 后端。
    pub(crate) fn disables_device(&self) -> bool {
        let Self::Api { operation, source } = self else {
            return false;
        };
        matches!(
            source.code(),
            DXGI_ERROR_DEVICE_REMOVED
                | DXGI_ERROR_DEVICE_RESET
                | DXGI_ERROR_DEVICE_HUNG
                | DXGI_ERROR_DRIVER_INTERNAL_ERROR
                | D2DERR_RECREATE_TARGET
                | D2DERR_INTERNAL_ERROR
        ) || (*operation == "device_status" && source.code() == DXGI_ERROR_INVALID_CALL)
    }

    /// 日志只记录固定阶段和数值 HRESULT，不带系统错误文本或用户路径。
    pub(crate) fn diagnostic_code(&self) -> String {
        match self {
            Self::Unsupported => "gpu_unsupported".into(),
            Self::Resource => "gpu_budget".into(),
            Self::Api { operation, source } => {
                format!("gpu_{operation}_0x{:08X}", source.code().0 as u32)
            }
        }
    }
}

/// 能力查询返回的真实平面尺寸。`target` 使用原图轴，方向变换在 GPU 上执行。
#[derive(Clone, Debug)]
pub(crate) struct PlanarLayout {
    size: (u32, u32),
    planes: [PlaneLayout; 2],
    target: (u32, u32),
    orientation: u32,
}

impl PlanarLayout {
    /// 查询 WIC 的原生缩小与两平面能力，不分配像素；源 JPEG / ICC 准入由调用方执行。
    pub(crate) fn query(
        frame: &IWICBitmapFrameDecode,
        target: (u32, u32),
        orientation: u32,
    ) -> Result<Option<Self>, GpuError> {
        oriented_size(target, orientation).ok_or(GpuError::Unsupported)?;
        // CMYK、灰度及其它像素格式不进入首版的 Y / CbCr 管线。
        if unsafe { frame.GetPixelFormat() }.map_err(|_| GpuError::Unsupported)?
            != GUID_WICPixelFormat24bppBGR
        {
            return Ok(None);
        }
        let transform: IWICPlanarBitmapSourceTransform = match frame.cast() {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let mut source = (0, 0);
        unsafe { frame.GetSize(&mut source.0, &mut source.1) }
            .map_err(|_| GpuError::Unsupported)?;
        if target.0 > source.0 || target.1 > source.1 {
            return Ok(None);
        }
        let formats = [GUID_WICPixelFormat8bppY, GUID_WICPixelFormat16bppCbCr];
        let mut descriptions = [WICBitmapPlaneDescription::default(); 2];
        let mut size = target;
        let mut supported = BOOL(0);
        unsafe {
            transform.DoesSupportTransform(
                &mut size.0,
                &mut size.1,
                WICBitmapTransformRotate0,
                WICPlanarOptionsPreserveSubsampling,
                formats.as_ptr(),
                descriptions.as_mut_ptr(),
                2,
                &mut supported,
            )
        }
        .map_err(|_| GpuError::Unsupported)?;
        if !supported.as_bool()
            || descriptions[0].Format != formats[0]
            || descriptions[1].Format != formats[1]
        {
            return Ok(None);
        }
        if size.0 < target.0
            || size.1 < target.1
            || size.0 > source.0
            || size.1 > source.1
            || (descriptions[0].Width, descriptions[0].Height) != size
        {
            return Ok(None);
        }
        let y = PlaneLayout::new(descriptions[0].Width, descriptions[0].Height, 1)
            .ok_or(GpuError::Resource)?;
        let cbcr = PlaneLayout::new(descriptions[1].Width, descriptions[1].Height, 2)
            .ok_or(GpuError::Resource)?;
        if !supported_chroma(y, cbcr) {
            return Ok(None);
        }
        Ok(Some(Self {
            size,
            planes: [y, cbcr],
            target,
            orientation,
        }))
    }

    /// 调用方在分配前据此预留 CPU 平面许可；目标 RGBA 的许可另计。
    pub(crate) fn cpu_bytes(&self) -> u64 {
        self.planes.iter().map(|p| u64::from(p.bytes)).sum()
    }

    /// 仅在 CPU 预算已预留后调用；WIC 使用能力查询给出的缩小尺寸直接解码平面。
    pub(crate) fn decode(&self, frame: &IWICBitmapFrameDecode) -> Result<PlanarImage, GpuError> {
        let transform: IWICPlanarBitmapSourceTransform =
            frame.cast().map_err(|_| GpuError::Unsupported)?;
        let mut y = allocate_bytes(self.planes[0].bytes as usize)?;
        let mut cbcr = allocate_bytes(self.planes[1].bytes as usize)?;
        let planes = [
            WICBitmapPlane {
                Format: GUID_WICPixelFormat8bppY,
                pbBuffer: y.as_mut_ptr(),
                cbStride: self.planes[0].stride,
                cbBufferSize: self.planes[0].bytes,
            },
            WICBitmapPlane {
                Format: GUID_WICPixelFormat16bppCbCr,
                pbBuffer: cbcr.as_mut_ptr(),
                cbStride: self.planes[1].stride,
                cbBufferSize: self.planes[1].bytes,
            },
        ];
        unsafe {
            transform.CopyPixels(
                std::ptr::null(),
                self.size.0,
                self.size.1,
                WICBitmapTransformRotate0,
                WICPlanarOptionsPreserveSubsampling,
                &planes,
            )
        }
        .map_err(|_| GpuError::Unsupported)?;
        Ok(PlanarImage {
            layout: self.clone(),
            planes: [y, cbcr],
        })
    }
}

/// 无 COM 引用，可由解码线程交给 GPU 线程；提交返回后可立即释放。
pub(crate) struct PlanarImage {
    layout: PlanarLayout,
    planes: [Vec<u8>; 2],
}

impl PlanarImage {
    /// 方向应用后的最终 RGBA 尺寸。
    pub(crate) fn target_size(&self) -> (u32, u32) {
        // layout 只能由已校验的 query 构造。
        oriented_size(self.layout.target, self.layout.orientation)
            .expect("validated planar orientation")
    }
}

/// 设备进程用量来自 DXGI；reserved 为本模块纹理的对齐估算，不含 D2D 驱动内部缓存。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuMemoryStats {
    pub reserved_peak_bytes: u64,
    pub usage_peak_bytes: u64,
    pub budget_bytes: u64,
}

pub(crate) struct GpuCompletion {
    pub id: u64,
    pub decoded: DecodedImage,
    pub thumbhash_input: Option<DecodedImage>,
    pub upload_us: u64,
    pub gpu_us: Option<u64>,
    pub readback_us: u64,
    pub completion_wait_us: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ResourceKey {
    planes: [PlaneLayout; 2],
    target: (u32, u32),
}

impl ResourceKey {
    fn reserved_bytes(self) -> Result<u64, GpuError> {
        let rgba = PlaneLayout::new(self.target.0, self.target.1, 4).ok_or(GpuError::Resource)?;
        let hash_bytes = thumbhash_resize_target(self.target.0, self.target.1)
            .map(|(width, height)| {
                PlaneLayout::new(width, height, 4)
                    .map(|plane| 2 * plane.reserved_bytes())
                    .ok_or(GpuError::Resource)
            })
            .transpose()?
            .unwrap_or(0);
        Ok(self.planes.iter().map(|p| p.reserved_bytes()).sum::<u64>()
            + 2 * rgba.reserved_bytes()
            + hash_bytes)
    }
}

struct Resources {
    key: ResourceKey,
    bytes: u64,
    y: ID3D11Texture2D,
    cbcr: ID3D11Texture2D,
    source: ID2D1ImageSource,
    display: RgbaTarget,
    thumbhash: Option<RgbaTarget>,
}

struct RgbaTarget {
    size: (u32, u32),
    target: ID3D11Texture2D,
    bitmap: ID2D1Bitmap1,
    readback: ID3D11Texture2D,
}

struct MappedReadback<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
    mapped: D3D11_MAPPED_SUBRESOURCE,
}

impl<'a> MappedReadback<'a> {
    fn try_map(
        context: &'a ID3D11DeviceContext,
        texture: &'a ID3D11Texture2D,
    ) -> Result<Option<Self>, GpuError> {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let result = unsafe {
            context.Map(
                texture,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            )
        };
        if !map_ready(result)? {
            return Ok(None);
        }
        Ok(Some(Self {
            context,
            texture,
            mapped,
        }))
    }

    fn copy_image(&self, (width, height): (u32, u32)) -> Result<DecodedImage, GpuError> {
        let extent = row_extent(width, height, self.mapped.RowPitch).ok_or(GpuError::Resource)?;
        if self.mapped.pData.is_null() {
            return Err(GpuError::Resource);
        }
        // EVENT 覆盖了两个 CopyResource；映射生命周期内只读有效行，排除末行 padding。
        let source = unsafe { std::slice::from_raw_parts(self.mapped.pData.cast::<u8>(), extent) };
        let pixels = copy_rgba_rows(source, width, height, self.mapped.RowPitch)
            .ok_or(GpuError::Resource)?;
        Ok(DecodedImage {
            pixels,
            width,
            height,
            icc: None,
        })
    }
}

impl Drop for MappedReadback<'_> {
    fn drop(&mut self) {
        // 第二张忙或任一复制失败时，也必须释放已经取得的第一张映射。
        unsafe { self.context.Unmap(self.texture, 0) };
    }
}

struct TimingQueries {
    disjoint: ID3D11Query,
    start: ID3D11Query,
    end: ID3D11Query,
}

struct Submission {
    id: u64,
    upload_us: u64,
    submitted: Instant,
}

struct Slot {
    resources: Option<Resources>,
    done: ID3D11Query,
    timing: Option<TimingQueries>,
    pending: Option<Submission>,
}

/// 在 GPU 线程上创建和使用；类型禁止跨线程搬运单线程设备/上下文。
pub(crate) struct GpuProcessor {
    slots: [Slot; 2],
    fifo: VecDeque<usize>,
    d2d: ID2D1DeviceContext2,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    adapter: IDXGIAdapter3,
    name: String,
    max_bytes: u64,
    max_texture: u32,
    stats: GpuMemoryStats,
    _thread: PhantomData<Rc<()>>,
}

impl GpuProcessor {
    /// 枚举硬件适配器并核对所需格式；不创建 WARP 或软件回退设备。
    pub(crate) fn new(max_gpu_bytes: u64) -> Result<Self, GpuError> {
        if max_gpu_bytes == 0 {
            return Err(GpuError::Resource);
        }
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let mut index = 0;
        let mut last_error = GpuError::Unsupported;
        loop {
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return Err(last_error),
                Err(error) => return Err(error.into()),
            };
            index += 1;
            let description = unsafe { adapter.GetDesc1()? };
            if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let name_len = description
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(description.Description.len());
            let name = String::from_utf16_lossy(&description.Description[..name_len]);
            match Self::from_adapter(&adapter, name, max_gpu_bytes) {
                Ok(processor) => return Ok(processor),
                Err(error) => last_error = error,
            }
        }
    }

    fn from_adapter(
        adapter: &IDXGIAdapter1,
        name: String,
        max_bytes: u64,
    ) -> Result<Self, GpuError> {
        let budget_adapter: IDXGIAdapter3 = adapter.cast()?;
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.ok_or(GpuError::Resource)?;
        let context = context.ok_or(GpuError::Resource)?;
        let dxgi_device: IDXGIDevice = device.cast()?;
        let factory: ID2D1Factory3 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let d2d_device = unsafe { factory.CreateDevice(&dxgi_device)? };
        let d2d = unsafe { d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        for format in [
            DXGI_FORMAT_R8_UNORM,
            DXGI_FORMAT_R8G8_UNORM,
            DXGI_FORMAT_R8G8B8A8_UNORM,
        ] {
            let flags = unsafe { device.CheckFormatSupport(format)? };
            let required =
                (D3D11_FORMAT_SUPPORT_TEXTURE2D.0 | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE.0) as u32;
            if flags & required != required
                || !unsafe { d2d.IsDxgiFormatSupported(format) }.as_bool()
            {
                return Err(GpuError::Unsupported);
            }
        }
        let rgba_support = unsafe { device.CheckFormatSupport(DXGI_FORMAT_R8G8B8A8_UNORM)? };
        if rgba_support & D3D11_FORMAT_SUPPORT_RENDER_TARGET.0 as u32 == 0 {
            return Err(GpuError::Unsupported);
        }
        let create_slot = || -> Result<Slot, GpuError> {
            let done = create_query(&device, D3D11_QUERY_EVENT)?;
            // 时间戳不是功能准入要求；驱动不支持时不伪造 GPU 用时。
            let timing = (|| -> Result<TimingQueries, GpuError> {
                Ok(TimingQueries {
                    disjoint: create_query(&device, D3D11_QUERY_TIMESTAMP_DISJOINT)?,
                    start: create_query(&device, D3D11_QUERY_TIMESTAMP)?,
                    end: create_query(&device, D3D11_QUERY_TIMESTAMP)?,
                })
            })()
            .ok();
            unsafe { device.GetDeviceRemovedReason() }
                .map_err(|error| GpuError::api("device_status", error))?;
            Ok(Slot {
                resources: None,
                done,
                timing,
                pending: None,
            })
        };
        let slots = [create_slot()?, create_slot()?];
        let max_texture = MAX_TEXTURE.min(unsafe { d2d.GetMaximumBitmapSize() });
        let mut processor = Self {
            slots,
            fifo: VecDeque::with_capacity(2),
            d2d,
            context,
            device,
            adapter: budget_adapter,
            name,
            max_bytes,
            max_texture,
            stats: GpuMemoryStats::default(),
            _thread: PhantomData,
        };
        processor.sample_memory()?;
        Ok(processor)
    }

    pub(crate) fn adapter_name(&self) -> &str {
        &self.name
    }
    pub(crate) fn available_slots(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.pending.is_none())
            .count()
    }
    pub(crate) fn stats(&self) -> GpuMemoryStats {
        self.stats
    }

    /// CPU 平面立即复制到纹理，方法返回后不再借用 `image`；最多两张在途。
    pub(crate) fn submit(&mut self, id: u64, image: &PlanarImage) -> Result<(), GpuError> {
        let key = ResourceKey {
            planes: image.layout.planes,
            target: image.target_size(),
        };
        let index = self
            .slots
            .iter()
            .position(|slot| {
                slot.pending.is_none()
                    && slot
                        .resources
                        .as_ref()
                        .is_some_and(|resources| resources.key == key)
            })
            .or_else(|| self.slots.iter().position(|slot| slot.pending.is_none()))
            .ok_or(GpuError::Resource)?;
        if key
            .planes
            .iter()
            .any(|plane| plane.width > self.max_texture || plane.height > self.max_texture)
            || key.target.0 > self.max_texture
            || key.target.1 > self.max_texture
        {
            return Err(GpuError::Resource);
        }
        self.ensure_resources(index, key)?;
        let slot = &self.slots[index];
        let resources = slot.resources.as_ref().ok_or(GpuError::Resource)?;
        let upload_start = Instant::now();
        unsafe {
            self.context.UpdateSubresource(
                &resources.y,
                0,
                None,
                image.planes[0].as_ptr().cast(),
                key.planes[0].stride,
                0,
            );
            self.context.UpdateSubresource(
                &resources.cbcr,
                0,
                None,
                image.planes[1].as_ptr().cast(),
                key.planes[1].stride,
                0,
            );
        }
        let upload_us = elapsed_us(upload_start);
        let m = orientation_matrix(
            image.layout.size,
            image.layout.target,
            image.layout.orientation,
        )
        .ok_or(GpuError::Unsupported)?;
        let matrix = Matrix3x2 {
            M11: m[0],
            M12: m[1],
            M21: m[2],
            M22: m[3],
            M31: m[4],
            M32: m[5],
        };
        unsafe {
            if let Some(timing) = &slot.timing {
                self.context.Begin(&timing.disjoint);
                self.context.End(&timing.start);
            }
            let result = (|| -> Result<(), GpuError> {
                self.d2d.SetTarget(&resources.display.bitmap);
                self.d2d.SetTransform(&matrix);
                self.d2d.BeginDraw();
                // JPEG 输出必须不透明；在 GPU 上写入 Alpha，CPU 回读只需原样复制 RGBA。
                let opaque = D2D1_COLOR_F {
                    r: 0.,
                    g: 0.,
                    b: 0.,
                    a: 1.,
                };
                self.d2d.Clear(Some(&opaque));
                self.d2d.DrawImage(
                    &resources.source,
                    None,
                    None,
                    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                    D2D1_COMPOSITE_MODE_SOURCE_OVER,
                );
                self.d2d
                    .EndDraw(None, None)
                    .map_err(|error| GpuError::api("end_draw", error))?;
                if let Some(hash) = &resources.thumbhash {
                    // 显示纹理已经应用方向；切换目标后仅按其最终轴缩小，不重复 EXIF 变换。
                    self.d2d.SetTarget(&hash.bitmap);
                    self.d2d.SetTransform(&Matrix3x2 {
                        M11: hash.size.0 as f32 / resources.display.size.0 as f32,
                        M12: 0.,
                        M21: 0.,
                        M22: hash.size.1 as f32 / resources.display.size.1 as f32,
                        M31: 0.,
                        M32: 0.,
                    });
                    self.d2d.BeginDraw();
                    self.d2d.Clear(Some(&opaque));
                    self.d2d.DrawImage(
                        &resources.display.bitmap,
                        None,
                        None,
                        D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                        D2D1_COMPOSITE_MODE_SOURCE_OVER,
                    );
                    self.d2d
                        .EndDraw(None, None)
                        .map_err(|error| GpuError::api("thumbhash_end_draw", error))?;
                }
                Ok(())
            })();
            // 解除上下文的目标引用，使已完成槽的纹理能按预算回收。
            self.d2d.SetTarget(None::<&ID2D1Image>);
            // 失败帧也要结束已开始的查询，后续图片才能安全复用这个槽。
            if let Some(timing) = &slot.timing {
                self.context.End(&timing.end);
                self.context.End(&timing.disjoint);
            }
            result?;
            self.context
                .CopyResource(&resources.display.readback, &resources.display.target);
            if let Some(hash) = &resources.thumbhash {
                self.context.CopyResource(&hash.readback, &hash.target);
            }
            self.context.End(&slot.done);
            // DONOTFLUSH 轮询自身不推进命令，提交时显式发出一次，不等待完成。
            self.context.Flush();
            self.device
                .GetDeviceRemovedReason()
                .map_err(|error| GpuError::api("device_status", error))?;
        }
        self.slots[index].pending = Some(Submission {
            id,
            upload_us,
            submitted: Instant::now(),
        });
        self.fifo.push_back(index);
        Ok(())
    }

    /// 按提交顺序非阻塞查询完成；只有 EVENT 完成后才 Map staging 纹理。
    pub(crate) fn poll(&mut self) -> Result<Option<GpuCompletion>, GpuError> {
        let result = self.poll_front();
        if result.as_ref().is_err_and(|error| !error.disables_device()) {
            // 普通错误只退役当前图片；调用方用同一个 FIFO 项走 CPU 回退。
            if let Some(index) = self.fifo.pop_front() {
                self.slots[index].pending = None;
            }
        }
        result
    }

    fn poll_front(&mut self) -> Result<Option<GpuCompletion>, GpuError> {
        let Some(&index) = self.fifo.front() else {
            return Ok(None);
        };
        let slot = &self.slots[index];
        let mut done = BOOL(0);
        if !get_query_data(&self.context, &slot.done, &mut done, "completion_query")?
            || !done.as_bool()
        {
            return Ok(None);
        }
        let completion_wait_us =
            elapsed_us(slot.pending.as_ref().ok_or(GpuError::Resource)?.submitted);
        let gpu_us = if let Some(timing) = &slot.timing {
            let mut disjoint = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
            let mut start = 0u64;
            let mut end = 0u64;
            if get_query_data(
                &self.context,
                &timing.disjoint,
                &mut disjoint,
                "timing_disjoint",
            )? && get_query_data(&self.context, &timing.start, &mut start, "timing_start")?
                && get_query_data(&self.context, &timing.end, &mut end, "timing_end")?
                && !disjoint.Disjoint.as_bool()
                && disjoint.Frequency != 0
            {
                end.checked_sub(start).map(|ticks| {
                    ((u128::from(ticks) * 1_000_000) / u128::from(disjoint.Frequency))
                        .min(u128::from(u64::MAX)) as u64
                })
            } else {
                None
            }
        } else {
            None
        };
        let resources = slot.resources.as_ref().ok_or(GpuError::Resource)?;
        let readback_start = Instant::now();
        let Some(display) = MappedReadback::try_map(&self.context, &resources.display.readback)?
        else {
            return Ok(None);
        };
        let hash = match &resources.thumbhash {
            Some(hash) => {
                let Some(mapped) = MappedReadback::try_map(&self.context, &hash.readback)? else {
                    return Ok(None);
                };
                Some((mapped, hash.size))
            }
            None => None,
        };
        // 两张都就绪后再分配输出，避免小图忙时反复复制显示图。
        let decoded = display.copy_image(resources.display.size)?;
        let thumbhash_input = hash
            .as_ref()
            .map(|(mapped, size)| mapped.copy_image(*size))
            .transpose()?;
        drop(hash);
        drop(display);
        let readback_us = elapsed_us(readback_start);
        // 此处仍保留当前 FIFO 项，预算查询失败不会误退役后面的图片。
        self.sample_memory()?;
        let pending = self.slots[index].pending.take().ok_or(GpuError::Resource)?;
        self.fifo.pop_front();
        Ok(Some(GpuCompletion {
            id: pending.id,
            decoded,
            thumbhash_input,
            upload_us: pending.upload_us,
            gpu_us,
            readback_us,
            completion_wait_us,
        }))
    }

    fn reserved_bytes(&self) -> u64 {
        self.slots
            .iter()
            .filter_map(|slot| slot.resources.as_ref())
            .map(|resources| resources.bytes)
            .sum()
    }

    fn sample_memory(&mut self) -> Result<u64, GpuError> {
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        unsafe {
            self.adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                .map_err(|error| GpuError::api("memory_budget", error))?;
        }
        self.stats.reserved_peak_bytes = self.stats.reserved_peak_bytes.max(self.reserved_bytes());
        self.stats.usage_peak_bytes = self.stats.usage_peak_bytes.max(info.CurrentUsage);
        self.stats.budget_bytes = self.max_bytes.min(info.Budget);
        Ok(info.Budget.saturating_sub(info.CurrentUsage))
    }

    fn ensure_resources(&mut self, index: usize, key: ResourceKey) -> Result<(), GpuError> {
        let reuse = self.slots[index]
            .resources
            .as_ref()
            .is_some_and(|resources| resources.key == key);
        if !reuse {
            self.slots[index].resources = None;
        }
        let additional = if reuse { 0 } else { key.reserved_bytes()? };
        let mut available = self.sample_memory()?;
        if !fits_budget(
            self.reserved_bytes(),
            additional,
            self.stats.budget_bytes,
            available,
        ) {
            // 空闲缓存不是在途工作集：先回收它，再重新查询预算，避免无意义地拒绝可处理任务。
            for (other, slot) in self.slots.iter_mut().enumerate() {
                if other != index && slot.pending.is_none() {
                    slot.resources = None;
                }
            }
            available = self.sample_memory()?;
            if !fits_budget(
                self.reserved_bytes(),
                additional,
                self.stats.budget_bytes,
                available,
            ) {
                return Err(GpuError::Resource);
            }
        }
        if !reuse {
            let resources = self.create_resources(key)?;
            self.slots[index].resources = Some(resources);
            // DXGI 计入驱动分配后再核对实际预算，超出即释放尚未提交的资源。
            self.sample_memory()?;
            if self.reserved_bytes() > self.stats.budget_bytes {
                self.slots[index].resources = None;
                return Err(GpuError::Resource);
            }
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            unsafe {
                self.adapter
                    .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                    .map_err(|error| GpuError::api("memory_budget", error))?;
            }
            if info.CurrentUsage > info.Budget {
                self.slots[index].resources = None;
                return Err(GpuError::Resource);
            }
        }
        Ok(())
    }

    fn create_resources(&self, key: ResourceKey) -> Result<Resources, GpuError> {
        let y = create_texture(
            &self.device,
            (key.planes[0].width, key.planes[0].height),
            DXGI_FORMAT_R8_UNORM,
            false,
            false,
            "texture_y",
        )?;
        let cbcr = create_texture(
            &self.device,
            (key.planes[1].width, key.planes[1].height),
            DXGI_FORMAT_R8G8_UNORM,
            false,
            false,
            "texture_cbcr",
        )?;
        let surfaces = [
            Some(y.cast::<IDXGISurface>()?),
            Some(cbcr.cast::<IDXGISurface>()?),
        ];
        let source = unsafe {
            self.d2d
                .CreateImageSourceFromDxgi(
                    &surfaces,
                    DXGI_COLOR_SPACE_YCBCR_FULL_G22_NONE_P709_X601,
                    D2D1_IMAGE_SOURCE_FROM_DXGI_OPTIONS_NONE,
                )
                .map_err(|error| GpuError::api("image_source", error))?
        };
        let display = self.create_rgba_target(
            key.target,
            ["texture_target", "target_bitmap", "texture_readback"],
        )?;
        let thumbhash = thumbhash_resize_target(key.target.0, key.target.1)
            .map(|size| {
                self.create_rgba_target(
                    size,
                    ["thumbhash_target", "thumbhash_bitmap", "thumbhash_readback"],
                )
            })
            .transpose()?;
        Ok(Resources {
            key,
            bytes: key.reserved_bytes()?,
            y,
            cbcr,
            source,
            display,
            thumbhash,
        })
    }

    fn create_rgba_target(
        &self,
        size: (u32, u32),
        operations: [&'static str; 3],
    ) -> Result<RgbaTarget, GpuError> {
        let target = create_texture(
            &self.device,
            size,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            true,
            false,
            operations[0],
        )?;
        let target_surface: IDXGISurface = target.cast()?;
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.,
            dpiY: 96.,
            // 显示结果还会成为 ThumbHash 小图的来源，因此保留可绘制能力。
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            ..Default::default()
        };
        let bitmap = unsafe {
            self.d2d
                .CreateBitmapFromDxgiSurface(&target_surface, Some(&properties))
                .map_err(|error| GpuError::api(operations[1], error))?
        };
        let readback = create_texture(
            &self.device,
            size,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            false,
            true,
            operations[2],
        )?;
        Ok(RgbaTarget {
            size,
            target,
            bitmap,
            readback,
        })
    }
}

fn create_texture(
    device: &ID3D11Device,
    size: (u32, u32),
    format: DXGI_FORMAT,
    target: bool,
    staging: bool,
    operation: &'static str,
) -> Result<ID3D11Texture2D, GpuError> {
    let descriptor = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: if staging {
            D3D11_USAGE_STAGING
        } else {
            D3D11_USAGE_DEFAULT
        },
        BindFlags: if staging {
            0
        } else if target {
            (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32
        } else {
            D3D11_BIND_SHADER_RESOURCE.0 as u32
        },
        CPUAccessFlags: if staging {
            D3D11_CPU_ACCESS_READ.0 as u32
        } else {
            0
        },
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&descriptor, None, Some(&mut texture))
            .map_err(|error| GpuError::api(operation, error))?;
    }
    texture.ok_or(GpuError::Resource)
}

fn create_query(device: &ID3D11Device, kind: D3D11_QUERY) -> Result<ID3D11Query, GpuError> {
    let mut query = None;
    unsafe {
        device
            .CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: kind,
                    MiscFlags: 0,
                },
                Some(&mut query),
            )
            .map_err(|error| GpuError::api("create_query", error))?;
    }
    query.ok_or(GpuError::Resource)
}

fn get_query_data<T>(
    context: &ID3D11DeviceContext,
    query: &ID3D11Query,
    output: &mut T,
    operation: &'static str,
) -> Result<bool, GpuError> {
    // windows 0.58 的 GetData().ok() 会把 S_FALSE 也变成 Ok；必须保留 HRESULT 完成语义。
    let result = unsafe {
        (context.vtable().GetData)(
            context.as_raw(),
            query.as_raw(),
            (output as *mut T).cast::<c_void>(),
            size_of::<T>() as u32,
            D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
        )
    };
    match result {
        HRESULT(0) => Ok(true),
        HRESULT(1) => Ok(false),
        error => Err(GpuError::api(
            operation,
            windows::core::Error::from_hresult(error),
        )),
    }
}

fn map_ready(result: windows::core::Result<()>) -> Result<bool, GpuError> {
    match result {
        Ok(()) => Ok(true),
        // DO_NOT_WAIT 的忙码只表示尚未能映射，不能退役任务或停用设备。
        Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => Ok(false),
        Err(error) => Err(GpuError::api("map_readback", error)),
    }
}

fn allocate_bytes(len: usize) -> Result<Vec<u8>, GpuError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| GpuError::Resource)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

fn elapsed_us(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{D2DERR_UNSUPPORTED_OPERATION, E_INVALIDARG, E_OUTOFMEMORY};

    fn api(operation: &'static str, code: HRESULT) -> GpuError {
        GpuError::api(operation, windows::core::Error::from_hresult(code))
    }

    #[test]
    fn device_loss_disables_but_api_rejection_preserves_backend() {
        assert!(api("map_readback", DXGI_ERROR_DEVICE_REMOVED).disables_device());
        assert!(api("completion_query", DXGI_ERROR_DEVICE_RESET).disables_device());
        assert!(api("end_draw", DXGI_ERROR_DEVICE_HUNG).disables_device());
        assert!(api("end_draw", DXGI_ERROR_DRIVER_INTERNAL_ERROR).disables_device());
        assert!(api("end_draw", D2DERR_RECREATE_TARGET).disables_device());
        assert!(api("end_draw", D2DERR_INTERNAL_ERROR).disables_device());
        assert!(api("device_status", DXGI_ERROR_INVALID_CALL).disables_device());
        assert!(!api("image_source", E_INVALIDARG).disables_device());
        assert!(!api("texture_y", E_OUTOFMEMORY).disables_device());
        assert!(!api("end_draw", D2DERR_UNSUPPORTED_OPERATION).disables_device());
        assert!(!api("map_readback", DXGI_ERROR_WAS_STILL_DRAWING).disables_device());
        assert!(!api("image_source", DXGI_ERROR_INVALID_CALL).disables_device());
        assert!(!GpuError::Unsupported.disables_device());
        assert!(!GpuError::Resource.disables_device());
    }
}
