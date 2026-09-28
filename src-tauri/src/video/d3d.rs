//! D3D11 设备集与 MF 硬件候选；成功解码后的 MFT 事实另行确认。
//!
//! 宿主 MF 视频与原生图片/视频请求共用两项 GPU 准入，其中一项保留给快速域。
//! 槽满时沿用 CPU 回退；超时遗留 reader 持有名额至隔离进程退出或宿主结束。
//! AI 推理仍沿用其既有优先关系，本模块不重构 AI 设备服务。

use std::sync::OnceLock;

use windows::core::Interface;
use windows::core::GUID;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIFactory6,
    DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
};
use windows::Win32::Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager};

struct Hw {
    manager: IMFDXGIDeviceManager,
    identity: HardwareAdapterId,
    /// manager 内部持有 device,自留一份使生命周期与单例显式绑定(防仅剩 COM 内部引用的歧义)。
    _device: ID3D11Device,
    advertised_profiles: Vec<GUID>,
}

/// 视频设备候选的稳定标识；是否真正硬解由 MF 变换链另行确认。
#[derive(Clone, Copy)]
pub struct HardwareAdapterId {
    pub vendor_id: u32,
    pub device_id: u32,
    pub luid_high: i32,
    pub luid_low: u32,
}

// SAFETY: device 已经 `ID3D11Multithread::SetMultithreadProtected(true)` 开启多线程保护;
// `IMFDXGIDeviceManager` 本身就是为跨线程共享 device 而设计(MF 内部 worker 线程也经它取用)。
// 本模块只经不可变引用暴露两者,无内部可变性依赖调用方同步。
unsafe impl Send for Hw {}
unsafe impl Sync for Hw {}

static HW: OnceLock<Vec<Hw>> = OnceLock::new();

fn codec_profiles(codec: &str) -> &'static [GUID] {
    match codec.to_ascii_uppercase().as_str() {
        "H264" => &[
            D3D11_DECODER_PROFILE_H264_VLD_NOFGT,
            D3D11_DECODER_PROFILE_H264_VLD_FGT,
        ],
        "HEVC" => &[
            D3D11_DECODER_PROFILE_HEVC_VLD_MAIN,
            D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10,
        ],
        "MPEG2" => &[
            D3D11_DECODER_PROFILE_MPEG2_VLD,
            D3D11_DECODER_PROFILE_MPEG2and1_VLD,
        ],
        "MPEG4" => &[
            D3D11_DECODER_PROFILE_MPEG4PT2_VLD_SIMPLE,
            D3D11_DECODER_PROFILE_MPEG4PT2_VLD_ADVSIMPLE_NOGMC,
            D3D11_DECODER_PROFILE_MPEG4PT2_VLD_ADVSIMPLE_GMC,
        ],
        "VC1" => &[D3D11_DECODER_PROFILE_VC1_VLD],
        "VP9" => &[
            D3D11_DECODER_PROFILE_VP9_VLD_PROFILE0,
            D3D11_DECODER_PROFILE_VP9_VLD_10BIT_PROFILE2,
        ],
        "AV1" => &[
            D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0,
            D3D11_DECODER_PROFILE_AV1_VLD_PROFILE1,
            D3D11_DECODER_PROFILE_AV1_VLD_PROFILE2,
        ],
        _ => &[],
    }
}

fn supports_codec(profiles: &[GUID], codec: &str) -> bool {
    codec_profiles(codec)
        .iter()
        .any(|profile| profiles.contains(profile))
}

unsafe fn advertised_profiles(device: &ID3D11Device) -> Vec<GUID> {
    let Ok(video) = device.cast::<ID3D11VideoDevice>() else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    for index in 0..video.GetVideoDecoderProfileCount() {
        let Ok(profile) = video.GetVideoDecoderProfile(index) else {
            continue;
        };
        // profile 是驱动候选，不代表该片源尺寸、表面格式或 MF 变换链已成功硬解。
        profiles.push(profile);
    }
    profiles
}

fn is_hardware_adapter(desc: &DXGI_ADAPTER_DESC1) -> bool {
    desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0
}

/// 按 DXGI 高性能偏好列出非软件适配器，供视频与图像共同选择。
pub(crate) fn hardware_adapters() -> windows::core::Result<Vec<(IDXGIAdapter1, DXGI_ADAPTER_DESC1)>>
{
    // 高性能偏好由系统排序；旧 DXGI 仅能按默认枚举顺序回退。
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let preferred = factory.cast::<IDXGIFactory6>().ok();
    let mut adapters = Vec::new();
    let mut index = 0;
    loop {
        let adapter = unsafe {
            if let Some(factory6) = preferred.as_ref() {
                factory6.EnumAdapterByGpuPreference::<IDXGIAdapter1>(
                    index,
                    DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
                )
            } else {
                factory.EnumAdapters1(index)
            }
        };
        let Ok(adapter) = adapter else { break };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            index += 1;
            continue;
        };
        // 核显常无专用显存，不能按 DedicatedVideoMemory 排除。
        if is_hardware_adapter(&desc) {
            adapters.push((adapter, desc));
        }
        index += 1;
    }
    if adapters.is_empty() && preferred.is_some() {
        let mut index = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(index) } {
            if let Ok(desc) = unsafe { adapter.GetDesc1() } {
                if is_hardware_adapter(&desc) {
                    adapters.push((adapter, desc));
                }
            }
            index += 1;
        }
    }
    Ok(adapters)
}

unsafe fn create_hw_for_adapter(
    adapter: Option<&IDXGIAdapter1>,
    desc: Option<&DXGI_ADAPTER_DESC1>,
) -> Option<Hw> {
    let mut device: Option<ID3D11Device> = None;
    let driver_type = if adapter.is_some() {
        D3D_DRIVER_TYPE_UNKNOWN
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };
    if let Err(error) = D3D11CreateDevice(
        adapter.map(|adapter| &**adapter as &IDXGIAdapter),
        driver_type,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    ) {
        tracing::info!(
            ?error,
            vendor_id = desc.map(|d| d.VendorId),
            device_id = desc.map(|d| d.DeviceId),
            "video adapter unavailable"
        );
        return None;
    }
    let device = device?;
    // MF 会在其内部线程访问该 device，必须开多线程保护。
    if let Ok(ctx) = device.GetImmediateContext() {
        if let Ok(mt) = ctx.cast::<ID3D11Multithread>() {
            let _ = mt.SetMultithreadProtected(true);
        }
    }

    let mut token = 0u32;
    let mut mgr: Option<IMFDXGIDeviceManager> = None;
    if let Err(error) = MFCreateDXGIDeviceManager(&mut token, &mut mgr) {
        tracing::info!(?error, "video device manager unavailable");
        return None;
    }
    let mgr = mgr?;
    if let Err(error) = mgr.ResetDevice(&device, token) {
        tracing::info!(?error, "video device manager rejected adapter");
        return None;
    }
    tracing::info!(
        vendor_id = desc.map(|d| d.VendorId),
        device_id = desc.map(|d| d.DeviceId),
        dedicated_bytes = desc.map(|d| d.DedicatedVideoMemory),
        "video D3D11 device ready"
    );
    Some(Hw {
        manager: mgr,
        identity: HardwareAdapterId {
            vendor_id: desc.map_or(0, |d| d.VendorId),
            device_id: desc.map_or(0, |d| d.DeviceId),
            luid_high: desc.map_or(0, |d| d.AdapterLuid.HighPart),
            luid_low: desc.map_or(0, |d| d.AdapterLuid.LowPart),
        },
        advertised_profiles: advertised_profiles(&device),
        _device: device,
    })
}

fn hw() -> &'static [Hw] {
    HW.get_or_init(|| {
        // MFCreateDXGIDeviceManager 属 MF 平台函数,须在 MFStartup 之后(调用点均先 ensure_mf)。
        super::media_foundation::ensure_mf();
        match hardware_adapters() {
            Ok(adapters) => {
                let mut devices = Vec::new();
                for (adapter, desc) in &adapters {
                    if let Some(hw) = unsafe { create_hw_for_adapter(Some(adapter), Some(desc)) } {
                        devices.push(hw);
                    }
                }
                if devices.is_empty() {
                    tracing::info!(
                        "No usable video adapter; software decode | 无可用视频适配器，走软解"
                    );
                }
                devices
            }
            Err(error) => {
                tracing::info!(
                    ?error,
                    "DXGI enumeration failed; trying default hardware adapter"
                );
                unsafe { create_hw_for_adapter(None, None) }
                    .into_iter()
                    .collect()
            }
        }
    })
    .as_slice()
}

/// RAII 硬解槽位:持有期间占一个并发额度,Drop 归还。经 `manager()` 取 device manager
/// 挂到 SourceReader 属性上。
pub struct HwSlot {
    _gpu: crate::engine::gpu::budget::GpuPermit<'static>,
    hw: &'static Hw,
    index: usize,
}

impl HwSlot {
    /// 返回被选中候选设备的标识，不代表硬解已发生。
    pub fn identity(&self) -> HardwareAdapterId {
        self.hw.identity
    }
    pub fn manager(&self) -> &IMFDXGIDeviceManager {
        &self.hw.manager
    }

    pub fn next_index(&self) -> usize {
        self.index + 1
    }
}

/// 从指定顺位起找有对应驱动解码 profile 的适配器；共享 GPU 预算暂满时返回 None。
pub fn try_acquire_from(codec_hint: Option<&str>, start: usize) -> Option<HwSlot> {
    let devices = hw();
    for (index, device) in devices.iter().enumerate().skip(start) {
        if device.advertised_profiles.is_empty()
            || codec_hint.is_some_and(|codec| !supports_codec(&device.advertised_profiles, codec))
        {
            continue;
        }
        let gpu = crate::engine::gpu::budget::try_acquire(false)?;
        return Some(HwSlot {
            _gpu: gpu,
            hw: device,
            index,
        });
    }
    None
}

/// 是否至少存在暴露解码 profile 的设备候选（不代表某个流实际硬解）。
pub fn hw_available() -> bool {
    hw().iter()
        .any(|device| !device.advertised_profiles.is_empty())
}

/// 共享 GPU 槽占用快照:`(已用, 总量)`。仅供可观测性日志(如读帧超时泄漏槽后的额度计数),
/// 读原子无副作用、不改并发状态。
pub(crate) fn slots_snapshot() -> (usize, usize) {
    (
        crate::engine::gpu::budget::snapshot().0,
        crate::thumbnail::limits::get().gpu_inflight,
    )
}
