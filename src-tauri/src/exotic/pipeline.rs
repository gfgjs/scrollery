// src-tauri/src/exotic/pipeline.rs
//! 冷门格式插件 · 任务流水线（v3 Part2 §4.2-4.4）。
//!
//! ```text
//! Claimer ── bounded Task channel ──> Worker 池（每线程 1 Supervisor）
//!                                          │ run_thumbnail（取共享 permit R4）
//!                                          v
//! Writer/Sink <── bounded Result channel ──┘ 原子落盘 + 条件 DB 更新（R2）+ layout + 合并事件
//! ```
//!
//! 关键不变量：
//!   - **原子领取 + 租约**（R2）：claim 一句 UPDATE...RETURNING；finish/fail 带 `status=1 AND lease_owner`，
//!     finish 还校验 claim 后读取的 `(source_revision, cache_key)` 快照。
//!   - **让步**（R1）：Claimer 派发新任务前 `should_yield_exotic()`（scan/thumbnail/interaction）；
//!     在途解码不 sleep 抢占，只自然完成或超时 kill。
//!   - **公平后台重活池**（R4）：每次 run 前取 `BackgroundHeavyLimiter` permit（与 derivation 同预算）。
//!   - **受租约保护的发布**（§4.4）：锁外准备临时文件，条件写事务内 rename，随后提交 DB。
//!   - **熔断**：进程级/协议级失败计 strike；坏数据（unsupported/malformed）不计 strike。
//!
//! Worker 经 [`ThumbnailWorker`] trait 抽象 → 单测用 mock worker + 内存 DB，不起真实进程。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use rusqlite::Connection;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use exotic_protocol::{RequestBody, WorkerErrorCode};

use crate::db::queries as q;
use crate::exotic::fingerprint::thumbnail_fingerprint;
use crate::exotic::limiter::BackgroundHeavyLimiter;
use crate::exotic::sink::prepare_thumbnail;
use crate::exotic::worker::{default_thumbnail_limits, TaskOutcome, WorkerLimits};

// U-P3(2026-07-16):Worker trait 分类学拆至 `super::worker_traits`(EmbedWorker 只服务
// AI 层,流水线用不到);此处 re-export 保住既有 `exotic::pipeline::{WorkerTask, ...}`
// 引用路径(消费方零迁移)。
pub use super::worker_traits::*;

// ── 常量 ───────────────────────────────────────────────────────────────────────
/// 单任务超时（PSD 缩略图很快；给足余量覆盖大画布）。
const TASK_TIMEOUT: Duration = crate::exotic::coordinator::op_timeouts::THUMBNAIL; // 收拢 per-op 表(T13/D3)
/// 租约 TTL：≥ task_timeout + kill/wait 宽限；孤儿恢复只回收超此时长的 processing。
const LEASE_TTL_SECS: i64 = 120;
/// 在途租约续租周期（须 << lease_ttl；取 ttl/3，R2 第4条）。
const RENEW_INTERVAL: Duration = Duration::from_secs((LEASE_TTL_SECS / 3) as u64);
/// 领取批大小：对齐可派发容量（pool + channel），避免一次标过多 processing（R4 规则4 / §4.2）。
/// 空批即结束本轮，非空则继续多轮领取——小批多轮不会饿，但不会超量占用租约。
const CLAIM_BATCH: i64 = (MAX_POOL as i64) * 2;
/// 让步轮询周期。
const YIELD_POLL: Duration = Duration::from_millis(200);
/// 崩溃后重启退避（防快速重启风暴）。
const CRASH_BACKOFF: Duration = Duration::from_millis(500);
/// 重试上限（崩溃/超时）。
const MAX_ATTEMPTS: i64 = 3;
/// 插件熔断 strike 阈值（进程级/协议级失败累计）；单轮计数，跨轮冷却由 coordinator 负责。
const STRIKE_THRESHOLD: u32 = 5;
/// Worker 池上限（Part2：PSD 快，permits 已封顶并发，进程数无需多）。
const MAX_POOL: usize = 2;

// ── 流水线依赖与统计 ─────────────────────────────────────────────────────────────

/// 流水线运行所需依赖（从 AppState 拆出具体部件，便于单测注入内存 DB）。
pub struct PipelineDeps<'a> {
    /// 写连接（claim/finish/fail/recover 都在此）。测试中读写同一连接。
    pub writer: &'a Mutex<Connection>,
    pub limiter: &'a Arc<BackgroundHeavyLimiter>,
    pub token: &'a CancellationToken,
    /// items 取数缓存：缩略图结果就地 patch（S3 后唯一 patch 目标——布局行仅存几何，
    /// 出口拼装自本缓存取载荷；真实接线 = AppState 字段）。
    pub items_cache: &'a crate::layout::items_cache::ItemsCacheSlot,
    pub cache_dir: PathBuf,
    /// 当前缩略图档位请求尺寸（吸附在指纹/Worker 内做）。
    pub requested_size: u32,
    pub plugin_id: String,
    /// 有状态变更时每 500ms 合并通知，退出补发（媒体刷新与任务状态变化）。
    pub on_progress: Arc<dyn Fn() + Send + Sync>,
    /// 让步判定（R1）：scan/thumbnail/interaction 活动时为 true。真实接线注入
    /// `state.should_yield_exotic()`；测试默认返回 false。
    pub should_yield: Arc<dyn Fn() -> bool + Send + Sync>,
    /// 派发前授权复核（§5.3：「每批领取前」校验）。真实接线注入
    /// `host.is_task_runnable(plugin_id, capability)`；运行期 License 失效/插件禁用/卸载后
    /// 返回 false → Claimer 停领新批（在途自然完成）。测试默认 true。
    pub is_runnable: Arc<dyn Fn() -> bool + Send + Sync>,
}

/// 流水线退出原因；阻塞与熔断由协调器冷却，取消不消耗失败预算。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStop {
    #[default]
    Drained,
    Blocked,
    Cancelled,
    CircuitOpen,
}

/// 一次运行统计。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PipelineStats {
    pub done: u64,
    pub retried: u64,
    pub terminal: u64,
    pub lease_lost: u64,
    pub stop: PipelineStop,
}

const CAPABILITY: &str = "thumbnail";

/// 进程级唯一 instance_id（pid + 纳秒 + 计数；仅内存，做 lease_owner）。
fn new_instance_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        nanos,
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Coordinator 每次 wake 评估前的孤儿租约清扫(2026-07-05 内测病历 #2)。
///
/// 恢复代码原本只存在于 run 的步骤 0——而「是否 run」由 has_ready(只认 pending/
/// retryable)决定:硬杀/管线 panic 遗留的 stale processing 行使 has_ready 恒 false,
/// pipeline 不再启动、步骤 0 永不可达,进度从此永久停摆(真机实证:卡 60/62)。
/// 在 wake 评估前清扫一次,过期租约回 pending 后 has_ready 恢复真值、pipeline 重新可启。
/// 只回收超过 LEASE_TTL_SECS 的租约:活实例经 renew_loop 以 TTL/3 周期续租,不受影响;
/// 极端租约丢失(如线程阻塞超 TTL)由 finish/fail 的 `status=1 AND lease_owner` 条件
/// 更新兜底——迟到结果只会被丢弃(计 lease_lost),不会双写。
pub(crate) fn recover_stale_exotic_leases(writer: &Mutex<Connection>) -> usize {
    let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
    match q::recover_orphaned_exotic_tasks(&conn, LEASE_TTL_SECS, now_secs()) {
        Ok(n) => {
            if n > 0 {
                info!("exotic:wake 前清扫恢复 {n} 个过期租约 processing→pending");
            }
            n
        }
        Err(e) => {
            warn!("exotic:wake 前孤儿清扫失败:{e}");
            0
        }
    }
}

/// 一个已领取、已构造请求的任务。
struct ClaimedTask {
    task_id: i64,
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    fingerprint: String,
    tier: u32,
    request: RequestBody,
    attempts: i64,
}

/// Worker → Writer 的结果。
struct WorkerResult {
    task: ClaimedTask,
    outcome: TaskOutcome,
}

/// 运行整条流水线（阻塞；Coordinator 在 spawn_blocking 内调用，或测试直接调用）。
///
/// 返回前保证：清理本实例残留租约（取消时把已领取未最终化的任务退回 pending）。
pub fn run_exotic_pipeline_blocking(
    deps: &PipelineDeps<'_>,
    factory: &dyn WorkerFactory,
) -> PipelineStats {
    if deps.token.is_cancelled() {
        return PipelineStats {
            stop: PipelineStop::Cancelled,
            ..Default::default()
        };
    }
    let instance_id = new_instance_id();
    let plugin_id = deps.plugin_id.clone();

    // ── 0. 孤儿恢复：回收**过期**租约（不动其他活实例）。──
    {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        match q::recover_orphaned_exotic_tasks(&conn, LEASE_TTL_SECS, now_secs()) {
            Ok(n) if n > 0 => info!("exotic：恢复 {n} 个过期租约 processing→pending"),
            Ok(_) => {}
            Err(e) => warn!("exotic：孤儿恢复失败：{e}"),
        }
    }

    // ── 1. 先探一个 Worker 拿 worker_version（指纹需要；失败则本次不处理，任务留 pending）。──
    let probe = match factory.spawn() {
        Ok(w) => w,
        Err(e) => {
            warn!("exotic：无法创建 Worker（{e}）→ 本次跳过，任务保持 pending");
            return PipelineStats {
                stop: PipelineStop::Blocked,
                ..Default::default()
            };
        }
    };
    let worker_version = probe.worker_version();

    // ── 2. Worker 升级失效：把该插件执行版本变化的已完成/失败任务退回 pending。──
    {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        match q::invalidate_exotic_tasks_for_plugin_version(&conn, &plugin_id, &worker_version) {
            Ok(n) if n > 0 => {
                info!("exotic：worker 升级重置 {n} 个缩略图任务（版本 {worker_version}）")
            }
            Ok(_) => {}
            Err(e) => {
                warn!("exotic：worker 版本失效失败：{e}");
                return PipelineStats {
                    stop: PipelineStop::Blocked,
                    ..Default::default()
                };
            }
        }
    }

    // ── 3. 通道 + 共享状态。──
    let pool_size = MAX_POOL.max(1);
    let (task_tx, task_rx) = bounded::<ClaimedTask>(pool_size * 2);
    let (result_tx, result_rx) = bounded::<WorkerResult>(pool_size * 2);
    // 探到的 Worker 放入种子槽，供池线程复用（避免二次 spawn）。
    let seed: Arc<Mutex<Vec<Box<dyn ThumbnailWorker>>>> = Arc::new(Mutex::new(vec![probe]));
    // 熔断闸：停止领取和派发新请求，在途结果仍按原租约收尾。
    let circuit_open = Arc::new(AtomicBool::new(false));

    let stats = Arc::new(Mutex::new(PipelineStats::default()));
    let limits = default_thumbnail_limits();

    std::thread::scope(|s| {
        // 续租线程（R2，问题2）：周期刷新本实例在途租约。退出靠 writer 结束时 drop renew_tx
        // → recv 返回 Disconnected，从而 scope 能正常 join（不依赖 token 取消）。
        let (renew_tx, renew_rx) = bounded::<()>(1);
        {
            let instance_id = instance_id.clone();
            s.spawn(move || renew_loop(deps, &instance_id, renew_rx));
        }

        // Claimer
        {
            let stats = Arc::clone(&stats);
            let circuit_open = Arc::clone(&circuit_open);
            let plugin_id = plugin_id.clone();
            let worker_version = worker_version.clone();
            let instance_id = instance_id.clone();
            s.spawn(move || {
                claimer_loop(
                    deps,
                    &plugin_id,
                    &worker_version,
                    &instance_id,
                    task_tx,
                    &circuit_open,
                    &stats,
                );
            });
        }

        // Worker 池
        for _ in 0..pool_size {
            let task_rx = task_rx.clone();
            let result_tx = result_tx.clone();
            let seed = Arc::clone(&seed);
            let limits = limits.clone();
            let circuit_open = Arc::clone(&circuit_open);
            s.spawn(move || {
                worker_loop(
                    deps,
                    factory,
                    &seed,
                    task_rx,
                    result_tx,
                    &limits,
                    &circuit_open,
                );
            });
        }
        drop(task_rx);
        drop(result_tx);

        // Writer/Sink
        {
            let circuit_open = Arc::clone(&circuit_open);
            let plugin_id = plugin_id.clone();
            let worker_version = worker_version.clone();
            let instance_id = instance_id.clone();
            let stats = Arc::clone(&stats);
            let renew_tx = renew_tx; // move：writer 退出即关停续租线程（唯一持有者）
            s.spawn(move || {
                writer_loop(
                    deps,
                    &plugin_id,
                    &worker_version,
                    &instance_id,
                    result_rx,
                    &circuit_open,
                    &stats,
                );
                drop(renew_tx);
            });
        }
    });

    // ── 结束清理：释放本实例残留租约（取消时把已领取未最终化的任务退回 pending）。──
    {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        match q::release_exotic_instance_leases(&conn, &instance_id) {
            Ok(n) if n > 0 => info!("exotic：释放 {n} 个残留租约 → pending"),
            Ok(_) => {}
            Err(e) => warn!("exotic：释放残留租约失败：{e}"),
        }
    }

    let mut out = stats.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if deps.token.is_cancelled() {
        out.stop = PipelineStop::Cancelled;
    } else if circuit_open.load(Ordering::SeqCst) {
        out.stop = PipelineStop::CircuitOpen;
    }
    out
}

/// Claimer：让步门控 → 原子领取 → 构造请求 → 入 task channel。空批即结束（关闭 channel）。
#[allow(clippy::too_many_arguments)]
fn claimer_loop(
    deps: &PipelineDeps<'_>,
    plugin_id: &str,
    worker_version: &str,
    instance_id: &str,
    task_tx: Sender<ClaimedTask>,
    circuit_open: &AtomicBool,
    stats: &Mutex<PipelineStats>,
) {
    loop {
        if deps.dispatch_stopped(circuit_open) {
            break;
        }
        // R1 让步：scan/thumbnail/interaction 活动时暂缓领取新任务（在途不抢占）。
        while deps.should_yield() && !deps.dispatch_stopped(circuit_open) {
            std::thread::sleep(YIELD_POLL);
        }
        if deps.dispatch_stopped(circuit_open) {
            break;
        }

        let claimed = {
            let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
            q::claim_exotic_tasks(
                &conn,
                plugin_id,
                CAPABILITY,
                CLAIM_BATCH,
                instance_id,
                now_secs(),
                worker_version,
            )
        };
        // Writer 先取统计锁再写库；领取失败也必须释放 DB 锁后才更新统计，避免反向等待。
        let claimed = match claimed {
            Ok(rows) => rows,
            Err(e) => {
                warn!("exotic：领取失败：{e}");
                stats.lock().unwrap_or_else(|e| e.into_inner()).stop = PipelineStop::Blocked;
                break;
            }
        };
        if claimed.is_empty() {
            break; // 无更多就绪任务 → 本次结束（新任务由 Coordinator 重新唤醒）
        }

        for row in claimed {
            if deps.dispatch_stopped(circuit_open) {
                return;
            }
            // 取源信息 → 指纹 → 构造请求。
            let src = {
                let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
                q::exotic_item_source(&conn, row.item_id)
            };
            let src = match src {
                Ok(s) => s,
                Err(e) => {
                    // 源不可读（item 删除等）→ 标 retryable io，跳过。
                    warn!("exotic：item {} 源不可读：{e}", row.item_id);
                    let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
                    let failed = q::fail_exotic_task(
                        &conn,
                        row.id,
                        instance_id,
                        true,
                        MAX_ATTEMPTS,
                        WorkerErrorCode::IoError.as_str(),
                        "源不可读",
                        now_secs() + 30,
                    );
                    drop(conn);
                    stats
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .record_failure(failed);
                    continue;
                }
            };
            let fp = thumbnail_fingerprint(
                src.cache_key,
                plugin_id,
                worker_version,
                deps.requested_size,
            );
            let request = RequestBody::Thumbnail {
                item_id: row.item_id,
                source_path: src.abs_path,
                target_long_edge: fp.tier,
                input_fingerprint: fp.fingerprint.clone(),
            };
            let task = ClaimedTask {
                task_id: row.id,
                item_id: row.item_id,
                source_revision: src.source_revision,
                cache_key: src.cache_key,
                fingerprint: fp.fingerprint,
                tier: fp.tier,
                request,
                attempts: row.attempts,
            };
            if task_tx.send(task).is_err() {
                return; // 下游已结束
            }
        }
    }
    // task_tx 在此 drop → Worker 池收到 channel 关闭后退出。
}

/// Worker 线程：取任务 → 取 permit → 确保活 Worker → run → 释放 permit → 送结果。
fn worker_loop(
    deps: &PipelineDeps<'_>,
    factory: &dyn WorkerFactory,
    seed: &Arc<Mutex<Vec<Box<dyn ThumbnailWorker>>>>,
    task_rx: Receiver<ClaimedTask>,
    result_tx: Sender<WorkerResult>,
    limits: &WorkerLimits,
    circuit_open: &AtomicBool,
) {
    let mut worker: Option<Box<dyn ThumbnailWorker>> = None;
    'tasks: for task in task_rx {
        if deps.dispatch_stopped(circuit_open) {
            break;
        }
        let permit = loop {
            // 等待额度期间也可能出现前台活动；取到后再检查，等待让步时不占共享额度。
            while deps.should_yield() && !deps.dispatch_stopped(circuit_open) {
                std::thread::sleep(YIELD_POLL);
            }
            if deps.dispatch_stopped(circuit_open) {
                break 'tasks;
            }
            let Some(permit) = deps.limiter.acquire_cancellable(&|| {
                deps.dispatch_stopped(circuit_open) || deps.should_yield()
            }) else {
                // 让步时退队等前台空闲；停止派发则由下一轮的同一判定退出。
                continue;
            };
            if deps.should_yield() {
                drop(permit);
                continue;
            }
            if deps.dispatch_stopped(circuit_open) {
                break 'tasks;
            }

            // 确保有活 Worker：先用种子，再 factory.spawn（带崩溃退避）。
            if worker.as_ref().map(|w| !w.is_alive()).unwrap_or(true) {
                worker = None;
                let seeded = seed.lock().unwrap_or_else(|e| e.into_inner()).pop();
                worker = match seeded {
                    Some(w) => Some(w),
                    None => match factory.spawn() {
                        Ok(w) => Some(w),
                        Err(e) => {
                            drop(permit);
                            if deps.dispatch_stopped(circuit_open) {
                                break 'tasks;
                            }
                            warn!("exotic：补充 Worker 失败：{e} → 任务按断开重试");
                            let _ = result_tx.send(WorkerResult {
                                task,
                                outcome: TaskOutcome::Disconnected,
                            });
                            std::thread::sleep(CRASH_BACKOFF);
                            continue 'tasks;
                        }
                    },
                };
            }

            // 握手可能耗时数秒；其间出现停止/前台活动时，不能沿用取额度时的资格。
            if deps.dispatch_stopped(circuit_open) {
                break 'tasks;
            }
            if deps.should_yield() {
                drop(permit);
                continue;
            }
            break permit;
        };

        let w = worker.as_mut().unwrap();
        let cancelled = || deps.token.is_cancelled();
        let outcome = w.run_thumbnail(&task.request, limits, TASK_TIMEOUT, &cancelled);
        let dead = !w.is_alive();
        drop(permit); // 任务结束即释放额度

        // stop / App 退出（R1 在途边界 / v3.1 §4.1）：取消时在途 Worker 已被 kill（run 返回
        // Disconnected → Supervisor kill_and_reap）。丢弃结果、不落库；任务保持 processing(1) →
        // 由结束清理 release_exotic_instance_leases 退回 pending（不计退避、立即可重领）。
        if deps.token.is_cancelled() {
            break;
        }

        if result_tx.send(WorkerResult { task, outcome }).is_err() {
            break;
        }
        if dead {
            worker = None; // 下次循环重建（带退避）
            std::thread::sleep(CRASH_BACKOFF);
        }
    }
    if let Some(w) = worker.take() {
        w.shutdown(Duration::from_secs(2));
    }
}

/// 续租线程（R2）：周期续租本实例所有在途 processing，防排队中任务被第二实例孤儿恢复误回收。
/// 退出：writer 结束 drop renew_tx → `recv_timeout` 返回 Disconnected；或 token 取消。
fn renew_loop(deps: &PipelineDeps<'_>, instance_id: &str, shutdown: Receiver<()>) {
    loop {
        match shutdown.recv_timeout(RENEW_INTERVAL) {
            // tx 被 drop（writer 退出）或意外收到信号 → 收尾退出。
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if deps.token.is_cancelled() {
            return;
        }
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        match q::renew_all_exotic_leases(&conn, instance_id, now_secs()) {
            Ok(n) if n > 0 => debug!("exotic：续租 {n} 个在途任务"),
            Ok(_) => {}
            Err(e) => warn!("exotic：续租失败：{e}"),
        }
    }
}

/// Writer/Sink：落盘 + 条件 DB 更新 + layout + strike/熔断 + 合并事件。
fn writer_loop(
    deps: &PipelineDeps<'_>,
    plugin_id: &str,
    worker_version: &str,
    instance_id: &str,
    result_rx: Receiver<WorkerResult>,
    circuit_open: &AtomicBool,
    stats: &Mutex<PipelineStats>,
) {
    let mut strikes: u32 = 0;
    let mut changed = false;
    let mut last_notify = Instant::now();
    const NOTIFY_INTERVAL: Duration = Duration::from_millis(500);

    loop {
        let res = match result_rx.recv_timeout(NOTIFY_INTERVAL) {
            Ok(res) => res,
            Err(RecvTimeoutError::Timeout) => {
                if changed {
                    (deps.on_progress)();
                    changed = false;
                    last_notify = Instant::now();
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        changed = true;
        let WorkerResult { task, outcome } = res;
        let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
        match outcome {
            TaskOutcome::Success {
                width,
                height,
                blob,
                thumbhash,
                ..
            } => {
                debug!("exotic：item {} 出图 {}x{}", task.item_id, width, height);
                match finalize_success(deps, &task, worker_version, instance_id, &blob, &thumbhash)
                {
                    Ok(true) => {
                        s.done += 1;
                    }
                    Ok(false) => {
                        // 租约或源已失效：丢弃临时文件，不替换当前文件。
                        s.lease_lost += 1;
                    }
                    Err(e) => {
                        // 落盘/DB 失败 → 退回 retryable（不丢任务）。
                        warn!("exotic：item {} 落盘失败：{e}", task.item_id);
                        fail_task(
                            deps,
                            &task,
                            instance_id,
                            true,
                            WorkerErrorCode::IoError,
                            "sink 失败",
                            &mut s,
                        );
                    }
                }
            }
            TaskOutcome::Failure(body) => {
                // 数据类错误（unsupported/malformed/resource）→ terminal，不计 strike；
                // io/internal → retryable。
                let retryable = body.retryable && body.code.default_retryable();
                fail_task(
                    deps,
                    &task,
                    instance_id,
                    retryable,
                    body.code,
                    &body.message,
                    &mut s,
                );
            }
            TaskOutcome::TimedOut | TaskOutcome::Disconnected => {
                // 进程级失败 → retryable + strike。
                strikes += 1;
                fail_task(
                    deps,
                    &task,
                    instance_id,
                    true,
                    WorkerErrorCode::InternalError,
                    "worker 超时/断开",
                    &mut s,
                );
            }
            TaskOutcome::Protocol(reason) => {
                // 协议级/非法输出 → terminal invalid_worker_output + strike。
                strikes += 1;
                warn!("exotic：item {} 协议违例：{reason}", task.item_id);
                fail_task(
                    deps,
                    &task,
                    instance_id,
                    false,
                    WorkerErrorCode::InternalError,
                    "invalid_worker_output",
                    &mut s,
                );
            }
        }
        drop(s);
        if last_notify.elapsed() >= NOTIFY_INTERVAL {
            (deps.on_progress)();
            changed = false;
            last_notify = Instant::now();
        }

        if strikes >= STRIKE_THRESHOLD && !circuit_open.load(Ordering::SeqCst) {
            warn!("exotic：插件 {plugin_id} strike 达 {strikes} → 本次熔断，停止领取与派发");
            circuit_open.store(true, Ordering::SeqCst);
        }
    }

    if changed {
        (deps.on_progress)();
    }
}

/// 成功路径：准备临时文件 → 条件事务内发布 → 提交 → items cache。
/// 返回 Ok(true)=本实例落库成功；Ok(false)=租约已失（丢弃）；Err=落盘/DB 错误。
fn finalize_success(
    deps: &PipelineDeps<'_>,
    task: &ClaimedTask,
    worker_version: &str,
    instance_id: &str,
    blob: &[u8],
    thumbhash: &[u8],
) -> crate::error::Result<bool> {
    // 0. 先做短快照预检并立即释放 writer 锁。source_revision/cache_key 已变化时，
    // 直接丢弃迟到结果，避免无用文件 IO；最终 DB CAS 在发布前复核源与租约。
    let source_current = {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        q::is_exotic_source_current(&conn, task.item_id, task.source_revision, task.cache_key)?
    };
    if !source_current {
        return Ok(false);
    }

    // 1. 慢文件 IO 在锁外准备，暂不替换最终文件。ThumbHash 已由宿主受限解码计算。
    let sink = prepare_thumbnail(&deps.cache_dir, task.tier, task.cache_key, blob)?;

    // 2. 条件 UPDATE 取得跨进程 SQLite 写锁后才允许 rename；提交前其他 owner 无法抢租约。
    {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.unchecked_transaction()?;
        let ok = q::finish_exotic_task(
            &tx,
            task.task_id,
            instance_id,
            task.source_revision,
            task.cache_key,
            &task.fingerprint,
            &sink.thumb_db_path,
            worker_version,
        )?;
        let thumb_applied = if ok {
            q::update_thumb_result_if_current(
                &tx,
                task.item_id,
                task.source_revision,
                task.cache_key,
                1,
                Some(&sink.thumb_db_path),
                Some(thumbhash),
            )? == 1
        } else {
            false
        };
        if !ok || !thumb_applied {
            return Ok(false);
        }
        sink.publish()?;
        tx.commit()?;
    }

    // 3. 同步 items 取数缓存（S3：布局行仅存几何，出口拼装自 items 缓存取载荷——
    //    patch 单点即可使产物在滚出再滚回时立即可见，无需整表重算）。
    let thumb = crate::db::models::ThumbResult {
        item_id: task.item_id,
        thumb_status: 1,
        thumb_path: Some(sink.thumb_db_path.clone()),
        thumbhash: Some(thumbhash.to_vec()),
        source_revision: task.source_revision,
        cache_key: task.cache_key,
    };
    {
        let slot = deps.items_cache.read().unwrap_or_else(|e| e.into_inner());
        crate::layout::items_cache::apply_thumb_results(&slot, std::slice::from_ref(&thumb));
    }
    Ok(true)
}

/// 失败路径：条件 fail（retryable 计退避，terminal 不退避）。
fn fail_task(
    deps: &PipelineDeps<'_>,
    task: &ClaimedTask,
    instance_id: &str,
    retryable: bool,
    code: WorkerErrorCode,
    message: &str,
    stats: &mut PipelineStats,
) {
    let next_retry_at = if retryable {
        now_secs() + backoff_secs(task.attempts, code)
    } else {
        0
    };
    let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
    stats.record_failure(q::fail_exotic_task(
        &conn,
        task.task_id,
        instance_id,
        retryable,
        MAX_ATTEMPTS,
        code.as_str(),
        message,
        next_retry_at,
    ));
}

impl PipelineStats {
    fn record_failure(&mut self, result: crate::error::Result<q::ExoticFailureOutcome>) {
        match result {
            Ok(q::ExoticFailureOutcome::RetryScheduled) => self.retried += 1,
            Ok(q::ExoticFailureOutcome::Terminal) => self.terminal += 1,
            Ok(q::ExoticFailureOutcome::LeaseLost) => self.lease_lost += 1,
            Err(e) => {
                warn!("exotic：标记失败写库错误：{e}");
                self.stop = PipelineStop::Blocked;
            }
        }
    }
}

/// 退避秒数：进程级（internal）按 1m/5m/30m；io 按 30s 指数。
fn backoff_secs(attempts: i64, code: WorkerErrorCode) -> i64 {
    match code {
        WorkerErrorCode::IoError => 30 * (1 << attempts.clamp(0, 5)),
        _ => match attempts {
            0 => 60,
            1 => 300,
            _ => 1800,
        },
    }
}

impl<'a> PipelineDeps<'a> {
    /// 仅约束尚未开始的请求；在途请求是否中断仍由用户取消 token 决定。
    fn dispatch_stopped(&self, circuit_open: &AtomicBool) -> bool {
        self.token.is_cancelled() || circuit_open.load(Ordering::SeqCst) || !(self.is_runnable)()
    }

    /// 让步判定（注入闭包；测试默认不让步）。
    fn should_yield(&self) -> bool {
        (self.should_yield)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::worker::{WorkerConfig, WorkerSpec};
    use exotic_protocol::FailureBody;
    use std::io::Cursor;

    const PID: &str = "exotic-image-psd";

    /// 测试用请求档位：绑 THUMB_TIERS 事实源。管线内部对 requested_size 做 snap_to_tier 后
    /// 落盘，thumb_path 又会断言档位合法——硬编码数值在 b554aa5「档位重定」后即失效
    /// （详见 exotic/fingerprint.rs 测试模块的同名常量注释）。取梯上已有值 → snap 后即自身。
    const TIER: u32 = crate::thumbnail::generator::THUMB_TIERS[3];

    /// mock Worker 行为。
    #[derive(Clone)]
    enum Behavior {
        Success,
        SuccessAndOpenCircuit(Arc<AtomicBool>),
        Failure {
            code: WorkerErrorCode,
            retryable: bool,
        },
        Timeout,
    }

    struct MockWorker {
        behavior: Behavior,
        dead: bool,
    }
    impl WorkerTask for MockWorker {
        fn worker_version(&self) -> String {
            "mock-1.0.0".into()
        }
        fn is_alive(&self) -> bool {
            !self.dead
        }
        fn shutdown(self: Box<Self>, _grace: Duration) {}
    }
    impl ThumbnailWorker for MockWorker {
        fn run_thumbnail(
            &mut self,
            req: &RequestBody,
            _limits: &WorkerLimits,
            _timeout: Duration,
            cancelled: &dyn Fn() -> bool,
        ) -> TaskOutcome {
            match self.behavior.clone() {
                Behavior::Success | Behavior::SuccessAndOpenCircuit(_) => {
                    if let Behavior::SuccessAndOpenCircuit(circuit) = &self.behavior {
                        circuit.store(true, Ordering::SeqCst);
                        assert!(!cancelled(), "熔断不应中断已在途请求");
                    }
                    TaskOutcome::Success {
                        width: 480,
                        height: 240,
                        mime: "image/webp".into(),
                        blob: make_webp(480, 240),
                        thumbhash: vec![1, 2, 3],
                    }
                }
                Behavior::Failure { code, retryable } => TaskOutcome::Failure(FailureBody {
                    item_id: req.item_id(),
                    input_fingerprint: req.input_fingerprint().map(String::from),
                    code,
                    retryable,
                    message: "mock".into(),
                }),
                Behavior::Timeout => {
                    self.dead = true; // 模拟 supervisor 超时后 kill
                    TaskOutcome::TimedOut
                }
            }
        }
    }

    struct MockFactory {
        behavior: Behavior,
    }
    impl WorkerFactory for MockFactory {
        fn spawn(&self) -> Result<Box<dyn ThumbnailWorker>, String> {
            Ok(Box::new(MockWorker {
                behavior: self.behavior.clone(),
                dead: false,
            }))
        }
    }

    fn make_webp(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([12, 34, 56, 255]));
        let mut buf = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut buf))
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    fn claim_test_task(conn: &Connection, item_id: i64, owner: &str) -> ClaimedTask {
        let row = q::claim_exotic_tasks(conn, PID, CAPABILITY, 1, owner, 1000, "mock-1.0.0")
            .unwrap()
            .pop()
            .unwrap();
        let src = q::exotic_item_source(conn, item_id).unwrap();
        ClaimedTask {
            task_id: row.id,
            item_id,
            source_revision: src.source_revision,
            cache_key: src.cache_key,
            fingerprint: "fp".into(),
            tier: TIER,
            attempts: row.attempts,
            request: RequestBody::Thumbnail {
                item_id,
                source_path: src.abs_path,
                target_long_edge: TIER,
                input_fingerprint: "fp".into(),
            },
        }
    }

    #[test]
    fn startup_failure_exits_without_claiming_or_retrying() {
        struct FailingFactory(std::sync::atomic::AtomicUsize);
        impl WorkerFactory for FailingFactory {
            fn spawn(&self) -> Result<Box<dyn ThumbnailWorker>, String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err("unavailable".into())
            }
        }
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let factory = FailingFactory(std::sync::atomic::AtomicUsize::new(0));
        assert_eq!(
            run_exotic_pipeline_blocking(&d, &factory).stop,
            PipelineStop::Blocked
        );
        assert_eq!(factory.0.load(Ordering::SeqCst), 1);
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 0);
        token.cancel();
        assert_eq!(
            run_exotic_pipeline_blocking(&d, &factory).stop,
            PipelineStop::Cancelled
        );
        assert_eq!(factory.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn foreground_activity_after_permit_prevents_new_decode() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let task = claim_test_task(&conn, item_id, "A");
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let mut d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let cancelled = token.clone();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        d.should_yield = Arc::new(move || {
            if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                false
            } else {
                cancelled.cancel();
                true
            }
        });
        let (tasks, task_rx) = bounded(1);
        tasks.send(task).unwrap();
        drop(tasks);
        let (results, result_rx) = bounded(1);
        let factory = MockFactory {
            behavior: Behavior::Success,
        };
        worker_loop(
            &d,
            &factory,
            &Arc::new(Mutex::new(Vec::new())),
            task_rx,
            results,
            &default_thumbnail_limits(),
            &AtomicBool::new(false),
        );
        assert!(result_rx.try_recv().is_err(), "已让步的任务不得继续解码");
        assert_eq!(limiter.available(), limiter.total());
    }

    #[test]
    fn authorization_revoked_during_yield_exits_without_claiming() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let mut d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let allowed = Arc::new(AtomicBool::new(true));
        let observed = Arc::clone(&allowed);
        d.is_runnable = Arc::new(move || observed.load(Ordering::SeqCst));
        let (yielding, entered) = bounded(1);
        d.should_yield = Arc::new(move || {
            let _ = yielding.try_send(());
            true
        });
        let factory = MockFactory {
            behavior: Behavior::Success,
        };
        let (done_tx, done_rx) = bounded(1);
        let finished = std::thread::scope(|scope| {
            scope.spawn(|| {
                let _ = done_tx.send(run_exotic_pipeline_blocking(&d, &factory));
            });
            let entered_wait = entered.recv_timeout(Duration::from_secs(2));
            allowed.store(false, Ordering::SeqCst);
            let finished = done_rx.recv_timeout(Duration::from_secs(1));
            // 无论断言结果如何都解除旧实现的等待，不能让失败回归挂住整个测试进程。
            token.cancel();
            assert!(entered_wait.is_ok());
            finished
        });
        assert!(finished.is_ok(), "授权失效应结束让步等待，无需用户另点取消");
        let conn = writer.lock().unwrap();
        assert_eq!(task_status(&conn, item_id), 0);
        let attempts: i64 = conn
            .query_row(
                "SELECT attempts FROM exotic_tasks WHERE item_id=?1",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attempts, 0);
    }

    #[test]
    fn circuit_during_permit_wait_exits_without_new_decode() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let task = claim_test_task(&conn, item_id, "A");
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(1);
        let token = CancellationToken::new();
        let held = limiter.acquire(&token).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let (checked, entered) = bounded(1);
        d.is_runnable = Arc::new(move || {
            let _ = checked.try_send(());
            true
        });
        let (tasks, task_rx) = bounded(1);
        tasks.send(task).unwrap();
        drop(tasks);
        let (results, result_rx) = bounded(1);
        let seed = Arc::new(Mutex::new(Vec::new()));
        let limits = default_thumbnail_limits();
        let circuit = AtomicBool::new(false);
        let factory = MockFactory {
            behavior: Behavior::Success,
        };
        let (done_tx, done_rx) = bounded(1);
        let finished = std::thread::scope(|scope| {
            scope.spawn(|| {
                worker_loop(&d, &factory, &seed, task_rx, results, &limits, &circuit);
                let _ = done_tx.send(());
            });
            let entered_wait = entered.recv_timeout(Duration::from_secs(2));
            circuit.store(true, Ordering::SeqCst);
            let finished = done_rx.recv_timeout(Duration::from_secs(1));
            token.cancel();
            assert!(entered_wait.is_ok());
            finished
        });
        assert!(
            finished.is_ok(),
            "熔断应退出额度等待，不必等其他任务释放额度"
        );
        assert!(result_rx.try_recv().is_err());
        assert_eq!(limiter.available(), 0, "已有持有者不受影响");
        drop(held);
        assert_eq!(limiter.available(), 1);
    }

    #[test]
    fn authorization_revoked_during_spawn_prevents_new_decode() {
        struct RevokingFactory(Arc<AtomicBool>);
        impl WorkerFactory for RevokingFactory {
            fn spawn(&self) -> Result<Box<dyn ThumbnailWorker>, String> {
                self.0.store(false, Ordering::SeqCst);
                MockFactory {
                    behavior: Behavior::Success,
                }
                .spawn()
            }
        }
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let task = claim_test_task(&conn, item_id, "A");
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(1);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let mut d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let allowed = Arc::new(AtomicBool::new(true));
        let observed = Arc::clone(&allowed);
        d.is_runnable = Arc::new(move || observed.load(Ordering::SeqCst));
        let (tasks, task_rx) = bounded(1);
        tasks.send(task).unwrap();
        drop(tasks);
        let (results, result_rx) = bounded(1);
        worker_loop(
            &d,
            &RevokingFactory(allowed),
            &Arc::new(Mutex::new(Vec::new())),
            task_rx,
            results,
            &default_thumbnail_limits(),
            &AtomicBool::new(false),
        );
        assert!(result_rx.try_recv().is_err(), "握手后失去授权不得开始解码");
        assert_eq!(limiter.available(), 1);
    }

    #[test]
    fn in_flight_result_survives_circuit_opening() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let task = claim_test_task(&conn, item_id, "A");
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(1);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let circuit = Arc::new(AtomicBool::new(false));
        let factory = MockFactory {
            behavior: Behavior::SuccessAndOpenCircuit(Arc::clone(&circuit)),
        };
        let (tasks, task_rx) = bounded(1);
        tasks.send(task).unwrap();
        drop(tasks);
        let (results, result_rx) = bounded(1);
        worker_loop(
            &d,
            &factory,
            &Arc::new(Mutex::new(Vec::new())),
            task_rx,
            results,
            &default_thumbnail_limits(),
            &circuit,
        );
        assert!(circuit.load(Ordering::SeqCst));
        // 已在途的有效结果继续交给原 Writer，验证它仍能正常提交。
        let stats = Mutex::new(PipelineStats::default());
        writer_loop(&d, PID, "mock-1.0.0", "A", result_rx, &circuit, &stats);
        assert_eq!(stats.lock().unwrap().done, 1);
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 2);
        assert_eq!(limiter.available(), 1);
    }

    #[test]
    fn writer_notifies_before_close_and_counts_exhausted_retry_as_terminal() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        conn.execute("UPDATE exotic_tasks SET attempts=2", [])
            .unwrap();
        let task = claim_test_task(&conn, item_id, "A");
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let mut d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let (notified_tx, notified_rx) = std::sync::mpsc::channel();
        d.on_progress = Arc::new(move || {
            let _ = notified_tx.send(());
        });
        let (tx, rx) = bounded(1);
        let stats = Mutex::new(PipelineStats::default());
        let circuit = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| writer_loop(&d, PID, "mock-1.0.0", "A", rx, &circuit, &stats));
            tx.send(WorkerResult {
                task,
                outcome: TaskOutcome::Failure(FailureBody {
                    item_id: Some(item_id),
                    input_fingerprint: Some("fp".into()),
                    code: WorkerErrorCode::IoError,
                    retryable: true,
                    message: "busy".into(),
                }),
            })
            .unwrap();
            let notified = notified_rx.recv_timeout(Duration::from_secs(2));
            drop(tx);
            assert!(notified.is_ok(), "通道尚未结束时应已通知进度");
        });
        let stats = stats.into_inner().unwrap();
        assert_eq!((stats.retried, stats.terminal), (0, 1));
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 4);
    }

    #[test]
    fn publish_failure_rolls_back_done_and_removes_temporary_file() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, cache_key) = setup_db(&conn);
        let task = claim_test_task(&conn, item_id, "A");
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        // 最终路径是目录，制造 rename 失败，验证未提交的 done 会回滚。
        std::fs::create_dir_all(crate::thumbnail::cache::thumb_path(
            dir.path(),
            TIER,
            cache_key,
        ))
        .unwrap();
        assert!(finalize_success(&d, &task, "mock-1.0.0", "A", &make_webp(20, 20), &[1]).is_err());
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 1);
        assert!(!walkdir::WalkDir::new(dir.path())
            .into_iter()
            .filter_map(Result::ok)
            .any(|e| e.path().extension().is_some_and(|ext| ext == "tmp")));
    }

    /// 建库 + 插入 root/dir/media(psd) + 播种 thumbnail 任务。返回 (item_id, cache_key)。
    fn setup_db(conn: &Connection) -> (i64, i64) {
        crate::db::schema::initialize_schema(conn).unwrap();
        conn.execute(
            "INSERT INTO scan_roots (path, alias) VALUES (?1, 'r')",
            rusqlite::params![std::env::temp_dir().to_string_lossy().to_string()],
        )
        .unwrap();
        let root_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO directories (root_id, parent_id, rel_path, name, depth, mtime)
             VALUES (?1, NULL, '', 'root', 0, 0)",
            rusqlite::params![root_id],
        )
        .unwrap();
        let dir_id = conn.last_insert_rowid();
        let cache_key: i64 = 0x0BAD_F00D;
        conn.execute(
            "INSERT INTO media_items
                (directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, 'synthetic.psd', 1, 1, 'psd', 'image', 0, 0, 0, ?2)",
            rusqlite::params![dir_id, cache_key],
        )
        .unwrap();
        let item_id = conn.last_insert_rowid();
        q::seed_exotic_tasks_for_item(conn, item_id, PID, &["thumbnail".to_string()]).unwrap();
        (item_id, cache_key)
    }

    /// 测试共享的进程级 items 缓存（初始 None、测试不读它——真实接线见 coordinator）。
    fn test_items_cache() -> &'static crate::layout::items_cache::ItemsCacheSlot {
        static CACHE: std::sync::OnceLock<crate::layout::items_cache::ItemsCacheSlot> =
            std::sync::OnceLock::new();
        CACHE.get_or_init(crate::layout::items_cache::new_items_cache_slot)
    }

    fn deps<'a>(
        writer: &'a Mutex<Connection>,
        limiter: &'a Arc<BackgroundHeavyLimiter>,
        token: &'a CancellationToken,
        cache_dir: PathBuf,
    ) -> PipelineDeps<'a> {
        PipelineDeps {
            writer,
            limiter,
            token,
            items_cache: test_items_cache(),
            cache_dir,
            requested_size: TIER,
            plugin_id: PID.to_string(),
            on_progress: Arc::new(|| {}),
            should_yield: Arc::new(|| false),
            is_runnable: Arc::new(|| true),
        }
    }

    fn task_status(conn: &Connection, item_id: i64) -> i64 {
        conn.query_row(
            "SELECT status FROM exotic_tasks WHERE item_id=?1 AND capability='thumbnail'",
            rusqlite::params![item_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn success_writes_thumbnail_and_marks_done() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, cache_key) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let cache_dir = std::env::temp_dir().join(format!("exotic-pl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache_dir);

        let d = deps(&writer, &limiter, &token, cache_dir.clone());
        let factory = MockFactory {
            behavior: Behavior::Success,
        };
        let stats = run_exotic_pipeline_blocking(&d, &factory);

        assert_eq!(stats.done, 1, "应完成 1 个");
        let conn = writer.lock().unwrap();
        assert_eq!(task_status(&conn, item_id), 2, "任务应为 done");
        let (thumb_status, has_path): (i64, bool) = conn
            .query_row(
                "SELECT thumb_status, thumb_path IS NOT NULL FROM media_items WHERE id=?1",
                rusqlite::params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(thumb_status, 1, "media_items thumb_status 应回填 1");
        assert!(has_path);
        // 产物文件落盘。
        let p = crate::thumbnail::cache::thumb_path(&cache_dir, TIER, cache_key);
        assert!(p.exists(), "缩略图文件应已落盘");
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn stale_source_snapshot_cannot_finalize_or_patch_cover() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _cache_key) = setup_db(&conn);
        let claimed =
            q::claim_exotic_tasks(&conn, PID, CAPABILITY, 1, "old-worker", 1000, "mock-1.0.0")
                .unwrap()
                .pop()
                .unwrap();
        let source = q::exotic_item_source(&conn, item_id).unwrap();
        let task = ClaimedTask {
            task_id: claimed.id,
            item_id,
            source_revision: source.source_revision,
            cache_key: source.cache_key,
            fingerprint: "old-fingerprint".into(),
            tier: TIER,
            request: RequestBody::Thumbnail {
                item_id,
                source_path: source.abs_path,
                target_long_edge: TIER,
                input_fingerprint: "old-fingerprint".into(),
            },
            attempts: claimed.attempts,
        };

        // 在旧 Worker 已读取快照后模拟扫描推进新源代次。
        conn.execute(
            "UPDATE media_items
             SET source_revision=source_revision+1, cache_key=cache_key+1,
                 thumb_status=0, thumb_path=NULL, thumbhash=NULL
             WHERE id=?1",
            rusqlite::params![item_id],
        )
        .unwrap();

        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let cache_dir =
            std::env::temp_dir().join(format!("exotic-pl-stale-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache_dir);
        let d = deps(&writer, &limiter, &token, cache_dir.clone());

        assert!(!finalize_success(
            &d,
            &task,
            "mock-1.0.0",
            "old-worker",
            &make_webp(480, 240),
            &[1, 2, 3]
        )
        .unwrap());

        let conn = writer.lock().unwrap();
        let task_status: i64 = conn
            .query_row(
                "SELECT status FROM exotic_tasks WHERE id=?1",
                rusqlite::params![claimed.id],
                |row| row.get(0),
            )
            .unwrap();
        let (thumb_status, thumb_path): (i64, Option<String>) = conn
            .query_row(
                "SELECT thumb_status, thumb_path FROM media_items WHERE id=?1",
                rusqlite::params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(task_status, 1, "旧快照不得完成任务");
        assert_eq!(thumb_status, 0, "旧快照不得写入新源封面");
        assert!(thumb_path.is_none());
        drop(conn);
        assert!(
            !crate::thumbnail::cache::thumb_path(&cache_dir, TIER, task.cache_key).exists(),
            "过期快照应在落盘前被丢弃"
        );
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn stale_lease_deadlock_recovered_by_wake_sweep() {
        // 病历 #2 回归锁(2026-07-05 真机 60/62):硬杀/panic 遗留的 stale processing 行使
        // has_ready(只认 0/3)恒 false → pipeline 不启动 → 其步骤 0 的孤儿恢复永不可达,
        // 进度永久停摆。wake 前清扫必须能独立打破该互锁。
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        // 模拟已死实例的过期租约:claimed_at 早于 now - LEASE_TTL_SECS。
        let stale_at = now_secs() - LEASE_TTL_SECS - 60;
        let claimed = q::claim_exotic_tasks(
            &conn,
            PID,
            CAPABILITY,
            10,
            "dead-instance",
            stale_at,
            "mock-1.0.0",
        )
        .unwrap();
        assert_eq!(claimed.len(), 1);
        // 死锁面:此刻无就绪任务 → Coordinator 的 evaluate_run 不会启动 pipeline。
        assert!(!q::has_ready_exotic_task(&conn, PID, CAPABILITY, now_secs()).unwrap());
        // wake 前清扫恢复过期租约 → has_ready 恢复真值。
        let writer = Mutex::new(conn);
        assert_eq!(recover_stale_exotic_leases(&writer), 1);
        let conn = writer.into_inner().unwrap();
        assert_eq!(
            task_status(&conn, item_id),
            0,
            "stale processing 应回 pending"
        );
        assert!(q::has_ready_exotic_task(&conn, PID, CAPABILITY, now_secs()).unwrap());
        // 对偶面:活租约(claimed_at=now)不得被误清。
        let _ = q::claim_exotic_tasks(
            &conn,
            PID,
            CAPABILITY,
            10,
            "alive",
            now_secs(),
            "mock-1.0.0",
        )
        .unwrap();
        let writer = Mutex::new(conn);
        assert_eq!(recover_stale_exotic_leases(&writer), 0, "活租约不得回收");
    }

    #[test]
    fn not_runnable_claims_nothing() {
        // §5.3：派发前授权复核为 false（运行期 License 失效/插件禁用）→ 不领取，任务留 pending。
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let mut d = deps(
            &writer,
            &limiter,
            &token,
            std::env::temp_dir().join("exotic-pl-norun"),
        );
        d.is_runnable = Arc::new(|| false); // 授权在运行期失效
        let factory = MockFactory {
            behavior: Behavior::Success,
        };
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        assert_eq!(stats.done, 0, "不应完成任何任务");
        assert_eq!(
            task_status(&writer.lock().unwrap(), item_id),
            0,
            "任务应留 pending"
        );
    }

    #[test]
    fn unsupported_variant_is_terminal() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let d = deps(
            &writer,
            &limiter,
            &token,
            std::env::temp_dir().join("exotic-pl-term"),
        );
        let factory = MockFactory {
            behavior: Behavior::Failure {
                code: WorkerErrorCode::UnsupportedVariant,
                retryable: false,
            },
        };
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        assert_eq!(stats.terminal, 1);
        assert_eq!(
            task_status(&writer.lock().unwrap(), item_id),
            4,
            "应为 terminal"
        );
    }

    #[test]
    fn timeout_is_retried_with_backoff() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let d = deps(
            &writer,
            &limiter,
            &token,
            std::env::temp_dir().join("exotic-pl-retry"),
        );
        let factory = MockFactory {
            behavior: Behavior::Timeout,
        };
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        assert_eq!(stats.retried, 1);
        let conn = writer.lock().unwrap();
        assert_eq!(task_status(&conn, item_id), 3, "应为 retryable");
        let next_retry: Option<i64> = conn
            .query_row(
                "SELECT next_retry_at FROM exotic_tasks WHERE item_id=?1",
                rusqlite::params![item_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(next_retry.is_some(), "retryable 应设 next_retry_at");
    }

    /// 合成最小合法 RGB 8-bit raw PSD（供真实 Worker e2e）。
    fn make_rgb_psd(w: u32, h: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"8BPS");
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&8u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u16.to_be_bytes());
        for ch in 0..3u32 {
            for y in 0..h {
                for x in 0..w {
                    b.push(match ch {
                        0 => {
                            if w > 1 {
                                (x * 255 / (w - 1)) as u8
                            } else {
                                200
                            }
                        }
                        1 => {
                            if h > 1 {
                                (y * 255 / (h - 1)) as u8
                            } else {
                                120
                            }
                        }
                        _ => 128,
                    });
                }
            }
        }
        b
    }

    /// 真实 psd-worker 穿过 Pipeline + Sink。显式 `--ignored` 启用并提供
    /// `EXOTIC_PSD_WORKER_PATH`，证明 spawn→握手→解 PSD→Host 验证→落盘→条件 DB 全链路。
    /// 可用 `EXOTIC_PSD_SAMPLE_PATH` 注入真实样本（仅复制到临时目录），
    /// `EXOTIC_PSD_OUTPUT_PATH` 可保留产物供目视验收。
    #[test]
    #[ignore = "需要 EXOTIC_PSD_WORKER_PATH 指向真实 PSD worker"]
    fn real_worker_pipeline_end_to_end() {
        use crate::exotic::worker::resolve_psd_worker_path;
        let exe = resolve_psd_worker_path().expect("必须设置 EXOTIC_PSD_WORKER_PATH");
        // 真实样本也复制到独立 scan_root，避免验收过程中写入用户样本目录。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let source = root.join("synthetic.psd");
        if let Some(sample) = std::env::var_os("EXOTIC_PSD_SAMPLE_PATH") {
            std::fs::copy(sample, &source).expect("复制 PSD 验收样本");
        } else {
            std::fs::write(&source, make_rgb_psd(300, 200)).unwrap();
        }
        use std::io::Read;
        let mut header = [0u8; 26];
        std::fs::File::open(&source)
            .unwrap()
            .read_exact(&mut header)
            .unwrap();
        assert_eq!(&header[..4], b"8BPS");
        let source_height = u32::from_be_bytes(header[14..18].try_into().unwrap());
        let source_width = u32::from_be_bytes(header[18..22].try_into().unwrap());
        assert!(source_width > 0 && source_height > 0);
        let scale = (TIER as f64 / source_width.max(source_height) as f64).min(1.0);
        let expected_dims = (
            (source_width as f64 * scale).round().max(1.0) as u32,
            (source_height as f64 * scale).round().max(1.0) as u32,
        );

        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO scan_roots (path, alias) VALUES (?1, 'r')",
            rusqlite::params![root.to_string_lossy().to_string()],
        )
        .unwrap();
        let root_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO directories (root_id, parent_id, rel_path, name, depth, mtime)
             VALUES (?1, NULL, '', 'root', 0, 0)",
            rusqlite::params![root_id],
        )
        .unwrap();
        let dir_id = conn.last_insert_rowid();
        let cache_key: i64 = 0x00C0_FFEE;
        conn.execute(
            "INSERT INTO media_items
                (directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, 'synthetic.psd', 1, 1, 'psd', 'image', 0, 0, 0, ?2)",
            rusqlite::params![dir_id, cache_key],
        )
        .unwrap();
        let item_id = conn.last_insert_rowid();
        q::seed_exotic_tasks_for_item(&conn, item_id, PID, &["thumbnail".into()]).unwrap();

        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let cache_dir = root.join("cache");
        let d = deps(&writer, &limiter, &token, cache_dir.clone());

        let factory = SupervisorFactory {
            spec: WorkerSpec {
                exe_path: exe,
                expected_worker_id: "psd-worker".into(),
                required_capabilities: vec!["thumbnail".into()],
            },
            cfg: WorkerConfig {
                handshake_timeout: Duration::from_secs(5),
                host_version: "0.1.0".into(),
                max_blob_len: exotic_protocol::MAX_BLOB_LEN,
            },
        };
        let start = Instant::now();
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        let elapsed = start.elapsed();
        assert_eq!(
            stats,
            PipelineStats {
                done: 1,
                ..Default::default()
            },
            "真实 Worker 应完成 1 张，无重试/终态/租约丢失"
        );
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 2);
        let output = crate::thumbnail::cache::thumb_path(&cache_dir, TIER, cache_key);
        let webp = std::fs::read(&output).expect("完成任务必须有落盘文件");
        let decoded = image::load_from_memory_with_format(&webp, image::ImageFormat::WebP)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.dimensions(), expected_dims);
        let expected_hash =
            crate::thumbnail::thumbhash::generate_thumbhash(&crate::engine::traits::DecodedImage {
                width: decoded.width(),
                height: decoded.height(),
                pixels: decoded.into_raw(),
                icc: None,
            })
            .unwrap();
        let conn = writer.lock().unwrap();
        let (status, path, hash): (i64, String, Vec<u8>) = conn
            .query_row(
                "SELECT thumb_status, thumb_path, thumbhash FROM media_items WHERE id=?1",
                rusqlite::params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, 1);
        assert_eq!(cache_dir.join("thumbnails").join(path), output);
        assert_eq!(hash, expected_hash);
        if let Some(destination) = std::env::var_os("EXOTIC_PSD_OUTPUT_PATH") {
            let destination = PathBuf::from(destination);
            std::fs::create_dir_all(destination.parent().expect("产物须指定父目录")).unwrap();
            std::fs::copy(&output, destination).expect("保留 PSD 验收产物");
        }
        eprintln!(
            "PSD source={}x{} output={}x{} webp_bytes={} pipeline_ms={:.2}",
            source_width,
            source_height,
            expected_dims.0,
            expected_dims.1,
            webp.len(),
            elapsed.as_secs_f64() * 1000.0
        );
    }

    /// 真实 RAW worker 的 Failure 穿过领取、回声校验、失败预算与 DB 写回，不依赖相机样张。
    #[test]
    #[ignore = "需要 EXOTIC_RAW_WORKER_PATH 指向真实 RAW worker"]
    fn real_raw_worker_pipeline_failures() {
        let exe =
            std::env::var_os("EXOTIC_RAW_WORKER_PATH").expect("必须设置 EXOTIC_RAW_WORKER_PATH");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("malformed.dng"), [0xabu8; 64]).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        let (malformed_id, _) = setup_db(&conn);
        const RAW_PID: &str = "exotic-image-raw";
        conn.execute(
            "UPDATE scan_roots SET path=?1",
            rusqlite::params![dir.path().to_string_lossy().to_string()],
        )
        .unwrap();
        conn.execute(
            "UPDATE media_items SET file_name=?1, file_format=?2 WHERE id=?3",
            rusqlite::params!["malformed.dng", "dng", malformed_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE exotic_tasks SET plugin_id=?1 WHERE item_id=?2",
            rusqlite::params![RAW_PID, malformed_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media_items
                (directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             SELECT directory_id, ?1, file_size, file_mtime, file_format,
                    media_type, width, height, sort_datetime, cache_key+1
             FROM media_items WHERE id=?2",
            rusqlite::params!["missing.dng", malformed_id],
        )
        .unwrap();
        let missing_id = conn.last_insert_rowid();
        q::seed_exotic_tasks_for_item(&conn, missing_id, RAW_PID, &[CAPABILITY.into()]).unwrap();

        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let cache_dir = dir.path().join("cache");
        let mut d = deps(&writer, &limiter, &token, cache_dir.clone());
        d.plugin_id = RAW_PID.into();
        let notifications = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&notifications);
        d.on_progress = Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        });
        let factory = SupervisorFactory {
            spec: WorkerSpec {
                exe_path: exe.into(),
                expected_worker_id: "raw-worker".into(),
                required_capabilities: vec![CAPABILITY.into()],
            },
            cfg: WorkerConfig {
                handshake_timeout: Duration::from_secs(5),
                host_version: "0.1.0".into(),
                max_blob_len: exotic_protocol::MAX_BLOB_LEN,
            },
        };
        let start = Instant::now();
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        assert_eq!(
            stats,
            PipelineStats {
                terminal: 1,
                retried: 1,
                ..Default::default()
            }
        );
        assert!(notifications.load(Ordering::SeqCst) > 0);
        let conn = writer.lock().unwrap();
        for (id, expected_status, expected_code) in [
            (malformed_id, 4, "malformed_input"),
            (missing_id, 3, "io_error"),
        ] {
            conn.query_row(
                "SELECT status, attempts, last_error_code, next_retry_at, lease_owner, worker_version
                 FROM exotic_tasks WHERE item_id=?1",
                rusqlite::params![id],
                |row| {
                    assert_eq!(row.get::<_, i64>(0)?, expected_status);
                    assert_eq!(row.get::<_, i64>(1)?, 1);
                    assert_eq!(row.get::<_, String>(2)?, expected_code);
                    assert_eq!(
                        row.get::<_, Option<i64>>(3)?.is_some(),
                        expected_status == 3
                    );
                    assert!(row.get::<_, Option<String>>(4)?.is_none());
                    assert!(!row.get::<_, String>(5)?.is_empty());
                    Ok(())
                },
            )
            .unwrap();
        }
        assert!(!cache_dir.exists(), "失败任务不应写入缩略图");
        eprintln!(
            "RAW failure_pipeline_ms={:.2}",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn cancelled_before_run_leaves_task_pending() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, _) = setup_db(&conn);
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        token.cancel(); // 预先取消
        let d = deps(
            &writer,
            &limiter,
            &token,
            std::env::temp_dir().join("exotic-pl-cancel"),
        );
        let factory = MockFactory {
            behavior: Behavior::Success,
        };
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        assert_eq!(stats.done, 0);
        assert_eq!(
            task_status(&writer.lock().unwrap(), item_id),
            0,
            "取消后任务仍 pending"
        );
    }

    #[test]
    fn late_owner_cannot_replace_committed_thumbnail() {
        let conn = Connection::open_in_memory().unwrap();
        let (item_id, cache_key) = setup_db(&conn);
        let row = q::claim_exotic_tasks(&conn, PID, CAPABILITY, 1, "old", 1000, "mock-1.0.0")
            .unwrap()
            .pop()
            .unwrap();
        let src = q::exotic_item_source(&conn, item_id).unwrap();
        let task = ClaimedTask {
            task_id: row.id,
            item_id,
            source_revision: src.source_revision,
            cache_key,
            fingerprint: "fp".into(),
            tier: TIER,
            attempts: 0,
            request: RequestBody::Thumbnail {
                item_id,
                source_path: src.abs_path,
                target_long_edge: TIER,
                input_fingerprint: "fp".into(),
            },
        };
        q::release_exotic_instance_leases(&conn, "old").unwrap();
        q::claim_exotic_tasks(&conn, PID, CAPABILITY, 1, "new", 2000, "mock-1.0.0").unwrap();
        let writer = Mutex::new(conn);
        let limiter = BackgroundHeavyLimiter::new(2);
        let token = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();
        let d = deps(&writer, &limiter, &token, dir.path().to_path_buf());
        let current = make_webp(20, 20);
        assert!(finalize_success(&d, &task, "mock-1.0.0", "new", &current, &[1, 2, 3]).unwrap());
        assert!(!finalize_success(
            &d,
            &task,
            "mock-1.0.0",
            "old",
            &make_webp(10, 10),
            &[4, 5, 6]
        )
        .unwrap());
        assert_eq!(
            std::fs::read(crate::thumbnail::cache::thumb_path(
                dir.path(),
                TIER,
                cache_key
            ))
            .unwrap(),
            current
        );
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 2);
    }
}
