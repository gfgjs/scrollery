// src-tauri/src/exotic/crypto.rs
//! 冷门格式插件的信任根验签原语，复用 `scrollery-exotic-trust`。
//!
//! 本文件退化为**再导出薄壳**，保持既有 `crate::exotic::crypto::{...}` 引用路径不变——
//! installer / registry / package / install / coordinator / exotic_commands 均通过
//! 这些路径复用同一套验签原语。
//!
//! `test_support`（确定性签名/keyset 构造）经 exotic-trust 的 `test-support` feature 暴露，
//! src-tauri 在 `[dev-dependencies]` 启用后，`crate::exotic::crypto::test_support::*` 仍可用。

pub use scrollery_exotic_trust::crypto::*;
