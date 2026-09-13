// src-tauri/src/thumbnail/qos.rs
//! 缩略图工作线程的 QoS/线程预算策略(U-P4-a,2026-07-16 从 ipc/thumbnail_commands.rs 下沉)。
//!
//! 这是线程调度策略而非 IPC 语义:`set_app_foreground` 由 lib.rs 窗口 `Focused`
//! 事件驱动,与 IPC 无关;预算与 QoS 档位被批量/全量两条流水线共用。
//! 预算数学拆为纯函数 [`thumb_cpu_budget_for`](单测钉边界),运行期 wrapper
//! 再读 `available_parallelism`。

/// 纯预算数学:给定逻辑核数,返回缩略图流水线的线程数上限。
///
/// 保留策略(可调):≤8 逻辑核留 2,>8 留 4;至少保留 1 个工作线程。
pub(crate) fn thumb_cpu_budget_for(logical: usize) -> usize {
    let reserve = if logical <= 8 { 2 } else { 4 };
    logical.saturating_sub(reserve).max(1)
}

/// 缩略图流水线的**线程数上限** = 逻辑核数 − 少量保留。
///
/// 原实现按逻辑核数直接铺满(解码 `cores*2` + 编码 `cores` + deferred `cores/2` ≈ 3.5×核),
/// 线程远多于核只是徒增上下文切换与线程创建开销。本预算把三池收敛到 ≈ 逻辑核数一档。
///
/// **注意**:让整机不卡的「核级隔离/响应性」不靠这里的线程数,而靠 [`apply_thread_qos`] / [`refresh_worker_qos`]
/// ——worker 降优先级 + 按前后台切效率档,由 OS 调度器决定 P/E 落核与抢占。本函数只管别建过多线程。
pub(crate) fn thumb_cpu_budget() -> usize {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    thumb_cpu_budget_for(logical)
}

/// 缩略图 worker 的前后台状态(由 Tauri 窗口 `Focused` 事件经 [`set_app_foreground`] 更新)。
/// 默认 true:命令通常在 app 活跃时触发。worker 每处理一项读一次,决定 QoS 档位。
static APP_FOREGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 由 lib.rs 的窗口 `Focused` 事件驱动:记录 app 是否在前台。
pub fn set_app_foreground(foreground: bool) {
    APP_FOREGROUND.store(foreground, std::sync::atomic::Ordering::Relaxed);
}

/// worker 每处理一项调一次:前后台档位**变化时**才真正重设本线程 QoS(`last` 由 worker 线程
/// 本地持有,避免每项都发系统调用)。这样一条跑数分钟的全量生成,用户中途切走/切回也会即时改档。
pub(crate) fn refresh_worker_qos(last: &mut Option<bool>) {
    let fg = APP_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed);
    if *last != Some(fg) {
        apply_thread_qos(fg);
        *last = Some(fg);
    }
}

/// 以**当前**前后台状态给本线程标 QoS(rayon `start_handler` 等一次性入口用,
/// 无线程本地 `last` 可比对)。
pub(crate) fn apply_current_thread_qos() {
    apply_thread_qos(APP_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed));
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
fn apply_thread_qos(foreground: bool) {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadInformation, SetThreadPriority, ThreadPowerThrottling,
        THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY:均作用于当前线程伪句柄;失败返回错误被忽略(退回默认调度),无 UB。
    unsafe {
        let h = GetCurrentThread();
        // 恒 BELOW_NORMAL:UI/前台(NORMAL)在共享核上抢占 worker —— 整机不卡的核心。
        let _ = SetThreadPriority(h, THREAD_PRIORITY_BELOW_NORMAL);
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
        let _ = SetThreadInformation(
            h,
            ThreadPowerThrottling,
            ptr,
            std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

#[cfg(target_vendor = "apple")]
fn apply_thread_qos(foreground: bool) {
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
    unsafe {
        let _ = pthread_set_qos_class_self_np(class, 0);
    }
}

#[cfg(not(any(windows, target_vendor = "apple")))]
fn apply_thread_qos(_foreground: bool) {
    // Linux/Android:后续用 setpriority(nice)/sched_setscheduler(SCHED_BATCH);当前 no-op。
}

#[cfg(test)]
mod tests {
    use super::thumb_cpu_budget_for;

    /// 预算边界:保留策略(≤8 留 2 / >8 留 4)与「至少 1 线程」下限逐值钉死。
    #[test]
    fn budget_boundaries() {
        // 下限:极小核数不会算出 0 线程。
        assert_eq!(thumb_cpu_budget_for(0), 1);
        assert_eq!(thumb_cpu_budget_for(1), 1);
        assert_eq!(thumb_cpu_budget_for(2), 1);
        assert_eq!(thumb_cpu_budget_for(3), 1);
        // ≤8:留 2。
        assert_eq!(thumb_cpu_budget_for(4), 2);
        assert_eq!(thumb_cpu_budget_for(8), 6);
        // 阈值切换:9 核起留 4(9 核预算反而低于 8 核是保留策略的已知折点,钉死防回归漂移)。
        assert_eq!(thumb_cpu_budget_for(9), 5);
        assert_eq!(thumb_cpu_budget_for(16), 12);
        assert_eq!(thumb_cpu_budget_for(28), 24);
    }
}
