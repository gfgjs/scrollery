// src-tauri/src/reader/mod.rs
//! 阅读器文本管线。
//!
//! 纯领域逻辑,不含 IPC/DB 依赖 —— 便于单测且可被 doc_commands 的 spawn_blocking 闭包直接调用:
//!   - [`encoding`]:txt 编码识别与解码。
//!   - [`text_index`]:txt 分章与伪章兜底。
//!   - [`paragraph`]:txt 分段与重排。

pub mod encoding;
pub mod paragraph;
pub mod text_index;
pub mod zh_convert;
