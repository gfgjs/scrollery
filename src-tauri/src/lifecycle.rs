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

/// 关闭请求分派:`minimize_to_tray` 由 on_window_event 更早的分支原生隐藏(不进入本分派,避免把托盘
/// 动作绑在 WebView 上);`ask` 前端存活交前端(弹确认框),心跳陈旧进宽限等待;`exit` 前端存活也交
/// 前端走「先 flush 再 EXIT_APP」,失联才原生关闭兜底;未知值按旧语义:存活交前端、失联原生关闭。
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
        // exit:前端存活时也交给前端走「先 flush 再 EXIT_APP」的正常路径 —— 直接放行原生关闭会让
        // 页面先销毁,防抖窗口里尚未提交的设置连 ExitRequested 都赶不上(那一刻窗口已不在
        // webview_windows 里,后端等不到任何回执)。前端失联才退原生关闭兜底。
        "exit" if frontend_alive => CloseDecision::Frontend,
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
    // 窗口几何采集(设计 §6):移动/改尺寸/DPI 变化都可能是用户调整,按 500ms 空闲、2s 上限
    // 合并提交(见 config::window);visible 与最小化状态不参与采集。
    if matches!(
        event,
        tauri::WindowEvent::Moved(_)
            | tauri::WindowEvent::Resized(_)
            | tauri::WindowEvent::ScaleFactorChanged { .. }
    ) {
        crate::config::window::note_geometry_event(window);
    }
    // 辅助窗口(日志窗)销毁:立即把它的待保存几何落盘 —— 之后不会再有它的几何事件,在待保存
    // 集合里等防抖窗口只会把它丢掉。
    if matches!(event, tauri::WindowEvent::Destroyed) && window.label() != "main" {
        let label = window.label().to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::config::window::commit_pending_for_label(&label).await {
                // 失败时该窗口的几何已还原进待保存集合,由退出流程或下次调整重试;不静默吞掉。
                tracing::warn!(label, "日志窗口关闭时的窗口几何未保存成功 | {e}");
            }
        });
    }
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
                CloseDecision::AllowNative => {} // 失联(exit/未知值) → 放行原生关闭
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

// ── 退出前设置 flush(设计 §5.4)────────────────────────────────────────────────

/// 等前端回报的超时上界:超过即按未完成处理,不无限拖住退出。
const FRONTEND_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// 已决定本次退出可以放行(用户明确放弃,或 flush 已结清):ExitRequested 收到它才不再拦。
///
/// **不能**用「上一次 flush 成功过」当这个标记:应用会继续运行、继续有新设置写入,旧成功不代表
/// 现在可以退出。故本标记只在真正决定退出的那一刻置位(见 allow_exit)。
static EXIT_ALLOWED: AtomicBool = AtomicBool::new(false);

/// 已开始一次退出前的 flush/裁决:ExitRequested 的重复事件据此只拦不重复 spawn。
static EXIT_FLUSH_STARTED: AtomicBool = AtomicBool::new(false);

/// 标记「本次退出已可放行」。调用方:用户放弃未保存修改、flush 结清后即将 app.exit、以及开机
/// 冒烟测试等已知无需等待的退出。
pub fn allow_exit() {
    EXIT_ALLOWED.store(true, Ordering::Release);
}

/// ExitRequested 的处置动作。
#[derive(Debug, PartialEq, Eq)]
enum ExitRequestAction {
    /// 已决定退出 → 放行原生退出。
    Allow,
    /// 已有一次 flush/裁决在进行 → **拦下但不重复 spawn**(直接 return 会放行退出,把在途写盘丢掉)。
    Suppress,
    /// 拦下并开始一次 flush。
    Start,
}

/// 纯决策:先看是否已允许放行,再解决重复事件的去重。
///
/// 关键不变量:**只有 Allow 才不拦**。Suppress 必须仍然 prevent_exit —— 这正是「重复 ExitRequested
/// 在 flush 进行中把进程放行」的根因。
fn exit_request_action(allowed: bool, already_started: bool) -> ExitRequestAction {
    if allowed {
        return ExitRequestAction::Allow;
    }
    if already_started {
        return ExitRequestAction::Suppress;
    }
    ExitRequestAction::Start
}

/// flush 请求序号(仅进程内唯一:前端据此去重,迟到回报据此丢弃)。
static FLUSH_SEQ: AtomicU64 = AtomicU64::new(0);

/// 发给前端窗口的 flush 请求(事件 settings-flush-requested 的载荷)。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FlushRequest {
    request_id: String,
}

/// 正在等待的一次 flush:收齐 expected 里全部窗口的回报才结清。
struct FlushWaiter {
    request_id: String,
    expected: std::collections::BTreeSet<String>,
    acked: std::collections::BTreeSet<String>,
    /// 全部回报都成功才为真(任一个 false → 本轮按未完成处理)。
    all_ok: bool,
    tx: tokio::sync::oneshot::Sender<bool>,
}

static FLUSH_WAITER: std::sync::Mutex<Option<FlushWaiter>> = std::sync::Mutex::new(None);

/// 记账一次回报,收齐则结清等待。返回是否匹配**当前**请求 id —— 迟到或跨轮的回报只丢弃,
/// 不影响本轮结论。unreachable 用于「窗口已不可用(事件发不出去)」:从 expected 里移除,
/// 既不拖住退出,也不误报成写盘失败。
fn flush_ack(request_id: &str, label: &str, ok: bool, unreachable: bool) -> bool {
    let mut guard = FLUSH_WAITER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    flush_ack_locked(&mut guard, request_id, label, ok, unreachable)
}

/// flush_ack 的实现体:对给定的等待状态记账。
///
/// 拆出来是为了让调用方传入**自己的**状态 —— 生产走 FLUSH_WAITER 全局静态,测试用局部状态。
/// 若测试直接改全局,并行执行的多个用例会互相顶掉对方的等待者(表现为随机失败),那并不是被测
/// 逻辑的问题。
fn flush_ack_locked(
    state: &mut Option<FlushWaiter>,
    request_id: &str,
    label: &str,
    ok: bool,
    unreachable: bool,
) -> bool {
    let (complete, all_ok) = match state.as_mut() {
        None => return false,
        Some(waiter) => {
            if waiter.request_id != request_id {
                return false;
            }
            if unreachable {
                waiter.expected.remove(label);
            } else {
                waiter.acked.insert(label.to_string());
                waiter.all_ok &= ok;
            }
            (waiter.acked.is_superset(&waiter.expected), waiter.all_ok)
        }
    };
    if complete {
        if let Some(done) = state.take() {
            let _ = done.tx.send(all_ok);
        }
    }
    true
}

/// 前端窗口回报 flush 结果(IPC settings_flush_done)的唯一入口。返回是否匹配当前请求 id。
pub fn acknowledge_settings_flush(window_label: &str, request_id: &str, ok: bool) -> bool {
    flush_ack(request_id, window_label, ok, false)
}

/// 一次向前端要 flush 的原始结果。
struct FlushReport {
    /// 是否收齐全部存活窗口的回执(false = 超时或有窗口在发送阶段即不可用)。
    complete: bool,
    /// 全部应答都是成功。
    all_ok: bool,
}

/// 退出前 flush 的对外结果(供 exit_app 与退出裁决使用)。
pub struct FlushOutcome {
    /// 前端是否**全部应答**(false = 超时或不可用:前端失联,等待已无意义)。
    pub complete: bool,
    /// 设置是否确实全部落盘(前端写盘成功,且后端窗口几何也已保存)。
    pub saved: bool,
}

/// 向全部存活窗口要一次设置 flush,等待回报至多 timeout。
///
/// 没有存活窗口时直接返回 true:前端已不存在,等待无意义 —— 失联场景不阻塞退出(与关闭路径的
/// 宽限等待同一条取舍)。窗口在事件发送阶段失败同样按不可用扣除,不误报为写盘失败。
async fn request_frontend_flush(
    app: &tauri::AppHandle,
    timeout: std::time::Duration,
) -> FlushReport {
    let windows: Vec<tauri::WebviewWindow> = app.webview_windows().into_values().collect();
    if windows.is_empty() {
        return FlushReport {
            complete: true,
            all_ok: true,
        };
    }
    let request_id = format!("flush-{}", FLUSH_SEQ.fetch_add(1, Ordering::Relaxed) + 1);
    let expected: std::collections::BTreeSet<String> =
        windows.iter().map(|w| w.label().to_string()).collect();
    let (tx, rx) = tokio::sync::oneshot::channel();
    *FLUSH_WAITER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(FlushWaiter {
        request_id: request_id.clone(),
        expected,
        acked: std::collections::BTreeSet::new(),
        all_ok: true,
        tx,
    });

    let payload = FlushRequest {
        request_id: request_id.clone(),
    };
    for window in &windows {
        if let Err(e) = window.emit("settings-flush-requested", payload.clone()) {
            tracing::warn!(
                label = window.label(),
                "设置 flush 请求发送失败,该窗口按不可用处理 | {e}"
            );
            flush_ack(&request_id, window.label(), true, true);
        }
    }

    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(ok)) => FlushReport {
            complete: true,
            all_ok: ok,
        },
        // 发送端被丢弃:本轮已在别处结清。
        Ok(Err(_)) => FlushReport {
            complete: true,
            all_ok: true,
        },
        Err(_) => {
            let mut guard = FLUSH_WAITER
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.as_ref().is_some_and(|w| w.request_id == request_id) {
                *guard = None;
                tracing::warn!("设置 flush 等待超时,未收齐前端回执 | settings flush timed out");
                FlushReport {
                    complete: false,
                    all_ok: false,
                }
            } else {
                // 极窄竞态:超时与收齐同时发生。收齐方已给出结论,以它为准。
                FlushReport {
                    complete: true,
                    all_ok: true,
                }
            }
        }
    }
}

/// 退出前收尾:等前端在途提交落盘,再把后端自持的窗口待保存几何落盘。
///
/// 返回结果区分「前端是否全部应答」与「是否确实全部落盘」:complete=false 表示前端失联或超时
/// (等待已无意义,按失联兜底继续退出);complete=true 且 saved=false 才是**确实有人报了写盘失败**,
/// 才值得弹对话框让用户裁决重试或明确放弃。本函数不替用户丢修改,也不把未保存说成已保存。
pub async fn flush_settings_before_exit(app: &tauri::AppHandle) -> FlushOutcome {
    let report = request_frontend_flush(app, FRONTEND_FLUSH_TIMEOUT).await;
    // 窗口几何是后端自持的待保存值,与前端 flush 结果无关:两条路径都要落盘一次。
    // 这里同时会等在途的窗口几何提交结束(提交闸串行),失败批次已还原并报错给调用方。
    let geometry_saved = match crate::config::window::commit_pending_now().await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("退出前窗口几何未落盘,已保留待保存值 | {e}");
            false
        }
    };
    FlushOutcome {
        complete: report.complete,
        saved: report.all_ok && geometry_saved,
    }
}

/// 退出前 flush 失败 → 交给用户裁决:重试,或明确放弃未保存修改并退出。
///
/// 只在失败时弹一次原生对话框(兑现「保存失败可重试或明确放弃」的方案要求);用户选重试则再走
/// 一轮 flush,选放弃则返回 false 由调用方继续退出。对话框回调经 oneshot 回传,不阻塞事件循环,
/// 也不在 UI 线程上做文件写入。
async fn resolve_failed_flush(app: &tauri::AppHandle) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    loop {
        let outcome = flush_settings_before_exit(app).await;
        if outcome.saved {
            return true;
        }
        if !outcome.complete {
            // 前端失联或超时:等待已无意义。按与关闭路径一致的失联兜底继续退出,不弹对话框把退出
            // 悬住(用户点 ✕/托盘退出时,进程必须能结束)。
            tracing::warn!(
                "退出前设置 flush 未收齐前端回执(前端失联或超时),按放弃未保存修改继续退出 | settings flush incomplete, falling back to exit"
            );
            return false;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let message = "部分设置尚未保存成功。可以重试,或放弃未保存的修改并退出。\n\nSome settings could not be saved. Retry, or discard the unsaved changes and exit.";
        app.dialog()
            .message(message)
            .title("Scrollery — 设置未保存 / Settings not saved")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "重试 / Retry".to_string(),
                "放弃并退出 / Discard and exit".to_string(),
            ))
            .show(move |retry| {
                let _ = tx.send(retry);
            });
        // 对话框被丢弃(窗口已销毁等极端情况)按「放弃」处理,不让退出悬住。
        if !rx.await.unwrap_or(false) {
            return false;
        }
    }
}

/// 测试用:构造一份**局部**等待状态(不碰全局静态,故用例之间互不干扰)。
#[cfg(test)]
fn local_flush_waiter(
    request_id: &str,
    labels: &[&str],
) -> (Option<FlushWaiter>, tokio::sync::oneshot::Receiver<bool>) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let waiter = FlushWaiter {
        request_id: request_id.to_string(),
        expected: labels.iter().map(|l| l.to_string()).collect(),
        acked: std::collections::BTreeSet::new(),
        all_ok: true,
        tx,
    };
    (Some(waiter), rx)
}

#[cfg(test)]
mod tests {
    use super::{close_decision, CloseDecision};

    /// exit + 前端存活 → 交前端走「先 flush 再 EXIT_APP」:原生直接关闭会让页面先销毁,防抖窗口里
    /// 还没提交的设置连 ExitRequested 都赶不上(那一刻窗口已不在 webview_windows 里)。
    #[test]
    fn exit_behavior_with_live_frontend_goes_to_frontend() {
        assert_eq!(close_decision("exit", true), CloseDecision::Frontend);
    }

    /// exit + 前端失联 → 原生关闭兜底:前端已无法消费关闭事件,拦下来也只会得到关不掉的窗口。
    #[test]
    fn exit_behavior_with_stale_frontend_falls_back_to_native_close() {
        assert_eq!(close_decision("exit", false), CloseDecision::AllowNative);
    }

    /// 只有「本次退出已获允许」才放行;重复的 ExitRequested 必须被拦下(直接 return 会放行退出,
    /// 把在途 flush 丢掉)。
    #[test]
    fn exit_request_only_allows_when_explicitly_allowed() {
        assert_eq!(
            super::exit_request_action(false, false),
            super::ExitRequestAction::Start
        );
        assert_eq!(
            super::exit_request_action(false, true),
            super::ExitRequestAction::Suppress,
            "已在 flush 的重复事件只拦不重开"
        );
        assert_eq!(
            super::exit_request_action(true, false),
            super::ExitRequestAction::Allow
        );
        assert_eq!(
            super::exit_request_action(true, true),
            super::ExitRequestAction::Allow,
            "已允许时无论是否在跑都放行"
        );
    }

    /// 显式放弃(exit_app force)与「已决定退出」的路径都会先调 allow_exit:随后的 ExitRequested
    /// 必须放行 —— 否则用户点了「放弃并退出」还要再被拦一次、再走一轮 flush + 确认框。
    #[test]
    fn allow_exit_grants_permission_for_followup_exit_request() {
        super::allow_exit();
        let allowed = super::EXIT_ALLOWED.load(std::sync::atomic::Ordering::Acquire);
        assert!(allowed, "allow_exit 应置位放行标记");
        assert_eq!(
            super::exit_request_action(allowed, true),
            super::ExitRequestAction::Allow,
            "已允许时即便有一次 flush 标记在跑也要放行"
        );
    }

    /// 「上一次 flush 成功」不得当作永久放行:EXIT_ALLOWED 只能由 allow_exit 显式置位,而 flush
    /// 报告本身不写这个标记(应用继续运行、继续有新设置的场景由此不再被旧成功短路)。
    #[test]
    fn flush_report_does_not_grant_permission_by_itself() {
        let outcome = super::FlushOutcome {
            complete: true,
            saved: true,
        };
        assert!(outcome.saved);
        // 仅凭一份成功报告,决策仍是「拦下并 flush」而不是放行。
        assert_eq!(
            super::exit_request_action(false, false),
            super::ExitRequestAction::Start
        );
    }

    /// 前端失联(未收齐回执)时退出决策走「放弃并退出」,不弹对话框把退出悬住;只有确实报失败
    /// (complete 且未 saved)才交给用户裁决。
    #[test]
    fn incomplete_flush_is_discard_and_failed_flush_is_user_choice() {
        let unreachable = super::FlushOutcome {
            complete: false,
            saved: false,
        };
        assert!(!unreachable.saved, "失联不算已保存");
        assert!(
            !unreachable.complete,
            "失联应可被识别,从而走失联兜底而非对话框"
        );

        let reported_failure = super::FlushOutcome {
            complete: true,
            saved: false,
        };
        assert!(reported_failure.complete && !reported_failure.saved);

        let all_saved = super::FlushOutcome {
            complete: true,
            saved: true,
        };
        assert!(all_saved.complete && all_saved.saved);
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
    /// 旧 request id 的回报直接丢弃:重试轮次之间不能互相串结论。
    #[test]
    fn flush_ack_ignores_stale_request_ids() {
        let (mut state, mut rx) = super::local_flush_waiter("id-1", &["main", "logs"]);
        assert!(
            !super::flush_ack_locked(&mut state, "id-0", "main", true, false),
            "不匹配的 request id 应丢弃"
        );
        assert!(
            super::flush_ack_locked(&mut state, "id-1", "main", true, false),
            "尚未收齐"
        );
        assert!(
            super::flush_ack_locked(&mut state, "id-1", "logs", true, false),
            "收齐最后一个回报"
        );
        assert_eq!(rx.try_recv().ok(), Some(true));
    }

    /// 任一个窗口报失败 → 本轮整体按未完成处理(不把未保存当已保存)。
    #[test]
    fn flush_ack_reports_failure_when_any_window_fails() {
        let (mut state, mut rx) = super::local_flush_waiter("id-2", &["main", "logs"]);
        assert!(super::flush_ack_locked(
            &mut state, "id-2", "main", false, false
        ));
        assert!(super::flush_ack_locked(
            &mut state, "id-2", "logs", true, false
        ));
        assert_eq!(rx.try_recv().ok(), Some(false));
    }

    /// 事件发不出去的窗口按不可用扣除,不拖住退出也不误报写盘失败。
    #[test]
    fn unreachable_window_is_settled_without_failing_the_round() {
        let (mut state, mut rx) = super::local_flush_waiter("id-3", &["main", "logs"]);
        assert!(
            super::flush_ack_locked(&mut state, "id-3", "logs", true, true),
            "按不可用结清该窗口"
        );
        assert!(rx.try_recv().is_err(), "main 尚未回报");
        assert!(super::flush_ack_locked(
            &mut state, "id-3", "main", true, false
        ));
        assert_eq!(rx.try_recv().ok(), Some(true));
    }

    /// 同一窗口重复回报只记一次,不因重入提前结清。
    #[test]
    fn duplicate_ack_from_same_window_is_idempotent() {
        let (mut state, mut rx) = super::local_flush_waiter("id-4", &["main", "logs"]);
        assert!(super::flush_ack_locked(
            &mut state, "id-4", "main", true, false
        ));
        assert!(super::flush_ack_locked(
            &mut state, "id-4", "main", true, false
        ));
        assert!(rx.try_recv().is_err(), "logs 未回报不该结清");
        assert!(super::flush_ack_locked(
            &mut state, "id-4", "logs", true, false
        ));
        assert_eq!(rx.try_recv().ok(), Some(true));
    }
}

/// `RunEvent` 分发:退出路径做 WAL 截断 + 后台任务 abort/join,Ready 路径记启动耗时。
#[allow(clippy::items_after_test_module)]
pub fn on_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            // 正常退出等待前端在途提交与后端窗口待保存值(设计 §5.4)。
            //
            // 关键不变量:**只有「本次退出已获允许」才不拦**。重复到达的 ExitRequested 若直接
            // return(不 prevent_exit),会在 flush 尚未完成时放行原生退出,把在途的设置写盘与窗口
            // 待保存几何一起丢掉 —— 故 Suppress 分支也必须拦。
            match exit_request_action(
                EXIT_ALLOWED.load(Ordering::Acquire),
                EXIT_FLUSH_STARTED.swap(true, Ordering::AcqRel),
            ) {
                ExitRequestAction::Allow => {}
                ExitRequestAction::Suppress => api.prevent_exit(),
                ExitRequestAction::Start => {
                    api.prevent_exit();
                    let app = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        if !resolve_failed_flush(&app).await {
                            tracing::warn!(
                                "退出前设置 flush 未完成,按放弃未保存修改继续退出 | proceeding to exit with unsaved settings discarded"
                            );
                        }
                        // 先置「已允许」再请求退出:随后的 ExitRequested 必须放行,否则会与本任务形成
                        // 「拦下 → 再 flush」的循环。
                        allow_exit();
                        app.exit(0);
                    });
                }
            }
        }
        tauri::RunEvent::Exit => {
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

            // 最后一道兜底:进程即刻结束,把后端自持的窗口待保存几何同步落盘(正常路径已在
            // flush_settings_before_exit 里提交过,这里是空表早退)。
            // 正常路径已在 flush_settings_before_exit 里提交完毕;此处只做不阻塞的如实报告 ——
            // 事件循环收尾阶段不得再做文件写入或等待,否则会卡住 UI 线程。
            if crate::config::window::has_pending() {
                tracing::warn!(
                    "进程退出时仍有未落盘的窗口几何(防抖窗内被强制结束) | window geometry still pending at exit"
                );
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
                // 冒烟测试的意图就是「启动即退」,没有前端设置需要等:先允许退出,免得 ExitRequested
                // 又拦一次并等满 flush 超时(把 CI 的启动冒烟拖慢数秒)。
                allow_exit();
                app_handle.exit(0);
            }
        }
        _ => {}
    }
}
