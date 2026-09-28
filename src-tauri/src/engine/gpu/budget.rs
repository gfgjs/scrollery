//! 宿主统一的图像/视频 GPU 准入；原生子进程额度由请求生命周期持有。

use std::sync::Mutex;

#[derive(Default)]
struct State {
    occupied: Vec<bool>,
    peak: usize,
    denied: u64,
}

#[derive(Default)]
struct GpuBudget(Mutex<State>);

impl GpuBudget {
    fn try_acquire(&self, fast: bool) -> Option<GpuPermit<'_>> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let limit = crate::thumbnail::limits::get().gpu_inflight;
        state.occupied.resize(limit, false);
        // 多槽时为快速域留一席；单槽时两类任务共享。
        let index = (if fast { 0 } else { usize::from(limit > 1) }..limit)
            .find(|index| !state.occupied[*index]);
        let Some(index) = index else {
            state.denied = state.denied.saturating_add(1);
            return None;
        };
        state.occupied[index] = true;
        state.peak = state
            .peak
            .max(state.occupied.iter().filter(|used| **used).count());
        Some(GpuPermit {
            budget: self,
            index,
        })
    }
}

pub(crate) struct GpuPermit<'a> {
    budget: &'a GpuBudget,
    index: usize,
}

impl Drop for GpuPermit<'_> {
    fn drop(&mut self) {
        self.budget
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .occupied[self.index] = false;
    }
}

static BUDGET: GpuBudget = GpuBudget(Mutex::new(State {
    occupied: Vec::new(),
    peak: 0,
    denied: 0,
}));

pub(crate) fn try_acquire(fast: bool) -> Option<GpuPermit<'static>> {
    BUDGET.try_acquire(fast)
}

/// 应用宿主的在途 GPU 请求、峰值和准入拒绝次数；拒绝后允许 CPU 回退。
pub(crate) fn snapshot() -> (usize, usize, u64) {
    let state = BUDGET.0.lock().unwrap_or_else(|e| e.into_inner());
    (
        state.occupied.iter().filter(|used| **used).count(),
        state.peak,
        state.denied,
    )
}
