// src-tauri/src/thumbnail/qos.rs
//! 缩略图工作线程的 QoS/线程预算策略(U-P4-a,2026-07-16 从 ipc/thumbnail_commands.rs 下沉)。
//!
//! 这是线程调度策略而非 IPC 语义:`set_app_foreground` 由 lib.rs 窗口 `Focused`
//! 事件驱动,与 IPC 无关;预算与 QoS 档位被批量/全量两条流水线共用。
//! 预算数学拆为纯函数 [`processing_budgets_for`](单测钉边界),运行期 wrapper
//! 再读 `available_parallelism`。

/// 纯预算数学:给定逻辑核数,返回缩略图流水线的线程数上限。
///
/// 保留策略(可调):≤8 逻辑核留 2,>8 留 4;至少保留 1 个工作线程。
pub(crate) fn thumb_cpu_budget_for(logical: usize) -> usize {
    let reserve = if logical <= 8 { 2 } else { 4 };
    logical.saturating_sub(reserve).max(1)
}

/// 返回缩略图线程数与共享 CPU 准入总额，其中一个名额保留给快速项。
/// 低核至少容纳快速/重活各一项，原生域另受 Job CPU 时间上限约束。
pub(crate) fn processing_budgets_for(logical: usize) -> (usize, usize) {
    let thumbnail = thumb_cpu_budget_for(logical);
    (thumbnail, thumbnail.max(2))
}

/// 低核机器上两个原生隔离域共享的 CPU 时间上限，单位为整机的万分之一。
///
/// 保留隔离域的独立线程，但不让 fast/tail 各自消耗一份 CPU 预算。
/// 使用内核调度额度，避免尾批持有宿主信号量时堵住快速域。
#[cfg(windows)]
pub(crate) fn native_cpu_rate_for(logical: usize) -> Option<u32> {
    let logical = logical.max(1);
    (logical <= 5).then(|| (thumb_cpu_budget_for(logical) * 10_000 / logical) as u32)
}

/// 源介质策略来自系统探测；未知介质使用保守截止档。
#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SourceMedia {
    Ssd,
    Hdd,
    Unknown,
}

/// 原生实际执行的有界截止；排队等待不消耗此额度。
#[cfg(windows)]
pub(super) fn native_timeout(
    fast: bool,
    media: SourceMedia,
    foreground: bool,
) -> std::time::Duration {
    let base = if fast { 2 } else { 30 };
    let media_factor = if media == SourceMedia::Ssd { 1 } else { 2 };
    let foreground_factor = if foreground { 1 } else { 2 };
    std::time::Duration::from_secs(base * media_factor * foreground_factor)
}

/// 运行期同时取得缩略图线程和共享重活 permit 额度，避免两次探测不一致。
///
/// 原实现按逻辑核数直接铺满(解码 `cores*2` + 编码 `cores` + deferred `cores/2` ≈ 3.5×核),
/// 线程远多于核只是徒增上下文切换与线程创建开销。本预算把三池收敛到 ≈ 逻辑核数一档。
///
/// **注意**:让整机不卡的「核级隔离/响应性」不靠这里的线程数,而靠 [`apply_thread_qos`] / [`refresh_worker_qos`]
/// ——worker 降优先级 + 按前后台切效率档,由 OS 调度器决定 P/E 落核与抢占。本函数只管并行额度。
pub(crate) fn processing_budgets() -> (usize, usize) {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    processing_budgets_for(logical)
}

/// 缩略图 worker 的前后台状态(由 Tauri 窗口 `Focused` 事件经 [`set_app_foreground`] 更新)。
/// 默认 true:命令通常在 app 活跃时触发。worker 每处理一项读一次,决定 QoS 档位。
// 最低位为前后台，其余为 revision；一次原子读取避免新档位配到旧 revision。
static QOS_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static QOS_APPLIED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static QOS_FAILED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// watch 只保留一个唤醒版本，快速焦点变化不会积压事件或创建线程。
static FOCUS_CHANGED: std::sync::OnceLock<tokio::sync::watch::Sender<()>> =
    std::sync::OnceLock::new();

/// 订阅应用焦点变化；接收方醒来后读取最新的原子 QoS 快照。
pub(crate) fn subscribe_focus_changes() -> tokio::sync::watch::Receiver<()> {
    FOCUS_CHANGED
        .get_or_init(|| tokio::sync::watch::channel(()).0)
        .subscribe()
}

type HostReceipts = std::collections::HashMap<std::thread::ThreadId, Option<(u64, bool)>>;
static HOST_RECEIPTS: std::sync::OnceLock<std::sync::Mutex<HostReceipts>> =
    std::sync::OnceLock::new();

/// 处理线程对同一 QoS revision 的确认数；未到请求边界的线程计入 applying。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct QosCounts {
    pub(crate) expected: usize,
    pub(crate) accepted: usize,
    pub(crate) failed: usize,
    pub(crate) applying: usize,
}

impl QosCounts {
    pub(crate) fn record(&mut self, revision: u64, receipt: Option<(u64, bool)>) {
        self.expected += 1;
        match receipt {
            Some((applied_revision, applied)) if applied_revision == revision => {
                if applied {
                    self.accepted += 1;
                } else {
                    self.failed += 1;
                }
            }
            _ => self.applying += 1,
        }
    }
}

fn next_request(current: u64, foreground: bool) -> Option<u64> {
    (current & 1 != u64::from(foreground))
        .then(|| (current.wrapping_add(2) & !1) | u64::from(foreground))
}

/// 由全窗口焦点聚合驱动；档位和 revision 一起发布。
pub fn set_app_foreground(foreground: bool) {
    use std::sync::atomic::Ordering;
    if let Ok(previous) = QOS_REQUEST.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        next_request(current, foreground)
    }) {
        if let Some(changed) = FOCUS_CHANGED.get() {
            changed.send_replace(());
        }
        let revision = next_request(previous, foreground).expect("request changed") >> 1;
        tracing::info!(target: "scrollery::thumb_perf", foreground, qos_revision = revision,
            host_qos_applied_total = QOS_APPLIED.load(Ordering::Relaxed),
            host_qos_failed_total = QOS_FAILED.load(Ordering::Relaxed),
            "thumbnail host QoS requested");
    }
}

/// 返回原生请求使用的同一次前后台档位和焦点修订。
pub fn native_worker_qos_request() -> (bool, u64) {
    let current = QOS_REQUEST.load(std::sync::atomic::Ordering::Acquire);
    (current & 1 != 0, current >> 1)
}

pub(crate) fn host_qos_counts(revision: u64) -> QosCounts {
    let mut counts = QosCounts::default();
    let receipts = HOST_RECEIPTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for &receipt in receipts.values() {
        counts.record(revision, receipt);
    }
    counts
}

/// 在处理线程存活期间登记 QoS，退出时移除，避免历史线程污染当前确认数。
pub(crate) struct WorkerQosState {
    thread: std::thread::ThreadId,
    receipt: Option<(u64, bool)>,
}

impl WorkerQosState {
    pub(crate) fn new() -> Self {
        let mut state = Self {
            thread: std::thread::current().id(),
            receipt: None,
        };
        refresh_worker_qos(&mut state);
        state
    }
}

impl Drop for WorkerQosState {
    fn drop(&mut self) {
        HOST_RECEIPTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.thread);
    }
}

/// 原生子进程的处理线程在请求边界应用 QoS；返回系统是否接受两项设置。
#[cfg(windows)]
pub fn apply_native_worker_qos(foreground: bool) -> bool {
    apply_thread_qos(foreground)
}

/// 在安全边界应用当前 revision；同 revision 不重复发系统调用。
pub(crate) fn refresh_worker_qos(last: &mut WorkerQosState) {
    let (foreground, revision) = native_worker_qos_request();
    if last
        .receipt
        .is_none_or(|(previous, _)| previous != revision)
    {
        let applied = apply_thread_qos(foreground);
        let counter = if applied { &QOS_APPLIED } else { &QOS_FAILED };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        last.receipt = Some((revision, applied));
        HOST_RECEIPTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(last.thread, last.receipt);
        tracing::debug!(target: "scrollery::thumb_perf", foreground, qos_revision = revision,
            system_request_accepted = applied, "thumbnail host worker QoS applied");
    }
}

/// 按前后台把当前(worker)线程标 QoS,让各平台调度器接管 P/E 落核、超线程与抢占。
///
/// 取代原先的硬 CPU 亲和性(2026-07-13 真机迭代):固定掩码在混合大小核上留错核、又跟 OS 的
/// QoS/混合调度器打架;macOS/iOS 更根本没有硬亲和性。改用各平台一等公民的 QoS/优先级**提示**:
/// - **前台**:worker 降优先级但**不**开效率节流 → 调度器可用 P 大核提速,而 BELOW_NORMAL 让 UI
///   在共享核上抢占它 →「用 P 核但不卡」(修真机反馈:恒 EcoQoS 时前台 P 核占用为 0)。
/// - **后台**:开效率节流(EcoQoS)→ 挤回 E 小核、限频省电,把 P 核整体让给前台其它应用。
///
/// Windows:`SetThreadPriority(BELOW_NORMAL)` 恒定 + `SetThreadInformation` EcoQoS 前台关/后台开。
/// macOS/iOS:`pthread_set_qos_class_self_np`,前台 `QOS_CLASS_UTILITY`(可借 P 核)/ 后台
/// `QOS_CLASS_BACKGROUND`(Apple Silicon 硬性 E 核)。
/// Linux/Android:暂 no-op(后续 setpriority(nice)/SCHED_BATCH,与派生·AI 流水线一并处理)。
#[cfg(windows)]
fn apply_thread_qos(foreground: bool) -> bool {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadInformation, SetThreadPriority, ThreadPowerThrottling,
        THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY:均作用于当前线程伪句柄;失败返回错误被忽略(退回默认调度),无 UB。
    unsafe {
        let h = GetCurrentThread();
        // 恒 BELOW_NORMAL:UI/前台(NORMAL)在共享核上抢占 worker —— 整机不卡的核心。
        let priority = SetThreadPriority(h, THREAD_PRIORITY_BELOW_NORMAL).is_ok();
        // EcoQoS 开关:前台 StateMask=0(不节流,可用 P 核)/ 后台 =EXECUTION_SPEED(挤 E 核限频)。
        // ControlMask 恒置该位 = 声明「EXECUTION_SPEED 这一位由我们管理」。
        let state = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if foreground {
                0
            } else {
                THREAD_POWER_THROTTLING_EXECUTION_SPEED
            },
        };
        let ptr: *const core::ffi::c_void = (&state as *const THREAD_POWER_THROTTLING_STATE).cast();
        let power = SetThreadInformation(
            h,
            ThreadPowerThrottling,
            ptr,
            std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        )
        .is_ok();
        priority && power
    }
}

#[cfg(target_vendor = "apple")]
fn apply_thread_qos(foreground: bool) -> bool {
    // <sys/qos.h>:UTILITY=0x11(前台,可借 P 核)/ BACKGROUND=0x09(后台,Apple Silicon 硬 E 核)。
    const QOS_CLASS_UTILITY: u32 = 0x11;
    const QOS_CLASS_BACKGROUND: u32 = 0x09;
    extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    let class = if foreground {
        QOS_CLASS_UTILITY
    } else {
        QOS_CLASS_BACKGROUND
    };
    // SAFETY:稳定 Darwin API,作用于当前线程。
    unsafe { pthread_set_qos_class_self_np(class, 0) == 0 }
}

#[cfg(not(any(windows, target_vendor = "apple")))]
fn apply_thread_qos(_foreground: bool) -> bool {
    // Linux/Android:后续用 setpriority(nice)/sched_setscheduler(SCHED_BATCH);当前 no-op。
    false
}
