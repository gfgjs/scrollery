// crates/exotic-workers/video-worker/src/error.rs
//! worker 内部结构化错误(thiserror)+ 到协议稳定错误码 [`WorkerErrorCode`] 的映射
//! (视频格式扩展子系统 design.md §2.3 错误映射)。
//!
//! # 红线
//! - 诊断 `message` **不得**含完整绝对路径(协议红线 `message.rs`);ffmpeg stderr 明细
//!   只进 worker 侧日志行(WorkerLogLine),不进 `FailureBody.message`。
//! - 映射逐变体显式,禁 catch-all 泛化(结尾无 `_` 兜底:新增变体须编译期补齐)。

use exotic_protocol::{FailureBody, Frame, FrameType, WorkerErrorCode};

/// worker 内部错误。每个变体承载**用户可见诊断**(不含绝对路径/stderr 原文)。
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// ffmpeg 路径缺失/sha256 不符/`-version` 起不来/configuration 含 `--enable-gpl`
    /// → terminal(§3.4 许可运行时保险丝),host 收到即标记工具待重下载。
    #[error("ffmpeg 不可用:{0}")]
    FfmpegUnavailable(String),
    /// codec/编码器不支持(ffmpeg stderr 判别)→ terminal,等源/工具变化再失效。
    #[error("不支持的编码变体:{0}")]
    Unsupported(String),
    /// 源损坏/probe 失败/0 字节截断 → terminal,重试无意义。
    #[error("输入畸形:{0}")]
    Malformed(String),
    /// 盘满(os error 112/ENOSPC)/产物超限 → terminal。
    #[error("资源上限:{0}")]
    ResourceLimit(String),
    /// 暂时性 IO(文件占用/读写失败)→ retryable。
    #[error("IO 错误:{0}")]
    Io(String),
    /// 其余非零退出无法归因 / worker 内部 → retryable(预算由 host 控)。
    #[error("内部错误:{0}")]
    Internal(String),
    /// 无 session / session 未知 → retryable:host 重发 VideoSessionInit 后重派。
    #[error("视频会话未加载或已卸载")]
    SessionExpired,
    /// 任务被取消(host kill / 取消轮询命中):清 tmp 后回执。无专属错误码,归 IO(retryable)。
    #[error("任务已取消")]
    Cancelled,
}

impl VideoError {
    /// 到协议稳定错误码的**逐变体**显式映射(无 catch-all)。
    pub fn code(&self) -> WorkerErrorCode {
        match self {
            VideoError::FfmpegUnavailable(_) => WorkerErrorCode::FfmpegUnavailable,
            VideoError::Unsupported(_) => WorkerErrorCode::UnsupportedVariant,
            VideoError::Malformed(_) => WorkerErrorCode::MalformedInput,
            VideoError::ResourceLimit(_) => WorkerErrorCode::ResourceLimit,
            VideoError::Io(_) => WorkerErrorCode::IoError,
            VideoError::Internal(_) => WorkerErrorCode::InternalError,
            VideoError::SessionExpired => WorkerErrorCode::SessionExpired,
            VideoError::Cancelled => WorkerErrorCode::IoError,
        }
    }

    /// retryable 语义:多数随错误码默认;Cancelled 显式 retryable(host 可重派)。
    pub fn retryable(&self) -> bool {
        match self {
            VideoError::Cancelled => true,
            other => other.code().default_retryable(),
        }
    }

    /// 该错误是否**源侧终态**(源损坏/不支持):transcode 编码器阶梯遇之立即中止,
    /// 不再试下一枚编码器(换编码器对同一坏源/不可解 codec 无济于事)。
    pub fn is_source_terminal(&self) -> bool {
        matches!(self, VideoError::Malformed(_) | VideoError::Unsupported(_))
    }

    /// 构造 Failure 帧(会话/单文件 op 无单项 item 语义,item_id/fingerprint 恒 None)。
    pub fn to_failure_frame(&self, request_id: u64) -> Frame {
        let fail = FailureBody {
            item_id: None,
            input_fingerprint: None,
            code: self.code(),
            retryable: self.retryable(),
            message: self.to_string(),
        };
        Frame::control(FrameType::Failure, request_id, &fail).unwrap()
    }
}

/// `std::io::Error` → VideoError:盘满归 ResourceLimit,其余归 Io(retryable)。
impl From<std::io::Error> for VideoError {
    fn from(e: std::io::Error) -> Self {
        // Windows: ERROR_DISK_FULL = 112;类 Unix: ENOSPC = 28。两者 raw_os_error 直判。
        if matches!(e.raw_os_error(), Some(112) | Some(28)) {
            VideoError::ResourceLimit(format!("磁盘空间不足:{}", e.kind()))
        } else {
            VideoError::Io(format!("{}", e.kind()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_mapping_is_stable() {
        assert_eq!(
            VideoError::FfmpegUnavailable("x".into()).code(),
            WorkerErrorCode::FfmpegUnavailable
        );
        assert_eq!(
            VideoError::Unsupported("x".into()).code(),
            WorkerErrorCode::UnsupportedVariant
        );
        assert_eq!(
            VideoError::Malformed("x".into()).code(),
            WorkerErrorCode::MalformedInput
        );
        assert_eq!(
            VideoError::ResourceLimit("x".into()).code(),
            WorkerErrorCode::ResourceLimit
        );
        assert_eq!(VideoError::Io("x".into()).code(), WorkerErrorCode::IoError);
        assert_eq!(
            VideoError::Internal("x".into()).code(),
            WorkerErrorCode::InternalError
        );
        assert_eq!(
            VideoError::SessionExpired.code(),
            WorkerErrorCode::SessionExpired
        );
    }

    #[test]
    fn source_terminal_gates_ladder_fallthrough() {
        assert!(VideoError::Malformed("x".into()).is_source_terminal());
        assert!(VideoError::Unsupported("x".into()).is_source_terminal());
        // 内部错误 = 可能是本编码器不可用 → 允许试下一枚。
        assert!(!VideoError::Internal("x".into()).is_source_terminal());
    }

    #[test]
    fn enospc_maps_to_resource_limit() {
        let e = std::io::Error::from_raw_os_error(112);
        assert_eq!(VideoError::from(e).code(), WorkerErrorCode::ResourceLimit);
        let e2 = std::io::Error::from_raw_os_error(28);
        assert_eq!(VideoError::from(e2).code(), WorkerErrorCode::ResourceLimit);
    }
}
