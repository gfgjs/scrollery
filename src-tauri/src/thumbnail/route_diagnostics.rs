//! 可选的原生路由诊断：固定原因聚合，前八次及二次幂采样，不记录源路径。

use std::cell::RefCell;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// 由 SCROLLERY_THUMB_DIAGNOSTICS=1 显式启用。
pub fn enabled() -> bool {
    static ENABLED: LazyLock<bool> = LazyLock::new(|| {
        std::env::var("SCROLLERY_THUMB_DIAGNOSTICS").is_ok_and(|value| value == "1")
    });
    *ENABLED
}

/// 固定数量的路由与子阶段。
#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Vpl,
    D2d,
    Read,
    Init,
    Decode,
    Vpp,
    Sync,
    Encode,
    Probe,
}

/// 不包含路径或底层错误字符串的路由结果。
#[derive(Clone, Copy, Debug)]
pub enum Reason {
    Success,
    TargetLimit,
    SourceLimit,
    NoSession,
    Busy,
    Header,
    Icc,
    Unsupported,
    DeviceLost,
    SyncFailed,
    Failed,
    NotCompiled,
}

thread_local! {
    static COUNTS: RefCell<[[(u64, u64); 12]; 9]> = const { RefCell::new([[(0, 0); 12]; 9]) };
}

/// 各子阶段是路由总耗时的分项，不能与 Vpl/D2d 总耗时相加。
pub fn record(stage: Stage, reason: Reason, elapsed: Duration) {
    if !enabled() {
        return;
    }
    COUNTS.with_borrow_mut(|counts| {
        let (count, total_us) = &mut counts[stage as usize][reason as usize];
        *count = count.saturating_add(1);
        *total_us = total_us.saturating_add(elapsed.as_micros().min(u64::MAX as u128) as u64);
        if *count <= 8 || count.is_power_of_two() {
            tracing::info!(target: "scrollery::thumb_routes", ?stage, ?reason,
                count = *count, total_us = *total_us, "native route sample");
        }
    });
}

/// 对所有提前退出统一记时，默认按失败计数。
pub struct Attempt {
    stage: Stage,
    started: Instant,
    pub reason: Reason,
}

impl Attempt {
    /// 开始一次有界路由尝试。
    pub fn new(stage: Stage) -> Self {
        Self {
            stage,
            started: Instant::now(),
            reason: Reason::Failed,
        }
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        record(self.stage, self.reason, self.started.elapsed());
    }
}
