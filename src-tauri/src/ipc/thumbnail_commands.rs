//! 用于缩略图生成的 Tauri IPC 命令（§ 6.1 — 缩略图）。

use std::sync::Arc;

use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::db::models::ThumbResult;
use crate::error::{AppError, Result};
use crate::exotic::{ExoticHost, ExoticTaskStatus};
use crate::scanner::enricher::MediaEnrichedPayload;
use crate::state::AppState;
use crate::thumbnail::generator::snap_to_tier;
use crate::thumbnail::{route_thumbnail, ThumbnailRoute, ThumbnailRouteInput};

// 全库与视口普通图片均由 ThumbnailCoordinator 领取并条件提交；冷门格式仍由 exotic 接管。

// 全库生成流水线（run_thumbnail_generation + 进度/开关命令）已拆至 `thumbnail_full_gen.rs`
// （见超长文件拆分方案 tierB-3）；下方 `pub use *` 转发保持 `ipc::thumbnail_commands::X` 外部路径
// 不变 —— `ipc/registry.rs` 的 `generate_handler!` 与 `state.rs` 均按此路径引用，漏转发即编译失败。
// 必须用 glob（非具名列表）：`#[tauri::command]` 宏在同一展开里额外生成 `__cmd__*` 等隐藏兄弟
// 项，具名 `pub use {a, b, c}` 只转发写出的名字、漏转发这些隐藏项会令 `generate_handler!` 报
// "cannot find __cmd__x"。
pub use super::thumbnail_full_gen::*;

/// 视口请求的逐项回包；pending 表示本次尚无可显示的最终结果。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportThumbResult {
    #[serde(flatten)]
    result: ThumbResult,
    pending: bool,
}

impl ViewportThumbResult {
    fn from_result(result: ThumbResult) -> Self {
        Self {
            pending: result.thumb_status == 0,
            result,
        }
    }
}

#[tauri::command]
pub async fn batch_request_thumbnails(
    item_ids: Vec<i64>,
    target_size: Option<u32>,
    request_id: String,
    on_result: tauri::ipc::Channel<ViewportThumbResult>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 debug 档:视口滚动热路径查询,默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:batch_request_thumbnails");
    let state_arc = state.inner().clone();
    let Some(database_epoch) = state_arc.current_database_epoch() else {
        return Ok(());
    };
    let request = state_arc
        .thumb_coordinator
        .register_viewport_request(request_id, &item_ids)?;
    let mut config = {
        state_arc
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    };
    if let Some(size) = target_size {
        config.size = size;
    }
    // 归一到有效档位:本路径直调 decode/encode_media_step(出口不吸附档位),
    // config.size 可能来自前端 target_size 或库中旧值(如历史默认 480),非档位值会令
    // thumb_path 断言失败/写入无效档位目录。幂等——已是档位则不变。
    config.size = snap_to_tier(config.size);
    let output_fp = crate::thumbnail::scheduler::OutputFingerprint::for_image(&config);
    config.output_fingerprint = Some(output_fp);
    let video_fp = crate::thumbnail::scheduler::OutputFingerprint::for_native_video_cover(&config);
    let expected_size = config.size;

    // R1-3：批量缓存查询走 read_blocking。
    let ids_for_query = item_ids.clone();
    let (fast_results, route_fmt, route_type, route_cache_key) =
        super::blocking::read_blocking(&state, move |conn| {
            // 批量查缓存
            let mut fast_results = std::collections::HashMap::new();
            // id → file_format（R7：缓存查询前置扩列 file_format，供 Router 判定，避免 N+1）。
            let mut route_fmt: std::collections::HashMap<i64, String> =
                std::collections::HashMap::new();
            let mut route_type: std::collections::HashMap<i64, String> =
                std::collections::HashMap::new();
            // id → cache_key（问题4：done 任务重算期望指纹需要，避免 Router 内回查）。
            let mut route_cache_key: std::collections::HashMap<i64, i64> =
                std::collections::HashMap::new();
            let placeholders = ids_for_query
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");

            if !placeholders.is_empty() {
                let sql = format!(
                    "SELECT id, thumb_status, thumb_path, thumbhash, file_format, cache_key, source_revision, media_type FROM media_items WHERE id IN ({})",
                    placeholders
                );
                let mut stmt = conn.prepare(&sql).map_err(AppError::Db)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(&ids_for_query), |row| {
                        Ok((
                            ThumbResult {
                                item_id: row.get(0)?,
                                thumb_status: row.get(1)?,
                                thumb_path: row.get(2)?,
                                thumbhash: row.get(3)?,
                                source_revision: row.get(6)?,
                                cache_key: row.get(5)?,
                            },
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(7)?,
                        ))
                    })
                    .map_err(AppError::Db)?;

                for (r, fmt, cache_key, media_type) in rows.flatten() {
                    route_cache_key.insert(r.item_id, cache_key);
                    route_fmt.insert(r.item_id, fmt);
                    let fp = if media_type == "video" { video_fp } else { output_fp };
                    route_type.insert(r.item_id, media_type);
                    let expected = crate::thumbnail::cache::thumb_variant_db_path(
                        expected_size,
                        cache_key,
                        fp,
                    );
                    if (r.thumb_status == 1
                        && r.thumb_path.as_deref() == Some(expected.as_str()))
                        || r.thumb_status == 3
                        || r.thumb_status == 2
                    {
                        fast_results.insert(r.item_id, r);
                    }
                }
            }
            Ok((fast_results, route_fmt, route_type, route_cache_key))
        })
        .await?;
    let mut needs_gen = Vec::new();

    for &id in &item_ids {
        if let Some(r) = fast_results.get(&id) {
            let sent = state_arc.with_database_lifecycle_read(database_epoch, || {
                request.deliver(id, || {
                    on_result
                        .send(ViewportThumbResult::from_result(r.clone()))
                        .is_ok()
                })
            });
            match sent {
                Some(Some(true)) | Some(None) => {}
                Some(Some(false)) => {
                    tracing::debug!("Channel disconnected, ignoring thumb result send");
                }
                None => return Ok(()),
            }
        } else {
            needs_gen.push(id);
        }
    }

    // 将快速路径结果同步到 layout_cache，使 fetchRowsByY 返回
    // 最新的 thumb_status（避免先前生成后仍返回陈旧的 status=0）。
    if !fast_results.is_empty() {
        let fast_vec: Vec<ThumbResult> = fast_results.values().cloned().collect();
        let _ = state_arc.with_database_lifecycle_read(database_epoch, || {
            state_arc.apply_thumb_results(&fast_vec);
        });
    }

    // ── 冷门格式让路（R3）：needs_gen 中命中未完成 Exotic 的项不进主 generator ──────────
    // 在 needs_gen 过滤点接入 route_thumbnail（与 full 命令共享同一纯函数判定）。
    // 让路项绝不进 decode_media_step，也绝不写 thumb_status=2。
    if !needs_gen.is_empty() {
        let snap = state_arc.exotic_catalog.snapshot();
        // 仅当批内确有 catalog 已认领的格式时才走完整路由，常见库零额外成本。
        let has_exotic = needs_gen.iter().any(|id| {
            route_fmt
                .get(id)
                .map(|f| snap.resolve_format(f).is_some())
                .unwrap_or(false)
        });
        if has_exotic {
            // R1-3：路由段含读池 SQL（任务态批查）+ 写锁 SQL（指纹失效退回 pending），与指纹
            // 计算一并下沉 blocking；闭包返回过滤后的 needs_gen（kept）。
            let state_gate = state_arc.clone();
            let on_result_gate = on_result.clone();
            let request_gate = request.clone();
            let route_fmt_for_gate = route_fmt.clone();
            needs_gen = tokio::task::spawn_blocking(move || -> Result<Vec<i64>> {
                // 路由仅需 catalog 认领 + 任务态（route_thumbnail 不读 availability）——未授权/未安装的 PSD
                // 同样不能进主 generator（主解码必失败）。故用 stub Host（无 DB/keyring），避免每项安装/授权
                // 查询造成 N+1（R7）；真实安装/授权真相由 get_exotic_item_state 等命令按需读取。
                let host = ExoticHost::new(state_gate.exotic_catalog.clone());
                let task_map = {
                    let conn = state_gate.db_read_pool.get().map_err(AppError::from)?;
                    crate::db::queries::exotic_thumbnail_route_info_for_items(&conn, &needs_gen)
                        .unwrap_or_default()
                };
                // 指纹档位用全局 thumb_config（与 Coordinator/Pipeline 同源），非 batch 的 target_size
                // override——exotic 不经 batch 生成，指纹须对齐 Coordinator 所用档位（问题4）。
                let global_size = {
                    state_gate
                        .thumb_config
                        .read()
                        .unwrap_or_else(|e| e.into_inner())
                        .size
                };
                let mut kept = Vec::with_capacity(needs_gen.len());
                let mut gated = Vec::new();
                // done 但指纹已失效（如用户改档位）→ 须先失效为 pending 再让路重做（Part2 §4.3，问题4）。
                let mut stale = Vec::new();
                for &id in &needs_gen {
                    let fmt = route_fmt_for_gate
                        .get(&id)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    if snap.resolve_format(fmt).is_none() {
                        kept.push(id); // 常见格式快速路径，不构造 resolution。
                        continue;
                    }
                    let res = host.resolve_format(fmt);
                    let info = task_map.get(&id);
                    let task_status = info.map(|i| i.status);
                    // done 任务重算期望指纹比对存储指纹；缺任一指纹输入 → 保守失效、让路重做。
                    let fingerprint_valid = match (task_status, info) {
                        (Some(ExoticTaskStatus::Done), Some(i)) => match (
                            route_cache_key.get(&id),
                            i.worker_version.as_deref(),
                            i.input_fingerprint.as_deref(),
                            res.plugin_id.as_deref(),
                        ) {
                            (Some(&ck), Some(wv), Some(stored), Some(pid)) => {
                                crate::exotic::fingerprint::thumbnail_fingerprint(
                                    ck,
                                    pid,
                                    wv,
                                    global_size,
                                )
                                .fingerprint
                                    == stored
                            }
                            _ => false,
                        },
                        _ => false, // 非 done：router 不使用该值
                    };
                    let route = route_thumbnail(&ThumbnailRouteInput {
                        item_id: id,
                        file_format: fmt,
                        thumb_status: 0, // needs_gen 项均为 thumb_status=0
                        resolution: Some(&res),
                        task_status,
                        fingerprint_valid,
                    });
                    match route {
                        // offering 不认领 thumbnail（如仅 metadata）→ 主 generator。
                        ThumbnailRoute::Common => kept.push(id),
                        // exotic 项一律不送主 generator（PSD 主解码必失败）；done+valid 走此。
                        ThumbnailRoute::Existing => gated.push(id),
                        ThumbnailRoute::Exotic(_) => {
                            // done 却被判 Exotic ⟺ 指纹失效（done+valid 会判 Existing）→ 失效重做。
                            if task_status == Some(ExoticTaskStatus::Done) {
                                stale.push(id);
                            }
                            gated.push(id);
                        }
                    }
                }
                // 指纹失效的 done 先退回 pending（否则 Coordinator claim 只取 0/3，永不重做）。
                if !stale.is_empty() {
                    let invalidated =
                        state_gate.with_database_lifecycle_read(database_epoch, || {
                            let conn = state_gate
                                .db_writer
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            for &id in &stale {
                                let _ =
                                    crate::db::queries::invalidate_exotic_tasks_for_item(&conn, id);
                            }
                        });
                    if invalidated.is_none() {
                        return Ok(Vec::new());
                    }
                    info!(
                        "batch_request_thumbnails: {} 项 exotic done 指纹失效 → 退回 pending 重做",
                        stale.len()
                    );
                }

                if !state_gate.is_database_epoch_current(database_epoch) {
                    return Ok(Vec::new());
                }

                // 让路项回送当前状态（thumb_status=0、无产物），平衡前端在途计数（问题9），
                // 绝不写 thumb_status=2。真正出图由 exotic Worker 流水线完成。
                if !gated.is_empty() {
                    info!(
                        "batch_request_thumbnails: {} 项让路冷门格式插件（不调主 generator）",
                        gated.len()
                    );
                    for id in &gated {
                        request_gate.deliver(*id, || {
                            let _ = on_result_gate.send(ViewportThumbResult::from_result(
                                ThumbResult {
                                    item_id: *id,
                                    thumb_status: 0,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision: 0,
                                    cache_key: 0,
                                },
                            ));
                        });
                    }
                }
                // 合并发一次 wake：让 Coordinator 领取 pending（含刚失效的 stale）。
                if !gated.is_empty() || !stale.is_empty() {
                    state_gate.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
                }
                Ok(kept)
            })
            .await
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
        }
    }

    let native_video_enabled = state_arc
        .config
        .get("enable_video_cover")
        .map(|value| value != "false")
        .unwrap_or(true);
    let mut image_ids = Vec::new();
    let mut video_ids = Vec::new();
    for id in needs_gen {
        match route_type.get(&id).map(String::as_str) {
            Some("image") => image_ids.push(id),
            Some("video")
                if native_video_enabled
                    && route_fmt.get(&id).is_some_and(|fmt| {
                        crate::video::native_cover_formats().contains(&fmt.as_str())
                    }) =>
            {
                video_ids.push(id);
            }
            _ => {
                request.deliver(id, || {
                    let _ = on_result.send(ViewportThumbResult::from_result(ThumbResult {
                        item_id: id,
                        thumb_status: 0,
                        thumb_path: None,
                        thumbhash: None,
                        source_revision: 0,
                        cache_key: 0,
                    }));
                });
            }
        }
    }
    info!(
        total = item_ids.len(),
        images = image_ids.len(),
        native_videos = video_ids.len(),
        "viewport thumbnail requests accepted"
    );

    // 两类任务独立提交，共用 Coordinator 的持久队列；慢视频不延迟普通图片回传。
    for (ids, is_video) in [(image_ids, false), (video_ids, true)] {
        if ids.is_empty() {
            continue;
        }
        let state_for_work = state_arc.clone();
        let on_result_for_work = on_result.clone();
        let request_for_work = request.clone();
        let mut task_config = config.clone();
        if is_video {
            task_config.ai_hq_cache = false;
            task_config.output_fingerprint = Some(video_fp);
        }
        let _worker = tokio::task::spawn_blocking(move || {
            let requested = ids.clone();
            let result = if is_video {
                run_viewport_video_covers(
                    state_for_work.clone(),
                    database_epoch,
                    ids,
                    task_config,
                    |result| {
                        request_for_work.deliver(result.item_id, || {
                            let _ =
                                on_result_for_work.send(ViewportThumbResult::from_result(result));
                        });
                    },
                    Some(&request_for_work),
                )
            } else {
                run_viewport_images(
                    state_for_work.clone(),
                    database_epoch,
                    ids,
                    task_config,
                    on_result_for_work.clone(),
                    &request_for_work,
                )
            };
            if let Err(error) = result {
                tracing::warn!(error = %error, "viewport thumbnail batch failed");
                if state_for_work.is_database_epoch_current(database_epoch) {
                    for item_id in requested {
                        request_for_work.deliver(item_id, || {
                            let _ = on_result_for_work.send(ViewportThumbResult::from_result(
                                ThumbResult {
                                    item_id,
                                    thumb_status: 0,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision: 0,
                                    cache_key: 0,
                                },
                            ));
                        });
                    }
                }
            }
        });
    }
    Ok(())
}

/// 撤销单格视口回传与等待；共享任务仍由 DB lease 和其他请求方管理。
#[tauri::command]
pub fn cancel_viewport_thumbnail_request(
    request_id: String,
    item_ids: Vec<i64>,
    state: State<'_, Arc<AppState>>,
) {
    state
        .thumb_coordinator
        .cancel_viewport_request(&request_id, &item_ids);
}

fn run_viewport_images(
    state: Arc<AppState>,
    epoch: u64,
    item_ids: Vec<i64>,
    config: crate::thumbnail::ThumbConfig,
    on_result: tauri::ipc::Channel<ViewportThumbResult>,
    request: &Arc<crate::thumbnail::coordinator::ViewportRequest>,
) -> Result<()> {
    use crate::db::queries::{self as q, ThumbnailLane, ThumbnailTaskKey, ThumbnailTaskKind};
    use crate::thumbnail::coordinator::{unique_id, ImageExecution};
    use crate::thumbnail::scheduler::classify_image_cost;

    let fp = config
        .output_fingerprint
        .ok_or_else(|| AppError::Internal("viewport thumbnail fingerprint missing".into()))?;
    let run_id = unique_id()?;
    let candidates = {
        let conn = state.db_read_pool.get().map_err(AppError::from)?;
        q::image_thumbnail_candidates_for_ids(&conn, &item_ids)?
    };
    let by_id: std::collections::HashMap<_, _> =
        candidates.into_iter().map(|c| (c.item.id, c)).collect();
    let cancel = tokio_util::sync::CancellationToken::new();
    let deferred = std::sync::Mutex::new(Vec::new());
    let skipped = std::sync::Mutex::new(Vec::new());
    let send = |result: ThumbResult| {
        request.deliver(result.item_id, || {
            let _ = on_result.send(ViewportThumbResult::from_result(result));
        });
    };

    let collect = |outcome: ImageExecution| match outcome {
        ImageExecution::Published(result, _) => {
            send(result);
        }
        ImageExecution::Unavailable(id) => {
            if let Some(candidate) = by_id.get(&id) {
                send(pending_thumbnail_result(candidate));
            }
        }
        ImageExecution::Deferred(id) => {
            deferred.lock().unwrap_or_else(|e| e.into_inner()).push(id);
        }
        ImageExecution::Skipped(id) => {
            skipped.lock().unwrap_or_else(|e| e.into_inner()).push(id);
        }
    };
    state.thumb_coordinator.run_images(
        &state,
        epoch,
        &config,
        &cancel,
        |tx| {
            for &id in &item_ids {
                if !state.is_database_epoch_current(epoch) {
                    break;
                }
                if !request.is_active(id) {
                    continue;
                }
                let Some(candidate) = by_id.get(&id) else {
                    send(ThumbResult {
                        item_id: id,
                        thumb_status: 0,
                        thumb_path: None,
                        thumbhash: None,
                        source_revision: 0,
                        cache_key: 0,
                    });
                    continue;
                };
                let lane = classify_image_cost(
                    &candidate.item.file_format,
                    candidate.item.file_size,
                    candidate.item.width,
                    candidate.item.height,
                );
                let key = ThumbnailTaskKey {
                    item_id: id,
                    source_revision: candidate.item.source_revision,
                    kind: ThumbnailTaskKind::Image,
                    output_fingerprint: fp.hex(),
                };
                let offered = state.with_database_lifecycle_read(epoch, || -> Result<()> {
                    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    q::enqueue_thumbnail_task(&conn, &key, lane, lane.as_str(), &run_id)?;
                    q::promote_thumbnail_task(&conn, &key, lane)?;
                    Ok(())
                });
                if offered.transpose()?.is_none() {
                    break;
                }
                let dispatch_lane = match lane {
                    ThumbnailLane::Fast => ThumbnailLane::ViewportFast,
                    ThumbnailLane::Heavy => ThumbnailLane::ViewportHeavy,
                    other => other,
                };
                if tx
                    .send_viewport((candidate.clone(), dispatch_lane), request)
                    .is_err()
                {
                    break;
                }
            }
            Ok(())
        },
        collect,
    )?;

    let retry_ids = std::mem::take(&mut *deferred.lock().unwrap_or_else(|e| e.into_inner()));
    if !retry_ids.is_empty() && state.is_database_epoch_current(epoch) {
        state.thumb_coordinator.run_images(
            &state,
            epoch,
            &config,
            &cancel,
            |tx| {
                for id in retry_ids {
                    if !request.is_active(id) {
                        continue;
                    }
                    if let Some(candidate) = by_id.get(&id) {
                        if tx
                            .send_viewport((candidate.clone(), ThumbnailLane::Exception), request)
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Ok(())
            },
            collect,
        )?;
    }

    let mut waiting = std::mem::take(&mut *skipped.lock().unwrap_or_else(|e| e.into_inner()));
    waiting.extend(std::mem::take(
        &mut *deferred.lock().unwrap_or_else(|e| e.into_inner()),
    ));
    wait_for_viewport_results(
        &state,
        epoch,
        &config,
        &by_id,
        waiting,
        &send,
        Some(request),
    )
}

fn run_viewport_video_covers<F>(
    state: Arc<AppState>,
    epoch: u64,
    item_ids: Vec<i64>,
    config: crate::thumbnail::ThumbConfig,
    on_result: F,
    request: Option<&Arc<crate::thumbnail::coordinator::ViewportRequest>>,
) -> Result<()>
where
    F: Fn(ThumbResult) + Sync,
{
    use crate::db::queries::{self as q, ThumbnailLane, ThumbnailTaskKey, ThumbnailTaskKind};
    use crate::thumbnail::coordinator::{unique_id, ImageExecution};

    let fp = config
        .output_fingerprint
        .ok_or_else(|| AppError::Internal("viewport video fingerprint missing".into()))?;
    let run_id = unique_id()?;
    let candidates = {
        let conn = state.db_read_pool.get().map_err(AppError::from)?;
        q::native_video_cover_candidates_for_ids(&conn, &item_ids)?
    };
    let by_id: std::collections::HashMap<_, _> =
        candidates.into_iter().map(|c| (c.item.id, c)).collect();
    let cancel = tokio_util::sync::CancellationToken::new();
    let deferred = std::sync::Mutex::new(Vec::new());
    let skipped = std::sync::Mutex::new(Vec::new());
    let collect = |outcome: ImageExecution| match outcome {
        ImageExecution::Published(result, _) => {
            on_result(result);
        }
        ImageExecution::Unavailable(id) => {
            if let Some(candidate) = by_id.get(&id) {
                on_result(pending_thumbnail_result(candidate));
            }
        }
        ImageExecution::Deferred(id) => {
            deferred.lock().unwrap_or_else(|e| e.into_inner()).push(id);
        }
        ImageExecution::Skipped(id) => {
            skipped.lock().unwrap_or_else(|e| e.into_inner()).push(id);
        }
    };
    state.thumb_coordinator.run_native_video_covers(
        &state,
        epoch,
        &config,
        &cancel,
        |tx| {
            for &id in &item_ids {
                if !state.is_database_epoch_current(epoch) {
                    break;
                }
                if request.is_some_and(|request| !request.is_active(id)) {
                    continue;
                }
                let Some(candidate) = by_id.get(&id) else {
                    on_result(ThumbResult {
                        item_id: id,
                        thumb_status: 0,
                        thumb_path: None,
                        thumbhash: None,
                        source_revision: 0,
                        cache_key: 0,
                    });
                    continue;
                };
                let key = ThumbnailTaskKey {
                    item_id: id,
                    source_revision: candidate.item.source_revision,
                    kind: ThumbnailTaskKind::VideoCover,
                    output_fingerprint: fp.hex(),
                };
                let offered = state.with_database_lifecycle_read(epoch, || -> Result<()> {
                    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    q::enqueue_thumbnail_task(&conn, &key, ThumbnailLane::Heavy, "video", &run_id)?;
                    q::promote_thumbnail_task(&conn, &key, ThumbnailLane::Heavy)?;
                    Ok(())
                });
                if offered.transpose()?.is_none() {
                    break;
                }
                let submitted = if let Some(request) = request {
                    tx.send_viewport((candidate.clone(), ThumbnailLane::ViewportHeavy), request)
                } else {
                    tx.send((candidate.clone(), ThumbnailLane::ViewportHeavy))
                };
                if submitted.is_err() {
                    break;
                }
            }
            Ok(())
        },
        collect,
    )?;

    let retry_ids = std::mem::take(&mut *deferred.lock().unwrap_or_else(|e| e.into_inner()));
    if !retry_ids.is_empty() && state.is_database_epoch_current(epoch) {
        state.thumb_coordinator.run_native_video_covers(
            &state,
            epoch,
            &config,
            &cancel,
            |tx| {
                for id in retry_ids {
                    if request.is_some_and(|request| !request.is_active(id)) {
                        continue;
                    }
                    if let Some(candidate) = by_id.get(&id) {
                        let submitted = if let Some(request) = request {
                            tx.send_viewport(
                                (candidate.clone(), ThumbnailLane::ViewportHeavy),
                                request,
                            )
                        } else {
                            tx.send((candidate.clone(), ThumbnailLane::ViewportHeavy))
                        };
                        if submitted.is_err() {
                            break;
                        }
                    }
                }
                Ok(())
            },
            collect,
        )?;
    }
    let mut waiting = std::mem::take(&mut *skipped.lock().unwrap_or_else(|e| e.into_inner()));
    waiting.extend(std::mem::take(
        &mut *deferred.lock().unwrap_or_else(|e| e.into_inner()),
    ));
    wait_for_viewport_results(
        &state,
        epoch,
        &config,
        &by_id,
        waiting,
        &on_result,
        request.map(Arc::as_ref),
    )
}

fn wait_for_viewport_results<F>(
    state: &AppState,
    epoch: u64,
    config: &crate::thumbnail::ThumbConfig,
    by_id: &std::collections::HashMap<i64, crate::db::queries::ThumbnailCandidate>,
    mut waiting: Vec<i64>,
    on_result: &F,
    request: Option<&crate::thumbnail::coordinator::ViewportRequest>,
) -> Result<()>
where
    F: Fn(ThumbResult),
{
    use crate::db::queries as q;

    let fp = config
        .output_fingerprint
        .ok_or_else(|| AppError::Internal("viewport thumbnail fingerprint missing".into()))?;
    // 同键已由全库或另一视口请求持有时等待其条件提交；整批共用截止时间。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    waiting.sort_unstable();
    waiting.dedup();
    while !waiting.is_empty() && state.is_database_epoch_current(epoch) {
        waiting.retain(|id| {
            by_id.contains_key(id) && request.is_none_or(|request| request.is_active(*id))
        });
        if waiting.is_empty() {
            break;
        }
        let current = state
            .db_read_pool
            .get()
            .ok()
            .and_then(|conn| q::thumbnail_results_for_ids(&conn, &waiting).ok());
        if !state.is_database_epoch_current(epoch) {
            break;
        }
        let mut epoch_lost = false;
        waiting.retain(|id| {
            if request.is_some_and(|request| !request.is_active(*id)) {
                return false;
            }
            let candidate = &by_id[id];
            let Some(item) = current.as_ref().and_then(|rows| rows.get(id)) else {
                return true;
            };
            let expected =
                crate::thumbnail::cache::thumb_variant_db_path(config.size, item.cache_key, fp);
            if item.source_revision != candidate.item.source_revision
                || item.thumb_status == 0
                || (item.thumb_status == 1 && item.thumb_path.as_deref() != Some(expected.as_str()))
            {
                return true;
            }
            if state
                .with_database_lifecycle_read(epoch, || on_result(item.clone()))
                .is_none()
            {
                epoch_lost = true;
                return true;
            }
            false
        });
        if epoch_lost || waiting.is_empty() || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !state.is_database_epoch_current(epoch) {
        return Ok(());
    }
    for id in waiting {
        if request.is_some_and(|request| !request.is_active(id)) {
            continue;
        }
        let Some(candidate) = by_id.get(&id) else {
            continue;
        };
        if state
            .with_database_lifecycle_read(epoch, || {
                on_result(pending_thumbnail_result(candidate));
            })
            .is_none()
        {
            return Ok(());
        }
    }
    Ok(())
}

// FullThumbProgressPayload / full_thumb_gen_status / start_full_thumbnail_generation /
// start_incremental_thumbnail_generation / stop_full_thumbnail_generation /
// run_thumbnail_generation 已迁至 `thumbnail_full_gen.rs`（`pub use` 转发见文件顶部，
// 超长文件拆分方案 tierB-3）。

/// 懒自愈（前端 `MediaThumb` 在 `thumb_status=1` 的封面 404 时按格触发）：DB 声称已生成、但
/// `thumb_path` 文件已被 LRU 缓存驱逐（`enforce_cache_limit` 删文件不改 `thumb_status`，而
/// `route_thumbnail` 对 status=1 短路不重生成 → 永久 404）。本命令把该项复位为待重生成：
/// `media_items` 退回 `thumb_status=0/thumb_path=NULL` + 封面派生行 `2→0`，再发 `db:media_enriched`
/// —— 既让画廊刷新，也让旧派生流水线重跑音频/文档/冷门视频封面。原生视频封面直接加入共享
/// Coordinator，避免派生流水线已在运行时自动启动器跳过唤醒。图像类无封面派生：仅退
/// media_items，滚回视口时主 generator 缺文件 CACHE_MISS 自愈。
///
/// 防御：**先 stat 确认文件确实缺失**才复位——`img.decode()`/DOM `<img>` 可能因瞬时原因触发
/// `@error`，若文件其实健在就复位会造成无谓重生成 churn。仅对 `thumb_status=1` 的项动作。
#[tauri::command]
pub async fn regenerate_missing_thumb(
    app: AppHandle,
    id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let state_arc = state.inner().clone();
    let Some(database_epoch) = state_arc.current_database_epoch() else {
        return Ok(());
    };
    // 读当前状态 + 路径（读池）。
    let row: Option<(i64, Option<String>, i64, i64, String, String)> =
        super::blocking::read_blocking(&state, move |conn| {
            conn.query_row(
                "SELECT thumb_status, thumb_path, source_revision, cache_key, media_type, file_format
             FROM media_items WHERE id = ?1 AND is_deleted = 0",
                [id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(AppError::Db)
        })
        .await?;

    let Some((thumb_status, Some(thumb_path), source_revision, cache_key, media_type, format)) =
        row
    else {
        return Ok(()); // 项不存在 / 已软删 / 无路径 —— 无可自愈。
    };
    // 仅处理「DB 说已生成」的项；status=0/2/3 由既有 pending/失败流程管，勿插手。
    if thumb_status != 1 {
        return Ok(());
    }

    // 防御性存在性校验：文件其实健在（瞬时 @error）→ 不动，避免无谓重生成。
    let cache_dir = {
        state_arc
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone()
    };
    let full = cache_dir.join("thumbnails").join(&thumb_path);
    if full.exists() {
        return Ok(());
    }

    // 复位（写锁）：media_items + 封面派生行一并退回 pending。
    // 复位查询必须遵循 lifecycle → writer 的锁序；不能把 `write_blocking`（先拿 writer）
    // 与生命周期读锁嵌套，否则会和清库的 writer-write 路径形成反向等待。文件 stat 已在
    // 锁外完成，最终 SQL 用完整源快照做 CAS。
    let expected_thumb_path = thumb_path.clone();
    let reset_state = state_arc.clone();
    let healed = tokio::task::spawn_blocking(move || {
        reset_state
            .with_database_lifecycle_read(database_epoch, || {
                let conn = reset_state
                    .db_writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                crate::db::queries::reset_cover_thumb_for_regen_if_current(
                    &conn,
                    id,
                    &expected_thumb_path,
                    source_revision,
                    cache_key,
                )
            })
            .transpose()
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??
    .unwrap_or(0);
    if healed != 1 {
        return Ok(());
    }

    info!(
        "[HealThumb] id={} 封面文件缺失（LRU 驱逐残留）→ 复位待重生成 | reset evicted cover for regen",
        id
    );

    // 失效三件套（对齐 clear_all_thumbnails / config 档位变更）：items 快照是布局行载荷源，
    // 不清则 HIT 路径仍端出旧 status=1 + 已删路径 → 继续 404 且前端不重取。
    state_arc.clear_layout_caches();
    state_arc.bump_data_version();

    // 发 db:media_enriched：① MediaGrid 防抖重算刷新可见行；② useDerivationAutoStart kick
    // 派生流水线领取刚复位的封面派生（视频/音频/文档）重跑。空跑幂等，故不惧被多格并发触发。
    let _ = app.emit("db:media_enriched", MediaEnrichedPayload::refresh_signal());

    if media_type == "video"
        && crate::video::native_cover_formats().contains(&format.as_str())
        && state_arc
            .config
            .get("enable_video_cover")
            .is_none_or(|value| value != "false")
    {
        let mut config = state_arc
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        config.size = crate::thumbnail::generator::snap_to_tier(config.size);
        config.ai_hq_cache = false;
        config.output_fingerprint =
            Some(crate::thumbnail::scheduler::OutputFingerprint::for_native_video_cover(&config));
        let job_state = state_arc.clone();
        tokio::task::spawn_blocking(move || {
            let notify_app = app.clone();
            if let Err(error) = run_viewport_video_covers(
                job_state,
                database_epoch,
                vec![id],
                config,
                move |result| {
                    if result.thumb_status == 1 || result.thumb_status == 2 {
                        let _ = notify_app
                            .emit("db:media_enriched", MediaEnrichedPayload::refresh_signal());
                    }
                },
                None,
            ) {
                tracing::warn!(item_id = id, error = %error, "native cover self-heal failed");
            }
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn clear_all_thumbnails(state: State<'_, Arc<AppState>>) -> Result<()> {
    info!("User action: Clearing all thumbnails | 用户操作：清除所有缩略图");

    // R1-3：全表重置（写锁 SQL）+ 缓存目录递归删除（可达数万文件的重阻塞 IO）一并下沉 blocking。
    // 生命周期写区先使所有已捕获 epoch 失效，防止旧 worker 在清理过程中重新落盘。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        // 与全库 worker 的锁序一致：先拿缩略图代次闸门，再拿数据库生命周期写锁。
        // 这样取消、epoch 失效和缓存目录清理之间没有 start/publish 的反向锁死窗口。
        state_arc.with_thumb_generation_gate(|| {
            state_arc.with_scan_lifecycle_exclusive(|| -> Result<()> {
                state_arc.thumb_gen_token.cancel_keep_generation();

                // 1. 重置数据库 thumb_status。只在此短 DB 段持有 writer 锁。
                {
                    let conn = state_arc
                        .db_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    conn.execute("UPDATE media_items SET thumb_status = 0, thumb_path = NULL, thumbhash = NULL WHERE thumb_status != 0", [])
                        .map_err(AppError::Db)?;
                    // 同步失效 exotic thumbnail 任务（问题1）：删目录已清掉 exotic 产物（与主缩略图同一缓存布局），
                    // 但 done 任务仍 status=2，不重置则永不重做且会被放回主 generator。
                    crate::db::queries::reset_all_exotic_thumbnail_tasks(&conn)?;
                    // 同步退回封面派生行(2026-07-10 审查 B5,同 P1-4 纪律):thumbnails/ 目录内还存放
                    // video_cover/doc_thumb 封面产物,删目录后派生行仍 status=2 → backfill 不再入队、前端
                    // pdf/svg 渲染队列(驱动源=derivations.status)也不重做 → 封面永不重建。
                    // kinds 映射复用 clear_cache 的单一事实源。
                    let (deriv_kinds, _) =
                        crate::thumbnail::cache::derivations_to_reset_for_kind("thumbnails");
                    crate::db::queries::reset_derivations_by_kinds(&conn, deriv_kinds)?;
                    crate::db::queries::reset_image_thumbnail_leases(&conn)?;
                    crate::db::queries::reset_native_video_cover_leases(&conn)?;
                }

                // 2. 删除缓存目录。writer 锁已释放，生命周期写锁仍阻止 worker 重建同一目录。
                let cache_dir = state_arc
                    .thumb_config
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .cache_dir
                    .clone();
                let thumb_dir = cache_dir.join("thumbnails");
                if thumb_dir.exists() {
                    std::fs::remove_dir_all(&thumb_dir).map_err(AppError::Io)?;
                }

                // 失效三件套须齐全(2026-07-06 审查 P1-7,对照 clear_database 正例):items 快照是布局行
                // 载荷源,只清 layout_cache 时 HIT 路径仍按未变的 data_version 端出 thumb_status=1 +
                // 指向已删文件的 thumb_path → 全屏裂图且前端不会重新请求生成。
                state_arc.clear_layout_caches();
                state_arc.bump_data_version();
                Ok(())
            })
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // 4. 唤醒 Coordinator 重做被退回 pending 的 exotic 缩略图。
    state.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);

    Ok(())
}

/// 暂不可用与等待超时沿同一 pending 契约回包，不把临时状态写回媒体展示行。
fn pending_thumbnail_result(candidate: &crate::db::queries::ThumbnailCandidate) -> ThumbResult {
    ThumbResult {
        item_id: candidate.item.id,
        thumb_status: 0,
        thumb_path: None,
        thumbhash: None,
        source_revision: candidate.item.source_revision,
        cache_key: candidate.item.cache_key,
    }
}
