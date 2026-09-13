// src-tauri/src/video/media_foundation.rs
//! Media Foundation video backend (§3.2 / §3.3) — Windows-native, zero-bundle decoding via
//! the `windows` crate (no FFmpeg / no external binary). Used in BOTH Lite and Perf variants.
//!
//! Media Foundation 视频后端（§3.2 / §3.3）—— 基于 `windows` crate 的 Windows 原生、零捆绑解码
//! （无 FFmpeg / 无外部二进制）。Lite 与 Perf 两变体都使用。
//!
//! 关键设计（2026-07-17 性能线重构后）：
//!  - 用 `IMFSourceReader` + `MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING`，由 XVP
//!    （视频处理器 MFT）把任意输入像素格式转成 **RGB32**（内存字节序 B,G,R,X），并**在 XVP 内
//!    直接缩放到目标输出尺寸**（封面=缩略图 tier、雪碧格=200px 高）——此前按原生分辨率直出，
//!    4K 单帧 33MB 要在 CPU 上做 4-5 趟全尺寸拷贝，是本后端最大的性能浪费。
//!  - **DXVA/D3D11 硬解（原「可选动作 A」，现已接）**：能拿到 `video::d3d` 硬解槽位时给
//!    SourceReader 挂 `MF_SOURCE_READER_D3D_MANAGER`，解码走 GPU 视频引擎、XVP 转换/缩放上
//!    GPU，CPU 只 readback 小图；拿不到槽位或初始化失败即回退软解（正确性不依赖 GPU）。
//!  - **旋转**：ADVANCED 管线下 XVP 会按 `MF_MT_VIDEO_ROTATION` **自动转正**,且实测
//!    （Win11 + rot90 基准片）转正后输出类型的 rotation 属性并不清零 —— 属性不可作判据,
//!    曾与 CPU 侧 apply_rotation 叠成双旋转。定案:协商后枚举变换链,经
//!    `IMFVideoProcessorControl::SetRotation(ROTATION_NONE)` **强制关掉** XVP 自动转正,
//!    旋转权威唯一归 CPU `apply_rotation`（legacy 管线无该控制接口 = 本就不转,两路归一;
//!    90/180/270/方形全覆盖）。尺寸请求相应固定在**解码坐标系**（旋转前）。
//!    （防回归断言见 examples/video_bench.rs 的 --rotation-check。）
//!  - **负 stride**：RGB32 常以 bottom-up（负 stride）交付，按 `MF_MT_DEFAULT_STRIDE` 符号翻转行序。
//!  - **单会话复用**：封面时间戳选择所需的时长在同一 reader 会话内读取，不再为 probe 单开
//!    一个 reader;音频流反选,省掉音频解码器初始化。

use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Once};
use std::time::{Duration, Instant};

use windows::core::{implement, Interface, GUID, HRESULT, HSTRING, PROPVARIANT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};
use crate::video::{VideoBackend, VideoInfo};

// 属性/编解码器辅助（尾段拆出，零 unsafe 耦合，见超长文件拆分方案 tierB-3）。
use super::mf_attrs::{attr_ratio, attr_size, codec_label, normalize_rotation, read_duration_ms};
// 帧后处理（尾段拆出，全程不碰 `IMFSourceReader`，见超长文件拆分方案 tierB-3）。
use super::frame_post::{apply_rotation, copy_bgr32_to_rgba, is_too_dark, resize_rgba};

/// MF 一般能用系统已装编解码器处理的容器。mkv/webm/flv/ogv 有意排除（需 FFmpeg / Perf，§9）。
const MF_VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "mov", "wmv", "avi", "3gp", "3g2", "ts", "mts", "m2ts", "asf", "mpg", "mpeg",
];

/// 流选择子（MF SDK 中的 `u32` 哨兵值）。
const FIRST_VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const FIRST_AUDIO_STREAM: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;
/// `pub(super)`：`mf_attrs::read_duration_ms`（video 模块内的兄弟文件）需要引用此哨兵值。
pub(super) const MEDIASOURCE: u32 = MF_SOURCE_READER_MEDIASOURCE.0 as u32;

pub struct MediaFoundationBackend;

impl VideoBackend for MediaFoundationBackend {
    fn name(&self) -> &'static str {
        "media-foundation"
    }

    fn can_handle(&self, ext: &str) -> bool {
        MF_VIDEO_EXTS.contains(&ext)
    }

    fn probe(&self, path: &Path) -> Result<VideoInfo> {
        ensure_mf();
        init_com();
        unsafe {
            // 探测不解码帧,无需硬解/尺寸协商,开个裸 reader 读原生属性即可。
            // （异步回调已挂上但 probe 从不 ReadSample,故回调永不触发；仅取元数据同步方法。）
            // 批量导入会把单文件探测失败汇总到富化日志；这里不逐项写 ERROR，避免损坏/不兼容
            // 容器在大批量导入时制造错误风暴。
            let (reader, _cb) = open_reader(path, None, false)?;
            // 从 NATIVE 类型（转换前）读取几何/旋转/帧率，更准确。
            let native = reader
                .GetNativeMediaType(FIRST_VIDEO_STREAM, 0)
                .map_err(mf_probe_err)?;

            let (nw, nh) = attr_size(&native, &MF_MT_FRAME_SIZE).unwrap_or((0, 0));
            let rotation = normalize_rotation(native.GetUINT32(&MF_MT_VIDEO_ROTATION).unwrap_or(0));
            let fps = attr_ratio(&native, &MF_MT_FRAME_RATE)
                .map(|(n, d)| if d > 0 { n as f32 / d as f32 } else { 0.0 })
                .unwrap_or(0.0);
            let bitrate = native.GetUINT32(&MF_MT_AVG_BITRATE).unwrap_or(0);
            let codec = native.GetGUID(&MF_MT_SUBTYPE).ok().and_then(codec_label);

            // 90/270 旋转交换显示宽高（与图片 EXIF orientation 同理）。
            let (width, height) = if rotation == 90 || rotation == 270 {
                (nh, nw)
            } else {
                (nw, nh)
            };

            let duration_ms = read_duration_ms(&reader);
            let has_audio = reader.GetNativeMediaType(FIRST_AUDIO_STREAM, 0).is_ok();

            Ok(VideoInfo {
                width,
                height,
                duration_ms,
                rotation,
                fps,
                bitrate,
                has_audio,
                codec,
            })
        }
    }

    fn cover(&self, path: &Path, max_long_edge: u32) -> Result<DecodedImage> {
        ensure_mf();
        init_com();
        unsafe {
            let s = open_session(path, SizePolicy::FitLongEdge(max_long_edge))?;

            // 封面时间戳 = min(1s, 时长 10%)，避开常为黑帧的最初一帧;时长未知回退 1s。
            // 时长来自本会话（read_duration_ms），免掉旧实现里独立 probe() 的一次 reader open。
            let t_ms = if s.duration_ms > 0 {
                1000u64.min(s.duration_ms / 10)
            } else {
                1000
            };

            // 避开第 0 帧（常为黑帧）。尝试几个时间戳，取首个足够亮的帧；最后一次尝试则照单全收。
            let mut t_100ns = (t_ms as i64) * 10_000;
            let mut last: Option<DecodedImage> = None;
            for attempt in 0..5 {
                match read_frame_at(&s.reader, &s.cb, t_100ns, &s.geom, path) {
                    Ok(img) => {
                        let dark = is_too_dark(&img);
                        if !dark || attempt == 4 {
                            return Ok(apply_rotation(img, s.rotation));
                        }
                        last = Some(img);
                    }
                    Err(_) => break,
                }
                t_100ns += 5_000_000; // +0.5s
            }
            // 全部偏暗或读取在末尾失败：用最后拿到的一帧，否则回退到第 0 帧。
            if let Some(img) = last {
                return Ok(apply_rotation(img, s.rotation));
            }
            let img = read_frame_at(&s.reader, &s.cb, 0, &s.geom, path)?;
            Ok(apply_rotation(img, s.rotation))
        }
    }

    fn keyframes(&self, path: &Path, n: usize, cell_height: u32) -> Result<Vec<DecodedImage>> {
        ensure_mf();
        init_com();
        let n = n.max(1);
        unsafe {
            let s = open_session(path, SizePolicy::CellHeight(cell_height))?;
            // 由显示比例推导统一格尺寸，使雪碧图为整齐的水平条带。
            let (cell_w, cell_h) = sprite_cell(s.display_w, s.display_h, cell_height);

            // 在 [5%, 95%] 区间采样，跳过片头/片尾黑帧。
            let dur = s.duration_ms.max(1) as f64;
            let mut frames = Vec::with_capacity(n);
            for i in 0..n {
                let frac = if n == 1 {
                    0.5
                } else {
                    0.05 + 0.90 * (i as f64 / (n - 1) as f64)
                };
                let t_100ns = (dur * frac * 10_000.0) as i64;
                if let Ok(img) = read_frame_at(&s.reader, &s.cb, t_100ns, &s.geom, path) {
                    let upright = apply_rotation(img, s.rotation);
                    // XVP 已按格尺寸出图时为等尺寸直通（zero-copy）;原生回退路径下做真缩放。
                    frames.push(resize_rgba(upright, cell_w, cell_h));
                }
            }
            if frames.is_empty() {
                return Err(AppError::Internal(
                    "no keyframes decoded | 未解码到关键帧".into(),
                ));
            }
            Ok(frames)
        }
    }
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────
// ── 生命周期 ─────────────────────────────────────────────────────────────────

/// 进程生命周期内只启动一次 MF 平台。有意不调用 `MFShutdown` —— 长期运行应用的标准做法，
/// 避免在 rayon 派生池中逐任务配平的脆弱性。
/// （pub(crate)：`video::d3d` 创建 DXGI device manager 前也须保证 MF 已启动。）
pub(crate) fn ensure_mf() {
    static MF_INIT: Once = Once::new();
    MF_INIT.call_once(|| unsafe {
        // MF_VERSION = (SDK<<16)|API = (0x0002<<16)|0x0070；MFSTARTUP_FULL = 0。
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    });
}

/// Ensure COM is initialised (MTA) on the current thread — MF objects require it. Rayon worker
/// threads start uninitialised; STA elsewhere returns `RPC_E_CHANGED_MODE`, harmless here.
/// 确保当前线程已初始化 COM（MTA）—— MF 对象需要。rayon 工作线程初始为未初始化；
/// 别处若为 STA 会返回 `RPC_E_CHANGED_MODE`，对此处无害。
fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

// ── Decode session ──────────────────────────────────────────────────────────────
// ── 解码会话 ────────────────────────────────────────────────────────────────────

/// 输出尺寸策略。请求值最终换算到**解码坐标系**（旋转前）交给 XVP。
enum SizePolicy {
    /// 正立后长边 ≤ n（**不上采样**;0 = 保持原生）。封面用，与缩略图 tier 对齐，
    /// 使 encode_media_step 的 resize 成为直通。
    FitLongEdge(u32),
    /// 正立后格高恒为 h、格宽按显示比例（小视频**允许上采样**，保证雪碧格统一）。雪碧图用。
    CellHeight(u32),
}

/// 一次打开、配置完毕的解码会话:reader + 协商后的输出几何 + 元数据。
struct Session {
    /// `ManuallyDrop`:读超时后 reader 可能卡在 MF 共享工作队列,`Release()`/`Drop` 会与
    /// `ReadSample` 同样死等永不返回 —— 届时由 `Drop for Session` 泄漏隔离(见下)。
    reader: ManuallyDrop<IMFSourceReader>,
    /// 异步读的回调共享态(样本交接槽 + 超时/僵死标记)。
    cb: Arc<CallbackShared>,
    geom: Geometry,
    /// **残余**旋转:XVP 已自动转正时为 0;legacy 管线透传原值,由 CPU `apply_rotation` 补旋。
    rotation: i32,
    duration_ms: u64,
    /// 正立（显示）尺寸，来自 NATIVE 类型 + rotation（与 probe 口径一致）。
    display_w: u32,
    display_h: u32,
    /// 硬解槽位（RAII，Drop 归还并发额度;None = 软解）。仅作所有权锚。
    _hw: Option<crate::video::d3d::HwSlot>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.cb.timed_out.load(Ordering::Acquire) {
            // 曾读超时 = reader 已卡在 MF 进程级共享工作队列;此时 `Release()`/`Drop` 极可能
            // 与 `ReadSample` 一样死等永不返回。泄漏隔离:**不析构 reader**(相当于 mem::forget),
            // 连同硬解槽一并弃掉(该槽可能仍被 GPU 占用)。callback COM 对象经泄漏的 reader 保活,
            // 迟到的 `OnReadSample`(30s 后终于返回时)仍有合法共享态可写,不会 use-after-free。
            if self._hw.is_some() {
                // 硬解槽随 reader 一并被永久占用(其 Drop 的额度归还被 forget 跳过)。累计泄漏
                // 会逐步耗尽有限的硬解额度,故留一条 warn 使「多毒文件耗尽额度」在日志可见。
                // 读快照须在 forget 之前——此刻该槽仍计入 SLOTS_IN_USE(此后永久停留在此值)。
                let (used, total) = crate::video::d3d::slots_snapshot();
                tracing::warn!(
                    slots_used = used,
                    slots_total = total,
                    "硬解槽因读帧超时被永久占用(泄漏隔离),已用/总量={used}/{total} | \
                     hw decode slot permanently leaked due to read timeout"
                );
            }
            std::mem::forget(self._hw.take());
            // 注:此处刻意不调用 `ManuallyDrop::drop(&mut self.reader)` —— 即泄漏 reader。
        } else {
            // 正常路径:reader 仅此一处析构。
            // SAFETY: `drop` 只跑一次,之后 `self.reader` 不再被访问。
            unsafe { ManuallyDrop::drop(&mut self.reader) }
        }
    }
}

/// 打开解码会话:硬解优先（拿到槽位才试），open/协商失败回退软解 —— GPU 缺失/驱动异常
/// 只影响速度，不影响正确性。
unsafe fn open_session(path: &Path, policy: SizePolicy) -> Result<Session> {
    if let Some(slot) = crate::video::d3d::try_acquire() {
        match open_session_inner(path, &policy, Some(&slot)) {
            Ok(mut s) => {
                s._hw = Some(slot);
                return Ok(s);
            }
            Err(e) => {
                tracing::debug!(
                    "hw video session failed, falling back to software | 硬解会话失败，回退软解: {e}"
                );
            }
        }
    }
    open_session_inner(path, &policy, None)
}

unsafe fn open_session_inner(
    path: &Path,
    policy: &SizePolicy,
    hw: Option<&crate::video::d3d::HwSlot>,
) -> Result<Session> {
    let (reader, cb) = open_reader(path, hw, true)?;
    select_video_only(&reader);

    let native = reader
        .GetNativeMediaType(FIRST_VIDEO_STREAM, 0)
        .map_err(mf_err)?;
    let raw_rot = native.GetUINT32(&MF_MT_VIDEO_ROTATION).unwrap_or(0);
    let native_rotation = normalize_rotation(raw_rot);
    let (nw, nh) = attr_size(&native, &MF_MT_FRAME_SIZE).unwrap_or((0, 0));
    let (display_w, display_h) = if native_rotation == 90 || native_rotation == 270 {
        (nh, nw)
    } else {
        (nw, nh)
    };
    let duration_ms = read_duration_ms(&reader);

    // 请求尺寸:策略按显示坐标系计算,再换算回**解码坐标系**(旋转前)交给 XVP ——
    // 下面会把 XVP 自动转正关掉,解码输出恒为未旋转朝向,CPU apply_rotation 统一转正。
    let request = request_size(display_w, display_h, policy).map(|(w, h)| {
        if native_rotation == 90 || native_rotation == 270 {
            (h, w)
        } else {
            (w, h)
        }
    });
    configure_rgb32(&reader, request, raw_rot, (nw, nh))?;
    // 双保险:变换链组建后再对实现 IMFVideoProcessorControl 的 MFT 显式 SetRotation(NONE)。
    pin_no_xvp_rotation(&reader);
    let geom = output_geometry(&reader)?;

    Ok(Session {
        reader: ManuallyDrop::new(reader),
        cb,
        geom,
        rotation: native_rotation,
        duration_ms,
        display_w,
        display_h,
        _hw: None,
    })
}

/// 强制关闭源 reader 变换链内 XVP 的自动转正(ADVANCED 管线下 XVP 会按 MF_MT_VIDEO_ROTATION
/// 自动旋转,与 CPU 侧 apply_rotation 叠成双旋转 —— rot90 基准片实测抓获;且转正后输出类型的
/// rotation 属性不清零,无法作判据)。枚举链上全部 MFT,对实现 IMFVideoProcessorControl 的
/// 调 SetRotation(ROTATION_NONE)。legacy 管线取不到该接口 = 本就不自动旋转,失败静默。
unsafe fn pin_no_xvp_rotation(reader: &IMFSourceReader) {
    let Ok(ex) = reader.cast::<IMFSourceReaderEx>() else {
        return;
    };
    for idx in 0..8 {
        let mut category = GUID::zeroed();
        let mut transform: Option<IMFTransform> = None;
        if ex
            .GetTransformForStream(FIRST_VIDEO_STREAM, idx, Some(&mut category), &mut transform)
            .is_err()
        {
            break; // 链枚举尽
        }
        if let Some(t) = transform {
            if let Ok(ctrl) = t.cast::<IMFVideoProcessorControl>() {
                let _ = ctrl.SetRotation(ROTATION_NONE);
            }
        }
    }
}

/// 统一的雪碧格尺寸推导（与 request_size 的 CellHeight 分支**必须同算式**，
/// 否则协商输出与目标格差 1px，逐帧空跑一次缩放）。
fn sprite_cell(display_w: u32, display_h: u32, cell_h: u32) -> (u32, u32) {
    let aspect = if display_h > 0 {
        display_w as f32 / display_h as f32
    } else {
        16.0 / 9.0
    };
    ((((cell_h as f32) * aspect).round() as u32).max(1), cell_h)
}

/// 按策略算向 XVP 请求的输出尺寸（**显示坐标系**，正立后）。None = 保持原生。
/// 坐标系与残余旋转的耦合（90/270 时按管线行为换算/重协商）在 `open_session_inner` 处理。
fn request_size(display_w: u32, display_h: u32, policy: &SizePolicy) -> Option<(u32, u32)> {
    if display_w == 0 || display_h == 0 {
        return None;
    }
    let (dw, dh) = (display_w, display_h);
    match *policy {
        SizePolicy::FitLongEdge(max) => {
            let long = dw.max(dh);
            if max == 0 || long <= max {
                return None; // 不上采样：小于目标直接原生直出
            }
            let r = max as f32 / long as f32;
            let mut tw = ((dw as f32 * r).round() as u32).max(1);
            let mut th = ((dh as f32 * r).round() as u32).max(1);
            // 长边钉死为 max，消除浮点回绕(保证 encode 侧 `w<=target && h<=target` 直通)。
            if dw >= dh {
                tw = max;
            } else {
                th = max;
            }
            Some((tw, th))
        }
        SizePolicy::CellHeight(cell_h) => Some(sprite_cell(dw, dh, cell_h)),
    }
}

// ── Async ReadSample:回调交接 + 超时护栏 ─────────────────────────────────────────
// 同步 `IMFSourceReader::ReadSample` 对个别损坏/不支持字节流的容器会死等 MF 内部事件永不
// 返回(MF_E_UNSUPPORTED_BYTESTREAM_TYPE 错误路径丢事件),且僵死 reader 会占住 MF **进程级**
// 共享工作队列并扩散到后续所有 reader,rayon 派生池被逐个占满 → 整条派生流水线挂死。
// 对策:reader 挂 `MF_SOURCE_READER_ASYNC_CALLBACK` 异步回调发起读,结果经 Mutex+Condvar 交回
// 发起线程;`READ_SAMPLE_TIMEOUT` 内无回调即返回结构化 Err,并置位 `timed_out`——`Drop for Session`
// 据此**泄漏隔离**僵死 reader(僵死态下 `Release`/`Drop` 可能同样死等)。

/// 读样本超时。取 30s——远大于任何正常单帧解码(4K HEVC 硬解实测 < 数百 ms),又能在派生池
/// 被逐个拖垮前止损。命中即判定 reader 僵死并弃用。
const READ_SAMPLE_TIMEOUT: Duration = Duration::from_secs(30);

/// 一次 `OnReadSample` 交付的结果。`sample` 已 AddRef(`.cloned()`),回调返回、MF 释放其自身
/// 引用后仍有效。
#[derive(Debug)]
struct ReadOutcome {
    hr: HRESULT,
    stream_flags: u32,
    sample: Option<IMFSample>,
}

// SAFETY(跨线程移动 IMFSample):`OnReadSample` 在 MF 工作队列线程写槽、rayon 派生线程读取并
// 消费,确跨线程。依据:① MF Source Reader 异步回调对象(reader/样本)在 MTA 中为 agile /
// free-threaded——可在任意 MTA 线程调用而无需 marshaling;回调线程与消费线程都经
// `CoInitializeEx(MULTITHREADED)`(init_com)进入同一 MTA。② windows-rs 的 `IMFSample` 仅是
// `NonNull` 指针包装,其 `!Send` 纯因 `NonNull` 保守默认,并非真实线程约束。使用面亦极窄:
// 单写(回调,同一 reader 的 ReadSample 串行、至多一个在途)+ 单读(消费方 `slot.take()` 恰一次),
// 全程经 `slot` 的 `Mutex` 保护建立 happens-before;`sample` 已 AddRef、消费后恰释放一次。
// 仅 `Send` 即足:`Mutex<Option<ReadOutcome>>` 在内层 `Send` 下自带 `Sync`,连带 `CallbackShared`
// 得 `Send + Sync`,`Arc<CallbackShared>` 遂可跨线程(消除 clippy::arc_with_non_send_sync)。
unsafe impl Send for ReadOutcome {}

/// 发起线程与 MF 回调线程之间的样本交接槽。
/// 锁纪律:回调与等待方都只在**持锁瞬间**读写 `slot`,绝不在持锁期间调用任何 COM / 阻塞;
/// 本文件为纯阻塞上下文(无 `.await`),`std::sync::Mutex` 合规。
struct CallbackShared {
    slot: Mutex<Option<ReadOutcome>>,
    ready: Condvar,
    /// 置位 = 曾发生读超时。`Drop for Session` 据此泄漏 reader(避免 `Release` 二次挂死),
    /// 且 `read_frame_at` 据此对已弃用 reader 快速失败,不再发起注定挂死的读。
    timed_out: AtomicBool,
}

impl CallbackShared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            slot: Mutex::new(None),
            ready: Condvar::new(),
            timed_out: AtomicBool::new(false),
        })
    }
}

/// `IMFSourceReaderCallback` 实现:把每次 `OnReadSample` 结果塞入共享槽并唤醒等待线程。
/// `OnFlush`/`OnEvent` 最小实现(seek 经 `SetCurrentPosition` 内部完成、不产生我们关心的事件;
/// 亦不订阅源事件)。
#[implement(IMFSourceReaderCallback)]
struct ReaderCallback {
    shared: Arc<CallbackShared>,
}

impl IMFSourceReaderCallback_Impl for ReaderCallback_Impl {
    fn OnReadSample(
        &self,
        hrstatus: HRESULT,
        _dwstreamindex: u32,
        dwstreamflags: u32,
        _lltimestamp: i64,
        psample: Option<&IMFSample>,
    ) -> windows::core::Result<()> {
        // 只在持锁瞬间写槽,随即释放锁再唤醒 —— 不持锁跨任何调用。
        {
            let mut slot = self.shared.slot.lock().unwrap();
            *slot = Some(ReadOutcome {
                hr: hrstatus,
                stream_flags: dwstreamflags,
                // AddRef 保活:回调返回后 MF 会释放它自己那份引用。
                sample: psample.cloned(),
            });
        }
        self.shared.ready.notify_one();
        Ok(())
    }

    fn OnFlush(&self, _dwstreamindex: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnEvent(
        &self,
        _dwstreamindex: u32,
        _pevent: Option<&IMFMediaEvent>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// 等待 `OnReadSample` 交付结果:命中返回样本;`timeout` 内无回调则置 `timed_out` 并返回结构化
/// Err(信息含 "read sample timeout" 与 seek 流位置,不泄底层原始串)。纯等待逻辑、不碰真实 MF
/// —— 便于单测「事件永不 signal → 超时 Err」(见本文件 tests)。
fn wait_sample_or_timeout(
    shared: &CallbackShared,
    timeout: Duration,
    seek_100ns: i64,
) -> Result<ReadOutcome> {
    let deadline = Instant::now() + timeout;
    let mut slot = shared.slot.lock().unwrap();
    loop {
        if let Some(outcome) = slot.take() {
            return Ok(outcome);
        }
        let now = Instant::now();
        if now >= deadline {
            drop(slot);
            shared.timed_out.store(true, Ordering::Release);
            return Err(AppError::Os(format!(
                "Media Foundation: read sample timeout after {}s (seek={seek_100ns} in 100ns units) | 读样本超时",
                timeout.as_secs()
            )));
        }
        // `wait_timeout` 释放锁并挂起,返回时重新持锁;spurious wakeup 由外层 loop 复检。
        let (next, _) = shared.ready.wait_timeout(slot, deadline - now).unwrap();
        slot = next;
    }
}

// ── Source reader setup ─────────────────────────────────────────────────────────

/// 打开一个挂了异步回调的 SourceReader。返回 reader 与其回调共享态(等待/超时/僵死交接)。
unsafe fn open_reader(
    path: &Path,
    hw: Option<&crate::video::d3d::HwSlot>,
    emit_errors: bool,
) -> Result<(IMFSourceReader, Arc<CallbackShared>)> {
    let url = HSTRING::from(path.as_os_str());
    let mut attrs: Option<IMFAttributes> = None;
    // 属性数(容量提示):ADVANCED + ASYNC_CALLBACK(+ 可选 D3D_MANAGER)。
    MFCreateAttributes(&mut attrs, 3).map_err(|e| mf_err_with_mode(e, emit_errors))?;
    let attrs =
        attrs.ok_or_else(|| AppError::Internal("MFCreateAttributes returned null".into()))?;
    // ADVANCED 处理器（XVP）：任意输入格式 → RGB32，且支持**输出尺寸协商**（缩放在 XVP 内完成，
    // 挂 D3D manager 时上 GPU）。旧式 ENABLE_VIDEO_PROCESSING 只能原生尺寸直出，已弃用。
    attrs
        .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
        .map_err(|e| mf_err_with_mode(e, emit_errors))?;
    // 异步回调:读帧改为「发起 + 回调交付」,配 30s 超时护栏(见 read_frame_at)。reader 会持有
    // 该回调 COM 对象的引用直至自身释放 —— 超时泄漏 reader 时,回调对象随之保活。
    let shared = CallbackShared::new();
    let callback: IMFSourceReaderCallback = ReaderCallback {
        shared: shared.clone(),
    }
    .into();
    attrs
        .SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &callback)
        .map_err(|e| mf_err_with_mode(e, emit_errors))?;
    if let Some(slot) = hw {
        attrs
            .SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, slot.manager())
            .map_err(|e| mf_err_with_mode(e, emit_errors))?;
    }
    let reader =
        MFCreateSourceReaderFromURL(&url, &attrs).map_err(|e| mf_err_with_mode(e, emit_errors))?;
    Ok((reader, shared))
}

/// 只保留首个视频流：反选全部流再单独选回视频，省掉音频解码器初始化与无谓 demux（T1b）。
/// 个别源不支持流选择 —— 失败不致命，照常全选跑。
/// (审查 F-07:第二步失败必须回滚重开全部流——旧实现两步都 `let _`,首步成功+次步失败会留下
/// 「全流禁用」的 reader,后续 ReadSample 必失败,与上行承诺相反。)
unsafe fn select_video_only(reader: &IMFSourceReader) {
    if reader.SetStreamSelection(ALL_STREAMS, false).is_err() {
        // 反选都不支持 → 什么都没改,保持默认全选。
        return;
    }
    if reader.SetStreamSelection(FIRST_VIDEO_STREAM, true).is_err() {
        // 单流选回失败 → 回滚全选,兑现「失败照常全选跑」。回滚自身失败无计可施,
        // 后续 ReadSample 报错走既有错误路径(不比旧行为更差)。
        let _ = reader.SetStreamSelection(ALL_STREAMS, true);
    }
}

/// 将首个视频流输出定为 RGB32（内存中 BGRA），可选地请求 XVP 缩放到 `request`（解码坐标系宽高）。
/// 返回后调用方一律以 `output_geometry`（当前类型实读）为准，故带尺寸协商被拒时静默回退原生。
///
/// `raw_rot`：源流的 `MF_MT_VIDEO_ROTATION` 原值。把它**原样声明在输出类型上** =
/// 「输出仍携带该旋转、由下游（我们的 apply_rotation）补偿」，XVP 据此不自动转内容。
/// `native`：解码坐标系原生宽高。**输出 FRAME_SIZE 恒显式钉死**（无缩放请求时钉原生）——
/// 只声明旋转不钉尺寸时,MF 仍会把默认输出尺寸定成旋后宽高,XVP 于是把未旋内容**信箱式**
/// 塞进转置画幅(四角黑边,rot90 基准片 diag 实证);双属性一起钉才得到未旋、满幅的输出。
unsafe fn configure_rgb32(
    reader: &IMFSourceReader,
    request: Option<(u32, u32)>,
    raw_rot: u32,
    native: (u32, u32),
) -> Result<()> {
    let make_type = |size: (u32, u32)| -> Result<IMFMediaType> {
        let mt = MFCreateMediaType().map_err(mf_err)?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(mf_err)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(mf_err)?;
        if raw_rot != 0 {
            mt.SetUINT32(&MF_MT_VIDEO_ROTATION, raw_rot)
                .map_err(mf_err)?;
        }
        if size.0 > 0 && size.1 > 0 {
            mt.SetUINT64(&MF_MT_FRAME_SIZE, ((size.0 as u64) << 32) | size.1 as u64)
                .map_err(mf_err)?;
        }
        Ok(mt)
    };
    if let Some(req) = request {
        let mt = make_type(req)?;
        if reader
            .SetCurrentMediaType(FIRST_VIDEO_STREAM, None, &mt)
            .is_ok()
        {
            return Ok(());
        }
        // 个别管线/驱动拒绝带尺寸的协商：退回原生尺寸 RGB32，CPU resize 兜底（慢但正确）。
        tracing::debug!(
            "XVP sized negotiation rejected, falling back to native | 尺寸协商被拒，回退原生"
        );
    }
    let mt = make_type(native)?;
    reader
        .SetCurrentMediaType(FIRST_VIDEO_STREAM, None, &mt)
        .map_err(mf_err)?;
    Ok(())
}

/// RGB32 协商后的输出帧几何：宽/高 + 带符号行 stride。
struct Geometry {
    width: u32,
    height: u32,
    /// 带符号 stride：负值 ⇒ 行为 bottom-up。
    stride: i32,
}

unsafe fn output_geometry(reader: &IMFSourceReader) -> Result<Geometry> {
    let mt = reader
        .GetCurrentMediaType(FIRST_VIDEO_STREAM)
        .map_err(mf_err)?;
    let (width, height) = attr_size(&mt, &MF_MT_FRAME_SIZE)
        .ok_or_else(|| AppError::Internal("MF: missing frame size | 缺少帧尺寸".into()))?;
    // 默认 stride 以 u32 存储，但语义为 i32（符号 = 朝向）。
    let stride = mt
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|s| s as i32)
        .unwrap_or((width as i32) * 4);
    Ok(Geometry {
        width,
        height,
        stride,
    })
}

// ── Frame reading ───────────────────────────────────────────────────────────────

/// 跳转到 `t_100ns`（100 纳秒单位）并读取一帧解码后的 RGBA。
///
/// 异步模式:每次读经 `ReadSample`(输出参数全 None)发起,结果由 `OnReadSample` 回调经
/// `shared` 交回,配 `READ_SAMPLE_TIMEOUT` 超时。正常路径行为与旧同步实现等价(含 seek 后的
/// 读帧序列;async 下 `SetCurrentPosition` 仍同步返回,下一次 `ReadSample` 从新位置起始)。
unsafe fn read_frame_at(
    reader: &IMFSourceReader,
    shared: &CallbackShared,
    t_100ns: i64,
    geom: &Geometry,
    path: &Path,
) -> Result<DecodedImage> {
    // 该 reader 已因先前读超时被判僵死:不再发起注定挂死的 `ReadSample`,直接快速失败
    // (否则封面回退读 / 关键帧后续格会各自再等一个 30s 超时)。
    if shared.timed_out.load(Ordering::Acquire) {
        return Err(AppError::Internal(
            "MF: reader abandoned after prior read timeout | reader 已因先前超时弃用".into(),
        ));
    }
    if t_100ns > 0 {
        // GUID_NULL time format = 100-ns reference time. `PROPVARIANT::from(i64)` → VT_I8.
        // GUID_NULL 时间格式 = 100 纳秒参考时间。`PROPVARIANT::from(i64)` → VT_I8。
        //
        // ⚠ 已知边界:超时护栏只覆盖 `ReadSample`。现有挂死栈证据仅指向 `ReadSample`,而
        // `SetCurrentPosition`(seek)走同步返回、不经异步回调,故此处不设护栏——理论上 seek
        // 亦可能死等(至今未观测到)。彻底解归二期 video-worker 进程隔离(F-029):届时整条
        // MF 会话可被父进程外部超时终止,无需逐调用护栏。
        let pos = PROPVARIANT::from(t_100ns);
        let _ = reader.SetCurrentPosition(&GUID::zeroed(), &pos);
    }
    // 读取至多数个样本 —— null 样本（流 tick / 间隙）跳过。
    for _ in 0..16 {
        // 异步发起:async 模式下输出参数必须全 None,结果经 `OnReadSample` 回调交付。
        reader
            .ReadSample(FIRST_VIDEO_STREAM, 0, None, None, None, None)
            .map_err(mf_err)?;
        let outcome = match wait_sample_or_timeout(shared, READ_SAMPLE_TIMEOUT, t_100ns) {
            Ok(o) => o,
            Err(e) => {
                // reader 已置 timed_out(见 wait_sample_or_timeout)。泄漏隔离在 Drop for Session。
                tracing::warn!(
                    path = %path.display(),
                    timeout_secs = READ_SAMPLE_TIMEOUT.as_secs(),
                    "MF ReadSample timed out; abandoning reader to isolate hung MF work queue | \
                     MF 读样本超时,弃用并泄漏 reader 隔离僵死工作队列"
                );
                return Err(e);
            }
        };

        // 回调携带的 HRESULT 先行传播(读失败时 sample 为空,如 MF_E_UNSUPPORTED_BYTESTREAM_TYPE
        // 经回调如实上报而非挂死 —— 走既有错误路径 → 派生行 status=3)。
        outcome.hr.ok().map_err(mf_err)?;
        if (outcome.stream_flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
            return Err(AppError::Internal(
                "MF: end of stream before a frame | 帧前已到流尾".into(),
            ));
        }
        if let Some(sample) = outcome.sample {
            return sample_to_rgba(&sample, geom);
        }
        // null sample → continue reading
    }
    Err(AppError::Internal(
        "MF: no decodable sample | 无可解码样本".into(),
    ))
}

/// Convert an `IMFSample` (RGB32, memory order B,G,R,X) into a top-down RGBA `DecodedImage`,
/// honouring the (possibly negative) stride. On the D3D path this Lock performs the GPU→CPU
/// readback — by then the frame is already XVP-downscaled, so the copy is small.
/// 将 `IMFSample`（RGB32，内存序 B,G,R,X）转换为 top-down 的 RGBA `DecodedImage`，
/// 并尊重（可能为负的）stride。D3D 路径下此处 Lock 即 GPU→CPU readback —— 帧已被 XVP
/// 缩小，拷贝量小。
unsafe fn sample_to_rgba(sample: &IMFSample, geom: &Geometry) -> Result<DecodedImage> {
    let buffer = sample.ConvertToContiguousBuffer().map_err(mf_err)?;
    let mut data: *mut u8 = std::ptr::null_mut();
    let mut cur_len: u32 = 0;
    buffer
        .Lock(&mut data, None, Some(&mut cur_len))
        .map_err(mf_err)?;

    // RAII-ish: always Unlock even on early return.
    let result = (|| {
        if data.is_null() {
            return Err(AppError::Internal("MF: locked null buffer".into()));
        }
        let cur = cur_len as usize;
        // SAFETY: `data` 已校验非空；MF 的 Lock 契约保证 locked 缓冲区自 `data` 起至少
        // `cur` 字节有效。转成切片后下方拷贝走安全索引（守卫保证不越界）。
        let src = std::slice::from_raw_parts(data, cur);
        let out = copy_bgr32_to_rgba(
            src,
            geom.width as usize,
            geom.height as usize,
            geom.stride.unsigned_abs() as usize,
            geom.stride < 0,
        );
        Ok(DecodedImage {
            pixels: out,
            width: geom.width,
            height: geom.height,
            icc: None, // 视频帧无 ICC 来源,按 sRGB 假定
        })
    })();

    let _ = buffer.Unlock();
    result
}

// ── Attribute helpers / frame post-processing ────────────────────────────────
// 迁至 `super::mf_attrs` / `super::frame_post`（见文件顶部 use，超长文件拆分方案 tierB-3）。

/// 将 Windows COM 错误映射为我们的 `AppError`。
fn mf_err(e: windows::core::Error) -> AppError {
    AppError::os(
        "Media Foundation 操作失败 | Media Foundation operation failed",
        e,
    )
}

/// 批量探测的失败由扫描器按格式汇总；不在底层构造 `AppError` 时逐项写错误日志。
fn mf_probe_err(_e: windows::core::Error) -> AppError {
    AppError::Os("Media Foundation 探测失败 | Media Foundation probe failed".into())
}

fn mf_err_with_mode(e: windows::core::Error, emit_errors: bool) -> AppError {
    if emit_errors {
        mf_err(e)
    } else {
        mf_probe_err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::{request_size, sprite_cell, SizePolicy};
    use super::{wait_sample_or_timeout, CallbackShared, ReadOutcome};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    /// 临时诊断(#[ignore],手动跑):VIDEO_DIAG=<path> cargo test -p scrollery --lib \
    ///   video::media_foundation::tests::diag_pipeline_env -- --ignored --nocapture
    /// 打印 native/协商类型的尺寸与 rotation 属性、stride、解码首帧的角点像素,
    /// 用于查 XVP 自动转正/几何错位类问题。
    #[test]
    #[ignore]
    fn diag_pipeline_env() {
        let Some(p) = std::env::var_os("VIDEO_DIAG") else {
            return;
        };
        let path = std::path::PathBuf::from(p);
        super::ensure_mf();
        super::init_com();
        unsafe {
            let (reader, cb) = super::open_reader(&path, None, true).expect("open");
            super::select_video_only(&reader);
            let native = reader
                .GetNativeMediaType(super::FIRST_VIDEO_STREAM, 0)
                .expect("native");
            let raw_rot = native
                .GetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_VIDEO_ROTATION)
                .unwrap_or(999);
            let (nw, nh) = super::attr_size(
                &native,
                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
            )
            .unwrap_or((0, 0));
            println!("native: {nw}x{nh} rot_attr={raw_rot}");

            super::configure_rgb32(
                &reader,
                None,
                if raw_rot == 999 { 0 } else { raw_rot },
                (nw, nh),
            )
            .expect("configure");
            super::pin_no_xvp_rotation(&reader);
            let cur = reader
                .GetCurrentMediaType(super::FIRST_VIDEO_STREAM)
                .expect("current");
            let (cw, ch) = super::attr_size(
                &cur,
                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
            )
            .unwrap_or((0, 0));
            let cur_rot = cur
                .GetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_VIDEO_ROTATION)
                .map(|v| v as i64)
                .unwrap_or(-1);
            let geom = super::output_geometry(&reader).expect("geom");
            println!(
                "negotiated: {cw}x{ch} rot_attr={cur_rot} geom={}x{} stride={}",
                geom.width, geom.height, geom.stride
            );

            let img = super::read_frame_at(&reader, &cb, 10_000_000, &geom, &path).expect("frame");
            let px = |x: u32, y: u32| -> (u8, u8, u8) {
                let i = ((y * img.width + x) * 4) as usize;
                (img.pixels[i], img.pixels[i + 1], img.pixels[i + 2])
            };
            let (w, h) = (img.width, img.height);
            println!(
                "frame {}x{} TL={:?} TR={:?} BL={:?} BR={:?} C={:?}",
                w,
                h,
                px(w / 8, h / 8),
                px(w - 1 - w / 8, h / 8),
                px(w / 8, h - 1 - h / 8),
                px(w - 1 - w / 8, h - 1 - h / 8),
                px(w / 2, h / 2),
            );
        }
    }

    // 🔴 copy_bgr32_to_rgba 尾像素/bottom-up/padding/截断四单测已随函数迁至
    // `video::frame_post::tests`（见超长文件拆分方案 tierB-3）。

    /// 尺寸协商（显示坐标系）：FitLongEdge 不上采样、长边钉死为 max（保证 encode 侧直通）;
    /// CellHeight 与 sprite_cell 同算式（差 1px 都会让逐帧缩放空跑）。
    /// 旋转坐标系换算/重协商在 open_session_inner（依赖 COM,由 bench --rotation-check 钉住）。
    #[test]
    fn request_size_policies() {
        // 横 4K,长边限 512:长边钉 512,短边按比例。
        assert_eq!(
            request_size(3840, 2160, &SizePolicy::FitLongEdge(512)),
            Some((512, 288))
        );
        // 竖幅(显示尺寸已正立):长边=高。
        assert_eq!(
            request_size(1080, 1920, &SizePolicy::FitLongEdge(512)),
            Some((288, 512))
        );
        // 小于目标:不上采样,原生直出。
        assert_eq!(request_size(320, 240, &SizePolicy::FitLongEdge(512)), None);
        // 0 = 不限制(原生)。
        assert_eq!(request_size(3840, 2160, &SizePolicy::FitLongEdge(0)), None);
        // 雪碧格:与 sprite_cell 完全一致。
        assert_eq!(
            request_size(1920, 1080, &SizePolicy::CellHeight(200)),
            Some(sprite_cell(1920, 1080, 200))
        );
        // 小视频雪碧格允许上采样(保证格统一)。
        assert_eq!(
            request_size(160, 120, &SizePolicy::CellHeight(200)),
            Some(sprite_cell(160, 120, 200))
        );
        // 零尺寸防御。
        assert_eq!(request_size(0, 0, &SizePolicy::FitLongEdge(512)), None);
    }

    /// 🔴 超时护栏:模拟「`OnReadSample` 事件永不 signal」——不往交接槽写任何东西,断言在
    /// `timeout` 后返回结构化 Err(含 "read sample timeout" 与 seek 流位置)且置位 `timed_out`
    /// (`Drop for Session` 据此泄漏隔离僵死 reader)。纯等待逻辑,不触碰真实 MF。
    #[test]
    fn wait_times_out_when_callback_never_signals() {
        let shared = CallbackShared::new();
        let err = wait_sample_or_timeout(&shared, Duration::from_millis(40), 1_234_567)
            .expect_err("无信号且超时须返回 Err");
        let msg = format!("{err}");
        assert!(msg.contains("read sample timeout"), "err msg = {msg}");
        assert!(msg.contains("1234567"), "err msg 须含 seek 流位置: {msg}");
        assert!(
            shared.timed_out.load(Ordering::Acquire),
            "超时须置位 timed_out"
        );
    }

    /// 交接槽已有结果时立即返回 Ok、不误判超时、不置 `timed_out`。等待方唤醒后复检 `slot.take()`
    /// 走的正是这条取值路径(此处不构造真 IMFSample,sample=None,只验交接语义)。
    #[test]
    fn wait_returns_outcome_when_slot_filled() {
        let shared = CallbackShared::new();
        {
            let mut slot = shared.slot.lock().unwrap();
            *slot = Some(ReadOutcome {
                hr: windows::core::HRESULT(0),
                stream_flags: 0xABCD,
                sample: None,
            });
        }
        let out = wait_sample_or_timeout(&shared, Duration::from_secs(5), 0)
            .expect("已填充交接槽须返回 Ok");
        assert_eq!(out.stream_flags, 0xABCD);
        assert!(
            !shared.timed_out.load(Ordering::Acquire),
            "命中不应置 timed_out"
        );
    }
}
