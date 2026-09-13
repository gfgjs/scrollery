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

/// 读取某后端的完整配置（含 `cred_ref`）以构建 `StorageBackend`（§3.8）。
/// 返回 `(kind, host, base_path, username, cred_ref)`。
// 返回元组已在上方 doc 注明各字段语义，构建 StorageBackend 一次性消费，抽别名收益有限。
#[allow(clippy::type_complexity)]
pub fn get_storage_backend_config(
    conn: &Connection,
    id: i64,
) -> Result<
    Option<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )>,
> {
    conn.query_row(
        "SELECT kind, host, base_path, username, cred_ref FROM storage_backends WHERE id = ?1",
        params![id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )
    .optional()
    .map_err(AppError::from)
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

#[cfg(test)]
mod volume_dao_tests {
    use super::super::scan::{
        insert_scan_root, set_scan_root_volume, upsert_directory, upsert_fast_scan_item,
        FastScanItem,
    };
    use super::*;
    use crate::db::models::{NewVolume, VolumeKind};

    /// 全新内存库（含 V10 卷表）。FK 状态由各测试**显式**设置——FK 是建连接时（connection.rs）开，
    /// 非 run_migrations 副作用，故不依赖隐式默认。
    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c
    }

    fn new_vol(stable: &str) -> NewVolume {
        NewVolume {
            stable_id: stable.into(),
            label: Some("U盘".into()),
            kind: VolumeKind::Removable,
            last_mount_path: Some("E:\\".into()),
            last_seen: Some(1000),
            is_online: true,
        }
    }

    /// upsert：首次插入返回新 id；同 stable_id 再 upsert 返回**同一 id** 并更新字段（非新建行）。
    #[test]
    fn upsert_inserts_then_updates_on_stable_id_conflict() {
        let c = mem_db();
        let id1 = upsert_volume(&c, &new_vol("vol-A")).unwrap();

        let mut v2 = new_vol("vol-A");
        v2.label = Some("改名盘".into());
        v2.kind = VolumeKind::Network;
        v2.is_online = false;
        let id2 = upsert_volume(&c, &v2).unwrap();
        assert_eq!(id1, id2, "stable_id 冲突应更新同一行、返回同 id");

        let got = get_volume_by_stable_id(&c, "vol-A").unwrap().unwrap();
        assert_eq!(got.label.as_deref(), Some("改名盘"));
        assert!(matches!(got.kind, VolumeKind::Network), "kind 应被覆盖");
        assert!(!got.is_online, "is_online 应被覆盖为 false");
        assert_eq!(list_volumes(&c).unwrap().len(), 1, "不得新建第二行");

        // 未知 stable_id → None。
        assert!(get_volume_by_stable_id(&c, "missing").unwrap().is_none());
    }

    /// 端到端链（C5 Piece1+Piece2 组合）：新根建卷 + 绑定 volume_id → 该根新媒体继承卷。
    /// 证明新增扫描根的媒体能参与缺失检测守门1（修复前新根 volume_id 恒 NULL → 休眠）。
    #[test]
    fn new_root_binds_volume_and_media_inherits() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();

        // 1) 建根（add_scan_root 路径：backend_id=None）。
        let root_id = insert_scan_root(&c, "C:\\Photos", Some("照片"), None).unwrap();
        // 2) 建卷 + 绑定（add_scan_root 内 Piece2 逻辑）。
        let vid = upsert_volume(&c, &new_vol("path:C:")).unwrap();
        set_scan_root_volume(&c, root_id, Some(vid)).unwrap();

        // 回读：scan_roots.volume_id 已绑定。
        let bound: Option<i64> = c
            .query_row(
                "SELECT volume_id FROM scan_roots WHERE id=?1",
                params![root_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bound, Some(vid), "新根应绑定卷 id");

        // 3) 建目录 + 扫描插入一项（Piece1：upsert 传本根卷）。
        upsert_directory(&c, root_id, None, "", "Photos", 0, None).unwrap();
        let dir_id: i64 = c
            .query_row(
                "SELECT id FROM directories WHERE root_id=?1",
                params![root_id],
                |r| r.get(0),
            )
            .unwrap();
        let item = FastScanItem {
            directory_id: dir_id,
            file_name: "x.jpg".into(),
            file_size: 10,
            file_mtime: 100,
            file_mtime_ns: 0,
            file_format: "jpg".into(),
            media_type: "image".into(),
            width: 0,
            height: 0,
            sort_datetime: 100,
            cache_key: 0,
        };
        let out = upsert_fast_scan_item(&c, &item, bound).unwrap();
        let mid = out.id();

        // 媒体继承了本根卷 → 缺失检测守门1 可见。
        let media_vol: Option<i64> = c
            .query_row(
                "SELECT volume_id FROM media_items WHERE id=?1",
                params![mid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(media_vol, Some(vid), "新根的新媒体应继承本根卷 id");
    }

    /// set_volume_online：离线保留 last_seen + 挂载点；重连刷新 last_seen 且 mount_path=Some 时更新挂载点。
    #[test]
    fn set_volume_online_offline_keeps_last_known() {
        let c = mem_db();
        upsert_volume(&c, &new_vol("vol-B")).unwrap(); // last_seen=1000, mount=E:\

        // 离线：is_online=0，last_seen 不变，挂载点保留（mount_path=None）。
        set_volume_online(&c, "vol-B", false, None, 5000).unwrap();
        let off = get_volume_by_stable_id(&c, "vol-B").unwrap().unwrap();
        assert!(!off.is_online);
        assert_eq!(off.last_seen, Some(1000), "离线不得刷新 last_seen");
        assert_eq!(
            off.last_mount_path.as_deref(),
            Some("E:\\"),
            "离线保留最后已知挂载点"
        );

        // 重连到新盘符：is_online=1，last_seen=now，挂载点更新。
        set_volume_online(&c, "vol-B", true, Some("F:\\"), 9000).unwrap();
        let on = get_volume_by_stable_id(&c, "vol-B").unwrap().unwrap();
        assert!(on.is_online);
        assert_eq!(on.last_seen, Some(9000), "重连应刷新 last_seen");
        assert_eq!(
            on.last_mount_path.as_deref(),
            Some("F:\\"),
            "重连应更新挂载点"
        );
    }

    /// bulk_set_availability：整盘 online→offline 切换受影响行数正确，且**绝不触碰 is_deleted**（离线≠删除）。
    #[test]
    fn bulk_set_availability_never_touches_is_deleted() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap(); // 免构造 directory 链，直接插 media
        let vid = upsert_volume(&c, &new_vol("vol-C")).unwrap();
        for i in 1..=3 {
            c.execute(
                "INSERT INTO media_items
                    (id, directory_id, file_name, file_size, file_mtime, file_format,
                     media_type, width, height, sort_datetime, cache_key, volume_id, availability)
                 VALUES (?1, 1, ?2, 0, 0, 'jpg', 'image', 0, 0, 0, 0, ?3, 'online')",
                params![i, format!("{i}.jpg"), vid],
            )
            .unwrap();
        }

        let n = bulk_set_availability(&c, vid, "online", "offline").unwrap();
        assert_eq!(n, 3, "整盘 3 行应全部 online→offline");

        // 再切一次 online→offline：0 行（已无 online）。
        assert_eq!(
            bulk_set_availability(&c, vid, "online", "offline").unwrap(),
            0
        );

        // 离线≠删除：is_deleted 必须全为 0。
        let deleted: i64 = c
            .query_row(
                "SELECT count(*) FROM media_items WHERE is_deleted = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 0, "bulk_set_availability 绝不得写 is_deleted");
    }

    /// delete_volume：FK ON 时 `scan_roots.volume_id` 经 ON DELETE SET NULL 自动置空（不删 scan_root）。
    #[test]
    fn delete_volume_cascades_set_null_with_fk_on() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=ON;").unwrap(); // 显式开 FK 验级联
        let vid = upsert_volume(&c, &new_vol("vol-D")).unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias, volume_id) VALUES (1, '/r', 'R', ?1)",
            params![vid],
        )
        .unwrap();

        delete_volume(&c, vid).unwrap();

        assert!(
            get_volume_by_stable_id(&c, "vol-D").unwrap().is_none(),
            "卷应已删除"
        );
        let sr_vol: Option<i64> = c
            .query_row("SELECT volume_id FROM scan_roots WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            sr_vol.is_none(),
            "删卷后 scan_roots.volume_id 应 SET NULL（scan_root 本身保留）"
        );
    }

    /// 在某卷上插一条 media（指定 availability + is_deleted）。FK 需先关（免构造 directory 链）。
    fn seed_item(c: &Connection, id: i64, vol_id: i64, avail: &str, deleted: bool) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id,
                 availability, is_deleted)
             VALUES (?1, 1, ?2, 0,0,'jpg','image',0,0,0,0, ?3, ?4, ?5)",
            params![id, format!("{id}.jpg"), vol_id, avail, deleted as i64],
        )
        .unwrap();
    }

    /// 面板计数：LEFT JOIN 使零媒体卷也出现；item_count 只数未删除项（回收站项不计）。
    #[test]
    fn list_volumes_with_item_counts_excludes_deleted_and_keeps_empty_volumes() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        let v1 = upsert_volume(&c, &new_vol("vol-A")).unwrap();
        let v2 = upsert_volume(&c, &new_vol("vol-B")).unwrap(); // 零媒体卷
        seed_item(&c, 1, v1, "online", false);
        seed_item(&c, 2, v1, "offline", false);
        seed_item(&c, 3, v1, "online", true); // 回收站项，不计

        let infos = list_volumes_with_item_counts(&c).unwrap();
        assert_eq!(infos.len(), 2, "两个卷都应出现（含零媒体卷）");
        let c1 = infos.iter().find(|(v, _)| v.id == v1).unwrap().1;
        let c2 = infos.iter().find(|(v, _)| v.id == v2).unwrap().1;
        assert_eq!(c1, 2, "vol-A 未删项应为 2（排除 is_deleted）");
        assert_eq!(c2, 0, "vol-B 零媒体应计 0（LEFT JOIN 仍出现）");
    }

    /// 打开门控：卷离线返回 Some(标签)，在线 / 无绑定卷返回 None。
    #[test]
    fn get_item_volume_offline_label_gates_by_online_state() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // 离线卷（label 缺省用 stable_id 兜底路径另测；此处带 label）。
        let mut off = new_vol("vol-off");
        off.is_online = false;
        let voff = upsert_volume(&c, &off).unwrap();
        let von = upsert_volume(&c, &new_vol("vol-on")).unwrap(); // is_online=true
        seed_item(&c, 1, voff, "offline", false);
        seed_item(&c, 2, von, "online", false);
        // 无绑定卷的项（volume_id=0，JOIN 不中）。
        seed_item(&c, 3, 0, "online", false);

        assert_eq!(
            get_item_volume_offline_label(&c, 1).unwrap().as_deref(),
            Some("U盘"),
            "离线卷上的项应返回卷标签"
        );
        assert!(
            get_item_volume_offline_label(&c, 2).unwrap().is_none(),
            "在线卷上的项应返回 None"
        );
        assert!(
            get_item_volume_offline_label(&c, 3).unwrap().is_none(),
            "无绑定卷的项应返回 None"
        );
    }

    /// stable_id 兜底：卷 label 为 NULL 时，离线门控返回 stable_id。
    #[test]
    fn get_item_volume_offline_label_falls_back_to_stable_id() {
        let c = mem_db();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        let mut off = new_vol("vol-nolabel");
        off.label = None;
        off.is_online = false;
        let voff = upsert_volume(&c, &off).unwrap();
        seed_item(&c, 1, voff, "offline", false);
        assert_eq!(
            get_item_volume_offline_label(&c, 1).unwrap().as_deref(),
            Some("vol-nolabel"),
            "label 缺省应回退 stable_id"
        );
    }
}
