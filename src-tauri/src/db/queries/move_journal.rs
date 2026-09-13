//! 目录移动阶段日志（V33 `directory_move_journal`）的读写。
//!
//! 目录移动的物理动作先于数据库事务：两者之间任何一步失败或进程退出，磁盘与库就会分叉。
//! 本模块只做一件事——把「搬到哪一步」持久化，使失败与重启后的收尾可判定、可重试、幂等。
//! 表结构与阶段语义见 `db/schema/late.rs::SCHEMA_V33` 的文档注释。

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};

/// 已登记意图：物理搬运尚未确认完成。
pub const STAGE_INTENT: &str = "intent";
/// 目标目录已完整落盘（跨卷路径已发布、同卷 rename 已完成），数据库尚未改写。
pub const STAGE_PUBLISHED: &str = "published";
/// 数据库已改写，源目录树残留（跨卷删源中断）。
pub const STAGE_SOURCE_LEFTOVER: &str = "source_leftover";

/// 一条未完成的目录移动。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveJournalEntry {
    pub id: i64,
    pub stage: String,
    pub source_dir_id: i64,
    pub source_root_id: i64,
    pub source_rel_path: String,
    pub source_abs_path: String,
    pub target_root_id: i64,
    pub target_rel_path: String,
    pub target_abs_path: String,
    /// 目标父目录行 id（扫描根下 = 该根 rel_path='' 的目录行）。
    pub target_parent_id: i64,
    /// 我们自己在目标卷上创建的独占暂存目录；`None` = 本次移动不经暂存（同卷 rename）。
    pub staging_abs_path: Option<String>,
    /// 跨卷复制时对**实际写出树**算出的内容凭据（发布 rename 之前持久化）。
    /// `None` = 同卷 rename 路径：没有「我们写出的树」，发布由「源已不在」自证。
    pub payload_digest: Option<String>,
    pub payload_files: i64,
    pub affected_dirs: i64,
    pub affected_media: i64,
}

/// 登记一次移动意图，返回日志行 id。
#[allow(clippy::too_many_arguments)]
pub fn insert_intent(
    conn: &Connection,
    source_dir_id: i64,
    source_root_id: i64,
    source_rel_path: &str,
    source_abs_path: &str,
    target_root_id: i64,
    target_rel_path: &str,
    target_abs_path: &str,
    target_parent_id: i64,
    staging_abs_path: Option<&str>,
) -> Result<i64> {
    conn.query_row(
        "INSERT INTO directory_move_journal
            (stage, source_dir_id, source_root_id, source_rel_path, source_abs_path,
             target_root_id, target_rel_path, target_abs_path, target_parent_id, staging_abs_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         RETURNING id",
        params![
            STAGE_INTENT,
            source_dir_id,
            source_root_id,
            source_rel_path,
            source_abs_path,
            target_root_id,
            target_rel_path,
            target_abs_path,
            target_parent_id,
            staging_abs_path,
        ],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 持久化「我们写出树」的内容凭据。**必须在发布 rename 之前调用**：崩溃在「发布后、删源前」
/// 时，恢复只有靠这行记录才能证明目标属于本次移动（否则双端存在会被判成外部冲突）。
pub fn set_payload(
    conn: &Connection,
    id: i64,
    digest: Option<&str>,
    files: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE directory_move_journal SET payload_digest=COALESCE(?1, payload_digest),
                payload_files=COALESCE(?2, payload_files), updated_at=strftime('%s','now')
         WHERE id=?3",
        params![digest, files, id],
    )?;
    Ok(())
}

/// 推进阶段（`updated_at` 同写）。
pub fn set_stage(conn: &Connection, id: i64, stage: &str) -> Result<()> {
    conn.execute(
        "UPDATE directory_move_journal SET stage=?1, updated_at=strftime('%s','now') WHERE id=?2",
        params![stage, id],
    )?;
    Ok(())
}

/// 记录受影响规模（仅诊断用；不参与恢复判定）。
pub fn set_counts(conn: &Connection, id: i64, dirs: i64, media: i64) -> Result<()> {
    conn.execute(
        "UPDATE directory_move_journal SET affected_dirs=?1, affected_media=?2,
                updated_at=strftime('%s','now') WHERE id=?3",
        params![dirs, media, id],
    )?;
    Ok(())
}

/// 扫描根的最小定位信息（引擎按 id 现查根路径与卷绑定；重挂载后也能对上）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRef {
    pub path: String,
    pub volume_id: Option<i64>,
    pub volume_subpath: Option<String>,
}

/// 按 id 取扫描根定位：`Ok(None)` = 该行确实不存在（删根/清库后的终态，日志可作废）；
/// `Err` = 查询本身失败，调用方**必须保留**恢复线索而不是当成「根没了」。
pub fn find_scan_root_ref(conn: &Connection, root_id: i64) -> Result<Option<RootRef>> {
    conn.query_row(
        "SELECT path, volume_id, volume_subpath FROM scan_roots WHERE id=?1",
        params![root_id],
        |row| {
            Ok(RootRef {
                path: row.get(0)?,
                volume_id: row.get(1)?,
                volume_subpath: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

/// 收尾：删除日志行（移动已完整完成）。
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM directory_move_journal WHERE id=?1",
        params![id],
    )?;
    Ok(())
}

/// 全部未完成项（按登记顺序，先来先收）。
pub fn list_pending(conn: &Connection) -> Result<Vec<MoveJournalEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, stage, source_dir_id, source_root_id, source_rel_path, source_abs_path,
                target_root_id, target_rel_path, target_abs_path, staging_abs_path,
                target_parent_id, payload_digest, payload_files, affected_dirs, affected_media
         FROM directory_move_journal ORDER BY id",
    )?;
    let rows = stmt.query_map([], map_entry)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 按 id 取一条。
pub fn get(conn: &Connection, id: i64) -> Result<Option<MoveJournalEntry>> {
    conn.query_row(
        "SELECT id, stage, source_dir_id, source_root_id, source_rel_path, source_abs_path,
                target_root_id, target_rel_path, target_abs_path, staging_abs_path,
                target_parent_id, payload_digest, payload_files, affected_dirs, affected_media
         FROM directory_move_journal WHERE id=?1",
        params![id],
        map_entry,
    )
    .optional()
    .map_err(AppError::from)
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MoveJournalEntry> {
    Ok(MoveJournalEntry {
        id: row.get(0)?,
        stage: row.get(1)?,
        source_dir_id: row.get(2)?,
        source_root_id: row.get(3)?,
        source_rel_path: row.get(4)?,
        source_abs_path: row.get(5)?,
        target_root_id: row.get(6)?,
        target_rel_path: row.get(7)?,
        target_abs_path: row.get(8)?,
        staging_abs_path: row.get(9)?,
        target_parent_id: row.get(10)?,
        payload_digest: row.get(11)?,
        payload_files: row.get(12)?,
        affected_dirs: row.get(13)?,
        affected_media: row.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c
    }

    #[test]
    fn journal_round_trip_and_stage_advance() {
        let c = mem_db();
        let id = insert_intent(
            &c,
            10,
            1,
            "A",
            "/r1/A",
            2,
            "B/A",
            "/r2/B/A",
            20,
            Some("/r2/B/.A.tmp"),
        )
        .unwrap();
        let e = get(&c, id).unwrap().expect("row");
        assert_eq!(e.stage, STAGE_INTENT);
        assert_eq!(e.source_rel_path, "A");
        assert_eq!(e.target_abs_path, "/r2/B/A");
        assert_eq!(e.target_parent_id, 20);
        assert_eq!(e.staging_abs_path.as_deref(), Some("/r2/B/.A.tmp"));
        assert_eq!(e.payload_digest, None, "发布前没有凭据");

        // 发布前先落凭据，再推进阶段（崩溃窗的安全序）。
        set_payload(&c, id, Some("deadbeef"), Some(4)).unwrap();
        set_stage(&c, id, STAGE_PUBLISHED).unwrap();
        set_counts(&c, id, 3, 7).unwrap();
        let e = get(&c, id).unwrap().unwrap();
        assert_eq!(e.stage, STAGE_PUBLISHED);
        assert_eq!(e.payload_digest.as_deref(), Some("deadbeef"));
        assert_eq!(e.payload_files, 4);
        assert_eq!((e.affected_dirs, e.affected_media), (3, 7));
        assert_eq!(list_pending(&c).unwrap().len(), 1);

        // set_payload(None) 不擦掉既有凭据（阶段推进不丢证据）。
        set_payload(&c, id, None, None).unwrap();
        assert_eq!(
            get(&c, id).unwrap().unwrap().payload_digest.as_deref(),
            Some("deadbeef")
        );

        delete(&c, id).unwrap();
        assert!(list_pending(&c).unwrap().is_empty());
        assert!(get(&c, id).unwrap().is_none());
    }

    /// 根定位三态：存在 → Some；行确实不在 → None（可作废日志）；查询失败 → Err（必须保留线索）。
    #[test]
    fn find_scan_root_ref_distinguishes_absent_from_error() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias, volume_id, volume_subpath) VALUES (1, '/r', 'R', NULL, NULL)",
            [],
        )
        .unwrap();
        let found = find_scan_root_ref(&c, 1).unwrap().expect("root exists");
        assert_eq!(found.path, "/r");
        assert_eq!(found.volume_id, None);
        assert_eq!(find_scan_root_ref(&c, 999).unwrap(), None, "行不在 → None");

        c.execute_batch("DROP TABLE scan_roots;").unwrap();
        assert!(
            find_scan_root_ref(&c, 1).is_err(),
            "查询失败不能伪装成「根不存在」，否则会丢掉恢复线索"
        );
    }
}
