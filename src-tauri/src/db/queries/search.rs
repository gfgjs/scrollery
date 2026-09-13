//! 文件名搜索 DAO:LIKE 搜索 + 语义搜索按 id 回表(T 线拆分自 queries.rs,SQL 与行为不变)。
//! 与 layout builder 有意不合并(设计 §4.1);`push_in_predicate` 从 owner(layout)定向引用
//! (D-012 方案 a 收尾,可见性 pub(in crate::db::queries))。

use rusqlite::Connection;

use super::layout::push_in_predicate;
use crate::db::models::{MediaFilter, SearchResult};
use crate::error::{AppError, Result};

// ── 搜索 ────────────────────────────────────────────────────────────────────

pub fn search_media(
    conn: &Connection,
    query: &str,
    filter: &MediaFilter,
    limit: i64,
) -> Result<Vec<SearchResult>> {
    // LIKE 通配转义(2026-07-06 审查 R19):同 build_media_filter,转义 \ % _ 再包 %…% + ESCAPE。
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let mut sql = String::from(
        "SELECT id, file_name, media_type, width, height, thumb_path, thumbhash, thumb_status
         FROM media_items
         WHERE is_deleted=0 AND companion_of IS NULL AND file_name LIKE ?1 ESCAPE '\\'",
    );

    let mut extras: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(pattern)];
    let mut param_idx = 1usize;

    if let Some(dir_id) = filter.directory_id {
        param_idx += 1;
        sql.push_str(&format!(
            " AND directory_id IN (
            WITH RECURSIVE dir_tree(id) AS (
                SELECT ?{param_idx}
                UNION ALL
                SELECT d.id FROM directories d
                JOIN dir_tree t ON d.parent_id = t.id
            )
            SELECT id FROM dir_tree
        )"
        ));
        extras.push(Box::new(dir_id));
    }

    if let Some(ref types) = filter.media_types {
        param_idx = push_in_predicate(&mut sql, &mut extras, param_idx, "media_type", types);
    }
    // 搜索侧同样吃格式筛选：搜索结果与画廊是同一批筛选维度，只在这里漏一处，用户就会看到
    // 「画廊按 PNG 筛过了、搜同一个词却把 JPG 也搜出来」。
    if let Some(ref formats) = filter.file_formats {
        param_idx = push_in_predicate(&mut sql, &mut extras, param_idx, "file_format", formats);
    }

    // 隐藏根排除(V21):搜索结果随可见集收窄——隐藏根内的文件不出现在搜索里,与画廊/facet 一致。
    // 空集(常态)→ 不加谓词。无别名 `directory_id`(本查询 FROM media_items)。
    let hidden = super::scan::hidden_root_ids(conn)?;
    if !hidden.is_empty() {
        let placeholders: Vec<String> = (0..hidden.len())
            .map(|i| format!("?{}", param_idx + i + 1))
            .collect();
        sql.push_str(&format!(
            " AND directory_id NOT IN (SELECT id FROM directories WHERE root_id IN ({}))",
            placeholders.join(",")
        ));
        for r in &hidden {
            extras.push(Box::new(*r));
        }
        param_idx += hidden.len();
    }

    param_idx += 1;
    sql.push_str(&format!(" ORDER BY sort_datetime DESC LIMIT ?{param_idx}"));
    extras.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = extras.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok(SearchResult {
            id: row.get(0)?,
            file_name: row.get(1)?,
            media_type: row.get(2)?,
            width: row.get(3)?,
            height: row.get(4)?,
            thumb_path: row.get(5)?,
            thumbhash: row.get(6)?,
            thumb_status: row.get(7)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 获取一批 ID 的媒体项缩略图信息（用于语义搜索结果）。
///
/// 对于 `thumb_status=3`（小文件直接显示），`thumb_path` 列为 NULL，通过 JOIN 解析绝对路径
/// 供前端直显。NULL 路径行被折算为 status=3 + 源文件路径:搜索结果以「直显原图」优雅降级,
/// 而非裂图占位。
pub fn get_search_results_by_ids(
    conn: &Connection,
    ids: &[i64],
) -> Result<Vec<crate::db::models::SearchResult>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT m.id, m.file_name, m.media_type, m.width, m.height,
                CASE
                    WHEN m.thumb_status = 3 OR m.thumb_path IS NULL THEN
                        CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                             ELSE r.path || '/' || d.rel_path || '/' || m.file_name
                        END
                    ELSE m.thumb_path
                END AS thumb_path,
                m.thumbhash,
                CASE
                    WHEN m.thumb_path IS NULL THEN 3
                    ELSE m.thumb_status
                END AS thumb_status
         FROM media_items m
         JOIN directories d ON m.directory_id = d.id
         JOIN scan_roots r ON d.root_id = r.id
         WHERE m.id IN ({})",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok(crate::db::models::SearchResult {
            id: row.get(0)?,
            file_name: row.get(1)?,
            media_type: row.get(2)?,
            width: row.get(3)?,
            height: row.get(4)?,
            thumb_path: row.get(5)?,
            thumbhash: row.get(6)?,
            thumb_status: row.get(7)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

#[cfg(test)]
mod r2_6_query_tests {
    use rusqlite::params;

    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // root1 → A(顶层) → A/B(子);C(顶层,无子)。
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth) VALUES
                 (10, 1, NULL, 'A', 'A', 0),
                 (11, 1, 10, 'A/B', 'B', 1),
                 (12, 1, NULL, 'C', 'C', 0);",
        )
        .unwrap();
        c
    }

    /// R19:search_media 的 LIKE 通配转义——下划线按字面匹配,不当「任意单字符」通配。
    #[test]
    fn search_escapes_like_wildcards() {
        let c = mem_db();
        // 两个只差下划线位的文件名:字面 "IMG_20" vs "IMGX20"。
        for (id, name) in [(1_i64, "IMG_20.jpg"), (2, "IMGX20.jpg"), (3, "IMG_200.jpg")] {
            c.execute(
                "INSERT INTO media_items
                    (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                     width, height, sort_datetime, cache_key, is_favorited, is_deleted,
                     is_live_photo, companion_of)
                 VALUES (?1, 10, ?2, 0, 0, 'jpg', 'image', 0, 0, 0, 0, 0, 0, 0, NULL)",
                params![id, name],
            )
            .unwrap();
        }

        let filter = super::MediaFilter::default();
        let hits = super::search_media(&c, "IMG_20", &filter, 100).unwrap();
        let names: std::collections::HashSet<&str> =
            hits.iter().map(|r| r.file_name.as_str()).collect();

        // 转义后 "_" 是字面 → "IMGX20" 不应命中;"IMG_20" 与前缀相同的 "IMG_200" 命中(%…%)。
        assert!(names.contains("IMG_20.jpg"), "字面匹配应命中");
        assert!(names.contains("IMG_200.jpg"), "含子串 IMG_20 应命中");
        assert!(
            !names.contains("IMGX20.jpg"),
            "下划线不再当通配 → IMGX20 不应命中(R19)"
        );
    }
}
