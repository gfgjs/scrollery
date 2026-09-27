//! 导出(方案 A §2.1/§2.3)批量元数据查询:分块取路径三段 + 用户态字段 + tags/albums 名称。
//! 供 `export::core` 组装文件名与 manifest 用,不经 IPC 直接序列化。

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;

/// 单批元数据 IN 查询的分块大小(方案 §2.1:「建议每批 500～1000」)。与 `layout::SELECTION_BATCH_CHUNK`
/// (5000,选区 id 解析用,不同用途)刻意不共用——查询这里每行还要展开路径 JOIN + 两个从表,
/// 单批不宜与选区批一样大。
pub const EXPORT_METADATA_CHUNK: usize = 750;

/// 单个导出项的完整元数据(路径三段 + 用户态字段 + tags/albums 名称)。
#[derive(Debug, Clone)]
pub struct ExportItemMeta {
    pub id: i64,
    pub root_path: String,
    pub root_alias: Option<String>,
    pub rel_path: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_mtime: i64,
    pub sort_datetime: i64,
    pub view_rotation: i64,
    pub rating: i64,
    pub color_label: i64,
    pub favorited: bool,
    /// 'online' | 'offline' | 'missing'(离线/缺失预检计数用)。
    pub availability: String,
    pub tags: Vec<String>,
    pub albums: Vec<String>,
}

fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",")
}

/// 批量取导出元数据(方案 §2.1 分块查询,不拼超长 `IN`、不逐项 N+1)。返回
/// `id -> ExportItemMeta` 映射;调用方按自己持有的顺序(`resolve_selection` 的结果序)重建列表——
/// 本函数不承诺返回顺序。`ids` 中不存在/已被并发删除的项在返回映射中直接缺失,调用方据此判「缺失」。
///
/// `include_relations`(审查 P2):预检只需计数(条数/估算体积/离线数/旋转数),不需要 tags/albums——
/// 关掉可省两段查询 + 两个 `Vec<String>` 物化;`start_export` 组装 manifest 才需要,传 `true`。
pub fn fetch_export_meta(
    conn: &Connection,
    ids: &[i64],
    include_relations: bool,
) -> Result<HashMap<i64, ExportItemMeta>> {
    let mut out: HashMap<i64, ExportItemMeta> = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(EXPORT_METADATA_CHUNK) {
        let ph = placeholders(chunk.len());
        let sql = format!(
            "SELECT m.id, r.path, r.alias, d.rel_path, m.file_name, m.file_size, m.file_mtime,
                    m.sort_datetime, m.view_rotation, m.rating, m.color_label, m.is_favorited,
                    m.availability
             FROM media_items m
             JOIN directories d ON d.id = m.directory_id
             JOIN scan_roots r ON r.id = d.root_id
             WHERE m.id IN ({ph})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            Ok(ExportItemMeta {
                id: row.get(0)?,
                root_path: row.get(1)?,
                root_alias: row.get(2)?,
                rel_path: row.get(3)?,
                file_name: row.get(4)?,
                file_size: row.get(5)?,
                file_mtime: row.get(6)?,
                sort_datetime: row.get(7)?,
                view_rotation: row.get(8)?,
                rating: row.get(9)?,
                color_label: row.get(10)?,
                favorited: row.get(11)?,
                availability: row.get(12)?,
                tags: Vec::new(),
                albums: Vec::new(),
            })
        })?;
        for row in rows {
            let meta = row?;
            out.insert(meta.id, meta);
        }

        if !include_relations {
            continue;
        }

        let tag_ph = placeholders(chunk.len());
        let tag_sql = format!(
            "SELECT it.item_id, t.name FROM item_tags it JOIN tags t ON t.id = it.tag_id
             WHERE it.item_id IN ({tag_ph}) ORDER BY it.item_id, t.name"
        );
        let mut tag_stmt = conn.prepare(&tag_sql)?;
        let tag_rows = tag_stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in tag_rows {
            let (item_id, name) = row?;
            if let Some(meta) = out.get_mut(&item_id) {
                meta.tags.push(name);
            }
        }

        let album_ph = placeholders(chunk.len());
        let album_sql = format!(
            "SELECT ai.item_id, a.name FROM album_items ai JOIN albums a ON a.id = ai.album_id
             WHERE ai.item_id IN ({album_ph}) ORDER BY ai.item_id, a.sort_order, a.name"
        );
        let mut album_stmt = conn.prepare(&album_sql)?;
        let album_rows = album_stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in album_rows {
            let (item_id, name) = row?;
            if let Some(meta) = out.get_mut(&item_id) {
                meta.albums.push(name);
            }
        }
    }
    Ok(out)
}
