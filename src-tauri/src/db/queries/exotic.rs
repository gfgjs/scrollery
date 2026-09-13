//! 冷门格式插件域 DAO:task seed/claim/lease/finish/fail/recover、plugin CRUD/status、
//! item source(T 线拆分自 queries.rs,SQL 与行为不变;lease/status guard 原样)。
//! 「exotic 接管门控」谓词 NOT_BLOCKED_BY_EXOTIC(_M) 归本域(P0 增补裁决):与
//! `has_blocking_exotic_thumbnail_task` 同一不变量,改任务状态语义时须同步双处。

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::scan::EXCLUDE_HIDDEN_ROOT_ITEMS;
use crate::error::{AppError, Result};
use crate::exotic::task::{ExoticTaskRow, ExoticTaskStatus};

// 主缩略图 pending 查询统一排除「有未完成 exotic thumbnail 任务」的项（v3 §6.2 / Part1 §2.2）。
// 与 §2.4 给 CLIP/face 的门控同一模式：exotic 项由 Worker 流水线出图，绝不进主 generator
// （主解码引擎无法解码 PSD 等冷门格式，否则会 UnsupportedFormat / 写 thumb_status=2）。
pub(in crate::db::queries) const NOT_BLOCKED_BY_EXOTIC: &str = "AND NOT EXISTS (
        SELECT 1 FROM exotic_tasks et
        WHERE et.item_id = media_items.id AND et.capability='thumbnail' AND et.status<>2)";

/// 同上，但用于以 `m` 为 media_items 别名的查询（CLIP/face/derive 生产者，§2.4）。
/// exotic 已认领 thumbnail 的 item 在其完成前，不进 CLIP/人脸/主派生；完成后这些流水线
/// 优先用生成的 thumb_path（WebP），避免再尝试解码原始 PSD。
pub(in crate::db::queries) const NOT_BLOCKED_BY_EXOTIC_M: &str = "AND NOT EXISTS (
        SELECT 1 FROM exotic_tasks et
        WHERE et.item_id = m.id AND et.capability='thumbnail' AND et.status<>2)";

// ════════════════════════════════════════════════════════════════════════════
// 冷门格式插件 · 任务 DAO（v3 Part1 §1.6 / 勘误 R2）
// ════════════════════════════════════════════════════════════════════════════
//
// 状态码（与 ExoticTaskStatus 同步）：0=pending 1=processing 2=done 3=retryable 4=terminal。
// SQL 内只能用整数字面量；变更须与 src/exotic/task.rs 同步。
//
// 原子领取与租约（R2）：
//   - 领取用单条 `UPDATE ... WHERE id IN (SELECT ... LIMIT) RETURNING`，一句完成 = 原子；
//     条件 `status IN (0,3)` 防两实例重复领取（输的一方 UPDATE 命中 0 行）。
//   - `lease_owner`(进程级 instance_id) + ttl：防活实例任务被误恢复、隔离过期 Writer。
//   - finish/fail 的最终更新均带 `status=1 AND lease_owner=?`，失去租约的旧结果只能丢弃。

/// `exotic_tasks` 全列（顺序与 `map_exotic_task` 严格一致）。
const EXOTIC_TASK_COLS: &str = "id, item_id, plugin_id, capability, status, input_fingerprint, \
    attempts, next_retry_at, claimed_at, lease_owner, last_error_code, last_error_message, \
    output_path, worker_version";

fn map_exotic_task(row: &Row<'_>) -> rusqlite::Result<ExoticTaskRow> {
    let status_i: i64 = row.get(4)?;
    Ok(ExoticTaskRow {
        id: row.get(0)?,
        item_id: row.get(1)?,
        plugin_id: row.get(2)?,
        capability: row.get(3)?,
        // 未知状态码视为 Pending（损坏行不致 panic；领取条件会自然忽略非 0/3）。
        status: ExoticTaskStatus::from_i64(status_i).unwrap_or(ExoticTaskStatus::Pending),
        input_fingerprint: row.get(5)?,
        attempts: row.get(6)?,
        next_retry_at: row.get(7)?,
        claimed_at: row.get(8)?,
        lease_owner: row.get(9)?,
        last_error_code: row.get(10)?,
        last_error_message: row.get(11)?,
        output_path: row.get(12)?,
        worker_version: row.get(13)?,
    })
}

/// 为单个 item 按 capabilities 播种任务（扫描事务内调用）。已存在则 IGNORE（UNIQUE 约束）。
pub fn seed_exotic_tasks_for_item(
    conn: &Connection,
    item_id: i64,
    plugin_id: &str,
    capabilities: &[String],
) -> Result<()> {
    for cap in capabilities {
        conn.execute(
            "INSERT OR IGNORE INTO exotic_tasks (item_id, plugin_id, capability) VALUES (?1,?2,?3)",
            params![item_id, plugin_id, cap],
        )?;
    }
    Ok(())
}

/// 集合式 backfill：为某格式所有未拥有该 (plugin,capability) 任务的现存媒体补建任务。
/// Catalog 更新后调用（只为已登记格式建任务，不重跑 enrichment）。返回新建行数。
pub fn backfill_exotic_tasks_for_format(
    conn: &Connection,
    format: &str,
    plugin_id: &str,
    capability: &str,
) -> Result<usize> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO exotic_tasks (item_id, plugin_id, capability)
         SELECT id, ?2, ?3 FROM media_items
         WHERE file_format=?1 AND is_deleted=0",
        params![format, plugin_id, capability],
    )?)
}

/// SourceChanged 失效：把该 item 全部 exotic 任务退回 pending，清空输出/指纹/错误/租约。
/// 注意：本函数只管 DB；旧产物文件由调用方/维护任务清理（Part2 Sink 重做时覆盖）。返回受影响行数。
pub fn invalidate_exotic_tasks_for_item(conn: &Connection, item_id: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, input_fingerprint=NULL, attempts=0, next_retry_at=NULL,
             claimed_at=NULL, lease_owner=NULL, last_error_code=NULL, last_error_message=NULL,
             output_path=NULL, worker_version=NULL, updated_at=strftime('%s','now')
         WHERE item_id=?1",
        params![item_id],
    )?)
}

/// 升级 Worker 后失效：把该插件「已完成但 worker_version 不同」的任务退回 pending（指纹会变）。
/// 只失效受影响 capability/版本的任务，不动其他插件。返回受影响行数。
pub fn invalidate_exotic_tasks_for_plugin_version(
    conn: &Connection,
    plugin_id: &str,
    new_worker_version: &str,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, input_fingerprint=NULL, claimed_at=NULL, lease_owner=NULL,
             updated_at=strftime('%s','now')
         WHERE plugin_id=?1 AND status=2 AND (worker_version IS NULL OR worker_version<>?2)",
        params![plugin_id, new_worker_version],
    )?)
}

/// 全量重建 / 清空缩略图语义：把所有 thumbnail exotic 任务退回 pending，清空旧输出/指纹/错误/租约。
/// 必须与主缩略图「全部重做」对齐——否则 done(2) 任务因 worker_version 仍在，**既不**被 Coordinator
/// 重领（claim 只取 0/3），**又因** `NOT_BLOCKED_BY_EXOTIC`（status<>2 才算 blocking）把已清空的 PSD
/// 放回主 generator → UnsupportedFormat（违 R3/R7、Part1 DoD#4，问题1）。
/// 不动 processing(1)：本实例在途任务自然完成，其结果对清空后的 media_items 仍有效（Sink 会重写 thumb_path）。
/// 返回受影响行数。调用方须在重置后 `wake_exotic` 让 Coordinator 重领。
pub fn reset_all_exotic_thumbnail_tasks(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, input_fingerprint=NULL, attempts=0, next_retry_at=NULL,
             claimed_at=NULL, lease_owner=NULL, last_error_code=NULL, last_error_message=NULL,
             output_path=NULL, worker_version=NULL, updated_at=strftime('%s','now')
         WHERE capability='thumbnail' AND status IN (2,3,4)",
        [],
    )?)
}

/// 跨流水线门控：该 item 是否仍有**未完成**的 thumbnail exotic 任务。
/// CLIP/人脸/派生在此为 true 时不应处理该 item（v3 §6.3 / Part1 §2.4）。
pub fn has_blocking_exotic_thumbnail_task(conn: &Connection, item_id: i64) -> Result<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM exotic_tasks
            WHERE item_id=?1 AND capability='thumbnail' AND status<>2)",
        params![item_id],
        |r| r.get(0),
    )?;
    Ok(exists != 0)
}

/// 写缓存前的轻量源快照预检。
///
/// 这是 finish CAS 之前的降噪闸门：源已变或已被软删除时，不让迟到 Worker 先把同一
/// `cache_key` 的缓存文件覆盖一遍。最终正确性仍由 [`finish_exotic_task`] 和条件封面写回
/// 再次保证；调用方必须在返回后释放 writer 锁，再执行文件 IO。
pub fn is_exotic_source_current(
    conn: &Connection,
    item_id: i64,
    expected_source_revision: i64,
    expected_cache_key: i64,
) -> Result<bool> {
    let current: Option<(i64, i64)> = conn
        .query_row(
            "SELECT source_revision, cache_key FROM media_items
             WHERE id=?1 AND is_deleted=0",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(current == Some((expected_source_revision, expected_cache_key)))
}

/// 原子领取就绪任务（pending 或到期 retryable）；写 processing + claimed_at + lease_owner。
/// 单条 UPDATE...RETURNING 完成领取，避免 SELECT/UPDATE 之间的竞态窗口（R2）。
/// 隐藏根排除（V21）：第 5 条生成线与缩略图/AI/人脸/派生同口径——隐藏根下的 exotic 任务
/// 不领取（留在 pending，不烧解码算力）；取消隐藏后由 `wake_exotic` 踢 Coordinator 重领。
pub fn claim_exotic_tasks(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
    limit: i64,
    instance_id: &str,
    now: i64,
) -> Result<Vec<ExoticTaskRow>> {
    let sql = format!(
        "UPDATE exotic_tasks
         SET status=1, claimed_at=?3, lease_owner=?4, updated_at=strftime('%s','now')
         WHERE id IN (
             SELECT id FROM exotic_tasks
             WHERE plugin_id=?1 AND capability=?2
               AND ( status=0 OR (status=3 AND (next_retry_at IS NULL OR next_retry_at<=?3)) )
               {EXCLUDE_HIDDEN_ROOT_ITEMS}
             ORDER BY id LIMIT ?5 )
         RETURNING {EXOTIC_TASK_COLS}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![plugin_id, capability, now, instance_id, limit],
            map_exotic_task,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 续租（Supervisor 周期调用）：仅当仍持有租约时更新 claimed_at。返回是否续租成功。
pub fn renew_exotic_lease(conn: &Connection, id: i64, instance_id: &str, now: i64) -> Result<bool> {
    let n = conn.execute(
        "UPDATE exotic_tasks SET claimed_at=?3 WHERE id=?1 AND status=1 AND lease_owner=?2",
        params![id, instance_id, now],
    )?;
    Ok(n == 1)
}

/// 批量续租：刷新本实例**全部**在途 processing 任务的 claimed_at（R2，问题2）。
/// 一次 claim 可领多个、再经 channel/worker 排队消化，单 claimed_at 不续则排队中任务会在
/// lease_ttl 后被第二实例的孤儿恢复误回收。续租周期须 << lease_ttl。返回续租行数。
pub fn renew_all_exotic_leases(conn: &Connection, instance_id: &str, now: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks SET claimed_at=?2 WHERE status=1 AND lease_owner=?1",
        params![instance_id, now],
    )?)
}

/// 完成任务：条件更新（必须仍 processing、租约属本实例且源快照仍匹配），写 done + 指纹/输出/版本。
/// 返回 true=本实例成功落库；false=租约或源快照已失（旧结果须丢弃）。
#[allow(clippy::too_many_arguments)]
pub fn finish_exotic_task(
    conn: &Connection,
    id: i64,
    instance_id: &str,
    expected_source_revision: i64,
    expected_cache_key: i64,
    fingerprint: &str,
    output_path: &str,
    worker_version: &str,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE exotic_tasks
         SET status=2, input_fingerprint=?5, output_path=?6, worker_version=?7,
             last_error_code=NULL, last_error_message=NULL, claimed_at=NULL, lease_owner=NULL,
             updated_at=strftime('%s','now')
         WHERE id=?1 AND status=1 AND lease_owner=?2
           AND EXISTS (
               SELECT 1 FROM media_items m
               WHERE m.id=exotic_tasks.item_id AND m.is_deleted=0
                 AND m.source_revision=?3 AND m.cache_key=?4
           )",
        params![
            id,
            instance_id,
            expected_source_revision,
            expected_cache_key,
            fingerprint,
            output_path,
            worker_version
        ],
    )?;
    Ok(n == 1)
}

/// 失败任务：retryable 且未超次数 → status=3 + attempts+1 + next_retry_at；否则 terminal(4)。
/// 条件更新（仍 processing 且租约属本实例）。返回是否本实例成功记录。
#[allow(clippy::too_many_arguments)]
pub fn fail_exotic_task(
    conn: &Connection,
    id: i64,
    instance_id: &str,
    retryable: bool,
    max_attempts: i64,
    code: &str,
    message: &str,
    next_retry_at: i64,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE exotic_tasks
         SET attempts = attempts + 1,
             status = CASE WHEN ?3=1 AND attempts+1 < ?4 THEN 3 ELSE 4 END,
             next_retry_at = CASE WHEN ?3=1 AND attempts+1 < ?4 THEN ?7 ELSE NULL END,
             last_error_code=?5, last_error_message=?6,
             claimed_at=NULL, lease_owner=NULL, updated_at=strftime('%s','now')
         WHERE id=?1 AND status=1 AND lease_owner=?2",
        params![
            id,
            instance_id,
            retryable as i64,
            max_attempts,
            code,
            message,
            next_retry_at
        ],
    )?;
    Ok(n == 1)
}

/// 列出已安装插件（安装真相投影）。Part1 安装表为空 → 返回空列表。
pub fn list_installed_exotic_plugins(
    conn: &Connection,
) -> Result<Vec<crate::exotic::InstalledExoticPlugin>> {
    let mut stmt = conn.prepare(
        "SELECT plugin_id, version, package_sequence, install_state, installed_at, updated_at
         FROM exotic_plugins ORDER BY plugin_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(crate::exotic::InstalledExoticPlugin {
            plugin_id: row.get(0)?,
            version: row.get(1)?,
            package_sequence: row.get(2)?,
            install_state: row.get(3)?,
            installed_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 读单个已安装插件的完整安装真相行（含 manifest_hash）。未安装 → None（Part3 §6）。
pub fn get_exotic_plugin(
    conn: &Connection,
    plugin_id: &str,
) -> Result<Option<crate::exotic::InstalledPluginRecord>> {
    let row = conn
        .query_row(
            "SELECT plugin_id, version, manifest_hash, package_sequence, install_state,
                    installed_at, updated_at, entitlement_source
             FROM exotic_plugins WHERE plugin_id=?1",
            params![plugin_id],
            |row| {
                Ok(crate::exotic::InstalledPluginRecord {
                    plugin_id: row.get(0)?,
                    version: row.get(1)?,
                    manifest_hash: row.get(2)?,
                    package_sequence: row.get(3)?,
                    install_state: row.get(4)?,
                    installed_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    entitlement_source: row.get(7)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// upsert 安装真相（安装/升级/修复成功后在同一 DB 事务调用，Part3 §6.4 第 10 步）。
/// 主键冲突时整行覆盖并刷新 updated_at；installed_at 首装时由调用方给定，升级保留原值由上层决定。
pub fn upsert_exotic_plugin(
    conn: &Connection,
    rec: &crate::exotic::InstalledPluginRecord,
) -> Result<()> {
    conn.execute(
        "INSERT INTO exotic_plugins
            (plugin_id, version, manifest_hash, package_sequence, install_state, installed_at,
             updated_at, entitlement_source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(plugin_id) DO UPDATE SET
            version=excluded.version,
            manifest_hash=excluded.manifest_hash,
            package_sequence=excluded.package_sequence,
            install_state=excluded.install_state,
            updated_at=excluded.updated_at,
            entitlement_source=excluded.entitlement_source",
        params![
            rec.plugin_id,
            rec.version,
            rec.manifest_hash,
            rec.package_sequence,
            rec.install_state,
            rec.installed_at,
            rec.updated_at,
            rec.entitlement_source,
        ],
    )?;
    Ok(())
}

/// 仅更新某插件的安装状态（installed/disabled/broken），刷新 updated_at。返回行数。
/// 完整性校验失败 → broken；禁用 → disabled。
pub fn set_exotic_plugin_state(conn: &Connection, plugin_id: &str, state: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_plugins SET install_state=?2, updated_at=strftime('%s','now')
         WHERE plugin_id=?1",
        params![plugin_id, state],
    )?)
}

/// 删除安装真相（卸载时原子移走目录后在同一事务调用，Part3 §6.5）。返回是否删除。
/// 不删除媒体记录与历史任务（卸载不丢用户数据，§6.5）。
pub fn delete_exotic_plugin(conn: &Connection, plugin_id: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM exotic_plugins WHERE plugin_id=?1",
        params![plugin_id],
    )?;
    Ok(n == 1)
}

/// 孤儿恢复：只回收**过期租约**的 processing 任务（claimed_at 为空或早于 now-lease_ttl）。
/// 不能在另一合法 App 实例仍工作时全量 1→0（R2）。返回回收行数。
pub fn recover_orphaned_exotic_tasks(
    conn: &Connection,
    lease_ttl_secs: i64,
    now: i64,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, claimed_at=NULL, lease_owner=NULL, updated_at=strftime('%s','now')
         WHERE status=1 AND (claimed_at IS NULL OR claimed_at < ?1)",
        params![now - lease_ttl_secs],
    )?)
}

/// 释放某实例仍持有的全部 processing 租约 → 退回 pending（Pipeline 结束/取消时清理）。
/// 已 finish/fail 的任务 lease_owner 已为 NULL，不受影响；只回收「领了但未最终化」的任务。返回行数。
pub fn release_exotic_instance_leases(conn: &Connection, instance_id: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, claimed_at=NULL, lease_owner=NULL, updated_at=strftime('%s','now')
         WHERE status=1 AND lease_owner=?1",
        params![instance_id],
    )?)
}

/// 统计某 (plugin,capability) 的任务计数：(pending_or_retry, processing, done, error)。
/// 进度/状态命令用。`pending_or_retry` = status 0 或 3（可领取/待重试）。
pub fn count_exotic_tasks_by_status(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
) -> Result<(i64, i64, i64, i64)> {
    conn.query_row(
        "SELECT
            SUM(CASE WHEN status IN (0,3) THEN 1 ELSE 0 END),
            SUM(CASE WHEN status=1 THEN 1 ELSE 0 END),
            SUM(CASE WHEN status=2 THEN 1 ELSE 0 END),
            SUM(CASE WHEN status=4 THEN 1 ELSE 0 END)
         FROM exotic_tasks WHERE plugin_id=?1 AND capability=?2",
        params![plugin_id, capability],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        },
    )
    .map_err(AppError::from)
}

/// 处理详情行(商店进度「展开详情」,2026-07-05):任务 × 媒体文件的展示投影。
#[derive(Debug)]
pub struct ExoticTaskDetailRow {
    pub item_id: i64,
    pub file_name: String,
    /// 所在目录相对扫描根的路径(展示用;根目录为空串)。
    pub dir_path: String,
    pub format: String,
    /// 原始状态码(0-4;命令层映射为字符串,3 细分为 retrying)。
    pub status: i64,
    pub attempts: i64,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
}

/// 列某 (plugin,capability) 的任务处理详情(JOIN media/directories 取展示字段)。
/// `bucket` 过滤桶与进度摘要四桶对齐:None=全部 / pending(0,3) / processing(1) / done(2) / error(4)。
/// 排序 = 活动优先:processing → 待重试(3) → pending(0) → error → done,同桶 updated_at DESC
/// (done 桶即「最近处理的在前」)。limit/offset 供「加载更多」分页,钳制在命令层。
/// 桶谓词来自固定白名单 match(非拼接外部输入),不违参数绑定纪律。
pub fn list_exotic_task_details(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
    bucket: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ExoticTaskDetailRow>> {
    let bucket_pred = match bucket {
        None => "",
        Some("pending") => " AND t.status IN (0,3)",
        Some("processing") => " AND t.status=1",
        Some("done") => " AND t.status=2",
        Some("error") => " AND t.status=4",
        // 命令层已白名单;此处防御性空结果而非全量(未知桶不得静默放大数据面)。
        Some(_) => return Ok(Vec::new()),
    };
    let sql = format!(
        "SELECT t.item_id, m.file_name, d.rel_path, m.file_format, t.status, t.attempts,
                t.last_error_code, t.last_error_message
         FROM exotic_tasks t
         JOIN media_items m ON m.id = t.item_id
         JOIN directories d ON d.id = m.directory_id
         WHERE t.plugin_id=?1 AND t.capability=?2{bucket_pred}
         ORDER BY CASE t.status WHEN 1 THEN 0 WHEN 3 THEN 1 WHEN 0 THEN 2 WHEN 4 THEN 3 ELSE 4 END,
                  t.updated_at DESC, t.id DESC
         LIMIT ?3 OFFSET ?4"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![plugin_id, capability, limit, offset], |r| {
            Ok(ExoticTaskDetailRow {
                item_id: r.get(0)?,
                file_name: r.get(1)?,
                dir_path: r.get(2)?,
                format: r.get(3)?,
                status: r.get(4)?,
                attempts: r.get(5)?,
                last_error_code: r.get(6)?,
                last_error_message: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 单任务重置为 pending（用户「重试此项」命令）：清输出/指纹/错误/租约/退避。返回行数。
pub fn reset_exotic_task_for_retry(
    conn: &Connection,
    item_id: i64,
    capability: &str,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, attempts=0, next_retry_at=NULL, claimed_at=NULL, lease_owner=NULL,
             last_error_code=NULL, last_error_message=NULL, updated_at=strftime('%s','now')
         WHERE item_id=?1 AND capability=?2 AND status IN (3,4)",
        params![item_id, capability],
    )?)
}

/// 某插件全部 error 任务（status 3/4）重置为 pending（用户「重试插件失败」命令）。返回行数。
pub fn reset_exotic_plugin_failures(conn: &Connection, plugin_id: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE exotic_tasks
         SET status=0, attempts=0, next_retry_at=NULL, claimed_at=NULL, lease_owner=NULL,
             last_error_code=NULL, last_error_message=NULL, updated_at=strftime('%s','now')
         WHERE plugin_id=?1 AND status IN (3,4)",
        params![plugin_id],
    )?)
}

/// 是否存在「现在就绪」的任务（status=0，或 status=3 且 next_retry_at 已到）。
/// Coordinator 据此决定是否启动 Pipeline，避免空跑。
pub fn has_ready_exotic_task(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
    now: i64,
) -> Result<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM exotic_tasks
            WHERE plugin_id=?1 AND capability=?2
              AND ( status=0 OR (status=3 AND (next_retry_at IS NULL OR next_retry_at<=?3)) ))",
        params![plugin_id, capability, now],
        |r| r.get(0),
    )?;
    Ok(exists != 0)
}

/// 取 exotic 任务处理所需的源信息：绝对路径 + source snapshot + 小写扩展名。
/// 经 directories JOIN scan_roots 解析绝对路径（与 `get_media_detail` 同路径解析）。
pub fn exotic_item_source(conn: &Connection, item_id: i64) -> Result<ExoticItemSource> {
    let (file_name, file_format, source_revision, cache_key, rel_path, root_path): (
        String,
        String,
        i64,
        i64,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT m.file_name, m.file_format, m.source_revision, m.cache_key,
                    d.rel_path, r.path
             FROM media_items m
             JOIN directories d ON m.directory_id = d.id
             JOIN scan_roots r ON d.root_id = r.id
             WHERE m.id=?1 AND m.is_deleted=0",
            params![item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|_| AppError::MediaNotFound(item_id))?;
    let abs_path = crate::utils::path::resolve_media_path(&root_path, &rel_path, &file_name);
    Ok(ExoticItemSource {
        abs_path,
        source_revision,
        cache_key,
        file_format,
    })
}

/// [`exotic_item_source`] 返回值。
#[derive(Debug, Clone)]
pub struct ExoticItemSource {
    pub abs_path: String,
    pub source_revision: i64,
    pub cache_key: i64,
    pub file_format: String,
}

#[cfg(test)]
mod exotic_dao_tests {
    use super::super::ai::count_pending_ai_items;
    use super::super::faces::count_pending_face_items;
    use super::super::thumbnail::{
        count_pending_thumb_items, exotic_thumbnail_route_info_for_items,
        exotic_thumbnail_task_status_for_items, get_all_pending_thumb_ids,
    };
    use super::*;

    const PID: &str = "exotic-image-psd";
    const CAP: &str = "thumbnail";

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        // 关 FK 以便用最小 media_items 夹具覆盖任务 DAO，而不构造完整目录树。
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c
    }

    fn seed(c: &Connection, item_id: i64) {
        c.execute(
            "INSERT OR IGNORE INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, 1, ?2, 1, 1, 'psd', 'image', 0, 0, 0, ?1)",
            params![item_id, format!("f{item_id}.psd")],
        )
        .unwrap();
        seed_exotic_tasks_for_item(c, item_id, PID, &[CAP.to_string()]).unwrap();
    }

    fn claim(c: &Connection, limit: i64, owner: &str, now: i64) -> Vec<ExoticTaskRow> {
        claim_exotic_tasks(c, PID, CAP, limit, owner, now).unwrap()
    }

    fn source_snapshot(c: &Connection, item_id: i64) -> (i64, i64) {
        c.query_row(
            "SELECT source_revision, cache_key FROM media_items WHERE id=?1",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    fn finish_current(
        c: &Connection,
        task_id: i64,
        instance_id: &str,
        fingerprint: &str,
        output_path: &str,
        worker_version: &str,
    ) -> bool {
        let item_id: i64 = c
            .query_row(
                "SELECT item_id FROM exotic_tasks WHERE id=?1",
                params![task_id],
                |row| row.get(0),
            )
            .unwrap();
        let (source_revision, cache_key) = source_snapshot(c, item_id);
        finish_exotic_task(
            c,
            task_id,
            instance_id,
            source_revision,
            cache_key,
            fingerprint,
            output_path,
            worker_version,
        )
        .unwrap()
    }

    #[test]
    fn atomic_claim_no_double() {
        let c = mem_db();
        for i in 1..=3 {
            seed(&c, i);
        }
        assert_eq!(claim(&c, 2, "inst-A", 1000).len(), 2);
        assert_eq!(claim(&c, 2, "inst-A", 1000).len(), 1); // 剩 1
        assert_eq!(claim(&c, 2, "inst-A", 1000).len(), 0); // 全 processing
        assert!(claim(&c, 2, "inst-A", 1000).is_empty());
    }

    /// 隐藏根排除(V21):claim 跳过隐藏根下媒体的任务(留 pending,不烧算力);
    /// unhide 后原任务原地可再领(非破坏暂停,与四条主流水线同口径)。
    #[test]
    fn claim_skips_hidden_root_and_unhide_reclaims() {
        let c = mem_db();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, '', 'r1'), (20, 2, '', 'r2');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a.psd', 1, 1, 'psd', 'image', 0, 0, 100, 1),
                 (2, 20, 'b.psd', 1, 1, 'psd', 'image', 0, 0, 200, 2);",
        )
        .unwrap();
        seed(&c, 1);
        seed(&c, 2);
        super::super::scan::set_scan_root_hidden(&c, 2, true).unwrap();
        let got: Vec<i64> = claim(&c, 10, "A", 1000).iter().map(|t| t.item_id).collect();
        assert_eq!(got, vec![1], "隐藏根任务不领取");
        super::super::scan::set_scan_root_hidden(&c, 2, false).unwrap();
        let got: Vec<i64> = claim(&c, 10, "A", 1000).iter().map(|t| t.item_id).collect();
        assert_eq!(got, vec![2], "unhide 后 pending 任务原地可领");
    }

    /// 详情列表(展开详情):活动优先排序 + 桶筛选 + 分页 + JOIN 展示字段。
    #[test]
    fn task_details_order_filter_pagination() {
        let c = mem_db();
        // 最小媒体链(FK 已关,只为 JOIN 供数):一目录 + 五文件。
        c.execute_batch(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, 'art/psd', 'psd');",
        )
        .unwrap();
        for i in 1..=5 {
            c.execute(
                "INSERT INTO media_items
                    (id, directory_id, file_name, file_size, file_mtime, file_format,
                     media_type, width, height, sort_datetime, cache_key)
                 VALUES (?1, 10, ?2, 1, 1, 'psd', 'image', 0, 0, 0, ?1)",
                params![i, format!("f{i}.psd")],
            )
            .unwrap();
            seed(&c, i);
        }
        // 造五态:1=done, 2=processing, 3=terminal error, 4=retryable, 5=pending。
        let id1 = claim(&c, 1, "A", 1000)[0].id;
        assert!(finish_current(&c, id1, "A", "fp", "/p.webp", "1.0.0"));
        let _id2 = claim(&c, 1, "A", 1000)[0].id; // item 2 → processing
        let id3 = claim(&c, 1, "A", 1000)[0].id; // item 3 → terminal
        fail_exotic_task(&c, id3, "A", false, 1, "decode_failed", "bad psd", 0).unwrap();
        let id4 = claim(&c, 1, "A", 1000)[0].id; // item 4 → retryable
        fail_exotic_task(&c, id4, "A", true, 3, "worker_crash", "boom", 9999).unwrap();
        // item 5 保持 pending。

        // 全部:processing → retryable → pending → error → done。
        let all = list_exotic_task_details(&c, PID, CAP, None, 50, 0).unwrap();
        let order: Vec<(i64, i64)> = all.iter().map(|r| (r.item_id, r.status)).collect();
        assert_eq!(
            order,
            vec![(2, 1), (4, 3), (5, 0), (3, 4), (1, 2)],
            "活动优先序"
        );
        // JOIN 展示字段。
        let done = &all[4];
        assert_eq!(done.file_name, "f1.psd");
        assert_eq!(done.dir_path, "art/psd");
        assert_eq!(done.format, "psd");
        // 错误字段透出。
        let err = &all[3];
        assert_eq!(err.last_error_code.as_deref(), Some("decode_failed"));
        assert_eq!(err.attempts, 1);

        // 桶筛选:pending 桶含 0 与 3;error 桶仅 terminal。
        let pend = list_exotic_task_details(&c, PID, CAP, Some("pending"), 50, 0).unwrap();
        assert_eq!(
            pend.iter().map(|r| r.item_id).collect::<Vec<_>>(),
            vec![4, 5]
        );
        let errs = list_exotic_task_details(&c, PID, CAP, Some("error"), 50, 0).unwrap();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].item_id, 3);

        // 分页:limit/offset 拼接 == 全量。
        let p1 = list_exotic_task_details(&c, PID, CAP, None, 2, 0).unwrap();
        let p2 = list_exotic_task_details(&c, PID, CAP, None, 2, 2).unwrap();
        let p3 = list_exotic_task_details(&c, PID, CAP, None, 2, 4).unwrap();
        let paged: Vec<i64> = p1.iter().chain(&p2).chain(&p3).map(|r| r.item_id).collect();
        assert_eq!(paged, vec![2, 4, 5, 3, 1], "分页拼接等于全量序");

        // 未知桶防御性空结果。
        assert!(list_exotic_task_details(&c, PID, CAP, Some("bogus"), 50, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn lease_guards_finish() {
        let c = mem_db();
        seed(&c, 1);
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        // 错误 owner 不能完成（旧 Writer 失租）。
        assert!(!finish_exotic_task(&c, id, "inst-B", 1, 1, "fp", "/p.webp", "1.0.0").unwrap());
        assert!(has_blocking_exotic_thumbnail_task(&c, 1).unwrap());
        // 正确 owner 完成。
        assert!(finish_current(&c, id, "inst-A", "fp", "/p.webp", "1.0.0"));
        assert!(!has_blocking_exotic_thumbnail_task(&c, 1).unwrap()); // done 不再阻塞
    }

    #[test]
    fn stale_source_snapshot_cannot_finish_exotic_task() {
        let c = mem_db();
        seed(&c, 1);
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        let (source_revision, cache_key) = source_snapshot(&c, 1);

        // 模拟扫描在 Worker 处理期间发现同大小/同抽样指纹的源代次变化；cache_key
        // 也变化时必须同时满足两项快照条件，旧结果不能把 processing 标成 done。
        c.execute(
            "UPDATE media_items
             SET source_revision=?2, cache_key=?3, thumb_status=0, thumb_path=NULL
             WHERE id=?1",
            params![1, source_revision + 1, cache_key + 1],
        )
        .unwrap();
        assert!(!is_exotic_source_current(&c, 1, source_revision, cache_key).unwrap());

        assert!(!finish_exotic_task(
            &c,
            id,
            "inst-A",
            source_revision,
            cache_key,
            "old-fingerprint",
            "480/aa/old.webp",
            "1.0.0"
        )
        .unwrap());
        let task_status: i64 = c
            .query_row(
                "SELECT status FROM exotic_tasks WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(task_status, 1, "旧结果不得结束新源的 processing 任务");
        let (thumb_status, thumb_path): (i64, Option<String>) = c
            .query_row(
                "SELECT thumb_status, thumb_path FROM media_items WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(thumb_status, 0);
        assert!(thumb_path.is_none());
    }

    #[test]
    fn install_truth_upsert_get_delete() {
        let c = mem_db();
        // 未安装 → None。
        assert!(get_exotic_plugin(&c, PID).unwrap().is_none());

        // upsert 首装。
        let rec = crate::exotic::InstalledPluginRecord {
            plugin_id: PID.into(),
            version: "1.0.0".into(),
            manifest_hash: "h1".into(),
            package_sequence: 3,
            install_state: crate::exotic::install_state::INSTALLED.into(),
            installed_at: 100,
            updated_at: 100,
            entitlement_source: "direct".into(),
        };
        upsert_exotic_plugin(&c, &rec).unwrap();
        let got = get_exotic_plugin(&c, PID).unwrap().unwrap();
        assert_eq!(got, rec);

        // upsert 同主键升级（version/sequence/hash 覆盖）。
        let upgraded = crate::exotic::InstalledPluginRecord {
            version: "1.1.0".into(),
            manifest_hash: "h2".into(),
            package_sequence: 4,
            updated_at: 200,
            ..rec.clone()
        };
        upsert_exotic_plugin(&c, &upgraded).unwrap();
        let got = get_exotic_plugin(&c, PID).unwrap().unwrap();
        assert_eq!(got.version, "1.1.0");
        assert_eq!(got.package_sequence, 4);

        // 仅改状态 → broken。
        assert_eq!(
            set_exotic_plugin_state(&c, PID, crate::exotic::install_state::BROKEN).unwrap(),
            1
        );
        assert_eq!(
            get_exotic_plugin(&c, PID).unwrap().unwrap().install_state,
            "broken"
        );

        // 删除。
        assert!(delete_exotic_plugin(&c, PID).unwrap());
        assert!(get_exotic_plugin(&c, PID).unwrap().is_none());
        assert!(!delete_exotic_plugin(&c, PID).unwrap()); // 再删 → false
    }

    #[test]
    fn recover_only_expired_lease() {
        let c = mem_db();
        seed(&c, 1);
        let _ = claim(&c, 1, "inst-A", 1000);
        // ttl=100：now=1010 未过期 → 不回收。
        assert_eq!(recover_orphaned_exotic_tasks(&c, 100, 1010).unwrap(), 0);
        // now=2000：claimed_at=1000 < 1900 → 回收。
        assert_eq!(recover_orphaned_exotic_tasks(&c, 100, 2000).unwrap(), 1);
        // 回收后可被另一实例重新领取。
        assert_eq!(claim(&c, 1, "inst-B", 2000).len(), 1);
    }

    #[test]
    fn invalidate_resets_done_task() {
        let c = mem_db();
        seed(&c, 1);
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        assert!(finish_current(&c, id, "inst-A", "fp", "/p.webp", "1.0.0"));
        assert!(!has_blocking_exotic_thumbnail_task(&c, 1).unwrap());
        assert_eq!(invalidate_exotic_tasks_for_item(&c, 1).unwrap(), 1);
        // 退回 pending → 重新阻塞 + 可领取，且输出已清。
        assert!(has_blocking_exotic_thumbnail_task(&c, 1).unwrap());
        let again = claim(&c, 1, "inst-A", 2000);
        assert_eq!(again.len(), 1);
        assert!(again[0].output_path.is_none());
        assert!(again[0].input_fingerprint.is_none());
    }

    #[test]
    fn reset_all_redoes_done_retry_terminal_keeps_processing() {
        let c = mem_db();
        for i in 1..=4 {
            seed(&c, i);
        }
        // item1 → done(2)
        let id1 = claim(&c, 1, "A", 1000)[0].id;
        assert!(finish_current(&c, id1, "A", "fp", "/p.webp", "1.0.0"));
        // item2 → retry(3)
        let id2 = claim(&c, 1, "A", 1000)[0].id;
        fail_exotic_task(&c, id2, "A", true, 3, "io_error", "busy", 1500).unwrap();
        // item3 → terminal(4)
        let id3 = claim(&c, 1, "A", 1000)[0].id;
        fail_exotic_task(&c, id3, "A", true, 1, "malformed_input", "bad", 0).unwrap();
        // item4 → processing(1)，只领不最终化（模拟在途）
        let _id4 = claim(&c, 1, "A", 1000)[0].id;

        // 重置只动 done/retry/terminal（3 条）；processing 不动（在途结果仍有效）。
        assert_eq!(reset_all_exotic_thumbnail_tasks(&c).unwrap(), 3);

        // item1/2/3 退回 pending → 可领、输出/指纹已清；item4 仍 processing 领不到。
        let again = claim(&c, 9, "B", 9999);
        assert_eq!(again.len(), 3);
        assert!(again
            .iter()
            .all(|r| r.output_path.is_none() && r.input_fingerprint.is_none()));
    }

    #[test]
    fn renew_all_refreshes_inflight_leases() {
        let c = mem_db();
        seed(&c, 1);
        seed(&c, 2);
        let _ = claim(&c, 2, "A", 1000); // 两条 claimed_at=1000
                                         // 续租把本实例在途刷新到 5000。
        assert_eq!(renew_all_exotic_leases(&c, "A", 5000).unwrap(), 2);
        // ttl=100、now=1200：旧 claimed_at(1000<1100) 本会被回收；续租后 claimed_at=5000 不回收。
        assert_eq!(recover_orphaned_exotic_tasks(&c, 100, 1200).unwrap(), 0);
        // 别的实例续租不到本实例任务（lease_owner 不符）。
        assert_eq!(renew_all_exotic_leases(&c, "B", 9000).unwrap(), 0);
    }

    #[test]
    fn retryable_respects_next_retry_at() {
        let c = mem_db();
        seed(&c, 1);
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        // 可重试，下次重试时刻 1500，最多 3 次。
        assert!(fail_exotic_task(&c, id, "inst-A", true, 3, "io_error", "busy", 1500).unwrap());
        assert_eq!(claim(&c, 1, "inst-A", 1000).len(), 0); // 未到期
        let due = claim(&c, 1, "inst-A", 1600);
        assert_eq!(due.len(), 1); // 到期可再领
        assert_eq!(due[0].attempts, 1);
    }

    #[test]
    fn terminal_when_attempts_exhausted() {
        let c = mem_db();
        seed(&c, 1);
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        // max_attempts=1：attempts+1=1 不 < 1 → 直接 terminal(4)，不再可领。
        assert!(
            fail_exotic_task(&c, id, "inst-A", true, 1, "malformed_input", "bad", 1500).unwrap()
        );
        assert_eq!(claim(&c, 1, "inst-A", 9999).len(), 0);
    }

    /// 插一条最小 media_items（FK 已关），返回 id。
    fn insert_media(c: &Connection, fmt: &str) -> i64 {
        c.execute(
            "INSERT INTO media_items
                (directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (1, ?1, 1, 1, ?2, 'image', 0, 0, 0, ?3)",
            params![format!("f.{fmt}"), fmt, rand_key()],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn rand_key() -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64
    }

    #[test]
    fn full_gen_pending_excludes_blocked_exotic() {
        let c = mem_db();
        let jpg = insert_media(&c, "jpg"); // 常见格式，无 exotic 任务
        let psd = insert_media(&c, "psd");
        seed(&c, psd); // psd 有 pending thumbnail 任务 → 应被主 generator 的 pending 查询排除

        let pending = get_all_pending_thumb_ids(&c).unwrap();
        assert!(pending.contains(&jpg), "jpg 应进主缩略图 pending");
        assert!(
            !pending.contains(&psd),
            "未完成 exotic 的 psd 不得进主 generator"
        );
        assert_eq!(count_pending_thumb_items(&c).unwrap(), 1);

        // 批量任务状态查询命中 psd=pending，jpg 无任务。
        let map = exotic_thumbnail_task_status_for_items(&c, &[jpg, psd]).unwrap();
        assert_eq!(map.get(&psd), Some(&ExoticTaskStatus::Pending));
        assert!(!map.contains_key(&jpg));

        // 任务完成后 psd 不再被 exotic 谓词阻塞（此时通常 Sink 已置 thumb_status=1）。
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        assert!(finish_current(&c, id, "inst-A", "fp", "/p.webp", "1.0.0"));
        assert!(get_all_pending_thumb_ids(&c).unwrap().contains(&psd));
    }

    #[test]
    fn route_info_returns_fingerprint_and_worker_version_for_done() {
        let c = mem_db();
        let psd = insert_media(&c, "psd");
        seed(&c, psd);
        // 未完成：route info 含 pending、无指纹/版本。
        let m = exotic_thumbnail_route_info_for_items(&c, &[psd]).unwrap();
        let info = m.get(&psd).unwrap();
        assert_eq!(info.status, ExoticTaskStatus::Pending);
        assert!(info.input_fingerprint.is_none() && info.worker_version.is_none());

        // 完成后：route info 带回存储的指纹与 worker 版本（供入口重算比对，问题4）。
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        assert!(finish_current(
            &c, id, "inst-A", "fp-abc", "/p.webp", "1.0.0"
        ));
        let m = exotic_thumbnail_route_info_for_items(&c, &[psd]).unwrap();
        let info = m.get(&psd).unwrap();
        assert_eq!(info.status, ExoticTaskStatus::Done);
        assert_eq!(info.input_fingerprint.as_deref(), Some("fp-abc"));
        assert_eq!(info.worker_version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn ai_and_face_counts_exclude_blocked_exotic() {
        let c = mem_db();
        let _jpg = insert_media(&c, "jpg");
        let psd = insert_media(&c, "psd");
        seed(&c, psd); // psd 有未完成 thumbnail 任务

        // 两张图均 ai_status=0/face_status=0，但 psd 被 exotic 门控排除 → 计数为 1。
        assert_eq!(count_pending_ai_items(&c).unwrap(), 1);
        assert_eq!(count_pending_face_items(&c).unwrap(), 1);

        // 任务完成后门控解除，psd 计入（此后 AI/face 优先用其 thumb_path，§2.4）。
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        assert!(finish_current(&c, id, "inst-A", "fp", "/p.webp", "1.0.0"));
        assert_eq!(count_pending_ai_items(&c).unwrap(), 2);
        assert_eq!(count_pending_face_items(&c).unwrap(), 2);
    }

    #[test]
    fn upgrade_invalidates_done_with_old_version() {
        let c = mem_db();
        seed(&c, 1);
        let id = claim(&c, 1, "inst-A", 1000)[0].id;
        assert!(finish_current(&c, id, "inst-A", "fp", "/p.webp", "1.0.0"));
        // 升级到 1.1.0：旧版本 done 任务退回 pending。
        assert_eq!(
            invalidate_exotic_tasks_for_plugin_version(&c, PID, "1.1.0").unwrap(),
            1
        );
        assert!(has_blocking_exotic_thumbnail_task(&c, 1).unwrap());
        // 相同版本不重复失效。
        let id = claim(&c, 1, "inst-A", 2000)[0].id;
        assert!(finish_current(&c, id, "inst-A", "fp2", "/p.webp", "1.1.0"));
        assert_eq!(
            invalidate_exotic_tasks_for_plugin_version(&c, PID, "1.1.0").unwrap(),
            0
        );
    }
}
