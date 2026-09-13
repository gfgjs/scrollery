//! 窗口材质(毛玻璃)——`window_material` 配置键的 DWM 背板应用器。
//!
//! 只负责原生层:把主窗口挂上 window-vibrancy 的 DWM 背板(mica / acrylic / blur / 清除),
//! 对应的 CSS 半透明层由前端 `html[data-glass]` 单独负责。选用理由与坑位见
//! `docs/designs/2026-08-24-窗口材质毛玻璃方案.md`(为何不用 tauri 内置 set_effects:它吞错误
//! 且是「取第一个支持项」而非按序 fallback,Win10 上 mica→blur 退化必须拿到 `Result`)。

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// 从当前配置读 `window_material` 并应用。供托盘、关闭宽限等「窗口由隐藏变为可见」的
/// 路径调用——可见性变化可能重置材质(tauri#12854),show 后按配置重挂一次(零失败成本)。
pub fn apply_from_config(app: &AppHandle) {
    let value = app
        .try_state::<Arc<AppState>>()
        .and_then(|s| s.config.get("window_material"))
        .unwrap_or_else(|| "mica".to_string()); // ConfigManager::get 对 schema 键自带默认,None 仅理论兜底
    apply(app, &value);
}

/// 应用指定材质值(raw 文本:`mica`/`acrylic`/`none`)。对外可见(除配置热应用外无别的调用方),
/// `apply_setting_effects` 的 window_material 分支与 `apply_from_config` 收口于此。
///
/// 材质切换经由 `run_on_main_thread` 投递到主线程执行:window-vibrancy 走底层窗口句柄发
/// DWM/SWCA 消息,要求与窗口事件循环同线程(tauri 对窗口操作的一致惯例),且本函数可能被
/// watcher 回调(任意 tokio worker 线程)与 IPC 命令两处并发调用,投递到主线程同时天然串行化。
pub fn apply(app: &AppHandle, value: &str) {
    if let Some(main_win) = app.get_webview_window("main") {
        let win = main_win.clone();
        let v = value.to_string();
        if let Err(error) = main_win.run_on_main_thread(move || apply_material(&win, &v)) {
            tracing::warn!(%error, "Failed to enqueue window material update");
        }
    }
}

/// 在启动阶段首次显示窗口前同步应用材质。调用方必须已经位于 Tauri 主线程；
/// 这是为了避免 `show()` 与异步主线程投递之间出现一帧透明窗口。
pub(crate) fn apply_now(win: &tauri::WebviewWindow, value: &str) {
    apply_material(win, value);
}

#[cfg(target_os = "windows")]
fn apply_material(win: &tauri::WebviewWindow, value: &str) {
    // window-vibrancy 0.6 在 Win11 22000–22522 的 Acrylic(SWCA) 与 Mica(旧 DWM 属性)
    // 走不同状态通道；切换前先归零，避免旧通道残留让新材质静默不生效。清理函数对当前
    // Windows 版本不支持的通道会返回 Err，但这属于正常探测结果，不应阻断目标材质应用。
    clear_existing_material(win);

    match value {
        "acrylic" => {
            // 色调仅作用于 Win10 SWCA 路径;Win11 22H2+ 走 SYSTEMBACKDROP 忽略 color,
            // 观感浓度由前端 glass.css 半透明层控制。SWCA 全透无 tint 时近全透明,故带底色。
            if let Err(e) = window_vibrancy::apply_acrylic(win, Some((28, 28, 32, 40))) {
                tracing::warn!(error = %e, "apply_acrylic failed");
            }
        }
        "none" => {
            // clear_existing_material 已完成三种背板的清理；none 不再挂任何效果。
        }
        _ => {
            // mica:Win11 专属;失败(Win10)退化到 DWM blur,与 CSS 玻璃层仍成配对观感。
            if window_vibrancy::apply_mica(win, None).is_err() {
                let _ = window_vibrancy::apply_blur(win, Some((28, 28, 32, 60)));
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn clear_existing_material(win: &tauri::WebviewWindow) {
    let _ = window_vibrancy::clear_acrylic(win);
    let _ = window_vibrancy::clear_mica(win);
    let _ = window_vibrancy::clear_blur(win);
}

#[cfg(not(target_os = "windows"))]
fn apply_material(_win: &tauri::WebviewWindow, _value: &str) {}
