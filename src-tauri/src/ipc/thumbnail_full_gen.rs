//! 全库/增量图片与原生视频封面入口。成员快照、分级、领取和执行由统一任务机制管理。

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::db::queries::{self as q, ThumbnailLane, ThumbnailTaskKey, ThumbnailTaskKind};
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::thumbnail::coordinator::{unique_id, ExecutionFacts, ImageExecution, VideoCoverPhase};
use crate::thumbnail::generator::snap_to_tier;
use crate::thumbnail::native_protocol::NativeExecution;
use crate::thumbnail::scheduler::{
    advance_run_phase, classify_image_cost, OutputFingerprint, ThumbnailRunPhase,
};

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

/// 本轮已条件完成的结果；available = newly_generated + cache_hit，暂不可用不改媒体展示行。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResultCounts {
    pub available: u64,
    pub direct: u64,
    pub failed: u64,
    #[serde(default)]
    pub newly_generated: u64,
    #[serde(default)]
    pub cache_hit: u64,
    #[serde(default)]
    pub temporarily_unavailable: u64,
}

/// 本轮新发布产物的实际后端；共享执行向各订阅轮次分别归属，不累计历史缓存的硬件。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbExecutionCount {
    /// 宿主未提供后端事实时为 None；MF 普通路径仍保持加速程度未知。
    pub native: Option<NativeExecution>,
    pub count: u64,
}

#[derive(Default)]
struct PublishedSummary {
    results: ThumbResultCounts,
    executions: Vec<ThumbExecutionCount>,
}

impl PublishedSummary {
    fn record(&mut self, status: i64, execution: ExecutionFacts) {
        match status {
            3 => self.results.direct += 1,
            2 => self.results.failed += 1,
            1 => {
                self.results.available += 1;
                if execution.newly_generated {
                    self.results.newly_generated += 1;
                    if let Some(entry) = self
                        .executions
                        .iter_mut()
                        .find(|entry| entry.native == execution.native)
                    {
                        entry.count += 1;
                    } else {
                        self.executions.push(ThumbExecutionCount {
                            native: execution.native,
                            count: 1,
                        });
                    }
                } else {
                    self.results.cache_hit += 1;
                }
            }
            _ => {}
        }
    }

    fn processed(&self) -> u64 {
        self.results.available
            + self.results.direct
            + self.results.failed
            + self.results.temporarily_unavailable
    }
}

impl FullThumbProgressPayload {
    fn with_results(mut self, metrics: &ThumbPerfMetrics) -> Self {
        let published = metrics.published.lock().unwrap_or_else(|e| e.into_inner());
        self.generated = published.processed();
        self.results = published.results.clone();
        self.executions = published.executions.clone();
        self
    }
}

pub const THUMB_GEN_PROGRESS_EVENT: &str = "thumb:gen_progress";

#[derive(Default)]
struct ThumbPerfMetrics {
    processed: AtomicU64,
    published: Mutex<PublishedSummary>,
    deferred: AtomicU64,
    skipped: AtomicU64,
    phase: AtomicU8,
}

impl ThumbPerfMetrics {
    fn record_committed(&self, outcome: &ImageExecution) {
        let mut published = self.published.lock().unwrap_or_else(|e| e.into_inner());
        match outcome {
            ImageExecution::Published(result, execution) => {
                published.record(result.thumb_status, *execution)
            }
            ImageExecution::Unavailable(_) => published.results.temporarily_unavailable += 1,
            _ => return,
        }
        self.processed.fetch_add(1, Ordering::Relaxed);
    }

    fn running_progress(&self, total: u64) -> FullThumbProgressPayload {
        let phase = match self.phase.load(Ordering::Relaxed) {
            0 => ThumbnailRunPhase::Fast,
            1 => ThumbnailRunPhase::Heavy,
            2 => ThumbnailRunPhase::Exception,
            3 => ThumbnailRunPhase::ImageRs,
            4 => ThumbnailRunPhase::Video,
            _ => ThumbnailRunPhase::Complete,
        };
        progress(
            self.processed.load(Ordering::Relaxed),
            total,
            "running",
            Some(phase),
        )
        .with_results(self)
    }
}

fn log_thumb_window(
    run_id: &str,
    origin: &str,
    event: &str,
    total: u64,
    started: Instant,
    metrics: &ThumbPerfMetrics,
    state: &AppState,
) {
    let phase = match metrics.phase.load(Ordering::Relaxed) {
        0 => "fast",
        1 => "heavy",
        2 => "exception",
        _ => "complete",
    };
    let (shared_workset_reserved_bytes, shared_workset_peak_reserved_bytes, thumb_queued) =
        state.thumb_coordinator.resource_snapshot();
    let native = state.thumb_coordinator.native_counters();
    let host_qos = crate::thumbnail::qos::host_qos_counts(native.qos_revision);
    let combined_cpu_ms = native
        .host_cpu_ms
        .zip(native.worker_job_cpu_ms)
        .map(|(host, worker)| host.saturating_add(worker));
    let host_stage = state.thumb_coordinator.host_stage_timings();
    let heavy_available = state.background_heavy_limiter.available();
    let (cpu_admission_total, cpu_admission_active, cpu_fast_active, cpu_admission_peak) =
        state.background_heavy_limiter.snapshot();
    let source_io = state.background_volume_io_budget.snapshot();
    let published = metrics.published.lock().unwrap_or_else(|e| e.into_inner());
    info!(
        target: "scrollery::thumb_perf",
        run_id,
        origin,
        event,
        phase,
        total,
        processed = published.processed(),
        encoded_published = published.results.available,
        encoded_new = published.results.newly_generated,
        cache_hit = published.results.cache_hit,
        direct_registered = published.results.direct,
        failed = published.results.failed,
        temporarily_unavailable = published.results.temporarily_unavailable,
        run_published_executions = ?published.executions,
        deferred = metrics.deferred.load(Ordering::Relaxed),
        skipped = metrics.skipped.load(Ordering::Relaxed),
        shared_workset_reserved_bytes,
        shared_workset_peak_reserved_bytes,
        thumb_queued,
        heavy_available,
        cpu_admission_total,
        cpu_admission_active,
        cpu_fast_active,
        cpu_admission_peak,
        heavy_total = state.background_heavy_limiter.total(),
        source_io_active_by_volume = ?source_io.active_by_volume,
        source_io_permits = source_io.current,
        app_source_io_peak_permits = source_io.peak,
        app_source_io_denied_attempts = source_io.denied_attempts,
        source_io_media_epoch = source_io.media_epoch,
        source_io_volume_device_seek_penalty = ?source_io.media_by_volume,
        source_io_ssd_limit = source_io.ssd_limit,
        source_io_hdd_limit = source_io.hdd_limit,
        source_io_unknown_volume_limit = source_io.unknown_limit,
        app_native_embedded_jpeg_total = native.embedded_jpeg,
        app_gpu_inflight = native.gpu_inflight,
        app_gpu_peak_inflight = native.gpu_peak,
        app_gpu_admission_denied = native.gpu_denied,
        app_gpu_inflight_limit = native.gpu_limit,
        app_native_image_d2d_total = native.image_d2d,
        app_native_image_vpl_total = native.image_vpl,
        app_native_image_wic_total = native.image_wic,
        app_native_image_rs_total = native.image_rs,
        app_native_video_mf_total = native.video_mf,
        app_native_video_hardware_mft_total = native.video_hardware_mft,
        app_native_failed_total = native.failed_responses,
        app_native_timeout_total = native.timed_out,
        app_native_worker_lost_total = native.worker_lost,
        app_native_qos_apply_accepted_total = native.qos_apply_accepted,
        app_native_qos_apply_failed_total = native.qos_apply_failed,
        app_native_qos_cross_boundary_total = native.qos_cross_boundary,
        app_native_domain_wait_count = native.domain_wait.count,
        app_native_domain_wait_ms_sum = native.domain_wait.sum_ms,
        app_native_domain_wait_ms_max = native.domain_wait.max_ms,
        app_native_domain_wait_p50_ms_upper = native.domain_wait.p50_ms_upper,
        app_native_domain_wait_p95_ms_upper = native.domain_wait.p95_ms_upper,
        app_native_request_wall_count = native.request_wall.count,
        app_native_request_wall_ms_sum = native.request_wall.sum_ms,
        app_native_request_wall_ms_max = native.request_wall.max_ms,
        app_native_request_wall_p50_ms_upper = native.request_wall.p50_ms_upper,
        app_native_request_wall_p95_ms_upper = native.request_wall.p95_ms_upper,
        app_native_decode_transform_count = native.decode_transform.count,
        app_native_decode_transform_ms_sum = native.decode_transform.sum_ms,
        app_native_decode_transform_ms_max = native.decode_transform.max_ms,
        app_native_decode_transform_p50_ms_upper = native.decode_transform.p50_ms_upper,
        app_native_decode_transform_p95_ms_upper = native.decode_transform.p95_ms_upper,
        app_native_encode_hash_count = native.encode_hash.count,
        app_native_encode_hash_ms_sum = native.encode_hash.sum_ms,
        app_native_encode_hash_ms_max = native.encode_hash.max_ms,
        app_native_encode_hash_p50_ms_upper = native.encode_hash.p50_ms_upper,
        app_native_encode_hash_p95_ms_upper = native.encode_hash.p95_ms_upper,
        app_native_embedded_jpeg_combined_count = native.embedded_jpeg_combined.count,
        app_native_embedded_jpeg_combined_ms_sum = native.embedded_jpeg_combined.sum_ms,
        app_native_embedded_jpeg_combined_ms_max = native.embedded_jpeg_combined.max_ms,
        app_native_embedded_jpeg_combined_p50_ms_upper = native.embedded_jpeg_combined.p50_ms_upper,
        app_native_embedded_jpeg_combined_p95_ms_upper = native.embedded_jpeg_combined.p95_ms_upper,
        app_host_working_set_bytes = ?native.host_working_set_bytes,
        app_host_private_bytes = ?native.host_private_bytes,
        app_host_cpu_ms = ?native.host_cpu_ms,
        app_native_job_cpu_ms = ?native.worker_job_cpu_ms,
        app_host_native_cpu_ms = ?combined_cpu_ms,
        cpu_scope = "whole host process + native thumbnail Job, not exclusive to this run",
        qos_revision = native.qos_revision,
        qos_foreground = native.qos_foreground,
        qos_host_current = ?host_qos,
        qos_native_current = ?native.qos_workers,
        host_qos_platform_supported = cfg!(any(windows, target_vendor = "apple")),
        qos_confirmation = "system request accepted; not CPU placement or frequency",
        qos_scope = "coordinator processing and native request threads; codec-internal threads unavailable",
        stage_timing_scope = "app lifetime; decode+transform and hash+encode combined; native encoding breakdown in thumbnail encode summary",
        unavailable_stage_timings = "per-item read/upload/GPU-wait/GPU-timestamp/readback; embedded JPEG and host fallback encode substages",
        gpu_utilization = "unavailable",
        app_native_worker_processes = native.worker_processes,
        app_native_worker_memory_samples = native.worker_memory_samples,
        app_native_worker_working_set_bytes = ?native.worker_working_set_bytes,
        app_native_worker_private_bytes = ?native.worker_private_bytes,
        queue_timing_scope = "unique execution: enqueue to host resource admission; excludes producer backpressure and pre-admission cancellations",
        e2e_timing_scope = "unique execution: enqueue to result fanout, including cancellation/failure/defer; retries are separate executions",
        app_host_queue = ?host_stage.queue_wall,
        app_host_e2e = ?host_stage.e2e_wall,
        app_host_encode_count = host_stage.encode_wall.count,
        app_host_encode_ms_sum = host_stage.encode_wall.sum_ms,
        app_host_encode_ms_max = host_stage.encode_wall.max_ms,
        app_host_encode_p50_ms_upper = host_stage.encode_wall.p50_ms_upper,
        app_host_encode_p95_ms_upper = host_stage.encode_wall.p95_ms_upper,
        app_host_write_count = host_stage.write_wall.count,
        app_host_write_ms_sum = host_stage.write_wall.sum_ms,
        app_host_write_ms_max = host_stage.write_wall.max_ms,
        app_host_write_p50_ms_upper = host_stage.write_wall.p50_ms_upper,
        app_host_write_p95_ms_upper = host_stage.write_wall.p95_ms_upper,
        app_host_db_transaction_count = host_stage.db_transaction_wall.count,
        app_host_db_transaction_ms_sum = host_stage.db_transaction_wall.sum_ms,
        app_host_db_transaction_ms_max = host_stage.db_transaction_wall.max_ms,
        app_host_db_transaction_p50_ms_upper = host_stage.db_transaction_wall.p50_ms_upper,
        app_host_db_transaction_p95_ms_upper = host_stage.db_transaction_wall.p95_ms_upper,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "thumbnail run summary"
    );
    // 编码分项单独成组，避免总览事件继续扩张触及 tracing 宏递归上限。
    info!(
        target: "scrollery::thumb_perf",
        run_id,
        origin,
        event,
        phase,
        app_native_encode_hash_count = native.encode_hash.count,
        app_native_encode_hash_us_sum = native.encode_hash.sum_us,
        app_native_ai_cache = ?native.encode_stages[0],
        app_native_thumb_resize = ?native.encode_stages[1],
        app_native_thumb_color = ?native.encode_stages[2],
        app_native_thumb_hash = ?native.encode_stages[3],
        app_native_thumb_encode = ?native.encode_stages[4],
        stage_timing_scope = "app lifetime; successful native replies; substages included in encode_hash, not additive to it; parallel sums are not wall time",
        encode_substage_scope = "AI cache check+optional generation; thumbnail resize/color/hash/encode incl fallback; zero for skipped work; embedded JPEG remains combined",
        "thumbnail encode summary"
    );
}

struct RunConfigs<'a> {
    image: &'a crate::thumbnail::ThumbConfig,
    video: Option<&'a crate::thumbnail::ThumbConfig>,
}

fn publish_thumb_progress(
    app: &AppHandle,
    state: &AppState,
    epoch: u64,
    generation: u64,
    cancel_token: &CancellationToken,
    payload: impl FnOnce() -> FullThumbProgressPayload,
) -> bool {
    state.with_thumb_generation_gate(|| {
        if !state.thumb_gen_token.is_generation_current(generation) || cancel_token.is_cancelled() {
            return false;
        }
        state
            .with_database_lifecycle_read(epoch, || {
                // 在发布门内采样，避免定时器旧快照晚到后覆盖更新进度或终态。
                let payload = payload();
                *state
                    .thumb_gen_progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(payload.clone());
                let _ = app.emit(THUMB_GEN_PROGRESS_EVENT, payload);
            })
            .is_some()
    })
}

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

fn abandon_thumb_generation(
    app: &AppHandle,
    state: &AppState,
    epoch: u64,
    generation: u64,
    cancel_token: &CancellationToken,
) {
    cancel_token.cancel();
    let _ = finish_thumb_generation(
        app,
        state,
        epoch,
        generation,
        progress(0, 0, "cancelled", None),
    );
}

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
    let native_video_enabled = state
        .config
        .get("enable_video_cover")
        .map(|value| value != "false")
        .unwrap_or(true);
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
                    let tx = conn.unchecked_transaction().map_err(AppError::Db)?;
                    tx.execute("UPDATE media_items SET thumb_status=0, thumb_path=NULL, thumbhash=NULL WHERE is_deleted=0 AND media_type='image'", [])
                        .map_err(AppError::Db)?;
                    q::reset_image_thumbnail_leases(&tx)?;
                    if native_video_enabled {
                        q::reset_native_video_covers_for_full_run(&tx)?;
                        q::reset_native_video_cover_leases(&tx)?;
                    }
                    q::reset_all_exotic_thumbnail_tasks(&tx)?;
                    tx.commit().map_err(AppError::Db)?;
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
        state.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
        Some(reset)
    } else {
        None
    };
    let (generation, cancel_token, epoch) = match reset_generation {
        Some(value) => value,
        None => {
            let Some(value) = state.try_new_thumb_gen_token() else {
                return Ok(());
            };
            value
        }
    };
    if (reset_all && !state.is_database_epoch_current(epoch))
        || (!reset_all && epoch != start_epoch)
    {
        cancel_token.cancel();
        state.with_thumb_generation_gate(|| {
            let _ = state.thumb_gen_token.finish(generation);
        });
        return Ok(());
    }

    let mut config = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    config.size = snap_to_tier(config.size);
    let fp = OutputFingerprint::for_image(&config);
    config.output_fingerprint = Some(fp);
    let mut video_config = config.clone();
    video_config.ai_hq_cache = false;
    let video_fp = OutputFingerprint::for_native_video_cover(&video_config);
    video_config.output_fingerprint = Some(video_fp);
    let run_id = match unique_id() {
        Ok(id) => id,
        Err(error) => {
            abandon_thumb_generation(&app, &state, epoch, generation, &cancel_token);
            return Err(error);
        }
    };
    let metrics = Arc::new(ThumbPerfMetrics::default());
    let observer = state.thumb_coordinator.observe_run(&run_id, {
        let metrics = Arc::clone(&metrics);
        Arc::new(move |outcome| metrics.record_committed(outcome))
    });
    let state_arc = Arc::clone(&*state);
    let enrolled = tokio::task::spawn_blocking(move || {
        state_arc.with_database_lifecycle_read(epoch, || {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let images = q::enroll_image_thumbnail_run(&conn, &run_id, fp)?;
            let videos = if native_video_enabled {
                q::enroll_native_video_cover_run(&conn, &run_id, video_fp)?
            } else {
                0
            };
            Ok((run_id, images + videos))
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))
    .and_then(|value| value.transpose());
    let (run_id, total) = match enrolled {
        Ok(Some(value)) => value,
        Ok(None) => {
            abandon_thumb_generation(&app, &state, epoch, generation, &cancel_token);
            return Ok(());
        }
        Err(error) => {
            abandon_thumb_generation(&app, &state, epoch, generation, &cancel_token);
            return Err(error);
        }
    };
    if total == 0 {
        info!(target: "scrollery::thumb_perf", run_id,
            origin = if reset_all { "full" } else { "incremental" },
            epoch, generation, total = 0, status = "completed",
            "thumbnail run empty");
        let _ = finish_thumb_generation(
            &app,
            &state,
            epoch,
            generation,
            progress(0, 0, "completed", None),
        );
        return Ok(());
    }

    state.thumb_coordinator.begin_native_run();

    let origin = if reset_all { "full" } else { "incremental" };
    let started = Instant::now();
    info!(
        target: "scrollery::thumb_perf",
        run_id,
        origin,
        epoch,
        generation,
        total,
        image_fingerprint = %fp.hex(),
        native_video_enabled,
        build_profile = env!("SCROLLERY_BUILD_PROFILE"),
        build_opt_level = env!("SCROLLERY_BUILD_OPT_LEVEL"),
        build_target = env!("SCROLLERY_BUILD_TARGET"),
        package_kind = env!("SCROLLERY_PACKAGE_KIND"),
        debug_assertions = cfg!(debug_assertions),
        app_version = env!("CARGO_PKG_VERSION"),
        schema_version = crate::db::schema::SCHEMA_VERSION,
        requested_strategy = %config.strategy,
        size = config.size,
        webp_quality = config.webp_quality,
        ai_hq_cache = config.ai_hq_cache,
        "thumbnail run started"
    );
    let ticker_stop = CancellationToken::new();
    let ticker_stop_task = ticker_stop.clone();
    let ticker_metrics = Arc::clone(&metrics);
    let ticker_run_id = run_id.clone();
    let ticker_state = Arc::clone(&*state);
    let ticker_app = app.clone();
    let ticker_cancel = cancel_token.clone();
    let mut focus_changes = crate::thumbnail::qos::subscribe_focus_changes();
    let mut observed_focus = crate::thumbnail::qos::native_worker_qos_request();
    tokio::spawn(async move {
        let mut focus_observed_at = Instant::now();
        let period = Duration::from_secs(5);
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = ticker_stop_task.cancelled() => break,
                changed = focus_changes.changed() => {
                    if changed.is_err() { break; }
                    let current = crate::thumbnail::qos::native_worker_qos_request();
                    if current != observed_focus {
                        info!(target: "scrollery::thumb_perf", run_id = %ticker_run_id,
                            previous_foreground = observed_focus.0, previous_revision = observed_focus.1,
                            foreground = current.0, qos_revision = current.1,
                            observed_window_ms = focus_observed_at.elapsed().as_millis() as u64,
                            observed_revision_changes = current.1.wrapping_sub(observed_focus.1),
                            focus_window_scope = "async observation boundary; cumulative counters, rapid changes may coalesce",
                            "thumbnail focus window closed");
                        log_thumb_window(&ticker_run_id, origin, "focus_change", total as u64, started, &ticker_metrics, &ticker_state);
                        observed_focus = current;
                        focus_observed_at = Instant::now();
                    }
                },
                _ = ticker.tick() => {
                    log_thumb_window(&ticker_run_id, origin, "window", total as u64, started, &ticker_metrics, &ticker_state);
                    publish_thumb_progress(&ticker_app, &ticker_state, epoch, generation, &ticker_cancel,
                        || ticker_metrics.running_progress(total as u64));
                }
            }
        }
    });
    let state_arc = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || {
        let report = |phase: ThumbnailRunPhase| {
            let phase_code = match phase {
                ThumbnailRunPhase::Fast => 0,
                ThumbnailRunPhase::Heavy => 1,
                ThumbnailRunPhase::Exception => 2,
                ThumbnailRunPhase::ImageRs => 3,
                ThumbnailRunPhase::Video => 4,
                ThumbnailRunPhase::Complete => 5,
            };
            let prior = metrics.phase.swap(phase_code, Ordering::Relaxed);
            if prior != phase_code {
                log_thumb_window(
                    &run_id,
                    origin,
                    "phase",
                    total as u64,
                    started,
                    &metrics,
                    &state_arc,
                );
            }
            let _ =
                publish_thumb_progress(&app, &state_arc, epoch, generation, &cancel_token, || {
                    metrics.running_progress(total as u64)
                });
        };
        let on_result = |outcome: ImageExecution| match outcome {
            ImageExecution::Published(..) | ImageExecution::Unavailable(_) => {
                let current = metrics.processed.load(Ordering::Relaxed);
                if current.is_multiple_of(50) {
                    let phase = match metrics.phase.load(Ordering::Relaxed) {
                        0 => ThumbnailRunPhase::Fast,
                        1 => ThumbnailRunPhase::Heavy,
                        2 => ThumbnailRunPhase::Exception,
                        3 => ThumbnailRunPhase::ImageRs,
                        _ => ThumbnailRunPhase::Video,
                    };
                    report(phase);
                }
            }
            ImageExecution::Deferred(_) => {
                metrics.deferred.fetch_add(1, Ordering::Relaxed);
            }
            ImageExecution::Skipped(_) => {
                metrics.skipped.fetch_add(1, Ordering::Relaxed);
            }
        };
        report(ThumbnailRunPhase::Fast);
        let result = run_background_images(
            &state_arc,
            epoch,
            &run_id,
            RunConfigs {
                image: &config,
                video: native_video_enabled.then_some(&video_config),
            },
            &cancel_token,
            &on_result,
            &report,
        );
        observer.synchronize();
        let status = match result {
            Err(error) => {
                error!("thumbnail run failed: {error}");
                cancel_token.cancel();
                "error"
            }
            Ok(()) if cancel_token.is_cancelled() => "cancelled",
            Ok(()) => "completed",
        };
        let _ = finish_thumb_generation(
            &app,
            &state_arc,
            epoch,
            generation,
            progress(
                metrics.processed.load(Ordering::Relaxed),
                total as u64,
                status,
                None,
            )
            .with_results(&metrics),
        );
        ticker_stop.cancel();
        log_thumb_window(
            &run_id,
            origin,
            status,
            total as u64,
            started,
            &metrics,
            &state_arc,
        );
    });
    Ok(())
}

fn progress(
    generated: u64,
    total: u64,
    status: &str,
    phase: Option<ThumbnailRunPhase>,
) -> FullThumbProgressPayload {
    FullThumbProgressPayload {
        generated,
        total,
        status: status.into(),
        current_item: None,
        results: ThumbResultCounts::default(),
        executions: Vec::new(),
        phase: phase.map(|phase| {
            match phase {
                ThumbnailRunPhase::Fast => "fast",
                ThumbnailRunPhase::Heavy => "heavy",
                ThumbnailRunPhase::Exception => "exception",
                ThumbnailRunPhase::ImageRs => "image_rs",
                ThumbnailRunPhase::Video => "video",
                ThumbnailRunPhase::Complete => "complete",
            }
            .to_owned()
        }),
    }
}

fn run_background_images<F, P>(
    state: &Arc<AppState>,
    epoch: u64,
    run_id: &str,
    configs: RunConfigs<'_>,
    cancel: &CancellationToken,
    on_result: &F,
    report: &P,
) -> Result<()>
where
    F: Fn(ImageExecution) + Sync,
    P: Fn(ThumbnailRunPhase),
{
    let RunConfigs {
        image: config,
        video: video_config,
    } = configs;
    let fp = config
        .output_fingerprint
        .expect("run config has fingerprint");
    // 分类生产者不打开源文件，逐页送 Q1；Q2 只留下持久任务行，不占图片缓冲。
    state.thumb_coordinator.run_images(
        state,
        epoch,
        config,
        cancel,
        |tx| {
            let mut cursor = 0;
            loop {
                if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
                    break;
                }
                let page = {
                    let conn = state.db_read_pool.get().map_err(AppError::from)?;
                    q::image_thumbnail_run_page(&conn, run_id, cursor, 256)?
                };
                if page.is_empty() {
                    break;
                }
                let mut candidates = page.into_iter();
                loop {
                    let batch: Vec<_> = candidates
                        .by_ref()
                        .take(50)
                        .map(|candidate| {
                            cursor = candidate.item.id;
                            let lane = classify_image_cost(
                                &candidate.item.file_format,
                                candidate.item.file_size,
                                candidate.item.width,
                                candidate.item.height,
                            );
                            (candidate, lane)
                        })
                        .collect();
                    if batch.is_empty() {
                        break;
                    }
                    let entries: Vec<_> = batch
                        .iter()
                        .map(|(candidate, lane)| {
                            (
                                ThumbnailTaskKey {
                                    item_id: candidate.item.id,
                                    source_revision: candidate.item.source_revision,
                                    kind: ThumbnailTaskKind::Image,
                                    output_fingerprint: fp.hex(),
                                },
                                *lane,
                            )
                        })
                        .collect();
                    let classified = state.with_database_lifecycle_read(epoch, || {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        q::classify_image_thumbnail_batch(&conn, run_id, &entries)
                    });
                    let Some(classified) = classified.transpose()? else {
                        return Ok(());
                    };
                    for ((candidate, lane), accepted) in batch.into_iter().zip(classified) {
                        if accepted
                            && lane == ThumbnailLane::Fast
                            && tx.send((candidate, lane)).is_err()
                        {
                            return Ok(());
                        }
                    }
                }
            }
            Ok(())
        },
        on_result,
    )?;

    let mut phase = ThumbnailRunPhase::Fast;
    for lane in [
        ThumbnailLane::Fast,
        ThumbnailLane::Heavy,
        ThumbnailLane::Exception,
        ThumbnailLane::ImageRs,
    ] {
        if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
            break;
        }
        report(phase);
        let counts =
            run_background_image_phase(state, epoch, run_id, config, lane, cancel, on_result)?;
        phase = advance_run_phase(phase, true, counts);
    }
    // 软件图片完成后才派发视频，两类媒体不再抢占同阶段额度。
    if let Some(video_config) = video_config {
        for (lane, index) in [(ThumbnailLane::Heavy, 1), (ThumbnailLane::Exception, 2)] {
            if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
                break;
            }
            report(ThumbnailRunPhase::Video);
            state.thumb_coordinator.run_background_video_cover_phase(
                state,
                epoch,
                video_config,
                VideoCoverPhase {
                    run_id,
                    lane,
                    index,
                },
                cancel,
                on_result,
            )?;
        }
    }
    Ok(())
}
/// 完成本段图片任务（包括在途提交）后才推进阶段。
fn run_background_image_phase<F>(
    state: &Arc<AppState>,
    epoch: u64,
    run_id: &str,
    config: &crate::thumbnail::ThumbConfig,
    lane: ThumbnailLane,
    cancel: &CancellationToken,
    on_result: &F,
) -> Result<[u64; 4]>
where
    F: Fn(ImageExecution) + Sync,
{
    let index = match lane {
        ThumbnailLane::Fast => 0,
        ThumbnailLane::Heavy => 1,
        ThumbnailLane::Exception => 2,
        ThumbnailLane::ImageRs => 3,
        _ => unreachable!("background image phase uses only run lanes"),
    };
    loop {
        if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
            return Ok([0; 4]);
        }
        let counts = {
            let conn = state.db_read_pool.get().map_err(AppError::from)?;
            q::image_thumbnail_run_open_counts(&conn, run_id)?
        };
        if counts[index] == 0 {
            return Ok(counts);
        }
        let has_ready = {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| AppError::Internal("system clock before Unix epoch".into()))?
                .as_secs() as i64;
            let conn = state.db_read_pool.get().map_err(AppError::from)?;
            !q::image_thumbnail_lane_page(&conn, run_id, lane, 0, 1, now)?.is_empty()
        };
        if !has_ready {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let mut dispatched = 0;
        state.thumb_coordinator.run_images(
            state,
            epoch,
            config,
            cancel,
            |tx| {
                let mut cursor = 0;
                loop {
                    if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
                        break;
                    }
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| AppError::Internal("system clock before Unix epoch".into()))?
                        .as_secs() as i64;
                    let page = {
                        let conn = state.db_read_pool.get().map_err(AppError::from)?;
                        q::image_thumbnail_lane_page(&conn, run_id, lane, cursor, 256, now)?
                    };
                    if page.is_empty() {
                        break;
                    }
                    for candidate in page {
                        cursor = candidate.item.id;
                        if tx.send((candidate, lane)).is_err() {
                            return Ok(());
                        }
                        dispatched += 1;
                    }
                }
                Ok(())
            },
            on_result,
        )?;
        if dispatched == 0 {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

#[tauri::command]
pub fn stop_full_thumbnail_generation(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("User action: Stopping full thumbnail generation | 用户操作：停止全量缩略图生成");
    state.cancel_thumb_gen();
    Ok(())
}
