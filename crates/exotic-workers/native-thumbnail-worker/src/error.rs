//! worker 内部的图像领域错误；跨进程只输出共享协议中的 NativeFailure。

#[derive(Debug, thiserror::Error)]
pub(crate) enum WorkerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image engine error: {0}")]
    Engine(#[from] image::ImageError),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("OS error: {0}")]
    Os(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("edit error [{code}]: {message}")]
    Edit { code: &'static str, message: String },
}

impl WorkerError {
    pub(crate) fn os(msg: impl Into<String>, source: impl std::fmt::Display) -> Self {
        let msg = msg.into();
        tracing::error!("{msg} | {source}");
        Self::Os(msg)
    }
}

// 同一份图像源码在两种进程中解析到各自的错误域，不引入应用或数据库依赖。
pub(crate) use WorkerError as AppError;
pub(crate) type Result<T> = std::result::Result<T, WorkerError>;
