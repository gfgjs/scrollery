//! 视频探测与取帧的后端接口，按扩展名选择 Windows Media Foundation 或已就绪的视频 worker。

use std::path::Path;

use crate::engine::traits::DecodedImage;
use crate::error::Result;

#[cfg(windows)]
pub mod d3d;
/// MF 解码帧后处理。
#[cfg(windows)]
mod frame_post;
#[cfg(windows)]
pub mod media_foundation;
/// MF 属性与编解码器辅助。
#[cfg(windows)]
mod mf_attrs;
/// 视频 worker 的宿主侧服务。
pub mod worker_service;
// 播放链路的判定表与独立可播产物缓存池。
pub mod playable_cache;
/// 播放链路的解析、派生与进度编排；命令入口位于 `ipc/video_commands.rs`。
pub(crate) mod playback_orchestrator;
pub mod playback_policy;
/// 缩略图后端桥：把 `VideoBackend` trait 委托给
/// `worker_service::VideoWorkerService`,供 `backend_for()` 覆盖 MF 不认的容器。
pub mod worker_backend;

/// 廉价探测的视频元数据（不全解码）。宽高为**显示**尺寸 —— 已应用 rotation，
/// 即 `width`/`height` 是正立帧的尺寸。
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

/// 视频能力后端。每个实现处理一组容器，返回**正立**的 RGBA 帧（已应用旋转），
/// 调用方无需再处理方向。
pub trait VideoBackend: Send + Sync {
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

/// setup 期一次性注入 `AppState`；未绑定时，`backend_for` 无法启用 video worker 桥。
static VIDEO_HOST_STATE: std::sync::OnceLock<std::sync::Arc<crate::state::AppState>> =
    std::sync::OnceLock::new();

/// 绑定 `AppState`(setup 内调用)。
pub fn bind_app_state(state: std::sync::Arc<crate::state::AppState>) {
    let _ = VIDEO_HOST_STATE.set(state);
}

/// 只有授权且 FFmpeg 组件就绪时才启用 video worker 桥。
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

/// 为扩展名挑选后端：Windows 优先 Media Foundation，其余尝试已授权且 FFmpeg 就绪的 worker。
/// 这里仅查询可用性；worker 在首次派活时启动。
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
