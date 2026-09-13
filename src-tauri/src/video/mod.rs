// src-tauri/src/video/mod.rs
//! Video backend abstraction (§1.4.1 / §3.2) — the capability-trait layer for video
//! probing & frame extraction, plus a tiny runtime registry that picks the best backend
//! for a given extension.
//!
//! This is the first concrete backend trait of the §1.4 plan: rather than landing an empty
//! abstraction in P0, the trait is introduced here together with its first real
//! implementation (`MediaFoundationBackend`, Windows, zero-bundle). FFmpeg (feature `ffmpeg`,
//! Perf-only) and AVFoundation (macOS) slot in later behind the same trait.
//!
//! 视频后端抽象（§1.4.1 / §3.2）—— 视频探测与取帧的能力 trait 层，外加一个极小的运行期注册表，
//! 按扩展名挑选最佳后端。这是 §1.4 计划落地的第一个具体后端 trait：不在 P0 留空抽象，
//! 而是与首个真实实现（`MediaFoundationBackend`，Windows，零捆绑）一并引入。FFmpeg
//! （feature `ffmpeg`，仅 Perf）与 AVFoundation（macOS）后续按同一 trait 接入。

use std::path::Path;

use crate::engine::traits::DecodedImage;
use crate::error::Result;

#[cfg(windows)]
pub mod d3d;
/// MF 解码帧后处理（media_foundation.rs 尾段拆出，零 unsafe 耦合，见超长文件拆分方案 tierB-3）。
#[cfg(windows)]
mod frame_post;
#[cfg(windows)]
pub mod media_foundation;
/// MF 属性/编解码器辅助（media_foundation.rs 尾段拆出，纯 GUID/PROPVARIANT 解析，见
/// 超长文件拆分方案 tierB-3）。
#[cfg(windows)]
mod mf_attrs;
/// 视频格式扩展 · host 侧 Service 型 worker(design.md §2.1;backend_for 接入见下方 §8 V5)。
pub mod worker_service;
// V6 播放链路后端(design.md §5.1/§5.4):判定表 + 独立可播产物缓存池。
pub mod playable_cache;
/// 视频格式扩展 · 播放链路编排内核(design.md §5;超长文件拆分方案 tierB-2):从
/// `ipc/video_commands.rs` 拆出的 resolve/派生/进度私有调用链,命令壳留在原文件。
pub(crate) mod playback_orchestrator;
pub mod playback_policy;
/// 视频格式扩展 · 缩略图后端桥(design.md §1 修正2 / §8 V5):把 `VideoBackend` trait 委托给
/// `worker_service::VideoWorkerService`,供 `backend_for()` 覆盖 MF 不认的容器。
pub mod worker_backend;

/// Cheaply-probed video metadata (no full decode). Dimensions are **display** dimensions —
/// rotation already applied, so `width`/`height` are what the upright frame measures
/// (mirrors how image EXIF orientation swaps w/h). Layout depends on these (§3.2).
/// 廉价探测的视频元数据（不全解码）。宽高为**显示**尺寸 —— 已应用 rotation，
/// 即 `width`/`height` 是正立帧的尺寸（与图片 EXIF orientation 交换宽高同理）。布局强依赖之（§3.2）。
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    /// 旋转元数据（度，0/90/180/270）。仅作记录；上面的显示尺寸已计入。
    pub rotation: i32,
    pub fps: f32,
    /// 平均比特率（比特/秒，未知为 0）。
    pub bitrate: u32,
    pub has_audio: bool,
    /// 简短编解码标签（如 "H264"、"HEVC"），无法识别为 `None`。
    pub codec: Option<String>,
}

/// Video capability backend (§1.4.1). Each implementation handles a set of containers and
/// returns **upright** RGBA frames (rotation applied) so callers never re-handle orientation.
/// 视频能力后端（§1.4.1）。每个实现处理一组容器，返回**正立**的 RGBA 帧（已应用旋转），
/// 调用方无需再处理方向。
pub trait VideoBackend: Send + Sync {
    /// Stable backend id, e.g. "media-foundation" | "ffmpeg".
    /// 稳定后端 id，如 "media-foundation" | "ffmpeg"。
    fn name(&self) -> &'static str;

    /// 本后端是否（很可能）能处理给定的小写扩展名。
    fn can_handle(&self, ext: &str) -> bool;

    /// 在不全解码的情况下探测宽高 / 时长 / 旋转 / 帧率 / 是否含音频。
    fn probe(&self, path: &Path) -> Result<VideoInfo>;

    /// 解码一帧正立封面，长边 ≤ `max_long_edge`（0 = 原生尺寸;绝不上采样）。时间戳由后端
    /// 自选（≈ min(1s, 时长 10%)，含黑帧规避）—— 时长取自同一解码会话，调用方**不要**为选
    /// 时间戳而先行 probe（那会多开一次解码会话）。
    fn cover(&self, path: &Path, max_long_edge: u32) -> Result<DecodedImage>;

    /// 解码 `n` 张正立、等尺寸、跨视频均匀采样的帧，用于悬停/进度条 scrub 雪碧图（§3.3）。
    /// 返回帧共享同一格尺寸 —— 格高固定为 `cell_height`(px,设置键 `sprite_cell_height`,批次C），
    /// 格宽按视频显示比例推导。
    fn keyframes(&self, path: &Path, n: usize, cell_height: u32) -> Result<Vec<DecodedImage>>;
}

/// 批量视频元数据探测的结果状态。`Succeeded` 只表示后端返回了结果，调用方仍应校验
/// 宽高等关键字段后再落库。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoProbeStatus {
    Succeeded,
    Failed,
    Unsupported,
}

/// 批量视频元数据探测的轻量结果：不把底层错误串带出并行热路径，失败原因由调用方按格式
/// 汇总记录；`fallback_attempted` 用于判断 MF 失败后是否实际尝试了 video worker。
#[derive(Debug)]
pub(crate) struct VideoProbeOutcome {
    pub(crate) info: Option<VideoInfo>,
    pub(crate) status: VideoProbeStatus,
    pub(crate) fallback_attempted: bool,
}

/// 批量元数据探测运行时。worker 能力只在一个富化阶段开始时检查一次，避免每个文件重复
/// 查询授权/组件状态；MF 失败时复用同一个 worker 服务实例做回退。
pub(crate) struct VideoProbeRuntime {
    worker: Option<Box<dyn VideoBackend>>,
}

impl VideoProbeRuntime {
    pub(crate) fn new() -> Self {
        Self {
            worker: worker_backend_if_ready(),
        }
    }

    pub(crate) fn has_worker_fallback(&self) -> bool {
        self.worker.is_some()
    }

    /// 先尝试 Windows Media Foundation；若扩展名属于 MF 且运行时探测失败，再尝试已就绪的
    /// video worker。MF 不负责的扩展名直接走 worker；两者都不可用时报告 Unsupported。
    pub(crate) fn probe(&self, ext: &str, path: &Path) -> VideoProbeOutcome {
        let ext = ext.to_ascii_lowercase();
        #[cfg(windows)]
        {
            let mf = media_foundation::MediaFoundationBackend;
            if mf.can_handle(&ext) {
                match mf.probe(path) {
                    Ok(info) => {
                        return VideoProbeOutcome {
                            info: Some(info),
                            status: VideoProbeStatus::Succeeded,
                            fallback_attempted: false,
                        };
                    }
                    Err(_) => return self.probe_worker(path, true),
                }
            }
        }

        let _ = &ext;
        self.probe_worker(path, false)
    }

    fn probe_worker(&self, path: &Path, fallback_attempted: bool) -> VideoProbeOutcome {
        let Some(worker) = self.worker.as_deref() else {
            return VideoProbeOutcome {
                info: None,
                status: if fallback_attempted {
                    VideoProbeStatus::Failed
                } else {
                    VideoProbeStatus::Unsupported
                },
                fallback_attempted,
            };
        };

        match worker.probe(path) {
            Ok(info) => VideoProbeOutcome {
                info: Some(info),
                status: VideoProbeStatus::Succeeded,
                fallback_attempted,
            },
            Err(_) => VideoProbeOutcome {
                info: None,
                status: VideoProbeStatus::Failed,
                fallback_attempted,
            },
        }
    }
}

impl Default for VideoProbeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// 供 setup 期一次性注入 `AppState` 句柄(视频格式扩展子系统 design.md §8 V5)。`backend_for()`
/// 据此判定 video-worker 桥是否可用;未注入(如纯单测环境、非视频格式扩展相关构建路径)时
/// `backend_for` 对 MF 不认的容器恒回退 `None`,与 V5 之前的行为完全一致(零回归)。
/// 仅写一次(晚绑定,与 `AppState::set_exotic_coordinator`/`set_video_worker_service` 同姿态)。
static VIDEO_HOST_STATE: std::sync::OnceLock<std::sync::Arc<crate::state::AppState>> =
    std::sync::OnceLock::new();

/// 绑定 `AppState`(setup 内调用)。
pub fn bind_app_state(state: std::sync::Arc<crate::state::AppState>) {
    let _ = VIDEO_HOST_STATE.set(state);
}

/// video-worker 桥的核心门控判据(抽成纯函数供单测:构造完整 `AppState`/`ExoticHost` 成本高,
/// 而 `backend_for` 里这行 `authorized && ffmpeg_ready` 正是「插件不可用→None」这条红线的
/// 全部逻辑——单测直接钉住该布尔表达式,和 `backend_for` 内联调用保持逐字一致)。
fn video_bridge_gate_open(authorized: bool, ffmpeg_ready: bool) -> bool {
    authorized && ffmpeg_ready
}

fn worker_backend_if_ready() -> Option<Box<dyn VideoBackend>> {
    // video-worker 桥(design.md §1 修正2):MF 不认的容器落到这里。三重门控——
    // ① AppState 已绑定(setup 已跑);② 授权折叠为 Authorized(builtin+free 直放行既有语义);
    // ③ FFmpeg 组件已就绪(纯文件系统检查,不下载/不 spawn)。任一不满足 → None(今天的行为)。
    if let Some(state) = VIDEO_HOST_STATE.get() {
        use crate::exotic::catalog::Capability;
        use crate::exotic::coordinator::VIDEO_PLUGIN_ID;
        use crate::exotic::tools::{ffmpeg_tool_status, ToolStatus};

        // ffmpeg 就绪判定纯 fs 检查,先查;is_task_runnable 内含 SQLite 读,惰性置后 ——
        // 常见路径(ffmpeg 未就绪)不必再打一次数据库(V5 深审批4)。
        let ffmpeg_ready = matches!(
            ffmpeg_tool_status(&state.app_data_dir),
            ToolStatus::Ready { .. }
        );
        if !ffmpeg_ready {
            return None;
        }
        let authorized = state
            .exotic_host()
            .is_task_runnable(VIDEO_PLUGIN_ID, Capability::Thumbnail);
        if video_bridge_gate_open(authorized, ffmpeg_ready) {
            if let Some(svc) = state.video_worker_service.get() {
                return Some(Box::new(worker_backend::WorkerVideoBackend::new(
                    std::sync::Arc::clone(svc),
                )));
            }
        }
    }
    None
}

/// Pick the best available backend for a lowercase extension (§1.4.3). Windows 下先试 Media
/// Foundation;MF 不认的容器(mkv/webm/flv/ogv 等)再试 video-worker 桥(§1 修正2 / §8 V5)——
/// 判定「便宜」:只做状态查询(catalog/授权折叠 + FFmpeg 就绪的纯文件系统检查),**不 spawn**
/// 任何进程;真正的 worker 子进程按需惰性起于首次实际派活(`VideoWorkerService::ensure_session`,
/// 位于 `worker_service.rs`,本函数不重复该判定)。无匹配时调用方标记 `unsupported`。
/// 为小写扩展名挑选最佳可用后端（§1.4.3）。
pub fn backend_for(ext: &str) -> Option<Box<dyn VideoBackend>> {
    let ext = ext.to_ascii_lowercase();
    #[cfg(windows)]
    {
        let mf = media_foundation::MediaFoundationBackend;
        if mf.can_handle(&ext) {
            return Some(Box::new(mf));
        }
    }
    let _ = &ext;
    worker_backend_if_ready()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 门控核心判据:插件不可用(未授权)或 FFmpeg 未就绪,任一为假 → 关闭(`backend_for` 回退
    /// `None`,今天的行为)。两者皆真才开放 video-worker 桥。
    #[test]
    fn video_bridge_gate_requires_both_authorized_and_ffmpeg_ready() {
        assert!(!video_bridge_gate_open(false, false));
        assert!(!video_bridge_gate_open(false, true), "插件不可用 → 门关");
        assert!(!video_bridge_gate_open(true, false), "FFmpeg 未就绪 → 门关");
        assert!(video_bridge_gate_open(true, true));
    }

    /// 未绑定 `AppState`(本单测二进制内没有任何测试调用过 `bind_app_state`)时,`backend_for`
    /// 对 MF 不认的扩展名恒回退 `None` —— V5 之前的既有行为零回归。
    #[test]
    fn backend_for_falls_back_to_none_without_app_state_binding() {
        assert!(backend_for("mkv").is_none());
        assert!(backend_for("webm").is_none());
    }
}
