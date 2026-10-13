//! 用于缩略图生成的 Tauri IPC 命令（§ 6.1 — 缩略图）。

use std::sync::Arc;

use rusqlite::OptionalExtension;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::db::models::ThumbResult;
use crate::error::{AppError, Result};
use crate::exotic::{ExoticHost, ExoticTaskStatus};
use crate::scanner::enricher::MediaEnrichedPayload;
use crate::state::AppState;
use crate::thumbnail::generator::snap_to_tier;
use crate::thumbnail::service::ImageSubscription;
use crate::thumbnail::viewport::{pending_result, ViewportRequest, ViewportThumbResult};
use crate::thumbnail::{route_thumbnail, ThumbnailRoute, ThumbnailRouteInput};

// 图片计算统一走共享常驻服务 `thumbnail::service`(同源同配置去重、订阅级取消、常驻解码/编码
// 线程)。本文件的批量视口入口与 `thumbnail_full_gen.rs` 的全库入口都只做订阅编排:加载候选、
// 提交请求、按完成顺序收集,不再各自搭建 decode/encode 线程池。

// 全库生成流水线（run_thumbnail_generation + 进度/开关命令）已拆至 `thumbnail_full_gen.rs`
// （见超长文件拆分方案 tierB-3）；下方 `pub use *` 转发保持 `ipc::thumbnail_commands::X` 外部路径
// 不变 —— `ipc/registry.rs` 的 `generate_handler!` 与 `state.rs` 均按此路径引用，漏转发即编译失败。
// 必须用 glob（非具名列表）：`#[tauri::command]` 宏在同一展开里额外生成 `__cmd__*` 等隐藏兄弟
// 项，具名 `pub use {a, b, c}` 只转发写出的名字、漏转发这些隐藏项会令 `generate_handler!` 报
// "cannot find __cmd__x"。
pub use super::thumbnail_full_gen::*;

/// 后台取消及轮次换代与统一发布出口共用同一闸门。
pub(super) fn flush_thumb_results_for_generation(
    state: &AppState,
    epoch: u64,
    generation: u64,
    cancel_token: &CancellationToken,
    subscriptions: &[ImageSubscription],
) -> Option<Vec<crate::thumbnail::benchmark::Publication>> {
    state.with_thumb_generation_gate(|| {
        if cancel_token.is_cancelled() || !state.thumb_gen_token.is_generation_current(generation) {
            return None;
        }
        match crate::thumbnail::publication::publish_batch(state, epoch, subscriptions, |_| {}) {
            Ok(published) => published,
            Err(error) => {
                error!(%error, "Image publication failed; no results were announced");
                None
            }
        }
    })
}
/// 先登记可取消身份并确认受理；图片结果通过 Channel 独立返回。
#[tauri::command]
pub async fn batch_request_thumbnails(
    item_ids: Vec<i64>,
    target_size: Option<u32>,
    request_id: String,
    on_result: tauri::ipc::Channel<ViewportThumbResult>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = state.inner().clone();
    let epoch = state.current_database_epoch();
    let request = state.viewport_requests.register(request_id, &item_ids)?;
    tokio::task::spawn_blocking(move || {
        let deliver = |result: ViewportThumbResult| {
            request.deliver(result.result.item_id, || on_result.send(result).is_ok())
        };
        let result = match epoch {
            Some(epoch) => {
                run_viewport_request(&state, epoch, item_ids, target_size, &request, deliver)
            }
            None => Err(AppError::Cancelled),
        };
        if let Err(error) = &result {
            tracing::warn!(%error, "viewport thumbnail request failed");
        }
        // 确定错误原样分类；库换代结束本请求；仅同库发布时源/配置已过期的剩余项可重试。
        for id in request.remaining() {
            let terminal = match &result {
                Err(error) => ViewportThumbResult::failed(id, error),
                Ok(()) if epoch.is_some_and(|epoch| state.is_database_epoch_current(epoch)) => {
                    ViewportThumbResult::retry(id)
                }
                Ok(()) => ViewportThumbResult::failed(id, &AppError::Cancelled),
            };
            if !deliver(terminal) {
                break;
            }
        }
    });
    Ok(())
}

/// 按本批身份撤销订阅，后台全库与其它窗口的消费者保持独立。
#[tauri::command]
pub fn cancel_viewport_thumbnail_request(
    request_id: String,
    item_ids: Vec<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    state.viewport_requests.cancel(&request_id, &item_ids);
    Ok(())
}

fn run_viewport_request(
    state_arc: &Arc<AppState>,
    database_epoch: u64,
    item_ids: Vec<i64>,
    target_size: Option<u32>,
    request: &ViewportRequest,
    mut deliver: impl FnMut(ViewportThumbResult) -> bool,
) -> Result<()> {
    let _span = crate::logging::SpanTimer::debug("ipc:batch_request_thumbnails");
    if request.cancellation.is_cancelled() || !state_arc.is_database_epoch_current(database_epoch) {
        return Ok(());
    }
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
    // 归一到有效档位:本路径提交图片执行服务请求(编码出口不吸附档位),
    // config.size 可能来自前端 target_size 或库中旧值(如历史默认 480),非档位值会令
    // thumb_path 断言失败/写入无效档位目录。幂等——已是档位则不变。
    config.size = snap_to_tier(config.size);

    // 当前已在阻塞线程，读连接仅覆盖批量查询。
    let ids_for_query = item_ids.clone();
    let (fast_results, route_fmt, route_cache_key) = (|| -> Result<_> {
        let conn = state_arc.db_read_pool.get()?;
        // 批量查缓存
        let mut fast_results = std::collections::HashMap::new();
        // id → file_format（R7：缓存查询前置扩列 file_format，供 Router 判定，避免 N+1）。
        let mut route_fmt: std::collections::HashMap<i64, String> =
            std::collections::HashMap::new();
        // id → cache_key（问题4：done 任务重算期望指纹需要，避免 Router 内回查）。
        let mut route_cache_key: std::collections::HashMap<i64, i64> =
            std::collections::HashMap::new();
        for ids in ids_for_query.chunks(256) {
            let ordinary: std::collections::HashSet<_> =
                crate::db::queries::get_image_sources_batch(&conn, ids)?
                    .into_iter()
                    .map(|source| source.item.id)
                    .collect();
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

            if !placeholders.is_empty() {
                let sql = format!(
                    "SELECT id, thumb_status, thumb_path, thumbhash, file_format, cache_key, source_revision FROM media_items WHERE id IN ({})",
                    placeholders
                );
                let mut stmt = conn.prepare(&sql).map_err(AppError::Db)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(ids), |row| {
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
                        ))
                    })
                    .map_err(AppError::Db)?;

                for (r, fmt, cache_key) in rows.collect::<rusqlite::Result<Vec<_>>>()? {
                    route_cache_key.insert(r.item_id, cache_key);
                    route_fmt.insert(r.item_id, fmt);
                    if !ordinary.contains(&r.item_id) && matches!(r.thumb_status, 1 | 2 | 3) {
                        fast_results.insert(r.item_id, r);
                    }
                }
            }
        }
        Ok((fast_results, route_fmt, route_cache_key))
    })()?;
    let mut needs_gen = Vec::new();

    for &id in &item_ids {
        if let Some(r) = fast_results.get(&id) {
            if state_arc.with_database_lifecycle_read(database_epoch, || deliver(r.clone().into()))
                != Some(true)
            {
                return Ok(());
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
    // 让路项绝不进主图片生成器，也绝不写 thumb_status=2。
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
            // 计算沿当前 blocking 线程执行；闭包返回过滤后的 needs_gen（kept）。
            let state_gate = state_arc.clone();
            needs_gen = (|| -> Result<Vec<i64>> {
                // 路由仅需 catalog 认领 + 任务态（route_thumbnail 不读 availability）——未授权/未安装的 PSD
                // 同样不能进主 generator（主解码必失败）。故用 stub Host（无 DB/keyring），避免每项安装/授权
                // 查询造成 N+1（R7）；真实安装/授权真相由 get_exotic_item_state 等命令按需读取。
                let host = ExoticHost::new(state_gate.exotic_catalog.clone());
                let task_map = {
                    let conn = state_gate.db_read_pool.get().map_err(AppError::from)?;
                    crate::db::queries::exotic_thumbnail_route_info_for_items(&conn, &needs_gen)?
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
                    let fmt = route_fmt.get(&id).map(|s| s.as_str()).unwrap_or("");
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
                            invalidate_stale_exotic_tasks(&conn, &stale)
                        });
                    let Some(invalidated) = invalidated.transpose()? else {
                        return Ok(Vec::new());
                    };
                    info!(
                        "batch_request_thumbnails: {} 条 exotic 任务已退回 pending 重做",
                        invalidated
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
                        if state_gate.with_database_lifecycle_read(database_epoch, || {
                            deliver(pending_result(*id).into())
                        }) != Some(true)
                        {
                            return Ok(Vec::new());
                        }
                    }
                }
                // 合并发一次 wake：让 Coordinator 领取 pending（含刚失效的 stale）。
                if !gated.is_empty() || !stale.is_empty() {
                    state_gate.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
                }
                Ok(kept)
            })()?;
        }
    }

    if !needs_gen.is_empty() && !request.cancellation.is_cancelled() {
        crate::thumbnail::viewport_images::run_images(
            state_arc,
            database_epoch,
            &needs_gen,
            &config,
            request,
            |result| deliver(result.into()),
        )?;
    }
    Ok(())
}

fn invalidate_stale_exotic_tasks(conn: &rusqlite::Connection, ids: &[i64]) -> Result<usize> {
    let mut affected = 0;
    for &id in ids {
        affected += crate::db::queries::invalidate_exotic_tasks_for_item(conn, id)?;
    }
    Ok(affected)
}

#[cfg(test)]
mod viewport_error_tests {
    use super::*;

    fn route_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE exotic_tasks (
            item_id INTEGER, capability TEXT, status INTEGER, input_fingerprint TEXT,
            worker_version TEXT, attempts INTEGER, next_retry_at INTEGER, claimed_at INTEGER,
            lease_owner TEXT, last_error_code TEXT, last_error_message TEXT, output_path TEXT,
            updated_at INTEGER);
            INSERT INTO exotic_tasks (item_id, capability, status) VALUES (1, 'thumbnail', 2), (2, 'thumbnail', 2);")
            .unwrap();
        conn
    }

    #[test]
    fn route_query_failure_is_not_an_empty_task_map() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(matches!(
            crate::db::queries::exotic_thumbnail_route_info_for_items(&conn, &[1]),
            Err(AppError::Db(_))
        ));
    }

    #[test]
    fn route_row_decode_failure_is_not_a_missing_task() {
        let conn = route_db();
        conn.execute(
            "UPDATE exotic_tasks SET status='broken' WHERE item_id=1",
            [],
        )
        .unwrap();
        assert!(matches!(
            crate::db::queries::exotic_thumbnail_route_info_for_items(&conn, &[1]),
            Err(AppError::Db(_))
        ));
    }

    #[test]
    fn invalidation_failure_is_reported_and_retry_counts_actual_rows() {
        let conn = route_db();
        conn.execute_batch(
            "CREATE TRIGGER reject_reset BEFORE UPDATE ON exotic_tasks WHEN OLD.item_id=2
            BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
        )
        .unwrap();
        assert!(matches!(
            invalidate_stale_exotic_tasks(&conn, &[1, 2]),
            Err(AppError::Db(_))
        ));
        let status: i64 = conn
            .query_row(
                "SELECT status FROM exotic_tasks WHERE item_id=2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, 2);
        conn.execute_batch("DROP TRIGGER reject_reset").unwrap();
        assert_eq!(
            invalidate_stale_exotic_tasks(&conn, &[1, 2, 999]).unwrap(),
            2
        );
    }
}

/// 懒自愈（前端 `MediaThumb` 在 `thumb_status=1` 的封面 404 时按格触发）：DB 声称已生成、但
/// `thumb_path` 文件已被 LRU 缓存驱逐（`enforce_cache_limit` 删文件不改 `thumb_status`，而
/// `route_thumbnail` 对 status=1 短路不重生成 → 永久 404）。本命令把该项复位为待重生成：
/// `media_items` 退回 `thumb_status=0/thumb_path=NULL` + 封面派生行 `2→0`，再发 `db:media_enriched`
/// —— 既让画廊刷新，又经 `useDerivationAutoStart` kick 派生流水线重跑封面（run_cover 重写文件并
/// 回填 status=1）。图像类无封面派生：仅退 media_items，滚回视口时主 generator 缺文件 CACHE_MISS
/// 自愈。
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
    let row: Option<(i64, Option<String>, i64, i64)> =
        super::blocking::read_blocking(&state, move |conn| {
            conn.query_row(
                "SELECT thumb_status, thumb_path, source_revision, cache_key
             FROM media_items WHERE id = ?1 AND is_deleted = 0",
                [id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(AppError::Db)
        })
        .await?;

    let Some((thumb_status, Some(thumb_path), source_revision, cache_key)) = row else {
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
                let affected = crate::db::queries::reset_cover_thumb_for_regen_if_current(
                    &conn,
                    id,
                    &expected_thumb_path,
                    source_revision,
                    cache_key,
                )?;
                if affected == 1 {
                    conn.execute(
                        "DELETE FROM image_artifacts WHERE item_id=?1 AND source_revision=?2 AND cache_key=?3 AND kind='display' AND path=?4",
                        rusqlite::params![id, source_revision, cache_key, expected_thumb_path],
                    )?;
                }
                Ok::<_, AppError>(affected)
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
                    conn.execute("DELETE FROM image_artifacts", [])?;
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
