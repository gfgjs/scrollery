// src-tauri/src/ai/pipeline.rs
//! Background AI analysis pipeline — 控制面(Producer/Writer)与入口。
//! 后台 AI 分析流水线:控制面(生产者/写入器)与入口。
//!
//! T16 收束:推理恒经 ai-worker 子进程派发(worker_pipeline.rs),本模块只保留
//! 两条路径曾共用的控制面——Producer(领取/让步/续传)、Writer(落库/状态机)、
//! 孤儿恢复与 batch 解析。进程内 ort 推理中段(预处理线程池/推理线程/解码源决策)
//! 已随 T16 删除,历史实现见 git。
//!
//! 1. Producer: batch-query media_items WHERE ai_status=0 → AiTask 通道
//! 2. 中段: worker_pipeline dispatch(攒批 → CPU permit → GPU 令牌 → EmbedBatch)
//! 3. Writer: 批量收集结果,写 ai_embeddings + ai_status,失效嵌入缓存
//! 4. 每批检查 ai_yield_blockers() + CancellationToken

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::ai::profile::ModelProfile;
use crate::db::models::AiStatus;
use crate::db::queries::{
    batch_finish_ai_items, batch_update_ai_status, batch_update_ai_status_guarded,
    get_pending_ai_items,
};
use crate::state::AppState;

/// 从数据库读取和写入嵌入向量的批次大小。
const BATCH_SIZE: i64 = 512;

/// 写入器时间驱动 flush 上界(2026-07-11 加固批 B-2):批未满也按此陈龄落库,
/// 「满 512 或 3s 先到先落」。进度可见性与崩溃丢失窗口由此封顶;快速稳态仍满批走,
/// 写事务频次不受影响(专家建议的固定 16/64 小批会把写事务放大 8-32 倍,不取)。
pub(crate) const WRITE_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// 从生产者发送到 worker 派发线程的任务项。
pub(crate) struct AiTask {
    pub(crate) item_id: i64,
    /// 原图绝对路径(缺 ai_cache 时现场派生的解码源,T18)。
    pub(crate) source_path: PathBuf,
    pub(crate) file_format: String,
    /// 经 `ai_cache_path(cache_dir, cache_key)` 定位 AI 缓存;worker 端解码的唯一源。
    pub(crate) cache_key: i64,
}

/// 从消费者发送到写入器的嵌入向量结果。
pub(crate) struct AiResult {
    pub(crate) item_id: i64,
    /// 任务领取时的 `cache_key` 快照(X1 条件写:落库仅当行内当前值仍相等——SourceChanged
    /// 失效会换 key,迟到写落空)。
    pub(crate) cache_key: i64,
    /// 成功时为 `Some(bytes)`，推理失败时为 `None`。
    pub(crate) embedding: Option<Vec<u8>>,
}

/// 启动后台 AI 分析流水线。
///
/// 立即返回；所有工作在后台线程中完成。
pub fn start_ai_pipeline(state: Arc<AppState>, generation: u64, token: CancellationToken) {
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        // span 埋点(W1,方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入):覆盖整个 run
        // 的墙钟时间,块尾自然 Drop——正常完成/失败/panic 三条路径都会触达(async 块作用域即可,
        // 不必逐分支手动收尾)。既有的完成/失败/panic 汇总日志(下方 info!/warn!)保留不动。
        let _span = crate::logging::SpanTimer::info("pipeline:ai");
        let start_time = std::time::Instant::now();
        // 保留一个 token 句柄，使阻塞运行返回后能区分自然完成与暂停/停止取消（问题7）。
        let token_outer = token.clone();
        // 在 spawn_blocking 中运行阻塞工作，避免阻塞异步运行时。
        let result =
            tokio::task::spawn_blocking(move || run_pipeline_blocking(&state_clone, &token)).await;

        let elapsed_ms = start_time.elapsed().as_millis();
        match result {
            Ok(Ok(())) => info!(
                "AI analysis pipeline completed: elapsed={}ms | AI 分析流水线完成: 耗时={}ms",
                elapsed_ms, elapsed_ms
            ),
            Ok(Err(e)) => warn!("AI analysis pipeline error | AI 分析流水线错误: {}", e),
            Err(e) => warn!("AI analysis task panicked | AI 分析任务崩溃: {}", e),
        }

        // 若自然完成（未被暂停/停止取消），清除自动续传标志——无可续传。被取消则保留标志为
        // 暂停/停止命令设定的值。（问题7）
        if !token_outer.is_cancelled() {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            let _ = crate::db::queries::set_config(&conn, "ai_analysis_active", "0");
            drop(conn);
            // 仅在自然完成时释放共享 GPU 分析槽（F5 互斥）。被取消的情形要么是暂停/停止（由命令
            // 释放槽），要么是 restart（取消后重启同一流水线，须保持持有）——此处若在取消时释放，
            // 会让刚重启的运行丢掉槽位。
            state.release_gpu_analysis(crate::state::GPU_OWNER_AI);
        }

        // (2026-07-10 审查 F10):旧轮 blocking 任务迟退出时,槽内可能已是 restart 装的新一轮
        // token,无条件 take+cancel 会静默中断新运行;仅当仍是本轮才清。
        // 完成后按代次清除令牌(仅当槽内仍是本轮)。返回值刻意丢弃:本流水线的终态副作用
        // 门控在上面的 `!token_outer.is_cancelled()`,不采用 thumb 的 finish-bool 门控姿态。
        let _ = state.finish_ai_analysis(generation);
    });
}

/// Blocking pipeline runner:T16 起恒走 worker 派发(进程内 ort 路径已删)。
fn run_pipeline_blocking(
    state: &Arc<AppState>,
    token: &CancellationToken,
) -> crate::error::Result<()> {
    crate::ai::worker_pipeline::run_pipeline_worker_blocking(state, token)
}

/// 孤儿恢复(问题7,进程内与 worker 两条路径共用):Processing → Pending。
pub(crate) fn recover_orphaned_ai_items(state: &Arc<AppState>) {
    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    match crate::db::queries::reset_processing_ai_items(&conn) {
        Ok(n) if n > 0 => info!("Recovered {} orphaned AI items (processing → pending) | 恢复 {} 个孤儿 AI 项（处理中 → 待处理）", n, n),
        Ok(_) => {}
        Err(e) => warn!("Failed to recover orphaned AI items | 恢复孤儿 AI 项失败: {}", e),
    }
}

/// 生产者：批量查询待处理项，推送任务到通道。
pub(crate) fn produce_tasks(
    state: &Arc<AppState>,
    task_tx: Sender<AiTask>,
    token: &CancellationToken,
) {
    // 让步阻塞源变化追踪(可观测性三修 #1):集合与上次不同(含首次非空/恢复为空)才 info,
    // 未变化保持 debug——避免让步期间刷屏,同时不再让「阻塞源常驻(如派生挂死)→AI 永久
    // 让步」在日志里悄无声息(本次故障:用户点「开始」后零反馈、日志零可见，被误判为没跑起来)。
    let mut last_blockers: Option<Vec<&'static str>> = None;
    loop {
        if token.is_cancelled() {
            info!("AI producer cancelled | AI 生产者已取消");
            break;
        }

        // 让步给更高优先级的任务，并记录具体阻塞源便于排查。
        let blockers = state.ai_yield_blockers();
        if !blockers.is_empty() {
            if last_blockers.as_ref() != Some(&blockers) {
                info!(
                    target: "scrollery::pipeline::ai",
                    blockers = %blockers.join(","),
                    "AI producer yielding to higher priority task"
                );
                last_blockers = Some(blockers.clone());
            } else {
                debug!(
                    target: "scrollery::pipeline::ai",
                    blockers = %blockers.join(","),
                    "AI producer yielding to higher priority task"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        } else if last_blockers.take().is_some() {
            info!(
                target: "scrollery::pipeline::ai",
                "AI producer: yield blockers cleared, resuming | 让步解除，继续生产"
            );
        }

        // 读连接圈进块作用域:只为取批查询,查完立即归还池(2026-07-11 加固批 B-1)。
        // 旧代码持读连接跨越下方 bounded send 循环——消费端慢/让步时 send 长阻塞,
        // 读连接被钉住,叠加 derive 占用即抽干读池,状态轮询全部 Pool(Error) 饿死。
        let batch = {
            let conn = match state.db_read_pool.get() {
                Ok(c) => c,
                Err(e) => {
                    warn!("DB pool error in AI producer | AI 生产者 DB 池错误: {}", e);
                    break;
                }
            };
            match get_pending_ai_items(&conn, BATCH_SIZE) {
                Ok(b) => b,
                Err(e) => {
                    warn!(
                        "Query pending AI items failed | 查询待处理 AI 项失败: {}",
                        e
                    );
                    break;
                }
            }
        };

        if batch.is_empty() {
            info!("AI producer: no more pending items | AI 生产者：没有更多待处理项");
            break;
        }

        // 将项标记为"处理中"，避免重启时重新排队
        // 从中毒锁恢复而非 panic —— 此处 panic 会永久毒化共享写连接，并级联成别处的卡死/失败（问题6）。
        let write_conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<i64> = batch.iter().map(|it| it.id).collect();
        if let Err(e) = batch_update_ai_status(&write_conn, &ids, AiStatus::Processing.as_i64()) {
            warn!(
                "Failed to mark items as processing | 标记项为处理中失败: {}",
                e
            );
        }
        drop(write_conn);

        for item in batch {
            if token.is_cancelled() {
                break;
            }

            if task_tx
                .send(AiTask {
                    item_id: item.id,
                    source_path: PathBuf::from(item.abs_path),
                    file_format: item.file_format,
                    cache_key: item.cache_key,
                })
                .is_err()
            {
                break;
            }
        }
    }

    info!("AI producer finished | AI 生产者已完成");
}

/// 统一解析有效 batch(进程内推理与 worker 派发/SessionInit 快照共用,T17 提取):
/// 配置 `ai_batch_size`(0=按 VRAM 自动)→ 上限 256 防 OOM → 固定 batch 模型抬到 ≥k。
///
/// A2:`ai_batch_size` 是 schema 设置类键,唯一真源已切到 `ConfigManager`(内存 `RwLock`
/// 读,无需再取读池连接)。
pub(crate) fn resolve_batch_size(state: &AppState, profile: &ModelProfile) -> usize {
    resolve_batch_size_from(state.config.get("ai_batch_size"), profile)
}

/// A2 前称「连接已在手的变体」——`conn` 参数已随 `ai_batch_size` 迁往 config.toml 而不再需要,
/// 改收 `&ConfigManager`(与 `resolve_batch_size` 同源,消除原「先取池连接才能读配置」的间接层)。
pub(crate) fn resolve_batch_size_with(
    config: &crate::config::ConfigManager,
    profile: &ModelProfile,
) -> usize {
    resolve_batch_size_from(config.get("ai_batch_size"), profile)
}

fn resolve_batch_size_from(batch_size_str: Option<String>, profile: &ModelProfile) -> usize {
    let mut val = batch_size_str
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    if val == 0 {
        // 按显存自动探测
        let vram_bytes = crate::ai::provider::detect_vram_bytes();
        let gb = vram_bytes.map(|b| b / (1024 * 1024 * 1024)).unwrap_or(0);
        val = if gb >= 12 {
            256
        } else if gb >= 8 {
            128
        } else if gb >= 4 {
            64
        } else if gb >= 2 {
            32
        } else {
            16
        };
        tracing::info!(
            "AI Batch Size auto-configured to {} based on {}GB VRAM",
            val,
            gb
        );
    } else {
        // 硬上限，防止盲设过大导致 OOM
        if val > 256 {
            tracing::warn!(
                "User requested batch size {} exceeds safe limit, clamping to 256",
                val
            );
            val = 256;
        }
    }
    // 固定 batch 模型（图像塔 bN 导出）要求每次喂入 ≥ k 行才高效（不足 k 的块会被补齐浪费）；
    // 这里把有效 batch 抬到 ≥ k，与设置页的最小限制一致。动态 batch / 单批模型不受影响。
    if let Some(crate::ai::remote_registry::BatchKind::Fixed(k)) =
        crate::ai::remote_registry::parse_batch(&profile.image_file)
    {
        let k = k as usize;
        if k > 1 && val < k {
            tracing::info!(
                "Active model is fixed-batch k={}, raising batch size {} → {}",
                k,
                val,
                k
            );
            val = k;
        }
    }
    val
}

/// 写入器：批量收集结果并写入 DB。
pub(crate) fn write_results(
    state: &Arc<AppState>,
    result_rx: Receiver<AiResult>,
    token: &CancellationToken,
    profile: &ModelProfile,
) {
    let mut batch: Vec<(i64, String, Vec<u8>, i64, i64)> = Vec::with_capacity(BATCH_SIZE as usize);
    let mut total_written = 0u64;

    let mut failed_ids: Vec<(i64, i64)> = Vec::new();
    let mut last_flush = std::time::Instant::now();

    // 时间驱动 flush(2026-07-11 加固批 B-2):「满 512 或陈龄超 WRITE_FLUSH_INTERVAL,
    // 先到先落」。旧行为只按满批落库,慢速期(大图/让步/低端 GPU)进度在 DB 里最长冻结
    // 一整批——状态轮询读 DB,用户看到的就是 0% 长挂;时间上界同时封顶崩溃丢失窗口。
    // 刻意不改小批阈值:快速稳态仍满批走,写事务数不增。
    loop {
        match result_rx.recv_timeout(WRITE_FLUSH_INTERVAL) {
            Ok(result) => {
                if token.is_cancelled() {
                    info!("AI writer cancelled | AI 写入器已取消");
                    break;
                }

                match result.embedding {
                    Some(emb) => {
                        batch.push((result.item_id, profile.id.clone(), emb, 1, result.cache_key));
                    }
                    None => {
                        // 推理失败 — 收集起来批量更新状态
                        failed_ids.push((result.item_id, result.cache_key));
                    }
                }

                if batch.len() >= BATCH_SIZE as usize {
                    flush_batch(state, &mut batch, &mut total_written);
                    last_flush = std::time::Instant::now();
                }

                if failed_ids.len() >= BATCH_SIZE as usize {
                    flush_failed(state, &mut failed_ids);
                    last_flush = std::time::Instant::now();
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if token.is_cancelled() {
                    info!("AI writer cancelled | AI 写入器已取消");
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        if last_flush.elapsed() >= WRITE_FLUSH_INTERVAL
            && (!batch.is_empty() || !failed_ids.is_empty())
        {
            if !batch.is_empty() {
                flush_batch(state, &mut batch, &mut total_written);
            }
            if !failed_ids.is_empty() {
                flush_failed(state, &mut failed_ids);
            }
            last_flush = std::time::Instant::now();
        }
    }

    // 刷新剩余项
    if !batch.is_empty() {
        flush_batch(state, &mut batch, &mut total_written);
    }
    if !failed_ids.is_empty() {
        flush_failed(state, &mut failed_ids);
    }

    info!(
        "AI writer finished, total embeddings written: {} | AI 写入器完成，总共写入嵌入向量: {}",
        total_written, total_written
    );
}

/// 将一批嵌入向量刷新到数据库(X1:向量+Done 的条件写在 batch_finish_ai_items 内单事务完成,
/// cache_key 已换的失效项整体跳过——迟到写不再能把旧内容向量+status=2 永久留库)。
fn flush_batch(
    state: &Arc<AppState>,
    batch: &mut Vec<(i64, String, Vec<u8>, i64, i64)>,
    total_written: &mut u64,
) {
    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());

    match batch_finish_ai_items(&conn, batch) {
        Ok(done) => {
            *total_written += done as u64;
            if done < batch.len() {
                info!(
                    "AI flush:{} 项中 {} 项因源变更失效被跳过(下轮按新内容重分析)",
                    batch.len(),
                    batch.len() - done
                );
            }
            debug!(
                target: "scrollery::pipeline::ai",
                count = done,
                "Flushed embeddings to DB"
            );
            // 新嵌入向量已写入 —— 使常驻缓存失效，下次搜索将重新加载。
            drop(conn);
            state.invalidate_embedding_cache();
        }
        Err(e) => {
            warn!("Batch embedding write failed | 批量嵌入向量写入失败: {}", e);
            // 将失败的项标记为错误，避免无限重新处理(同样条件写:失效项不标)
            let guarded: Vec<(i64, i64)> =
                batch.iter().map(|(id, _, _, _, key)| (*id, *key)).collect();
            let _ = batch_update_ai_status_guarded(&conn, &guarded, AiStatus::Error.as_i64());
        }
    }

    batch.clear();
}

/// 将一批失败的项在数据库中标记为 `ai_status=3`（错误;X1 条件写,失效项不标)。
fn flush_failed(state: &Arc<AppState>, failed_ids: &mut Vec<(i64, i64)>) {
    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    match batch_update_ai_status_guarded(&conn, failed_ids, AiStatus::Error.as_i64()) {
        Err(e) => warn!("Failed to mark items as error | 标记项为错误失败: {}", e),
        Ok(n) => debug!(
            "Marked {} items as ai_status=Error | 已将 {} 个项标记为 ai_status=Error",
            n, n
        ),
    }
    failed_ids.clear();
}
