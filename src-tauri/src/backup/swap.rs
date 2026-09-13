//! 恢复的启动交换状态机(方案 B §6.2)。恢复不在活进程内拆写连接/池,而是:
//! **validate→stage(§6.1)→arm(写 pre-restore 回滚包 + pending-restore.json)→restart→swap(本模块,
//! 启动期、DB 池创建之前)→verify/rollback**。
//!
//! 崩溃安全靠 **phase marker + 实际文件存在性双判**(非「固定顺序大概成功」):每步先移动文件、
//! 再原子更新 marker;重入时按相位 + 文件是否就位决定「继续前进」或「逆向从 old 恢复现场」。
//! 每个移动幂等(目标已存在即跳过),任意点崩溃后重跑收敛到自洽终态。
//! **无 marker 时立即返回,对正常启动零开销。**

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::core::{run_backup, BackupParams};
use super::manifest::BackupKind;
use super::restore::CODE_IO;
use crate::error::{AppError, Result};

/// 预恢复回滚包生成失败(方案 §9)。
pub const CODE_ROLLBACK_FAILED: &str = "restore_rollback_failed";
/// marker 损坏且无法安全恢复现场(须人工介入)。
pub const CODE_MARKER_CORRUPT: &str = "restore_marker_corrupt";

fn err(code: &'static str, msg: &str) -> AppError {
    AppError::Restore {
        code,
        message: msg.to_string(),
    }
}
fn io_err(_e: std::io::Error) -> AppError {
    err(CODE_IO, "恢复交换读写失败 | restore swap io failed")
}

/// 交换相位(pending-restore.json 持久化)。
/// - `Prepared`:回滚包已成、marker 已写,**尚未移动任何活库文件**。
/// - `CurrentMoved`:当前活库(db/-wal/-shm/documents)已移入 old 目录。
/// - `Installed`:暂存库/documents 已装入活库位置(交换完成,待 verify)。
/// - `Verified`:新库已打开/迁移/就绪,可清理 old/staging/marker。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestorePhase {
    Prepared,
    CurrentMoved,
    Installed,
    Verified,
}

/// pending-restore.json:恢复交换的持久状态(方案 §6.2.2)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRestore {
    /// 被恢复包的 backupId(定位 restore-staging/{id} 与 restore-old/{id})。
    pub backup_id: String,
    /// 暂存目录(§6.1 解压落点)。
    pub staging_dir: String,
    /// 预恢复回滚包路径(§6.2.1;失败前绝不 arm)。至少保留 7 天,UI 手动清理。
    pub rollback_path: String,
    pub phase: RestorePhase,
}

fn marker_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("pending-restore.json")
}
fn old_dir(app_data_dir: &Path, backup_id: &str) -> PathBuf {
    app_data_dir.join("restore-old").join(backup_id)
}
fn staging_db(staging: &Path) -> PathBuf {
    staging.join("db").join("scrollery.db")
}
fn live_db(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("scrollery.db")
}

/// 活库随身文件(单文件 DB + WAL/SHM 边车 + documents 目录)。
const LIVE_DB_FILES: [&str; 3] = ["scrollery.db", "scrollery.db-wal", "scrollery.db-shm"];

/// 读 marker(不存在返回 None)。存在但无法解析 → `restore_marker_corrupt`(须人工介入,
/// 而非静默忽略——文件可能半移动)。
fn read_marker(marker: &Path) -> Result<Option<PendingRestore>> {
    match std::fs::read_to_string(marker) {
        Ok(s) => match serde_json::from_str::<PendingRestore>(&s) {
            Ok(p) => Ok(Some(p)),
            Err(_) => Err(err(
                CODE_MARKER_CORRUPT,
                "pending-restore 标记损坏,请人工检查",
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(e)),
    }
}

/// 原子写 marker(tmp → 同卷 rename,复用 write_atomic)。
fn write_marker(marker: &Path, pending: &PendingRestore) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(pending)
        .map_err(|_| err(CODE_IO, "pending-restore 标记序列化失败"))?;
    crate::thumbnail::generator::write_atomic(marker, &bytes).map_err(io_err)
}

fn move_path(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    std::fs::rename(from, to).map_err(io_err)
}

/// 当前活库文件 → old 目录(幂等:src 已不在即跳过)。
fn move_current_to_old(app_data_dir: &Path, old: &Path) -> Result<()> {
    std::fs::create_dir_all(old).map_err(io_err)?;
    for name in LIVE_DB_FILES {
        let src = app_data_dir.join(name);
        if src.exists() {
            move_path(&src, &old.join(name))?;
        }
    }
    let docs = app_data_dir.join("documents");
    let dst = old.join("documents");
    if docs.exists() && !dst.exists() {
        move_path(&docs, &dst)?;
    }
    Ok(())
}

/// 逆向:old → 活库位置(幂等)。用于「当前已移走但暂存不可用」时恢复原始现场。
fn restore_current_from_old(app_data_dir: &Path, old: &Path) -> Result<()> {
    for name in LIVE_DB_FILES {
        let src = old.join(name);
        let dst = app_data_dir.join(name);
        if src.exists() && !dst.exists() {
            move_path(&src, &dst)?;
        }
    }
    let odocs = old.join("documents");
    let adocs = app_data_dir.join("documents");
    if odocs.exists() && !adocs.exists() {
        move_path(&odocs, &adocs)?;
    }
    Ok(())
}

/// 暂存库/documents → 活库位置(幂等:目标已存在即跳过)。暂存 DB 是 VACUUM 出的单文件,
/// 装入后无 WAL/SHM(新库首开即建)。
fn install_staging_to_live(app_data_dir: &Path, staging: &Path) -> Result<()> {
    let sdb = staging_db(staging);
    let live = live_db(app_data_dir);
    if sdb.exists() && !live.exists() {
        move_path(&sdb, &live)?;
    }
    let sdocs = staging.join("documents");
    let adocs = app_data_dir.join("documents");
    if sdocs.exists() && !adocs.exists() {
        move_path(&sdocs, &adocs)?;
    }
    Ok(())
}

/// 启动期恢复交换(方案 §6.2.3-4)。**须在 DB 写连接/读池创建之前调用**(setup 内 db_path 解析后)。
/// 无 pending marker → 立即 `Ok(None)`(正常启动零开销)。有则按相位 + 文件存在性幂等推进或逆向:
/// 返回 `Ok(Some(backup_id))` 表示已装入待 verify(调用方在迁移/就绪后调 [`finalize_restore_verified`]);
/// `Ok(None)` 表示无恢复或已逆向恢复原始现场(正常启动)。
pub fn perform_swap_at_boot(app_data_dir: &Path) -> Result<Option<String>> {
    let marker = marker_path(app_data_dir);
    let Some(mut pending) = read_marker(&marker)? else {
        return Ok(None);
    };
    let staging = PathBuf::from(&pending.staging_dir);
    let old = old_dir(app_data_dir, &pending.backup_id);

    match pending.phase {
        RestorePhase::Prepared => {
            // 暂存不可用(被删/损坏)→ 中止恢复。**须先确保原始现场在位**:虽然 Prepared 名义上
            // 「尚未移动活库文件」,但崩溃可能发生在 move_current_to_old 之后、写 CurrentMoved marker
            // 之前——此时活库已移入 old、活库位置为空。若不逆向,中止后活库缺失、app 建全新空库、
            // 真实数据搁浅 old(§#6)。restore_current_from_old 幂等:未移动过则为 no-op。
            if !staging_db(&staging).exists() {
                restore_current_from_old(app_data_dir, &old)?;
                let _ = std::fs::remove_file(&marker);
                tracing::warn!(
                    "恢复中止:暂存库缺失,已确保原始数据在位 | restore aborted: staging missing, original ensured in place"
                );
                return Ok(None);
            }
            move_current_to_old(app_data_dir, &old)?;
            pending.phase = RestorePhase::CurrentMoved;
            write_marker(&marker, &pending)?;
            install_staging_to_live(app_data_dir, &staging)?;
            pending.phase = RestorePhase::Installed;
            write_marker(&marker, &pending)?;
            Ok(Some(pending.backup_id))
        }
        RestorePhase::CurrentMoved => {
            if live_db(app_data_dir).exists() || staging_db(&staging).exists() {
                // 活库已在位或暂存库仍在 → 幂等补装再标 Installed。
                // 关键(§#2):即使 live_db 已存在(崩溃发生在「db 已移入、documents 未移入」之间),
                // 也必须**无条件**再跑一次 install_staging_to_live——它对已就位的 db 跳过、只补移
                // 尚在 staging 的 documents。原实现在此仅补写相位而不 install,会把 documents 永远
                // 留在 staging,随后 finalize_restore_verified 连同 old 一并 remove_dir_all,导致
                // document_versions 行指向不存在的文件(永久文档丢失)。
                install_staging_to_live(app_data_dir, &staging)?;
                pending.phase = RestorePhase::Installed;
                write_marker(&marker, &pending)?;
                Ok(Some(pending.backup_id))
            } else {
                // 活库已移走且暂存 db 不可用 → 逆向从 old 恢复原始现场,放弃本次恢复。
                restore_current_from_old(app_data_dir, &old)?;
                let _ = std::fs::remove_file(&marker);
                tracing::warn!("恢复逆向:暂存不可用,已从 old 恢复原始数据 | restore reversed");
                Ok(None)
            }
        }
        RestorePhase::Installed => {
            // 交换已完成,继续正常启动;verify 在迁移/就绪后由调用方 finalize。
            Ok(Some(pending.backup_id))
        }
        RestorePhase::Verified => {
            // 上次已 verify、但清理前崩溃 → 本次收尾清理。
            finalize_restore_verified(app_data_dir, &pending.backup_id);
            Ok(None)
        }
    }
}

/// 恢复成功(新库已打开/迁移/就绪)后收尾(方案 §6.2.5):先写 Verified(崩溃在清理前也能续),
/// 再清理 old 目录与暂存目录、删 marker。**回滚包保留**(≥7 天,UI 手动清理)。
pub fn finalize_restore_verified(app_data_dir: &Path, backup_id: &str) {
    let marker = marker_path(app_data_dir);
    // 尽力先把相位推到 Verified(便于清理前崩溃时下次启动续清)。
    if let Ok(Some(mut pending)) = read_marker(&marker) {
        if pending.phase != RestorePhase::Verified {
            pending.phase = RestorePhase::Verified;
            let _ = write_marker(&marker, &pending);
        }
    }
    let _ = std::fs::remove_dir_all(old_dir(app_data_dir, backup_id));
    let _ = std::fs::remove_dir_all(app_data_dir.join("restore-staging").join(backup_id));
    let _ = std::fs::remove_file(&marker);
}

/// 恢复回滚(§#3):换入库不可用(典型为**换入库迁移失败**)时,把已装入的新库/documents 撤下、
/// 从 `restore-old` 逆向恢复原始现场、清 marker 与半装 staging。**须在活库写连接/读池创建前调用,
/// 且调用前必须已 drop 掉指向活库的所有连接**(Windows 下被占用文件不可删/移)。
///
/// 与 [`perform_swap_at_boot`] 的 `CurrentMoved` 逆向不同:那里活库位置为空(尚未装入);此处已到
/// `Installed`——活库位置是**已装入的新库**,故须先删新库文件再从 old 恢复。回滚包(`restore-rollback`)
/// 不在此清理,保留作最终人工兜底。
pub fn rollback_restore_at_boot(app_data_dir: &Path, backup_id: &str) -> Result<()> {
    let old = old_dir(app_data_dir, backup_id);
    // 1. 撤下已装入的新库文件(单文件 DB + 首开可能新建的 WAL/SHM)。
    for name in LIVE_DB_FILES {
        let p = app_data_dir.join(name);
        if p.exists() {
            std::fs::remove_file(&p).map_err(io_err)?;
        }
    }
    // 2. 撤下已装入的新 documents。
    let adocs = app_data_dir.join("documents");
    if adocs.exists() {
        std::fs::remove_dir_all(&adocs).map_err(io_err)?;
    }
    // 3. 从 old 逆向恢复原始现场(幂等:目标空位方移入)。
    restore_current_from_old(app_data_dir, &old)?;
    // 4. 清半装 staging 与 marker(old 已被逆向移空,remove 无害)。
    let _ = std::fs::remove_dir_all(app_data_dir.join("restore-staging").join(backup_id));
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_file(marker_path(app_data_dir));
    Ok(())
}

/// arm:生成 pre-restore 回滚包(当前状态完整备份)→ 成功后写 pending marker(Prepared)。
/// 方案 §6.2.1:回滚包成功前绝不 arm。**须在持 `document_storage_guard` write guard 的 blocking
/// 上下文调用**(run_backup 前置不变量)。id/时间戳由调用方注入(便于确定性测试)。
#[allow(clippy::too_many_arguments)]
pub fn arm_restore(
    app_data_dir: &Path,
    restore_backup_id: &str,
    staging_dir: &Path,
    rollback_backup_id: &str,
    created_at_utc: &str,
    timestamp_label: &str,
    app_version: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    // 1. pre-restore 回滚包 = 当前状态完整备份 → restore-rollback/。
    let rollback_dir = app_data_dir.join("restore-rollback");
    std::fs::create_dir_all(&rollback_dir).map_err(io_err)?;
    let params = BackupParams {
        source_db_path: &live_db(app_data_dir),
        app_data_dir,
        dest_dir: &rollback_dir,
        kind: BackupKind::Manual,
        app_version: app_version.to_string(),
        backup_id: rollback_backup_id.to_string(),
        created_at_utc: created_at_utc.to_string(),
        timestamp_label: timestamp_label.to_string(),
        auto_retention: None,
    };
    let outcome = run_backup(&params, cancel)
        .map_err(|_| err(CODE_ROLLBACK_FAILED, "预恢复回滚包生成失败"))?;

    // 2. 回滚包成功后才写 marker(Prepared)——此后重启即触发交换。
    let pending = PendingRestore {
        backup_id: restore_backup_id.to_string(),
        staging_dir: staging_dir.to_string_lossy().to_string(),
        rollback_path: outcome.path.to_string_lossy().to_string(),
        phase: RestorePhase::Prepared,
    };
    write_marker(&marker_path(app_data_dir), &pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "scrollery_swaptest_{}_{}_{}",
            std::process::id(),
            tag,
            seq
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造活库现场(scrollery.db + documents/) + 暂存现场(restore-staging/{id}/db/scrollery.db +
    /// documents/) + marker(给定相位)。db 内容用可区分字节(非真 sqlite——交换只移文件不开库)。
    fn setup(app: &Path, id: &str, phase: RestorePhase, live_db_content: Option<&str>) {
        if let Some(c) = live_db_content {
            std::fs::write(app.join("scrollery.db"), c).unwrap();
            std::fs::create_dir_all(app.join("documents")).unwrap();
            std::fs::write(app.join("documents").join("live.txt"), "live-doc").unwrap();
        }
        let staging = app.join("restore-staging").join(id);
        std::fs::create_dir_all(staging.join("db")).unwrap();
        std::fs::write(staging.join("db").join("scrollery.db"), "NEWDB").unwrap();
        std::fs::create_dir_all(staging.join("documents")).unwrap();
        std::fs::write(staging.join("documents").join("new.txt"), "new-doc").unwrap();

        let pending = PendingRestore {
            backup_id: id.into(),
            staging_dir: staging.to_string_lossy().to_string(),
            rollback_path: app
                .join("restore-rollback")
                .join("rb.scrollerybackup")
                .to_string_lossy()
                .to_string(),
            phase,
        };
        write_marker(&marker_path(app), &pending).unwrap();
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    /// 无 marker → 正常启动零动作。
    #[test]
    fn no_marker_is_noop() {
        let app = unique_dir("noop");
        std::fs::write(app.join("scrollery.db"), "ORIGINAL").unwrap();
        assert_eq!(perform_swap_at_boot(&app).unwrap(), None);
        assert_eq!(read(&live_db(&app)), "ORIGINAL");
        let _ = std::fs::remove_dir_all(&app);
    }

    /// 完整交换:Prepared → 装入新库,原始进 old,相位 Installed;幂等重跑不变;finalize 后清理。
    #[test]
    fn full_swap_prepared_to_installed_then_finalize() {
        let app = unique_dir("full");
        setup(&app, "bk1", RestorePhase::Prepared, Some("ORIGINAL"));

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r.as_deref(), Some("bk1"));
        assert_eq!(read(&live_db(&app)), "NEWDB", "活库应为新库");
        assert_eq!(read(&app.join("documents").join("new.txt")), "new-doc");
        // 原始进 old。
        assert_eq!(read(&old_dir(&app, "bk1").join("scrollery.db")), "ORIGINAL");
        assert_eq!(
            read_marker(&marker_path(&app)).unwrap().unwrap().phase,
            RestorePhase::Installed
        );

        // 幂等重跑(模拟写 Installed 后又崩溃再启动):不改变已装状态。
        let r2 = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r2.as_deref(), Some("bk1"));
        assert_eq!(read(&live_db(&app)), "NEWDB");

        // finalize:清 old + staging + marker,回滚包保留(此测未造回滚包)。
        finalize_restore_verified(&app, "bk1");
        assert!(!marker_path(&app).exists());
        assert!(!old_dir(&app, "bk1").exists());
        assert!(!app.join("restore-staging").join("bk1").exists());
        assert_eq!(read(&live_db(&app)), "NEWDB", "finalize 不动活库");
        let _ = std::fs::remove_dir_all(&app);
    }

    /// 崩溃于 CurrentMoved(当前已移走、暂存在、活库空)→ 重启补装 → Installed。
    #[test]
    fn crash_at_current_moved_resumes_install() {
        let app = unique_dir("cm_resume");
        setup(&app, "bk2", RestorePhase::CurrentMoved, None);
        // 模拟:当前已移入 old(活库位置空)。
        let old = old_dir(&app, "bk2");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("scrollery.db"), "ORIGINAL").unwrap();
        assert!(!live_db(&app).exists(), "活库位置应为空(已移走)");

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r.as_deref(), Some("bk2"));
        assert_eq!(read(&live_db(&app)), "NEWDB", "应补装暂存库");
        assert_eq!(
            read_marker(&marker_path(&app)).unwrap().unwrap().phase,
            RestorePhase::Installed
        );
        let _ = std::fs::remove_dir_all(&app);
    }

    /// §#2 核心场景:崩溃于「db 已移入、documents 未移入」之间(活库=新库,但 documents 仍在 staging)→
    /// 重启须**补装 documents**(而非仅补写相位),否则 finalize 会连同 old 删掉 staging documents。
    #[test]
    fn crash_after_db_installed_before_docs_backfills_documents() {
        let app = unique_dir("cm_installed");
        setup(&app, "bk3", RestorePhase::CurrentMoved, None);
        // 模拟:db 已装入(暂存 db 已移走、活库=新库),但 documents 尚未从 staging 移入。
        std::fs::remove_file(staging_db(&app.join("restore-staging").join("bk3"))).unwrap();
        std::fs::write(live_db(&app), "NEWDB").unwrap();
        assert!(
            !app.join("documents").exists(),
            "前置:活库 documents 尚未装入"
        );
        assert!(
            app.join("restore-staging")
                .join("bk3")
                .join("documents")
                .join("new.txt")
                .exists(),
            "前置:新 documents 仍在 staging"
        );

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r.as_deref(), Some("bk3"));
        assert_eq!(
            read_marker(&marker_path(&app)).unwrap().unwrap().phase,
            RestorePhase::Installed
        );
        // 关键断言(#2 修复):documents 已补装到活库,不再滞留 staging。
        assert_eq!(
            read(&app.join("documents").join("new.txt")),
            "new-doc",
            "documents 须补装入活库(否则 finalize 会删除它)"
        );
        let _ = std::fs::remove_dir_all(&app);
    }

    /// §#6:Prepared 但崩溃发生在 move_current_to_old 之后、写 CurrentMoved 之前(活库已移入 old、
    /// 活库位置空),此时暂存又缺失 → 中止须从 old **逆向恢复原始现场**,不得留空库。
    #[test]
    fn prepared_crash_after_move_then_staging_lost_reverses_from_old() {
        let app = unique_dir("prep_reverse");
        setup(&app, "bk7", RestorePhase::Prepared, None);
        // 暂存 db 缺失。
        std::fs::remove_file(staging_db(&app.join("restore-staging").join("bk7"))).unwrap();
        // 活库已移入 old(相位仍标 Prepared——marker 未及更新)。
        let old = old_dir(&app, "bk7");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("scrollery.db"), "ORIGINAL").unwrap();
        std::fs::create_dir_all(old.join("documents")).unwrap();
        std::fs::write(old.join("documents").join("live.txt"), "live-doc").unwrap();
        assert!(!live_db(&app).exists(), "前置:活库位置为空(已移入 old)");

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r, None, "中止:无恢复应用");
        assert_eq!(
            read(&live_db(&app)),
            "ORIGINAL",
            "须从 old 逆向恢复原始库,不得留空"
        );
        assert_eq!(read(&app.join("documents").join("live.txt")), "live-doc");
        assert!(!marker_path(&app).exists());
        let _ = std::fs::remove_dir_all(&app);
    }

    /// §#3:Installed 相位下换入库迁移失败 → rollback_restore_at_boot 撤下新库、从 old 恢复原始、
    /// 清 marker。避免 Installed marker 永久 boot-loop、原始数据永不启用。
    #[test]
    fn rollback_at_boot_restores_original_and_clears_marker() {
        let app = unique_dir("rollback");
        setup(&app, "bk8", RestorePhase::Installed, None);
        // 已装入的新库现场(活库=新库 + 新 documents)。
        std::fs::write(live_db(&app), "NEWDB").unwrap();
        std::fs::create_dir_all(app.join("documents")).unwrap();
        std::fs::write(app.join("documents").join("new.txt"), "new-doc").unwrap();
        // 原始现场在 old。
        let old = old_dir(&app, "bk8");
        std::fs::create_dir_all(old.join("documents")).unwrap();
        std::fs::write(old.join("scrollery.db"), "ORIGINAL").unwrap();
        std::fs::write(old.join("documents").join("live.txt"), "live-doc").unwrap();

        rollback_restore_at_boot(&app, "bk8").unwrap();

        assert_eq!(read(&live_db(&app)), "ORIGINAL", "活库须回滚为原始库");
        assert_eq!(read(&app.join("documents").join("live.txt")), "live-doc");
        assert!(
            !app.join("documents").join("new.txt").exists(),
            "新 documents 须撤下"
        );
        assert!(!marker_path(&app).exists(), "marker 须清除,避免 boot-loop");
        assert!(!old.exists(), "old 已逆向移空并清理");
        let _ = std::fs::remove_dir_all(&app);
    }

    /// 逆向:CurrentMoved 但暂存 db 缺失且活库空 → 从 old 恢复原始现场,放弃恢复。
    #[test]
    fn current_moved_but_staging_lost_reverses_from_old() {
        let app = unique_dir("reverse");
        setup(&app, "bk4", RestorePhase::CurrentMoved, None);
        // 暂存 db 缺失。
        std::fs::remove_file(staging_db(&app.join("restore-staging").join("bk4"))).unwrap();
        // 当前已移入 old。
        let old = old_dir(&app, "bk4");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("scrollery.db"), "ORIGINAL").unwrap();
        std::fs::create_dir_all(old.join("documents")).unwrap();
        std::fs::write(old.join("documents").join("live.txt"), "live-doc").unwrap();
        assert!(!live_db(&app).exists());

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r, None, "逆向后无恢复应用");
        assert_eq!(read(&live_db(&app)), "ORIGINAL", "应从 old 恢复原始库");
        assert_eq!(read(&app.join("documents").join("live.txt")), "live-doc");
        assert!(!marker_path(&app).exists(), "marker 应已删");
        let _ = std::fs::remove_dir_all(&app);
    }

    /// Prepared 但暂存 db 缺失(尚未移动任何东西)→ 中止,原始现场完好,marker 删。
    #[test]
    fn prepared_but_staging_missing_aborts_keeping_original() {
        let app = unique_dir("abort");
        setup(&app, "bk5", RestorePhase::Prepared, Some("ORIGINAL"));
        std::fs::remove_file(staging_db(&app.join("restore-staging").join("bk5"))).unwrap();

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r, None);
        assert_eq!(read(&live_db(&app)), "ORIGINAL", "原始库完好");
        assert!(!marker_path(&app).exists());
        let _ = std::fs::remove_dir_all(&app);
    }

    /// Verified 相位遗留(清理前崩溃)→ 下次启动收尾清理。
    #[test]
    fn verified_phase_cleans_up_on_next_boot() {
        let app = unique_dir("verified");
        setup(&app, "bk6", RestorePhase::Verified, Some("NEWDB"));
        let old = old_dir(&app, "bk6");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("scrollery.db"), "ORIGINAL").unwrap();

        let r = perform_swap_at_boot(&app).unwrap();
        assert_eq!(r, None);
        assert!(!marker_path(&app).exists());
        assert!(!old.exists(), "old 应清理");
        assert_eq!(read(&live_db(&app)), "NEWDB", "活库不动");
        let _ = std::fs::remove_dir_all(&app);
    }

    /// 损坏 marker → restore_marker_corrupt(须人工介入,不静默)。
    #[test]
    fn corrupt_marker_is_rejected() {
        let app = unique_dir("corrupt");
        std::fs::write(marker_path(&app), "{ not valid json").unwrap();
        match perform_swap_at_boot(&app) {
            Err(AppError::Restore { code, .. }) => assert_eq!(code, CODE_MARKER_CORRUPT),
            other => panic!("期望 marker_corrupt,得 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&app);
    }
}
