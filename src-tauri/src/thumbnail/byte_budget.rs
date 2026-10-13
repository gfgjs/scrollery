//! 阶段缓冲账本：先取得额度再分配，交接移动许可，只缩减而不持有旧额度申请增量。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub(crate) struct ByteBudget {
    limit: u64,
    used: Mutex<u64>,
    changed: Condvar,
    peak: AtomicU64,
}

impl ByteBudget {
    pub(crate) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: Mutex::new(0),
            changed: Condvar::new(),
            peak: AtomicU64::new(0),
        })
    }

    /// 超过整个阶段上限或执行域已关闭时返回 None，不进入永不满足的等待。
    pub(crate) fn acquire(
        self: &Arc<Self>,
        bytes: u64,
        stopped: &AtomicBool,
    ) -> Option<BytePermit> {
        if bytes > self.limit {
            return None;
        }
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if stopped.load(Ordering::Acquire) {
                return None;
            }
            if bytes <= self.limit - *used {
                *used += bytes;
                self.peak.fetch_max(*used, Ordering::Relaxed);
                return Some(BytePermit {
                    budget: Arc::clone(self),
                    bytes,
                });
            }
            used = self
                .changed
                .wait_timeout(used, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    pub(crate) fn peak(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }
}

pub(crate) struct BytePermit {
    budget: Arc<ByteBudget>,
    bytes: u64,
}

impl BytePermit {
    /// 真实结果变小时归还多余额度；禁止持有小图后再阻塞申请更大缓冲。
    pub(crate) fn shrink(&mut self, bytes: u64) {
        assert!(
            bytes <= self.bytes,
            "stage reservation cannot grow after allocation"
        );
        let mut used = self.budget.used.lock().unwrap_or_else(|e| e.into_inner());
        *used -= self.bytes - bytes;
        self.bytes = bytes;
        self.budget.changed.notify_all();
    }
}

impl Drop for BytePermit {
    fn drop(&mut self) {
        *self.budget.used.lock().unwrap_or_else(|e| e.into_inner()) -= self.bytes;
        self.budget.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_breaks_backpressure_without_releasing_another_work_items_bytes() {
        let budget = ByteBudget::new(10);
        let stopped = Arc::new(AtomicBool::new(false));
        let permit = budget.acquire(10, &stopped).unwrap();
        let thread_budget = Arc::clone(&budget);
        let thread_stopped = Arc::clone(&stopped);
        let thread =
            std::thread::spawn(move || thread_budget.acquire(1, &thread_stopped).is_none());
        stopped.store(true, Ordering::Release);
        assert!(thread.join().unwrap());
        assert_eq!(*budget.used.lock().unwrap(), 10);
        drop(permit);
        assert_eq!(*budget.used.lock().unwrap(), 0);
    }

    #[test]
    fn oversized_work_is_rejected_instead_of_waiting_forever() {
        assert!(ByteBudget::new(10)
            .acquire(11, &AtomicBool::new(false))
            .is_none());
    }
}
