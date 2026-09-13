// src-tauri/src/tasks.rs
//! 启动期后台任务聚合装配(自 `lib.rs::run()` 的 setup 段 r 迁出,D-450 纯结构移动,
//! 行为不变):5 个常驻任务 + 句柄池。
//!
//! 顺序不变量(拆分方案 §3.1):
//! - 必须在 `app.manage(app_state)` **之后**调用(部分任务持 `AppState` clone);
//! - `handles_pool` 创建后必须**立即** `app.manage`——`RunEvent::ExitRequested`/`Exit`
//!   靠 `try_state` 取出它 abort + join,漏掉 manage 即失去优雅停止。

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::state::AppState;

/// 后台任务句柄池:`lifecycle::on_run_event` 退出分支据此 abort + join。
pub type HandlesPool = Arc<std::sync::Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>>;

/// 拉起全部启动期后台任务,并把句柄池交给 Tauri 托管。
pub fn spawn_all(
    app: &tauri::App,
    state: &Arc<AppState>,
    log_ring: Arc<crate::logging::LogRingBuffer>,
    cache_dir: PathBuf,
    thumb_cache_max_mb: u64,
) {
    let app_state_for_task = state.clone();
    let app_state_for_volwatch = state.clone();
    let db_pool_for_gc = state.db_read_pool.clone(); // 缓存治理周期任务(下方 h2)用
    let app_state_for_gc = state.clone(); // 同上:驱逐即时复位需写连接 + items 缓存
    let app_state_for_rank = state.clone(); // B-file-iii:开机后台预建全局 filename rank

    let handles_pool: HandlesPool = Arc::new(std::sync::Mutex::new(Vec::new()));
    app.manage(handles_pool.clone());

    // UI 环形缓冲抽干任务(方案 §5/§9.3-D,S4):~100ms 一批,空批跳过 emit(无订阅者时
    // 缓冲恒空,故该任务空转的开销只是一次 Mutex lock+len 检查,近零成本)。app 级广播
    // (`.emit`,同 lib.rs 其余进度事件的既有惯例)——只有日志窗口会监听该事件名,其余
    // 窗口忽略无成本。
    let log_ring_for_drain = log_ring;
    let app_handle_for_log_drain = app.handle().clone();
    let h_log_drain = tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            ticker.tick().await;
            if let Some(batch) = log_ring_for_drain.drain() {
                let _ = app_handle_for_log_drain.emit("log:batch", batch);
            }
        }
    });
    handles_pool.lock().unwrap().push(h_log_drain);

    // 卷插拔监听（Part2 T2 / C5 Piece B）：冷启动延迟后每 15s 对账已知卷在线态，
    // 实时维护 availability(online↔offline)，拔盘/插回 ≤15s 反映到画廊。
    let volwatch_handle =
        crate::scanner::volume_watch::spawn(app.handle().clone(), app_state_for_volwatch);
    handles_pool.lock().unwrap().push(volwatch_handle);

    // B-file-iii：开机后台预建全局 filter-invariant filename rank。延迟 15s 避让冷启动首帧
    // IPC 高峰（构建持一个读池连接 ~546ms release / 数秒 dev，避免与首帧读争抢），此后用户
    // 首次切 filename 序时 rank 已就绪——免每 filter 一次全表 NATURAL_CMP 查询。构建本身
    // 已下沉 spawn_blocking 且幂等（见 spawn_global_filename_rank_build）；Stage 3 的惰性触发
    // 作兜底（若用户在 15s 内即用 filename 序，compute_layout 会先补一次构建）。
    let h_rank = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        app_state_for_rank.spawn_global_filename_rank_build();
    });
    handles_pool.lock().unwrap().push(h_rank);

    let h1 = tauri::async_runtime::spawn(async move {
        // Delay first run by 3 minutes so it doesn't block cold start | 延迟 3 分钟执行，避免影响冷启动
        tokio::time::sleep(std::time::Duration::from_secs(3 * 60)).await;
        loop {
            tracing::info!("Running PRAGMA optimize for database | 正在执行数据库碎片优化");
            // rusqlite 下沉 spawn_blocking(2026-07-10 审查 B3,CLAUDE.md 硬化条款零豁免):
            // PRAGMA optimize 是同步 SQL 且写锁可能被扫描批持有数秒,直跑 async 块会
            // 同步占住 tokio worker。锁中毒即恢复(R11);guard 生命周期整个在闭包内,不跨 await。
            let state_for_optimize = app_state_for_task.clone();
            let res = tokio::task::spawn_blocking(move || {
                let conn = state_for_optimize
                    .db_writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                conn.execute_batch("PRAGMA optimize;")
            })
            .await;
            match res {
                Ok(Err(e)) => tracing::warn!(
                    "Failed to run PRAGMA optimize | 执行数据库碎片优化失败: {}",
                    e
                ),
                Err(e) => {
                    tracing::warn!("PRAGMA optimize task join failed | 碎片优化任务异常: {}", e)
                }
                Ok(Ok(())) => {}
            }
            // Run every 24 hours | 每 24 小时执行一次
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    });
    handles_pool.lock().unwrap().push(h1);

    let cache_dir_clone = cache_dir;
    // 缓存治理周期任务(Part3-T6 / §3.3):对账 GC(删 DB 无主孤儿)→ LRU 上限收敛。
    // 节奏:启动后 5 分钟首跑(错开上方 3 分钟的 PRAGMA optimize,避开冷启动高峰),
    // 此后每 24h 一轮;「全量重建完成后即时触发」为 T6 余项,暂未接事件。
    // epoch 护栏:GC 只删早于进程启动时刻的文件——本会话新写的产物可能尚未落
    // DB 行(写文件与写行之间有窗口),留到下次会话收敛,见 reconcile_orphan_gc。
    let gc_epoch = std::time::SystemTime::now();
    let app_handle_for_gc = app.handle().clone();
    let h2 = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5 * 60)).await;
        loop {
            let pool = db_pool_for_gc.clone();
            let cache_dir = cache_dir_clone.clone();
            let state_gc = app_state_for_gc.clone();
            let handle_gc = app_handle_for_gc.clone();
            let joined = tauri::async_runtime::spawn_blocking(move || {
                // 取 key 全集后连接随作用域即还池,不跨磁盘扫描持有池连接。
                let live_keys = match pool.get() {
                    Ok(conn) => match crate::db::queries::all_cache_keys(&conn) {
                        Ok(keys) => keys,
                        Err(e) => {
                            tracing::warn!("对账 GC 读取 cache_key 全集失败,本轮跳过 | orphan GC key query failed: {e}");
                            return;
                        }
                    },
                    Err(e) => {
                        tracing::warn!("对账 GC 获取读连接失败,本轮跳过 | orphan GC pool get failed: {e}");
                        return;
                    }
                };
                crate::thumbnail::cache::reconcile_orphan_gc(&cache_dir, &live_keys, gc_epoch);
                let evicted =
                    crate::thumbnail::cache::enforce_cache_limit(&cache_dir, thumb_cache_max_mb);
                // 事件驱动复位(深审 defer ⑥,根治 d503843 病灶):驱逐者自己知道删了哪些
                // 文件,当场把受影响行(media_items + 封面派生行)退回 pending——启动期
                // 全量 stat 扫描自此降级为 7 天一跑的兜底。同步 patch 常驻 items 缓存
                // (两层一致纪律)并广播 media_enriched,前端 useDerivationAutoStart /
                // DocThumbRenderer 据此接续重生成,可视区图像项走懒加载按需重生成。
                if !evicted.is_empty() {
                    let reset = {
                        let conn = state_gc.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        crate::db::queries::reset_thumbs_by_evicted_paths(&conn, &evicted)
                    };
                    match reset {
                        Ok(ids) if !ids.is_empty() => {
                            let patches: Vec<crate::db::models::ThumbResult> = ids
                                .iter()
                                .map(|&item_id| crate::db::models::ThumbResult {
                                    item_id,
                                    thumb_status: 0,
                                    thumb_path: None,
                                    thumbhash: None,
                                    source_revision: 0,
                                    cache_key: 0,
                                })
                                .collect();
                            state_gc.apply_thumb_results(&patches);
                            let _ = handle_gc.emit(
                                "db:media_enriched",
                                crate::scanner::enricher::MediaEnrichedPayload::refresh_signal(),
                            );
                            tracing::info!(
                                "LRU 驱逐即时复位 {} 项待重生成 | reset {} items after LRU eviction",
                                ids.len(),
                                ids.len()
                            );
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!(
                            "LRU 驱逐复位失败(启动期兜底扫描会收敛) | evicted-path reset failed: {e}"
                        ),
                    }
                }
            })
            .await;
            if let Err(e) = joined {
                tracing::warn!("缓存治理任务 join 失败 | cache governance task join failed: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    });
    handles_pool.lock().unwrap().push(h2);
}
