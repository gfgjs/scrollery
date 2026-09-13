// src-tauri/src/video/worker_service/types.rs
//! 视频服务纯类型/枚举 + 结果映射(自 worker_service.rs 结构性拆分,tierB-1)。

use std::sync::Arc;
use std::time::Duration;

use exotic_protocol::{
    RequestBody, VideoFramesInfo, VideoFramesMode, VideoOutInfo, VideoProbeInfo, WorkerErrorCode,
};

use crate::error::AppError;
use crate::exotic::worker::{RawOutcome, PROGRESS_TOTAL_CAP};

/// per-op 超时表(design.md §2.4)。`timeout` 均为**静默限时**(收 Progress 即重置,不发
/// Progress 的 op 即事实总限时);`total_cap` 为不可重置总上界(transcode 专属放宽)。
pub(super) mod op_timeout {
    use std::time::Duration;
    /// VideoSessionInit(总限时,不发 Progress)。
    pub const SESSION_INIT: Duration = Duration::from_secs(30);
    /// VideoProbe(ffprobe 秒级,总限时)。
    pub const PROBE: Duration = Duration::from_secs(30);
    /// VideoFrames(封面/雪碧图,总限时)。
    pub const FRAMES: Duration = Duration::from_secs(120);
    /// VideoRemux 静默限时(ffmpeg `-progress` 每 ≤1s 出数,60s 无 Progress = 真卡死)。
    pub const REMUX_SILENCE: Duration = Duration::from_secs(60);
    /// VideoTranscode 静默限时(同 remux)。
    pub const TRANSCODE_SILENCE: Duration = Duration::from_secs(60);
    /// transcode 总上界下限(慢机 4h 影片,3600s 缺省不够,§2.4)。
    pub const TRANSCODE_CAP_MIN: Duration = Duration::from_secs(2 * 3600);
    /// transcode 总上界上限。
    pub const TRANSCODE_CAP_MAX: Duration = Duration::from_secs(12 * 3600);
    /// remux 总上界下限(全量 IO 拷贝 + faststart 二遍重写,慢盘几十 GB 源可超 1h,§2.4 深审 V4-1)。
    pub const REMUX_CAP_MIN: Duration = Duration::from_secs(3600);
    /// remux 总上界上限(同 transcode)。
    pub const REMUX_CAP_MAX: Duration = Duration::from_secs(12 * 3600);
    /// probe 时长不可得时的源文件吞吐估算兜底(慢盘量级,保守取值:构成 remux 总上界下限估计,
    /// 而非实际吞吐上限——宁可估多、不可估少导致假超时)。
    const REMUX_BYTES_PER_SEC: u64 = 25 * 1024 * 1024;

    /// transcode 总上界 = `max(2h, 探测时长 × 6)`,上限 12h(design.md §2.4)。
    pub fn transcode_total_cap(probe_duration_ms: Option<u64>) -> Duration {
        let by_dur = probe_duration_ms
            .map(|ms| Duration::from_millis(ms).saturating_mul(6))
            .unwrap_or(TRANSCODE_CAP_MIN);
        by_dur.max(TRANSCODE_CAP_MIN).min(TRANSCODE_CAP_MAX)
    }

    /// remux 总上界 = `max(1h, 探测时长 × 6)`,上限 12h;probe 时长不可得时按源文件字节数
    /// 估算兜底 `source_bytes / 25MB·s`(remux 是全量 IO 拷贝 + faststart 二遍重写,慢盘几十 GB
    /// 源单程可超 1h,3600s 缺省不够,§2.4 深审 V4-1)。
    pub fn remux_total_cap(probe_duration_ms: Option<u64>, source_bytes: Option<u64>) -> Duration {
        let estimate = probe_duration_ms
            .map(|ms| Duration::from_millis(ms).saturating_mul(6))
            .or_else(|| source_bytes.map(|b| Duration::from_secs(b / REMUX_BYTES_PER_SEC)))
            .unwrap_or(REMUX_CAP_MIN);
        estimate.max(REMUX_CAP_MIN).min(REMUX_CAP_MAX)
    }
}

/// 调度优先级:交互(播放)恒抢占背景(封面/雪碧图)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPriority {
    Interactive,
    Background,
}

/// 去重维度(§9.12:同 `(item, kind)` 在途挂载同一任务)。remux 与 transcode 同属
/// `Playable`(design.md §5.2 同一派生 kind `video_playable`),host 对同一 item 只择一策略。
///
/// `ProbeVerify`(§V6-9 产物级深检 dedup 隔离):video_commands.rs 的产物 rename 前复探与源
/// `Probe` 语义相同(仍是一次 VideoProbe 请求),但**去重键**独立——不与源 probe 共
/// `(item_id, VideoKind::Probe)` 键,避免并发窗口内两者互相挂靠/误取消。仅经
/// [`VideoWorkerService::probe_verify`] 使用,不出现在任何 [`VideoOp`] 变体映射中
/// (`VideoOp::kind()` 恒不产出此值,故其余处的穷尽匹配只需补空臂)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoKind {
    Probe,
    Playable,
    Cover,
    Keyframes,
    ProbeVerify,
}

/// 一次 op 的输入(exotic-protocol 类型;paths/指纹为 String)。`Clone` 供 driver 在锁外取出后运行。
#[derive(Debug, Clone)]
pub enum VideoOp {
    Probe {
        source_path: String,
        input_fingerprint: String,
    },
    Remux {
        source_path: String,
        output_tmp_path: String,
        audio_transcode: bool,
        audio_track_index: Option<u32>,
        /// host 侧探测时长(仅用于算 remux 总上界,不下发 worker;§2.4 深审 V4-1)。
        probe_duration_ms: Option<u64>,
    },
    Transcode {
        source_path: String,
        output_tmp_path: String,
        encoder_ladder: Vec<String>,
        crf: Option<u8>,
        bitrate_kbps: Option<u32>,
        max_long_edge: Option<u32>,
        audio_track_index: Option<u32>,
        hw_decode: bool,
        /// host 侧探测时长(仅用于算 transcode 总上界,不下发 worker)。
        probe_duration_ms: Option<u64>,
    },
    Frames {
        source_path: String,
        input_fingerprint: String,
        mode: VideoFramesMode,
    },
}

impl VideoOp {
    pub(super) fn kind(&self) -> VideoKind {
        match self {
            VideoOp::Probe { .. } => VideoKind::Probe,
            VideoOp::Remux { .. } | VideoOp::Transcode { .. } => VideoKind::Playable,
            VideoOp::Frames { mode, .. } => match mode {
                VideoFramesMode::Cover { .. } => VideoKind::Cover,
                VideoFramesMode::Keyframes { .. } => VideoKind::Keyframes,
            },
        }
    }

    /// 静默限时(design.md §2.4 超时表)。
    pub(super) fn silence_timeout(&self) -> Duration {
        match self {
            VideoOp::Probe { .. } => op_timeout::PROBE,
            VideoOp::Remux { .. } => op_timeout::REMUX_SILENCE,
            VideoOp::Transcode { .. } => op_timeout::TRANSCODE_SILENCE,
            VideoOp::Frames { .. } => op_timeout::FRAMES,
        }
    }

    /// 总上界:transcode 走 `max(2h, 时长×6)`、remux 走 `max(1h, 时长×6)`(probe 时长不可得
    /// 时按源文件字节估算兜底,§2.4 深审 V4-1);其余 op 沿用缺省 `PROGRESS_TOTAL_CAP`(3600s)。
    pub(super) fn total_cap(&self) -> Duration {
        match self {
            VideoOp::Transcode {
                probe_duration_ms, ..
            } => op_timeout::transcode_total_cap(*probe_duration_ms),
            VideoOp::Remux {
                source_path,
                probe_duration_ms,
                ..
            } => {
                // probe 时长缺省时的兜底:源文件字节数(远端/慢盘元数据读失败则回落纯时长下限)。
                let source_bytes = std::fs::metadata(source_path).ok().map(|m| m.len());
                op_timeout::remux_total_cap(*probe_duration_ms, source_bytes)
            }
            _ => PROGRESS_TOTAL_CAP,
        }
    }

    pub(super) fn to_request(&self, session_id: u64) -> RequestBody {
        match self.clone() {
            VideoOp::Probe {
                source_path,
                input_fingerprint,
            } => RequestBody::VideoProbe {
                session_id,
                source_path,
                input_fingerprint,
            },
            VideoOp::Remux {
                source_path,
                output_tmp_path,
                audio_transcode,
                audio_track_index,
                probe_duration_ms: _,
            } => RequestBody::VideoRemux {
                session_id,
                source_path,
                output_tmp_path,
                audio_transcode,
                audio_track_index,
            },
            VideoOp::Transcode {
                source_path,
                output_tmp_path,
                encoder_ladder,
                crf,
                bitrate_kbps,
                max_long_edge,
                audio_track_index,
                hw_decode,
                probe_duration_ms: _,
            } => RequestBody::VideoTranscode {
                session_id,
                source_path,
                output_tmp_path,
                encoder_ladder,
                crf,
                bitrate_kbps,
                max_long_edge,
                audio_track_index,
                hw_decode,
            },
            VideoOp::Frames {
                source_path,
                input_fingerprint,
                mode,
            } => RequestBody::VideoFrames {
                session_id,
                source_path,
                input_fingerprint,
                mode,
            },
        }
    }
}

/// op 成功产物(经 [`VideoWorkerService`] 公开方法解包为具体类型)。
#[derive(Debug, PartialEq)]
pub enum VideoOutput {
    Probe(VideoProbeInfo),
    Out(VideoOutInfo),
    /// Frames:blob 用 `Arc` 承载供多 waiter(去重)零拷贝共享;`info` 仅 Keyframes 填。
    Frames {
        blob: Arc<Vec<u8>>,
        info: Option<VideoFramesInfo>,
    },
}

/// host 侧视频服务错误(thiserror,稳定 code 不漏内部串)。映射进既有 [`AppError::Exotic`] 链:
/// `code` 以 `video_` 前缀原样透到 IPC,ffmpeg stderr / worker 内部串只进 tracing、不入错误载荷。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VideoServiceError {
    #[error("视频扩展未授权")]
    NotAuthorized,
    #[error("FFmpeg 组件未就绪,需下载")]
    NeedsComponent,
    #[error("video-worker 不可用")]
    WorkerUnavailable,
    #[error("视频会话初始化失败")]
    SessionInitFailed,
    #[error("不支持的视频变体")]
    UnsupportedVariant,
    #[error("源文件损坏")]
    MalformedSource,
    #[error("资源超限")]
    ResourceLimit,
    #[error("处理超时")]
    Timeout,
    #[error("worker 通路失败")]
    WorkerFailed,
    #[error("FFmpeg 不可用")]
    FfmpegUnavailable,
    #[error("已取消")]
    Cancelled,
}

impl VideoServiceError {
    /// 稳定分流 code(原样透到 IPC `code` 字段)。
    pub fn code(&self) -> &'static str {
        match self {
            VideoServiceError::NotAuthorized => "video_unauthorized",
            VideoServiceError::NeedsComponent => "video_needs_component",
            VideoServiceError::WorkerUnavailable => "video_worker_unavailable",
            VideoServiceError::SessionInitFailed => "video_session_init_failed",
            VideoServiceError::UnsupportedVariant => "video_unsupported_variant",
            VideoServiceError::MalformedSource => "video_malformed_source",
            VideoServiceError::ResourceLimit => "video_resource_limit",
            VideoServiceError::Timeout => "video_timeout",
            VideoServiceError::WorkerFailed => "video_worker_failed",
            VideoServiceError::FfmpegUnavailable => "video_ffmpeg_unavailable",
            VideoServiceError::Cancelled => "video_cancelled",
        }
    }

    /// 是否值得 respawn 后重试一次:仅「worker 通路瞬时失败」(genuine Disconnected/协议违例)。
    /// needs_component/unauthorized/unsupported/malformed/timeout 等确定性/终态失败不重试。
    pub(super) fn retryable(&self) -> bool {
        matches!(self, VideoServiceError::WorkerFailed)
    }
}

impl From<VideoServiceError> for AppError {
    fn from(e: VideoServiceError) -> Self {
        // 复用 exotic 子系统错误变体:code 稳定透出、message 为可安全展示的中文文案(无路径/内部串)。
        AppError::Exotic {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

/// worker 侧原始应答 → host 结果(host 不信任 worker:按 op kind 取对应应答体,缺失即通路失败)。
/// ffmpeg stderr / worker message **只进 tracing**,不进 [`VideoServiceError`](§6 泄漏面红线)。
pub(super) fn map_outcome(
    op: &VideoOp,
    outcome: RawOutcome,
) -> Result<VideoOutput, VideoServiceError> {
    match outcome {
        RawOutcome::Success { body, blob } => match op.kind() {
            // ProbeVerify 只是去重键、从不由 `VideoOp::kind()` 产出(见该变体注释),此臂为
            // 穷尽匹配占位,实际不可达。
            VideoKind::Probe | VideoKind::ProbeVerify => body
                .video_probe
                .map(VideoOutput::Probe)
                .ok_or(VideoServiceError::WorkerFailed),
            VideoKind::Playable => body
                .video_out
                .map(VideoOutput::Out)
                .ok_or(VideoServiceError::WorkerFailed),
            // V4 只做传输:blob(WebP)的独立解码复核 + 雪碧条切格归 V5 缩略图后端桥。
            VideoKind::Cover | VideoKind::Keyframes => Ok(VideoOutput::Frames {
                blob: Arc::new(blob),
                info: body.video_frames,
            }),
        },
        RawOutcome::Failure(fb) => {
            tracing::warn!("video-worker 显式失败 code={}", fb.code.as_str());
            Err(map_worker_code(fb.code))
        }
        RawOutcome::TimedOut => Err(VideoServiceError::Timeout),
        RawOutcome::Disconnected => Err(VideoServiceError::WorkerFailed),
        RawOutcome::Protocol(p) => {
            tracing::warn!("video-worker 协议违例:{p}");
            Err(VideoServiceError::WorkerFailed)
        }
    }
}

fn map_worker_code(code: WorkerErrorCode) -> VideoServiceError {
    match code {
        WorkerErrorCode::FfmpegUnavailable => VideoServiceError::FfmpegUnavailable,
        WorkerErrorCode::UnsupportedVariant => VideoServiceError::UnsupportedVariant,
        WorkerErrorCode::MalformedInput => VideoServiceError::MalformedSource,
        WorkerErrorCode::ResourceLimit => VideoServiceError::ResourceLimit,
        _ => VideoServiceError::WorkerFailed,
    }
}
