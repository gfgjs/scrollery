// crates/scrollery-ai-core/src/error.rs
//! AI 核心错误类型(T15 迁出时自 src-tauri AppError 拆分的最小子集)。
//!
//! 变体与原 AppError 使用面一一对应:`Ort` ← AppError::Ai、`Internal` ← AppError::Internal、
//! `Tokenizer` ← AppError::AiTokenizer;src-tauri 侧以 `From<AiError> for AppError` 收敛回去,
//! 调用点的 `?` 传播语义不变。

use thiserror::Error;

/// AI 推理核心统一错误。
///
/// `non_exhaustive`:变体集随 `inference` feature 变化(Ort 臂仅推理面存在),
/// 且 workspace 构建会因 feature 并集把 Ort 臂带回关默认特性的消费方——外部
/// crate 一律写通配臂,两种构建形态下都无 unreachable/non-exhaustive 警告。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AiError {
    /// ONNX Runtime 错误(Session 构建/推理)。仅 `inference` feature 下存在——
    /// 纯契约面构建(T16 后的 src-tauri)不链接 ort,自然没有此错误来源。
    #[cfg(feature = "inference")]
    #[error("ONNX Runtime error: {0}")]
    Ort(#[from] ort::Error),

    /// 内部不变量/数据形状错误(张量越界、池断开、变换奇异等)。
    #[error("{0}")]
    Internal(String),

    /// 分词器错误(词表加载/编码失败;含错误词表防护)。
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),

    /// OCR 域错误(字典/模型契约错配、几何奇异、会话缺失等)。
    /// 纯契约面亦可构造(不依赖 ort),故不门控。
    #[error("OCR error: {0}")]
    Ocr(String),
}

/// 本 crate 统一 Result。
pub type Result<T> = std::result::Result<T, AiError>;
