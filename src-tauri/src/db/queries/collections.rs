//! Collections(albums/album_items 收藏夹)DAO(T 线拆分自 queries.rs,SQL 与行为不变)。

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::scan::EXCLUDE_HIDDEN_ROOTS_M;
use crate::db::models::Collection;
use crate::error::{AppError, Result};

// ── 收藏夹（需求7, §3.7） ──────────────────────────────────────────────────────
//
// 复用 albums/album_items，不另造机制。系统夹（kind='system'）成员虚拟：
//   该类型 + is_favorited（走 idx_media_fav 快路径，红心即收藏，无需写 album_items）。
// 用户夹（kind='user'）成员实体：存 album_items，可跨类型混装。
// list_collections 用 CASE 分别计算两类的 cover/count。

fn map_collection(row: &Row<'_>) -> rusqlite::Result<Collection> {
    Ok(Collection {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        media_type_filter: row.get(3)?,
        icon: row.get(4)?,
        cover_item_id: row.get(5)?,
        item_count: row.get(6)?,
        sort_order: row.get(7)?,
    })
}

/// 列出所有收藏夹（系统夹在前、用户夹在后），各带计算出的封面项与成员数。
pub fn list_collections(conn: &Connection) -> Result<Vec<Collection>> {
    // 隐藏根排除(V21):卡片封面/计数与夹内列表(ViewScope::Collection,经 layout 排除)同口径,
    // 否则隐藏根图片会直接当封面泄漏、计数与打开后的内容不符。冷路径(侧栏刷新),静态谓词
    // 无条件注入(空集全通过),与流水线侧同款。
    // 用户夹显式封面 a.cover_item_id 不能裸走 COALESCE(会短路绕过排除),改经 media_items 校验:
    // 封面被隐藏(或 id 悬空)时回退到最新可见成员。
    let mut stmt = conn.prepare(&format!(
        "SELECT a.id, a.name, a.kind, a.media_type_filter, a.icon,
            CASE WHEN a.kind='system'
                 THEN (SELECT m.id FROM media_items m
                       WHERE m.media_type=a.media_type_filter AND m.is_favorited=1 AND m.is_deleted=0
                         {EXCLUDE_HIDDEN_ROOTS_M}
                       ORDER BY m.sort_datetime DESC LIMIT 1)
                 ELSE COALESCE(
                       (SELECT m.id FROM media_items m
                        WHERE m.id=a.cover_item_id {EXCLUDE_HIDDEN_ROOTS_M}),
                       (SELECT ai.item_id FROM album_items ai
                        JOIN media_items m ON ai.item_id=m.id
                        WHERE ai.album_id=a.id AND m.is_deleted=0 {EXCLUDE_HIDDEN_ROOTS_M}
                        ORDER BY ai.added_at DESC LIMIT 1))
            END AS cover_item_id,
            CASE WHEN a.kind='system'
                 THEN (SELECT COUNT(*) FROM media_items m
                       WHERE m.media_type=a.media_type_filter AND m.is_favorited=1 AND m.is_deleted=0
                         {EXCLUDE_HIDDEN_ROOTS_M})
                 ELSE (SELECT COUNT(*) FROM album_items ai
                       JOIN media_items m ON ai.item_id=m.id
                       WHERE ai.album_id=a.id AND m.is_deleted=0 {EXCLUDE_HIDDEN_ROOTS_M})
            END AS item_count,
            a.sort_order
         FROM albums a
         WHERE a.deleted_at IS NULL
         ORDER BY (a.kind='user'), a.sort_order, a.id"
    ))?;
    let rows = stmt.query_map([], map_collection)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 列出已软删除的用户收藏夹（最近删除在前）——让 `deleted_at` 在内存 undo 栈随重启消失后仍可捞回。
///
/// 与 `list_collections` 的取反关系:那边 `deleted_at IS NULL`，这边 `IS NOT NULL`，两者不重不漏。
/// 系统夹永不会被软删(`delete_collection` 有 `kind='user'` 守卫)，故无需 system 分支的 cover/count
/// CASE，直接走用户夹口径(同 `recent_collections`)。
///
/// **无索引是有意的**:`media_items` 的回收站有 `idx_media_trash`(百万行量级、keyset 翻页)，albums
/// 是数十行量级、一次全取，加部分索引只增写开销。二者的另一个共同点是**都不 purge**——软删行永久
/// 保留、恒可恢复(2026-07-16 裁决:只补读路径，不做定期清理)。
pub fn list_deleted_collections(conn: &Connection) -> Result<Vec<Collection>> {
    // 隐藏根排除(V21):口径同 list_collections(封面经校验、计数排隐藏根)。
    let mut stmt = conn.prepare(&format!(
        "SELECT a.id, a.name, a.kind, a.media_type_filter, a.icon,
            COALESCE((SELECT m.id FROM media_items m
                      WHERE m.id=a.cover_item_id {EXCLUDE_HIDDEN_ROOTS_M}),
                     (SELECT ai.item_id FROM album_items ai
                      JOIN media_items m ON ai.item_id=m.id
                      WHERE ai.album_id=a.id AND m.is_deleted=0 {EXCLUDE_HIDDEN_ROOTS_M}
                      ORDER BY ai.added_at DESC LIMIT 1)) AS cover_item_id,
            (SELECT COUNT(*) FROM album_items ai
             JOIN media_items m ON ai.item_id=m.id
             WHERE ai.album_id=a.id AND m.is_deleted=0 {EXCLUDE_HIDDEN_ROOTS_M}) AS item_count,
            a.sort_order
         FROM albums a
         WHERE a.kind='user' AND a.deleted_at IS NOT NULL
         ORDER BY a.deleted_at DESC, a.id DESC"
    ))?;
    let rows = stmt.query_map([], map_collection)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 用户收藏夹按最近使用（最新加入成员）排序，用于「加入收藏夹」toast 快捷 chips，限 `limit` 个。
pub fn recent_collections(conn: &Connection, limit: i64) -> Result<Vec<Collection>> {
    // 隐藏根排除(V21):toast chips 的计数同口径;封面 id 经 media_items 校验(隐藏则置空,
    // chips 无成员回退需求,空封面由前端占位)。
    let mut stmt = conn.prepare(&format!(
        "SELECT a.id, a.name, a.kind, a.media_type_filter, a.icon,
            (SELECT m.id FROM media_items m
             WHERE m.id=a.cover_item_id {EXCLUDE_HIDDEN_ROOTS_M}) AS cover_item_id,
            (SELECT COUNT(*) FROM album_items ai JOIN media_items m ON ai.item_id=m.id
             WHERE ai.album_id=a.id AND m.is_deleted=0 {EXCLUDE_HIDDEN_ROOTS_M}) AS item_count,
            a.sort_order
         FROM albums a
         WHERE a.kind='user' AND a.deleted_at IS NULL
         ORDER BY COALESCE((SELECT MAX(added_at) FROM album_items WHERE album_id=a.id), a.created_at) DESC,
                  a.id DESC
         LIMIT ?1"
    ))?;
    let rows = stmt.query_map(params![limit], map_collection)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 新建一个用户收藏夹。返回新 album id。
pub fn create_collection(conn: &Connection, name: &str, icon: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO albums (name, kind, icon) VALUES (?1, 'user', ?2)",
        params![name, icon],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 软删除一个用户收藏夹（系统夹受保护）：置 `deleted_at` 而非删行，使删除可撤销（成员保留）。
/// `list_collections` / `recent_collections` 过滤 `deleted_at IS NULL`，故夹在 UI 消失但
/// `album_items` 保留，供 `restore_collection` 还原；`deleted_at IS NULL` 守卫使重复删除为空操作
///（不覆盖首次删除时间戳）。
pub fn delete_collection(conn: &Connection, album_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE albums SET deleted_at=strftime('%s','now')
         WHERE id=?1 AND kind='user' AND deleted_at IS NULL",
        params![album_id],
    )?;
    Ok(())
}

/// 撤销软删除的用户收藏夹（承接删除 undo）。清 `deleted_at`，夹重新出现在列表中。系统夹受 `kind='user'` 守卫保护、为空操作。
pub fn restore_collection(conn: &Connection, album_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE albums SET deleted_at=NULL WHERE id=?1 AND kind='user'",
        params![album_id],
    )?;
    Ok(())
}

/// 重命名一个用户收藏夹（`kind='user'` 守卫保护系统夹不被改名）。非用户夹为空操作。
pub fn rename_collection(conn: &Connection, album_id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE albums SET name=?2 WHERE id=?1 AND kind='user'",
        params![album_id, name],
    )?;
    Ok(())
}

/// 向用户收藏夹添加项（`INSERT OR IGNORE`）。系统夹/不存在的夹为空操作（系统成员虚拟，由
/// `is_favorited` 驱动）。返回插入行数。
pub fn add_to_collection(conn: &Connection, album_id: i64, item_ids: &[i64]) -> Result<usize> {
    if item_ids.is_empty() {
        return Ok(0);
    }
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM albums WHERE id=?1",
            params![album_id],
            |r| r.get(0),
        )
        .optional()?;
    if kind.as_deref() != Some("user") {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0usize;
    for &id in item_ids {
        inserted += tx.execute(
            "INSERT OR IGNORE INTO album_items (album_id, item_id) VALUES (?1, ?2)",
            params![album_id, id],
        )?;
    }
    tx.commit()?;
    Ok(inserted)
}

/// 从收藏夹移除项。返回删除行数。
pub fn remove_from_collection(conn: &Connection, album_id: i64, item_ids: &[i64]) -> Result<usize> {
    if item_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut deleted = 0usize;
    for &id in item_ids {
        deleted += tx.execute(
            "DELETE FROM album_items WHERE album_id=?1 AND item_id=?2",
            params![album_id, id],
        )?;
    }
    tx.commit()?;
    Ok(deleted)
}

// ── 隐藏根排除(V21 收藏夹墙)──────────────────────────────────────────────────
// 锁卡片封面/计数与夹内列表(ViewScope::Collection,经 layout 排除)同口径:系统夹(虚拟
// 成员)与用户夹(实体成员+显式封面 COALESCE 短路)都不得把隐藏根媒体当封面/计入计数。
