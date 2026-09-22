//! 用于缩略图生成的 Tauri IPC 命令（§ 6.1 — 缩略图）。

use std::sync::Arc;

use crossbeam_channel::bounded;
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
use crate::thumbnail::{
    decode_media_step, process_deferred_cpu, route_thumbnail, DecodeResult, ThumbnailRoute,
    ThumbnailRouteInput,
};

// 【生成引擎终局(2026-07-10 真机 A/B 裁决)】唯一实现 = 多阶段流水线(原「方案二」):
// 解码(cores×2)/编码(cores)/CPU 兜底(cores/2)各自线程池 + crossbeam channel,IO·GPU·CPU 重叠。
// 历史:曾与「方案一」(Rayon 直线并发,每项全程一个工作线程)以 USE_PIPELINE 开关并存,注释宣称
// 方案一「多核性能最好」但从未实测;2026-07-10 公平化(进度同节流/每 id 恰一结果/Phase2 并行)后
// 用户真机 A/B 定案:**流水线全量生成稳定 7.3s,Rayon 直线 16-20s(慢 2.2~2.7 倍)**——
// 方案一分支与运行时开关(thumb_use_pipeline)一并删除退役(daab834 引入,本提交裁决移除)。
// 若未来要复盘旧实现,见 daab834 之前的 git 历史。

// QoS/线程调度块(thumb_cpu_budget/set_app_foreground/refresh_worker_qos/
// apply_thread_qos 三平台)已下沉 `thumbnail::qos`(U-P4-a):它是线程调度策略而非
// IPC 语义,被批量/全量两条流水线共用,lib.rs 的窗口事件也直接消费。
use crate::thumbnail::qos::{refresh_worker_qos, thumb_cpu_budget};

// 全库生成流水线（run_thumbnail_generation + 进度/开关命令）已拆至 `thumbnail_full_gen.rs`
// （见超长文件拆分方案 tierB-3）；下方 `pub use *` 转发保持 `ipc::thumbnail_commands::X` 外部路径
// 不变 —— `ipc/registry.rs` 的 `generate_handler!` 与 `state.rs` 均按此路径引用，漏转发即编译失败。
// 必须用 glob（非具名列表）：`#[tauri::command]` 宏在同一展开里额外生成 `__cmd__*` 等隐藏兄弟
// 项，具名 `pub use {a, b, c}` 只转发写出的名字、漏转发这些隐藏项会令 `generate_handler!` 报
// "cannot find __cmd__x"。
pub use super::thumbnail_full_gen::*;

/// 批量写入缩略图结果到 db_writer（锁中毒即恢复,审查 R11):生命周期读锁 → writer 锁 →
/// 单事务 → 逐条 update_thumb_result_if_current → commit。生命周期锁必须先于 writer 锁，避免和清库
/// 的反向路径死锁；返回 false 表示 worker epoch 已失效，整批被丢弃。
/// 抽出以消除 6 处逐字复制的「锁+事务+批写」块,并统一锁中毒策略(收敛 R11 的「静默跳过整批」臂——
/// 旧 `if let Ok(conn)` 在毒锁下永久丢写而 UI 仍显示)。
/// 批写尽力而为:update/commit 失败吞掉(下轮生成自愈),与原逐行 `let _ =` 语义一致。
/// 用 let-else 绑定 tx,避免 `if let` 作块尾表达式时 Transaction 临时值晚于 conn 析构的借用冲突。
/// `pub(super)`：两条流水线（本文件的批量视口路径 / `thumbnail_full_gen.rs` 的全库生成路径）
/// 共享的唯一写入助手。
pub(super) fn flush_thumb_results(state: &AppState, epoch: u64, results: &[ThumbResult]) -> bool {
    if results.is_empty() {
        return true;
    }
    state
        .with_database_lifecycle_read(epoch, || {
            let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            let Ok(tx) = conn.transaction() else {
                return false;
            };
            let mut accepted = Vec::new();
            for r in results {
                let applied = crate::db::queries::update_thumb_result_if_current(
                    &tx,
                    r.item_id,
                    r.source_revision,
                    r.cache_key,
                    r.thumb_status,
                    r.thumb_path.as_deref(),
                    r.thumbhash.as_deref(),
                );
                if matches!(applied, Ok(1)) {
                    accepted.push(r.clone());
                }
            }
            if tx.commit().is_err() {
                return false;
            }
            // DB CAS 拒绝的旧结果不能再污染常驻布局缓存；否则 DB 已经保住新源，内存
            // 出口却会在下一次重算前短暂显示旧产物/失败状态。
            state.apply_thumb_results(&accepted);
            true
        })
        .unwrap_or(false)
}

/// 为全库生成流水线写入一批结果，并在同一个缩略图代次临界区内更新内存布局缓存。
///
/// 全库 worker 的收尾可能晚于 stop→restart；代次、取消令牌和数据库 epoch 必须在同一
/// 短状态区内检查，不能只复用面向视口请求的 epoch-only helper。该函数不包住文件 IO，
/// 只持有一个短 DB 事务和内存缓存更新。
pub(super) fn flush_thumb_results_for_generation(
    state: &AppState,
    epoch: u64,
    generation: u64,
    cancel_token: &CancellationToken,
    results: &[ThumbResult],
) -> bool {
    if results.is_empty() {
        return true;
    }
    state.with_thumb_generation_gate(|| {
        if cancel_token.is_cancelled() || !state.thumb_gen_token.is_generation_current(generation) {
            return false;
        }
        state
            .with_database_lifecycle_read(epoch, || {
                let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                let Ok(tx) = conn.transaction() else {
                    return false;
                };
                let mut accepted = Vec::new();
                for r in results {
                    let applied = crate::db::queries::update_thumb_result_if_current(
                        &tx,
                        r.item_id,
                        r.source_revision,
                        r.cache_key,
                        r.thumb_status,
                        r.thumb_path.as_deref(),
                        r.thumbhash.as_deref(),
                    );
                    if matches!(applied, Ok(1)) {
                        accepted.push(r.clone());
                    }
                }
                if tx.commit().is_err() {
                    return false;
                }
                state.apply_thumb_results(&accepted);
                true
            })
            .unwrap_or(false)
    })
}

#[tauri::command]
pub async fn batch_request_thumbnails(
    item_ids: Vec<i64>,
    target_size: Option<u32>,
    on_result: tauri::ipc::Channel<ThumbResult>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 debug 档:视口滚动热路径查询,默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:batch_request_thumbnails");
    let state_arc = state.inner().clone();
    let Some(database_epoch) = state_arc.current_database_epoch() else {
        return Ok(());
    };
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

    // R1-3：批量缓存查询走 read_blocking。
    let ids_for_query = item_ids.clone();
    let (fast_results, route_fmt, route_cache_key) =
        super::blocking::read_blocking(&state, move |conn| {
            // 批量查缓存
            let mut fast_results = std::collections::HashMap::new();
            // id → file_format（R7：缓存查询前置扩列 file_format，供 Router 判定，避免 N+1）。
            let mut route_fmt: std::collections::HashMap<i64, String> =
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
                    "SELECT id, thumb_status, thumb_path, thumbhash, file_format, cache_key, source_revision FROM media_items WHERE id IN ({})",
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
                        ))
                    })
                    .map_err(AppError::Db)?;

                for (r, fmt, cache_key) in rows.flatten() {
                    route_cache_key.insert(r.item_id, cache_key);
                    route_fmt.insert(r.item_id, fmt);
                    if r.thumb_status == 1 || r.thumb_status == 3 || r.thumb_status == 2 {
                        fast_results.insert(r.item_id, r);
                    }
                }
            }
            Ok((fast_results, route_fmt, route_cache_key))
        })
        .await?;
    let mut needs_gen = Vec::new();

    for &id in &item_ids {
        if let Some(r) = fast_results.get(&id) {
            let sent = state_arc
                .with_database_lifecycle_read(database_epoch, || on_result.send(r.clone()).is_ok());
            match sent {
                Some(true) => {}
                Some(false) => {
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
                        let _ = on_result_gate.send(ThumbResult {
                            item_id: *id,
                            thumb_status: 0,
                            thumb_path: None,
                            thumbhash: None,
                            source_revision: 0,
                            cache_key: 0,
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

    info!(
        "batch_request_thumbnails: total={} needs_gen={} | 批量请求缩略图: 总计={} 需要生成={}",
        item_ids.len(),
        needs_gen.len(),
        item_ids.len(),
        needs_gen.len()
    );

    if !needs_gen.is_empty() {
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            // 多阶段流水线(生成引擎终局,2026-07-10 A/B 实测裁决后唯一实现,见文件头注)。
            let (decode_tx, decode_rx) = bounded(1024);
            let (encode_tx, encode_rx) = bounded(1024);
            let (result_tx, result_rx) = bounded(needs_gen.len().max(1024));
            // T12(§3.5.1):deferred CPU 专用通道——CPU 密集回退绝不在 decode worker 上
            // inline 跑(会占住 decode 线程、反让 GPU 提交空等),转投下方独立小池消化。
            let (deferred_tx, deferred_rx) = bounded(1024);

            let needs_gen_clone = needs_gen.clone();
            let state_dispatcher = state_arc.clone();
            let result_tx_dispatch = result_tx.clone();
            let decode_tx_dispatch = decode_tx.clone();
            std::thread::spawn(move || {
                for id in needs_gen_clone {
                    if !state_dispatcher.is_database_epoch_current(database_epoch) {
                        break;
                    }
                    // 加载项；对任何无法加载的项也发一个失败结果，使每个请求 id 都恰好产出一个结果。
                    // 静默跳过会让前端在途计数失衡，「处理中 N 项」指示永久卡住（问题9）。
                    let loaded = state_dispatcher.db_read_pool.get().ok().and_then(|pool| {
                        let item = crate::db::queries::get_media_item(&pool, id).ok()?;
                        let (root_path, rel_path, file_name) =
                            crate::db::queries::get_item_path_info(&pool, id).ok()?;
                        let abs_path_str = crate::utils::path::resolve_media_path(&root_path, &rel_path, &file_name);
                        Some((item, std::path::PathBuf::from(abs_path_str)))
                    });
                    match loaded {
                        Some((item, abs_path)) => {
                            if decode_tx_dispatch.send((item, abs_path)).is_err() {
                                return;
                            }
                        }
                        None => {
                            error!("[batch_thumb] could not load item id={id}; emitting failure result | 无法加载项，发送失败结果");
                            let _ = result_tx_dispatch.send(ThumbResult {
                                item_id: id,
                                thumb_status: 2,
                                thumb_path: None,
                                thumbhash: None,
                                source_revision: 0,
                                cache_key: 0,
                            });
                        }
                    }
                }
            });
            // 关闭原始发送端；调度线程的 clone 结束后，decode workers 才能退出，
            // result_rx 才会收尾，避免批次结果已发完但 invoke 永远不返回。
            drop(decode_tx);

            let config_decode = config.clone();
            // CPU 预算限流(2026-07-13):三池均以 thumb_cpu_budget 为上限,不再按逻辑核数铺满。
            let budget = thumb_cpu_budget();
            let decode_threads = budget;
            for _ in 0..decode_threads {
                let rx = decode_rx.clone();
                let tx = encode_tx.clone();
                let res_tx = result_tx.clone();
                let def_tx = deferred_tx.clone();
                let cfg = config_decode.clone();
                let state_worker = state_arc.clone();
                std::thread::spawn(move || {
                    let mut qos_state: Option<bool> = None;
                    while let Ok((item, abs_path)) = rx.recv() {
                        refresh_worker_qos(&mut qos_state);
                        let decoded = match state_worker.with_database_lifecycle_read(
                            database_epoch,
                            || decode_media_step(&item, &abs_path, &state_worker.engine_arena, &cfg),
                        ) {
                            Some(result) => result,
                            None => break,
                        };
                        match decoded {
                            Ok(DecodeResult::Ready(res)) => {
                                let _ = res_tx.send(res);
                            }
                            Ok(DecodeResult::ToEncode {
                                item_id,
                                source_revision,
                                cache_key,
                                decoded,
                            }) => {
                                let _ = tx.send((item_id, source_revision, cache_key, decoded));
                            }
                            Ok(DecodeResult::DeferredToCpu { item, abs_path }) => {
                                // T12(§3.5.1):不再 inline——CPU 密集回退会占住本 decode worker、
                                // 反让 GPU decode 空等;转投专用 deferred 小池。通道已关(池退出)
                                // 时兜底发失败结果,保持「每 id 恰一结果」不变量(问题9)。
                                if let Err(e) = def_tx.send((item, abs_path)) {
                                    let (item, _abs) = e.into_inner();
                                    error!("Deferred channel closed for id={} | deferred 通道已关", item.id);
                                    let _ = res_tx.send(ThumbResult {
                                        item_id: item.id,
                                        thumb_status: 2,
                                        thumb_path: None,
                                        thumbhash: None,
                                        source_revision: item.source_revision,
                                        cache_key: item.cache_key,
                                    });
                                }
                            }
                            Err(e) => {
                                error!("Decode failed for id={}: {}", item.id, e);
                                let _ = res_tx.send(ThumbResult {
                                    item_id: item.id,
                                    thumb_status: 2,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision: item.source_revision,
                                    cache_key: item.cache_key,
                                });
                            }
                        }
                    }
                });
            }
            drop(encode_tx);
            // 主句柄仅供 decode worker 克隆;此处即弃——decode 阶段全部退出后 deferred 池
            // 随通道关闭收尾(否则其 result_tx 克隆悬活,result_rx 永不结束、invoke 不返回)。
            drop(deferred_tx);

            // T12(§3.5.1)deferred CPU 专用小池:与 decode/encode 阶段解耦。池宽 max(1, budget/2)
            // ——CPU 密集解码本就吃核,池小不损吞吐,却保证 decode 通道永不被 CPU 回退占住。
            let deferred_threads = (budget / 2).max(1);
            let config_deferred = config.clone();
            for _ in 0..deferred_threads {
                let rx = deferred_rx.clone();
                let res_tx = result_tx.clone();
                let cfg = config_deferred.clone();
                let state_worker = state_arc.clone();
                std::thread::spawn(move || {
                    let mut qos_state: Option<bool> = None;
                    while let Ok((item, abs_path)) = rx.recv() {
                        refresh_worker_qos(&mut qos_state);
                        let deferred = match state_worker.with_database_lifecycle_read(
                            database_epoch,
                            || process_deferred_cpu(&item, &abs_path, &state_worker.engine_arena, &cfg),
                        ) {
                            Some(result) => result,
                            None => break,
                        };
                        match deferred {
                            Ok(res) => {
                                let _ = res_tx.send(res);
                            }
                            Err(e) => {
                                error!("Deferred CPU Decode failed for id={}: {}", item.id, e);
                                    let _ = res_tx.send(ThumbResult {
                                        item_id: item.id,
                                        thumb_status: 2,
                                        thumb_path: None,
                                        thumbhash: None,
                                        source_revision: item.source_revision,
                                        cache_key: item.cache_key,
                                    });
                            }
                        }
                    }
                });
            }

            let config_encode = config.clone();
            for _ in 0..budget {
                let rx = encode_rx.clone();
                let tx = result_tx.clone();
                let state_worker = state_arc.clone();
                let cfg = config_encode.clone();
                std::thread::spawn(move || {
                    let mut qos_state: Option<bool> = None;
                    while let Ok((item_id, source_revision, cache_key, decoded)) = rx.recv() {
                        refresh_worker_qos(&mut qos_state);
                        let encoded = match state_worker.with_database_lifecycle_read(
                            database_epoch,
                            || crate::thumbnail::encode_media_step_with_snapshot(
                                item_id,
                                source_revision,
                                cache_key,
                                decoded,
                                &cfg,
                            ),
                        ) {
                            Some(result) => result,
                            None => break,
                        };
                        match encoded {
                            Ok(res) => { let _ = tx.send(res); }
                            Err(e) => {
                                error!("Encode failed for id={}: {}", item_id, e);
                                let _ = tx.send(ThumbResult {
                                    item_id,
                                    thumb_status: 2,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision,
                                    cache_key,
                                });
                            }
                        }
                    }
                });
            }
            drop(result_tx);

            let mut results = Vec::new();
            while let Ok(res) = result_rx.recv() {
                let sent = state_arc.with_database_lifecycle_read(database_epoch, || {
                    if on_result.send(res.clone()).is_err() {
                        tracing::debug!("Channel disconnected, ignoring thumb result send");
                    }
                    results.push(res);
                });
                if sent.is_none() {
                    break;
                }
            }

            if !results.is_empty() {
                // flush helper 只把通过 source_revision + cache_key CAS 的结果同步到
                // layout_cache；DB 拒绝的旧结果不能在这里再次无条件 patch 回去。
                let _ = flush_thumb_results(&state_arc, database_epoch, &results);
            }

            info!("batch_request_thumbnails: finished pipeline | 批量请求生成完成 (Pipeline Scheme 2)");
        })
        .await
        .map_err(|e| AppError::Io(e.into()))?;
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
