// src-tauri/src/video/d3d.rs
//! D3D11 device manager 单例 + 硬解槽位限流(视频性能线 T2,即 media_foundation.rs
//! 头注释多年的「可选动作 A」):给 `IMFSourceReader` 挂 `MF_SOURCE_READER_D3D_MANAGER`,
//! 使解码器 MFT 走 DXVA 硬解、XVP 色彩转换+缩放上 GPU,CPU 只做小图 readback。
//!
//! 设计:
//!  - **进程级单例**:一个 `ID3D11Device` + `IMFDXGIDeviceManager`,`OnceLock` 惰性创建;
//!    创建失败(无 GPU/远程会话/驱动异常)记一次日志,此后恒走软解 —— 失败不可影响正确性。
//!  - **槽位限流**:同时活跃的硬解 reader ≤ `HW_SLOTS`。GPU 硬解会话有限(NVDEC/QuickSync
//!    引擎并发有限),超发不提速反占显存;拿不到槽就走软解,所有 CPU 核仍满载,吞吐最优
//!    (task_plan D-002:try_acquire 不阻塞)。
//!  - 与 AI 推理的 `gpu_token` 无耦合:硬解用的是 GPU 视频引擎(NVDEC),与 3D/compute
//!    单元不同;仅共享显存,几十 MB 级 DPB 占用可忽略。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Multithread, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager};

/// 同时活跃的硬解 reader 上限。经验值:消费级 GPU 单视频引擎,3-4 路并发解码已到引擎吞吐;
/// 其余任务软解并行,避免排队串行化。后续如需按 GPU 档位调整,再做成配置。
const HW_SLOTS: usize = 4;

struct Hw {
    manager: IMFDXGIDeviceManager,
    /// manager 内部持有 device,自留一份使生命周期与单例显式绑定(防仅剩 COM 内部引用的歧义)。
    _device: ID3D11Device,
}

// SAFETY: device 已经 `ID3D11Multithread::SetMultithreadProtected(true)` 开启多线程保护;
// `IMFDXGIDeviceManager` 本身就是为跨线程共享 device 而设计(MF 内部 worker 线程也经它取用)。
// 本模块只经不可变引用暴露两者,无内部可变性依赖调用方同步。
unsafe impl Send for Hw {}
unsafe impl Sync for Hw {}

static HW: OnceLock<Option<Hw>> = OnceLock::new();
static SLOTS_IN_USE: AtomicUsize = AtomicUsize::new(0);

fn hw() -> Option<&'static Hw> {
    HW.get_or_init(|| {
        // MFCreateDXGIDeviceManager 属 MF 平台函数,须在 MFStartup 之后(调用点均先 ensure_mf)。
        super::media_foundation::ensure_mf();
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            if let Err(e) = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            ) {
                tracing::info!(
                    "Video hw decode unavailable (D3D11CreateDevice: {e}) — software path | 硬解不可用,走软解"
                );
                return None;
            }
            let device = device?;

            // MF 会在其内部线程访问该 device,必须开多线程保护(经 immediate context QI)。
            if let Ok(ctx) = device.GetImmediateContext() {
                if let Ok(mt) = ctx.cast::<ID3D11Multithread>() {
                    let _ = mt.SetMultithreadProtected(true);
                }
            }

            let mut token = 0u32;
            let mut mgr: Option<IMFDXGIDeviceManager> = None;
            if let Err(e) = MFCreateDXGIDeviceManager(&mut token, &mut mgr) {
                tracing::info!("MFCreateDXGIDeviceManager failed: {e} — software path | 硬解不可用");
                return None;
            }
            let mgr = mgr?;
            if let Err(e) = mgr.ResetDevice(&device, token) {
                tracing::info!("IMFDXGIDeviceManager::ResetDevice failed: {e} — software path | 硬解不可用");
                return None;
            }
            tracing::info!("Video hw decode ready (D3D11 + DXVA) | 视频硬解就绪");
            Some(Hw {
                manager: mgr,
                _device: device,
            })
        }
    })
    .as_ref()
}

/// RAII 硬解槽位:持有期间占一个并发额度,Drop 归还。经 `manager()` 取 device manager
/// 挂到 SourceReader 属性上。
pub struct HwSlot(&'static Hw);

impl HwSlot {
    pub fn manager(&self) -> &IMFDXGIDeviceManager {
        &self.0.manager
    }
}

impl Drop for HwSlot {
    fn drop(&mut self) {
        SLOTS_IN_USE.fetch_sub(1, Ordering::AcqRel);
    }
}

/// 尝试取一个硬解槽位:无 GPU/初始化失败/槽满 → None(调用方走软解,不阻塞不等待)。
pub fn try_acquire() -> Option<HwSlot> {
    let hw = hw()?;
    let mut cur = SLOTS_IN_USE.load(Ordering::Acquire);
    loop {
        if cur >= HW_SLOTS {
            return None;
        }
        match SLOTS_IN_USE.compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(HwSlot(hw)),
            Err(actual) => cur = actual,
        }
    }
}

/// 硬解是否可用(诊断/基准用)。
pub fn hw_available() -> bool {
    hw().is_some()
}

/// 硬解槽占用快照:`(已用, 总量)`。仅供可观测性日志(如读帧超时泄漏槽后的额度计数),
/// 读原子无副作用、不改并发状态。
pub(crate) fn slots_snapshot() -> (usize, usize) {
    (SLOTS_IN_USE.load(Ordering::Acquire), HW_SLOTS)
}
