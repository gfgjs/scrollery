//! 「在系统文件管理器中显示」（reveal）的**单一实现**（F-015）。
//!
//! 此前有两套：`system_commands::show_in_explorer` 手写三平台 `Command::spawn`，
//! `tree_commands::reveal_tree_entry` 走 `tauri_plugin_opener`。两套的行为并不等价 ——
//! 手写版的 Linux 分支只 `xdg-open` 父目录，**不选中**目标文件。统一到 opener 后：
//! Windows `SHOpenFolderAndSelectItems`、macOS `open -R`、Linux D-Bus
//! `org.freedesktop.FileManager1.ShowItems`（失败回退 XDG portal `OpenDirectory`）——
//! 三平台都选中，Linux 缺陷随之消失。
//!
//! 另一处行为差异值得留意：手写版是 `.spawn()`（发射后不管，打开器起不来也返回 `Ok`），
//! opener 是同步等结果。故统一后 reveal **可能真的返回错误**，调用方需要能吃到。

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
/// `unsupported_platform` 单列，因为前端要据此永久隐藏该动作（而非提示重试）。
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
