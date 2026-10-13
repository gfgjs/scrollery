//! 备份 manifest 与恢复摘要共用的只读查询，避免两处计数口径漂移。
//! 返回 `rusqlite::Result`；备份和恢复各自映射领域错误码。

use rusqlite::Connection;

use super::manifest::{Counts, RootEntry};

/// 计数摘要:未删条目 / 相册 / 标签 / 具名人物。
pub(super) fn read_counts(conn: &Connection) -> rusqlite::Result<Counts> {
    let q = |sql: &str| -> rusqlite::Result<i64> { conn.query_row(sql, [], |r| r.get(0)) };
    Ok(Counts {
        items: q("SELECT COUNT(*) FROM media_items WHERE is_deleted=0")?,
        albums: q("SELECT COUNT(*) FROM albums")?,
        tags: q("SELECT COUNT(*) FROM tags")?,
        named_persons: q("SELECT COUNT(*) FROM persons WHERE is_named=1")?,
    })
}

/// 扫描根摘要(id / alias / hidden,按 id 升序)。
pub(super) fn read_roots(conn: &Connection) -> rusqlite::Result<Vec<RootEntry>> {
    let mut stmt = conn.prepare("SELECT id, alias, is_hidden FROM scan_roots ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(RootEntry {
            id: r.get(0)?,
            alias: r.get(1)?,
            hidden: r.get::<_, i64>(2)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// `storage='external'` 的文档版本数(不入包,仅摘要提示)。
pub(super) fn read_external_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM document_versions WHERE storage='external'",
        [],
        |r| r.get(0),
    )
}

/// 页结构与外键完整性检查。调用方据 false / Err 映射各自域码。
pub(super) fn integrity_ok(conn: &Connection) -> rusqlite::Result<bool> {
    let first: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if first != "ok" {
        return Ok(false);
    }
    // quick_check 不检查外键；不能发布恢复端会拒绝的快照并轮转掉可用旧包。
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    Ok(rows.next()?.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{run_backup, BackupKind, BackupParams};
    use crate::error::AppError;
    use rusqlite::{params, OpenFlags};
    use std::path::{Path, PathBuf};
    use tokio_util::sync::CancellationToken;

    fn seed_database(app_data: &Path) -> PathBuf {
        let path = app_data.join("scrollery.db");
        let conn = Connection::open(&path).unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        path
    }

    fn insert_orphan_directory(path: &Path) {
        let conn = Connection::open(path).unwrap();
        // 只在临时数据库模拟已有逻辑损坏，不借助页损坏影响其他校验。
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        conn.execute(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES (?1, ?2, ?3, ?4)",
            params![1, 999, "orphan", "orphan"],
        )
        .unwrap();
    }

    #[test]
    fn integrity_rejects_foreign_key_violation_on_readonly_connection() {
        let app_data = tempfile::tempdir().unwrap();
        let path = seed_database(app_data.path());
        insert_orphan_directory(&path);
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let quick: String = conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(quick, "ok", "页结构正常的外键断链也必须被识别");
        assert!(!integrity_ok(&conn).unwrap());
    }

    #[test]
    fn invalid_snapshot_must_not_publish_or_rotate_last_valid_backup() {
        let app_data = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let path = seed_database(app_data.path());
        let mut params = BackupParams {
            source_db_path: &path,
            app_data_dir: app_data.path(),
            dest_dir: destination.path(),
            kind: BackupKind::Auto,
            app_version: "0.1.0".into(),
            backup_id: "valid-snapshot".into(),
            created_at_utc: "2026-10-11T00:00:00Z".into(),
            timestamp_label: "20261011-000000".into(),
            auto_retention: Some(1),
        };
        let valid = run_backup(&params, &CancellationToken::new()).unwrap();
        insert_orphan_directory(&path);
        params.backup_id = "invalid-snapshot".into();
        params.timestamp_label = "20261011-000001".into();
        params.created_at_utc = "2026-10-11T00:00:01Z".into();

        let result = run_backup(&params, &CancellationToken::new());

        assert!(
            valid.path.is_file(),
            "损坏快照不能淘汰最后的正常备份: {result:?}"
        );
        assert!(matches!(
            result,
            Err(AppError::Backup {
                code: crate::backup::core::CODE_IO,
                ..
            })
        ));
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 1);
    }
}
