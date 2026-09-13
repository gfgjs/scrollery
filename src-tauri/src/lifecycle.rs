// src-tauri/src/lifecycle.rs
//! 应用生命周期回调:窗口事件拦截 + `RunEvent` 处理(退出前 WAL checkpoint、后台任务
//! 优雅停止、Ready 计时与冒烟测试出口)。
//!
//! 自 `lib.rs::run()` 的 `.on_window_event(...)` / `.run(...)` 两个闭包迁出
//! (D-450 纯结构移动,行为不变)。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tauri::{Emitter, Manager};
use tracing::info;

use crate::state::AppState;

/// 启动计时锚(2026-07-13 排查热启动变慢):`RunEvent::Ready` 读它算 Rust boot→Ready 总耗时,
/// 用于一刀切分「后端 setup 耗时」与「前端 Vite/WebView2/Vue 挂载耗时」。
/// 拆分前是 `run()` 的函数局部 static;跨 `run()`/[`on_run_event`] 两处共享,故上提为模块级。
pub static BOOT_INSTANT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 前端最近一次心跳的 Unix 毫秒时间戳；0 表示本进程尚未收到前端心跳。
static FRONTEND_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);

/// 超过此时长未收到心跳时，认为 WebView 已失联。
const FRONTEND_HEARTBEAT_TIMEOUT_MS: u64 = 3_000;

/// ask 模式心跳陈旧时，宽限等待心跳恢复的最长时长：宽限内恢复 → 转交前端弹确认框；
/// 耗尽仍无心跳 → 原生关闭。10s 是「启动慢/主线程长卡」的余量，也把失联窗的可关闭延迟压到有界。
const CLOSE_GRACE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// 宽限任务进行中标记：防止用户反复点 ✕ 时堆叠多个宽限任务/重复 emit。
static CLOSE_GRACE_PENDING: AtomicBool = AtomicBool::new(false);

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 前端 IPC 心跳的唯一写入口。
pub fn mark_frontend_heartbeat() {
    FRONTEND_HEARTBEAT_MS.store(unix_now_ms(), Ordering::Release);
}

fn frontend_is_alive() -> bool {
    let last = FRONTEND_HEARTBEAT_MS.load(Ordering::Acquire);
    last != 0 && unix_now_ms().saturating_sub(last) <= FRONTEND_HEARTBEAT_TIMEOUT_MS
}

/// 关闭请求分派：`exit` 直接原生关闭；`minimize_to_tray` 由 on_window_event 更早的分支原生隐藏
/// （不进入本分派，避免把托盘动作绑在 WebView 上）；`ask` 前端存活交前端，心跳陈旧进宽限等待；
/// 未知值按旧语义：存活交前端、失联原生关闭。
#[derive(Debug, PartialEq, Eq)]
enum CloseDecision {
    /// 直接放行原生关闭。
    AllowNative,
    /// 阻止原生关闭，转交前端 `window-close-requested`（ask 弹确认框）。
    Frontend,
    /// 阻止原生关闭，启动宽限任务等心跳恢复（ask 且心跳陈旧，不静默直退）。
    GraceWait,
}

fn close_decision(close_behavior: &str, frontend_alive: bool) -> CloseDecision {
    match close_behavior {
        "exit" => CloseDecision::AllowNative,
        _ if frontend_alive => CloseDecision::Frontend,
        _ if close_behavior == "ask" => CloseDecision::GraceWait,
        _ => CloseDecision::AllowNative,
    }
}

/// ask 模式心跳陈旧时的宽限任务：先恢复窗口可见（Chromium 对隐藏页计时器节流——隐藏约 5 分钟后
/// 约 1 次/分，隐藏期的心跳不能反映真实存活；恢复可见即解除节流），再轮询心跳至多
/// [`CLOSE_GRACE_WINDOW`]：心跳恢复 → 转交前端弹确认框；仍无心跳 → 原生销毁窗口
/// （保住「失联窗可关闭」的原始修复目标，只是延迟到宽限耗尽）。
async fn resolve_ask_close(window: tauri::Window) {
    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
    }
    if !window.is_visible().unwrap_or(true) {
        let _ = window.show();
        let _ = window.set_focus();
        // 恢复可见(解除心跳节流)会重置窗口材质(tauri#12854),按配置重挂一次。
        crate::window_material::apply_from_config(window.app_handle());
    }
    let deadline = std::time::Instant::now() + CLOSE_GRACE_WINDOW;
    loop {
        if frontend_is_alive() {
            CLOSE_GRACE_PENDING.store(false, Ordering::Release);
            let _ = window.emit("window-close-requested", ());
            return;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    CLOSE_GRACE_PENDING.store(false, Ordering::Release);
    let _ = window.destroy();
}

/// 主窗口关闭处理(按配置优先走原生路径，ask 且前端存活时交前端决策)+ 前后台切换驱动缩略图
/// worker QoS 档位。
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() == "main" {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            let close_behavior = window
                .app_handle()
                .try_state::<Arc<AppState>>()
                .and_then(|state| state.config.get("close_behavior"))
                .unwrap_or_else(|| "ask".to_string());

            if close_behavior == "minimize_to_tray" {
                // 托盘隐藏是无状态的原生窗口操作，不需要等待前端确认。
                api.prevent_close();
                if let Err(e) = window.hide() {
                    tracing::error!(error = %e, "Failed to hide main window for close behavior=minimize_to_tray; exiting");
                    window.app_handle().exit(1);
                }
                return;
            }

            match close_decision(&close_behavior, frontend_is_alive()) {
                CloseDecision::AllowNative => {} // exit / 未知值且失联 → 放行原生关闭
                CloseDecision::Frontend => {
                    // 阻止默认的窗口关闭物理行为，向前端发送事件，由前端根据用户设置
                    // 处理（最小化到托盘、退出或询问）。
                    api.prevent_close();
                    if let Err(e) = window.emit("window-close-requested", ()) {
                        tracing::warn!("Failed to emit window-close-requested event: {}", e);
                    }
                }
                CloseDecision::GraceWait => {
                    tracing::warn!(
                        "Frontend heartbeat stale; starting close grace window before native close fallback"
                    );
                    api.prevent_close();
                    // 已有宽限任务在跑则忽略本次重复请求（该任务会统一收口）。
                    if !CLOSE_GRACE_PENDING.swap(true, Ordering::AcqRel) {
                        tauri::async_runtime::spawn(resolve_ask_close(window.clone()));
                    }
                }
            }
        }
        // 前后台切换 → 驱动缩略图 worker 的 QoS 档位:前台用 P 大核提速(仍被 UI 抢占)、
        // 后台挤回 E 小核限频省电(2026-07-13,配合 thumbnail::qos 的 refresh_worker_qos)。
        if let tauri::WindowEvent::Focused(focused) = event {
            crate::thumbnail::qos::set_app_foreground(*focused);
            if *focused {
                // Win11 Acrylic 可能在失焦后丢失(上游 window-vibrancy#139),回到前台时按当前
                // 配置重挂；apply 内部投递主线程，且与已有 show 后重挂路径共用同一清理/应用链。
                crate::window_material::apply_from_config(window.app_handle());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{close_decision, CloseDecision};

    #[test]
    fn exit_behavior_always_allows_native_close() {
        assert_eq!(close_decision("exit", true), CloseDecision::AllowNative);
        assert_eq!(close_decision("exit", false), CloseDecision::AllowNative);
    }

    #[test]
    fn ask_behavior_with_live_frontend_goes_to_frontend() {
        assert_eq!(close_decision("ask", true), CloseDecision::Frontend);
    }

    #[test]
    fn ask_behavior_with_stale_frontend_waits_grace_window() {
        // 不再静默直退：心跳陈旧先进宽限等待，耗尽才原生关闭（resolve_ask_close 内收口）。
        assert_eq!(close_decision("ask", false), CloseDecision::GraceWait);
    }

    #[test]
    fn unknown_behavior_falls_back_to_native_close_when_frontend_is_stale() {
        assert_eq!(close_decision("unexpected", true), CloseDecision::Frontend);
        assert_eq!(
            close_decision("unexpected", false),
            CloseDecision::AllowNative
        );
    }
}

/// `RunEvent` 分发:退出路径做 WAL 截断 + 后台任务 abort/join,Ready 路径记启动耗时。
#[allow(clippy::items_after_test_module)]
pub fn on_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
            info!("Application exiting — checkpointing WAL before termination | 退出前检查点 WAL");
            // Truncate the WAL so it doesn't grow unbounded across sessions.
            // `process::exit` skips Drop, so we must checkpoint explicitly here.
            // 截断 WAL，避免跨会话无限增长。process::exit 会跳过 Drop，
            // 因此必须在此显式检查点。
            if let Some(state) = app_handle.try_state::<Arc<AppState>>() {
                {
                    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
                        tracing::warn!(
                            "WAL checkpoint on exit failed | 退出时 WAL 检查点失败: {}",
                            e
                        );
                    }
                }
            }

            if let Some(handles_pool) = app_handle
                .try_state::<Arc<std::sync::Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>>>()
            {
                if let Ok(mut lock) = handles_pool.lock() {
                    let handles: Vec<_> = lock.drain(..).collect();
                    for h in &handles {
                        h.abort();
                    }
                    tauri::async_runtime::block_on(async move {
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                            for h in handles {
                                let _ = h.await;
                            }
                        })
                        .await;
                    });
                    info!("Background tasks gracefully stopped | 后台任务已优雅停止");
                }
            }

            std::process::exit(0);
        }
        tauri::RunEvent::Ready => {
            // 启动计时(2026-07-13 排查热启动变慢):Rust boot→Ready 总耗时。仅覆盖后端 setup +
            // 事件循环拉起,不含前端 Vite/WebView2/Vue 挂载——若本行远小于观感热启动时长,瓶颈
            // 在前端而非后端。慢于阈值升 warn(warn 级也可见)。
            if let Some(t0) = BOOT_INSTANT.get() {
                let ms = t0.elapsed().as_millis();
                if ms > 3000 {
                    tracing::warn!(
                        "Rust boot → Ready {}ms(后端启动偏慢,查 Boot WAL checkpoint 等阻塞步骤) | slow backend boot to ready",
                        ms
                    );
                } else {
                    info!("Rust boot → Ready {}ms | backend boot to ready", ms);
                }
            }
            // 开机冒烟测试：设 PICASA_SMOKE_TEST 时，应用一旦「启动就绪」即退出 0。
            // CI headless 启动构建产物 + 断言退出码非 101 → 把「开机 panic」
            //（coordinator 无 reactor / 迁移失败 等）挡在合并前，而非等 run dev 才发现。
            if std::env::var_os("PICASA_SMOKE_TEST").is_some() {
                eprintln!(
                    "[smoke] boot reached RunEvent::Ready — startup OK | 开机就绪，冒烟测试通过"
                );
                app_handle.exit(0);
            }
        }
        _ => {}
    }
}
