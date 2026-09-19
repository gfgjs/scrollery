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
//!   - **先文件后 DB**（§4.4）：Sink 原子 rename 后才在条件事务里写 task done + media_items。
//!   - **熔断**：进程级/协议级失败计 strike；坏数据（unsupported/malformed）不计 strike。
//!
//! Worker 经 [`ThumbnailWorker`] trait 抽象 → 单测用 mock worker + 内存 DB，不起真实进程。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use rusqlite::Connection;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use exotic_protocol::{RequestBody, WorkerErrorCode};

use crate::db::queries as q;
use crate::exotic::fingerprint::thumbnail_fingerprint;
use crate::exotic::limiter::BackgroundHeavyLimiter;
use crate::exotic::sink::write_thumbnail_atomic;
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
/// 插件熔断 strike 阈值（进程级/协议级失败累计）。本卷在内存内按 run 计；跨 run 持久化留 Part3。
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
    pub items_cache: &'a crate::layout::items_cache::ItemsCache,
    pub cache_dir: PathBuf,
    /// 当前缩略图档位请求尺寸（吸附在指纹/Worker 内做）。
    pub requested_size: u32,
    pub plugin_id: String,
    /// 合并事件回调（落地一批产物后触发一次；真实接线发 db:media_enriched + exotic:status-changed）。
    pub on_progress: Arc<dyn Fn() + Send + Sync>,
    /// 让步判定（R1）：scan/thumbnail/interaction 活动时为 true。真实接线注入
    /// `state.should_yield_exotic()`；测试默认返回 false。
    pub should_yield: Arc<dyn Fn() -> bool + Send + Sync>,
    /// 派发前授权复核（§5.3：「每批领取前」校验）。真实接线注入
    /// `host.is_task_runnable(plugin_id, capability)`；运行期 License 失效/插件禁用/卸载后
    /// 返回 false → Claimer 停领新批（在途自然完成）。测试默认 true。
    pub is_runnable: Arc<dyn Fn() -> bool + Send + Sync>,
}

/// 一次运行统计。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PipelineStats {
    pub done: u64,
    pub retried: u64,
    pub terminal: u64,
    pub lease_lost: u64,
    /// 插件是否在本次 run 内熔断。
    pub circuit_opened: bool,
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
            return PipelineStats::default();
        }
    };
    let worker_version = probe.worker_version();

    // ── 2. Worker 升级失效：把该插件「done 但 worker_version 不同」的任务退回 pending。──
    {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        match q::invalidate_exotic_tasks_for_plugin_version(&conn, &plugin_id, &worker_version) {
            Ok(n) if n > 0 => {
                info!("exotic：worker 升级失效 {n} 个 done 任务（版本 {worker_version}）")
            }
            Ok(_) => {}
            Err(e) => warn!("exotic：worker 版本失效失败：{e}"),
        }
    }

    // ── 3. 通道 + 共享状态。──
    let pool_size = MAX_POOL.max(1);
    let (task_tx, task_rx) = bounded::<ClaimedTask>(pool_size * 2);
    let (result_tx, result_rx) = bounded::<WorkerResult>(pool_size * 2);
    // 探到的 Worker 放入种子槽，供池线程复用（避免二次 spawn）。
    let seed: Arc<Mutex<Vec<Box<dyn ThumbnailWorker>>>> = Arc::new(Mutex::new(vec![probe]));
    // 熔断闸：strike 达阈值置位 → Claimer 停止领取。
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
                );
            });
        }

        // Worker 池
        for _ in 0..pool_size {
            let task_rx = task_rx.clone();
            let result_tx = result_tx.clone();
            let seed = Arc::clone(&seed);
            let limits = limits.clone();
            s.spawn(move || {
                worker_loop(deps, factory, &seed, task_rx, result_tx, &limits);
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
    out.circuit_opened = circuit_open.load(Ordering::SeqCst);
    out
}

/// Claimer：让步门控 → 原子领取 → 构造请求 → 入 task channel。空批即结束（关闭 channel）。
fn claimer_loop(
    deps: &PipelineDeps<'_>,
    plugin_id: &str,
    worker_version: &str,
    instance_id: &str,
    task_tx: Sender<ClaimedTask>,
    circuit_open: &AtomicBool,
) {
    loop {
        if deps.token.is_cancelled() || circuit_open.load(Ordering::SeqCst) {
            break;
        }
        // R1 让步：scan/thumbnail/interaction 活动时暂缓领取新任务（在途不抢占）。
        while deps.should_yield() && !deps.token.is_cancelled() {
            std::thread::sleep(YIELD_POLL);
        }
        if deps.token.is_cancelled() {
            break;
        }
        // §5.3 派发前授权复核：运行期 License 失效/插件禁用/卸载 → 停领新批（在途自然完成）。
        // 与 evaluate_run 的「Pipeline 启动前」检查互补，覆盖「运行期间授权变化」窗口。
        if !(deps.is_runnable)() {
            info!("exotic：授权/可领取态在运行期失效，停止领取新批");
            break;
        }

        let claimed = {
            let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
            match q::claim_exotic_tasks(
                &conn,
                plugin_id,
                CAPABILITY,
                CLAIM_BATCH,
                instance_id,
                now_secs(),
            ) {
                Ok(rows) => rows,
                Err(e) => {
                    warn!("exotic：领取失败：{e}");
                    break;
                }
            }
        };
        if claimed.is_empty() {
            break; // 无更多就绪任务 → 本次结束（新任务由 Coordinator 重新唤醒）
        }

        for row in claimed {
            if deps.token.is_cancelled() {
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
                    let _ = q::fail_exotic_task(
                        &conn,
                        row.id,
                        instance_id,
                        true,
                        MAX_ATTEMPTS,
                        WorkerErrorCode::IoError.as_str(),
                        "源不可读",
                        now_secs() + 30,
                    );
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
) {
    let mut worker: Option<Box<dyn ThumbnailWorker>> = None;
    for task in task_rx {
        if deps.token.is_cancelled() {
            break;
        }
        // R4：取共享后台重活 permit（与 derivation 同预算，FIFO 公平）。取消 → 退出（任务由结束清理退回）。
        let permit = match deps.limiter.acquire(deps.token) {
            Some(p) => p,
            None => break,
        };

        // 确保有活 Worker：先用种子，再 factory.spawn（带崩溃退避）。
        if worker.as_ref().map(|w| !w.is_alive()).unwrap_or(true) {
            worker = None;
            let seeded = seed.lock().unwrap_or_else(|e| e.into_inner()).pop();
            worker = match seeded {
                Some(w) => Some(w),
                None => match factory.spawn() {
                    Ok(w) => Some(w),
                    Err(e) => {
                        warn!("exotic：补充 Worker 失败：{e} → 任务按断开重试");
                        let _ = result_tx.send(WorkerResult {
                            task,
                            outcome: TaskOutcome::Disconnected,
                        });
                        drop(permit);
                        std::thread::sleep(CRASH_BACKOFF);
                        continue;
                    }
                },
            };
        }

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
    let mut landed_any = false;

    for res in result_rx {
        let WorkerResult { task, outcome } = res;
        let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
        match outcome {
            TaskOutcome::Success {
                width,
                height,
                blob,
                ..
            } => {
                debug!("exotic：item {} 出图 {}x{}", task.item_id, width, height);
                match finalize_success(deps, &task, worker_version, instance_id, &blob) {
                    Ok(true) => {
                        s.done += 1;
                        landed_any = true;
                    }
                    Ok(false) => {
                        // 租约已失（孤儿被另一实例/恢复处理）→ 丢弃，文件留作可回收孤儿。
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
                        );
                        s.retried += 1;
                    }
                }
            }
            TaskOutcome::Failure(body) => {
                // 数据类错误（unsupported/malformed/resource）→ terminal，不计 strike；
                // io/internal → retryable。
                let retryable = body.retryable && body.code.default_retryable();
                if retryable {
                    fail_task(deps, &task, instance_id, true, body.code, &body.message);
                    s.retried += 1;
                } else {
                    fail_task(deps, &task, instance_id, false, body.code, &body.message);
                    s.terminal += 1;
                }
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
                );
                s.retried += 1;
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
                );
                s.terminal += 1;
            }
        }
        drop(s);

        if strikes >= STRIKE_THRESHOLD && !circuit_open.load(Ordering::SeqCst) {
            warn!("exotic：插件 {plugin_id} strike 达 {strikes} → 本次熔断，停止领取");
            circuit_open.store(true, Ordering::SeqCst);
        }
    }

    if landed_any {
        (deps.on_progress)();
    }
}

/// 成功路径：原子落盘 → 条件事务（finish task + update media_items）→ layout cache。
/// 返回 Ok(true)=本实例落库成功；Ok(false)=租约已失（丢弃）；Err=落盘/DB 错误。
fn finalize_success(
    deps: &PipelineDeps<'_>,
    task: &ClaimedTask,
    worker_version: &str,
    instance_id: &str,
    blob: &[u8],
) -> crate::error::Result<bool> {
    // 0. 先做短快照预检并立即释放 writer 锁。source_revision/cache_key 已变化时，
    // 直接丢弃迟到结果，避免它先覆盖同一 cache_key 的文件；最终 DB CAS 仍在落盘后复核。
    let source_current = {
        let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
        q::is_exotic_source_current(&conn, task.item_id, task.source_revision, task.cache_key)?
    };
    if !source_current {
        return Ok(false);
    }

    // 1. 先文件：原子落盘 + Host 计算 thumbhash。
    let sink = write_thumbnail_atomic(&deps.cache_dir, task.tier, task.cache_key, blob)?;

    // 2. 后 DB：条件事务（status=1 AND lease_owner=instance），同事务回填 media_items。
    let committed = {
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
                Some(&sink.thumbhash),
            )? == 1
        } else {
            false
        };
        tx.commit()?;
        (ok, thumb_applied)
    };

    // 3. 同步 items 取数缓存（S3：布局行仅存几何，出口拼装自 items 缓存取载荷——
    //    patch 单点即可使产物在滚出再滚回时立即可见，无需整表重算）。
    if committed.0 && committed.1 {
        let thumb = crate::db::models::ThumbResult {
            item_id: task.item_id,
            thumb_status: 1,
            thumb_path: Some(sink.thumb_db_path),
            thumbhash: Some(sink.thumbhash),
            source_revision: task.source_revision,
            cache_key: task.cache_key,
        };
        crate::layout::items_cache::apply_thumb_results(
            deps.items_cache,
            std::slice::from_ref(&thumb),
        );
    }
    Ok(committed.0)
}

/// 失败路径：条件 fail（retryable 计退避，terminal 不退避）。
fn fail_task(
    deps: &PipelineDeps<'_>,
    task: &ClaimedTask,
    instance_id: &str,
    retryable: bool,
    code: WorkerErrorCode,
    message: &str,
) {
    let next_retry_at = if retryable {
        now_secs() + backoff_secs(task.attempts, code)
    } else {
        0
    };
    let conn = deps.writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = q::fail_exotic_task(
        &conn,
        task.task_id,
        instance_id,
        retryable,
        MAX_ATTEMPTS,
        code.as_str(),
        message,
        next_retry_at,
    ) {
        warn!("exotic：标记失败写库错误：{e}");
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
            _cancelled: &dyn Fn() -> bool,
        ) -> TaskOutcome {
            match self.behavior.clone() {
                Behavior::Success => TaskOutcome::Success {
                    width: 480,
                    height: 240,
                    mime: "image/webp".into(),
                    blob: make_webp(480, 240),
                },
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
    fn test_items_cache() -> &'static crate::layout::items_cache::ItemsCache {
        static CACHE: std::sync::OnceLock<crate::layout::items_cache::ItemsCache> =
            std::sync::OnceLock::new();
        CACHE.get_or_init(crate::layout::items_cache::new_items_cache)
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
        let claimed = q::claim_exotic_tasks(&conn, PID, CAPABILITY, 1, "old-worker", 1000)
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

        assert!(
            !finalize_success(&d, &task, "mock-1.0.0", "old-worker", &make_webp(480, 240),)
                .unwrap()
        );

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
        let claimed =
            q::claim_exotic_tasks(&conn, PID, CAPABILITY, 10, "dead-instance", stale_at).unwrap();
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
        let _ = q::claim_exotic_tasks(&conn, PID, CAPABILITY, 10, "alive", now_secs()).unwrap();
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

    /// 端到端：**真实** psd-worker 子进程穿过整条 Pipeline + 原子 Sink（仅设
    /// `EXOTIC_PSD_WORKER_PATH` 时运行）。证明 spawn→握手→解 PSD→Host 验证→落盘→条件 DB 全链路。
    #[test]
    fn real_worker_pipeline_end_to_end() {
        use crate::exotic::worker::resolve_psd_worker_path;
        let Some(exe) = resolve_psd_worker_path() else {
            eprintln!("[skip] 未设 EXOTIC_PSD_WORKER_PATH，跳过真实 Worker pipeline e2e");
            return;
        };
        // 用专属临时目录做 scan_root，写入真实合成 PSD。
        let root = std::env::temp_dir().join(format!("exotic-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("synthetic.psd"), make_rgb_psd(300, 200)).unwrap();

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
        let stats = run_exotic_pipeline_blocking(&d, &factory);
        assert_eq!(stats.done, 1, "真实 Worker 应出图 1 张");
        assert_eq!(task_status(&writer.lock().unwrap(), item_id), 2);
        assert!(crate::thumbnail::cache::thumb_path(&cache_dir, TIER, cache_key).exists());
        let _ = std::fs::remove_dir_all(&root);
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
}
