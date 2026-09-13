//! 布局域 facet 查询:库内实际存在的媒体格式列表、全库目录标签映射。域内零依赖。

use rusqlite::Connection;

use crate::db::models::DirLabel;
use crate::error::{AppError, Result};

/// 库内**实际存在**的媒体格式（S 线 §7.2 / D-005）—— 细分格式弹层的 facet 取数。
///
/// ## 为什么只取 `DISTINCT file_format` 而不带 `media_type`、不带 counts
///
/// 分组是 **registry 的知识**（`png → Image`），不是 DB 的知识。实测（生产库 543,449 项）：
///
/// | 查法 | 耗时 | 计划 |
/// |---|---|---|
/// | `DISTINCT file_format` + 基础谓词 | **0.0 ms** | `USING INDEX idx_media_format` |
/// | `DISTINCT (media_type, file_format)` + 基础谓词 | 114.3 ms | 全扫 |
/// | `GROUP BY (media_type,file_format) + COUNT(*)` | 216.0 ms | `TEMP B-TREE` |
///
/// 故大类归属交给前端拿 registry 去配，counts 不显示（唯一用途是预判结果数，点下去就知道，
/// 不值 216ms + 缓存 + 失效）。0ms 也意味着**不建缓存层、不做失效逻辑**，每次开弹层现查。
///
/// ## 基础谓词是契约不是装饰
///
/// `is_deleted=0 AND companion_of IS NULL` 与正常媒体视图（[`push_where_predicates`](super::query_builder::push_where_predicates)）一致，
/// 阻止只存在于回收站 / Live-Photo 伴随视频里的格式污染全局 facet。
///
/// ⚠ 当前生产库 raw distinct == base distinct == 29 **是数据巧合**（实测：被基础谓词滤掉的只有
/// 4 行 `is_deleted=1` 的 jpg，而 jpg 在 base 里也存在）—— 不能拿巧合替代契约。
///
/// ## 口径是全局的
///
/// facet 回答「这个库能筛什么」，**不随视图收窄**。文件夹视图里可能出现选中后 0 结果的格式 ——
/// 有意行为，与评分 / 颜色 chip 既有语义一致（评分 chip 在空文件夹里也不消失）。
pub fn list_library_formats(conn: &Connection) -> Result<Vec<String>> {
    // 隐藏根排除（V21）：facet 口径随可见集收窄——隐藏根独有的格式不应仍出现在筛选弹层。空集
    // （常态）→ 不加谓词，仍走 idx_media_format 的 0ms 快路径；有隐藏根时退化为一次全扫（罕见 +
    // 弹层按需现查，可接受）。此处用无别名 `directory_id`（本查询 FROM media_items 无 `m` 别名）。
    let hidden = super::super::scan::hidden_root_ids(conn)?;
    let mut sql = String::from(
        "SELECT DISTINCT file_format FROM media_items
         WHERE is_deleted = 0 AND companion_of IS NULL",
    );
    if !hidden.is_empty() {
        let placeholders: Vec<String> = (0..hidden.len()).map(|i| format!("?{}", i + 1)).collect();
        sql.push_str(&format!(
            " AND directory_id NOT IN (SELECT id FROM directories WHERE root_id IN ({}))",
            placeholders.join(",")
        ));
    }
    sql.push_str("\n         ORDER BY file_format ASC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> =
        hidden.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(refs.as_slice(), |row| row.get::<_, String>(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 全库目录标签映射（S2）：`dir_id → DirLabel`。目录量级 ~10^3，布局重算时一次性取回，
/// 替代原布局查询里逐行（10^6）的双 JOIN 与路径拼接（1M 库实测占查询成本 2/3）。
pub fn query_dir_labels(conn: &Connection) -> Result<std::collections::HashMap<i64, DirLabel>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.rel_path, d.name, r.path, r.created_at, r.id
         FROM directories d
         JOIN scan_roots r ON d.root_id = r.id",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let rel_path: String = row.get(1)?;
        let name: String = row.get(2)?;
        let root_path: String = row.get(3)?;
        let root_created_at: i64 = row.get(4)?;
        let root_id: i64 = row.get(5)?;
        // display 语义 = 原 SQL `CASE WHEN d.rel_path='' THEN r.path ELSE r.path||'/'||d.rel_path END`。
        let display = if rel_path.is_empty() {
            root_path
        } else {
            format!("{root_path}/{rel_path}")
        };
        Ok((
            id,
            DirLabel {
                rel_path,
                display,
                name,
                root_created_at,
                root_id,
            },
        ))
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}
