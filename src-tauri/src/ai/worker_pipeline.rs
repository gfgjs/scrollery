// src-tauri/src/ai/worker_pipeline.rs
//! AI 分析流水线的 worker 派发路径(Part4-T17;T16 起为**唯一**路径,进程内 ort 推理已删)。
//!
//! 控制面(Producer 领取/让步/续传、Writer 落库/状态机)与进程内路径**共用同一套代码**
//! (pipeline.rs 的 produce_tasks/write_results);只有中段不同——进程内的
//! 「预处理线程 + 推理线程」被替换为单条派发线程:
//!
//!   Producer → 攒批 → [CPU permit → GPU 令牌] → AiWorkerClient(EmbedBatch)→ Writer
//!
//! 关键语义:
//!   - **派发前保证 ai_cache 就位**(T18,Part4 §3.8):EmbedItem 只载 cache_key、worker
//!     不解原图,缺缓存的项由 host 在 CPU permit 内**现场派生**(解原图短边 336 → 原子写,
//!     与派生管线共用 generate_ai_cache);派生失败标 Error(同进程内解码失败语义)。
//!     首跑全库现场派生性能次优;新导入项由缩略图流水线顺带产出/派生侧预产覆盖(快路径)。
//!   - **先 CPU permit 后 GPU 令牌**(D2 顺序天条):每批派发前依次取
//!     `background_heavy_limiter` 与 `gpu_token`,批结束即释放(RAII)。
//!   - 逐项结果:Ok → 写库;retryable Err(IoError 等瞬态)→ 跳过(同缓存缺失语义);
//!     terminal Err → 标 Error(不再重查)。
//!   - 批级失败(client 硬止损后仍失败/terminal Failure)→ 终止本轮(系统性错误,
//!     继续硬跑只会逐批重复失败);在途项保持 Processing,下次运行恢复。
//!   - 运行结束(自然/取消)即 `close_session` 卸载会话释放 VRAM,对齐进程内路径
//!     「结束即置空引擎」;worker 进程留存,空闲 300s 自杀兜底(D3 §4④)。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use exotic_protocol::EmbedItem;
use rayon::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::ai::pipeline::{
    produce_tasks, recover_orphaned_ai_items, write_results, AiResult, AiTask,
};
use crate::ai::worker_client::{build_session_spec, SessionSpec};
use crate::error::AppError;
use crate::exotic::worker::EmbedItemOutcome;
use crate::state::AppState;
use crate::utils::hash::cache_key_to_hex;

/// 通道容量(与进程内路径同值)。
const CHANNEL_CAPACITY: usize = 1024;
/// 攒批的空闲刷新周期(与进程内推理线程同值:低速进图时不无限等满批)。
const FLUSH_TIMEOUT: Duration = Duration::from_millis(50);

/// worker 派发路径主入口(由 pipeline::run_pipeline_blocking 无条件调用;T16 后为唯一路径)。
pub(crate) fn run_pipeline_worker_blocking(
    state: &Arc<AppState>,
    token: &CancellationToken,
) -> crate::error::Result<()> {
    // 活跃 profile 纯由配置解析(不触进程内引擎——worker 模式全程零 ort)。
    let profile = crate::ai::runtime_config::active_profile(state);
    let spec = build_session_spec(state, profile.clone(), None);

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

    rayon::scope(|s| {
        s.spawn(|_| {
            produce_tasks(&state_prod, task_tx, &token_prod);
        });

        let state_disp = Arc::clone(state);
        let token_disp = token.clone();
        let fatal_ref = &fatal;
        let spec_ref = &spec;
        s.spawn(move |_| {
            if let Err(e) = dispatch_loop(&state_disp, spec_ref, task_rx, result_tx, &token_disp) {
                *fatal_ref.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
            }
        });

        s.spawn(move |_| {
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

/// 派发线程主循环:攒批 → 派发;通道关闭(生产者收尾)时刷余批后退出。
fn dispatch_loop(
    state: &Arc<AppState>,
    spec: &SessionSpec,
    task_rx: Receiver<AiTask>,
    result_tx: Sender<AiResult>,
    token: &CancellationToken,
) -> Result<(), String> {
    let batch_cap = (spec.batch_size as usize).max(1);
    let mut buf: Vec<AiTask> = Vec::with_capacity(batch_cap);
    // 瞬态失败的跳过计数(保持 Processing,下次运行恢复)。缺缓存不再跳过——T18 起
    // host 现场派生(见 dispatch_batch)。
    let mut skipped: u64 = 0;

    loop {
        if token.is_cancelled() {
            info!("AI worker 派发已取消 | dispatcher cancelled");
            break;
        }
        match task_rx.recv_timeout(FLUSH_TIMEOUT) {
            Ok(task) => {
                buf.push(task);
                if buf.len() >= batch_cap {
                    dispatch_batch(state, spec, &mut buf, &result_tx, token, &mut skipped)?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if !buf.is_empty() {
                    dispatch_batch(state, spec, &mut buf, &result_tx, token, &mut skipped)?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                if !buf.is_empty() {
                    dispatch_batch(state, spec, &mut buf, &result_tx, token, &mut skipped)?;
                }
                break;
            }
        }
    }

    if skipped > 0 {
        // 不静默截断(工作约定):瞬态失败的项保持 Processing,下次运行回 Pending。
        warn!("AI worker 派发跳过 {skipped} 项(瞬态失败;保持 Processing,下次运行恢复)");
    }
    info!("AI worker 派发完成 | dispatcher finished");
    Ok(())
}

/// 派发一批:CPU permit → 缺缓存现场派生(T18 降级;批内 rayon 并行)→ GPU 令牌 →
/// EmbedBatch(worker 端批内并行解码,T18.5)→ 逐项落结果。
/// 返回 Err = 批级致命(终止本轮);取消返回 Ok 且清空 buf(在途项保持 Processing)。
fn dispatch_batch(
    state: &Arc<AppState>,
    spec: &SessionSpec,
    buf: &mut Vec<AiTask>,
    result_tx: &Sender<AiResult>,
    token: &CancellationToken,
    skipped: &mut u64,
) -> Result<(), String> {
    let tasks: Vec<AiTask> = std::mem::take(buf);

    // D2 顺序天条:先 CPU permit(公平后台池)后 GPU 令牌;None = 已取消,直接收手
    // (本批项保持 Processing,下次运行恢复)。两者随作用域 Drop 释放。
    // permit 提前到生成段之前:缺缓存的现场派生是重 CPU 解码,必须在后台池配额内。
    let Some(_cpu_permit) = state.background_heavy_limiter.acquire(token) else {
        return Ok(());
    };

    // T18(Part4 §3.8):EmbedItem 只载 cache_key、worker 不解原图,故缺 ai_cache 的项由
    // host **现场派生**(解原图短边 336 → 原子写,与派生管线共用 generate_ai_cache)——
    // 功能正确、首跑性能次优;新导入项由缩略图流水线顺带产出/派生侧预产覆盖(快路径)。
    // cache_key 含 mtime(xxh3 键),兼作陈旧防护指纹。
    let (cache_dir, ai_cache_short_edge) = {
        let cfg = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
        (cfg.cache_dir.clone(), cfg.ai_cache_short_edge)
    };

    // 缺缓存的现场派生**并行化**(T18.5;rayon 全局池,与进程内预处理的并行语义对齐——
    // 进程内路径同样在全局池上解码,CPU permit 保持「1 批=1 槽」记账)。取消检查在
    // 每项开工前:已派生项落盘幂等可复用,未开工项随本批放弃(保持 Processing)。
    let derive_failed: std::collections::HashSet<i64> = tasks
        .par_iter()
        .filter(|t| !crate::thumbnail::cache::ai_cache_path(&cache_dir, t.cache_key).exists())
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
        return Ok(()); // 派生中途取消:本批项保持 Processing,下次运行恢复。
    }

    let mut items: Vec<EmbedItem> = Vec::with_capacity(tasks.len());
    // (item_id, cache_key 快照):cache_key 随结果传给 Writer 做 X1 条件写。
    let mut item_keys: Vec<(i64, i64)> = Vec::with_capacity(tasks.len());
    for t in &tasks {
        if derive_failed.contains(&t.item_id) {
            let _ = result_tx.send(AiResult {
                item_id: t.item_id,
                cache_key: t.cache_key,
                embedding: None,
            });
            continue;
        }
        let hex = cache_key_to_hex(t.cache_key);
        items.push(EmbedItem {
            item_id: t.item_id,
            cache_key: hex.clone(),
            fingerprint: hex,
        });
        item_keys.push((t.item_id, t.cache_key));
    }
    if items.is_empty() {
        return Ok(());
    }

    let Some(_gpu_permit) = state.gpu_token.acquire(token) else {
        return Ok(());
    };

    let outcomes = {
        let mut client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
        client.embed_batch(spec, &items, &|| token.is_cancelled())
    };
    let outcomes = match outcomes {
        Ok(o) => o,
        Err(e) => {
            // client 已做硬止损(重建重发一次);到这里即系统性失败,终止本轮。
            return Err(e.to_string());
        }
    };

    for ((item_id, cache_key), outcome) in item_keys.into_iter().zip(outcomes) {
        match outcome {
            EmbedItemOutcome::Ok(embedding) => {
                let bytes = crate::ai::clip::embedding_to_bytes(&embedding);
                let _ = result_tx.send(AiResult {
                    item_id,
                    cache_key,
                    embedding: Some(bytes),
                });
            }
            EmbedItemOutcome::Err(code) if code.default_retryable() => {
                // 瞬态(IoError 等):同缓存缺失语义——跳过,保持 Processing。
                *skipped += 1;
            }
            EmbedItemOutcome::Err(code) => {
                // terminal(MalformedInput 等):标 Error,不再无限重查。
                warn!("item {item_id} 嵌入失败[{}](terminal)", code.as_str());
                let _ = result_tx.send(AiResult {
                    item_id,
                    cache_key,
                    embedding: None,
                });
            }
        }
    }
    Ok(())
}
