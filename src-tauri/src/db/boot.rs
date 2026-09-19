// src-tauri/src/db/boot.rs
//! 启动期数据库装配:恢复交换 → 写连接 + 结构初始化(含恢复回滚分支)→ 恢复收尾 → 读池,
//! 以及 tracing 就绪后才能跑的启动期自愈四项。
//!
//! 自 `lib.rs::run()` 的 setup 段 c/e/f/g/l 迁出(D-450 纯结构移动,行为不变)。
//!
//! 顺序不变量(拆分方案 §3.1,改动前必读):
//! 1. `perform_swap_at_boot` 必须先于 `create_write_connection`/`create_read_pool`;
//! 2. `initialize_schema` 与 `ConfigManager::load_or_init` 之间已无先后依赖(P24 起配置不再读 DB)。
//! 3. 结构初始化失败 + 恢复场景的回滚分支必须先 `drop(db_writer)` 释放 Windows 文件句柄,
//!    才能物理回滚 old→current,再重新开连接 + 重新初始化——不可简化这个 drop/重开时序;
//! 4. [`run_startup_reconciliation`] 必须在 tracing subscriber 就绪**之后**调用,
//!    否则自愈日志静默丢失;
//! 5. DB 自愈项在同一把写锁内、后台派生流水线拉起前、无并发读者时一次性收敛。
//!
//! 本模块不依赖 `tauri::AppHandle`/dialog:致命失败以 [`StartupFailure`] 上抛,由
//! `run()` 编排层统一走 `fatal_startup_error`(错误文案逐字不变)。

use std::path::{Path, PathBuf};

use tracing::info;

use crate::db::queries::{get_config, set_config};
use crate::db::schema::initialize_schema;
use crate::db::{create_read_pool, create_write_connection, DbPool, DbWriter};
use crate::StartupFailure;

/// [`init`] 的产出:后续 `AppState::new` 与自愈所需的全部 DB 侧句柄。
pub struct DbBoot {
    /// 活库路径(`<app_data_dir>/scrollery.db`)。
    pub db_path: PathBuf,
    pub writer: DbWriter,
    pub read_pool: DbPool,
}

/// 恢复交换 → 写连接 + 结构初始化 → 恢复收尾 → 读池(setup 段 c/e/f/g)。
pub fn init(app_data_dir: &Path) -> Result<DbBoot, StartupFailure> {
    let db_path = app_data_dir.join("scrollery.db");

    // ── 恢复启动交换（方案 B §6.2）─────────────────────────────────────
    // **须在 DB 写连接/读池创建之前**：staged 库/documents 在此原子换入活库位置。
    // 无 pending-restore 标记 → 立即返回、对正常启动零开销。返回 Some(backupId) 表示
    // 已装入待 verify（下方结构初始化成功即视为 verified，收尾清理）。交换失败给可诊断提示：
    // marker 记录已完成相位，重启可幂等续做（不裸 panic）。
    let mut restore_applied = crate::backup::perform_swap_at_boot(app_data_dir)
        .map_err(|e| StartupFailure::new("恢复交换失败 / restore swap failed", e))?;

    // ── 写入连接 + 结构初始化 ─────────────────────────────
    let mut db_writer = create_write_connection(&db_path).map_err(|e| {
        StartupFailure::new(
            "无法打开数据库写入连接 / cannot open DB write connection",
            e,
        )
    })?;

    // 结构初始化换入库(恢复场景下 staged 库已在 restore_stage 校验为当前格式,此处通常为 no-op)。
    let schema_err = {
        let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
        initialize_schema(&conn).err()
    };
    if let Some(e) = schema_err {
        match &restore_applied {
            // ── §#3:换入库结构初始化失败但本次是恢复 → 回滚到原始库 ─────────────────
            // 原实现在此直接 fatal 退出,而 Installed marker 仍在 → 每次启动都换入坏库、
            // 结构初始化再失败、再 fatal,形成**永久 boot-loop**,原始数据(在 restore-old)永不启用。
            // 修复:drop 写连接(释放文件句柄,Windows 占用文件不可删/移)→ 从 old 逆向回滚 →
            // 重开原始库并初始化(原库此前正常运行,应成功)。
            Some(backup_id) => {
                tracing::error!(
                    "恢复的库结构初始化失败,回滚到原始库 | restored db schema initialization failed, rolling back: {e}"
                );
                drop(db_writer);
                crate::backup::rollback_restore_at_boot(app_data_dir, backup_id).map_err(|re| {
                    StartupFailure::new("恢复回滚失败 / restore rollback failed", re)
                })?;
                db_writer = create_write_connection(&db_path).map_err(|e2| {
                    StartupFailure::new(
                        "回滚后无法打开原始库 / cannot open original db after rollback",
                        e2,
                    )
                })?;
                {
                    let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e2) = initialize_schema(&conn) {
                        // 原始库也初始化失败=真正不可恢复(回滚包 restore-rollback 仍保留供人工兜底)。
                        return Err(StartupFailure::new(
                            "回滚后原始库结构初始化失败 / original db schema initialization failed after rollback",
                            format!("{e2}（数据库 / db: {}）", db_path.display()),
                        ));
                    }
                }
                // marker 已在回滚中清除,勿再 finalize;标记恢复未生效。
                restore_applied = None;
                // TODO(§#5 同批后续):经启动事件向前端提示「恢复失败已回滚」,当前先记 error 日志。
            }
            // ── 非恢复场景:结构不兼容 / 初始化失败照旧给可诊断提示(事务化,重启可安全重跑)──────────
            None => {
                return Err(StartupFailure::new(
                    "数据库结构不兼容或初始化失败 / database schema incompatible or initialization failed",
                    format!("{e}（数据库 / db: {}）", db_path.display()),
                ));
            }
        }
    }

    // ── 恢复 verify + 收尾（方案 B §6.2.5）─────────────────────────────
    // 新库已打开 + 结构初始化成功 = 恢复已生效（quick_check 亦在 restore_stage 阶段做过）→
    // 写 Verified 并清理 old/staging/marker（回滚包保留 ≥7 天，UI 手动清理）。
    if let Some(backup_id) = &restore_applied {
        crate::backup::finalize_restore_verified(app_data_dir, backup_id);
        info!("数据恢复完成 | data restore completed: {backup_id}");
    }

    // ── Read pool (desktop) ───────────────────────────────────────
    // 8 connections: the foreground interleaves compute_layout + viewport meta +
    // thumbnail batches while background derivation/AI also read — 4 left those queuing
    // (布局被后台读饿死的次因). WAL makes extra read connections cheap.
    // ── 读取池（桌面端） ─────────────────────────────────────
    // 8 个连接：前台会交错 compute_layout + 可视区元数据 + 缩略图批，同时后台派生/AI 也在读
    // —— 4 个会让它们排队（布局被后台读饿死的次因）。WAL 下额外读连接开销很低。
    let db_read_pool = create_read_pool(&db_path, 8).map_err(|e| {
        StartupFailure::new("无法创建数据库读取连接池 / cannot create DB read pool", e)
    })?;

    Ok(DbBoot {
        db_path,
        writer: db_writer,
        read_pool: db_read_pool,
    })
}

/// 启动期 WAL 截断 + 三项自愈(setup 段 l)。
///
/// **调用时机是硬约束**:tracing subscriber 就绪之后(否则日志静默丢弃)、后台派生管线
/// 拉起之前(无并发读者,TRUNCATE 可截干净、自愈可一次性收敛)。
pub fn run_startup_reconciliation(db_writer: &DbWriter, db_path: &Path, cache_dir: &Path) {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    {
        let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::connection::checkpoint_wal_at_boot(&conn, db_path);
    }

    {
        let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());

        // P1-4 自愈：修复图像调度器竞写残留 —— 把专门流水线（DocThumbRenderer / 视频·音频
        // 封面）已产出、却被误冲成 thumb_status=2/NULL 的封面，从权威派生产物一次性回填。
        // 与上方同一写锁、管线拉起前、无并发读者时收敛；幂等（无分叉则改 0 行、不致命）。
        match crate::db::queries::reconcile_cover_thumbs(&conn) {
            Ok(0) => {}
            Ok(n) => info!(
                "[Startup] 自愈封面缩略图 {} 项（修复调度器竞写残留 P1-4） | reconciled {} cover thumbnails",
                n, n
            ),
            Err(e) => tracing::warn!(
                "[Startup] 封面缩略图自愈失败（不致命） | cover thumbnail reconcile failed: {}",
                e
            ),
        }

        // 视频可播产物在途复位(§V6-11):boot 时进程内无任何真实在途 video_playable job
        // (派活在 host 侧 VideoWorkerService 内存队列,随上次进程退出而灭),故残留的
        // status=1 必是假在途——退回 pending,下次 resolve 按需重触发。与下方 .tmp 清扫同属
        // 启动清账;同此写锁、管线拉起前、无并发读者时一次性收敛,幂等(无残留则改 0 行)。
        match crate::db::queries::reset_in_flight_derivations_for_kind(&conn, "video_playable") {
            Ok(0) => {}
            Ok(n) => info!(
                "[Startup] 视频可播在途任务复位 {} 项（假在途退回 pending） | reset {} in-flight video_playable",
                n, n
            ),
            Err(e) => tracing::warn!(
                "[Startup] 视频可播在途复位失败（不致命） | video_playable in-flight reset failed: {}",
                e
            ),
        }
    }

    // 自愈 ②（反向互补）：LRU 缓存驱逐（enforce_cache_limit）删了封面文件却不改
    // thumb_status，route_thumbnail 对 status=1 短路不重生成 → 被驱逐封面永久 404
    // （视频封面整批旧档 cohort 被驱逐是典型）。把「status=1 但 thumb_path 文件已缺失」
    // 的封面类项复位为待重生成（media_items + 派生行一并退回 pending），交派生流水线
    // 按需重跑。含磁盘 stat，但仅扫封面派生项（数千），startup 可承受；幂等。
    // 频控(深审 defer ⑥):LRU 驱逐已在缓存治理任务里**事件驱动即时复位**
    // (reset_thumbs_by_evicted_paths),本全量 stat 扫描降级为兜底(覆盖手动删文件/
    // 异常退出等旁路),至多 7 天一跑——O(封面数) 磁盘 stat 在 HDD/网络卷上可达秒级,
    // 不宜每次启动都付。单项缺失另有懒 404 自愈(regenerate_missing_thumb)托底。
    const COVER_STAT_RECONCILE_INTERVAL_SECS: u64 = 7 * 24 * 3600;
    let stat_sweep_due = {
        let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
        get_config(&conn, "last_cover_stat_reconcile")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|last| now_secs.saturating_sub(last) >= COVER_STAT_RECONCILE_INTERVAL_SECS)
            .unwrap_or(true)
    };
    if stat_sweep_due {
        // 先在短事务中读出候选路径，再释放 writer mutex 做文件 stat；绝不能把
        // 网络卷/HDD 上的 exists() 放进 SQLite writer 临界区。
        let candidates = {
            let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
            crate::db::queries::list_cover_thumb_paths(&conn)
        };
        let reconciled = match candidates {
            Ok(candidates) => {
                let missing: Vec<i64> = candidates
                    .into_iter()
                    .filter(|(_, thumb_path)| {
                        !cache_dir.join("thumbnails").join(thumb_path).exists()
                    })
                    .map(|(item_id, _)| item_id)
                    .collect();
                let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
                crate::db::queries::reset_cover_thumbs_for_regen(&conn, &missing)
            }
            Err(error) => Err(error),
        };
        match reconciled {
            Ok(0) => {}
            Ok(n) => info!(
                "[Startup] 自愈被驱逐封面 {} 项（修复 LRU 驱逐残留 → 交派生流水线重生成） | reconciled {} evicted cover thumbnails",
                n, n
            ),
            Err(e) => tracing::warn!(
                "[Startup] 被驱逐封面自愈失败（不致命） | evicted cover reconcile failed: {}",
                e
            ),
        }
        let conn = db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "last_cover_stat_reconcile", &now_secs.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    #[test]
    fn startup_reconciliation_releases_writer_between_phases() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("scrollery.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            crate::db::schema::initialize_schema(&conn).unwrap();
        }

        let writer = Mutex::new(Connection::open(&db_path).unwrap());
        run_startup_reconciliation(&writer, &db_path, temp.path());

        let conn = writer.lock().unwrap();
        assert!(get_config(&conn, "last_cover_stat_reconcile")
            .unwrap()
            .is_some());
    }
}
