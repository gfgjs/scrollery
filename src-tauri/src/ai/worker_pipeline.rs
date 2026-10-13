// src-tauri/src/ai/worker_pipeline.rs
//! AI 分析流水线的 worker 派发路径。
//!
//! Producer 领取/让步/续传与 Writer 落库/状态机由 pipeline.rs 处理。
//! 准备与推理分别在专用线程协调，批内派生留在 rayon 池：
//!
//!   Producer → CPU 准备 → 单批 Ready 交接 → [CPU permit → GPU 令牌]
//!   → AiWorkerClient(EmbedBatch) → Writer
//!
//! 关键语义:
//!   - **派发前保证 ai_cache 就位**:优先指定当前已提交的 AI 缓存路径；worker
//!     不解原图,缺缓存的项由 host 在 CPU permit 内**现场派生**(解原图短边 336 → 原子写,
//!     与派生管线共用 generate_ai_cache);派生失败标 Error(同进程内解码失败语义)。
//!     首跑全库现场派生性能次优;新导入项由缩略图流水线顺带产出/派生侧预产覆盖(快路径)。
//!   - 准备仅取 CPU 配额，交接前释放；推理先 CPU permit 后 GPU 令牌，
//!     结果交接前释放，慢下游不会占用资源配额。
//!   - 逐项结果:Ok → 写库;retryable Err(IoError 等瞬态)→ 跳过(同缓存缺失语义);
//!     terminal Err → 标 Error(不再重查)。
//!   - 批级失败(client 硬止损后仍失败/terminal Failure)→ 终止本轮(系统性错误,
//!     继续硬跑只会逐批重复失败);在途项保持 Processing,下次运行恢复。
//!   - 运行结束或取消时 `close_session` 卸载会话释放 VRAM；worker 进程空闲 300 秒后退出。

use std::sync::{Arc, Mutex};

use crossbeam_channel::{bounded, Receiver, Sender};
use exotic_protocol::EmbedItem;
use rayon::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ai::pipeline::{
    produce_tasks, recover_orphaned_ai_items, write_results, AiResult, AiTask,
};
use crate::ai::worker_client::{build_session_spec, SessionPurpose, SessionSpec};
use crate::error::AppError;
use crate::exotic::worker::EmbedItemOutcome;
use crate::state::AppState;
use crate::utils::hash::cache_key_to_hex;

/// 生产者、派发器与写入器之间的通道容量。
const CHANNEL_CAPACITY: usize = 1024;

/// worker 派发路径主入口，由 `pipeline::run_pipeline_blocking` 调用。
pub(crate) fn run_pipeline_worker_blocking(
    state: &Arc<AppState>,
    token: &CancellationToken,
) -> crate::error::Result<()> {
    // 活跃 profile 纯由配置解析(不触进程内引擎——worker 模式全程零 ort)。
    let profile = crate::ai::runtime_config::active_profile(state);
    let spec = build_session_spec(state, profile.clone(), SessionPurpose::Image);

    recover_orphaned_ai_items(state);

    let read_conn = state.db_read_pool.get()?;
    let total = crate::db::queries::count_pending_ai_items(&read_conn)?;
    drop(read_conn);
    info!(
        "AI worker 流水线启动:待分析 {total} 张(backend=worker, arch={}, batch={})",
        spec.profile.id, spec.batch_size
    );

    let (task_tx, task_rx) = bounded::<AiTask>(CHANNEL_CAPACITY);
    let (result_tx, result_rx) = bounded::<AiResult>(CHANNEL_CAPACITY);

    // 派发线程的批级致命错误经此带出 scope(系统性错误须让运行以 Err 收场,
    // 而非静默「完成」)。
    let fatal: Mutex<Option<String>> = Mutex::new(None);

    let token_prod = token.clone();
    let state_prod = Arc::clone(state);
    let token_writer = token.clone();
    let state_writer = Arc::clone(state);
    let profile_writer = profile.clone();

    std::thread::scope(|s| {
        s.spawn(|| {
            produce_tasks(&state_prod, task_tx, &token_prod);
        });

        let state_disp = Arc::clone(state);
        let token_disp = token.clone();
        let fatal_ref = &fatal;
        let spec_ref = &spec;
        s.spawn(move || {
            if let Err(e) = dispatch_loop(&state_disp, spec_ref, task_rx, result_tx, &token_disp) {
                *fatal_ref.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
            }
        });

        s.spawn(move || {
            write_results(&state_writer, result_rx, &token_writer, &profile_writer);
        });
    });

    // provider 回声落库(T16:探测发生在 worker 侧,写回配置供状态栏读取)——
    // 须在 close_session 之前(快照随 close 清空)。
    crate::ai::runtime_config::persist_provider_echo(state);
    // 结束即卸会话(自然完成/取消皆是;对齐进程内旧行为的 VRAM 语义)。
    state
        .ai_worker
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .close_session();

    match fatal.into_inner().unwrap_or_else(|p| p.into_inner()) {
        Some(e) => Err(AppError::internal(
            "AI worker 派发终止 | dispatch failed",
            e,
        )),
        None => Ok(()),
    }
}

/// 准备后继批并推理当前批；输入关闭时处理尾批，取消或批级错误停止两阶段。
fn dispatch_loop(
    state: &Arc<AppState>,
    spec: &SessionSpec,
    task_rx: Receiver<AiTask>,
    result_tx: Sender<AiResult>,
    token: &CancellationToken,
) -> Result<(), String> {
    let mut skipped = 0;
    crate::ai::prepared::run_prepared_batches(
        task_rx,
        spec.batch_size as usize,
        token,
        |tasks, local| prepare_batch(state, spec, tasks, &result_tx, local),
        |ready, wait_ms, local| {
            infer_batch(state, spec, ready, &result_tx, local, wait_ms, &mut skipped)
        },
    )?;
    if skipped > 0 {
        warn!("AI worker 派发跳过 {skipped} 项(瞬态失败;保持 Processing,下次运行恢复)");
    }
    info!("AI worker 派发完成 | dispatcher finished");
    Ok(())
}

struct ReadyAiBatch {
    items: Vec<EmbedItem>,
    item_keys: Vec<(i64, i64, i64)>,
    preparation_ms: u128,
    preparation_cpu_wait_ms: u128,
}

/// 在 CPU 配额内准备输入路径；返回前释放配额，交接等待不阻塞其它后台重活。
fn prepare_batch(
    state: &Arc<AppState>,
    spec: &SessionSpec,
    tasks: Vec<AiTask>,
    result_tx: &Sender<AiResult>,
    token: &CancellationToken,
) -> Result<Option<ReadyAiBatch>, String> {
    let preparation_start = std::time::Instant::now();

    // 缺缓存的现场派生是重 CPU 解码，须取得后台配额；交接前释放以避免背压死锁。
    let Some(cpu_permit) = state.background_heavy_limiter.acquire(token) else {
        return Ok(None);
    };

    let preparation_cpu_wait_ms = preparation_start.elapsed().as_millis();

    // 只复用已提交且源代次/缓存键仍匹配的 AI 产物；无命中仍走独立 AI 的旧缓存与派生。
    let (cache_dir, ai_cache_short_edge) = {
        let cfg = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
        (cfg.cache_dir.clone(), cfg.ai_cache_short_edge)
    };

    if cache_dir.join("ai_thumbs") != spec.ai_cache_dir {
        return Err("AI cache directory changed during session".into());
    }
    let requested: Vec<_> = tasks
        .iter()
        .map(|task| (task.item_id, task.cache_key, task.source_revision))
        .collect();
    let committed = {
        let conn = state
            .db_read_pool
            .get()
            .map_err(|error| error.to_string())?;
        crate::db::queries::find_ai_cache_artifacts(&conn, &requested, ai_cache_short_edge)
            .map_err(|error| error.to_string())?
    };
    let cache_paths: std::collections::HashMap<i64, String> = tasks
        .iter()
        .filter_map(|task| {
            committed_cache_path(
                task,
                ai_cache_short_edge,
                committed.get(&task.item_id).map(String::as_str),
                &spec.ai_cache_dir,
            )
            .map(|path| (task.item_id, path))
        })
        .collect();

    // 缺缓存的现场派生**并行化**(T18.5;rayon 全局池,与进程内预处理的并行语义对齐——
    // 进程内路径同样在全局池上解码,CPU permit 保持「1 批=1 槽」记账)。取消检查在
    // 每项开工前:已派生项落盘幂等可复用,未开工项随本批放弃(保持 Processing)。
    let derive_failed: std::collections::HashSet<i64> = tasks
        .par_iter()
        .filter(|t| {
            !cache_paths.contains_key(&t.item_id)
                && !crate::thumbnail::cache::ai_cache_path(&cache_dir, t.cache_key).exists()
        })
        .filter_map(|t| {
            if token.is_cancelled() {
                return None;
            }
            // panic 拦截伞(与 derive/pipeline.rs kind::run 同款防线):worker 端 rayon
            // par_iter 内裸跑第三方解码,单个畸形文件 panic 会中止整批,需转为 Err
            // 落入既有 derive_failed 失败路径(标 Error,不连坐整批)。
            crate::thumbnail::generator::panic_guard(
                &format!("ai_worker:generate_ai_cache item {}", t.item_id),
                || {
                    crate::derive::image::generate_ai_cache(
                        &cache_dir,
                        t.cache_key,
                        &t.file_format,
                        &t.source_path,
                        ai_cache_short_edge,
                    )
                },
            )
            .err()
            .map(|e| {
                // 与进程内路径的解码失败同语义:标 Error(不再无限重查),不连坐整批。
                warn!("item {} ai_cache 现场派生失败:{e}(标 Error)", t.item_id);
                t.item_id
            })
        })
        .collect();
    if token.is_cancelled() {
        return Ok(None); // 派生中途取消:本批项保持 Processing,下次运行恢复。
    }

    let mut failed_results = Vec::new();
    let mut items: Vec<EmbedItem> = Vec::with_capacity(tasks.len());
    // 源代次与缓存键随结果传给 Writer，阻止同 key 换源后的迟到结果入库。
    let mut item_keys: Vec<(i64, i64, i64)> = Vec::with_capacity(tasks.len());
    for t in &tasks {
        if derive_failed.contains(&t.item_id) {
            failed_results.push(AiResult {
                item_id: t.item_id,
                cache_key: t.cache_key,
                source_revision: t.source_revision,
                embedding: None,
            });
            continue;
        }
        let hex = cache_key_to_hex(t.cache_key);
        items.push(EmbedItem {
            item_id: t.item_id,
            cache_key: hex.clone(),
            cache_path: cache_paths.get(&t.item_id).cloned(),
            fingerprint: format!("r{}:{hex}", t.source_revision),
        });
        item_keys.push((t.item_id, t.cache_key, t.source_revision));
    }
    drop(cpu_permit);
    for result in failed_results {
        if !crate::ai::prepared::send_cancellable(result_tx, result, token) {
            return if token.is_cancelled() {
                Ok(None)
            } else {
                Err("AI writer closed".into())
            };
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(ReadyAiBatch {
        items,
        item_keys,
        preparation_ms: preparation_start.elapsed().as_millis(),
        preparation_cpu_wait_ms,
    }))
}

fn infer_batch(
    state: &Arc<AppState>,
    spec: &SessionSpec,
    ready: ReadyAiBatch,
    result_tx: &Sender<AiResult>,
    token: &CancellationToken,
    ready_wait_ms: u128,
    skipped: &mut u64,
) -> Result<(), String> {
    let ReadyAiBatch {
        items,
        item_keys,
        preparation_ms,
        preparation_cpu_wait_ms,
    } = ready;
    let cpu_start = std::time::Instant::now();
    let Some(cpu_permit) = state.background_heavy_limiter.acquire(token) else {
        return Ok(());
    };
    let inference_cpu_wait_ms = cpu_start.elapsed().as_millis();
    let gpu_start = std::time::Instant::now();
    let Some(gpu_permit) = state.gpu_token.acquire(token) else {
        return Ok(());
    };
    let gpu_wait_ms = gpu_start.elapsed().as_millis();
    let worker_start = std::time::Instant::now();
    let outcomes = {
        let mut client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
        client.embed_batch(spec, &items, &|| token.is_cancelled())
    };
    let outcomes = match outcomes {
        Ok(outcomes) => outcomes,
        // 明确取消不升级为派发故障，准备阶段错误仍由批编排保留。
        Err(AppError::Cancelled) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    info!("AI_HOST_PERF items={} prepare_ms={} prepare_cpu_wait_ms={} ready_wait_ms={} infer_cpu_wait_ms={} gpu_wait_ms={} worker_ms={}",
        items.len(), preparation_ms, preparation_cpu_wait_ms, ready_wait_ms, inference_cpu_wait_ms,
        gpu_wait_ms, worker_start.elapsed().as_millis());
    drop(gpu_permit);
    drop(cpu_permit);
    for ((item_id, cache_key, source_revision), outcome) in item_keys.into_iter().zip(outcomes) {
        let embedding = match outcome {
            EmbedItemOutcome::Ok(embedding) => {
                Some(crate::ai::clip::embedding_to_bytes(&embedding))
            }
            EmbedItemOutcome::Err(code) if code.default_retryable() => {
                *skipped += 1;
                continue;
            }
            EmbedItemOutcome::Err(code) => {
                warn!("item {item_id} 嵌入失败[{}](terminal)", code.as_str());
                None
            }
        };
        if !crate::ai::prepared::send_cancellable(
            result_tx,
            AiResult {
                item_id,
                cache_key,
                source_revision,
                embedding,
            },
            token,
        ) {
            return if token.is_cancelled() {
                Ok(())
            } else {
                Err("AI writer closed".into())
            };
        }
    }
    Ok(())
}

/// 只有 DB 返回的当前完整身份路径且文件存在，才跳过独立 AI 现场派生。
fn committed_cache_path(
    task: &AiTask,
    short_edge: u32,
    committed: Option<&str>,
    ai_cache_dir: &std::path::Path,
) -> Option<String> {
    let hex = cache_key_to_hex(task.cache_key);
    let relative = format!(
        "v2/{short_edge}/{}/r{}/{hex}.webp",
        &hex[..2],
        task.source_revision
    );
    (committed == Some(format!("ai_thumbs/{relative}").as_str())
        && ai_cache_dir.join(&relative).is_file())
    .then_some(relative)
}
