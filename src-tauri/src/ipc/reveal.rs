//! 「在系统文件管理器中显示」的共用入口，调用 opener 并将失败映射为稳定错误码。
//! opener 等待系统打开器的结果，调用方须处理实际返回的错误。

use std::path::PathBuf;

use crate::error::{AppError, Result};

/// 在系统文件管理器中显示 `path`（选中该项）。
///
/// 阻塞 IO（Windows 上还要 `CoInitialize` + Shell COM 调用），故进 `spawn_blocking`。
///
/// **不做路径校验** —— 调用方负责在此之前确立 `path` 的可信性（`tree_commands` 走
/// `resolve_within_root` 校验 WebView 传入的 `rel_path`；`system_commands` 的路径由
/// DB 行拼出，不经 WebView）。
pub(crate) async fn reveal_path(path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        tauri_plugin_opener::reveal_item_in_dir(&path).map_err(map_reveal_err)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 把 opener 的错误映射为**稳定 IPC code**（R-08：原先稳定子码寄生在 `PathResolution` 的
/// message 里，序列化 code 恒为 "PathResolution"，前端分流只能匹配文案）。
///
/// 有意不透传原始错误：Tauri IPC 错误不得泄漏内部路径/原始系统错误（项目硬约束）。
/// `unsupported_platform` 单列，供前端显示对应的失败提示。
pub(crate) fn map_reveal_err(e: tauri_plugin_opener::Error) -> AppError {
    match e {
        tauri_plugin_opener::Error::UnsupportedPlatform => AppError::Reveal {
            code: "unsupported_platform",
            message: "当前平台不支持在文件管理器中显示".into(),
        },
        _ => AppError::Reveal {
            code: "reveal_failed",
            message: "无法在文件管理器中显示该项".into(),
        },
    }
}
