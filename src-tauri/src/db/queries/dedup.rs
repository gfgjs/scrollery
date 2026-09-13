//! 精确内容去重查询。
//!
//! 这里仅负责短事务 SQL 和分页投影。摘要计算、路径访问和系统回收站操作必须在调用方
//! 释放数据库 writer 锁后执行；所有摘要写回均以 `(item_id, source_revision)` 为守门条件。

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};

use crate::dedup::DEDUP_HASH_VERSION;
use crate::error::Result;
use crate::utils::path::resolve_media_path;

/// 去重摘要索引的当前可用状态值。
pub const STATUS_QUICK: &str = "quick";
pub const STATUS_READY: &str = "ready";
pub const STATUS_STALE: &str = "stale";
pub const STATUS_UNSTABLE: &str = "unstable";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_ERROR: &str = "error";

/// 去重分析的 keyset 游标。候选按 `(file_size,item_id)` 排序；分组按
/// `(unit_digest,unit_size,first_item_id)` 排序。两种游标故意分成不同类型，防止调用方把
/// 成员游标误传给组查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupCandidateCursor {
    pub file_size: i64,
    pub item_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroupCursor {
    pub unit_digest: Vec<u8>,
    pub unit_size: i64,
    pub first_item_id: i64,
}

/// 分析任务需要的文件位置快照。`path` 仅由数据库中的可信 root/rel/file 三段在 Rust 侧
/// 拼出；它不接受 WebView 输入。
#[derive(Debug, Clone)]
pub struct DedupScanCandidate {
    pub item_id: i64,
    pub directory_id: i64,
    pub file_name: String,
    pub path: String,
    pub file_size: i64,
    pub file_mtime: i64,
    pub file_mtime_ns: Option<i64>,
    pub source_revision: i64,
    pub availability: String,
    pub is_live_photo: bool,
}

/// 一个 Live Photo companion 的数据库位置。
#[derive(Debug, Clone)]
pub struct DedupCompanion {
    pub item_id: i64,
    pub path: String,
    pub file_size: i64,
    pub file_mtime_ns: Option<i64>,
    pub source_revision: i64,
}

/// 去重组的聚合行。`potential_logical_bytes` 是逻辑重复字节，不是硬链接/稀疏文件下的
/// 实际可回收空间。
#[derive(Debug, Clone)]
pub struct DuplicateGroupRow {
    pub unit_digest: Vec<u8>,
    pub unit_size: i64,
    pub member_count: i64,
    pub potential_logical_bytes: i64,
    pub first_item_id: i64,
    /// 组内收藏、评分或颜色标签不一致时为 true；只提示人工复核，不自动合并。
    pub metadata_conflict: bool,
    /// 按在线状态和保护元数据确定的建议 keeper；调用方仍必须让用户确认。
    pub suggested_keeper_id: i64,
}

/// 去重组成员投影。保护标记只读展示，清理时仍必须重新查询并复核。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateMemberRow {
    pub item_id: i64,
    pub source_revision: i64,
    pub file_name: String,
    pub path: String,
    pub file_size: i64,
    pub file_mtime: i64,
    pub file_mtime_ns: Option<i64>,
    pub availability: String,
    pub is_favorited: bool,
    pub rating: i64,
    pub color_label: i64,
    pub is_live_photo: bool,
    pub physical_key: Option<Vec<u8>>,
    pub album_count: i64,
    pub tag_count: i64,
    pub bookmark_count: i64,
}

/// 文件夹范围的动态聚合统计。`retained_positions` 只供候选排序使用，不作为 IPC 字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderStatsRow {
    pub folder_id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    pub total_positions: i64,
    pub analyzed_positions: i64,
    pub duplicate_positions: i64,
    pub external_covered_positions: i64,
    pub internal_duplicate_positions: i64,
    pub unreviewed_positions: i64,
    pub protected_positions: i64,
    pub recommended_positions: i64,
    pub recommended_logical_bytes: i64,
    pub retained_positions: i64,
}

/// 候选排序使用的整数游标。覆盖率的分子/分母保留为整数，不把浮点值作为游标身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderCandidateCursor {
    pub actionable: i64,
    pub recommended_positions: i64,
    pub total_positions: i64,
    pub external_covered_positions: i64,
    pub recommended_logical_bytes: i64,
    pub retained_positions: i64,
    pub depth: i64,
    pub folder_id: i64,
}

/// 文件夹树按名称分页的游标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderTreeCursor {
    pub name: String,
    pub folder_id: i64,
}

/// 文件夹范围内一个画廊项的最小数据库投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderItemRow {
    pub item_id: i64,
    pub source_revision: i64,
    pub directory_id: i64,
    pub file_name: String,
    pub directory_path: String,
    pub file_size: i64,
    pub media_type: String,
    pub width: i64,
    pub height: i64,
    pub duration_ms: Option<i64>,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
    pub is_live_photo: bool,
    pub availability: String,
    pub is_favorited: bool,
    pub rating: i64,
    pub color_label: i64,
    pub album_count: i64,
    pub tag_count: i64,
    pub bookmark_count: i64,
    pub unit_digest: Option<Vec<u8>>,
    pub unit_size: Option<i64>,
    pub physical_key: Option<Vec<u8>>,
    pub group_count: Option<i64>,
    pub scoped_group_count: Option<i64>,
    pub suggested_keeper_id: Option<i64>,
    pub analysis_ready: bool,
    pub physical_alias: bool,
}

/// 目标范围计划生成所需的成员投影；它不会直接下发到前端。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderPlanMemberRow {
    pub item_id: i64,
    pub directory_id: i64,
    pub source_revision: i64,
    pub in_target: bool,
    pub availability: String,
    pub is_favorited: bool,
    pub rating: i64,
    pub color_label: i64,
    pub album_count: i64,
    pub tag_count: i64,
    pub bookmark_count: i64,
    pub physical_key: Option<Vec<u8>>,
}

/// 与目标范围相交的完整精确组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderPlanGroupRow {
    pub unit_digest: Vec<u8>,
    pub unit_size: i64,
    pub suggested_keeper_id: Option<i64>,
    pub members: Vec<DuplicateFolderPlanMemberRow>,
}

/// 用于把外部覆盖项归并到互斥来源目录的目录元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFolderDirectoryRow {
    pub id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub rel_path: String,
    pub name: String,
    pub depth: i64,
}

/// 一个精确摘要写回。`None` 表示清除该阶段摘要；BLOB 不转十六进制文本。
#[derive(Debug, Clone)]
pub struct DedupIndexUpdate<'a> {
    pub item_id: i64,
    pub source_revision: i64,
    /// hash 开始前从 media_items 读取的 size/mtime 快照；两者都必须在写回时仍相同。
    pub file_size: i64,
    pub file_mtime_ns: Option<i64>,
    pub hash_version: i64,
    pub quick_digest: Option<&'a [u8]>,
    pub exact_digest: Option<&'a [u8]>,
    pub unit_digest: Option<&'a [u8]>,
    pub unit_size: Option<i64>,
    pub physical_key: Option<&'a [u8]>,
    pub status: &'a str,
    pub error_code: Option<&'a str>,
}

/// 一批摘要写入前的数据库状态守门。`companion_of` 只在 Live Photo 组合写入时填写；
/// 普通主项使用 `None`，仍要求该行是当前非 companion 主项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupWriteCheck {
    pub item_id: i64,
    pub source_revision: i64,
    pub file_size: i64,
    pub file_mtime_ns: Option<i64>,
    pub companion_of: Option<i64>,
    /// 若已有 sidecar 物理身份，则写回必须仍属于同一物理文件对象。
    pub physical_key: Option<Vec<u8>>,
}

fn candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DedupScanCandidate> {
    let root_path: String = row.get(10)?;
    let rel_path: String = row.get(11)?;
    let file_name: String = row.get(2)?;
    Ok(DedupScanCandidate {
        item_id: row.get(0)?,
        directory_id: row.get(1)?,
        path: resolve_media_path(&root_path, &rel_path, &file_name),
        file_name,
        file_size: row.get(3)?,
        file_mtime: row.get(4)?,
        file_mtime_ns: row.get(5)?,
        source_revision: row.get(6)?,
        availability: row.get(7)?,
        is_live_photo: row.get::<_, i64>(8)? != 0,
    })
}

const CANDIDATE_SELECT: &str = "
    SELECT m.id, m.directory_id, m.file_name, m.file_size, m.file_mtime,
           m.file_mtime_ns, m.source_revision, m.availability, m.is_live_photo,
           m.companion_of, r.path, d.rel_path
      FROM media_items m
      JOIN directories d ON d.id = m.directory_id
      JOIN scan_roots r ON r.id = d.root_id
     WHERE m.is_deleted = 0
       AND m.companion_of IS NULL
       AND m.availability NOT IN ('missing', 'offline')
       AND r.is_active = 1
       AND r.is_hidden = 0
       AND m.file_size IN (
             SELECT m2.file_size
               FROM media_items m2
               JOIN directories d2 ON d2.id = m2.directory_id
               JOIN scan_roots r2 ON r2.id = d2.root_id
              WHERE m2.is_deleted = 0
                AND m2.companion_of IS NULL
                AND m2.availability NOT IN ('missing', 'offline')
                AND r2.is_active = 1
                AND r2.is_hidden = 0
              GROUP BY m2.file_size
               HAVING COUNT(*) > 1
       )";

fn pending_candidate_select() -> String {
    // 有效 quick/ready 行已经是可续跑的持久结果，不必在下一轮重复读取；源修订或
    // hash 版本变化会让这条 NOT EXISTS 自动失效，重新进入待分析集合。版本是编译期
    // 常量，游标/其它输入仍全部走绑定参数。
    format!(
        "{CANDIDATE_SELECT}
       AND NOT EXISTS (
             SELECT 1
               FROM dedup_index_working di_pending
              WHERE di_pending.item_id = m.id
                AND di_pending.source_revision = m.source_revision
                AND di_pending.hash_version = {}
                AND di_pending.status IN ('quick', 'ready')
       )",
        DEDUP_HASH_VERSION
    )
}

/// 返回下一批需要分析的主项。排序和游标只依赖稳定数值，不构造全库 ID 数组。
pub fn list_dedup_scan_candidates(
    conn: &Connection,
    cursor: Option<&DedupCandidateCursor>,
    limit: usize,
) -> Result<Vec<DedupScanCandidate>> {
    let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
    let mut sql = pending_candidate_select();
    if cursor.is_some() {
        sql.push_str(" AND (m.file_size > ?1 OR (m.file_size = ?1 AND m.id > ?2))");
    }
    sql.push_str(if cursor.is_some() {
        " ORDER BY m.file_size ASC, m.id ASC LIMIT ?3"
    } else {
        " ORDER BY m.file_size ASC, m.id ASC LIMIT ?1"
    });
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(cursor) = cursor {
        stmt.query_map(
            params![cursor.file_size, cursor.item_id, limit],
            candidate_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(params![limit], candidate_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

/// 返回候选总数，用于进度快照；只 count，不加载任何 ID。
pub fn count_dedup_scan_candidates(conn: &Connection) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) FROM ({})", pending_candidate_select());
    let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}

const MAX_DEDUP_COMPANIONS: usize = 64;

/// 返回一个主项的全部 companion，顺序由 item_id 稳定决定。companion 绝不作为独立组成员。
pub fn list_dedup_companions(conn: &Connection, item_id: i64) -> Result<Vec<DedupCompanion>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.file_name, c.file_size, c.file_mtime_ns, c.source_revision,
                r.path, d.rel_path
           FROM media_items c
           JOIN directories d ON d.id = c.directory_id
           JOIN scan_roots r ON r.id = d.root_id
          WHERE c.companion_of = ?1
          ORDER BY c.id ASC
          LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(
            params![
                item_id,
                i64::try_from(MAX_DEDUP_COMPANIONS + 1).unwrap_or(i64::MAX)
            ],
            |row| {
                let root_path: String = row.get(5)?;
                let rel_path: String = row.get(6)?;
                let file_name: String = row.get(1)?;
                Ok(DedupCompanion {
                    item_id: row.get(0)?,
                    path: resolve_media_path(&root_path, &rel_path, &file_name),
                    file_size: row.get(2)?,
                    file_mtime_ns: row.get(3)?,
                    source_revision: row.get(4)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.len() > MAX_DEDUP_COMPANIONS {
        // Live Photo 目前只有一个 companion。拒绝异常关系集合而不是无限物化，
        // 这样损坏数据库不会把后台分析推成无界内存任务。
        return Err(rusqlite::Error::InvalidQuery.into());
    }
    Ok(rows)
}

/// 开启一轮分析的数据库代次。每轮先重建 working 快照：续跑复制上一版已发布结果，
/// reset 则从空表开始。主画廊始终只读 `dedup_index`。
pub fn begin_dedup_run(conn: &Connection, generation: u64, reset: bool) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let current: Option<String> = tx
        .query_row(
            "SELECT value FROM app_config WHERE key='dedup_run_generation'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if current
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|current| current > generation)
    {
        // 旧线程可能在新线程开始后才真正进入 writer。它绝不能把 generation 倒退，
        // 也绝不能执行 reset 清掉新轮结果；空事务提交后由适配层报告 stale。
        tx.commit()?;
        return Ok(false);
    }
    tx.execute(
        "INSERT OR REPLACE INTO app_config(key, value) VALUES ('dedup_run_generation', ?1)",
        params![generation.to_string()],
    )?;
    tx.execute("DELETE FROM dedup_index_working", [])?;
    if !reset {
        tx.execute(
            "INSERT INTO dedup_index_working
                (item_id, source_revision, hash_version, quick_digest, exact_digest, unit_digest,
                 unit_size, physical_key, status, error_code, checked_at)
             SELECT item_id, source_revision, hash_version, quick_digest, exact_digest, unit_digest,
                    unit_size, physical_key, status, error_code, checked_at
               FROM dedup_index",
            [],
        )?;
    }
    tx.commit()?;
    Ok(true)
}

/// 使指定轮次及其所有迟到写入失效。调用方应在 stop 返回前执行，避免取消与旧
/// sidecar 写入之间留下可观察窗口；值保持单调，后续新轮可以用自己的 generation 接管。
pub fn invalidate_dedup_run(conn: &Connection, generation: u64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let current: Option<String> = tx
        .query_row(
            "SELECT value FROM app_config WHERE key='dedup_run_generation'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let current = current
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let invalidated = current.max(generation.saturating_add(1));
    tx.execute(
        "INSERT OR REPLACE INTO app_config(key, value) VALUES ('dedup_run_generation', ?1)",
        params![invalidated.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

/// 在同一个 writer 事务内确认一批摘要的源代次、删除状态、Live Photo 关系和分析代次。
/// 调用方必须在返回 `true` 后才写入任何一行，这样主项/companion 不会出现半组提交。
pub fn dedup_write_batch_current(
    conn: &Connection,
    generation: u64,
    checks: &[DedupWriteCheck],
) -> Result<bool> {
    if checks.is_empty() {
        return Ok(true);
    }
    let current_generation = dedup_run_generation(conn)?;
    if current_generation != Some(generation) {
        return Ok(false);
    }
    let mut stmt = conn.prepare(
        "SELECT m.source_revision, m.is_deleted, m.companion_of, m.file_size, m.file_mtime_ns,
                di.physical_key
           FROM media_items AS m
           LEFT JOIN dedup_index_working AS di ON di.item_id=m.id
          WHERE m.id=?1",
    )?;
    for check in checks {
        let state = stmt
            .query_row(params![check.item_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<Vec<u8>>>(5)?,
                ))
            })
            .optional()?;
        let Some((
            source_revision,
            is_deleted,
            companion_of,
            file_size,
            file_mtime_ns,
            current_physical_key,
        )) = state
        else {
            return Ok(false);
        };
        if source_revision != check.source_revision
            || is_deleted != 0
            || companion_of != check.companion_of
            || file_size != check.file_size
            || check.file_mtime_ns.is_none()
            || file_mtime_ns != check.file_mtime_ns
            || current_physical_key.is_some()
                && check.physical_key.as_ref() != current_physical_key.as_ref()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 仅在来源快照和分析代次仍有效时写入索引。
/// 代次检查和 upsert 在同一 writer 临界区内完成，因此旧任务无法在新任务安装后产生
/// side effect；返回 false 与来源快照失效统一表示“结果丢弃”。
pub fn write_dedup_index_if_generation_current(
    conn: &Connection,
    update: &DedupIndexUpdate<'_>,
    generation: u64,
) -> Result<bool> {
    let changed = conn.execute(
        "INSERT INTO dedup_index_working
            (item_id, source_revision, hash_version, quick_digest, exact_digest, unit_digest,
             unit_size, physical_key, status, error_code, checked_at)
         SELECT ?1, m.source_revision, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                CAST(strftime('%s','now') AS INTEGER)
           FROM media_items m
          WHERE m.id = ?1
            AND m.source_revision = ?2
            AND m.is_deleted = 0
            AND m.file_size = ?11
            AND ?12 IS NOT NULL
            AND m.file_mtime_ns = ?12
            AND (
                ?14 IS NULL
                OR NOT EXISTS (
                    SELECT 1 FROM dedup_index_working AS previous
                     WHERE previous.item_id=m.id AND previous.physical_key IS NOT NULL
                )
                OR EXISTS (
                    SELECT 1 FROM dedup_index_working AS previous
                     WHERE previous.item_id=m.id AND previous.physical_key=?14
                )
            )
            AND EXISTS (
                SELECT 1 FROM app_config
                 WHERE key='dedup_run_generation' AND value=?13
            )
         ON CONFLICT(item_id) DO UPDATE SET
            source_revision = excluded.source_revision,
            hash_version = excluded.hash_version,
            quick_digest = COALESCE(excluded.quick_digest, dedup_index_working.quick_digest),
            exact_digest = excluded.exact_digest,
            unit_digest = excluded.unit_digest,
            unit_size = excluded.unit_size,
            physical_key = excluded.physical_key,
            status = excluded.status,
            error_code = excluded.error_code,
            checked_at = excluded.checked_at
         WHERE dedup_index_working.source_revision = excluded.source_revision",
        params![
            update.item_id,
            update.source_revision,
            update.hash_version,
            update.quick_digest,
            update.exact_digest,
            update.unit_digest,
            update.unit_size,
            update.physical_key,
            update.status,
            update.error_code,
            update.file_size,
            update.file_mtime_ns,
            generation.to_string(),
            update.physical_key,
        ],
    )?;
    Ok(changed > 0)
}

/// 读取当前分析代次，仅用于测试/诊断；生产写入使用原子 SQL 守门。
pub fn dedup_run_generation(conn: &Connection) -> Result<Option<u64>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key='dedup_run_generation'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.and_then(|value| value.parse().ok()))
}

/// 当前轮仍有效时，原子发布完整 working 快照。返回 false 表示该轮已被停止或替代。
pub fn publish_dedup_run(conn: &Connection, generation: u64) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    if dedup_run_generation(&tx)? != Some(generation) {
        tx.commit()?;
        return Ok(false);
    }
    tx.execute("DELETE FROM dedup_index", [])?;
    tx.execute(
        "INSERT INTO dedup_index
            (item_id, source_revision, hash_version, quick_digest, exact_digest, unit_digest,
             unit_size, physical_key, status, error_code, checked_at)
         SELECT item_id, source_revision, hash_version, quick_digest, exact_digest, unit_digest,
                unit_size, physical_key, status, error_code, checked_at
           FROM dedup_index_working",
        [],
    )?;
    tx.execute("DELETE FROM dedup_index_working", [])?;
    tx.commit()?;
    Ok(true)
}

/// 返回 quick digest 碰撞桶中的下一批主项。
pub fn list_dedup_quick_collisions(
    conn: &Connection,
    cursor: Option<&DedupCandidateCursor>,
    limit: usize,
) -> Result<Vec<DedupScanCandidate>> {
    let mut sql = String::from(CANDIDATE_SELECT);
    sql.push_str(
        " AND EXISTS (
                SELECT 1
                  FROM dedup_index_working di
                 WHERE di.item_id = m.id
                   AND di.source_revision = m.source_revision
                   AND di.hash_version = ?1
                   AND di.quick_digest IS NOT NULL
                   AND di.status = 'quick'
                   AND (SELECT COUNT(*)
                          FROM dedup_index_working dj
                          JOIN media_items mj ON mj.id = dj.item_id
                         WHERE dj.quick_digest = di.quick_digest
                           AND dj.hash_version = ?1
                           AND dj.source_revision = mj.source_revision
                           AND dj.status IN ('quick', 'ready')
                           AND mj.is_deleted = 0
                           AND mj.companion_of IS NULL
                           AND mj.file_size = m.file_size) > 1
            )",
    );
    if cursor.is_some() {
        sql.push_str(" AND (m.file_size > ?2 OR (m.file_size = ?2 AND m.id > ?3))");
    }
    sql.push_str(if cursor.is_some() {
        " ORDER BY m.file_size ASC, m.id ASC LIMIT ?4"
    } else {
        " ORDER BY m.file_size ASC, m.id ASC LIMIT ?2"
    });
    let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(cursor) = cursor {
        stmt.query_map(
            params![
                DEDUP_HASH_VERSION as i64,
                cursor.file_size,
                cursor.item_id,
                limit
            ],
            candidate_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(
            params![DEDUP_HASH_VERSION as i64, limit],
            candidate_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

/// 返回当前 quick 碰撞候选总数。只读取计数，不把全库成员 ID 物化到 IPC 或任务状态。
pub fn count_dedup_quick_collisions(conn: &Connection) -> Result<u64> {
    let sql = format!(
        "SELECT COUNT(*) FROM ({CANDIDATE_SELECT}
          AND EXISTS (
                SELECT 1
                  FROM dedup_index_working di
                 WHERE di.item_id = m.id
                   AND di.source_revision = m.source_revision
                   AND di.hash_version = ?1
                   AND di.quick_digest IS NOT NULL
                   AND di.status = 'quick'
                   AND (SELECT COUNT(*)
                          FROM dedup_index_working dj
                          JOIN media_items mj ON mj.id = dj.item_id
                         WHERE dj.quick_digest = di.quick_digest
                           AND dj.hash_version = ?1
                           AND dj.source_revision = mj.source_revision
                           AND dj.status IN ('quick', 'ready')
                           AND mj.is_deleted = 0
                           AND mj.companion_of IS NULL
                           AND mj.file_size = m.file_size) > 1
            ))"
    );
    let count: i64 = conn.query_row(&sql, params![DEDUP_HASH_VERSION as i64], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}

/// 组/镜头成员查询共用的「当前有效重复成员」谓词（方案 §3.1 硬约束：两者必须
/// 同边界——提取为单一来源，使边界漂移在结构上不可能发生）。
///
/// 前提：调用方 SQL 已写好 `FROM dedup_index di JOIN media_items mi ON mi.id = di.item_id
/// JOIN directories d ON d.id = mi.directory_id JOIN scan_roots r ON r.id = d.root_id`
/// 与 `WHERE`，且绑定参数表前三槽为：
///   ?1 = 当前 `DEDUP_HASH_VERSION`
///   ?2 = include_offline（1 = 允许 missing/offline 参与）
///   ?3 = include_zero_byte（1 = 允许零字节参与）
/// 追加完毕后调用方可继续以 `AND ...` 拼接自有条件（如组游标、组规模过滤）。
///
/// 调用方：本模块的组/镜头成员查询与 folders 镜头行查询（`eligible` CTE），以及 layout
/// `view_to_sql` 的镜头 lowering（2026-09-02 主画廊重复项浏览方案 §10.2「同一 lowering」）
/// ——可见性即为此放开到 `crate::db::queries` 子树。
pub(in crate::db::queries) fn push_eligible_predicates(sql: &mut String) {
    sql.push_str(
        "di.unit_digest IS NOT NULL
               AND di.unit_size IS NOT NULL
               AND di.hash_version = ?1
               AND di.status = 'ready'
               AND di.source_revision = mi.source_revision
               AND mi.is_deleted = 0
               AND mi.companion_of IS NULL
               AND (?2 = 1 OR mi.availability NOT IN ('missing', 'offline'))
               AND (?3 = 1 OR mi.file_size > 0)
               AND r.is_active = 1
               AND r.is_hidden = 0",
    );
}

/// 根据逻辑单元摘要动态生成重复组，避免持久化第二套容易漂移的 group 表。
fn build_duplicate_groups_sql(has_cursor: bool) -> String {
    let mut sql = String::from(
        "WITH eligible AS (
            SELECT di.unit_digest, di.unit_size, mi.id,
                   mi.is_favorited,
                   COALESCE(mi.rating, 0) AS rating,
                   COALESCE(mi.color_label, 0) AS color_label,
                   mi.availability,
                   COALESCE((SELECT group_concat(tag_id, ',')
                               FROM (SELECT tag_id FROM item_tags
                                      WHERE item_id = mi.id ORDER BY tag_id)), '') AS tag_signature,
                   COALESCE((SELECT group_concat(album_id, ',')
                               FROM (SELECT ai.album_id FROM album_items ai
                                      JOIN albums a ON a.id = ai.album_id
                                     WHERE ai.item_id = mi.id AND a.deleted_at IS NULL
                                     ORDER BY ai.album_id)), '') AS album_signature,
                   (SELECT COUNT(*) FROM reader_bookmarks WHERE item_id = mi.id) AS bookmark_count,
                   (SELECT COUNT(*) FROM album_items ai JOIN albums a ON a.id = ai.album_id
                     WHERE ai.item_id = mi.id AND a.deleted_at IS NULL) AS album_count,
                   (SELECT COUNT(*) FROM item_tags WHERE item_id = mi.id) AS tag_count
              FROM dedup_index di
              JOIN media_items mi ON mi.id = di.item_id
              JOIN directories d ON d.id = mi.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE ",
    );
    push_eligible_predicates(&mut sql);
    if has_cursor {
        // `unit_digest,unit_size` 唯一确定一个组；保留游标组给外层
        // `first_item_id` 条件，先在有序索引输入阶段丢弃更早的组。
        sql.push_str("\n               AND (di.unit_digest, di.unit_size) >= (?5, ?6)");
    }
    sql.push_str(
        "
        ), groups AS (
            SELECT unit_digest, unit_size, COUNT(*) AS member_count,
                   MIN(id) AS first_item_id,
                   CASE WHEN MIN(is_favorited) <> MAX(is_favorited)
                              OR MIN(rating) <> MAX(rating)
                              OR MIN(color_label) <> MAX(color_label)
                              OR MIN(tag_signature) <> MAX(tag_signature)
                              OR MIN(album_signature) <> MAX(album_signature)
                              OR MIN(bookmark_count) <> MAX(bookmark_count)
                        THEN 1 ELSE 0 END AS metadata_conflict
              FROM eligible
             GROUP BY unit_digest, unit_size
            HAVING COUNT(*) >= COALESCE(?4, 2)
        )
        SELECT g.unit_digest, g.unit_size, g.member_count, g.first_item_id,
               g.metadata_conflict,
               (SELECT e.id
                  FROM eligible e
                 WHERE e.unit_digest = g.unit_digest
                   AND e.unit_size = g.unit_size
                 ORDER BY CASE WHEN e.availability = 'online' THEN 1 ELSE 0 END DESC,
                          CASE WHEN e.is_favorited <> 0 THEN 1 ELSE 0 END DESC,
                          e.rating DESC,
                          CASE WHEN e.color_label <> 0 THEN 1 ELSE 0 END DESC,
                          (e.album_count + e.tag_count + e.bookmark_count) DESC,
                          e.id ASC
                 LIMIT 1) AS suggested_keeper_id
          FROM groups g
         WHERE 1=1",
    );
    if has_cursor {
        sql.push_str(
            " AND (g.unit_digest > ?5
               OR (g.unit_digest = ?5 AND g.unit_size > ?6)
               OR (g.unit_digest = ?5 AND g.unit_size = ?6 AND g.first_item_id > ?7))",
        );
    }
    sql.push_str(if has_cursor {
        " ORDER BY g.unit_digest ASC, g.unit_size ASC, g.first_item_id ASC LIMIT ?8"
    } else {
        " ORDER BY g.unit_digest ASC, g.unit_size ASC, g.first_item_id ASC LIMIT ?5"
    });
    sql
}

pub fn list_duplicate_groups(
    conn: &Connection,
    cursor: Option<&DuplicateGroupCursor>,
    limit: usize,
    include_offline: bool,
    include_zero_byte: bool,
    min_member_count: Option<u32>,
) -> Result<Vec<DuplicateGroupRow>> {
    let min_member_count = i64::from(min_member_count.unwrap_or(2).max(2));
    let sql = build_duplicate_groups_sql(cursor.is_some());
    let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(cursor) = cursor {
        stmt.query_map(
            params![
                DEDUP_HASH_VERSION as i64,
                include_offline as i64,
                include_zero_byte as i64,
                min_member_count,
                &cursor.unit_digest,
                cursor.unit_size,
                cursor.first_item_id,
                limit
            ],
            |row| {
                let digest: Vec<u8> = row.get(0)?;
                let unit_size: i64 = row.get(1)?;
                let member_count: i64 = row.get(2)?;
                Ok(DuplicateGroupRow {
                    unit_digest: digest,
                    unit_size,
                    member_count,
                    potential_logical_bytes: unit_size
                        .max(0)
                        .saturating_mul(member_count.saturating_sub(1).max(0)),
                    first_item_id: row.get(3)?,
                    metadata_conflict: row.get::<_, i64>(4)? != 0,
                    suggested_keeper_id: row.get(5)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(
            params![
                DEDUP_HASH_VERSION as i64,
                include_offline as i64,
                include_zero_byte as i64,
                min_member_count,
                limit
            ],
            |row| {
                let digest: Vec<u8> = row.get(0)?;
                let unit_size: i64 = row.get(1)?;
                let member_count: i64 = row.get(2)?;
                Ok(DuplicateGroupRow {
                    unit_digest: digest,
                    unit_size,
                    member_count,
                    potential_logical_bytes: unit_size
                        .max(0)
                        .saturating_mul(member_count.saturating_sub(1).max(0)),
                    first_item_id: row.get(3)?,
                    metadata_conflict: row.get::<_, i64>(4)? != 0,
                    suggested_keeper_id: row.get(5)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

/// 查询一个组的当前主项成员，成员游标只按 item_id 前进。
pub fn list_duplicate_members(
    conn: &Connection,
    unit_digest: &[u8],
    unit_size: i64,
    include_offline: bool,
    include_zero_byte: bool,
    after_item_id: Option<i64>,
    limit: usize,
) -> Result<Vec<DuplicateMemberRow>> {
    let mut sql = String::from(
        "SELECT mi.id, mi.source_revision, mi.file_name, mi.file_size, mi.file_mtime,
                mi.file_mtime_ns, mi.availability, mi.is_favorited, mi.rating, mi.color_label,
                mi.is_live_photo, di.physical_key,
                (SELECT COUNT(*) FROM album_items ai JOIN albums a ON a.id = ai.album_id
                  WHERE ai.item_id = mi.id AND a.deleted_at IS NULL),
                (SELECT COUNT(*) FROM item_tags WHERE item_id = mi.id),
                (SELECT COUNT(*) FROM reader_bookmarks WHERE item_id = mi.id),
                r.path, d.rel_path
           FROM dedup_index di
           JOIN media_items mi ON mi.id = di.item_id
           JOIN directories d ON d.id = mi.directory_id
           JOIN scan_roots r ON r.id = d.root_id
          WHERE di.unit_digest = ?1
            AND di.unit_size = ?2
            AND di.hash_version = ?3
            AND di.status = 'ready'
            AND di.source_revision = mi.source_revision
            AND mi.is_deleted = 0
            AND mi.companion_of IS NULL
            AND (?4 = 1 OR mi.availability NOT IN ('missing', 'offline'))
            AND (?5 = 1 OR mi.file_size > 0)
            AND r.is_active = 1
            AND r.is_hidden = 0",
    );
    if after_item_id.is_some() {
        sql.push_str(" AND mi.id > ?6 ORDER BY mi.id ASC LIMIT ?7");
    } else {
        sql.push_str(" ORDER BY mi.id ASC LIMIT ?6");
    }
    let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(after) = after_item_id {
        stmt.query_map(
            params![
                unit_digest,
                unit_size,
                DEDUP_HASH_VERSION as i64,
                include_offline as i64,
                include_zero_byte as i64,
                after,
                limit
            ],
            member_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(
            params![
                unit_digest,
                unit_size,
                DEDUP_HASH_VERSION as i64,
                include_offline as i64,
                include_zero_byte as i64,
                limit
            ],
            member_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

/// 读取当前组的完整成员快照，仅供清理预检在数据库线程内做一致性比较。
///
/// 这不是 IPC 分页接口；调用方不会把结果直接下发前端。清理计划必须能发现未选中
/// 成员的代次/路径变化，因此这里保留完整组快照而不是只比较 keeper 和待删项。
pub fn list_duplicate_group_members_all(
    conn: &Connection,
    unit_digest: &[u8],
    unit_size: i64,
    include_offline: bool,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateMemberRow>> {
    let mut stmt = conn.prepare(
        "SELECT mi.id, mi.source_revision, mi.file_name, mi.file_size, mi.file_mtime,
                mi.file_mtime_ns, mi.availability, mi.is_favorited, mi.rating, mi.color_label,
                mi.is_live_photo, di.physical_key,
                (SELECT COUNT(*) FROM album_items ai JOIN albums a ON a.id = ai.album_id
                  WHERE ai.item_id = mi.id AND a.deleted_at IS NULL),
                (SELECT COUNT(*) FROM item_tags WHERE item_id = mi.id),
                (SELECT COUNT(*) FROM reader_bookmarks WHERE item_id = mi.id),
                r.path, d.rel_path
           FROM dedup_index di
           JOIN media_items mi ON mi.id = di.item_id
           JOIN directories d ON d.id = mi.directory_id
           JOIN scan_roots r ON r.id = d.root_id
          WHERE di.unit_digest=?1
            AND di.unit_size=?2
            AND di.hash_version=?3
            AND di.status='ready'
            AND di.source_revision=mi.source_revision
            AND mi.is_deleted=0
            AND mi.companion_of IS NULL
            AND (?4 = 1 OR mi.availability NOT IN ('missing', 'offline'))
            AND (?5 = 1 OR mi.file_size > 0)
            AND r.is_active=1
            AND r.is_hidden=0
          ORDER BY mi.id ASC",
    )?;
    let rows = stmt
        .query_map(
            params![
                unit_digest,
                unit_size,
                DEDUP_HASH_VERSION as i64,
                include_offline as i64,
                include_zero_byte as i64
            ],
            member_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn member_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DuplicateMemberRow> {
    let file_name: String = row.get(2)?;
    let root_path: String = row.get(15)?;
    let rel_path: String = row.get(16)?;
    let path = resolve_media_path(&root_path, &rel_path, &file_name);
    Ok(DuplicateMemberRow {
        item_id: row.get(0)?,
        source_revision: row.get(1)?,
        file_name,
        path,
        file_size: row.get(3)?,
        file_mtime: row.get(4)?,
        file_mtime_ns: row.get(5)?,
        availability: row.get(6)?,
        is_favorited: row.get::<_, i64>(7)? != 0,
        rating: row.get(8)?,
        color_label: row.get(9)?,
        is_live_photo: row.get::<_, i64>(10)? != 0,
        physical_key: row.get(11)?,
        album_count: row.get(12)?,
        tag_count: row.get(13)?,
        bookmark_count: row.get(14)?,
    })
}

/// 镜头布局成员行（主画廊重复项浏览方案 §12.1）：携带组身份 + 组内排序所需的全部键。
/// `normalized_dir_path` 为排序用的完整规范化路径，来源与 folder 轴显示路径同源
/// （`r.path || '/' || d.rel_path`；根目录本身 rel_path 为空串，即 `r.path`）。
#[derive(Debug, Clone)]
pub struct DuplicateLensMemberRow {
    pub unit_digest: Vec<u8>,
    pub unit_size: i64,
    pub item_id: i64,
    pub directory_id: i64,
    pub sort_datetime: i64,
    pub normalized_dir_path: String,
    pub file_name: String,
}

/// 镜头成员排序键「完整规范化目录路径」的 SQL 表达式（方案 §6.3 normalized_directory_path）：
/// `r.path || '/' || d.rel_path`，根目录本身 rel_path 为空串时即 `r.path`。
///
/// [`list_duplicate_lens_members`] 与 layout `view_to_sql` 的镜头 lowering（方案 §10.2
/// 「同一 lowering」）两处引用**同一常量**——路径键漂移在结构上不可能发生。采用完整路径
/// 而非 `rel_path` 的原因见 [`list_duplicate_lens_members`] 注释（跨根同名目录消歧）。
/// 前提与 [`push_eligible_predicates`] 相同：FROM 按 `directories d / scan_roots r` 别名连接。
pub fn normalized_dir_path_sql() -> &'static str {
    "CASE WHEN d.rel_path = '' THEN r.path ELSE r.path || '/' || d.rel_path END"
}

/// 当前有效精确重复组的全部成员（方案 §3.1/§12.1）：eligible 谓词与
/// `build_duplicate_groups_sql` 同源（`push_eligible_predicates`），仅保留组规模 ≥ 2 的组。
/// 镜头固定 include_offline=false / include_zero_byte=false——offline/missing/零字节不是
/// 有效成员，folders 模式下归「尚未确认」（方案 §3.4）。无 ORDER BY：组间/组内稳定排序
/// 在布局任务的内存组装阶段进行，这里只投影排序键（`sort_datetime`/`normalized_dir_path`/
/// `file_name`/`item_id`，方案 §6.3）。
///
/// `normalized_dir_path` 采用完整路径而非 `rel_path`：一个重复组可跨多个 scan root，
/// rel_path 只在 `(root_id, rel_path)` 内唯一，按它排序会让不同根的同名目录并列出歧义；
/// root path + rel_path 拼出的完整路径跨根确定、且与 UI 显示的文件夹路径同源，正是
/// 方案 §6.3 的 `normalized_directory_path`。folder 轴的「root created_at,id 先定根」
/// 惯例服务于整棵根子树连续的浏览序，不适用于组内成员序，故不在此复用。
pub fn list_duplicate_lens_members(conn: &Connection) -> Result<Vec<DuplicateLensMemberRow>> {
    let mut sql = String::from(
        "SELECT unit_digest, unit_size, item_id, directory_id, sort_datetime,
                normalized_dir_path, file_name
           FROM (
            SELECT di.unit_digest, di.unit_size, mi.id AS item_id,
                   mi.directory_id AS directory_id, mi.sort_datetime AS sort_datetime, ",
    );
    // 路径表达式单一事实源：与 view_to_sql 镜头 lowering 共用（方案 §10.2）。
    sql.push_str(normalized_dir_path_sql());
    sql.push_str(
        " AS normalized_dir_path,
                   mi.file_name AS file_name,
                   COUNT(*) OVER (PARTITION BY di.unit_digest, di.unit_size) AS member_count
              FROM dedup_index di
              JOIN media_items mi ON mi.id = di.item_id
              JOIN directories d ON d.id = mi.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE ",
    );
    push_eligible_predicates(&mut sql);
    // 窗口函数不能出现在同层 WHERE，包一层子查询再过滤组规模 ≥2（与组查询
    // HAVING COUNT(*) >= 2 同义；bundled SQLite ≥ 3.25 支持窗口函数）。
    sql.push_str("\n        ) WHERE member_count >= 2");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![DEDUP_HASH_VERSION as i64, false, false], |row| {
            Ok(DuplicateLensMemberRow {
                unit_digest: row.get(0)?,
                unit_size: row.get(1)?,
                item_id: row.get(2)?,
                directory_id: row.get(3)?,
                sort_datetime: row.get(4)?,
                normalized_dir_path: row.get(5)?,
                file_name: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// folders 镜头三桶分类（方案 §3.4）。
///
/// 判别值与 §7.5 的 `bucket_rank` 对齐（duplicate=0 < unconfirmed=1 < unique=2），
/// 派生 `Ord` 使 enum 判别序即 bucket_rank——布局内存组装（`lens_folder`）直接按
/// enum 序排桶序，无需另写 rank 映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FolderLensBucket {
    Duplicate,
    Unconfirmed,
    Unique,
}

/// folders 镜头行（方案 §3.4/§7.5）：纳入文件夹范围内的全部项 + 三桶分类证据。
/// `unit_digest`/`unit_size` 是重复项的组身份（组身份 = unit_digest + unit_size，
/// 方案 §3.1；独有①行也携带其有效单成员摘要，其余行为 `None`）。
#[derive(Debug, Clone)]
pub struct DuplicateFolderLensRow {
    pub item_id: i64,
    pub directory_id: i64,
    pub sort_datetime: i64,
    pub normalized_dir_path: String,
    pub file_name: String,
    pub bucket: FolderLensBucket,
    pub unit_digest: Option<Vec<u8>>,
    pub unit_size: Option<i64>,
}

fn folder_lens_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DuplicateFolderLensRow> {
    let bucket: i64 = row.get(5)?;
    Ok(DuplicateFolderLensRow {
        item_id: row.get(0)?,
        directory_id: row.get(1)?,
        sort_datetime: row.get(2)?,
        normalized_dir_path: row.get(3)?,
        file_name: row.get(4)?,
        // SQL CASE 只产出 0/1/2（§7.5 bucket_rank）；防御分支归 Unconfirmed。
        bucket: match bucket {
            0 => FolderLensBucket::Duplicate,
            2 => FolderLensBucket::Unique,
            _ => FolderLensBucket::Unconfirmed,
        },
        unit_digest: row.get(6)?,
        unit_size: row.get(7)?,
    })
}

/// folders 镜头行查询（方案 §3.4/§7.5/§12.1）：可见全库（is_deleted=0、
/// companion_of IS NULL、可见根）× LEFT JOIN dedup_index → 三桶分类，单条 SQL 直接
/// 产出 bucket，无内存二次分类。
///
/// 判定输入（§3.4 判定表）：
/// - `unit_group_size`：`eligible` CTE 内的窗口计数。eligible 以
///   [`push_eligible_predicates`] 为唯一边界（与 groups 镜头/组查询同源，漂移在结构上
///   不可能发生），因此 NULL unit 行、非 ready、版本不符、修订漂移、offline/零字节行
///   根本不进窗口输入——「NULL 分组」与「stale 同摘要行抬高组规模」的隐患在结构上被排除。
/// - `size_bucket`：可见全库按 `file_size` 计数（离线/零字节同在可见范围，会阻止证据②）。
/// - `quick_bucket`：可见全库按 `(file_size, quick_digest)` 计数，只认当前版本、当前
///   修订、状态 quick/ready 的行（与 `list_dedup_quick_collisions` 的碰撞口径一致）。
///
/// 三桶判定：
/// - Duplicate：在 eligible 中且 `unit_group_size >= 2`。offline/missing/零字节行不在
///   eligible，即使 ready 成组也落 Unconfirmed（§3.4 folders 语义）。
/// - Unique（基线为在线有效行，三类证据任一）：① 有效精确摘要单成员
///   （`unit_group_size = 1`）；② 唯一大小桶（无同大小项则无副本可能）；③ 同大小项
///   存在但 quick 摘要互异（quick 已排除碰撞）。②③接受 status='quick'——完整分析后
///   无候选的项停在 quick 是正常态；ready 行 unit 为空也按②③判。
/// - Unconfirmed：其余全部（无 di 行、stale/unstable/missing/error、版本不符、漂移、
///   offline/missing、零字节、quick 碰撞未定案）。
///
/// 纳入过滤（§3.4 末段）：只返回至少含一个当前有效重复成员的直接文件夹内的行——
/// `included_dirs` CTE 在同一查询内自含计算，纯独有/纯未确认目录不产生任何行。
///
/// 镜头固定严格口径（include_offline=false / include_zero_byte=false）。无 ORDER BY：
/// 与 [`list_duplicate_lens_members`] 同哲学，内存组装阶段排序，这里只投影排序键。
pub fn list_duplicate_folder_lens_rows(conn: &Connection) -> Result<Vec<DuplicateFolderLensRow>> {
    let mut sql = String::from(
        "WITH eligible AS (
            SELECT di.item_id, di.unit_digest, di.unit_size,
                   COUNT(*) OVER (PARTITION BY di.unit_digest, di.unit_size) AS unit_group_size
              FROM dedup_index di
              JOIN media_items mi ON mi.id = di.item_id
              JOIN directories d ON d.id = mi.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE ",
    );
    push_eligible_predicates(&mut sql);
    sql.push_str(
        "
        ),
        size_buckets AS (
            SELECT m.file_size, COUNT(*) AS size_bucket
              FROM media_items m
              JOIN directories d ON d.id = m.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE m.is_deleted = 0
               AND m.companion_of IS NULL
               AND r.is_active = 1
               AND r.is_hidden = 0
             GROUP BY m.file_size
        ),
        quick_buckets AS (
            SELECT m.file_size, di.quick_digest, COUNT(*) AS quick_bucket
              FROM dedup_index di
              JOIN media_items m ON m.id = di.item_id
              JOIN directories d ON d.id = m.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE m.is_deleted = 0
               AND m.companion_of IS NULL
               AND r.is_active = 1
               AND r.is_hidden = 0
               AND di.quick_digest IS NOT NULL
               AND di.status IN ('quick', 'ready')
               AND di.hash_version = ?1
               AND di.source_revision = m.source_revision
             GROUP BY m.file_size, di.quick_digest
        ),
        classified AS (
            SELECT m.id AS item_id, m.directory_id AS directory_id,
                   m.sort_datetime AS sort_datetime, ",
    );
    // 路径表达式单一事实源：与 groups 镜头/folder 轴显示路径同源（方案 §6.3）。
    sql.push_str(normalized_dir_path_sql());
    sql.push_str(
        " AS normalized_dir_path,
                   m.file_name AS file_name,
                   e.unit_digest AS unit_digest,
                   e.unit_size AS unit_size,
                   CASE WHEN e.item_id IS NOT NULL AND e.unit_group_size >= 2
                        THEN 1 ELSE 0 END AS is_duplicate,
                   CASE WHEN (e.item_id IS NOT NULL AND e.unit_group_size = 1)
                             OR (di.unit_digest IS NULL
                                 AND di.quick_digest IS NOT NULL
                                 AND di.status IN ('quick', 'ready')
                                 AND di.hash_version = ?1
                                 AND di.source_revision = m.source_revision
                                 AND m.availability NOT IN ('missing', 'offline')
                                 AND m.file_size > 0
                                 AND (sb.size_bucket = 1
                                      OR (qb.quick_bucket = 1 AND sb.size_bucket > 1)))
                        THEN 1 ELSE 0 END AS is_unique
              FROM media_items m
              JOIN directories d ON d.id = m.directory_id
              JOIN scan_roots r ON r.id = d.root_id
              LEFT JOIN dedup_index di ON di.item_id = m.id
              LEFT JOIN eligible e ON e.item_id = m.id
              LEFT JOIN size_buckets sb ON sb.file_size = m.file_size
              LEFT JOIN quick_buckets qb ON qb.file_size = m.file_size
                                        AND qb.quick_digest = di.quick_digest
             WHERE m.is_deleted = 0
               AND m.companion_of IS NULL
               AND r.is_active = 1
               AND r.is_hidden = 0
        ),
        included_dirs AS (
            SELECT DISTINCT directory_id
              FROM classified
             WHERE is_duplicate = 1
        )
        SELECT c.item_id, c.directory_id, c.sort_datetime, c.normalized_dir_path,
               c.file_name,
               CASE WHEN c.is_duplicate = 1 THEN 0
                    WHEN c.is_unique = 1 THEN 2
                    ELSE 1 END AS bucket,
               c.unit_digest, c.unit_size
          FROM classified c
          JOIN included_dirs i ON i.directory_id = c.directory_id",
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![DEDUP_HASH_VERSION as i64, false, false],
            folder_lens_row_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 用于汇总卡片的组数量；不返回成员 ID。
pub fn count_duplicate_groups(conn: &Connection) -> Result<u64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
            SELECT di.unit_digest, di.unit_size
              FROM dedup_index di
              JOIN media_items mi ON mi.id = di.item_id
              JOIN directories d ON d.id = mi.directory_id
              JOIN scan_roots r ON r.id = d.root_id
             WHERE di.unit_digest IS NOT NULL
               AND di.unit_size IS NOT NULL
               AND di.hash_version = ?1
               AND di.status = 'ready'
               AND di.source_revision = mi.source_revision
               AND mi.is_deleted = 0
               AND mi.companion_of IS NULL
               AND mi.availability NOT IN ('missing', 'offline')
               AND mi.file_size > 0
               AND r.is_active = 1
               AND r.is_hidden = 0
             GROUP BY di.unit_digest, di.unit_size
            HAVING COUNT(*) > 1
        )",
        params![DEDUP_HASH_VERSION as i64],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}

/// 汇总当前逻辑单元重复组，供后台任务生成进度而不保留全库 unit digest 哈希表。
pub fn summarize_duplicate_groups(conn: &Connection) -> Result<(u64, u64)> {
    let (groups, bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(unit_size * (member_count - 1)), 0)
           FROM (
                SELECT di.unit_digest, di.unit_size, COUNT(*) AS member_count
                  FROM dedup_index_working di
                  JOIN media_items mi ON mi.id = di.item_id
                  JOIN directories d ON d.id = mi.directory_id
                  JOIN scan_roots r ON r.id = d.root_id
                 WHERE di.unit_digest IS NOT NULL
                   AND di.unit_size IS NOT NULL
                   AND di.hash_version = ?1
                   AND di.status = 'ready'
                   AND di.source_revision = mi.source_revision
                   AND mi.is_deleted = 0
                   AND mi.companion_of IS NULL
                   AND mi.availability NOT IN ('missing', 'offline')
                   AND mi.file_size > 0
                   AND r.is_active = 1
                   AND r.is_hidden = 0
                 GROUP BY di.unit_digest, di.unit_size
                HAVING COUNT(*) > 1
           )",
        params![DEDUP_HASH_VERSION as i64],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((groups.max(0) as u64, bytes.max(0) as u64))
}

// ── 文件夹优先聚合 ────────────────────────────────────────────────────────────

/// 文件夹统计的唯一 SQL 投影。所有动态值（hash 版本、筛选器、游标）都由调用方绑定；
/// 这里拼接的只是本模块内固定 CTE 片段，避免为每个文件夹重复执行一组全库查询。
const FOLDER_STATS_CTE: &str = r#"
WITH RECURSIVE
visible_dirs AS (
    SELECT d.id, d.root_id, d.parent_id, d.name, d.rel_path, d.depth
      FROM directories d
      JOIN scan_roots r ON r.id = d.root_id
     WHERE r.is_active = 1 AND r.is_hidden = 0
),
visible_positions AS (
    SELECT m.id, m.directory_id, m.source_revision, m.file_size, m.availability,
           m.is_favorited, COALESCE(m.rating, 0) AS rating,
           COALESCE(m.color_label, 0) AS color_label,
           (SELECT COUNT(*)
              FROM album_items ai JOIN albums a ON a.id = ai.album_id
             WHERE ai.item_id = m.id AND a.deleted_at IS NULL) AS album_count,
           (SELECT COUNT(*) FROM item_tags WHERE item_id = m.id) AS tag_count,
           (SELECT COUNT(*) FROM reader_bookmarks WHERE item_id = m.id) AS bookmark_count
      FROM media_items m
      JOIN visible_dirs d ON d.id = m.directory_id
     WHERE m.is_deleted = 0 AND m.companion_of IS NULL
),
exact_members AS (
    SELECT p.id, p.directory_id, p.source_revision, p.file_size, p.availability,
           p.is_favorited, p.rating, p.color_label, p.album_count, p.tag_count,
           p.bookmark_count, di.unit_digest, di.unit_size, di.physical_key
      FROM visible_positions p
      JOIN dedup_index di
        ON di.item_id = p.id AND di.source_revision = p.source_revision
     WHERE di.unit_digest IS NOT NULL
       AND di.unit_size IS NOT NULL
       AND di.hash_version = ?1
       AND di.status = 'ready'
       AND p.availability NOT IN ('missing', 'offline')
       AND (?2 = 1 OR p.file_size > 0)
),
physical_aliases AS (
    SELECT e.id
      FROM exact_members e
      JOIN exact_members peer
        ON peer.unit_digest = e.unit_digest
       AND peer.unit_size = e.unit_size
       AND peer.physical_key IS NOT NULL
       AND peer.physical_key = e.physical_key
       AND peer.id <> e.id
     GROUP BY e.id
),
groups AS (
    SELECT e.unit_digest, e.unit_size, COUNT(*) AS group_count,
           (SELECT keeper.id
              FROM exact_members keeper
             WHERE keeper.unit_digest = e.unit_digest
               AND keeper.unit_size = e.unit_size
             ORDER BY CASE WHEN keeper.availability = 'online' THEN 1 ELSE 0 END DESC,
                      CASE WHEN keeper.is_favorited <> 0
                                  OR keeper.rating > 0
                                  OR keeper.color_label > 0
                                  OR keeper.album_count > 0
                                  OR keeper.tag_count > 0
                                  OR keeper.bookmark_count > 0
                           THEN 1 ELSE 0 END DESC,
                      keeper.rating DESC,
                      CASE WHEN keeper.color_label > 0 THEN 1 ELSE 0 END DESC,
                      (keeper.album_count + keeper.tag_count + keeper.bookmark_count) DESC,
                      keeper.id ASC
             LIMIT 1) AS keeper_id
      FROM exact_members e
     GROUP BY e.unit_digest, e.unit_size
     HAVING COUNT(*) > 1
),
position_ancestors(folder_id, position_id) AS (
    SELECT p.directory_id, p.id
      FROM visible_positions p
    UNION
    SELECT d.parent_id, pa.position_id
      FROM position_ancestors pa
      JOIN visible_dirs d ON d.id = pa.folder_id
     WHERE d.parent_id IS NOT NULL
),
scoped_groups AS (
    SELECT pa.folder_id, e.unit_digest, e.unit_size, COUNT(*) AS scoped_count
      FROM position_ancestors pa
      JOIN exact_members e ON e.id = pa.position_id
      JOIN groups g ON g.unit_digest = e.unit_digest AND g.unit_size = e.unit_size
     GROUP BY pa.folder_id, e.unit_digest, e.unit_size
),
folder_position_rows AS (
    SELECT pa.folder_id, p.id AS position_id, e.id AS analyzed_id,
           e.is_favorited, e.rating, e.color_label, e.album_count,
           e.tag_count, e.bookmark_count, e.unit_size,
           g.group_count, g.keeper_id, sg.scoped_count
      FROM position_ancestors pa
      JOIN visible_positions p ON p.id = pa.position_id
      LEFT JOIN exact_members e ON e.id = p.id
      LEFT JOIN groups g
        ON g.unit_digest = e.unit_digest AND g.unit_size = e.unit_size
      LEFT JOIN scoped_groups sg
        ON sg.folder_id = pa.folder_id
       AND sg.unit_digest = e.unit_digest
       AND sg.unit_size = e.unit_size
),
folder_stats AS (
    SELECT d.id AS folder_id, d.root_id, d.parent_id, d.name, d.rel_path, d.depth,
           COUNT(fp.position_id) AS total_positions,
           COUNT(fp.analyzed_id) AS analyzed_positions,
           COALESCE(SUM(CASE WHEN fp.group_count IS NOT NULL THEN 1 ELSE 0 END), 0)
               AS duplicate_positions,
           COALESCE(SUM(CASE WHEN fp.group_count > fp.scoped_count THEN 1 ELSE 0 END), 0)
               AS external_covered_positions,
           COALESCE(SUM(CASE WHEN fp.group_count = fp.scoped_count THEN 1 ELSE 0 END), 0)
               AS internal_duplicate_positions,
           COUNT(fp.position_id) - COUNT(fp.analyzed_id) AS unreviewed_positions,
           COALESCE(SUM(CASE WHEN fp.group_count IS NOT NULL
                                   AND (fp.is_favorited <> 0 OR fp.rating > 0
                                        OR fp.color_label > 0 OR fp.album_count > 0
                                        OR fp.tag_count > 0 OR fp.bookmark_count > 0)
                               THEN 1 ELSE 0 END), 0) AS protected_positions,
           COALESCE(SUM(CASE
                WHEN fp.group_count > fp.scoped_count
                     AND NOT (fp.is_favorited <> 0 OR fp.rating > 0
                              OR fp.color_label > 0 OR fp.album_count > 0
                              OR fp.tag_count > 0 OR fp.bookmark_count > 0)
                     AND NOT EXISTS (SELECT 1 FROM physical_aliases pa WHERE pa.id = fp.analyzed_id)
                  THEN 1
                WHEN fp.group_count = fp.scoped_count
                     AND fp.analyzed_id <> fp.keeper_id
                     AND NOT (fp.is_favorited <> 0 OR fp.rating > 0
                              OR fp.color_label > 0 OR fp.album_count > 0
                              OR fp.tag_count > 0 OR fp.bookmark_count > 0)
                     AND NOT EXISTS (SELECT 1 FROM physical_aliases pa WHERE pa.id = fp.analyzed_id)
                  THEN 1
                ELSE 0 END), 0) AS recommended_positions,
           COALESCE(SUM(CASE
                WHEN fp.group_count > fp.scoped_count
                     AND NOT (fp.is_favorited <> 0 OR fp.rating > 0
                              OR fp.color_label > 0 OR fp.album_count > 0
                              OR fp.tag_count > 0 OR fp.bookmark_count > 0)
                     AND NOT EXISTS (SELECT 1 FROM physical_aliases pa WHERE pa.id = fp.analyzed_id)
                  THEN CASE WHEN fp.unit_size > 0 THEN fp.unit_size ELSE 0 END
                WHEN fp.group_count = fp.scoped_count
                     AND fp.analyzed_id <> fp.keeper_id
                     AND NOT (fp.is_favorited <> 0 OR fp.rating > 0
                              OR fp.color_label > 0 OR fp.album_count > 0
                              OR fp.tag_count > 0 OR fp.bookmark_count > 0)
                     AND NOT EXISTS (SELECT 1 FROM physical_aliases pa WHERE pa.id = fp.analyzed_id)
                  THEN CASE WHEN fp.unit_size > 0 THEN fp.unit_size ELSE 0 END
                ELSE 0 END), 0) AS recommended_logical_bytes
       FROM visible_dirs d
       LEFT JOIN folder_position_rows fp ON fp.folder_id = d.id
      GROUP BY d.id, d.root_id, d.parent_id, d.name, d.rel_path, d.depth
)
"#;

fn optional_value(value: Option<i64>) -> Value {
    value.map_or(Value::Null, Value::Integer)
}

fn folder_stats_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DuplicateFolderStatsRow> {
    let total_positions: i64 = row.get(6)?;
    let recommended_positions: i64 = row.get(13)?;
    Ok(DuplicateFolderStatsRow {
        folder_id: row.get(0)?,
        root_id: row.get(1)?,
        parent_id: row.get(2)?,
        name: row.get(3)?,
        rel_path: row.get(4)?,
        depth: row.get(5)?,
        total_positions,
        analyzed_positions: row.get(7)?,
        duplicate_positions: row.get(8)?,
        external_covered_positions: row.get(9)?,
        internal_duplicate_positions: row.get(10)?,
        unreviewed_positions: row.get(11)?,
        protected_positions: row.get(12)?,
        recommended_positions,
        recommended_logical_bytes: row.get(14)?,
        retained_positions: total_positions.saturating_sub(recommended_positions),
    })
}

fn folder_stats_projection() -> &'static str {
    " SELECT fs.folder_id, fs.root_id, fs.parent_id, fs.name, fs.rel_path, fs.depth,\
            fs.total_positions, fs.analyzed_positions, fs.duplicate_positions,\
            fs.external_covered_positions, fs.internal_duplicate_positions,\
            fs.unreviewed_positions, fs.protected_positions, fs.recommended_positions,\
            fs.recommended_logical_bytes \
       FROM folder_stats fs "
}

/// 一次物化当前全部可见目录统计，供进程内文件夹快照缓存使用。
pub fn list_all_duplicate_folder_stats(
    conn: &Connection,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderStatsRow>> {
    let sql = format!(
        "{FOLDER_STATS_CTE} {} ORDER BY fs.folder_id ASC",
        folder_stats_projection()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![DEDUP_HASH_VERSION as i64, include_zero_byte as i64],
            folder_stats_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 获取一个可见目录的当前范围统计；范围恒含当前目录及全部后代目录。
pub fn get_duplicate_folder_stats(
    conn: &Connection,
    folder_id: i64,
    include_zero_byte: bool,
) -> Result<Option<DuplicateFolderStatsRow>> {
    let sql = format!(
        "{FOLDER_STATS_CTE} {} WHERE fs.folder_id = ?3",
        folder_stats_projection()
    );
    conn.query_row(
        &sql,
        params![
            DEDUP_HASH_VERSION as i64,
            include_zero_byte as i64,
            folder_id
        ],
        folder_stats_from_row,
    )
    .optional()
    .map_err(Into::into)
}

/// 列出候选目录。命中目录的祖先会被保留，以便前端能沿树展开到命中节点。
pub fn list_duplicate_folder_candidates(
    conn: &Connection,
    cursor: Option<&DuplicateFolderCandidateCursor>,
    limit: usize,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderStatsRow>> {
    let sql = format!(
        "{FOLDER_STATS_CTE},\
         candidate_hits(folder_id) AS (\
             SELECT folder_id FROM folder_stats \
              WHERE duplicate_positions > 0 OR unreviewed_positions > 0\
         ),\
         candidate_nodes(folder_id) AS (\
             SELECT folder_id FROM candidate_hits \
             UNION \
             SELECT fs.parent_id \
               FROM folder_stats fs \
               JOIN candidate_nodes cn ON cn.folder_id = fs.folder_id \
              WHERE fs.parent_id IS NOT NULL\
         )\
         {}\
        WHERE EXISTS (SELECT 1 FROM candidate_nodes cn WHERE cn.folder_id = fs.folder_id) \
          AND (?3 IS NULL OR (\
              CASE WHEN fs.recommended_positions > 0 THEN 1 ELSE 0 END < COALESCE(?4, 0) \
              OR (CASE WHEN fs.recommended_positions > 0 THEN 1 ELSE 0 END = COALESCE(?4, 0) \
                  AND (\
                    CAST(fs.recommended_positions AS REAL) * COALESCE(?6, 1)\
                        < CAST(COALESCE(?5, 0) AS REAL) * fs.total_positions \
                    OR (CAST(fs.recommended_positions AS REAL) * COALESCE(?6, 1) \
                            = CAST(COALESCE(?5, 0) AS REAL) * fs.total_positions \
                        AND (\
                            CAST(fs.external_covered_positions AS REAL) * COALESCE(?6, 1)\
                                < CAST(COALESCE(?7, 0) AS REAL) * fs.total_positions \
                            OR (CAST(fs.external_covered_positions AS REAL) * COALESCE(?6, 1) \
                                    = CAST(COALESCE(?7, 0) AS REAL) * fs.total_positions \
                                AND (\
                                    fs.recommended_logical_bytes < COALESCE(?8, 0) \
                                    OR (fs.recommended_logical_bytes = COALESCE(?8, 0) \
                                        AND (\
                                            (fs.total_positions - fs.recommended_positions)
                                                > COALESCE(?9, 0) \
                                            OR ((fs.total_positions - fs.recommended_positions)
                                                    = COALESCE(?9, 0) \
                                                AND (\
                                                     fs.depth > COALESCE(?10, 0) \
                                                     OR (fs.depth = COALESCE(?10, 0) \
                                                        AND fs.folder_id > COALESCE(?11, 0))\
                                                )\
                                            )\
                                        )\
                                    )\
                                )\
                            )\
                        )\
                    )\
                  )\
              )\
          ))\
        ORDER BY CASE WHEN fs.recommended_positions > 0 THEN 1 ELSE 0 END DESC,\
                 CAST(fs.recommended_positions AS REAL) / NULLIF(fs.total_positions, 0) DESC,\
                 CAST(fs.external_covered_positions AS REAL) / NULLIF(fs.total_positions, 0) DESC,\
                 fs.recommended_logical_bytes DESC,\
                 (fs.total_positions - fs.recommended_positions) ASC,\
                 fs.depth ASC, fs.folder_id ASC \
        LIMIT ?12",
        folder_stats_projection()
    );
    let cursor_values = cursor.map(|value| {
        (
            1_i64,
            value.actionable,
            value.recommended_positions,
            value.total_positions,
            value.external_covered_positions,
            value.recommended_logical_bytes,
            value.retained_positions,
            value.depth,
            value.folder_id,
        )
    });
    let values = if let Some((
        present,
        actionable,
        recommended,
        total,
        external,
        bytes,
        retained,
        depth,
        folder_id,
    )) = cursor_values
    {
        vec![
            Value::Integer(DEDUP_HASH_VERSION as i64),
            Value::Integer(include_zero_byte as i64),
            Value::Integer(present),
            Value::Integer(actionable),
            Value::Integer(recommended),
            Value::Integer(total),
            Value::Integer(external),
            Value::Integer(bytes),
            Value::Integer(retained),
            Value::Integer(depth),
            Value::Integer(folder_id),
            Value::Integer(i64::try_from(limit.max(1)).unwrap_or(i64::MAX)),
        ]
    } else {
        vec![
            Value::Integer(DEDUP_HASH_VERSION as i64),
            Value::Integer(include_zero_byte as i64),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Integer(i64::try_from(limit.max(1)).unwrap_or(i64::MAX)),
        ]
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), folder_stats_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 列出候选树中的一层。树序按名称/目录 id 稳定分页，候选优先级不会打散真实目录树。
pub fn list_duplicate_folder_tree_nodes(
    conn: &Connection,
    parent_id: Option<i64>,
    cursor: Option<&DuplicateFolderTreeCursor>,
    limit: usize,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderStatsRow>> {
    let sql = format!(
        "{FOLDER_STATS_CTE},
         candidate_hits(folder_id) AS (
             SELECT folder_id FROM folder_stats
              WHERE duplicate_positions > 0 OR unreviewed_positions > 0
         ),
         candidate_nodes(folder_id) AS (
             SELECT folder_id FROM candidate_hits
             UNION
             SELECT fs.parent_id
               FROM folder_stats fs
               JOIN candidate_nodes cn ON cn.folder_id = fs.folder_id
              WHERE fs.parent_id IS NOT NULL
         )
         {}
        WHERE EXISTS (SELECT 1 FROM candidate_nodes cn WHERE cn.folder_id = fs.folder_id)
          AND ((?3 IS NULL AND fs.parent_id IS NULL) OR fs.parent_id = ?3)
          AND (?4 IS NULL OR fs.name COLLATE NOCASE > ?4 COLLATE NOCASE
               OR (fs.name COLLATE NOCASE = ?4 COLLATE NOCASE AND fs.folder_id > ?5))
        ORDER BY fs.name COLLATE NOCASE ASC, fs.folder_id ASC
        LIMIT ?6",
        folder_stats_projection()
    );
    let values = vec![
        Value::Integer(DEDUP_HASH_VERSION as i64),
        Value::Integer(include_zero_byte as i64),
        optional_value(parent_id),
        cursor.map_or(Value::Null, |value| Value::Text(value.name.clone())),
        cursor.map_or(Value::Null, |value| Value::Integer(value.folder_id)),
        Value::Integer(i64::try_from(limit.max(1)).unwrap_or(i64::MAX)),
    ];
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), folder_stats_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 顶层候选目录；根数量通常很小，首版保持设计命令的无 cursor 形态。
pub fn list_duplicate_folder_roots(
    conn: &Connection,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderStatsRow>> {
    list_duplicate_folder_tree_nodes(conn, None, None, usize::MAX, include_zero_byte)
}

/// 候选树的直接子目录。
pub fn list_duplicate_folder_children(
    conn: &Connection,
    parent_id: i64,
    cursor: Option<&DuplicateFolderTreeCursor>,
    limit: usize,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderStatsRow>> {
    list_duplicate_folder_tree_nodes(conn, Some(parent_id), cursor, limit, include_zero_byte)
}

/// 返回当前可见目录元数据，供来源目录在 Rust 侧按祖先关系归并。
pub fn list_duplicate_folder_directories(
    conn: &Connection,
) -> Result<Vec<DuplicateFolderDirectoryRow>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.root_id, d.parent_id, d.rel_path, d.name, d.depth
           FROM directories d
           JOIN scan_roots r ON r.id = d.root_id
          WHERE r.is_active = 1 AND r.is_hidden = 0
          ORDER BY d.root_id ASC, d.rel_path ASC, d.id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DuplicateFolderDirectoryRow {
                id: row.get(0)?,
                root_id: row.get(1)?,
                parent_id: row.get(2)?,
                rel_path: row.get(3)?,
                name: row.get(4)?,
                depth: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

const FOLDER_ITEM_CTE: &str = r#"
WITH RECURSIVE
visible_dirs AS (
    SELECT d.id, d.root_id, d.parent_id, d.name, d.rel_path, d.depth
      FROM directories d
      JOIN scan_roots r ON r.id = d.root_id
     WHERE r.is_active = 1 AND r.is_hidden = 0
),
target_scope(id) AS (
    SELECT id FROM visible_dirs WHERE id = ?3
    UNION
    SELECT d.id FROM visible_dirs d JOIN target_scope s ON d.parent_id = s.id
),
visible_positions AS (
    SELECT m.id, m.source_revision, m.directory_id, m.file_name, m.file_size,
           m.media_type, m.width, m.height, m.duration_ms, m.thumb_status, m.thumb_path,
           m.thumbhash, m.is_live_photo, m.availability, m.is_favorited,
           COALESCE(m.rating, 0) AS rating,
           COALESCE(m.color_label, 0) AS color_label,
           d.root_id, d.rel_path, r.path AS root_path
      FROM media_items m
      JOIN visible_dirs d ON d.id = m.directory_id
      JOIN scan_roots r ON r.id = d.root_id
     WHERE m.is_deleted = 0 AND m.companion_of IS NULL
),
position_protection AS (
    SELECT p.*,
           (SELECT COUNT(*)
              FROM album_items ai JOIN albums a ON a.id = ai.album_id
             WHERE ai.item_id = p.id AND a.deleted_at IS NULL) AS album_count,
           (SELECT COUNT(*) FROM item_tags WHERE item_id = p.id) AS tag_count,
           (SELECT COUNT(*) FROM reader_bookmarks WHERE item_id = p.id) AS bookmark_count
      FROM visible_positions p
),
exact_members AS (
    SELECT p.id, p.source_revision, p.directory_id, p.availability, p.is_favorited,
           p.rating, p.color_label, p.album_count, p.tag_count, p.bookmark_count,
           di.unit_digest, di.unit_size, di.physical_key
      FROM position_protection p
      JOIN dedup_index di
        ON di.item_id = p.id AND di.source_revision = p.source_revision
     WHERE di.unit_digest IS NOT NULL
       AND di.unit_size IS NOT NULL
       AND di.hash_version = ?1
       AND di.status = 'ready'
       AND p.availability NOT IN ('missing', 'offline')
       AND (?2 = 1 OR p.file_size > 0)
),
groups AS (
    SELECT e.unit_digest, e.unit_size, COUNT(*) AS group_count,
           (SELECT keeper.id
              FROM exact_members keeper
             WHERE keeper.unit_digest = e.unit_digest
               AND keeper.unit_size = e.unit_size
             ORDER BY CASE WHEN keeper.availability = 'online' THEN 1 ELSE 0 END DESC,
                      CASE WHEN keeper.is_favorited <> 0
                                  OR keeper.rating > 0
                                  OR keeper.color_label > 0
                                  OR keeper.album_count > 0
                                  OR keeper.tag_count > 0
                                  OR keeper.bookmark_count > 0
                           THEN 1 ELSE 0 END DESC,
                      keeper.rating DESC,
                      CASE WHEN keeper.color_label > 0 THEN 1 ELSE 0 END DESC,
                      (keeper.album_count + keeper.tag_count + keeper.bookmark_count) DESC,
                      keeper.id ASC
             LIMIT 1) AS keeper_id
      FROM exact_members e
     GROUP BY e.unit_digest, e.unit_size
    HAVING COUNT(*) > 1
),
scoped_groups AS (
    SELECT e.unit_digest, e.unit_size, COUNT(*) AS scoped_count
      FROM exact_members e
      JOIN target_scope s ON s.id = e.directory_id
      JOIN groups g ON g.unit_digest = e.unit_digest AND g.unit_size = e.unit_size
     GROUP BY e.unit_digest, e.unit_size
)
"#;

fn folder_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DuplicateFolderItemRow> {
    let root_path: String = row.get(13)?;
    let rel_path: String = row.get(14)?;
    let file_name: String = row.get(3)?;
    let directory_path = if rel_path.is_empty() {
        root_path
    } else {
        resolve_media_path(&root_path, &rel_path, "")
    };
    Ok(DuplicateFolderItemRow {
        item_id: row.get(0)?,
        source_revision: row.get(1)?,
        directory_id: row.get(2)?,
        file_name,
        directory_path,
        file_size: row.get(4)?,
        media_type: row.get(5)?,
        width: row.get(6)?,
        height: row.get(7)?,
        duration_ms: row.get(8)?,
        thumb_status: row.get(9)?,
        thumb_path: row.get(10)?,
        thumbhash: row.get(11)?,
        is_live_photo: row.get::<_, i64>(12)? != 0,
        availability: row.get(15)?,
        is_favorited: row.get::<_, i64>(16)? != 0,
        rating: row.get(17)?,
        color_label: row.get(18)?,
        album_count: row.get(19)?,
        tag_count: row.get(20)?,
        bookmark_count: row.get(21)?,
        unit_digest: row.get(22)?,
        unit_size: row.get(23)?,
        physical_key: row.get(24)?,
        group_count: row.get(25)?,
        scoped_group_count: row.get(26)?,
        suggested_keeper_id: row.get(27)?,
        analysis_ready: row.get::<_, i64>(28)? != 0,
        physical_alias: row.get::<_, i64>(29)? != 0,
    })
}

/// 列出目标范围内全部当前主项，画廊按 item_id keyset 分页。
pub fn list_duplicate_folder_items(
    conn: &Connection,
    folder_id: i64,
    after_item_id: Option<i64>,
    limit: usize,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderItemRow>> {
    let sql = format!(
        "{FOLDER_ITEM_CTE} \
         SELECT p.id, p.source_revision, p.directory_id, p.file_name, p.file_size,
                p.media_type, p.width, p.height, p.duration_ms, p.thumb_status, p.thumb_path,
                p.thumbhash, p.is_live_photo, p.root_path, p.rel_path, p.availability,
                p.is_favorited, p.rating, p.color_label, p.album_count, p.tag_count,
                p.bookmark_count,
                e.unit_digest, e.unit_size, e.physical_key, g.group_count, sg.scoped_count,
                g.keeper_id, CASE WHEN e.id IS NOT NULL THEN 1 ELSE 0 END AS analysis_ready,
                CASE WHEN EXISTS (
                    SELECT 1 FROM exact_members peer
                     WHERE peer.unit_digest = e.unit_digest
                       AND peer.unit_size = e.unit_size
                       AND peer.physical_key IS NOT NULL
                       AND peer.physical_key = e.physical_key
                       AND peer.id <> e.id
                ) THEN 1 ELSE 0 END AS physical_alias
           FROM position_protection p
           JOIN target_scope target ON target.id = p.directory_id
           LEFT JOIN exact_members e ON e.id = p.id
           LEFT JOIN groups g ON g.unit_digest = e.unit_digest AND g.unit_size = e.unit_size
           LEFT JOIN scoped_groups sg
             ON sg.unit_digest = e.unit_digest AND sg.unit_size = e.unit_size
          WHERE (?4 IS NULL OR p.id > ?4)
          ORDER BY p.id ASC
          LIMIT ?5"
    );
    let values = vec![
        Value::Integer(DEDUP_HASH_VERSION as i64),
        Value::Integer(include_zero_byte as i64),
        Value::Integer(folder_id),
        optional_value(after_item_id),
        Value::Integer(i64::try_from(limit.max(1)).unwrap_or(i64::MAX)),
    ];
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), folder_item_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn folder_plan_member_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(Vec<u8>, i64, Option<i64>, DuplicateFolderPlanMemberRow)> {
    let digest: Vec<u8> = row.get(11)?;
    let unit_size: i64 = row.get(12)?;
    Ok((
        digest,
        unit_size,
        row.get(13)?,
        DuplicateFolderPlanMemberRow {
            item_id: row.get(0)?,
            directory_id: row.get(1)?,
            source_revision: row.get(2)?,
            in_target: row.get::<_, i64>(3)? != 0,
            availability: row.get(4)?,
            is_favorited: row.get::<_, i64>(5)? != 0,
            rating: row.get(6)?,
            color_label: row.get(7)?,
            album_count: row.get(8)?,
            tag_count: row.get(9)?,
            bookmark_count: row.get(10)?,
            physical_key: row.get(14)?,
        },
    ))
}

/// 读取与目标范围相交的完整 exact-ready 组，供纯计划生成器使用。
///
/// 结果只在后端内部物化；它不是 IPC 全库成员接口，也不把全量 ID 集合交给前端。
pub fn list_duplicate_folder_plan_groups(
    conn: &Connection,
    folder_id: i64,
    include_zero_byte: bool,
) -> Result<Vec<DuplicateFolderPlanGroupRow>> {
    let sql = format!(
        "{FOLDER_ITEM_CTE} \
         SELECT e.id, e.directory_id, e.source_revision,\
                CASE WHEN target.id IS NOT NULL THEN 1 ELSE 0 END AS in_target,\
                e.availability, e.is_favorited, e.rating, e.color_label,\
                e.album_count, e.tag_count, e.bookmark_count,\
                e.unit_digest, e.unit_size, g.keeper_id, e.physical_key \
           FROM exact_members e
           JOIN groups g ON g.unit_digest = e.unit_digest AND g.unit_size = e.unit_size
           JOIN scoped_groups sg ON sg.unit_digest = e.unit_digest AND sg.unit_size = e.unit_size
           LEFT JOIN target_scope target ON target.id = e.directory_id
          ORDER BY e.unit_digest ASC, e.unit_size ASC, e.id ASC"
    );
    let values = vec![
        Value::Integer(DEDUP_HASH_VERSION as i64),
        Value::Integer(include_zero_byte as i64),
        Value::Integer(folder_id),
    ];
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), folder_plan_member_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut groups: Vec<DuplicateFolderPlanGroupRow> = Vec::new();
    for (digest, unit_size, suggested_keeper_id, member) in rows {
        if let Some(group) = groups.last_mut() {
            if group.unit_digest == digest && group.unit_size == unit_size {
                group.members.push(member);
                continue;
            }
        }
        groups.push(DuplicateFolderPlanGroupRow {
            unit_digest: digest,
            unit_size,
            suggested_keeper_id,
            members: vec![member],
        });
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&conn).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             INSERT INTO scan_roots (id,path,alias) VALUES (1,'/root','root');
             INSERT INTO directories (id,root_id,rel_path,name) VALUES (10,1,'','root');",
        )
        .unwrap();
        assert!(begin_dedup_run(&conn, 1, false).unwrap());
        conn
    }

    fn add_in_directory(
        conn: &Connection,
        id: i64,
        directory_id: i64,
        name: &str,
        size: i64,
        companion_of: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO media_items
                (id,directory_id,file_name,file_size,file_mtime,file_mtime_ns,file_format,
                 media_type,width,height,sort_datetime,cache_key,companion_of)
             VALUES (?1,?2,?3,?4,1,1,'jpg','image',1,1,1,?1,?5)",
            params![id, directory_id, name, size, companion_of],
        )
        .unwrap();
    }

    fn add(conn: &Connection, id: i64, name: &str, size: i64, companion_of: Option<i64>) {
        add_in_directory(conn, id, 10, name, size, companion_of);
    }

    fn add_root(conn: &Connection, id: i64, path: &str) {
        conn.execute(
            "INSERT INTO scan_roots (id,path,alias) VALUES (?1,?2,?3)",
            params![id, path, path],
        )
        .unwrap();
    }

    fn add_directory_in_root(conn: &Connection, id: i64, root_id: i64, rel_path: &str, name: &str) {
        conn.execute(
            "INSERT INTO directories (id,root_id,rel_path,name) VALUES (?1,?2,?3,?4)",
            params![id, root_id, rel_path, name],
        )
        .unwrap();
    }

    fn write_ready_with_snapshot(
        conn: &Connection,
        id: i64,
        rev: i64,
        digest: &[u8],
        file_size: i64,
        file_mtime_ns: Option<i64>,
    ) -> bool {
        let written =
            write_working_ready_with_snapshot(conn, id, rev, digest, file_size, file_mtime_ns);
        if written {
            conn.execute(
                "INSERT OR REPLACE INTO dedup_index SELECT * FROM dedup_index_working WHERE item_id=?1",
                params![id],
            )
            .unwrap();
        }
        written
    }

    fn write_working_ready_with_snapshot(
        conn: &Connection,
        id: i64,
        rev: i64,
        digest: &[u8],
        file_size: i64,
        file_mtime_ns: Option<i64>,
    ) -> bool {
        let generation = dedup_run_generation(conn).unwrap().unwrap();
        write_dedup_index_if_generation_current(
            conn,
            &DedupIndexUpdate {
                item_id: id,
                source_revision: rev,
                file_size,
                file_mtime_ns,
                hash_version: 1,
                quick_digest: Some(digest),
                exact_digest: Some(digest),
                unit_digest: Some(digest),
                unit_size: Some(10),
                physical_key: None,
                status: STATUS_READY,
                error_code: None,
            },
            generation,
        )
        .unwrap()
    }

    fn write_ready(conn: &Connection, id: i64, rev: i64, digest: &[u8]) -> bool {
        let (file_size, file_mtime_ns): (i64, Option<i64>) = conn
            .query_row(
                "SELECT file_size, file_mtime_ns FROM media_items WHERE id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        write_ready_with_snapshot(conn, id, rev, digest, file_size, file_mtime_ns)
    }

    fn write_working_ready(conn: &Connection, id: i64, rev: i64, digest: &[u8]) -> bool {
        let (file_size, file_mtime_ns): (i64, Option<i64>) = conn
            .query_row(
                "SELECT file_size, file_mtime_ns FROM media_items WHERE id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        write_working_ready_with_snapshot(conn, id, rev, digest, file_size, file_mtime_ns)
    }

    #[test]
    fn lens_sees_only_successfully_published_results() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        add(&conn, 2, "b.jpg", 10, None);
        assert!(write_working_ready(&conn, 1, 1, b"pair"));
        assert!(write_working_ready(&conn, 2, 1, b"pair"));

        assert!(list_duplicate_lens_members(&conn).unwrap().is_empty());
        assert!(publish_dedup_run(&conn, 1).unwrap());
        assert_eq!(list_duplicate_lens_members(&conn).unwrap().len(), 2);
    }

    #[test]
    fn invalidated_run_keeps_previous_publication() {
        let conn = mem_db();
        for id in 1..=4 {
            add(&conn, id, &format!("{id}.jpg"), 10, None);
        }
        assert!(write_working_ready(&conn, 1, 1, b"old"));
        assert!(write_working_ready(&conn, 2, 1, b"old"));
        assert!(publish_dedup_run(&conn, 1).unwrap());

        assert!(begin_dedup_run(&conn, 2, true).unwrap());
        assert!(write_working_ready(&conn, 3, 1, b"new"));
        assert!(write_working_ready(&conn, 4, 1, b"new"));
        let running_ids = list_duplicate_lens_members(&conn)
            .unwrap()
            .into_iter()
            .map(|row| row.item_id)
            .collect::<Vec<_>>();
        assert_eq!(running_ids, vec![1, 2]);
        invalidate_dedup_run(&conn, 2).unwrap();

        assert!(!publish_dedup_run(&conn, 2).unwrap());
        let ids = list_duplicate_lens_members(&conn)
            .unwrap()
            .into_iter()
            .map(|row| row.item_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn folder_unique_evidence_is_hidden_until_full_publish() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        add(&conn, 2, "b.jpg", 10, None);
        add(&conn, 3, "unique.jpg", 20, None);
        assert!(write_working_ready(&conn, 1, 1, b"pair"));
        assert!(write_working_ready(&conn, 2, 1, b"pair"));
        assert!(write_working_ready(&conn, 3, 1, b"unique"));

        assert!(list_duplicate_folder_lens_rows(&conn).unwrap().is_empty());
        assert!(publish_dedup_run(&conn, 1).unwrap());
        let unique = list_duplicate_folder_lens_rows(&conn)
            .unwrap()
            .into_iter()
            .find(|row| row.item_id == 3)
            .unwrap();
        assert_eq!(unique.bucket, FolderLensBucket::Unique);
    }

    #[test]
    fn stale_revision_write_is_rejected() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        conn.execute("UPDATE media_items SET source_revision=2 WHERE id=1", [])
            .unwrap();
        assert!(!write_ready(&conn, 1, 1, b"x"));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dedup_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn stale_metadata_write_is_rejected_even_when_source_revision_matches() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);

        // 模拟分析期间文件被稳定替换，但扫描器尚未来得及推进 source_revision。
        conn.execute("UPDATE media_items SET file_size=11 WHERE id=1", [])
            .unwrap();
        assert!(
            !write_ready_with_snapshot(&conn, 1, 1, b"old-size", 10, Some(1)),
            "size 变化必须拒绝旧候选的摘要"
        );

        conn.execute(
            "UPDATE media_items SET file_size=10, file_mtime_ns=2 WHERE id=1",
            [],
        )
        .unwrap();
        assert!(
            !write_ready_with_snapshot(&conn, 1, 1, b"old-mtime", 10, Some(1)),
            "纳秒 mtime 变化必须拒绝旧候选的摘要"
        );
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dedup_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn groups_exclude_companions_and_require_matching_revision() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        add(&conn, 2, "b.jpg", 10, None);
        add(&conn, 3, "a.mov", 4, Some(1));
        assert!(write_ready(&conn, 1, 1, b"same"));
        assert!(write_ready(&conn, 2, 1, b"same"));
        assert!(write_ready(&conn, 3, 1, b"same"));
        let groups = list_duplicate_groups(&conn, None, 10, false, false, None).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].member_count, 2);
        assert_eq!(
            list_duplicate_members(&conn, b"same", 10, false, false, None, 10)
                .unwrap()
                .len(),
            2
        );
        conn.execute("UPDATE media_items SET source_revision=2 WHERE id=2", [])
            .unwrap();
        assert!(list_duplicate_groups(&conn, None, 10, false, false, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn groups_report_metadata_conflict_and_keeper_suggestion() {
        let conn = mem_db();
        add(&conn, 1, "favorite.jpg", 10, None);
        add(&conn, 2, "plain.jpg", 10, None);
        add(&conn, 3, "offline.jpg", 10, None);
        add(&conn, 4, "online.jpg", 10, None);
        conn.execute(
            "UPDATE media_items
                SET is_favorited=1, rating=5, color_label=2
              WHERE id=1",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE media_items SET availability='offline' WHERE id=3",
            [],
        )
        .unwrap();
        assert!(write_ready(&conn, 1, 1, b"protected"));
        assert!(write_ready(&conn, 2, 1, b"protected"));
        assert!(write_ready(&conn, 3, 1, b"availability"));
        assert!(write_ready(&conn, 4, 1, b"availability"));

        let groups = list_duplicate_groups(&conn, None, 10, true, false, None).unwrap();
        assert_eq!(groups.len(), 2);
        let protected = groups
            .iter()
            .find(|group| group.unit_digest.as_slice() == b"protected")
            .unwrap();
        assert!(protected.metadata_conflict);
        assert_eq!(protected.suggested_keeper_id, 1);

        let availability = groups
            .iter()
            .find(|group| group.unit_digest.as_slice() == b"availability")
            .unwrap();
        assert!(!availability.metadata_conflict);
        assert_eq!(availability.suggested_keeper_id, 4);
    }

    #[test]
    fn zero_byte_candidates_are_analyzed_but_hidden_by_default() {
        let conn = mem_db();
        add(&conn, 1, "a.zero", 0, None);
        add(&conn, 2, "b.zero", 0, None);
        conn.execute(
            "INSERT INTO dedup_index
                (item_id, source_revision, hash_version, exact_digest, unit_digest, unit_size, status, checked_at)
             VALUES (1, 1, 1, X'01', X'02', 0, 'ready', 1),
                    (2, 1, 1, X'03', X'02', 0, 'ready', 1)",
            [],
        )
        .unwrap();

        assert_eq!(
            list_duplicate_groups(&conn, None, 10, false, false, None)
                .unwrap()
                .len(),
            0,
            "零字节组默认隐藏"
        );
        let groups = list_duplicate_groups(&conn, None, 10, false, true, None).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            list_duplicate_members(&conn, &groups[0].unit_digest, 0, false, true, None, 10)
                .unwrap()
                .len(),
            2,
            "高级筛选产生的零字节组仍可审查成员"
        );
    }

    #[test]
    fn member_filters_match_group_filters() {
        let conn = mem_db();
        add(&conn, 1, "online.jpg", 10, None);
        add(&conn, 2, "online-copy.jpg", 10, None);
        add(&conn, 3, "offline.jpg", 10, None);
        add(&conn, 4, "zero-byte.jpg", 0, None);
        conn.execute(
            "UPDATE media_items SET availability='offline' WHERE id=3",
            [],
        )
        .unwrap();
        for id in 1..=4 {
            assert!(write_ready(&conn, id, 1, b"same"));
        }

        let default_groups = list_duplicate_groups(&conn, None, 10, false, false, None).unwrap();
        assert_eq!(default_groups.len(), 1);
        assert_eq!(default_groups[0].member_count, 2);
        let default_members =
            list_duplicate_members(&conn, b"same", 10, false, false, None, 10).unwrap();
        assert_eq!(
            default_members
                .iter()
                .map(|member| member.item_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        let all_members = list_duplicate_members(&conn, b"same", 10, true, true, None, 10).unwrap();
        assert_eq!(all_members.len(), 4);
    }

    #[test]
    fn companion_lookup_is_bounded() {
        let conn = mem_db();
        add(&conn, 1, "main.jpg", 10, None);
        for id in 2..=66 {
            add(&conn, id, &format!("part-{id}.mov"), 1, Some(1));
        }
        assert!(
            list_dedup_companions(&conn, 1).is_err(),
            "异常 companion 关系不能被无界物化"
        );
    }

    #[test]
    fn candidate_keyset_pages_without_full_id_array() {
        let conn = mem_db();
        for id in 1..=32 {
            add(&conn, id, &format!("{id}.jpg"), 99, None);
        }
        let first = list_dedup_scan_candidates(&conn, None, 7).unwrap();
        assert_eq!(first.len(), 7);
        let cursor = DedupCandidateCursor {
            file_size: first.last().unwrap().file_size,
            item_id: first.last().unwrap().item_id,
        };
        let second = list_dedup_scan_candidates(&conn, Some(&cursor), 7).unwrap();
        assert_eq!(second.len(), 7);
        assert!(second[0].item_id > first[6].item_id);
        assert_eq!(count_dedup_scan_candidates(&conn).unwrap(), 32);
    }

    #[test]
    fn group_keyset_pages_and_cursor_order_are_stable() {
        let conn = mem_db();
        for id in 1..=8 {
            add(&conn, id, &format!("{id}.jpg"), id, None);
        }
        for id in [1, 2, 3, 4, 5, 6] {
            let digest = if id <= 2 {
                b"a".as_slice()
            } else if id <= 4 {
                b"b"
            } else {
                b"c"
            };
            assert!(write_ready(&conn, id, 1, digest));
        }
        let first = list_duplicate_groups(&conn, None, 1, false, false, None).unwrap();
        assert_eq!(first.len(), 1);
        let cursor = DuplicateGroupCursor {
            unit_digest: first[0].unit_digest.clone(),
            unit_size: first[0].unit_size,
            first_item_id: first[0].first_item_id,
        };
        let rest = list_duplicate_groups(&conn, Some(&cursor), 10, false, false, None).unwrap();
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn group_cursor_uses_unit_group_index_before_aggregation() {
        let conn = mem_db();
        for id in 1..=12 {
            add(&conn, id, &format!("{id}.jpg"), 10, None);
            let digest = match id {
                1 | 2 => b"a".as_slice(),
                3 | 4 => b"b",
                5 | 6 => b"c",
                7 | 8 => b"d",
                9 | 10 => b"e",
                _ => b"f",
            };
            assert!(write_ready(&conn, id, 1, digest));
        }

        let cursor = DuplicateGroupCursor {
            unit_digest: b"b".to_vec(),
            unit_size: 10,
            first_item_id: 3,
        };
        let mut stmt = conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                build_duplicate_groups_sql(true)
            ))
            .unwrap();
        let plan = stmt
            .query_map(
                params![
                    DEDUP_HASH_VERSION as i64,
                    0_i64,
                    0_i64,
                    2_i64,
                    &cursor.unit_digest,
                    cursor.unit_size,
                    cursor.first_item_id,
                    2_i64,
                ],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        let uses_unit_group_index = plan
            .iter()
            .any(|detail| detail.contains("idx_dedup_unit_group"));
        assert!(
            uses_unit_group_index,
            "带组游标的 eligible 输入必须命中现有分组索引；plan={plan:?}"
        );
        assert!(
            plan.iter()
                .any(|detail| detail.contains("(unit_digest,unit_size)>(?,?)")),
            "分组索引计划必须包含游标下界范围；plan={plan:?}"
        );

        let after = list_duplicate_groups(&conn, Some(&cursor), 10, false, false, None).unwrap();
        assert_eq!(
            after
                .iter()
                .map(|group| group.unit_digest.as_slice())
                .collect::<Vec<_>>(),
            vec![b"c".as_slice(), b"d", b"e", b"f"]
        );
    }

    #[test]
    fn run_generation_is_monotonic_and_old_reset_is_noop() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        assert!(begin_dedup_run(&conn, 5, true).unwrap());
        assert!(write_ready(&conn, 1, 1, b"same"));
        assert!(!begin_dedup_run(&conn, 4, true).unwrap());
        assert_eq!(dedup_run_generation(&conn).unwrap(), Some(5));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dedup_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "迟到旧轮不能执行 reset");
    }

    #[test]
    fn generation_invalidation_rejects_late_sidecar_write() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        assert!(begin_dedup_run(&conn, 7, false).unwrap());
        invalidate_dedup_run(&conn, 7).unwrap();
        assert_eq!(dedup_run_generation(&conn).unwrap(), Some(8));
        let update = DedupIndexUpdate {
            item_id: 1,
            source_revision: 1,
            file_size: 10,
            file_mtime_ns: Some(1),
            hash_version: DEDUP_HASH_VERSION as i64,
            quick_digest: Some(b"late"),
            exact_digest: None,
            unit_digest: None,
            unit_size: None,
            physical_key: None,
            status: STATUS_QUICK,
            error_code: None,
        };
        assert!(!write_dedup_index_if_generation_current(&conn, &update, 7).unwrap());
    }

    #[test]
    fn folder_stats_use_descendant_scope_and_keep_unknown_separate() {
        let conn = mem_db();
        conn.execute_batch(
            "INSERT INTO directories (id,root_id,parent_id,rel_path,name,depth) VALUES
                 (11,1,10,'a','A',1),
                 (12,1,11,'a/nested','Nested',2);",
        )
        .unwrap();
        add_in_directory(&conn, 1, 11, "covered.jpg", 10, None);
        add_in_directory(&conn, 2, 10, "source.jpg", 10, None);
        add_in_directory(&conn, 3, 12, "internal.jpg", 10, None);
        add_in_directory(&conn, 4, 12, "internal-copy.jpg", 10, None);
        add_in_directory(&conn, 5, 11, "unique.jpg", 10, None);
        add_in_directory(&conn, 6, 12, "unknown.jpg", 10, None);
        conn.execute("UPDATE media_items SET is_favorited=1 WHERE id=4", [])
            .unwrap();
        assert!(write_ready(&conn, 1, 1, b"covered"));
        assert!(write_ready(&conn, 2, 1, b"covered"));
        assert!(write_ready(&conn, 3, 1, b"internal"));
        assert!(write_ready(&conn, 4, 1, b"internal"));
        assert!(write_ready(&conn, 5, 1, b"unique"));

        let stats = get_duplicate_folder_stats(&conn, 11, false)
            .unwrap()
            .expect("folder stats");
        assert_eq!(stats.total_positions, 5, "当前目录及后代各位置只计一次");
        assert_eq!(stats.analyzed_positions, 4);
        assert_eq!(stats.duplicate_positions, 3);
        assert_eq!(stats.external_covered_positions, 1);
        assert_eq!(stats.internal_duplicate_positions, 2);
        assert_eq!(stats.unreviewed_positions, 1);
        assert_eq!(stats.protected_positions, 1);
        assert_eq!(stats.recommended_positions, 2);
        assert_eq!(stats.recommended_logical_bytes, 20);

        let child_stats = get_duplicate_folder_stats(&conn, 12, false)
            .unwrap()
            .expect("child stats");
        assert_eq!(child_stats.total_positions, 3);
        assert_eq!(child_stats.duplicate_positions, 2);
        assert_eq!(child_stats.internal_duplicate_positions, 2);
        assert_eq!(child_stats.recommended_positions, 1);

        let groups = list_duplicate_folder_plan_groups(&conn, 11, false).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].members.len(), 2, "范围外 source 仍只作为证据成员");
        assert_eq!(groups[1].members.len(), 2);
        let items = list_duplicate_folder_items(&conn, 11, None, 50, false).unwrap();
        assert_eq!(items.len(), 5);
        assert!(items.iter().any(|item| item.item_id == 6));
        assert!(
            !items
                .iter()
                .find(|item| item.item_id == 6)
                .unwrap()
                .analysis_ready
        );

        let roots = list_duplicate_folder_tree_nodes(&conn, None, None, 20, false).unwrap();
        assert!(roots.iter().any(|row| row.folder_id == 10));
        let children = list_duplicate_folder_tree_nodes(&conn, Some(10), None, 20, false).unwrap();
        assert!(children.iter().any(|row| row.folder_id == 11));
    }

    #[test]
    fn folder_candidates_rank_coverage_ratio_before_parent_scope() {
        let conn = mem_db();
        conn.execute_batch(
            "INSERT INTO directories (id,root_id,parent_id,rel_path,name,depth) VALUES
                 (11,1,10,'a','A',1),
                 (12,1,10,'b','B',1);",
        )
        .unwrap();
        for index in 0..5 {
            let a_id = 100 + index;
            let b_id = 200 + index;
            add_in_directory(&conn, a_id, 11, &format!("a-{index}.jpg"), 10, None);
            add_in_directory(&conn, b_id, 12, &format!("b-{index}.jpg"), 10, None);
            assert!(write_ready(
                &conn,
                a_id,
                1,
                format!("shared-{index}").as_bytes()
            ));
            assert!(write_ready(
                &conn,
                b_id,
                1,
                format!("shared-{index}").as_bytes()
            ));
        }
        for index in 0..5 {
            let id = 300 + index;
            add_in_directory(&conn, id, 11, &format!("a-unique-{index}.jpg"), 10, None);
            assert!(write_ready(
                &conn,
                id,
                1,
                format!("a-unique-{index}").as_bytes()
            ));
        }
        add_in_directory(&conn, 400, 12, "b-unique.jpg", 10, None);
        assert!(write_ready(&conn, 400, 1, b"b-unique"));

        let candidates = list_duplicate_folder_candidates(&conn, None, 20, false).unwrap();
        let b_index = candidates
            .iter()
            .position(|row| row.folder_id == 12)
            .expect("B candidate");
        let a_index = candidates
            .iter()
            .position(|row| row.folder_id == 11)
            .expect("A candidate");
        assert!(b_index < a_index, "B 的 5/6 覆盖率应排在 A 的 5/10 前");
        assert_eq!(candidates[b_index].external_covered_positions, 5);
    }

    #[test]
    fn folder_stats_exclude_same_physical_alias_from_recommendations() {
        let conn = mem_db();
        add(&conn, 1, "original.jpg", 10, None);
        add(&conn, 2, "alias.jpg", 10, None);
        assert!(write_ready(&conn, 1, 1, b"same-file"));
        assert!(write_ready(&conn, 2, 1, b"same-file"));
        conn.execute(
            "UPDATE dedup_index SET physical_key=X'AA' WHERE item_id IN (1,2)",
            [],
        )
        .unwrap();

        let stats = get_duplicate_folder_stats(&conn, 10, false)
            .unwrap()
            .expect("root stats");
        assert_eq!(stats.duplicate_positions, 2);
        assert_eq!(stats.internal_duplicate_positions, 2);
        assert_eq!(stats.recommended_positions, 0);
        assert_eq!(stats.recommended_logical_bytes, 0);

        let items = list_duplicate_folder_items(&conn, 10, None, 10, false).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.physical_alias));
    }

    // ── 有效成员分类（2026-09-02 主画廊重复项浏览方案 §13 P0：固化分类边界）──────
    // （source_revision 漂移、Live Photo companion、include_offline 的 'offline' 侧与
    // include_zero_byte 已由 groups_exclude_companions_and_require_matching_revision /
    // member_filters_match_group_filters / zero_byte_candidates_are_analyzed_but_hidden_by_default
    // 覆盖，此处不重复；只补下列缺口。）

    /// 非 ready 状态行（stale/unstable/missing/error）即使 unit_digest 与 ready 组逐位一致，
    /// 也不进组：分组证据只认完整成功的精确分析，半成品状态一律排除。
    #[test]
    fn non_ready_status_rows_never_enter_groups() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        add(&conn, 2, "b.jpg", 10, None);
        assert!(write_ready(&conn, 1, 1, b"pair"));
        assert!(write_ready(&conn, 2, 1, b"pair"));

        for (id, status) in [
            (3, STATUS_STALE),
            (4, STATUS_UNSTABLE),
            (5, STATUS_MISSING),
            (6, STATUS_ERROR),
        ] {
            add(&conn, id, &format!("s{id}.jpg"), 10, None);
            conn.execute(
                "INSERT INTO dedup_index
                    (item_id, source_revision, hash_version, quick_digest, exact_digest,
                     unit_digest, unit_size, status, checked_at)
                 VALUES (?1, 1, ?2, ?3, ?3, ?3, 10, ?4, 1)",
                params![id, DEDUP_HASH_VERSION as i64, &b"pair"[..], status],
            )
            .unwrap();
        }

        // include_offline/include_zero_byte 全开：隔离 status 维度，不受其它排除通道干扰。
        let groups = list_duplicate_groups(&conn, None, 10, true, true, None).unwrap();
        assert_eq!(groups.len(), 1, "仍只有一个 ready 组");
        assert_eq!(groups[0].member_count, 2, "非 ready 行不得进组");
        let members = list_duplicate_members(&conn, b"pair", 10, true, true, None, 10).unwrap();
        assert_eq!(
            members.iter().map(|m| m.item_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    /// quick-only 行（有 quick_digest、exact/unit 为 NULL）永不进组：快照碰撞只进 quick
    /// collision 队列，分组必须等待精确分析落 ready。
    #[test]
    fn quick_only_rows_never_form_groups() {
        let conn = mem_db();
        for id in 1..=3 {
            add(&conn, id, &format!("q{id}.jpg"), 10, None);
            conn.execute(
                "INSERT INTO dedup_index
                    (item_id, source_revision, hash_version, quick_digest, status, checked_at)
                 VALUES (?1, 1, ?2, ?3, 'quick', 1)",
                params![id, DEDUP_HASH_VERSION as i64, &b"quick-digest"[..]],
            )
            .unwrap();
        }
        assert!(
            list_duplicate_groups(&conn, None, 10, true, true, None)
                .unwrap()
                .is_empty(),
            "quick-only 行不得成组"
        );
        assert!(
            list_duplicate_members(&conn, b"quick-digest", 10, true, true, None, 10)
                .unwrap()
                .is_empty()
        );
    }

    /// hash_version 不匹配的行不进组：摘要算法换代后旧结果不是有效证据。
    /// 双双降级 → 组消失；单行回当前版本 → 仍不成组（混行不得凑数）。
    #[test]
    fn hash_version_mismatch_rows_never_enter_groups() {
        let conn = mem_db();
        add(&conn, 1, "a.jpg", 10, None);
        add(&conn, 2, "b.jpg", 10, None);
        assert!(write_ready(&conn, 1, 1, b"same"));
        assert!(write_ready(&conn, 2, 1, b"same"));
        assert_eq!(
            list_duplicate_groups(&conn, None, 10, false, false, None)
                .unwrap()
                .len(),
            1
        );

        conn.execute("UPDATE dedup_index SET hash_version = hash_version - 1", [])
            .unwrap();
        assert!(list_duplicate_groups(&conn, None, 10, false, false, None)
            .unwrap()
            .is_empty());

        conn.execute(
            "UPDATE dedup_index SET hash_version = ?1 WHERE item_id = 1",
            params![DEDUP_HASH_VERSION as i64],
        )
        .unwrap();
        assert!(
            list_duplicate_groups(&conn, None, 10, false, false, None)
                .unwrap()
                .is_empty(),
            "版本混行不得成组"
        );

        conn.execute(
            "UPDATE dedup_index SET hash_version = ?1 WHERE item_id = 2",
            params![DEDUP_HASH_VERSION as i64],
        )
        .unwrap();
        assert_eq!(
            list_duplicate_groups(&conn, None, 10, false, false, None)
                .unwrap()
                .len(),
            1
        );
    }

    /// availability='missing' 与 'offline' 同等排除（include_offline=false）——
    /// member_filters_match_group_filters 只钉了 'offline'，此处补 'missing' 侧。
    #[test]
    fn missing_availability_is_excluded_like_offline() {
        let conn = mem_db();
        for (id, name) in [(1, "online.jpg"), (2, "offline.jpg"), (3, "missing.jpg")] {
            add(&conn, id, name, 10, None);
        }
        conn.execute(
            "UPDATE media_items SET availability='offline' WHERE id=2",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE media_items SET availability='missing' WHERE id=3",
            [],
        )
        .unwrap();
        for id in 1..=3 {
            assert!(write_ready(&conn, id, 1, b"same"));
        }

        // 默认（不含离线）：只剩 1 号在线项，凑不满组。
        assert!(list_duplicate_groups(&conn, None, 10, false, false, None)
            .unwrap()
            .is_empty());
        // 含离线：3 项全入组（'missing' 与 'offline' 同一排除通道）。
        let groups = list_duplicate_groups(&conn, None, 10, true, false, None).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].member_count, 3);
    }

    /// 组成员分页完整性：3 成员组经 list_duplicate_members 小 limit 逐页遍历，
    /// 汇总无重复、无遗漏，且与 list_duplicate_group_members_all 全量快照一致——
    /// 镜头布局（方案 §12.1）按成员流式建组，此分页契约是其正确性前提。
    #[test]
    fn duplicate_members_page_through_without_loss_or_duplication() {
        let conn = mem_db();
        for id in 1..=3 {
            add(&conn, id, &format!("m{id}.jpg"), 10, None);
            assert!(write_ready(&conn, id, 1, b"trio"));
        }

        let mut paged: Vec<i64> = Vec::new();
        let mut cursor: Option<i64> = None;
        loop {
            let page = list_duplicate_members(&conn, b"trio", 10, false, false, cursor, 2).unwrap();
            assert!(page.len() <= 2, "limit=2 不得超页");
            let done = page.len() < 2;
            paged.extend(page.iter().map(|m| m.item_id));
            cursor = page.last().map(|m| m.item_id);
            if done {
                break;
            }
        }

        let unique: std::collections::HashSet<i64> = paged.iter().copied().collect();
        assert_eq!(unique.len(), paged.len(), "分页不得重复返回成员");
        assert_eq!(paged, vec![1, 2, 3], "分页遍历无遗漏");

        let all = list_duplicate_group_members_all(&conn, b"trio", 10, false, false).unwrap();
        assert_eq!(
            all.iter().map(|m| m.item_id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "分页汇总与全量快照一致"
        );
    }

    /// 镜头成员与 组+成员 分页遍历的同边界一致性锁（方案 §3.1 硬约束的核心验证）：
    /// `list_duplicate_lens_members` 的 (组集, 每组成员集) 必须与
    /// `list_duplicate_groups` + `list_duplicate_members` 在默认严格口径下的汇总一致。
    #[test]
    fn lens_members_match_groups_and_members_pagination_boundary() {
        let conn = mem_db();
        add_root(&conn, 2, "/other");
        add_directory_in_root(&conn, 11, 1, "a", "a");
        add_directory_in_root(&conn, 12, 2, "b", "b");
        // pair-a：2 成员跨目录 + 1 个 companion（摘要已写入，但 companion 不是有效成员）；
        // pair-b：3 成员跨根；lone：单成员组；companion 也写 ready 摘要以防它混入边界。
        add(&conn, 1, "a1.jpg", 10, None);
        add_in_directory(&conn, 2, 11, "a2.jpg", 10, None);
        add(&conn, 7, "a1.mov", 4, Some(1));
        add(&conn, 3, "b1.jpg", 20, None);
        add_in_directory(&conn, 4, 12, "b2.jpg", 20, None);
        add_in_directory(&conn, 5, 11, "b3.jpg", 20, None);
        add(&conn, 6, "lone.jpg", 30, None);
        for (id, digest) in [(1, &b"pair-a"[..]), (2, b"pair-a"), (7, b"pair-a")] {
            assert!(write_ready(&conn, id, 1, digest));
        }
        for id in 3..=5 {
            assert!(write_ready(&conn, id, 1, b"pair-b"));
        }
        assert!(write_ready(&conn, 6, 1, b"lone"));

        // 期望侧：组查询（limit=1 逐组）+ 成员查询（limit=2 逐页）遍历汇总。
        let mut expected: std::collections::BTreeMap<(Vec<u8>, i64), Vec<i64>> = Default::default();
        let mut cursor: Option<DuplicateGroupCursor> = None;
        loop {
            let groups =
                list_duplicate_groups(&conn, cursor.as_ref(), 1, false, false, None).unwrap();
            if groups.is_empty() {
                break;
            }
            let group = &groups[0];
            let entry = expected
                .entry((group.unit_digest.clone(), group.unit_size))
                .or_default();
            let mut after: Option<i64> = None;
            loop {
                let members = list_duplicate_members(
                    &conn,
                    &group.unit_digest,
                    group.unit_size,
                    false,
                    false,
                    after,
                    2,
                )
                .unwrap();
                if members.is_empty() {
                    break;
                }
                entry.extend(members.iter().map(|m| m.item_id));
                after = members.last().map(|m| m.item_id);
            }
            cursor = Some(DuplicateGroupCursor {
                unit_digest: group.unit_digest.clone(),
                unit_size: group.unit_size,
                first_item_id: group.first_item_id,
            });
        }

        // 实际侧：镜头成员按组身份聚合。
        let lens = list_duplicate_lens_members(&conn).unwrap();
        let mut actual: std::collections::BTreeMap<(Vec<u8>, i64), Vec<i64>> = Default::default();
        for row in &lens {
            actual
                .entry((row.unit_digest.clone(), row.unit_size))
                .or_default()
                .push(row.item_id);
        }
        for members in expected.values_mut() {
            members.sort_unstable();
        }
        for members in actual.values_mut() {
            members.sort_unstable();
        }

        // fixture 护栏：两个组、companion 与单成员组都被组查询排除。
        assert_eq!(expected.len(), 2, "fixture 应恰好产出 pair-a/pair-b 两组");
        assert_eq!(
            expected.get(&(b"pair-a".to_vec(), 10)),
            Some(&vec![1, 2]),
            "companion(7) 不得进入有效边界"
        );
        // 同边界硬约束：两侧完全一致。
        assert_eq!(actual, expected, "镜头成员与 组+成员 分页遍历必须同边界");
    }

    /// 镜头成员排除不合格项：companion 与 source_revision 漂移无条件排除；
    /// offline/零字节经开关交叉验证——放宽后组查询能看到，证明 fixture 有效、
    /// 且镜头固定为严格口径（include_offline=false / include_zero_byte=false，方案 §3.4）。
    #[test]
    fn lens_members_exclude_ineligible_members() {
        let conn = mem_db();
        // 每对同摘要、一好一坏：若坏成员漏进 eligible，整组就会出现在镜头里。
        add(&conn, 1, "c1.jpg", 10, None);
        add(&conn, 2, "c2.jpg", 10, Some(1)); // companion
        add(&conn, 3, "r1.jpg", 10, None);
        add(&conn, 4, "r2.jpg", 10, None); // 下方推进 revision
        add(&conn, 5, "o1.jpg", 10, None);
        add(&conn, 6, "o2.jpg", 10, None); // 下方置 offline
        add(&conn, 7, "z1.jpg", 10, None);
        add(&conn, 8, "z2.jpg", 0, None); // 零字节
        assert!(write_ready(&conn, 1, 1, b"d-companion"));
        assert!(write_ready(&conn, 2, 1, b"d-companion"));
        assert!(write_ready(&conn, 3, 1, b"d-revision"));
        assert!(write_ready(&conn, 4, 1, b"d-revision"));
        assert!(write_ready(&conn, 5, 1, b"d-offline"));
        assert!(write_ready(&conn, 6, 1, b"d-offline"));
        assert!(write_ready(&conn, 7, 1, b"d-zero"));
        assert!(write_ready(&conn, 8, 1, b"d-zero"));
        conn.execute("UPDATE media_items SET source_revision=2 WHERE id=4", [])
            .unwrap();
        conn.execute(
            "UPDATE media_items SET availability='offline' WHERE id=6",
            [],
        )
        .unwrap();

        let lens = list_duplicate_lens_members(&conn).unwrap();
        assert!(
            lens.is_empty(),
            "不合格成员漏进 eligible 会使其所在组出现在镜头: {lens:?}"
        );

        // 交叉验证：放宽开关后组查询能看到对应组——镜头边界固定为严格口径。
        let offline_groups = list_duplicate_groups(&conn, None, 10, true, false, None).unwrap();
        assert!(offline_groups
            .iter()
            .any(|g| g.unit_digest.as_slice() == b"d-offline"));
        let zero_groups = list_duplicate_groups(&conn, None, 10, false, true, None).unwrap();
        assert!(zero_groups
            .iter()
            .any(|g| g.unit_digest.as_slice() == b"d-zero"));
    }

    /// normalized_dir_path 是跨根确定的全路径（root path + rel_path，与 folder 轴显示
    /// 路径同源）：同名 rel_path 在不同根下、以及根目录本身（rel_path=''）都能得到
    /// 互异、可排序的完整路径（方案 §6.3 的 normalized_directory_path）。
    #[test]
    fn lens_member_normalized_dir_path_is_full_sortable_path() {
        let conn = mem_db();
        add_root(&conn, 2, "/other");
        add_directory_in_root(&conn, 11, 1, "a", "a");
        add_directory_in_root(&conn, 12, 2, "a", "a");
        add(&conn, 1, "x.jpg", 10, None); // dir 10 = 根1 的根目录（rel_path=''）
        add_in_directory(&conn, 2, 11, "x.jpg", 10, None); // /root/a
        add_in_directory(&conn, 3, 12, "x.jpg", 10, None); // /other/a
        for id in 1..=3 {
            assert!(write_ready(&conn, id, 1, b"same"));
        }

        let rows = list_duplicate_lens_members(&conn).unwrap();
        assert_eq!(rows.len(), 3);
        let mut ids: Vec<(i64, i64)> = rows.iter().map(|r| (r.item_id, r.directory_id)).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![(1, 10), (2, 11), (3, 12)],
            "directory_id 投影正确"
        );
        let mut paths: Vec<&str> = rows
            .iter()
            .map(|r| r.normalized_dir_path.as_str())
            .collect();
        assert!(paths.iter().all(|p| !p.is_empty()));
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec!["/other/a", "/root", "/root/a"],
            "完整路径跨根互异且可排序"
        );
    }

    // ── folders 镜头行（2026-09-02 主画廊重复项浏览方案 §3.4/§7.5 P3）────────────

    /// quick-only 摘要行（无 exact/unit，status='quick'）。
    fn write_quick(conn: &Connection, id: i64, digest: &[u8]) {
        conn.execute(
            "INSERT INTO dedup_index
                (item_id, source_revision, hash_version, quick_digest, status, checked_at)
             VALUES (?1, 1, ?2, ?3, 'quick', 1)",
            params![id, DEDUP_HASH_VERSION as i64, digest],
        )
        .unwrap();
    }

    /// 指定状态的非 ready 行（摘要字段与 ready 行同构，用于验证状态门禁）。
    fn write_status_row(conn: &Connection, id: i64, digest: &[u8], status: &str) {
        conn.execute(
            "INSERT INTO dedup_index
                (item_id, source_revision, hash_version, quick_digest, exact_digest,
                 unit_digest, unit_size, status, checked_at)
             VALUES (?1, 1, ?2, ?3, ?3, ?3, 10, ?4, 1)",
            params![id, DEDUP_HASH_VERSION as i64, digest, status],
        )
        .unwrap();
    }

    /// 在目录 10 放一对 ready 重复锚点，使目录稳定处于「纳入」状态——否则分类
    /// 断言会因目录未纳入而空转变绿（方案 §3.4 末段的纳入过滤）。
    fn add_duplicate_anchor(conn: &Connection) {
        add(conn, 100, "anchor-a.jpg", 999, None);
        add(conn, 101, "anchor-b.jpg", 999, None);
        assert!(write_ready(conn, 100, 1, b"anchor"));
        assert!(write_ready(conn, 101, 1, b"anchor"));
    }

    fn folder_lens_buckets(conn: &Connection) -> std::collections::BTreeMap<i64, FolderLensBucket> {
        list_duplicate_folder_lens_rows(conn)
            .unwrap()
            .into_iter()
            .map(|row| (row.item_id, row.bucket))
            .collect()
    }

    /// 判定表（§3.4）：Duplicate、独有证据①②③、quick 碰撞未定案 → 尚未确认；
    /// 重复行携带组身份。
    #[test]
    fn folder_lens_classifies_duplicate_and_unique_evidence() {
        let conn = mem_db();
        add_duplicate_anchor(&conn);
        // 证据①：ready 单成员组（有效精确摘要无副本）。
        add(&conn, 1, "lone.jpg", 10, None);
        assert!(write_ready(&conn, 1, 1, b"lone"));
        // 证据②：唯一大小桶 quick 行（无同大小项则无副本可能）。
        add(&conn, 2, "solo-size.jpg", 77, None);
        write_quick(&conn, 2, b"quick-solo");
        // 证据③：同大小两项 quick 互异（quick 已排除碰撞）。
        add(&conn, 3, "quick-a.jpg", 55, None);
        add(&conn, 4, "quick-b.jpg", 55, None);
        write_quick(&conn, 3, b"quick-three-a");
        write_quick(&conn, 4, b"quick-three-b");
        // quick 碰撞：同大小同 quick → exact 候选未定案 → 尚未确认。
        add(&conn, 5, "collide-a.jpg", 66, None);
        add(&conn, 6, "collide-b.jpg", 66, None);
        write_quick(&conn, 5, b"quick-collide");
        write_quick(&conn, 6, b"quick-collide");

        let rows = list_duplicate_folder_lens_rows(&conn).unwrap();
        assert_eq!(rows.len(), 8, "纳入目录的全部可见行都返回");
        let buckets = folder_lens_buckets(&conn);
        assert_eq!(buckets.get(&100), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&101), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&1), Some(&FolderLensBucket::Unique), "证据①");
        assert_eq!(buckets.get(&2), Some(&FolderLensBucket::Unique), "证据②");
        assert_eq!(buckets.get(&3), Some(&FolderLensBucket::Unique), "证据③");
        assert_eq!(buckets.get(&4), Some(&FolderLensBucket::Unique), "证据③");
        assert_eq!(buckets.get(&5), Some(&FolderLensBucket::Unconfirmed));
        assert_eq!(buckets.get(&6), Some(&FolderLensBucket::Unconfirmed));
        // 重复行携带组身份（unit_digest + unit_size，方案 §3.1）。
        let anchor = rows.iter().find(|row| row.item_id == 100).unwrap();
        assert_eq!(anchor.unit_digest.as_deref(), Some(b"anchor".as_slice()));
        assert_eq!(anchor.unit_size, Some(10));
    }

    /// 判定表（§3.4）：无 di 行、非 ready 状态、hash_version 不匹配、修订漂移 →
    /// 尚未确认；stale 行的同摘要不得抬高有效组规模（分组计数只认有效行）。
    #[test]
    fn folder_lens_unconfirmed_without_valid_evidence() {
        let conn = mem_db();
        add_duplicate_anchor(&conn);
        add(&conn, 7, "no-index.jpg", 88, None);
        // 非 ready 状态即使 unit 摘要与 ready 组逐位一致也不进重复桶。
        add(&conn, 8, "stale.jpg", 21, None);
        add(&conn, 9, "unstable.jpg", 22, None);
        add(&conn, 10, "error.jpg", 23, None);
        write_status_row(&conn, 8, b"anchor", STATUS_STALE);
        write_status_row(&conn, 9, b"anchor", STATUS_UNSTABLE);
        write_status_row(&conn, 10, b"anchor", STATUS_ERROR);
        // hash_version 旧：摘要算法换代后旧结果不是有效证据。
        add(&conn, 11, "old-hash.jpg", 31, None);
        assert!(write_ready(&conn, 11, 1, b"old-hash"));
        conn.execute(
            "UPDATE dedup_index SET hash_version = hash_version - 1 WHERE item_id = 11",
            [],
        )
        .unwrap();
        // source_revision 漂移：漂移行本身未确认；留下的有效单成员归独有（证据①）。
        add(&conn, 12, "drift-a.jpg", 32, None);
        add(&conn, 13, "drift-b.jpg", 32, None);
        assert!(write_ready(&conn, 12, 1, b"drift-pair"));
        assert!(write_ready(&conn, 13, 1, b"drift-pair"));
        conn.execute(
            "UPDATE media_items SET source_revision = 2 WHERE id = 12",
            [],
        )
        .unwrap();

        let buckets = folder_lens_buckets(&conn);
        for id in [7, 8, 9, 10, 11, 12] {
            assert_eq!(
                buckets.get(&id),
                Some(&FolderLensBucket::Unconfirmed),
                "item {id} 应为尚未确认"
            );
        }
        assert_eq!(buckets.get(&100), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&101), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&13), Some(&FolderLensBucket::Unique));
    }

    /// 判定表（§3.4）：offline/missing/零字节即使 ready 成组也不进重复桶（folders
    /// 语义）；同组的在线侧按有效单成员归独有（严格口径）。
    #[test]
    fn folder_lens_offline_missing_and_zero_byte_stay_unconfirmed() {
        let conn = mem_db();
        add_duplicate_anchor(&conn);
        add(&conn, 14, "offline-a.jpg", 41, None);
        add(&conn, 15, "offline-b.jpg", 41, None);
        assert!(write_ready(&conn, 14, 1, b"pair-offline"));
        assert!(write_ready(&conn, 15, 1, b"pair-offline"));
        conn.execute(
            "UPDATE media_items SET availability = 'offline' WHERE id = 14",
            [],
        )
        .unwrap();
        add(&conn, 16, "missing-a.jpg", 42, None);
        add(&conn, 17, "missing-b.jpg", 42, None);
        assert!(write_ready(&conn, 16, 1, b"pair-missing"));
        assert!(write_ready(&conn, 17, 1, b"pair-missing"));
        conn.execute(
            "UPDATE media_items SET availability = 'missing' WHERE id = 16",
            [],
        )
        .unwrap();
        add(&conn, 18, "zero-a.jpg", 0, None);
        add(&conn, 19, "zero-b.jpg", 0, None);
        assert!(write_ready(&conn, 18, 1, b"pair-zero"));
        assert!(write_ready(&conn, 19, 1, b"pair-zero"));

        let buckets = folder_lens_buckets(&conn);
        for id in [14, 16, 18, 19] {
            assert_eq!(
                buckets.get(&id),
                Some(&FolderLensBucket::Unconfirmed),
                "item {id} 应为尚未确认"
            );
        }
        assert_eq!(buckets.get(&15), Some(&FolderLensBucket::Unique));
        assert_eq!(buckets.get(&17), Some(&FolderLensBucket::Unique));
    }

    /// 纳入过滤（§3.4 末段）：只纳入至少含一个当前有效重复成员的直接文件夹；
    /// 纳入目录返回全部三类行；纯独有/纯未确认目录不返回任何行。
    #[test]
    fn folder_lens_includes_only_directories_with_duplicate_members() {
        let conn = mem_db();
        // dir 11：重复 + 未确认 + 独有 三类混合。
        add_directory_in_root(&conn, 11, 1, "mixed", "mixed");
        add_in_directory(&conn, 20, 11, "dup-a.jpg", 50, None);
        add_in_directory(&conn, 21, 11, "dup-b.jpg", 50, None);
        assert!(write_ready(&conn, 20, 1, b"dir-pair"));
        assert!(write_ready(&conn, 21, 1, b"dir-pair"));
        add_in_directory(&conn, 22, 11, "pending.jpg", 51, None);
        add_in_directory(&conn, 23, 11, "unique.jpg", 52, None);
        write_quick(&conn, 23, b"dir-unique");
        // dir 12：纯独有。
        add_directory_in_root(&conn, 12, 1, "unique-only", "unique-only");
        add_in_directory(&conn, 24, 12, "u1.jpg", 60, None);
        write_quick(&conn, 24, b"u-one");
        add_in_directory(&conn, 25, 12, "u2.jpg", 61, None);
        assert!(write_ready(&conn, 25, 1, b"u-two"));
        // dir 13：纯未确认（quick 碰撞）。
        add_directory_in_root(&conn, 13, 1, "pending-only", "pending-only");
        add_in_directory(&conn, 26, 13, "p1.jpg", 70, None);
        add_in_directory(&conn, 27, 13, "p2.jpg", 70, None);
        write_quick(&conn, 26, b"p-collide");
        write_quick(&conn, 27, b"p-collide");

        let rows = list_duplicate_folder_lens_rows(&conn).unwrap();
        assert!(
            rows.iter().all(|row| row.directory_id == 11),
            "只返回纳入目录的行"
        );
        let buckets: std::collections::BTreeMap<i64, FolderLensBucket> = rows
            .into_iter()
            .map(|row| (row.item_id, row.bucket))
            .collect();
        assert_eq!(buckets.get(&20), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&21), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&22), Some(&FolderLensBucket::Unconfirmed));
        assert_eq!(buckets.get(&23), Some(&FolderLensBucket::Unique));
        for id in [24, 25, 26, 27] {
            assert!(!buckets.contains_key(&id), "非纳入目录的行不得返回: {id}");
        }
    }

    /// companion 与软删项不返回；其摘要也不得抬高有效组规模。
    #[test]
    fn folder_lens_excludes_companions_and_soft_deleted() {
        let conn = mem_db();
        add_duplicate_anchor(&conn);
        add(&conn, 102, "anchor.mov", 4, Some(100));
        assert!(write_ready(&conn, 102, 1, b"anchor"));
        add(&conn, 103, "deleted.jpg", 999, None);
        assert!(write_ready(&conn, 103, 1, b"anchor"));
        conn.execute("UPDATE media_items SET is_deleted = 1 WHERE id = 103", [])
            .unwrap();

        let rows = list_duplicate_folder_lens_rows(&conn).unwrap();
        assert_eq!(rows.len(), 2, "只剩两个锚点行");
        let ids: std::collections::HashSet<i64> = rows.iter().map(|row| row.item_id).collect();
        assert!(!ids.contains(&102), "companion 不得返回");
        assert!(!ids.contains(&103), "软删项不得返回");
        let buckets: std::collections::BTreeMap<i64, FolderLensBucket> = rows
            .into_iter()
            .map(|row| (row.item_id, row.bucket))
            .collect();
        assert_eq!(buckets.get(&100), Some(&FolderLensBucket::Duplicate));
        assert_eq!(buckets.get(&101), Some(&FolderLensBucket::Duplicate));
    }

    /// 跨目录重复组：组员分布的每个目录都纳入，目录内非重复行也随目录返回。
    #[test]
    fn folder_lens_cross_directory_group_includes_every_member_folder() {
        let conn = mem_db();
        add_root(&conn, 2, "/other");
        add_directory_in_root(&conn, 11, 1, "a", "a");
        add_directory_in_root(&conn, 12, 2, "b", "b");
        add(&conn, 30, "in-root.jpg", 80, None);
        add_in_directory(&conn, 31, 11, "in-a.jpg", 80, None);
        add_in_directory(&conn, 32, 12, "in-b.jpg", 80, None);
        assert!(write_ready(&conn, 30, 1, b"cross-pair"));
        assert!(write_ready(&conn, 31, 1, b"cross-pair"));
        assert!(write_ready(&conn, 32, 1, b"cross-pair"));
        add_in_directory(&conn, 33, 12, "extra.jpg", 81, None);

        let buckets = folder_lens_buckets(&conn);
        for id in [30, 31, 32] {
            assert_eq!(
                buckets.get(&id),
                Some(&FolderLensBucket::Duplicate),
                "item {id}"
            );
        }
        assert_eq!(buckets.get(&33), Some(&FolderLensBucket::Unconfirmed));
    }
}
