// crates/scrollery-ai-core/src/lib.rs
//! Scrollery · AI 推理核心。
//!
//! 提供 ORT 会话、CLIP、人脸、OCR、增强推理及模型契约；调度、数据库、缓存与下载由主进程负责。
//!
//! 消费方:
//!   - `ai-worker` 与 `enhance-worker` 启用默认特性，使用各自的推理模块。
//!   - `src-tauri` 关闭默认特性，只使用模型契约、几何与错误类型。
//!
//! 错误使用 [`error::AiError`]，主进程映射为 AppError。

// inference 在模块级门控，主进程关闭该特性以免链接 ORT 与 tokenizers。

#[cfg(feature = "inference")]
pub mod clip;
pub mod decoded;
pub mod embedding;
#[cfg(feature = "inference")]
pub mod engine;
pub mod enhance_profile;
// enhance 内部仅门控推理，类型、错误与分块几何始终可用。
pub mod enhance;
pub mod error;
#[cfg(feature = "inference")]
pub mod face;
pub mod face_profile;
pub mod face_types;
#[cfg(feature = "inference")]
pub mod ocr;
pub mod ocr_profile;
pub mod profile;
#[cfg(feature = "inference")]
pub mod provider;

pub use decoded::DecodedImage;
pub use error::{AiError, Result};
