//! 存储域 DAO:storage backend(网络盘)+ 卷(volumes)与整盘可用性
//! (T 线拆分自 queries.rs,SQL 与行为不变;「离线≠删除」硬规则的数据侧落点)。

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::db::models::{NewVolume, StorageBackendInfo, Volume, VolumeKind};
use crate::error::{AppError, Result};

fn map_volume(row: &Row<'_>) -> rusqlite::Result<Volume> {
    Ok(Volume {
        id: row.get(0)?,
        stable_id: row.get(1)?,
        label: row.get(2)?,
        // kind 读宽容：未知字符串归 Unknown（防御旧库 / 未来类型，物理清理 fail closed）。
        kind: VolumeKind::from_str_lossy(&row.get::<_, String>(3)?),
        last_mount_path: row.get(4)?,
        last_seen: row.get(5)?,
        is_online: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

// ── 存储后端（网络盘，§3.8 8B）─────────────────────────────────────────────────

fn map_storage_backend(row: &Row<'_>) -> rusqlite::Result<StorageBackendInfo> {
    let cred_ref: Option<String> = row.get(6)?;
    Ok(StorageBackendInfo {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        host: row.get(3)?,
        base_path: row.get(4)?,
        username: row.get(5)?,
        has_password: cred_ref.is_some(),
        created_at: row.get(7)?,
    })
}

/// 列出所有已配置的存储后端（§3.8）。密码绝不返回（仅 `has_password`）。
pub fn list_storage_backends(conn: &Connection) -> Result<Vec<StorageBackendInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, host, base_path, username, cred_ref, created_at
         FROM storage_backends ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], map_storage_backend)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 插入一个存储后端（密码另存 keyring；此处仅 `cred_ref`）。§3.8。
#[allow(clippy::too_many_arguments)]
pub fn insert_storage_backend(
    conn: &Connection,
    kind: &str,
    name: &str,
    host: Option<&str>,
    base_path: Option<&str>,
    username: Option<&str>,
    cred_ref: Option<&str>,
    options: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO storage_backends (kind, name, host, base_path, username, cred_ref, options)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![kind, name, host, base_path, username, cred_ref, options],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 按 id 删除存储后端；返回其 `cred_ref` 以便调用方清理 keyring。§3.8。
pub fn delete_storage_backend(conn: &Connection, id: i64) -> Result<Option<String>> {
    let cred_ref: Option<String> = conn
        .query_row(
            "SELECT cred_ref FROM storage_backends WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    conn.execute("DELETE FROM storage_backends WHERE id = ?1", params![id])?;
    Ok(cred_ref)
}

// ── 卷可用性 DAO（SCHEMA_V10；供 Part2 卷探测 probe_volumes / 扫描编排调用）─────────
//
// 🔴 离线≠删除硬规则的数据侧落点：`bulk_set_availability` 是仅有的合法整盘状态切换；
// 本模块**不提供**「绕过卷判断的批量删除」DAO——差集删除的 SQL 守门在 Part2。

/// upsert 卷：`stable_id` 冲突则更新 label/kind/last_mount_path/last_seen/is_online（保留 id/created_at）。
/// 返回卷 id（新建或既有）。用 RETURNING 一次拿回 id（与仓内既有 upsert 范式一致；rusqlite bundled SQLite 支持）。
pub fn upsert_volume(conn: &Connection, v: &NewVolume) -> Result<i64> {
    let id = conn.query_row(
        "INSERT INTO volumes (stable_id, label, kind, last_mount_path, last_seen, is_online)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(stable_id) DO UPDATE SET
             label           = excluded.label,
             kind            = excluded.kind,
             last_mount_path = excluded.last_mount_path,
             last_seen       = excluded.last_seen,
             is_online       = excluded.is_online
         RETURNING id",
        params![
            v.stable_id,
            v.label,
            v.kind.as_str(),
            v.last_mount_path,
            v.last_seen,
            v.is_online as i64,
        ],
        |row| row.get(0),
    )?;
    Ok(id)
}

/// 按 stable_id 取卷（不存在返回 None）。
pub fn get_volume_by_stable_id(conn: &Connection, stable_id: &str) -> Result<Option<Volume>> {
    conn.query_row(
        "SELECT id, stable_id, label, kind, last_mount_path, last_seen, is_online, created_at
         FROM volumes WHERE stable_id = ?1",
        params![stable_id],
        map_volume,
    )
    .optional()
    .map_err(AppError::from)
}

/// 列出全部卷（设置「已知卷」面板用）。按登记时间稳定排序。
pub fn list_volumes(conn: &Connection) -> Result<Vec<Volume>> {
    let mut stmt = conn.prepare(
        "SELECT id, stable_id, label, kind, last_mount_path, last_seen, is_online, created_at
         FROM volumes ORDER BY created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([], map_volume)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 切换卷在线态（probe 刷新）。online 时刷新 `last_seen`；`mount_path` 为 Some 时更新挂载点，
/// 为 None 时保留最后已知挂载点（离线后仍可向用户提示「上次在 X:」）。
pub fn set_volume_online(
    conn: &Connection,
    stable_id: &str,
    online: bool,
    mount_path: Option<&str>,
    now: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE volumes SET
             is_online       = ?2,
             last_seen       = CASE WHEN ?2 = 1 THEN ?3 ELSE last_seen END,
             last_mount_path = COALESCE(?4, last_mount_path)
         WHERE stable_id = ?1",
        params![stable_id, online as i64, now, mount_path],
    )?;
    Ok(())
}

/// 重命名卷标（用户在「已知卷」面板改名）。
pub fn rename_volume_label(conn: &Connection, volume_id: i64, label: &str) -> Result<()> {
    conn.execute(
        "UPDATE volumes SET label = ?2 WHERE id = ?1",
        params![volume_id, label],
    )?;
    Ok(())
}

/// 删除卷登记。依赖 FK `ON DELETE SET NULL`（需连接开启 `PRAGMA foreign_keys=ON`，迁移已启用）：
/// `scan_roots.volume_id` / `media_items.volume_id` 自动置 NULL；**是否软删媒体由调用方决定**，本 DAO 不删媒体。
pub fn delete_volume(conn: &Connection, volume_id: i64) -> Result<()> {
    conn.execute("DELETE FROM volumes WHERE id = ?1", params![volume_id])?;
    Ok(())
}

/// 列出全部卷 + 各卷「未删除媒体数」（「已知卷」面板展示用）。
/// LEFT JOIN 保证零媒体的卷也出现；`FILTER (is_deleted=0)` 排除回收站项（离线≠删除，回收站另计）。
pub fn list_volumes_with_item_counts(conn: &Connection) -> Result<Vec<(Volume, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT v.id, v.stable_id, v.label, v.kind, v.last_mount_path, v.last_seen,
                v.is_online, v.created_at,
                COUNT(m.id) FILTER (WHERE m.is_deleted = 0) AS item_count
         FROM volumes v
         LEFT JOIN media_items m ON m.volume_id = v.id
         GROUP BY v.id
         ORDER BY v.created_at ASC, v.id ASC",
    )?;
    let rows = stmt.query_map([], |row| Ok((map_volume(row)?, row.get::<_, i64>(8)?)))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 判定某媒体项当前是否「所在卷离线」：离线返回 `Some(卷标签)`（label 缺省用 stable_id 兜底），
/// 在线 / 无绑定卷返回 `None`。供打开原图/视频等实体访问的 IPC 门控（离线即返 `VolumeOffline`，非破图）。
pub fn get_item_volume_offline_label(conn: &Connection, item_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT COALESCE(v.label, v.stable_id)
         FROM media_items m JOIN volumes v ON m.volume_id = v.id
         WHERE m.id = ?1 AND v.is_online = 0",
        params![item_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(AppError::from)
}

/// 批量切换整盘可用性（拔出 `'online'→'offline'` / 重连 `'offline'→'online'`），单字段过滤、走 `idx_media_volume`。
/// 返回受影响行数。**绝不**在此写 `is_deleted`——离线≠删除（Part1 §3.3c 硬规则）。
pub fn bulk_set_availability(
    conn: &Connection,
    volume_id: i64,
    from: &str,
    to: &str,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE media_items SET availability = ?3 WHERE volume_id = ?1 AND availability = ?2",
        params![volume_id, from, to],
    )?)
}
