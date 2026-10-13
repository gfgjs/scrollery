//! 全库与增量图片生成：候选读取、共享服务订阅、批量条件发布和进度。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::bounded;
use crossbeam_channel::Select;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::thumbnail::benchmark::{PipelineMetrics, Publication, RunMetrics, WorkMetrics};
use crate::thumbnail::generator::snap_to_tier;
use crate::thumbnail::progress::{PublishedSummary, ThumbExecutionCount, ThumbResultCounts};
use crate::thumbnail::service::{ImageCompletion, ImageService, ImageSubscription, Priority};

use super::thumbnail_commands::flush_thumb_results_for_generation;

fn flush_measured_results(
    state: &AppState,
    epoch: u64,
    generation: u64,
    cancel_token: &CancellationToken,
    subscriptions: &[ImageSubscription],
    measurements: &[WorkMetrics],
    benchmark: Option<(&mut RunMetrics, Instant)>,
) -> Option<Vec<Publication>> {
    let started = benchmark.as_ref().map(|_| Instant::now());
    let published =
        flush_thumb_results_for_generation(state, epoch, generation, cancel_token, subscriptions);
    if let Some((metrics, invocation_started)) = benchmark {
        metrics.db_us += started.map_or(0, |t| t.elapsed().as_micros() as u64);
        metrics.record_batch(
            measurements,
            published.as_deref(),
            invocation_started.elapsed().as_micros() as u64,
        );
    }
    published
}

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
    #[serde(default)]
    pub results: ThumbResultCounts,
    #[serde(default)]
    pub executions: Vec<ThumbExecutionCount>,
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
                state.clear_layout_caches();
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
            results: ThumbResultCounts::default(),
            executions: Vec::new(),
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
    let invocation_started = Instant::now();
    let benchmark_enabled = crate::thumbnail::benchmark::BENCHMARK_ENABLED;
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
                    conn.execute("DELETE FROM image_artifacts", [])?;
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

    let mut config = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    config.size = snap_to_tier(config.size);
    let database_path = state.app_data_dir.join("scrollery.db");
    let display_fingerprint = config.display_fingerprint();
    let ai_fingerprint = config.ai_hq_cache.then(|| config.ai_fingerprint());
    let pipeline_metrics = Arc::new(PipelineMetrics::default());
    let discovery_started = benchmark_enabled.then(Instant::now);
    let snapshot = match tokio::task::spawn_blocking(move || {
        crate::db::queries::ImageCandidateSnapshot::open(
            &database_path,
            &display_fingerprint,
            ai_fingerprint.as_deref(),
        )
    })
    .await
    .map_err(|error| AppError::internal("候选快照失败 | candidate snapshot failed", error))
    .and_then(|result| result)
    {
        Ok(snapshot) => snapshot,
        Err(error) => {
            // 全量 reset 已安装代次；发现失败也必须收尾，且不能清掉随后启动的新轮。
            if let Some((generation, cancel_token, epoch)) = &reset_generation {
                cancel_token.cancel();
                finish_thumb_generation(
                    &app,
                    &state,
                    *epoch,
                    *generation,
                    FullThumbProgressPayload {
                        generated: 0,
                        total: 0,
                        status: "error".to_owned(),
                        current_item: None,
                        phase: None,
                        results: ThumbResultCounts::default(),
                        executions: Vec::new(),
                    },
                );
            }
            return Err(error);
        }
    };
    if let Some(started) = discovery_started {
        pipeline_metrics
            .discovery_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    let total = snapshot.total();
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
                results: ThumbResultCounts::default(),
                executions: Vec::new(),
            },
        );
        return Ok(());
    }

    let state_arc = Arc::clone(&*state);
    let generated_count = Arc::new(std::sync::atomic::AtomicU64::new(0));

    tokio::task::spawn_blocking(move || -> Result<()> {
        let start_time = std::time::Instant::now();
        let initialization_us = invocation_started.elapsed().as_micros() as u64;

        let mut run_metrics = RunMetrics::default();
        let mut published_summary = PublishedSummary::default();
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
                phase: Some("image".to_string()),
                results: ThumbResultCounts::default(),
                executions: Vec::new(),
            },
        );

        info!(
            "[FullThumbGen] START: total={} strategy={} gpu_engine={} size={} skip_max_bytes={} cache_dir={:?} | 全量缩略图生成开始",
            total, config.strategy, config.gpu_engine, config.size, config.skip_max_bytes, config.cache_dir
        );

        // 进度 IPC 节流:逐张发送在大库下是 IPC 风暴(8 万+ 条)并挤掉侧栏 rAF。
        // 最终 completed/cancelled 恒发送,进度条仍走到 100%。
        const PROGRESS_THROTTLE: std::time::Duration = std::time::Duration::from_millis(100);

        let service = ImageService::for_state(&state_arc);
        // 每轮仅保留一个候选读取者；所有图片计算共用常驻服务。
        let (subscription_tx, subscription_rx) = bounded::<ImageSubscription>(1);
        let dispatch_state = Arc::clone(&state_arc);
        let dispatch_service = Arc::clone(&service);
        let dispatch_cancel = cancel_token.clone();
        let dispatch_config = config.clone();
        let dispatch_metrics = Arc::clone(&pipeline_metrics);
        let dispatch_terminated = Arc::clone(&generated_count);
        let dispatcher = std::thread::spawn(move || {
            let mut snapshot = snapshot;
            loop {
                if dispatch_cancel.is_cancelled()
                    || !dispatch_state.is_database_epoch_current(epoch)
                {
                    break;
                }
                let page_started = benchmark_enabled.then(Instant::now);
                let page = match snapshot.next_page(256) {
                    Ok(page) if page.is_empty() => break,
                    Ok(page) => page,
                    Err(error) => {
                        error!(%error, "Image candidate page failed");
                        dispatch_metrics
                            .dispatch_errors
                            .fetch_add(1, Ordering::Relaxed);
                        dispatch_cancel.cancel();
                        break;
                    }
                };
                if let Some(started) = page_started {
                    dispatch_metrics
                        .candidate_read_us
                        .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                }
                dispatch_metrics
                    .candidates
                    .fetch_add(page.len() as u64, Ordering::Relaxed);
                let stale = page
                    .iter()
                    .filter(|candidate| candidate.source.is_none())
                    .count();
                dispatch_terminated.fetch_add(stale as u64, Ordering::Relaxed);
                let sources = page
                    .into_iter()
                    .filter_map(|candidate| candidate.source)
                    .collect();
                let requests = match crate::thumbnail::intake::prepare_sources(
                    &dispatch_state,
                    sources,
                    &dispatch_config,
                    epoch,
                    Priority::Background,
                    &dispatch_cancel,
                    benchmark_enabled,
                ) {
                    Ok(requests) => requests,
                    Err(error) => {
                        error!(%error, "Image candidate preparation failed");
                        dispatch_metrics
                            .dispatch_errors
                            .fetch_add(1, Ordering::Relaxed);
                        dispatch_cancel.cancel();
                        break;
                    }
                };
                if !dispatch_service.submit_background_page(requests, |subscription| {
                    if dispatch_cancel.is_cancelled()
                        || !dispatch_state.is_database_epoch_current(epoch)
                    {
                        return false;
                    }
                    subscription_tx.send(subscription).is_ok()
                }) {
                    return;
                }
            }
        });
        enum Collected {
            Subscription(Option<ImageSubscription>),
            Completion(usize, Option<ImageCompletion>),
            Timeout,
        }
        let mut pending: Vec<ImageSubscription> = Vec::new();
        let mut completed_subscriptions = Vec::new();
        let mut results = Vec::new();
        let mut measurements = Vec::new();
        let mut dispatch_finished = false;
        let mut unavailable = 0u64;
        let mut last_publish = Instant::now();
        let mut last_progress_emit = Instant::now();
        loop {
            if cancel_token.is_cancelled() || !state_arc.is_database_epoch_current(epoch) {
                break;
            }
            if dispatch_finished && pending.is_empty() {
                break;
            }
            // 按完成顺序收集；一个慢图不会挡住其它已完成图片的提交。
            let event = {
                let mut selection = Select::new();
                let accepting = !dispatch_finished && pending.len() < 256;
                if accepting {
                    selection.recv(&subscription_rx);
                }
                for subscription in &pending {
                    selection.recv(subscription.receiver());
                }
                let collected = match selection.select_timeout(PROGRESS_THROTTLE) {
                    Ok(operation) => {
                        let index = operation.index();
                        if accepting && index == 0 {
                            Collected::Subscription(operation.recv(&subscription_rx).ok())
                        } else {
                            let index = index - usize::from(accepting);
                            Collected::Completion(
                                index,
                                operation.recv(pending[index].receiver()).ok(),
                            )
                        }
                    }
                    Err(_) => Collected::Timeout,
                };
                collected
            };
            match event {
                Collected::Subscription(Some(subscription)) => pending.push(subscription),
                Collected::Subscription(None) => dispatch_finished = true,
                Collected::Completion(index, completion) => {
                    let subscription = pending.swap_remove(index);
                    if let Some(ImageCompletion {
                        result: Some(result),
                        mut metrics,
                        ..
                    }) = completion
                    {
                        metrics.dequeue();
                        metrics.enqueue();
                        results.push(result);
                        measurements.push(metrics);
                        completed_subscriptions.push(subscription);
                    } else {
                        unavailable += 1;
                        generated_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Collected::Timeout => {}
            }
            if !results.is_empty()
                && (results.len() >= 50 || last_publish.elapsed() >= PROGRESS_THROTTLE)
            {
                let published = flush_measured_results(
                    &state_arc,
                    epoch,
                    generation,
                    &cancel_token,
                    &completed_subscriptions,
                    &measurements,
                    benchmark_enabled.then_some((&mut run_metrics, invocation_started)),
                );
                if let Some(publications) = &published {
                    published_summary.record_batch(&results, &measurements, publications);
                    generated_count.fetch_add(results.len() as u64, Ordering::Relaxed);
                }
                results.clear();
                measurements.clear();
                completed_subscriptions.clear();
                last_publish = Instant::now();
                if published.is_none() {
                    cancel_token.cancel();
                    break;
                }
            }
            if last_progress_emit.elapsed() >= PROGRESS_THROTTLE {
                let processed = generated_count.load(Ordering::Relaxed);
                publish_thumb_progress(
                    &app,
                    &state_arc,
                    epoch,
                    generation,
                    &cancel_token,
                    FullThumbProgressPayload {
                        generated: processed,
                        total: total as u64,
                        status: "running".to_owned(),
                        current_item: None,
                        phase: Some("image".to_owned()),
                        results: published_summary.counts(processed),
                        executions: published_summary.executions.clone(),
                    },
                );
                last_progress_emit = Instant::now();
            }
        }
        if !results.is_empty() {
            if let Some(publications) = flush_measured_results(
                &state_arc,
                epoch,
                generation,
                &cancel_token,
                &completed_subscriptions,
                &measurements,
                benchmark_enabled.then_some((&mut run_metrics, invocation_started)),
            ) {
                published_summary.record_batch(&results, &measurements, &publications);
                generated_count.fetch_add(results.len() as u64, Ordering::Relaxed);
            } else {
                cancel_token.cancel();
            }
        }
        drop(completed_subscriptions);
        drop(pending);
        // submit 的队列准入只观察取消令牌；epoch 失效后须先取消，再 join，
        // 否则候选线程可能仍卡在准入，连同它持有的固定快照连接一起无法释放。
        if !state_arc.is_database_epoch_current(epoch) {
            cancel_token.cancel();
        }
        // 先关订阅通道解除候选读取者的 send 背压，再等待它退出。
        drop(subscription_rx);
        if dispatcher.join().is_err() {
            pipeline_metrics
                .dispatch_errors
                .fetch_add(1, Ordering::Relaxed);
            cancel_token.cancel();
        }
        let final_gen = generated_count.load(std::sync::atomic::Ordering::Relaxed);
        let lifecycle_current = state_arc.is_database_epoch_current(epoch);
        if !lifecycle_current {
            // clear_database 不依赖 thumb token；让本轮尽快收尾，但不要触碰新轮 token。
            cancel_token.cancel();
        }
        if benchmark_enabled {
            let elapsed_us = invocation_started.elapsed().as_micros() as u64;
            let summary = serde_json::json!({
                "schema": 2,
                "pid": std::process::id(),
                "generation": generation,
                "database_epoch": epoch,
                "executable": std::env::current_exe().ok(),
                "debug_assertions": cfg!(debug_assertions),
                "mode": if reset_all { "full" } else { "incremental" },
                "cancelled": cancel_token.is_cancelled(),
                "settings": {
                    "size": config.size, "quality": config.webp_quality,
                    "strategy": config.strategy, "gpu_engine": config.gpu_engine,
                    "skip_max_bytes": config.skip_max_bytes,
                    "ai_hq_cache": config.ai_hq_cache,
                    "ai_cache_short_edge": config.ai_cache_short_edge,
                    "cache_dir": config.cache_dir,
                },
                "counts": {
                    "initial_pending": total,
                    "candidates": pipeline_metrics.candidates.load(Ordering::Relaxed),
                    "dispatch_errors": pipeline_metrics.dispatch_errors.load(Ordering::Relaxed),
                    "observed": run_metrics.observed,
                    "cpu_fallbacks": run_metrics.stages.cpu_fallbacks,
                    "new_generated_published": run_metrics.generated,
                    "cache_hit_published": run_metrics.cache_hit,
                    "direct_published": run_metrics.direct,
                    "unsupported_published": run_metrics.unsupported,
                    "failed_published": run_metrics.failed,
                    "stale": run_metrics.stale,
                    "unpublished": run_metrics.unpublished,
                    "temporarily_unavailable": unavailable,
                    "unclassified": run_metrics.unclassified,
                    "unobserved": (total as u64).saturating_sub(run_metrics.observed),
                    "ai_written_before_publication": run_metrics.stages.ai_written,
                    "ai_cache_hit": run_metrics.stages.ai_cache_hit,
                    "ai_failed": run_metrics.stages.ai_failed,
                },
                "wall_us": {
                    "total": elapsed_us, "initialization": initialization_us,
                    "first_new_publish": run_metrics.first_publish_us,
                    "last_new_publish": run_metrics.last_publish_us,
                    "tail_after_last_new_publish": run_metrics.last_publish_us.map(|t| elapsed_us.saturating_sub(t)),
                },
                // 并发阶段累计值彼此重叠，不可相加当全程墙钟。
                "cumulative_us": {
                    "candidate_discovery": pipeline_metrics.discovery_us.load(Ordering::Relaxed),
                    "candidate_reads": pipeline_metrics.candidate_read_us.load(Ordering::Relaxed),
                    "queue_including_send_backpressure": run_metrics.queue_us,
                    "source_decode_resize": run_metrics.stages.decode_us,
                    "encode_hash_color_resize": run_metrics.stages.encode_us,
                    "embedded_decode_encode": run_metrics.stages.embedded_us,
                    "file_write": run_metrics.stages.write_us,
                    "db_publish_including_lock_and_layout": run_metrics.db_us,
                    "accepted_end_to_end": run_metrics.end_to_end_us,
                },
                "last_decode_backend_counts": run_metrics.backends,
                // 图像 GPU 子阶段包含于 decode；init_us 单独观测设备初始化，均不再累加到总时长。
                "gpu_experiment": run_metrics.gpu_experiment,
                "service_lifetime_sampled_queue_peak_lower_bounds": {
                    "decode": service.metrics.decode_queue_peak.load(Ordering::Relaxed),
                    "encode": service.metrics.encode_queue_peak.load(Ordering::Relaxed),
                },
                "service_lifetime_active_peak": {
                    "decode": service.metrics.decode.peak.load(Ordering::Relaxed),
                    "encode": service.metrics.encode.peak.load(Ordering::Relaxed),
                },
                "new_generated_per_second": (elapsed_us > 0).then(|| run_metrics.generated as f64 * 1_000_000.0 / elapsed_us as f64),
                "publication_span_per_second": run_metrics.publication_span_rate(),
                "service_threads_per_stage": service.width,
                "service_lifetime_memory_peak": {
                    "worker_working_reserved": service.metrics.working_bytes_peak.load(Ordering::Relaxed),
                    "worker_encoded_reserved": service.metrics.encoded_bytes_peak.load(Ordering::Relaxed),
                    "worker_private_sampled": service.metrics.worker_private_bytes_peak.load(Ordering::Relaxed),
                    "host_output_reserved": service.metrics.host_output_bytes_peak.load(Ordering::Relaxed),
                },
                "host_process_memory_sampling": "external_process_sampler",
            });
            info!("[FullThumbGen] BENCHMARK {}", summary);
        }
        info!(
            "[FullThumbGen] FINISHED: generated={} total={} cancelled={} elapsed={}ms | 全量缩略图生成完成",
            final_gen, total, cancel_token.is_cancelled(), start_time.elapsed().as_millis()
        );
        let final_status =
            if unavailable > 0 || pipeline_metrics.dispatch_errors.load(Ordering::Relaxed) > 0 {
                "error"
            } else if cancel_token.is_cancelled() {
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
                results: published_summary.counts(final_gen),
                executions: published_summary.executions,
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
