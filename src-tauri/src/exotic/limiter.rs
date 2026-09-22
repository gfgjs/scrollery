// src-tauri/src/exotic/limiter.rs
//! 后台重活公平并发池（v3.1 勘误 R4）。
//! 另含 GPU 推理令牌 [`GpuToken`](Part4 D2/T11)——复用同一公平队列骨架的独立类型,额度恒 1。
//!
//! **derivation 重型任务与 exotic Worker 请求共享同一个全局 permit 预算**——二者同级（总纲优先级阶梯），
//! 谁也不硬让步谁。若各开各的 semaphore，大视频库持续派生会饿死 exotic（PSD 永不出图）。
//!
//! 关键性质：
//!   - **FIFO 公平**：按到达顺序授予 permit（票号 + 队首服务），exotic 票一旦入队，下一个释放的
//!     permit 必落到它头上、不被后到的 derivation 票插队 → 最大排队等待有上界（R4 公平验收）。
//!   - **取消感知**：`acquire` 周期性醒来检查 `CancellationToken`；取消即退队返回 None，不泄漏票。
//!   - **即时释放**：`HeavyPermit` Drop 归还 permit 并唤醒队首（故障测试查无泄漏）。
//!
//! 调用约定（两条流水线一致）：在**派发线程**（非 rayon worker）取 permit → 移入任务闭包 → 任务
//! 完成/取消/kill 时 Drop 释放。派发线程阻塞在 acquire 即天然「预取不超过可派发容量」（R4 规则 4）。

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

/// 取消轮询周期：acquire 等待时每隔此时长醒来检查取消位（兼顾响应性与空转）。
const CANCEL_POLL: Duration = Duration::from_millis(100);

struct LimiterState {
    /// 当前可用 permit 数。
    available: usize,
    /// 下一张票号（单调递增）。
    next_ticket: u64,
    /// 等待队列（票号 FIFO）。队首才有资格在 available>0 时取走 permit。
    queue: VecDeque<u64>,
}

/// 公平后台重活池。`Arc` 共享给 derivation 与 exotic 两条流水线。
pub struct BackgroundHeavyLimiter {
    state: Mutex<LimiterState>,
    cv: Condvar,
    total: usize,
}

impl BackgroundHeavyLimiter {
    /// 以 `permits` 个并发额度创建（建议 = 后台重活目标并发，如 `available_parallelism()`）。
    pub fn new(permits: usize) -> Arc<Self> {
        let permits = permits.max(1);
        Arc::new(BackgroundHeavyLimiter {
            state: Mutex::new(LimiterState {
                available: permits,
                next_ticket: 0,
                queue: VecDeque::new(),
            }),
            cv: Condvar::new(),
            total: permits,
        })
    }

    /// 总额度。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 当前可用额度（瞬时快照；仅供观测/测试）。
    pub fn available(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .available
    }

    /// 公平取一个 permit；阻塞直到拿到或 `token` 取消。取消返回 `None`（已退队，不泄漏）。
    pub fn acquire(self: &Arc<Self>, token: &CancellationToken) -> Option<HeavyPermit> {
        self.acquire_cancellable(&|| token.is_cancelled())
    }

    /// 公平取额度，等待期间允许调用方按取消、熔断或授权状态退队。
    /// 判定在额度锁外执行，不能将调用方的状态锁带进共享 FIFO 临界区。
    pub(crate) fn acquire_cancellable(
        self: &Arc<Self>,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<HeavyPermit> {
        let ticket = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let ticket = state.next_ticket;
            state.next_ticket += 1;
            state.queue.push_back(ticket);
            ticket
        };

        loop {
            let cancelled = cancelled();
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if cancelled {
                remove_ticket(&mut state.queue, ticket);
                // 退队可能让出队首 → 唤醒其余等待者重新评估。
                self.cv.notify_all();
                return None;
            }
            // 仅队首在有额度时取走（FIFO，杜绝插队 → 等待上界）。
            if state.available > 0 && state.queue.front() == Some(&ticket) {
                state.available -= 1;
                state.queue.pop_front();
                return Some(HeavyPermit {
                    limiter: Arc::clone(self),
                });
            }
            let (state, _timeout) = self
                .cv
                .wait_timeout(state, CANCEL_POLL)
                .unwrap_or_else(|e| e.into_inner());
            drop(state);
        }
    }

    /// 释放一个 permit（仅由 [`HeavyPermit::drop`] 调用）。
    fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.available += 1;
        // 唤醒全部等待者：只有当前队首会真正取走，其余继续等（Condvar 无法精确点名队首）。
        drop(state);
        self.cv.notify_all();
    }
}

/// 从队列中移除某票号（取消退队用）。
fn remove_ticket(queue: &mut VecDeque<u64>, ticket: u64) {
    if let Some(pos) = queue.iter().position(|&t| t == ticket) {
        queue.remove(pos);
    }
}

/// permit 持有凭证。Drop 即归还额度并唤醒队首。
/// must_use:取到即弃 = 没有互斥,必是逻辑错误。
#[must_use]
pub struct HeavyPermit {
    limiter: Arc<BackgroundHeavyLimiter>,
}

impl Drop for HeavyPermit {
    fn drop(&mut self) {
        self.limiter.release();
    }
}

/// GPU 推理令牌:全局额度**恒 1**(集显/单 GPU 语义,Part4 D2 §3.1;多 permit 放行留
/// T22 按 VRAM 档位实测)。薄封装复用 [`BackgroundHeavyLimiter`] 的 FIFO 公平队列 +
/// 取消感知 + RAII 释放,但作为**独立类型**存在——防 CPU permit 池与 GPU 令牌在调用点
/// 混用,也让获取顺序纪律有类型可依。
///
/// 分层(D2 §3.2,勿合并):`AppState::gpu_analysis_owner` 管**会话语义**(哪条流水线在
/// 分析,分钟级,用户可见的 start/pause);本令牌管**物理并发**(哪个 worker 此刻真的在
/// 打 GPU,批粒度,秒级)。合并会让「暂停人脸让 CLIP 跑」这类语义纠缠进批调度。
///
/// 🔴 获取顺序天条(D2 §3.1/§4,防死锁):同时需要 CPU permit 与 GPU 令牌的路径,必须
/// **先 `BackgroundHeavyLimiter::acquire` 再 `GpuToken::acquire`**;释放顺序不限(RAII
/// Drop)。全局唯一获取顺序 → 等待图无环 → 无死锁。
///
/// 形态不变性(D2 §3.3):合并单 ai-worker(池宽 1)下令牌几乎恒空闲即得,退化为保险丝
/// (防未来第二 GPU 消费者);分离双 worker 下真跨池仲裁。两形态代码路径一致,T9.5/T20
/// 拍板不影响本模块。acquire 点接线随 T13/T15 批派发落地(发 EmbedBatch/FaceDetectEmbed
/// 前;文本塔恒 CPU 不占令牌,D2 §5)。
pub struct GpuToken {
    inner: Arc<BackgroundHeavyLimiter>,
}

impl GpuToken {
    /// 创建全局唯一 GPU 令牌(经 `AppState.gpu_token` 共享给全部 GPU 消费者)。
    pub fn new() -> Arc<Self> {
        Arc::new(GpuToken {
            inner: BackgroundHeavyLimiter::new(1),
        })
    }

    /// 公平取 GPU 令牌;阻塞直到拿到或 `token` 取消(取消返回 None,不泄漏票)。
    /// 语义与 [`BackgroundHeavyLimiter::acquire`] 完全一致。
    pub fn acquire(&self, token: &CancellationToken) -> Option<GpuPermit> {
        self.inner.acquire(token).map(|p| GpuPermit { _permit: p })
    }

    /// 令牌当前是否空闲(瞬时快照;仅供观测/测试)。
    pub fn is_idle(&self) -> bool {
        self.inner.available() == 1
    }
}

/// GPU 令牌持有凭证:Drop 即释放——批完成/超时/进程死/panic 展开均经 Drop,无泄漏面
/// (D2 §3.1「release 点」)。
#[must_use]
pub struct GpuPermit {
    _permit: HeavyPermit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn caps_concurrency_and_releases() {
        let lim = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let live = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let lim = Arc::clone(&lim);
            let token = token.clone();
            let live = Arc::clone(&live);
            let max_seen = Arc::clone(&max_seen);
            handles.push(thread::spawn(move || {
                let _permit = lim.acquire(&token).unwrap();
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(20));
                live.fetch_sub(1, Ordering::SeqCst);
                // permit drop 释放
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // 任一时刻并发不超过额度 2。
        assert!(max_seen.load(Ordering::SeqCst) <= 2);
        // 全部归还。
        assert_eq!(lim.available(), 2);
    }

    #[test]
    fn cancel_unblocks_waiter_without_leak() {
        let lim = BackgroundHeavyLimiter::new(1);
        let token = CancellationToken::new();
        // 主线程占满唯一 permit。
        let held = lim.acquire(&token).unwrap();

        let lim2 = Arc::clone(&lim);
        let token2 = token.clone();
        let waiter = thread::spawn(move || lim2.acquire(&token2).is_none());

        // 给 waiter 时间进入等待，然后取消。
        thread::sleep(Duration::from_millis(50));
        token.cancel();
        assert!(waiter.join().unwrap(), "取消应使等待者返回 None");

        // 持有者释放后额度回到 1，无票泄漏（队列已清）。
        drop(held);
        assert_eq!(lim.available(), 1);
    }

    #[test]
    fn fifo_fairness_first_waiter_served_first() {
        // 额度 1：占满后 A 先入队、B 后入队；释放一次必先给 A。
        let lim = BackgroundHeavyLimiter::new(1);
        let token = CancellationToken::new();
        let held = lim.acquire(&token).unwrap();

        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

        let mk = |name: &'static str, delay_ms: u64| {
            let lim = Arc::clone(&lim);
            let token = token.clone();
            let order = Arc::clone(&order);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(delay_ms));
                let _p = lim.acquire(&token).unwrap();
                order.lock().unwrap().push(name);
                thread::sleep(Duration::from_millis(10));
            })
        };
        let a = mk("A", 0);
        thread::sleep(Duration::from_millis(30)); // 确保 A 先入队
        let b = mk("B", 0);
        thread::sleep(Duration::from_millis(30));
        drop(held); // 释放 → 应先给 A

        a.join().unwrap();
        b.join().unwrap();
        let got = order.lock().unwrap().clone();
        assert_eq!(got, vec!["A", "B"], "FIFO：先入队者先获 permit");
    }

    #[test]
    fn caller_cancellation_removes_ticket_without_holding_limiter_lock() {
        let lim = BackgroundHeavyLimiter::new(1);
        let token = CancellationToken::new();
        let held = lim.acquire(&token).unwrap();
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let (checked_tx, checked_rx) = crossbeam_channel::bounded(1);
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        let finished = thread::scope(|scope| {
            scope.spawn(|| {
                let result = lim.acquire_cancellable(&|| {
                    assert!(lim.state.try_lock().is_ok(), "调用方判定不能持有额度锁");
                    let _ = checked_tx.try_send(());
                    cancelled.load(Ordering::SeqCst)
                });
                let _ = done_tx.send(result.is_none());
            });
            let checked = checked_rx.recv_timeout(Duration::from_secs(2));
            cancelled.store(true, Ordering::SeqCst);
            let finished = done_rx.recv_timeout(Duration::from_secs(1));
            // 失败路径也释放持有者，防止回归卡住测试进程。
            drop(held);
            assert!(checked.is_ok());
            finished
        });
        assert_eq!(finished, Ok(true), "取消不能等到额度归还才生效");
        assert!(lim.state.lock().unwrap().queue.is_empty());
        assert_eq!(lim.available(), 1);
    }
}

#[cfg(test)]
mod gpu_token_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    #[test]
    fn amount_fixed_at_one_with_raii_release() {
        let gpu = GpuToken::new();
        let token = CancellationToken::new();
        assert!(gpu.is_idle());
        let held = gpu.acquire(&token).expect("空闲时必得");
        assert!(!gpu.is_idle(), "额度 1:持有期间不空闲");

        // 第二个请求在持有期内必须阻塞,释放后立即获得。
        let got_after_release = Arc::new(AtomicBool::new(false));
        let gpu2 = Arc::clone(&gpu);
        let token2 = token.clone();
        let flag = Arc::clone(&got_after_release);
        let waiter = thread::spawn(move || {
            let _p = gpu2.acquire(&token2).expect("释放后应获得");
            flag.store(true, Ordering::SeqCst);
        });
        thread::sleep(Duration::from_millis(50));
        assert!(
            !got_after_release.load(Ordering::SeqCst),
            "持有期内第二请求不得放行"
        );
        drop(held); // RAII 释放
        waiter.join().unwrap();
        assert!(got_after_release.load(Ordering::SeqCst));
        assert!(gpu.is_idle(), "全部归还");
    }

    #[test]
    fn released_on_panic_path() {
        // D2 §4 验收②:panic 展开路径同样经 Drop 释放,无泄漏面。
        let gpu = GpuToken::new();
        let token = CancellationToken::new();
        let gpu2 = Arc::clone(&gpu);
        let panicker = thread::spawn(move || {
            let _p = gpu2.acquire(&token).unwrap();
            panic!("模拟批处理线程 panic");
        });
        assert!(panicker.join().is_err(), "线程应 panic");
        assert!(gpu.is_idle(), "panic 展开后令牌应已释放");
    }

    #[test]
    fn fifo_two_waiters_served_in_order() {
        // D2 §4 验收①:薄封装不得破坏内层 FIFO 公平。
        let gpu = GpuToken::new();
        let token = CancellationToken::new();
        let held = gpu.acquire(&token).unwrap();
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let mk = |name: &'static str| {
            let gpu = Arc::clone(&gpu);
            let token = token.clone();
            let order = Arc::clone(&order);
            thread::spawn(move || {
                let _p = gpu.acquire(&token).unwrap();
                order.lock().unwrap().push(name);
            })
        };
        let a = mk("A");
        thread::sleep(Duration::from_millis(30)); // 确保 A 先入队
        let b = mk("B");
        thread::sleep(Duration::from_millis(30));
        drop(held); // 释放 → 必先给 A
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(*order.lock().unwrap(), vec!["A", "B"]);
    }

    #[test]
    fn cancel_while_waiting_returns_none_without_leak() {
        let gpu = GpuToken::new();
        let token = CancellationToken::new();
        let held = gpu.acquire(&token).unwrap();
        let gpu2 = Arc::clone(&gpu);
        let token2 = token.clone();
        let waiter = thread::spawn(move || gpu2.acquire(&token2).is_none());
        thread::sleep(Duration::from_millis(50));
        token.cancel();
        assert!(waiter.join().unwrap(), "取消应返回 None");
        drop(held);
        assert!(gpu.is_idle(), "无票泄漏");
    }
}
