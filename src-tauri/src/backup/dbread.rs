//! 备份与恢复**共用**的只读查询(审查 #E 去重):counts / roots / external 计数与 quick_check。
//!
//! 原先 `core.rs`(产 manifest 摘要)与 `restore.rs`(产恢复摘要)各存一份**逐字相同**的查询——
//! schema 一改(如 counts 增列、表改名),漏改一处便使「备份 manifest 的数字」与「恢复确认页的
//! 数字」静默漂移,用户对着错数字做双确认。此模块把这些查询收拢为单一事实源;错误映射(backup_io /
//! restore_io 等域码)仍由各调用方按 `rusqlite::Result` 自理,不耦合到此。

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

/// `PRAGMA quick_check` 首行是否为 "ok"(完整性)。调用方据 false / Err 映射各自域码。
pub(super) fn integrity_ok(conn: &Connection) -> rusqlite::Result<bool> {
    let first: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    Ok(first == "ok")
}
