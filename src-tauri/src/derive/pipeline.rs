// src-tauri/src/derive/pipeline.rs
//! 后台派生流水线 —— 复用 AI 流水线模式（§1.2）：
//!
//!   生产者 → crossbeam 通道 → 消费者池（rayon）→ 写入器
//!   + CancellationToken（暂停/停止）+ should_yield_derivation()（让步）
//!   + 状态机（0待处理/1处理中/2完成/3错误）→ 断点续传 + 孤儿恢复
//!
//! Unlike AI (one model, GPU-batched), each derivation `kind` is a plain function
//! (`kind::run`) — the framework here is kind-agnostic. Adding a kind needs zero changes
//! to this file. P0 ships the framework with stub kinds; backends land in P2/P3/P4.
//! 与 AI（单模型、GPU 批处理）不同，每种派生 `kind` 是一个纯函数（`kind::run`）——
//! 本框架与具体 kind 无关。新增 kind 无需改动本文件。P0 交付框架 + 桩 kind，后端在 P2/P3/P4 落地。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, Sender};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::db::models::ThumbResult;
use crate::db::queries::{
    backfill_derivations, batch_finish_derivations_with_snapshot, count_derivations_by_status,
    get_pending_derivations, mark_derivations_processing, requeue_in_flight_derivations,
    reset_processing_derivations, DerivationClaim, DerivationResultWithSnapshot,
};
use crate::derive::kind::{self, DerivationContext, DerivationKind};
use crate::scanner::enricher::MediaEnrichedPayload;
use crate::state::AppState;

/// Fallback default when `derive_batch_size` is absent from config.toml (schema.rs
/// `SETTING_DEFS` default is authoritative; this only backs the `.unwrap_or` parse fallback).
/// 读取待处理任务 / 刷新结果的批次大小的回退默认值(config.toml 缺省时用;真实生效值以
/// `schema.rs::SETTING_DEFS` 为准)。
const DEFAULT_BATCH_SIZE: i64 = 256;

/// 生产者和消费者之间的通道容量。
const CHANNEL_CAPACITY: usize = 512;

/// Config-derived tuning values snapshotted once per pipeline run (avoids per-task
/// RwLock/ConfigManager reads) and threaded through `consume_tasks` as a single param — keeps
/// its arg count under clippy's `too_many_arguments` threshold (批次C新增 3 个 advanced 键后,
/// 逐参数堆会破阈值)。
/// 每次流水线启动快照一次的调优值(避免逐任务读 RwLock/ConfigManager),打包成单个参数穿给
/// `consume_tasks`——批次C新增 3 个 advanced 键后逐参数堆会撞 clippy 的 `too_many_arguments`。
#[derive(Clone)]
struct PipelineTuning {
    cache_dir: PathBuf,
    thumb_size: u32,
    webp_quality: u8,
    ai_cache_short_edge: u32,
    keyframe_count: usize,
    sprite_cell_height: u32,
}

/// 从生产者发送到消费者的任务。
struct DerivationTaskMsg {
    item_id: i64,
    kind: DerivationKind,
    abs_path: PathBuf,
    file_format: String,
    media_type: String,
    source_revision: i64,
    cache_key: i64,
}

/// Start the background derivation pipeline. Returns immediately; work runs in background
/// threads. `kind_filter` optionally limits processing to a set of kinds (e.g. the video
/// control card runs cover+keyframes together).
/// 启动后台派生流水线。立即返回；工作在后台线程中运行。`kind_filter` 可选地限定只处理一组 kind
/// （如视频控制卡把封面+关键帧作为一组运行）。
pub fn start_derivation_pipeline(
    app: AppHandle,
    state: Arc<AppState>,
    generation: u64,
    token: CancellationToken,
    kind_filter: Option<Vec<DerivationKind>>,
    // 显式字符串传参贯穿本次运行的任务级汇总日志(方案 §3.3/D-304 operation_id,S2):tokio::spawn 的
    // async 任务边界同样不传播 span,故这里跟 spawn_blocking 一样必须走参数而非依赖上下文。
    operation_id: Option<String>,
) {
    tokio::spawn(async move {
        // span 埋点(W1,方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入):覆盖整个 run
        // 的墙钟时间,块尾自然 Drop——正常完成/失败/panic 三条路径都会触达。既有的完成/失败/
        // panic 汇总日志(下方 info!/warn!)保留不动。operation_id 该函数已贯穿参数(D-304),
        // 直接挂载对齐同一次运行的日志行。
        let _span = crate::logging::SpanTimer::info("pipeline:derive")
            .with_operation_id(operation_id.clone());
        let start = std::time::Instant::now();
        // 保留句柄以区分自然完成与暂停/停止取消。
        let token_outer = token.clone();
        let state_run = Arc::clone(&state);

        let mut handle = tokio::task::spawn_blocking(move || {
            run_pipeline_blocking(&app, &state_run, generation, &token, kind_filter)
        });
        // 看门狗(可观测性三修 #4):join 迟迟不返回时(如派生本身挂死)每 300s 打一次周期性
        // warn,而非静默等待——本次故障(派生挂死 → AI/face 让步的阻塞源常驻 → 前端与日志
        // 双双零反馈)正是被这类长时间无输出误判为「没跑起来」。完成/错误路径语义与此前
        // 完全一致(下方 match 一字不动),这里只加告警,不改变对 result 的处理。
        // JoinHandle 实现 Unpin,可在循环中直接 `&mut handle` 反复 poll,无需 tokio::pin!。
        let result = loop {
            tokio::select! {
                res = &mut handle => break res,
                _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
                    let running_secs = start.elapsed().as_secs();
                    warn!(
                        operation_id = operation_id.as_deref(),
                        "Derivation pipeline still running after {}s | 派生流水线运行超过 {}s 仍未完成",
                        running_secs, running_secs
                    );
                }
            }
        };

        let elapsed = start.elapsed().as_millis();
        let op_id = operation_id.as_deref();
        match result {
            Ok(Ok(())) => info!(
                operation_id = op_id,
                elapsed_ms = elapsed as u64,
                "Derivation pipeline completed"
            ),
            Ok(Err(e)) => warn!(
                operation_id = op_id, error = %e,
                "Derivation pipeline error"
            ),
            Err(e) => warn!(
                operation_id = op_id, error = %e,
                "Derivation task panicked"
            ),
        }

        // Compare-and-clear (审查 F-01):仅当槽内仍是本轮代次才清 token。旧实现无条件
        // cancel_derivation() 会 take+cancel「停止→立即重启」时新轮刚安装的 token,
        // 使刚重启的提取静默停止(与 finish_ai_analysis 同款纪律,2026-07-10 审查 F10)。
        let was_current = state.finish_derivation(generation);

        // 自然完成（未被取消）→ 清除续传标志；无可续传。
        // 仅当本轮仍是当前轮:被新一轮取代时,新轮刚写入的 active=1 不得被旧轮清掉。
        // (finish 与此写之间理论上仍有一条新轮启动的微窗口,后果限于续传标志漂移,可接受。)
        if was_current && !token_outer.is_cancelled() {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            let _ = crate::db::queries::set_config(&conn, "derivation_active", "0");
        }
    });
}

/// 阻塞式流水线运行器 — 在 spawn_blocking + rayon 中运行。
fn run_pipeline_blocking(
    app: &AppHandle,
    state: &Arc<AppState>,
    generation: u64,
    token: &CancellationToken,
    kind_filter: Option<Vec<DerivationKind>>,
) -> crate::error::Result<()> {
    // ── Resume: recover orphaned (status=1) tasks left by a crash/force-quit → pending. ──
    // A normal graceful stop no longer lands here — the stop path (below, after
    // `std::thread::scope` joins all three workers) requeues its own in-flight rows via
    // `requeue_in_flight_derivations` without touching `orphan_count`. Residual windows that
    // still reach this counted path: requeue failing (warn-only), the process dying between
    // cancel and requeue, or a restart racing ahead of the old round's drain — all of which
    // look like a crash from here, so spending poison-guard budget on them is acceptable
    // (a completed task later resets its count to 0 anyway).
    // ── 续传：把崩溃/force-quit 遗留的孤儿任务（status=1）恢复为待处理。──
    // 正常的主动 stop 不再走到这里 —— 停止路径（下方，`std::thread::scope` 三个 worker 全部
    // 静默之后）已用 `requeue_in_flight_derivations` 优雅退回在途行，不动 `orphan_count`。
    // 仍会走到本计数路径的残余窗口（2026-07-23 复核裁定接受）：优雅回退失败（仅 warn）、
    // cancel 后回退前进程被杀、重启抢在旧轮排空之前——这些在此处与崩溃不可分辨，消耗毒任务
    // 预算可接受（任务此后正常完成会把计数清零）。
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        match reset_processing_derivations(&conn) {
            Ok(n) if n > 0 => info!("Recovered {} orphaned derivations (processing → pending) | 恢复 {} 个孤儿派生（处理中 → 待处理）", n, n),
            Ok(_) => {}
            Err(e) => warn!("Failed to recover orphaned derivations | 恢复孤儿派生失败: {}", e),
        }
    }

    // ── User toggles: whether video cover / keyframe extraction is desired (default ON). ──
    // A disabled kind is neither backfilled (below) nor picked up by the producer (excluded
    // from get_pending_derivations) — already-pending rows just pause until re-enabled, so this
    // is fully non-destructive and reuses the normal resume path.
    // ── 用户开关：是否要提取视频封面 / 关键帧（默认开）。被关闭的 kind 既不会被 backfill 入队（见下），
    // 也不会被生产者领取（在 get_pending_derivations 中排除）—— 已入队的待处理行只是暂停，待开关
    // 重新打开后续传。完全非破坏性，复用正常的续传路径。
    // A2:三键均为 schema 设置类,唯一真源已切到 ConfigManager(内存读,不必再借读池连接查 DB)。
    let mut disabled_kinds: Vec<&'static str> = {
        // None / 任何非 "false" 值 → 视为开启（默认开）。
        let enabled = |key: &str| state.config.get(key).map(|v| v != "false").unwrap_or(true);
        let mut v = Vec::new();
        if !enabled("enable_video_cover") {
            v.push(DerivationKind::VideoCover.as_str());
        }
        if !enabled("enable_video_keyframes") {
            v.push(DerivationKind::VideoKeyframes.as_str());
        }
        // AI 高清缓存与视频派生相反，是 **opt-in**（默认关）：仅当 `ai_hq_cache_enabled == "true"`
        // 时才入队/处理 ai_thumb，否则把它加入 disabled_kinds（既不 backfill 也不被生产者领取）。
        let ai_hq_enabled = state
            .config
            .get("ai_hq_cache_enabled")
            .map(|val| val == "true")
            .unwrap_or(false);
        if !ai_hq_enabled {
            v.push(DerivationKind::AiThumb.as_str());
        }
        v
    };
    // 显式 kind_filter(手动「全量/增量提取」点名的 kind)覆盖后台开关:用户明确要求跑的 kind
    // 不受 enable_* 拦截;无过滤的自动流水线仍完整尊重开关。
    if let Some(filter) = &kind_filter {
        disabled_kinds.retain(|d| !filter.iter().any(|k| k.as_str() == *d));
    }
    if !disabled_kinds.is_empty() {
        info!(
            "Derivation kinds disabled by user setting | 用户设置禁用的派生 kind: {:?}",
            disabled_kinds
        );
    }

    // ── Enqueue (backfill): insert pending rows for implemented kinds whose source items ──
    // exist but lack a (item, kind) row. Stub kinds (is_implemented=false) are skipped, so
    // P0 enqueues nothing and the pipeline is a clean no-op.
    // ── 入队（backfill）：为已实现的 kind 插入待处理行（源项存在但缺 (item,kind) 行）。──
    // 桩 kind（is_implemented=false）跳过，故 P0 不入队任何项，流水线干净空跑。
    {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let mut enqueued = 0usize;
        for k in DerivationKind::ALL {
            if !k.is_implemented() {
                continue;
            }
            // 用户在设置中关闭了该视频派生 → 不入队（与生产者的排除保持一致）。
            if disabled_kinds.contains(&k.as_str()) {
                continue;
            }
            if let Some(only) = &kind_filter {
                if !only.contains(&k) {
                    continue;
                }
            }
            let n = match k {
                DerivationKind::VideoCover | DerivationKind::VideoKeyframes => {
                    backfill_derivations(&conn, k.as_str(), "video", None)
                }
                DerivationKind::AudioCover | DerivationKind::AudioMeta => {
                    backfill_derivations(&conn, k.as_str(), "audio", None)
                }
                DerivationKind::DocThumb => backfill_derivations(
                    &conn,
                    k.as_str(),
                    "document",
                    Some(&DerivationKind::DOC_THUMB_FORMATS),
                ),
                DerivationKind::AiThumb => backfill_derivations(&conn, k.as_str(), "image", None),
                // 按需 kind(§5.2):播放交互路径直接 upsert+claim,绝不进背景 backfill(is_implemented=false
                // 本就不会入此循环,此处仅为 match 穷尽);入队 0。
                DerivationKind::VideoPlayable => Ok(0),
            }
            .unwrap_or_else(|e| {
                warn!(
                    "Backfill failed for kind {} | kind {} 入队失败: {}",
                    k.as_str(),
                    k.as_str(),
                    e
                );
                0
            });
            enqueued += n;
        }
        if enqueued > 0 {
            info!(
                "Enqueued {} new derivation task(s) | 新入队 {} 个派生任务",
                enqueued, enqueued
            );
        }
    }

    // 预先统计待处理数用于日志。
    let kind_filter_str: Option<Vec<String>> = kind_filter
        .as_ref()
        .map(|ks| ks.iter().map(|k| k.as_str().to_string()).collect());
    {
        let read_conn = state.db_read_pool.get()?;
        let pending =
            get_pending_derivations(&read_conn, 1, kind_filter_str.as_deref(), &disabled_kinds)?
                .len();
        if pending == 0 {
            info!("Derivation pipeline: nothing pending | 派生流水线：无待处理项");
            return Ok(());
        }
    }
    info!("Derivation pipeline starting | 派生流水线启动");

    // 一次性快照缓存配置（避免逐任务读 RwLock）。
    // 批次C:三个 advanced 键(video_keyframe_count / sprite_cell_height / derive_batch_size)
    // 同一「每次流水线启动读一次」的 hot 语义(非存量已生成产物,不需要热失效逻辑)。跨平台安全:
    // derive::video 模块本身不受 `#[cfg(windows)]` 限制,只有其内部 backend_for() 在非 Windows
    // 上返回 None。打包进 PipelineTuning 单参数穿给 consume_tasks(见其定义处的 clippy 说明)。
    let tuning = {
        let cfg = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
        PipelineTuning {
            cache_dir: cfg.cache_dir.clone(),
            thumb_size: cfg.size,
            webp_quality: cfg.webp_quality,
            ai_cache_short_edge: cfg.ai_cache_short_edge,
            keyframe_count: state
                .config
                .get("video_keyframe_count")
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::derive::video::KEYFRAME_COUNT),
            sprite_cell_height: state
                .config
                .get("sprite_cell_height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::derive::video::DEFAULT_SPRITE_CELL_H),
        }
    };
    // derive_batch_size(同上,批次C):读取/写入两侧批大小,决定 get_pending_derivations 的
    // LIMIT 与写入器的 flush 阈值——独立于 PipelineTuning(producer/writer 各自只需这一个值,
    // 不必为它们也搭一份快照结构体)。
    let batch_size: i64 = state
        .config
        .get("derive_batch_size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_BATCH_SIZE);

    // ── Channels ──────────────────────────────────────────────────────────────
    let (task_tx, task_rx) = bounded::<DerivationTaskMsg>(CHANNEL_CAPACITY);
    let (result_tx, result_rx) = bounded::<DerivationResultWithSnapshot>(CHANNEL_CAPACITY);

    let state_prod = Arc::clone(state);
    let state_consumer = Arc::clone(state);
    let state_writer = Arc::clone(state);
    let token_prod = token.clone();
    let token_consumer = token.clone();
    let token_writer = token.clone();
    let app_writer = app.clone();
    let kind_filter_owned = kind_filter_str.clone();
    let disabled_prod = disabled_kinds.clone();

    // Run producer / consumer / writer on dedicated OS threads (`std::thread::scope`), NOT on
    // rayon workers. The three are long-lived and mostly BLOCK on channels — parking them on
    // rayon workers stole pool threads, throttling the actual parallel decode (and any other
    // par_iter) on low-core machines. With scoped OS threads, the consumer's inner `rayon::scope`
    // gets the FULL rayon pool for cover/keyframe decode. The scope joins all three before return,
    // preserving the original blocking semantics (we're inside spawn_blocking).
    // 把生产者/消费者/写入器放到独立的 OS 线程（`std::thread::scope`），而非 rayon 工作线程。这三者
    // 长生命周期且多数时间阻塞在 channel 上 —— 占用 rayon 线程会拖慢真正的并行解码（及其它 par_iter），
    // 低核机器尤甚。改用作用域 OS 线程后，消费者内部的 `rayon::scope` 可独享整个 rayon 池做封面/关键帧
    // 解码。作用域在返回前 join 全部三者，保持原阻塞语义（仍在 spawn_blocking 内）。
    std::thread::scope(|s| {
        // Producer
        s.spawn(|| {
            produce_tasks(
                &state_prod,
                task_tx,
                &token_prod,
                kind_filter_owned.as_deref(),
                &disabled_prod,
                batch_size,
            );
        });

        // Consumer pool (its inner rayon::scope now has the whole rayon pool for decode)
        s.spawn(|| {
            consume_tasks(task_rx, result_tx, &token_consumer, &state_consumer, tuning);
        });

        // Writer
        s.spawn(|| {
            write_results(
                &app_writer,
                &state_writer,
                result_rx,
                &token_writer,
                batch_size,
            );
        });
    });

    // ── Graceful stop requeue (裁决 J10,2026-07-23):三个 worker(生产者/消费者/写入器)已
    // 经上面的 `std::thread::scope` 全部 join、彻底静默。若这是一次主动 stop(token 已取消），
    // 在途任务（部分已 mark_derivations_processing 但因取消提前退出、未写下结果，仍停在
    // status=1）不是挂死——把它们优雅退回 pending，且不计孤儿数，避免用户短时间多次
    // stop/start 同一批任务时被毒任务防线误伤（对照 reset_processing_derivations 仍计数，
    // 用于真正的崩溃/force-quit 场景）。自然完成（未取消）时不会有残留的 status=1 行，
    // 此调用是安全的 no-op。
    // 代次守卫(复核修,2026-07-23):stop→立即重启时,新轮不等旧轮 join 即启动——旧轮末尾
    // 这条 UPDATE 若无守卫,会把**新轮**刚 mark 的在途行打回 pending,新轮生产者重复领取,
    // 同一 (item,kind) 双消费者并发写同一派生产物。仅当本轮仍持槽(纯 stop 后槽空亦视为
    // 持有,见 RunTokenSlot::is_current)才回退;被新轮取代则交给新轮启动 reset 处理(计数,
    // 上方注释已声明该残余窗口)。check 与 UPDATE 之间的微窗口与 149 行同款,后果限于
    // 极小概率多计一次孤儿,可接受。
    if token.is_cancelled() && state.derivation_token.is_current(generation) {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        match requeue_in_flight_derivations(&conn) {
            Ok(n) if n > 0 => info!(
                "Gracefully requeued {} in-flight derivation(s) on stop | 优雅停止：退回 {} 个在途派生任务",
                n, n
            ),
            Ok(_) => {}
            Err(e) => warn!(
                "Failed to requeue in-flight derivations on stop | 优雅停止退回在途派生失败: {}",
                e
            ),
        }
    }

    Ok(())
}

/// 生产者：批量查询待处理任务，标记处理中，推送到通道。
fn produce_tasks(
    state: &Arc<AppState>,
    task_tx: Sender<DerivationTaskMsg>,
    token: &CancellationToken,
    kind_filter: Option<&[String]>,
    exclude_kinds: &[&str],
    batch_size: i64,
) {
    loop {
        if token.is_cancelled() {
            info!("Derivation producer cancelled | 派生生产者已取消");
            break;
        }

        // 让步给更高优先级工作（扫描 / 缩略图），与 AI 生产者一样 sleep。
        // 交互窗口不再拦生产者(T3):填队列是廉价 DB 读写,交互期照常入队,由消费侧涓流控节奏。
        if state.is_scan_or_thumb_running() {
            // reviewer 深审修正:本函数是全部 6 种 DerivationKind(含 doc/audio/ai)共用的通用生产者,
            // 此刻尚未挑到具体 kind,不该挂单一 pipeline target(会让按 ai/doc 过滤时漏看这条)。
            debug!("Derivation producer yielding to scan/thumbnail");
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }

        let conn = match state.db_read_pool.get() {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "DB pool error in derivation producer | 派生生产者 DB 池错误: {}",
                    e
                );
                break;
            }
        };
        let batch = match get_pending_derivations(&conn, batch_size, kind_filter, exclude_kinds) {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    "Query pending derivations failed | 查询待处理派生失败: {}",
                    e
                );
                break;
            }
        };
        drop(conn);

        if batch.is_empty() {
            info!("Derivation producer: no more pending tasks | 派生生产者：无更多待处理任务");
            break;
        }

        // 标记处理中，使在途任务在重启时不被重复排队。
        let candidates: Vec<DerivationClaim> = batch
            .iter()
            .map(|(id, kind, _, _, _, source_revision, cache_key)| {
                (*id, kind.clone(), *source_revision, *cache_key)
            })
            .collect();
        let claimed: HashSet<DerivationClaim> = {
            let write_conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            match mark_derivations_processing(&write_conn, &candidates) {
                Ok(claimed) => claimed.into_iter().collect(),
                Err(e) => {
                    warn!(
                        "Failed to mark derivations processing | 标记派生处理中失败: {}",
                        e
                    );
                    HashSet::new()
                }
            }
        };

        for (item_id, kind_str, abs_path, file_format, media_type, source_revision, cache_key) in
            batch
        {
            if token.is_cancelled() {
                break;
            }
            // 只有通过快照条件认领的任务才允许进入消费者；候选 batch 可能在查询后已经
            // 被其它 producer 领取，或其 source_revision/cache_key 已被扫描推进。
            if !claimed.contains(&(item_id, kind_str.clone(), source_revision, cache_key)) {
                continue;
            }
            // An unknown kind string (e.g. left by a newer build) — leave it processing;
            // a future build that knows it will recover & handle it. Skip here.
            // 未知 kind 字符串（如更高版本遗留）——保持处理中，由认识它的后续构建恢复处理。此处跳过。
            let Some(kind) = DerivationKind::from_str(&kind_str) else {
                warn!(
                    "Unknown derivation kind '{}' for item {} — skipping | 未知派生 kind",
                    kind_str, item_id
                );
                continue;
            };
            if task_tx
                .send(DerivationTaskMsg {
                    item_id,
                    kind,
                    abs_path: PathBuf::from(abs_path),
                    file_format,
                    media_type,
                    source_revision,
                    cache_key,
                })
                .is_err()
            {
                break;
            }
        }
    }

    info!("Derivation producer finished | 派生生产者已完成");
}

/// 消费者池：运行每个任务的 kind 函数，产出一条结果行。
fn consume_tasks(
    task_rx: Receiver<DerivationTaskMsg>,
    result_tx: Sender<DerivationResultWithSnapshot>,
    token: &CancellationToken,
    state: &Arc<AppState>,
    tuning: PipelineTuning,
) {
    // 交互涓流的在途任务计数(T3):派发前 +1,任务结束(含取消早退)经 RAII -1。
    // 仅统计本流水线派生任务,与 rayon 池其它工作无关。
    let in_flight = Arc::new(AtomicUsize::new(0));
    struct InFlightGuard(Arc<AtomicUsize>);
    impl Drop for InFlightGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    rayon::scope(|s| {
        for task in task_rx {
            if token.is_cancelled() {
                break;
            }
            // Dispatch throttle for new heavy decodes (video cover/keyframe):
            //  - scan/thumbnail running → HARD pause (higher tiers keep absolute priority);
            //  - user interacting → TRICKLE: allow 1 in-flight task instead of a full stop.
            //    A full stop starved derivation for the whole browsing session (covers never
            //    appeared); one task occupies one core, foreground compute_layout keeps the rest.
            // 新重型解码(视频封面/关键帧)的派发节流:
            //  - 扫描/缩略图运行中 → 硬暂停(高优先级层绝对优先);
            //  - 用户交互中 → 涓流:保留 1 个在途任务,而非全暂停(T3)。全暂停会让持续浏览期间
            //    派生彻底饿死(封面迟迟不出);单任务只占 1 核,前台 compute_layout 仍有余核。
            loop {
                if token.is_cancelled() {
                    break;
                }
                if state.is_scan_or_thumb_running() {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    continue;
                }
                if state.is_interactive() && in_flight.load(Ordering::Acquire) >= 1 {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    continue;
                }
                break;
            }
            if token.is_cancelled() {
                break;
            }
            // R4：派发前从**共享后台重活池**取 permit（与 exotic Worker 请求同一预算，FIFO 公平）。
            // 在此派发线程（非 rayon worker）阻塞取 permit = 天然「预取不超过可派发容量」；permit 移入
            // 任务闭包，完成/取消即 Drop 释放。取消时 acquire 返回 None → 退出派发循环。
            let Some(permit) = state.background_heavy_limiter.acquire(token) else {
                break;
            };
            let result_tx = result_tx.clone();
            let token_clone = token.clone();
            // 逐任务重克隆(同旧 cache_dir 惯例):`tuning` 需在循环各轮间保持可用,每轮 move 进
            // 闭包的须是新克隆而非外层绑定本身。
            let tuning = tuning.clone();
            in_flight.fetch_add(1, Ordering::AcqRel);
            let in_flight_task = Arc::clone(&in_flight);
            s.spawn(move |_| {
                // 持有 permit 直至任务结束（含提前 return 的取消路径）→ 归还额度。
                let _permit = permit;
                // 在途计数与 permit 同生命周期(Drop -1),涓流判据见上方派发节流。
                let _in_flight = InFlightGuard(in_flight_task);
                if token_clone.is_cancelled() {
                    return;
                }
                let ctx = DerivationContext {
                    item_id: task.item_id,
                    kind: task.kind,
                    abs_path: task.abs_path,
                    file_format: task.file_format,
                    media_type: task.media_type,
                    source_revision: task.source_revision,
                    cache_key: task.cache_key,
                    cache_dir: tuning.cache_dir,
                    thumb_size: tuning.thumb_size,
                    webp_quality: tuning.webp_quality,
                    ai_cache_short_edge: tuning.ai_cache_short_edge,
                    keyframe_count: tuning.keyframe_count,
                    sprite_cell_height: tuning.sprite_cell_height,
                };
                // panic 拦截伞(2026-07-06 审查 P1-1):kind::run 分发大量第三方解码(epub 封面
                // image::load_from_memory/lofty/MF/WIC),裸跑时单个畸形文件 panic 会经
                // rayon::scope → thread::scope → JoinError 中止整条派生流水线,且在途任务留
                // status=1、重启复位后同一毒文件再次触发。包 panic_guard 后 panic → status=3
                // error 行,仅废该项(与缩略图 generator 同款防线)。
                let guarded = crate::thumbnail::generator::panic_guard(
                    &format!("derive:{} item {}", ctx.kind.as_str(), ctx.item_id),
                    || kind::run(&ctx),
                );
                let row: DerivationResultWithSnapshot = match guarded {
                    // status=2 done, with optional payload path + (cover) thumbhash.
                    Ok(out) => (
                        task.item_id,
                        task.kind.as_str().to_string(),
                        2,
                        out.payload_path,
                        None,
                        out.thumbhash,
                        out.page_count,
                        task.source_revision,
                        task.cache_key,
                    ),
                    // status=3 error, store the message for the UI / diagnostics.
                    Err(e) => {
                        // reviewer 深审修正:target 必须是编译期字面量(tracing callsite 元数据在
                        // 编译期固定,不能塞运行时字符串),故按 kind 分三支各自挂字面量 target——
                        // 而非此前不分青红皂白地统一挂 video(doc/audio 派生失败按 ai 过滤时会漏看)。
                        match task.kind {
                            DerivationKind::VideoCover
                            | DerivationKind::VideoKeyframes
                            // 按需 kind(video_playable):get_pending_derivations 已 WHERE 排除
                            // `dv.kind != 'video_playable'`,故绝不会走到流水线 run/此失败分支
                            // (播放派活走 host 侧 VideoWorkerService 交互优先级);此分支仅为
                            // match 穷尽保留,归 video target。
                            | DerivationKind::VideoPlayable => debug!(
                                target: "scrollery::pipeline::video",
                                kind = task.kind.as_str(), item_id = task.item_id, error = %e,
                                "Derivation failed for item"
                            ),
                            DerivationKind::AiThumb => debug!(
                                target: "scrollery::pipeline::ai",
                                kind = task.kind.as_str(), item_id = task.item_id, error = %e,
                                "Derivation failed for item"
                            ),
                            DerivationKind::DocThumb
                            | DerivationKind::AudioCover
                            | DerivationKind::AudioMeta => debug!(
                                kind = task.kind.as_str(), item_id = task.item_id, error = %e,
                                "Derivation failed for item"
                            ),
                        }
                        (
                            task.item_id,
                            task.kind.as_str().to_string(),
                            3,
                            None,
                            Some(e.to_string()),
                            None,
                            None,
                            task.source_revision,
                            task.cache_key,
                        )
                    }
                };
                let _ = result_tx.send(row);
            });
        }
    });
    info!("Derivation consumers finished | 派生消费者已完成");
}

fn cover_thumb_from_result(row: &DerivationResultWithSnapshot) -> Option<ThumbResult> {
    let (
        item_id,
        kind,
        status,
        payload_path,
        _error,
        thumbhash,
        _page_count,
        source_revision,
        cache_key,
    ) = row;
    let kind = DerivationKind::from_str(kind)?;
    if *status != 2 || !kind.produces_thumbnail() {
        return None;
    }
    Some(ThumbResult {
        item_id: *item_id,
        thumb_status: 1,
        thumb_path: payload_path.clone(),
        thumbhash: thumbhash.clone(),
        source_revision: *source_revision,
        cache_key: *cache_key,
    })
}

/// Writer: batch-collect results and persist status/payload to the DB. For cover-producing
/// kinds (video/audio cover, doc thumb) it ALSO mirrors `thumb_status=1 / thumb_path / thumbhash`
/// onto `media_items` and the resident layout cache, then nudges the gallery to refresh — so
/// `MediaThumb` shows freshly-derived covers with zero frontend changes (invariant §1.3.4).
/// 写入器：批量收集结果并把状态/产物持久化到 DB。对封面类 kind（视频/音频封面、文档缩略图），
/// 还把 `thumb_status=1 / thumb_path / thumbhash` 回填到 `media_items` 与常驻布局缓存，
/// 再通知画廊刷新 —— 使 `MediaThumb` 零改动即可显示新派生的封面（不变量 §1.3.4）。
fn write_results(
    app: &AppHandle,
    state: &Arc<AppState>,
    result_rx: Receiver<DerivationResultWithSnapshot>,
    token: &CancellationToken,
    batch_size: i64,
) {
    let mut batch: Vec<DerivationResultWithSnapshot> = Vec::with_capacity(batch_size as usize);
    let mut done = 0u64;
    let mut errored = 0u64;
    let mut covers_landed = false;

    let flush = |state: &Arc<AppState>,
                 batch: &mut Vec<DerivationResultWithSnapshot>,
                 done: &mut u64,
                 errored: &mut u64,
                 covers_landed: &mut bool| {
        if batch.is_empty() {
            return;
        }
        let accepted = {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            match batch_finish_derivations_with_snapshot(&conn, batch) {
                Ok(accepted) => {
                    // `conn` writer lock 仍在持有：源代次变更也必须经过该锁，因此 accepted
                    // 后立即 patch 常驻缓存不会被同一时间窗的扫描更新穿透。
                    let cover_thumbs: Vec<ThumbResult> = accepted
                        .iter()
                        .filter_map(cover_thumb_from_result)
                        .collect();
                    if !cover_thumbs.is_empty() {
                        state.apply_thumb_results(&cover_thumbs);
                        *covers_landed = true;
                    }
                    accepted
                }
                Err(e) => {
                    warn!("Batch derivation write failed | 批量派生写入失败: {}", e);
                    Vec::new()
                }
            }
        };

        // 计数只统计通过快照 CAS 的结果；过期 worker 的结果不会进入任何持久化或缓存落点。
        // 同步常驻布局缓存（按 id 索引 O(batch)），使封面在滚出再滚回时无需整表重算（不变量 §1.3.4）。
        for (_, _, status, _, _, _, _, _, _) in accepted.iter() {
            if *status == 2 {
                *done += 1;
            } else {
                *errored += 1;
            }
        }
        batch.clear();
    };

    for row in result_rx {
        if token.is_cancelled() {
            info!("Derivation writer cancelled | 派生写入器已取消");
            break;
        }
        batch.push(row);
        if batch.len() >= batch_size as usize {
            flush(
                state,
                &mut batch,
                &mut done,
                &mut errored,
                &mut covers_landed,
            );
        }
    }
    flush(
        state,
        &mut batch,
        &mut done,
        &mut errored,
        &mut covers_landed,
    );

    // Nudge the gallery to recompute/refresh visible rows once covers have landed. Reuses the
    // enrichment event — MediaGrid already debounce-recomputes on it (payload ignored).
    // 封面落地后通知画廊重算/刷新可见行。复用 enrichment 事件 —— MediaGrid 已对其防抖重算（忽略 payload）。
    if covers_landed {
        let _ = app.emit("db:media_enriched", MediaEnrichedPayload::refresh_signal());
    }

    info!(
        "Derivation writer finished: done={} error={} | 派生写入器完成: 完成={} 错误={}",
        done, errored, done, errored
    );
}

/// 派生任务按状态的一次性计数，用于状态摘要 IPC。
pub fn derivation_counts(state: &AppState) -> crate::error::Result<(i64, i64, i64, i64)> {
    let conn = state.db_read_pool.get()?;
    count_derivations_by_status(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_cover_patch_keeps_the_worker_source_snapshot() {
        let row: DerivationResultWithSnapshot = (
            7,
            "video_cover".to_string(),
            2,
            Some("480/7.webp".to_string()),
            None,
            Some(vec![1, 2, 3]),
            None,
            19,
            7001,
        );
        let patch =
            cover_thumb_from_result(&row).expect("cover result should become a cache patch");
        assert_eq!(patch.item_id, 7);
        assert_eq!(patch.source_revision, 19);
        assert_eq!(patch.cache_key, 7001);
    }

    #[test]
    fn non_cover_or_failed_results_never_become_resident_cover_patches() {
        let failed: DerivationResultWithSnapshot = (
            7,
            "video_cover".to_string(),
            3,
            None,
            Some("failed".to_string()),
            None,
            None,
            19,
            7001,
        );
        let keyframes: DerivationResultWithSnapshot = (
            7,
            "video_keyframes".to_string(),
            2,
            Some("video/7.webp".to_string()),
            None,
            None,
            None,
            19,
            7001,
        );
        assert!(cover_thumb_from_result(&failed).is_none());
        assert!(cover_thumb_from_result(&keyframes).is_none());
    }
}
