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

/// Worker 版本变化时重置缩略图及失败预算；旧库失败记录先登记版本基线。
pub fn invalidate_exotic_tasks_for_plugin_version(
    conn: &Connection,
    plugin_id: &str,
    new_worker_version: &str,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(
        "UPDATE exotic_tasks
         SET status=0, input_fingerprint=NULL, claimed_at=NULL, lease_owner=NULL,
             attempts=0, next_retry_at=NULL, last_error_code=NULL, last_error_message=NULL,
             output_path=NULL, worker_version=?2,
             updated_at=strftime('%s','now')
         WHERE plugin_id=?1 AND capability='thumbnail' AND status IN (2,3,4)
           AND (worker_version<>?2 OR (status=2 AND worker_version IS NULL))",
        params![plugin_id, new_worker_version],
    )?;
    // 缺执行版本不能证明发生升级；只建一次基线，避免每次 Startup 复活坏文件。
    tx.execute(
        "UPDATE exotic_tasks SET worker_version=?2
         WHERE plugin_id=?1 AND capability='thumbnail' AND status IN (3,4) AND worker_version IS NULL",
        params![plugin_id, new_worker_version],
    )?;
    tx.commit()?;
    Ok(changed)
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
#[allow(clippy::too_many_arguments)]
pub fn claim_exotic_tasks(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
    limit: i64,
    instance_id: &str,
    now: i64,
    worker_version: &str,
) -> Result<Vec<ExoticTaskRow>> {
    claim_exotic_tasks_in_volume(
        conn,
        plugin_id,
        capability,
        limit,
        instance_id,
        now,
        worker_version,
        None,
    )
}

/// 按已取得的源卷额度原子领取；Some(None) 限定尚无卷身份的源。
#[allow(clippy::too_many_arguments)]
pub fn claim_exotic_tasks_in_volume(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
    limit: i64,
    instance_id: &str,
    now: i64,
    worker_version: &str,
    volume: Option<Option<i64>>,
) -> Result<Vec<ExoticTaskRow>> {
    let sql = format!(
        "UPDATE exotic_tasks
         SET status=1, claimed_at=?3, lease_owner=?4, worker_version=?6, updated_at=strftime('%s','now')
         WHERE id IN (
             SELECT id FROM exotic_tasks
             WHERE plugin_id=?1 AND capability=?2
               AND ( status=0 OR (status=3 AND (next_retry_at IS NULL OR next_retry_at<=?3)) )
               {EXCLUDE_HIDDEN_ROOT_ITEMS}
               AND (?7=0 OR (SELECT r.volume_id FROM media_items m
                    JOIN directories d ON d.id=m.directory_id
                    JOIN scan_roots r ON r.id=d.root_id
                    WHERE m.id=exotic_tasks.item_id) IS ?8)
             ORDER BY id LIMIT ?5 )
         RETURNING {EXOTIC_TASK_COLS}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![
                plugin_id,
                capability,
                now,
                instance_id,
                limit,
                worker_version,
                volume.is_some(),
                volume.flatten()
            ],
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

/// 条件失败更新的实际结果，供调度统计使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExoticFailureOutcome {
    RetryScheduled,
    Terminal,
    LeaseLost,
}

/// 失败任务：返回实际重试/终态/租约丢失，第三次失败不再误计为重试。
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
) -> Result<ExoticFailureOutcome> {
    let retry = conn
        .query_row(
            "UPDATE exotic_tasks
         SET attempts = attempts + 1,
             status = CASE WHEN ?3=1 AND attempts+1 < ?4 THEN 3 ELSE 4 END,
             next_retry_at = CASE WHEN ?3=1 AND attempts+1 < ?4 THEN ?7 ELSE NULL END,
             last_error_code=?5, last_error_message=?6,
             claimed_at=NULL, lease_owner=NULL, updated_at=strftime('%s','now')
         WHERE id=?1 AND status=1 AND lease_owner=?2 RETURNING status",
            params![
                id,
                instance_id,
                retryable as i64,
                max_attempts,
                code,
                message,
                next_retry_at
            ],
            |row| Ok(row.get::<_, i64>(0)? == 3),
        )
        .optional()?;
    Ok(match retry {
        Some(true) => ExoticFailureOutcome::RetryScheduled,
        Some(false) => ExoticFailureOutcome::Terminal,
        None => ExoticFailureOutcome::LeaseLost,
    })
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
                    installed_at, updated_at
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
             updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(plugin_id) DO UPDATE SET
            version=excluded.version,
            manifest_hash=excluded.manifest_hash,
            package_sequence=excluded.package_sequence,
            install_state=excluded.install_state,
            updated_at=excluded.updated_at",
        params![
            rec.plugin_id,
            rec.version,
            rec.manifest_hash,
            rec.package_sequence,
            rec.install_state,
            rec.installed_at,
            rec.updated_at,
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
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM exotic_tasks
            WHERE plugin_id=?1 AND capability=?2
              AND ( status=0 OR (status=3 AND (next_retry_at IS NULL OR next_retry_at<=?3)) )
              {EXCLUDE_HIDDEN_ROOT_ITEMS})"
    );
    let exists: i64 = conn.query_row(&sql, params![plugin_id, capability, now], |r| r.get(0))?;
    Ok(exists != 0)
}

/// 下一项就绪任务的源卷；外层 None 表示无任务，内层 None 表示卷身份未知。
pub fn next_ready_exotic_volume(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
    now: i64,
) -> Result<Option<Option<i64>>> {
    let sql = format!(
        "SELECT (SELECT r.volume_id FROM media_items m
                 JOIN directories d ON d.id=m.directory_id
                 JOIN scan_roots r ON r.id=d.root_id
                 WHERE m.id=exotic_tasks.item_id)
         FROM exotic_tasks WHERE plugin_id=?1 AND capability=?2
           AND (status=0 OR (status=3 AND (next_retry_at IS NULL OR next_retry_at<=?3)))
           {EXCLUDE_HIDDEN_ROOT_ITEMS}
         ORDER BY id LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params![plugin_id, capability, now], |row| row.get(0))
        .optional()?)
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
