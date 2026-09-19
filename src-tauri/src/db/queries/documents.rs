//! 文档域 DAO:replacements/versions/阅读进度/阅读器偏好/书签/document_meta
//! (T 线拆分自 queries.rs,SQL 与行为不变)。

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::db::models::DocumentMeta;
use crate::error::{AppError, Result};

// ── 文档替换规则（§5.2）──────────────────────────────────────────────────────
// 注：`replace` 是 SQLite 函数名，作列名一律加引号 `"replace"`。

fn map_replacement(row: &Row<'_>) -> rusqlite::Result<crate::db::models::ReplacementRule> {
    Ok(crate::db::models::ReplacementRule {
        id: row.get(0)?,
        scope_kind: row.get(1)?,
        scope_id: row.get(2)?,
        find: row.get(3)?,
        replace: row.get(4)?,
        is_regex: row.get::<_, i64>(5)? != 0,
        enabled: row.get::<_, i64>(6)? != 0,
        sort_order: row.get(7)?,
    })
}

/// 列出某具体作用域的替换规则（供规则编辑器）。`scope_id = None` → 全局（`scope_id IS NULL`）。
pub fn list_replacements(
    conn: &Connection,
    scope_kind: &str,
    scope_id: Option<i64>,
) -> Result<Vec<crate::db::models::ReplacementRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, scope_kind, scope_id, find, \"replace\", is_regex, enabled, sort_order
         FROM doc_replacements
         WHERE scope_kind = ?1 AND scope_id IS ?2
         ORDER BY sort_order, id",
    )?;
    let rows = stmt.query_map(params![scope_kind, scope_id], map_replacement)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 对某项实际生效的规则 = 启用的 global + item 作用域（group 暂缓），按序。
pub fn get_effective_replacements(
    conn: &Connection,
    item_id: i64,
) -> Result<Vec<crate::db::models::ReplacementRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, scope_kind, scope_id, find, \"replace\", is_regex, enabled, sort_order
         FROM doc_replacements
         WHERE enabled = 1
           AND (scope_kind = 'global' OR (scope_kind = 'item' AND scope_id = ?1))
         ORDER BY sort_order, id",
    )?;
    let rows = stmt.query_map(params![item_id], map_replacement)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 插入（id=None）或更新（id=Some）一条替换规则。返回行 id。
#[allow(clippy::too_many_arguments)]
pub fn upsert_replacement(
    conn: &Connection,
    id: Option<i64>,
    scope_kind: &str,
    scope_id: Option<i64>,
    find: &str,
    replace: &str,
    is_regex: bool,
    enabled: bool,
    sort_order: i64,
) -> Result<i64> {
    if let Some(id) = id {
        conn.execute(
            "UPDATE doc_replacements
             SET scope_kind=?2, scope_id=?3, find=?4, \"replace\"=?5, is_regex=?6, enabled=?7, sort_order=?8
             WHERE id=?1",
            params![id, scope_kind, scope_id, find, replace, is_regex as i64, enabled as i64, sort_order],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO doc_replacements (scope_kind, scope_id, find, \"replace\", is_regex, enabled, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![scope_kind, scope_id, find, replace, is_regex as i64, enabled as i64, sort_order],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// 按 id 删除一条替换规则。
pub fn delete_replacement(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM doc_replacements WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Document versions (§5.3) ──────────────────────────────────────────────────

fn map_version(row: &Row<'_>) -> rusqlite::Result<crate::db::models::DocumentVersion> {
    Ok(crate::db::models::DocumentVersion {
        id: row.get(0)?,
        item_id: row.get(1)?,
        parent_id: row.get(2)?,
        label: row.get(3)?,
        storage: row.get(4)?,
        abs_path: row.get(5)?,
        source: row.get(6)?,
        note: row.get(7)?,
        content_hash: row.get(8)?,
        is_current: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
    })
}

const VERSION_COLS: &str =
    "id, item_id, parent_id, label, storage, abs_path, source, note, content_hash, is_current, created_at";

/// 某文档的所有版本，最旧在前（§5.3）。
pub fn list_versions(
    conn: &Connection,
    item_id: i64,
) -> Result<Vec<crate::db::models::DocumentVersion>> {
    let sql = format!(
        "SELECT {VERSION_COLS} FROM document_versions WHERE item_id = ?1 ORDER BY created_at, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![item_id], map_version)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 按 id 取单个版本（§5.3）。
pub fn get_version(
    conn: &Connection,
    id: i64,
) -> Result<Option<crate::db::models::DocumentVersion>> {
    let sql = format!("SELECT {VERSION_COLS} FROM document_versions WHERE id = ?1");
    conn.query_row(&sql, params![id], map_version)
        .optional()
        .map_err(AppError::from)
}

/// 文档当前版本（若有标记，§5.3）。`None` → 以源文件为当前。
pub fn get_current_version(
    conn: &Connection,
    item_id: i64,
) -> Result<Option<crate::db::models::DocumentVersion>> {
    let sql = format!(
        "SELECT {VERSION_COLS} FROM document_versions WHERE item_id = ?1 AND is_current = 1 LIMIT 1"
    );
    conn.query_row(&sql, params![item_id], map_version)
        .optional()
        .map_err(AppError::from)
}

/// 插入一行版本（abs_path 在拿到 id 推导路径后再回填）。
#[allow(clippy::too_many_arguments)]
pub fn insert_version(
    conn: &Connection,
    item_id: i64,
    parent_id: Option<i64>,
    label: Option<&str>,
    storage: &str,
    abs_path: &str,
    source: &str,
    content_hash: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO document_versions (item_id, parent_id, label, storage, abs_path, source, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![item_id, parent_id, label, storage, abs_path, source, content_hash],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 写盘后回填版本文件路径（两步：插入 → 写文件 → 置路径）。
pub fn update_version_path(conn: &Connection, id: i64, abs_path: &str) -> Result<()> {
    conn.execute(
        "UPDATE document_versions SET abs_path = ?2 WHERE id = ?1",
        params![id, abs_path],
    )?;
    Ok(())
}

/// 将某版本标为当前（或全清 → 以源文件为当前）。当前版本至多一个。
pub fn set_current_version(conn: &Connection, item_id: i64, version_id: Option<i64>) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE document_versions SET is_current = 0 WHERE item_id = ?1",
        params![item_id],
    )?;
    if let Some(vid) = version_id {
        tx.execute(
            "UPDATE document_versions SET is_current = 1 WHERE id = ?1 AND item_id = ?2",
            params![vid, item_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 删除一行版本，返回其文件路径以便调用者删除文件。
pub fn delete_version(conn: &Connection, id: i64) -> Result<Option<String>> {
    let path = conn
        .query_row(
            "SELECT abs_path FROM document_versions WHERE id = ?1",
            params![id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    conn.execute("DELETE FROM document_versions WHERE id = ?1", params![id])?;
    Ok(path)
}

/// 读取文档已保存的阅读位置（页码 / CFI / 滚动比例），若有（§5.1）。
pub fn get_reading_progress(conn: &Connection, item_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT position FROM reading_progress WHERE item_id = ?1",
        params![item_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// 写入/更新文档阅读位置（§5.1）。不透明字符串：由渲染器决定格式（pdf 页码、epub CFI、文本滚动比例）。
pub fn set_reading_progress(conn: &Connection, item_id: i64, position: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO reading_progress (item_id, position, updated_at)
         VALUES (?1, ?2, strftime('%s','now'))
         ON CONFLICT(item_id) DO UPDATE SET position=excluded.position, updated_at=excluded.updated_at",
        params![item_id, position],
    )?;
    Ok(())
}

/// 取某项的 txt 章节索引缓存(阅读器 R1,§6.1)。返回 `(src_key, encoding, confidence, chapters_json)`;
/// 从未索引则 `None`。调用方比对 `src_key` 判定缓存是否有效(源指纹变即重建)。
pub fn get_text_book_index(
    conn: &Connection,
    item_id: i64,
) -> Result<Option<(String, String, String, String)>> {
    conn.query_row(
        "SELECT src_key, encoding, confidence, chapters FROM text_book_index WHERE item_id = ?1",
        params![item_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .optional()
    .map_err(AppError::from)
}

/// 写入/更新某项的 txt 章节索引缓存(阅读器 R1,§6.1)。`chapters_json` 为 ChapterMeta 数组的 JSON。
pub fn upsert_text_book_index(
    conn: &Connection,
    item_id: i64,
    src_key: &str,
    encoding: &str,
    confidence: &str,
    chapters_json: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO text_book_index (item_id, src_key, encoding, confidence, chapters, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))
         ON CONFLICT(item_id) DO UPDATE SET
             src_key=excluded.src_key, encoding=excluded.encoding,
             confidence=excluded.confidence, chapters=excluded.chapters, updated_at=excluded.updated_at",
        params![item_id, src_key, encoding, confidence, chapters_json],
    )?;
    Ok(())
}

/// 取某书的每书阅读偏好 JSON(阅读器 R1/R3,§6.1)。从未设置则 `None`。
pub fn get_reader_book_prefs(conn: &Connection, item_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT prefs FROM reader_book_prefs WHERE item_id = ?1",
        params![item_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// 写入/更新某书的每书阅读偏好 JSON(阅读器 R1/R3,§6.1)。`prefs` 为版本化 JSON diff。
pub fn set_reader_book_prefs(conn: &Connection, item_id: i64, prefs: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO reader_book_prefs (item_id, prefs, updated_at)
         VALUES (?1, ?2, strftime('%s','now'))
         ON CONFLICT(item_id) DO UPDATE SET prefs=excluded.prefs, updated_at=excluded.updated_at",
        params![item_id, prefs],
    )?;
    Ok(())
}

/// 列出某书全部书签(阅读器 R4,§6.2),按全书进度升序(同进度回退按 id)。
pub fn list_reader_bookmarks(
    conn: &Connection,
    item_id: i64,
) -> Result<Vec<crate::db::models::ReaderBookmark>> {
    let mut stmt = conn.prepare(
        "SELECT id, locator, label, fraction, created_at
         FROM reader_bookmarks WHERE item_id = ?1
         ORDER BY fraction ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![item_id], |row| {
        Ok(crate::db::models::ReaderBookmark {
            id: row.get(0)?,
            locator: row.get(1)?,
            label: row.get(2)?,
            fraction: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 新增书签(阅读器 R4)。同位置(item_id+locator)幂等:重复添加即刷新标签/进度/时间,不产生重复行。
pub fn add_reader_bookmark(
    conn: &Connection,
    item_id: i64,
    locator: &str,
    label: &str,
    fraction: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO reader_bookmarks (item_id, locator, label, fraction, created_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))
         ON CONFLICT(item_id, locator) DO UPDATE SET
             label=excluded.label, fraction=excluded.fraction, created_at=excluded.created_at",
        params![item_id, locator, label, fraction],
    )?;
    Ok(())
}

/// 删除书签(阅读器 R4),按书签 id。
pub fn delete_reader_bookmark(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM reader_bookmarks WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod reader_bookmarks_tests {
    //! 阅读器 R4 书签 CRUD:新增(含同位置幂等)/ 按全书进度排序列出 / 删除 / 跨书隔离。
    use super::*;

    fn seeded() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap(); // 免构造 media_items,直接插书签
        c
    }

    #[test]
    fn add_list_dedup_delete_roundtrip() {
        let c = seeded();
        // 乱序插入(fraction 0.5 先于 0.1),列出须按 fraction 升序归位。
        add_reader_bookmark(&c, 1, "cfi:/6/4!/2", "第三章", 0.5).unwrap();
        add_reader_bookmark(&c, 1, "cfi:/6/2!/2", "第一章", 0.1).unwrap();
        // 另一本书的书签不应串入 item 1 的列表。
        add_reader_bookmark(&c, 2, "cfi:/6/2!/2", "别的书", 0.2).unwrap();

        let list = list_reader_bookmarks(&c, 1).unwrap();
        assert_eq!(list.len(), 2, "item 1 应有 2 条书签");
        assert_eq!(list[0].fraction, 0.1, "须按全书进度升序");
        assert_eq!(list[0].label, "第一章");
        assert_eq!(list[1].fraction, 0.5);

        // 同位置(item_id+locator)再添加 → 幂等刷新标签/进度,不新增行。
        add_reader_bookmark(&c, 1, "cfi:/6/2!/2", "第一章(改)", 0.12).unwrap();
        let list = list_reader_bookmarks(&c, 1).unwrap();
        assert_eq!(list.len(), 2, "同位置重复添加不应产生重复行");
        let first = list.iter().find(|b| b.locator == "cfi:/6/2!/2").unwrap();
        assert_eq!(first.label, "第一章(改)", "同位置再添加须刷新标签");
        assert_eq!(first.fraction, 0.12);

        // 删除一条 → 只剩一条,且不波及别的书。
        let del_id = list[1].id;
        delete_reader_bookmark(&c, del_id).unwrap();
        assert_eq!(list_reader_bookmarks(&c, 1).unwrap().len(), 1);
        assert_eq!(
            list_reader_bookmarks(&c, 2).unwrap().len(),
            1,
            "删除不应波及别的书"
        );
    }
}

// ── 文档元数据 DAO（document_meta，Phase 2「死表」激活）────────────────────────
//
// 该表 SCHEMA_V1 即建但此前无 DAO（死表）。文档 enrichment（Part3）算出页数/子类型后写入，
// 供阅读器进度条 / 封面派生消费。

/// upsert 文档元数据（页数 / 子类型）。`item_id` 冲突则覆盖（重新 enrich 即更新）。
pub fn upsert_document_meta(
    conn: &Connection,
    item_id: i64,
    page_count: Option<i64>,
    doc_subtype: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO document_meta (item_id, page_count, doc_subtype) VALUES (?1, ?2, ?3)
         ON CONFLICT(item_id) DO UPDATE SET
             page_count  = excluded.page_count,
             doc_subtype = excluded.doc_subtype",
        params![item_id, page_count, doc_subtype],
    )?;
    Ok(())
}

/// 取媒体项的 file_format(doc_subtype 回填用权威源,不信前端回传;不存在返回 None)。
pub fn get_item_file_format(conn: &Connection, item_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT file_format FROM media_items WHERE id=?1",
        params![item_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// 取文档元数据（不存在返回 None）。
pub fn get_document_meta(conn: &Connection, item_id: i64) -> Result<Option<DocumentMeta>> {
    conn.query_row(
        "SELECT item_id, page_count, doc_subtype FROM document_meta WHERE item_id = ?1",
        params![item_id],
        |row| {
            Ok(DocumentMeta {
                item_id: row.get(0)?,
                page_count: row.get(1)?,
                doc_subtype: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

#[cfg(test)]
mod document_meta_tests {
    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        // document_meta.item_id FK→media_items；关 FK 免构造 media 行（DAO 逻辑测试）。
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c
    }

    /// upsert 首写 → get 命中；同 item_id 再 upsert → 覆盖（页数/子类型同时更新）。
    #[test]
    fn upsert_get_and_overwrite() {
        let c = mem_db();
        assert!(
            get_document_meta(&c, 1).unwrap().is_none(),
            "未写入应为 None"
        );

        upsert_document_meta(&c, 1, Some(10), Some("pdf")).unwrap();
        let m = get_document_meta(&c, 1).unwrap().unwrap();
        assert_eq!(m.item_id, 1);
        assert_eq!(m.page_count, Some(10));
        assert_eq!(m.doc_subtype.as_deref(), Some("pdf"));

        // 重新 enrich：覆盖为 epub + 新页数。
        upsert_document_meta(&c, 1, Some(12), Some("epub")).unwrap();
        let m2 = get_document_meta(&c, 1).unwrap().unwrap();
        assert_eq!(m2.page_count, Some(12), "页数应被覆盖");
        assert_eq!(m2.doc_subtype.as_deref(), Some("epub"), "子类型应被覆盖");

        // None 字段也能存（未知页数）。
        upsert_document_meta(&c, 2, None, Some("svg")).unwrap();
        let m3 = get_document_meta(&c, 2).unwrap().unwrap();
        assert_eq!(m3.page_count, None);
    }
}
