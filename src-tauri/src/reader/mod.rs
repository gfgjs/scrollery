// src-tauri/src/reader/mod.rs
//! 阅读器文本管线（阅读器完善方案 R1）。
//!
//! 纯领域逻辑,不含 IPC/DB 依赖 —— 便于单测且可被 doc_commands 的 spawn_blocking 闭包直接调用:
//!   - [`encoding`]:txt 编码自动识别 + 解码 seam(修 GBK 三链路缺陷,§5.1)。
//!   - [`text_index`]:txt 智能分章(规则集 + 选规则 + 伪章兜底,§5.2)。
//!   - [`paragraph`]:txt 智能分段(一级保守 / 二级重排,§5.2)。

pub mod encoding;
pub mod paragraph;
pub mod text_index;
pub mod zh_convert;
