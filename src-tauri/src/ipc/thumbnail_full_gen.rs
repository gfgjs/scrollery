//! 全库缩略图生成流水线（从 `thumbnail_commands.rs` 拆出，见超长文件拆分方案 tierB-3）:
//! 与 `batch_request_thumbnails` 服务的「批量视口路径」几乎独立的第二条流水线,
//! 服务全库生成 + Phase2 CPU 兜底,两者仅共享 generation-aware flush helper（留在
//! `thumbnail_commands.rs`, `pub(super)` 可见）。

use std::sync::Arc;

use crossbeam_channel::bounded;
use rayon::prelude::*;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::db::models::ThumbResult;
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::thumbnail::generator::snap_to_tier;
use crate::thumbnail::{decode_media_step, process_deferred_cpu, DecodeResult};

use crate::thumbnail::qos::{refresh_worker_qos, thumb_cpu_budget};

use super::thumbnail_commands::flush_thumb_results_for_generation;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullThumbProgressPayload {
    pub generated: u64,
    pub total: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// 进度传输事件名。原实现用 Tauri Channel——Channel 生命周期绑定发起 invoke 的 webview,
/// 页面刷新后永久失联而后台任务照跑,进度 UI 就此丢失。改 app 级事件 + AppState 快照:
/// 事件广播给当前(含重建后的)webview,快照供重载瞬间 `full_thumb_gen_status` 查询回填。
pub const THUMB_GEN_PROGRESS_EVENT: &str = "thumb:gen_progress";

/// 更新快照并广播进度事件(快照先行,保证事件消费者查询到的状态不落后于事件)。
///
/// generation、取消状态和数据库 epoch 在同一短临界区内检查；旧轮不能在新轮启动后
/// 覆盖全局快照，取消后的普通 running 进度也不会重新复活前端状态。
fn publish_thumb_progress(
    app: &AppHandle,
    state: &AppState,
    epoch: u64,
    generation: u64,
    cancel_token: &CancellationToken,
    payload: FullThumbProgressPayload,
) -> bool {
    state.with_thumb_generation_gate(|| {
        if !state.thumb_gen_token.is_generation_current(generation)
            || (payload.status == "running" && cancel_token.is_cancelled())
        {
            return false;
        }
        state
            .with_database_lifecycle_read(epoch, || {
                *state
                    .thumb_gen_progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(payload.clone());
                let _ = app.emit(THUMB_GEN_PROGRESS_EVENT, payload);
            })
            .is_some()
    })
}

/// 在同一缩略图代次临界区内发布终态、失效布局缓存并 compare-and-clear。
///
/// 终态发布必须先于清槽，且不能在旧轮释放闸门后再清新轮缓存；清库 epoch 失效时只
/// 清掉自己的 token，不写入新数据库快照。
fn finish_thumb_generation(
    app: &AppHandle,
    state: &AppState,
    epoch: u64,
    generation: u64,
    payload: FullThumbProgressPayload,
) -> bool {
    state.with_thumb_generation_gate(|| {
        if !state.thumb_gen_token.is_generation_current(generation) {
            return false;
        }
        let lifecycle_current = state
            .with_database_lifecycle_read(epoch, || {
                *state
                    .thumb_gen_progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(payload.clone());
                let _ = app.emit(THUMB_GEN_PROGRESS_EVENT, payload);
                *state
                    .layout_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                crate::layout::items_cache::invalidate(&state.layout_items_cache);
            })
            .is_some();
        let finished = state.thumb_gen_token.finish(generation);
        finished && lifecycle_current
    })
}

/// 缩略图生成状态查询(webview 重载恢复用):最近进度快照 + 运行态真相(token 存在)。
/// 快照说 "running" 但 token 已不在(停止后收尾窗口/异常终止)→ 报 "cancelled",
/// 避免前端恢复出一个永不结束的假运行态。
#[tauri::command]
pub fn full_thumb_gen_status(state: State<'_, Arc<AppState>>) -> Result<FullThumbProgressPayload> {
    Ok(state.with_thumb_generation_gate(|| {
        let is_running = state.thumb_gen_token.is_running();
        let is_cancelled = state.thumb_gen_token.current_is_cancelled();
        let snapshot = state
            .thumb_gen_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut payload = snapshot.unwrap_or(FullThumbProgressPayload {
            generated: 0,
            total: 0,
            status: "idle".to_string(),
            current_item: None,
            phase: None,
        });
        if (!is_running || is_cancelled) && payload.status == "running" {
            payload.status = "cancelled".to_string();
        }
        payload
    }))
}

#[tauri::command]
pub async fn start_full_thumbnail_generation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    run_thumbnail_generation(app, state, true).await
}

/// 增量生成:只处理 thumb_status=0 的项(从未生成,或经 mtime 变更/LRU 驱逐/启动 stat
/// 兜底被复位),不做全表重置。与「全量」共用同一条多阶段流水线,唯一分叉是前置 reset。
#[tauri::command]
pub async fn start_incremental_thumbnail_generation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    run_thumbnail_generation(app, state, false).await
}

async fn run_thumbnail_generation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    reset_all: bool,
) -> Result<()> {
    let Some(start_epoch) = state.current_database_epoch() else {
        return Ok(());
    };

    // 全量 reset 必须先撤销并换代，再清空数据库中的缩略图状态。否则旧 worker 会在
    // reset 提交后、新 generation 安装前通过旧代次检查，把刚复位的行重新写成完成。
    // thumb gate → database lifecycle write 的锁序与 clear_all_thumbnails 相同；reset
    // 闭包结束前一直持有 thumb gate，因此旧 flush 无法穿过这个线性化点。
    let reset_generation: Option<(u64, CancellationToken, u64)> = if reset_all {
        let reset_state = Arc::clone(&*state);
        let reset = tokio::task::spawn_blocking(move || {
            reset_state.with_thumb_generation_gate(|| -> Result<_> {
                reset_state.thumb_gen_token.cancel();
                reset_state.with_scan_lifecycle_exclusive(|| -> Result<()> {
                    let conn = reset_state
                        .db_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    // 复位面限 media_type='image'(2026-07-10 审查 B6)：video/epub 封面由派生
                    // 流水线重建，pdf/svg 由前端渲染队列重建，不能交给主 generator 标灰卡。
                    conn.execute("UPDATE media_items SET thumb_status = 0, thumb_path = NULL, thumbhash = NULL WHERE is_deleted = 0 AND media_type = 'image'", [])
                        .map_err(AppError::Db)?;
                    // 同步失效 exotic thumbnail 任务（问题1）：否则 done PSD 既不被重领、又被放回主 generator。
                    crate::db::queries::reset_all_exotic_thumbnail_tasks(&conn)?;
                    Ok(())
                })?;
                let epoch = reset_state.current_database_epoch().ok_or_else(|| {
                    AppError::System("数据库生命周期已停止 | database lifecycle is inactive".into())
                })?;
                let (generation, cancel_token) = reset_state.thumb_gen_token.begin();
                Ok((generation, cancel_token, epoch))
            })
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
        // 让 Coordinator 重领被退回 pending 的 exotic 任务（重做覆盖同路径产物）。
        state.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
        Some(reset)
    } else {
        None
    };

    // R1-3：计数（读池）走 read_blocking。
    let total = super::blocking::read_blocking(&state, |conn| {
        crate::db::queries::count_pending_thumb_items(conn)
    })
    .await?;

    let (generation, cancel_token, epoch) = match reset_generation {
        Some(generation) => generation,
        None => {
            let Some(generation) = state.try_new_thumb_gen_token() else {
                return Ok(());
            };
            generation
        }
    };

    // 增量入口保留原有的「调用期间生命周期已换代则不启动」语义；全量入口的 epoch
    // 必然因自身 reset 变化，因此只检查它在计数结束后仍然有效。
    let epoch_invalidated = if reset_all {
        !state.is_database_epoch_current(epoch)
    } else {
        epoch != start_epoch
    };
    if epoch_invalidated {
        cancel_token.cancel();
        state.with_thumb_generation_gate(|| {
            let _ = state.thumb_gen_token.finish(generation);
        });
        return Ok(());
    }
    if total == 0 {
        let _ = finish_thumb_generation(
            &app,
            &state,
            epoch,
            generation,
            FullThumbProgressPayload {
                generated: 0,
                total: 0,
                status: "completed".to_string(),
                current_item: None,
                phase: None,
            },
        );
        return Ok(());
    }

    let state_arc = Arc::clone(&*state);
    let generated_count = Arc::new(std::sync::atomic::AtomicU64::new(0));

    tokio::task::spawn_blocking(move || -> Result<()> {
        let start_time = std::time::Instant::now();
        publish_thumb_progress(
            &app,
            &state_arc,
            epoch,
            generation,
            &cancel_token,
            FullThumbProgressPayload {
                generated: 0,
                total: total as u64,
                status: "running".to_string(),
                current_item: None,
                phase: Some("GPU".to_string()),
            },
        );

        let mut config = state_arc
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        // 同 batch_request_thumbnails:直调 decode/encode_media_step,须先把可能的非档位尺寸
        // (如历史默认 480)吸附到有效档位,否则 thumb_path 断言失败(panic during encode_media_step)。
        config.size = snap_to_tier(config.size);
        info!(
            "[FullThumbGen] START: total={} strategy={} gpu_engine={} size={} skip_max_bytes={} cache_dir={:?} | 全量缩略图生成开始",
            total, config.strategy, config.gpu_engine, config.size, config.skip_max_bytes, config.cache_dir
        );

        // 进度 IPC 节流:逐张发送在大库下是 IPC 风暴(8 万+ 条)并挤掉侧栏 rAF。
        // 最终 completed/cancelled 恒发送,进度条仍走到 100%。
        const PROGRESS_THROTTLE: std::time::Duration = std::time::Duration::from_millis(100);

        {
            // 多阶段流水线(生成引擎终局,2026-07-10 A/B 实测裁决后唯一实现,见文件头注)。
            let (decode_tx, decode_rx) = bounded(1024);
            let (encode_tx, encode_rx) = bounded(1024);
            let (result_tx, result_rx) = bounded::<
                std::result::Result<
                    ThumbResult,
                    (crate::db::models::MediaItem, std::path::PathBuf),
                >,
            >(1024);

            let state_dispatcher = state_arc.clone();
            let cancel_dispatcher = cancel_token.clone();
            std::thread::spawn(move || {
                let all_ids = {
                    let pool = match state_dispatcher.db_read_pool.get() {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    crate::db::queries::get_all_pending_thumb_ids(&pool).unwrap_or_default()
                };
                info!("[FullThumbGen] Dispatcher: {} pending IDs fetched | 调度器: 获取到 {} 个待处理 ID", all_ids.len(), all_ids.len());

                for chunk in all_ids.chunks(50) {
                    if cancel_dispatcher.is_cancelled()
                        || !state_dispatcher.is_database_epoch_current(epoch)
                    {
                        break;
                    }
                    let pool = match state_dispatcher.db_read_pool.get() {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    for &id in chunk {
                        if cancel_dispatcher.is_cancelled()
                            || !state_dispatcher.is_database_epoch_current(epoch)
                        {
                            break;
                        }
                        if let Ok(item) = crate::db::queries::get_media_item(&pool, id) {
                            if let Ok((root_path, rel_path, file_name)) =
                                crate::db::queries::get_item_path_info(&pool, id)
                            {
                                let abs_path_str = crate::utils::path::resolve_media_path(
                                    &root_path, &rel_path, &file_name,
                                );
                                let abs_path = std::path::PathBuf::from(abs_path_str);
                                if decode_tx.send((item, abs_path)).is_err() {
                                    return;
                                }
                            } else {
                                error!("[FullThumbGen] path_info failed for id={}", id);
                            }
                        } else {
                            error!("[FullThumbGen] get_media_item failed for id={}", id);
                        }
                    }
                }
                info!("[FullThumbGen] Dispatcher: done sending items | 调度器: 发送完毕");
            });

            let config_decode = config.clone();
            let cancel_decode = cancel_token.clone();
            // CPU 预算限流(2026-07-13,同批量路径):解码/编码池按 thumb_cpu_budget 上限,
            // 让出 reserve 核给 UI;Phase 2 deferred(GPU 失败回退,罕见)仍走全局 rayon 池。
            let budget = thumb_cpu_budget();
            let decode_threads = budget;
            for _ in 0..decode_threads {
                let rx = decode_rx.clone();
                let tx = encode_tx.clone();
                let res_tx = result_tx.clone();
                let state_worker = state_arc.clone();
                let cfg = config_decode.clone();
                let cancel = cancel_decode.clone();
                std::thread::spawn(move || {
                    let mut qos_state: Option<bool> = None;
                    while let Ok((item, abs_path)) = rx.recv() {
                        refresh_worker_qos(&mut qos_state);
                        if cancel.is_cancelled() {
                            break;
                        }
                        let decoded = match state_worker.with_database_lifecycle_read(epoch, || {
                            decode_media_step(&item, &abs_path, &state_worker.engine_arena, &cfg)
                        }) {
                            Some(result) => result,
                            None => break,
                        };
                        match decoded {
                            Ok(DecodeResult::Ready(res)) => {
                                let _ = res_tx.send(Ok(res));
                            }
                            Ok(DecodeResult::ToEncode {
                                item_id,
                                source_revision,
                                cache_key,
                                decoded,
                            }) => {
                                if tx
                                    .send((item_id, source_revision, cache_key, decoded))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Ok(DecodeResult::DeferredToCpu { item, abs_path }) => {
                                let _ = res_tx.send(Err((item, abs_path)));
                            }
                            Err(e) => {
                                error!("Full gen decode failed for id={}: {}", item.id, e);
                                let _ = res_tx.send(Ok(ThumbResult {
                                    item_id: item.id,
                                    thumb_status: 2,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision: item.source_revision,
                                    cache_key: item.cache_key,
                                }));
                            }
                        }
                    }
                });
            }
            drop(encode_tx);

            let config_encode = config.clone();
            let cancel_encode = cancel_token.clone();
            for _ in 0..budget {
                let rx = encode_rx.clone();
                let tx = result_tx.clone();
                let state_worker = state_arc.clone();
                let cfg = config_encode.clone();
                let cancel = cancel_encode.clone();
                std::thread::spawn(move || {
                    let mut qos_state: Option<bool> = None;
                    while let Ok((item_id, source_revision, cache_key, decoded)) = rx.recv() {
                        refresh_worker_qos(&mut qos_state);
                        if cancel.is_cancelled() {
                            break;
                        }
                        let encoded = match state_worker.with_database_lifecycle_read(epoch, || {
                            crate::thumbnail::encode_media_step_with_snapshot(
                                item_id,
                                source_revision,
                                cache_key,
                                decoded,
                                &cfg,
                            )
                        }) {
                            Some(result) => result,
                            None => break,
                        };
                        match encoded {
                            Ok(res) => {
                                let _ = tx.send(Ok(res));
                            }
                            Err(e) => {
                                error!("Full gen encode failed for id={}: {}", item_id, e);
                                let _ = tx.send(Ok(ThumbResult {
                                    item_id,
                                    thumb_status: 2,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision,
                                    cache_key,
                                }));
                            }
                        }
                    }
                });
            }
            drop(result_tx);

            let mut successful_results = Vec::new();
            let mut deferred_items = Vec::new();

            // 进度节流时钟(常量已提升到两方案共用):收集器单线程,裸 Instant 即可。
            let mut last_progress_emit = std::time::Instant::now();

            while let Ok(msg) = result_rx.recv() {
                if cancel_token.is_cancelled() || !state_arc.is_database_epoch_current(epoch) {
                    break;
                }

                match msg {
                    Ok(res) => {
                        successful_results.push(res.clone());
                        generated_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        let now = std::time::Instant::now();
                        if now.duration_since(last_progress_emit) >= PROGRESS_THROTTLE {
                            last_progress_emit = now;
                            let current =
                                generated_count.load(std::sync::atomic::Ordering::Relaxed);
                            publish_thumb_progress(
                                &app,
                                &state_arc,
                                epoch,
                                generation,
                                &cancel_token,
                                FullThumbProgressPayload {
                                    generated: current,
                                    total: total as u64,
                                    status: "running".to_string(),
                                    current_item: None,
                                    phase: Some("GPU".to_string()),
                                },
                            );
                        }
                    }
                    Err(deferred) => {
                        deferred_items.push(deferred);
                    }
                }

                if successful_results.len() >= 50 {
                    // 记录批次状态分布
                    let n_encoded = successful_results
                        .iter()
                        .filter(|r| r.thumb_status == 1)
                        .count();
                    let n_direct = successful_results
                        .iter()
                        .filter(|r| r.thumb_status == 3)
                        .count();
                    let n_failed = successful_results
                        .iter()
                        .filter(|r| r.thumb_status == 2)
                        .count();
                    info!(
                        "[FullThumbGen] Batch flush: {} results (encoded={}, direct={}, failed={}) | 批次写入",
                        successful_results.len(), n_encoded, n_direct, n_failed
                    );

                    if !flush_thumb_results_for_generation(
                        &state_arc,
                        epoch,
                        generation,
                        &cancel_token,
                        &successful_results,
                    ) {
                        cancel_token.cancel();
                        break;
                    }
                    successful_results.clear();
                }
            }

            // 落盘剩余结果
            if !successful_results.is_empty()
                && !flush_thumb_results_for_generation(
                    &state_arc,
                    epoch,
                    generation,
                    &cancel_token,
                    &successful_results,
                )
                && !cancel_token.is_cancelled()
            {
                cancel_token.cancel();
            }

            // 并行化(2026-07-10 修复 F3):原实现单线程逐个消化——批量路径(T12)为
            // deferred 建了 cores/2 专用池,唯独全库路径此处单核爬行;strategy=gpu 且
            // GPU 回退较多时 Phase 2 成瓶颈。分块 par_iter:块尺寸取 max(cores, 10)
            // 喂饱 rayon 池,每块一次事务批写(近似原 10-flush 节奏),取消粒度=块。
            if !deferred_items.is_empty() && !cancel_token.is_cancelled() {
                info!("[FullThumbGen] Phase 2: Processing {} deferred CPU tasks | 阶段2：处理延迟的 CPU 任务", deferred_items.len());
                let phase2_chunk = budget.max(10);
                // Phase 2 也受亲和性约束:GPU 回退较多时它会吃满 CPU,若走全局 rayon 池会摊到所有
                // 核、抵消预留核。局部池 num_threads=budget + start_handler 逐线程钉核;建池失败(极少)
                // 退回全局池,行为不变。
                let phase2_pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(budget)
                    .start_handler(|_| crate::thumbnail::qos::apply_current_thread_qos())
                    .build()
                    .ok();
                for chunk in deferred_items.chunks(phase2_chunk) {
                    if cancel_token.is_cancelled() || !state_arc.is_database_epoch_current(epoch) {
                        break;
                    }

                    let run = || {
                        chunk
                            .par_iter()
                            .filter_map(|(item, abs_path)| {
                                let result =
                                    state_arc.with_database_lifecycle_read(epoch, || {
                                        process_deferred_cpu(
                                            item,
                                            abs_path,
                                            &state_arc.engine_arena,
                                            &config,
                                        )
                                    })?;
                                Some(match result {
                                    Ok(r) => r,
                                    Err(e) => {
                                        error!(
                                            "Full gen CPU fallback failed for id={}: {}",
                                            item.id, e
                                        );
                                        ThumbResult {
                                            item_id: item.id,
                                            thumb_status: 2,
                                            thumb_path: None,
                                            thumbhash: None,
                                            source_revision: item.source_revision,
                                            cache_key: item.cache_key,
                                        }
                                    }
                                })
                            })
                            .collect::<Vec<ThumbResult>>()
                    };
                    // 有局部池走池,否则退回全局池(直接调用)。
                    let chunk_results: Vec<ThumbResult> = match &phase2_pool {
                        Some(pool) => pool.install(run),
                        None => run(),
                    };

                    generated_count.fetch_add(
                        chunk_results.len() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    if !flush_thumb_results_for_generation(
                        &state_arc,
                        epoch,
                        generation,
                        &cancel_token,
                        &chunk_results,
                    ) {
                        cancel_token.cancel();
                        break;
                    }

                    let now = std::time::Instant::now();
                    if now.duration_since(last_progress_emit) >= PROGRESS_THROTTLE {
                        last_progress_emit = now;
                        let current = generated_count.load(std::sync::atomic::Ordering::Relaxed);
                        publish_thumb_progress(
                            &app,
                            &state_arc,
                            epoch,
                            generation,
                            &cancel_token,
                            FullThumbProgressPayload {
                                generated: current,
                                total: total as u64,
                                status: "running".to_string(),
                                current_item: None,
                                phase: Some("CPU".to_string()),
                            },
                        );
                    }
                }
            }
        }

        let final_gen = generated_count.load(std::sync::atomic::Ordering::Relaxed);
        let lifecycle_current = state_arc.is_database_epoch_current(epoch);
        if !lifecycle_current {
            // clear_database 不依赖 thumb token；让本轮尽快收尾，但不要触碰新轮 token。
            cancel_token.cancel();
        }
        info!(
            "[FullThumbGen] FINISHED: generated={} total={} cancelled={} elapsed={}ms | 全量缩略图生成完成",
            final_gen, total, cancel_token.is_cancelled(), start_time.elapsed().as_millis()
        );
        let final_status = if cancel_token.is_cancelled() {
            "cancelled"
        } else {
            "completed"
        };
        // Compare-and-clear:仅当槽内仍是本轮才在同一短临界区发布终态并清 token。
        // 旧轮迟到收尾不能覆盖新轮快照，也不能在新轮启动后清其布局缓存。
        let finished_current = finish_thumb_generation(
            &app,
            &state_arc,
            epoch,
            generation,
            FullThumbProgressPayload {
                generated: final_gen,
                total: total as u64,
                status: final_status.to_string(),
                current_item: None,
                phase: None,
            },
        );
        if finished_current {
            tracing::info!(
                "Thumbnail gen token cleared after completion | 全量缩略图 token 已清除"
            );
        } else {
            tracing::info!(
                "Thumbnail gen superseded or database lifecycle changed; skip final publish | 本轮已被新轮取代或数据库生命周期已变化,跳过终态发布"
            );
        }

        tracing::info!(
            "Layout cache invalidated after full thumb gen | 全量缩略图后已清空布局缓存"
        );

        Ok(())
    });

    Ok(())
}

#[tauri::command]
pub fn stop_full_thumbnail_generation(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("User action: Stopping full thumbnail generation | 用户操作：停止全量缩略图生成");
    state.cancel_thumb_gen();
    Ok(())
}
