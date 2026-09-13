// src-tauri/src/tray.rs
//! 系统托盘装配(自 `lib.rs::run()` 的 setup 段 t 迁出,D-450 纯结构移动,行为不变)。
//! 零跨域依赖:只用 `tauri::menu`/`tauri::tray`,不碰 `AppState`。

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

/// 构建托盘图标 + 菜单(显示主界面 / 退出),并挂上左键点击呼出主窗口的行为。
///
/// 托盘句柄由 Tauri 内部持有,故此处不回传——沿用拆分前 `let _tray = ...` 的姿态。
pub fn build(app: &tauri::App) -> tauri::Result<()> {
    let show_i = MenuItem::with_id(app, "show", "显示主界面 | Show Window", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "退出应用 | Exit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => {
                tracing::info!("Quit clicked from tray menu | 用户从托盘菜单点击了退出");
                app.exit(0);
            }
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    // 托盘隐藏→显示往返会重置窗口材质(tauri#12854),show 后按配置重挂。
                    crate::window_material::apply_from_config(app);
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    // 左键单击呼出的显示路径同菜单项:可见性变化可能重置材质(tauri#12854)。
                    crate::window_material::apply_from_config(app);
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    }
    let _tray = tray_builder.build(app)?;
    Ok(())
}
