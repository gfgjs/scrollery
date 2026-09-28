//! 统一缩略图任务的持久身份与 lease。普通图片复用 media_derivations 的执行状态，
//! media_items 仍只记录当前已提交的展示产物。

use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

use super::exotic::NOT_BLOCKED_BY_EXOTIC_M;
use super::media::{map_media_item, MEDIA_ITEM_COLUMNS};
use super::scan::EXCLUDE_HIDDEN_ROOTS_M;
use crate::db::models::{MediaItem, ThumbResult};
use crate::error::{AppError, Result};
use crate::thumbnail::scheduler::OutputFingerprint;
use crate::utils::path::resolve_media_path;

/// 一次查询取得缩略图源元数据及路径，供分类与后续解码复用。
#[derive(Clone)]
pub struct ThumbnailCandidate {
    pub item: MediaItem,
    pub abs_path: PathBuf,
    pub root_path: String,
    pub relative_dir: String,
    /// 源扫描根的稳定卷行；未探测时按未知卷保守限流。
    pub volume_id: Option<i64>,
    /// 已入库的视频 codec 仅用于设备偏好；实际媒体类型仍以本次 SourceReader 为准。
    pub video_codec: Option<String>,
}

/// 查询当前租约的轮次归属，供同一写事务成功提交后分发完成事实。
pub fn thumbnail_lease_run_id(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT run_id FROM media_derivations WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3 AND source_revision=?4 AND status=1 AND lease_id=?5",
        params![key.item_id, key.kind.as_str(), key.output_fingerprint, key.source_revision, lease_id],
        |row| row.get::<_, Option<String>>(0),
    ).optional().map(Option::flatten).map_err(AppError::from)
}

/// 在一次数据库快照中固定本轮普通图片成员。扫描期间后来出现的媒体不会混入本轮尾批。
/// 已在途且源未变化的 lease 保留，其他待重建项重新入队；调用方须使用唯一 run_id。
pub fn enroll_image_thumbnail_run(
    conn: &Connection,
    run_id: &str,
    output_fingerprint: OutputFingerprint,
) -> Result<usize> {
    let sql = format!(
        "INSERT INTO media_derivations
             (item_id, kind, output_fingerprint, source_revision, lane, cost_class, run_id)
         SELECT m.id, 'image_thumb', ?1, m.source_revision, 'unclassified', 'unknown', ?2
         FROM media_items m
         WHERE m.thumb_status=0 AND m.is_deleted=0 AND m.media_type='image'
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         ON CONFLICT(item_id, kind, output_fingerprint) DO UPDATE SET
             source_revision=excluded.source_revision,
             status=CASE WHEN media_derivations.source_revision=excluded.source_revision
                              AND media_derivations.status=1 THEN 1 ELSE 0 END,
             payload_path=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                    AND media_derivations.status=1
                               THEN media_derivations.payload_path ELSE NULL END,
             lease_id=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                AND media_derivations.status=1
                           THEN media_derivations.lease_id ELSE NULL END,
             lease_until=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                   AND media_derivations.status=1
                              THEN media_derivations.lease_until ELSE NULL END,
             lane=CASE WHEN media_derivations.source_revision=excluded.source_revision
                            AND media_derivations.status=1
                       THEN media_derivations.lane ELSE 'unclassified' END,
             cost_class=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                  AND media_derivations.status=1
                             THEN media_derivations.cost_class ELSE 'unknown' END,
             run_id=excluded.run_id,
             failure_reason=NULL, updated_at=strftime('%s','now')"
    );
    conn.execute(&sql, params![output_fingerprint.hex(), run_id])
        .map_err(AppError::from)
}

/// 固定本轮系统原生视频封面成员；冷门容器保留旧派生路径。
pub fn enroll_native_video_cover_run(
    conn: &Connection,
    run_id: &str,
    output_fingerprint: OutputFingerprint,
) -> Result<usize> {
    let formats = crate::video::native_cover_formats();
    if formats.is_empty() {
        return Ok(0);
    }
    let placeholders = (3..3 + formats.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "INSERT INTO media_derivations
             (item_id, kind, output_fingerprint, source_revision, lane, cost_class, run_id)
         SELECT m.id, 'video_cover', ?1, m.source_revision, 'heavy', 'video', ?2
         FROM media_items m
         WHERE (m.thumb_status=0 OR EXISTS (
             SELECT 1 FROM media_derivations pending
             WHERE pending.item_id=m.id AND pending.kind='video_cover'
               AND pending.output_fingerprint=?1 AND pending.status=0))
           AND m.is_deleted=0 AND m.media_type='video'
           AND m.file_format IN ({placeholders})
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         ON CONFLICT(item_id, kind, output_fingerprint) DO UPDATE SET
             source_revision=excluded.source_revision,
             status=CASE WHEN media_derivations.source_revision=excluded.source_revision
                              AND media_derivations.status=1 THEN 1 ELSE 0 END,
             payload_path=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                    AND media_derivations.status=1
                               THEN media_derivations.payload_path ELSE NULL END,
             lease_id=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                AND media_derivations.status=1
                           THEN media_derivations.lease_id ELSE NULL END,
             lease_until=CASE WHEN media_derivations.source_revision=excluded.source_revision
                                   AND media_derivations.status=1
                              THEN media_derivations.lease_until ELSE NULL END,
             lane=CASE WHEN media_derivations.source_revision=excluded.source_revision
                            AND media_derivations.status=1
                       THEN media_derivations.lane ELSE 'heavy' END,
             cost_class='video', run_id=excluded.run_id,
             failure_reason=NULL, updated_at=strftime('%s','now')"
    );
    let fp = output_fingerprint.hex();
    let mut bound: Vec<&dyn rusqlite::ToSql> = vec![&fp, &run_id];
    for format in formats {
        bound.push(format as &dyn rusqlite::ToSql);
    }
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(&sql, bound.as_slice())?;
    tx.execute(
        "DELETE FROM media_derivations
         WHERE kind='video_cover' AND output_fingerprint=''
           AND EXISTS (SELECT 1 FROM media_derivations next
                       WHERE next.item_id=media_derivations.item_id
                         AND next.kind='video_cover' AND next.output_fingerprint=?1
                         AND next.run_id=?2)",
        params![fp, run_id],
    )?;
    tx.commit()?;
    Ok(changed)
}

/// 只枚举本轮快照的成员；源换代或媒体被移除后不再向解码器派发旧路径。
pub fn image_thumbnail_run_page(
    conn: &Connection,
    run_id: &str,
    after_id: i64,
    limit: i64,
) -> Result<Vec<ThumbnailCandidate>> {
    let sql = format!(
        "SELECT m.*, r.path, d.rel_path, r.volume_id
         FROM media_derivations t
         JOIN (SELECT {MEDIA_ITEM_COLUMNS} FROM media_items) m
           ON m.id=t.item_id AND m.source_revision=t.source_revision
         JOIN directories d ON m.directory_id=d.id
         JOIN scan_roots r ON d.root_id=r.id
         WHERE t.run_id=?1 AND t.kind='image_thumb' AND t.lane='unclassified'
           AND t.item_id>?2 AND m.is_deleted=0 AND t.status IN (0,1)
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         ORDER BY t.item_id LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![run_id, after_id, limit], |row| {
        let item = map_media_item(row)?;
        let root_path: String = row.get(29)?;
        let rel_path: String = row.get(30)?;
        let abs_path = PathBuf::from(resolve_media_path(&root_path, &rel_path, &item.file_name));
        Ok(ThumbnailCandidate {
            item,
            abs_path,
            root_path,
            relative_dir: rel_path,
            volume_id: row.get(31)?,
            video_codec: None,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 按当前阶段分页读取仍需执行的本轮项。过期 lease 可被重新领取，当前在途项留待完成。
pub fn image_thumbnail_lane_page(
    conn: &Connection,
    run_id: &str,
    lane: ThumbnailLane,
    after_id: i64,
    limit: i64,
    now: i64,
) -> Result<Vec<ThumbnailCandidate>> {
    thumbnail_lane_page(conn, "image_thumb", run_id, lane, after_id, limit, now)
}

/// 读取本轮重型或异常阶段的原生视频封面任务。
pub fn native_video_cover_lane_page(
    conn: &Connection,
    run_id: &str,
    lane: ThumbnailLane,
    after_id: i64,
    limit: i64,
    now: i64,
) -> Result<Vec<ThumbnailCandidate>> {
    thumbnail_lane_page(conn, "video_cover", run_id, lane, after_id, limit, now)
}

fn thumbnail_lane_page(
    conn: &Connection,
    kind: &str,
    run_id: &str,
    lane: ThumbnailLane,
    after_id: i64,
    limit: i64,
    now: i64,
) -> Result<Vec<ThumbnailCandidate>> {
    let sql = format!(
        "SELECT m.*, r.path, d.rel_path, vm.video_codec, r.volume_id
         FROM media_derivations t
         JOIN (SELECT {MEDIA_ITEM_COLUMNS} FROM media_items) m
           ON m.id=t.item_id AND m.source_revision=t.source_revision
         JOIN directories d ON m.directory_id=d.id
         JOIN scan_roots r ON d.root_id=r.id
         LEFT JOIN video_meta vm ON vm.item_id=m.id
         WHERE t.run_id=?1 AND t.kind=?7 AND t.item_id>?3
           AND t.lane IN (?2, ?6)
           AND (t.status=0 OR (t.status=1 AND t.lease_until<=?5))
           AND m.is_deleted=0
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         ORDER BY t.item_id LIMIT ?4"
    );
    let promoted = match lane {
        ThumbnailLane::Fast => ThumbnailLane::ViewportFast,
        ThumbnailLane::Heavy => ThumbnailLane::ViewportHeavy,
        other => other,
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![
            run_id,
            lane.as_str(),
            after_id,
            limit,
            now,
            promoted.as_str(),
            kind
        ],
        |row| {
            let item = map_media_item(row)?;
            let root_path: String = row.get(29)?;
            let rel_path: String = row.get(30)?;
            let abs_path =
                PathBuf::from(resolve_media_path(&root_path, &rel_path, &item.file_name));
            Ok(ThumbnailCandidate {
                item,
                abs_path,
                root_path,
                relative_dir: rel_path,
                volume_id: row.get(32)?,
                video_codec: row.get(31)?,
            })
        },
    )?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 视口小批次一次 JOIN 读取源快照和路径；未授权/隐藏/冷门格式接管项不进入主生成器。
pub fn image_thumbnail_candidates_for_ids(
    conn: &Connection,
    item_ids: &[i64],
) -> Result<Vec<ThumbnailCandidate>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; item_ids.len()].join(",");
    let sql = format!(
        "SELECT m.*, r.path, d.rel_path, r.volume_id
         FROM (SELECT {MEDIA_ITEM_COLUMNS} FROM media_items) m
         JOIN directories d ON m.directory_id=d.id
         JOIN scan_roots r ON d.root_id=r.id
         WHERE m.id IN ({placeholders}) AND m.is_deleted=0 AND m.media_type='image'
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids), |row| {
        let item = map_media_item(row)?;
        let root_path: String = row.get(29)?;
        let rel_path: String = row.get(30)?;
        let abs_path = PathBuf::from(resolve_media_path(&root_path, &rel_path, &item.file_name));
        Ok(ThumbnailCandidate {
            item,
            abs_path,
            root_path,
            relative_dir: rel_path,
            volume_id: row.get(31)?,
            video_codec: None,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 视口批量读取原生视频封面源；冷门格式由旧派生消费者保留。
pub fn native_video_cover_candidates_for_ids(
    conn: &Connection,
    item_ids: &[i64],
) -> Result<Vec<ThumbnailCandidate>> {
    if item_ids.is_empty() || crate::video::native_cover_formats().is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; item_ids.len()].join(",");
    let sql = format!(
        "SELECT m.*, r.path, d.rel_path, vm.video_codec, r.volume_id
         FROM (SELECT {MEDIA_ITEM_COLUMNS} FROM media_items) m
         JOIN directories d ON m.directory_id=d.id
         JOIN scan_roots r ON d.root_id=r.id
         LEFT JOIN video_meta vm ON vm.item_id=m.id
         WHERE m.id IN ({placeholders}) AND m.is_deleted=0 AND m.media_type='video'
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids), |row| {
        let item = map_media_item(row)?;
        let root_path: String = row.get(29)?;
        let rel_path: String = row.get(30)?;
        let abs_path = PathBuf::from(resolve_media_path(&root_path, &rel_path, &item.file_name));
        Ok(ThumbnailCandidate {
            item,
            abs_path,
            root_path,
            relative_dir: rel_path,
            volume_id: row.get(32)?,
            video_codec: row.get(31)?,
        })
    })?;
    rows.filter_map(|row| match row {
        Ok(candidate)
            if crate::video::native_cover_formats()
                .contains(&candidate.item.file_format.as_str()) =>
        {
            Some(Ok(candidate))
        }
        Ok(_) => None,
        Err(error) => Some(Err(AppError::from(error))),
    })
    .collect()
}

/// 分类写回任务行时核对 run 和源快照；视口已提升的 lane 不会被分页分类降级。
pub fn classify_image_thumbnail_task(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    run_id: &str,
    lane: ThumbnailLane,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE media_derivations SET lane=?5, cost_class=?5,
             updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND run_id=?6 AND lane='unclassified'
           AND EXISTS (SELECT 1 FROM media_items m WHERE m.id=?1
                       AND m.source_revision=?4 AND m.is_deleted=0)",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lane.as_str(),
            run_id
        ],
    )?;
    Ok(changed == 1)
}

/// 一页分类在短事务内提交，逐项返回是否仍归属本轮；视口提升/源换代可独立跳过。
pub fn classify_image_thumbnail_batch(
    conn: &Connection,
    run_id: &str,
    entries: &[(ThumbnailTaskKey, ThumbnailLane)],
) -> Result<Vec<bool>> {
    let tx = conn.unchecked_transaction()?;
    let classified = entries
        .iter()
        .map(|(key, lane)| classify_image_thumbnail_task(&tx, key, run_id, *lane))
        .collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(classified)
}

/// 硬删媒体前读取其新缩略图产物指纹；删除行后任务 FK 级联，届时无法再恢复这些路径。
pub fn thumbnail_output_fingerprints(
    conn: &Connection,
    item_id: i64,
) -> Result<Vec<OutputFingerprint>> {
    let mut stmt = conn.prepare(
        "SELECT output_fingerprint FROM media_derivations
         WHERE item_id=?1 AND kind IN ('image_thumb','video_cover')",
    )?;
    let rows = stmt.query_map([item_id], |row| row.get::<_, String>(0))?;
    let mut fingerprints = Vec::new();
    for row in rows {
        if let Some(fp) = OutputFingerprint::from_hex(&row?) {
            fingerprints.push(fp);
        }
    }
    Ok(fingerprints)
}

/// 全量复位或启动清账撤销图片 lease。运行中调用方须先取消旧 token 并持数据库生命周期写区。
pub fn reset_image_thumbnail_leases(conn: &Connection) -> Result<usize> {
    conn.execute(
        "UPDATE media_derivations
         SET status=0, lease_id=NULL, lease_until=NULL, run_id=NULL,
             updated_at=strftime('%s','now')
         WHERE kind='image_thumb' AND status=1",
        [],
    )
    .map_err(AppError::from)
}

/// 全量重做、清缓存或启动清账撤销原生视频封面在途 lease。
pub fn reset_native_video_cover_leases(conn: &Connection) -> Result<usize> {
    conn.execute(
        "UPDATE media_derivations
         SET status=0, lease_id=NULL, lease_until=NULL, run_id=NULL,
             updated_at=strftime('%s','now')
         WHERE kind='video_cover' AND output_fingerprint!='' AND status=1",
        [],
    )
    .map_err(AppError::from)
}

/// 全量重做仅复位原生视频容器，保留冷门格式现有派生/插件产物。
pub fn reset_native_video_covers_for_full_run(conn: &Connection) -> Result<usize> {
    let formats = crate::video::native_cover_formats();
    if formats.is_empty() {
        return Ok(0);
    }
    let placeholders = (1..=formats.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "UPDATE media_items
         SET thumb_status=0, thumb_path=NULL, thumbhash=NULL
         WHERE is_deleted=0 AND media_type='video' AND file_format IN ({placeholders})"
    );
    conn.execute(&sql, rusqlite::params_from_iter(formats.iter()))
        .map_err(AppError::from)
}

/// 缩略图 Coordinator 接管的派生任务种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailTaskKind {
    Image,
    VideoCover,
}

impl ThumbnailTaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image_thumb",
            Self::VideoCover => "video_cover",
        }
    }
}

/// 任务队列阶段；ImageRs是实际路由发现后的软件解码尾批。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailLane {
    Fast,
    Heavy,
    Exception,
    ImageRs,
    ViewportFast,
    ViewportHeavy,
}

impl ThumbnailLane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Heavy => "heavy",
            Self::Exception => "exception",
            Self::ImageRs => "image_rs",
            Self::ViewportFast => "viewport_fast",
            Self::ViewportHeavy => "viewport_heavy",
        }
    }
}

/// 视口加入已有待处理任务时只提升队列优先级，不重置任务身份或在途 lease。
pub fn promote_thumbnail_task(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    classified_lane: ThumbnailLane,
) -> Result<bool> {
    let promoted = match classified_lane {
        ThumbnailLane::Fast => ThumbnailLane::ViewportFast,
        ThumbnailLane::Heavy => ThumbnailLane::ViewportHeavy,
        _ => {
            return Err(AppError::Internal(
                "viewport promotion requires cost class".into(),
            ))
        }
    };
    let changed = conn.execute(
        "UPDATE media_derivations
         SET lane=CASE lane WHEN 'fast' THEN 'viewport_fast'
                            WHEN 'heavy' THEN 'viewport_heavy'
                            ELSE ?5 END,
             updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND status=0
           AND lane IN ('fast','heavy','unclassified')
           AND EXISTS (SELECT 1 FROM media_items m WHERE m.id=?1
                       AND m.source_revision=?4 AND m.is_deleted=0)",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            promoted.as_str()
        ],
    )?;
    Ok(changed == 1)
}

/// 已领取任务是否处于最终尝试阶段；异常/软件尾批不能再回到已结束的阶段。
pub fn is_thumbnail_exception(conn: &Connection, key: &ThumbnailTaskKey) -> Result<bool> {
    conn.query_row(
        "SELECT lane IN ('exception','image_rs') FROM media_derivations
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision
        ],
        |row| row.get(0),
    )
    .optional()
    .map(|value| value.unwrap_or(false))
    .map_err(AppError::from)
}

/// 首次失败退出快速/重型 worker，移至异常尾批；旧 lease 无权延期新领取者的任务。
pub fn defer_thumbnail_failure(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
    reason: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE media_derivations
         SET status=0, lane='exception', lease_id=NULL, lease_until=NULL,
             failure_reason=?6, updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND status=1 AND lease_id=?5
           AND lane IN ('fast','heavy','viewport_fast','viewport_heavy')
           AND EXISTS (SELECT 1 FROM media_items m WHERE m.id=?1
                       AND m.source_revision=?4 AND m.is_deleted=0)",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lease_id,
            reason
        ],
    )?;
    Ok(changed == 1)
}

/// 软件回退在解码前延期至最后阶段；仅当前源版本的有效lease可移动成员。
pub fn defer_thumbnail_image_rs(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE media_derivations
         SET status=0, lane='image_rs', lease_id=NULL, lease_until=NULL,
             failure_reason=NULL, updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind='image_thumb' AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND status=1 AND lease_id=?5
           AND lane IN ('fast','heavy','exception','viewport_fast','viewport_heavy')
           AND EXISTS (SELECT 1 FROM media_items m WHERE m.id=?1
                       AND m.source_revision=?4 AND m.is_deleted=0)",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lease_id
        ],
    )?;
    Ok(changed == 1)
}

/// 本轮仍可运行的任务数；源已换代和已删除的成员不阻塞阶段转换。
pub fn image_thumbnail_run_open_counts(conn: &Connection, run_id: &str) -> Result<[u64; 4]> {
    thumbnail_run_open_counts(conn, run_id, "image_thumb")
}

/// 本轮原生视频封面的阶段未完成数。
pub fn native_video_cover_run_open_counts(conn: &Connection, run_id: &str) -> Result<[u64; 4]> {
    thumbnail_run_open_counts(conn, run_id, "video_cover")
}

fn thumbnail_run_open_counts(conn: &Connection, run_id: &str, kind: &str) -> Result<[u64; 4]> {
    let sql = format!(
        "SELECT t.lane, COUNT(*) FROM media_derivations t
         JOIN media_items m ON m.id=t.item_id AND m.source_revision=t.source_revision
         WHERE t.run_id=?1 AND t.kind=?2 AND t.status IN (0,1)
           AND m.is_deleted=0
           {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         GROUP BY t.lane"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![run_id, kind], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
    })?;
    let mut counts = [0; 4];
    for row in rows {
        let (lane, count) = row?;
        match lane.as_str() {
            "fast" | "viewport_fast" | "unclassified" => counts[0] += count,
            "heavy" | "viewport_heavy" => counts[1] += count,
            "exception" => counts[2] += count,
            "image_rs" => counts[3] += count,
            _ => {}
        }
    }
    Ok(counts)
}

/// 同一媒体源和同一输出配置的唯一任务身份。数据库 epoch 由持有连接的调用方检查。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailTaskKey {
    pub item_id: i64,
    pub source_revision: i64,
    pub kind: ThumbnailTaskKind,
    pub output_fingerprint: String,
}

/// 根据媒体源快照入队。源已换代时清理旧 lease；同代次重复入队不改变已完成状态。
/// `run_id` 仅标明本轮归属，不作为任务键。
pub fn enqueue_thumbnail_task(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lane: ThumbnailLane,
    cost_class: &str,
    run_id: &str,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(
        "INSERT INTO media_derivations
             (item_id, kind, output_fingerprint, source_revision, lane, cost_class, run_id)
         SELECT m.id, ?2, ?3, ?4, ?5, ?6, ?7 FROM media_items m
         WHERE m.id=?1 AND m.source_revision=?4 AND m.is_deleted=0
         ON CONFLICT(item_id, kind, output_fingerprint) DO UPDATE SET
             source_revision=excluded.source_revision,
             status=0, payload_path=NULL, error=NULL,
             lane=excluded.lane, cost_class=excluded.cost_class,
             attempt_count=0, failure_reason=NULL,
             lease_id=NULL, lease_until=NULL,
             run_id=CASE WHEN media_derivations.source_revision=excluded.source_revision
                             AND media_derivations.status IN (0,1)
                         THEN media_derivations.run_id ELSE excluded.run_id END,
             updated_at=strftime('%s','now')
         WHERE media_derivations.source_revision != excluded.source_revision
            OR (media_derivations.status IN (2,3) AND EXISTS (
                SELECT 1 FROM media_items m
                WHERE m.id=excluded.item_id
                  AND (m.thumb_status=0 OR
                       (m.thumb_status=1 AND m.thumb_path IS NOT media_derivations.payload_path))))",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lane.as_str(),
            cost_class,
            run_id,
        ],
    )?;
    if key.kind == ThumbnailTaskKind::VideoCover && !key.output_fingerprint.is_empty() {
        tx.execute(
            "DELETE FROM media_derivations
             WHERE item_id=?1 AND kind='video_cover' AND output_fingerprint=''
               AND EXISTS (SELECT 1 FROM media_derivations next
                           WHERE next.item_id=?1 AND next.kind='video_cover'
                             AND next.output_fingerprint=?2)",
            params![key.item_id, key.output_fingerprint],
        )?;
    }
    tx.commit()?;
    Ok(changed)
}

/// 在单个 SQL 更新中领取任务。过期 lease 可重领；原领取者的完成写被 lease_id 挡住。
/// `lease_until` 与 `now` 均为 Unix 秒，调用方必须保证 lease_id 在进程/轮次间唯一。
pub fn claim_thumbnail_task(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
    now: i64,
    lease_until: i64,
) -> Result<bool> {
    if lease_until <= now {
        return Err(AppError::Internal(
            "thumbnail lease deadline is not in the future".into(),
        ));
    }
    let changed = conn.execute(
        "UPDATE media_derivations
         SET status=1, lease_id=?5, lease_until=?6,
             attempt_count=attempt_count+1, updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4
           AND (status=0 OR (status=1 AND lease_until<=?7))
           AND EXISTS (
               SELECT 1 FROM media_items m WHERE m.id=?1 AND m.is_deleted=0
                 AND m.source_revision=?4
           )",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lease_id,
            lease_until,
            now,
        ],
    )?;
    Ok(changed == 1)
}

/// 执行方取消后只归还自己的 lease；其他请求方仍可领取同一任务。
pub fn release_thumbnail_lease(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE media_derivations SET status=0, lease_id=NULL, lease_until=NULL,
             updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND status=1 AND lease_id=?5",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lease_id
        ],
    )?;
    Ok(changed == 1)
}

/// 暂不可用结束本轮任务但不改展示行；下一轮或视口可重试，旧 lease 不能结束新任务。
pub fn finish_thumbnail_unavailable(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
    reason: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE media_derivations
         SET status=3, failure_reason=?6, lease_id=NULL, lease_until=NULL,
             updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND status=1 AND lease_id=?5
           AND EXISTS (SELECT 1 FROM media_items m WHERE m.id=?1
                       AND m.source_revision=?4 AND m.is_deleted=0)",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lease_id,
            reason
        ],
    )?;
    Ok(changed == 1)
}

/// 只有当前 lease 与源快照均匹配时才完成任务。`publish` 由 Coordinator 按当前输出配置
/// 决定；旧配置的成功产物可以记为完成，但不能覆盖 media_items 的当前展示路径。
pub fn finish_thumbnail_task(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
    result: &ThumbResult,
    publish: bool,
) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let finished = finish_thumbnail_task_in_transaction(&tx, key, lease_id, result, publish)?;
    tx.commit()?;
    Ok(finished.task_committed)
}

/// 任务行与当前展示行的独立写入结果；展示可能因已有成功产物而拒绝失败覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailFinish {
    /// lease 与源快照匹配，任务行已完成。
    pub task_committed: bool,
    /// media_items 当前展示行确实接受本次产物。
    pub display_updated: bool,
}

/// 调用方持有写事务时执行同一条件提交，供完成队列合批使用。
pub fn finish_thumbnail_task_in_transaction(
    conn: &Connection,
    key: &ThumbnailTaskKey,
    lease_id: &str,
    result: &ThumbResult,
    publish: bool,
) -> Result<ThumbnailFinish> {
    if result.item_id != key.item_id || result.source_revision != key.source_revision {
        return Err(AppError::Internal(
            "thumbnail result snapshot does not match task".into(),
        ));
    }
    let task_status = if result.thumb_status == 2 { 3 } else { 2 };
    let changed = conn.execute(
        "UPDATE media_derivations
         SET status=?6, payload_path=?7, lease_id=NULL, lease_until=NULL,
             failure_reason=CASE WHEN ?6=3 THEN 'decode_or_encode' ELSE NULL END,
             updated_at=strftime('%s','now')
         WHERE item_id=?1 AND kind=?2 AND output_fingerprint=?3
           AND source_revision=?4 AND status=1 AND lease_id=?5
           AND EXISTS (
               SELECT 1 FROM media_items m WHERE m.id=?1 AND m.is_deleted=0
                 AND m.source_revision=?4 AND m.cache_key=?8
           )",
        params![
            key.item_id,
            key.kind.as_str(),
            key.output_fingerprint,
            key.source_revision,
            lease_id,
            task_status,
            result.thumb_path,
            result.cache_key,
        ],
    )?;
    let display_updated = if changed == 1 && publish {
        super::thumbnail::update_thumb_result_if_current(
            conn,
            result.item_id,
            result.source_revision,
            result.cache_key,
            result.thumb_status,
            result.thumb_path.as_deref(),
            result.thumbhash.as_deref(),
        )? == 1
    } else {
        false
    };
    Ok(ThumbnailFinish {
        task_committed: changed == 1,
        display_updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name)
                 VALUES (1, 1, '', 'R');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, sort_datetime, cache_key, source_revision)
                 VALUES (7, 1, 'a.jpg', 100, 1, 'jpg', 'image', 1, 91, 4);",
        )
        .unwrap();
        conn
    }

    fn key(fingerprint: &str) -> ThumbnailTaskKey {
        ThumbnailTaskKey {
            item_id: 7,
            source_revision: 4,
            kind: ThumbnailTaskKind::Image,
            output_fingerprint: fingerprint.into(),
        }
    }

    #[test]
    fn distinct_outputs_have_one_lease_each_and_stale_lease_cannot_commit() {
        let conn = fixture();
        let small = key("size=256");
        let large = key("size=512");
        for task in [&small, &large] {
            enqueue_thumbnail_task(&conn, task, ThumbnailLane::Fast, "small", "run-1").unwrap();
        }
        assert!(claim_thumbnail_task(&conn, &small, "old", 10, 20).unwrap());
        assert!(!claim_thumbnail_task(&conn, &small, "other", 11, 21).unwrap());
        assert!(claim_thumbnail_task(&conn, &large, "large", 11, 21).unwrap());
        assert!(claim_thumbnail_task(&conn, &small, "new", 20, 30).unwrap());

        let result = ThumbResult {
            item_id: 7,
            thumb_status: 1,
            thumb_path: Some("256/a.webp".into()),
            thumbhash: None,
            source_revision: 4,
            cache_key: 91,
        };
        assert!(!finish_thumbnail_task(&conn, &small, "old", &result, true).unwrap());
        assert!(finish_thumbnail_task(&conn, &small, "new", &result, true).unwrap());
        let path: String = conn
            .query_row("SELECT thumb_path FROM media_items WHERE id=7", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(path, "256/a.webp");
    }

    #[test]
    fn commit_batch_rolls_back_together_and_publishes_only_current_rows() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, sort_datetime, cache_key, source_revision)
             VALUES (8, 1, 'b.jpg', 100, 1, 'jpg', 'image', 1, 92, 1)",
            [],
        )
        .unwrap();
        let first = key("batch");
        let second = ThumbnailTaskKey {
            item_id: 8,
            source_revision: 1,
            kind: ThumbnailTaskKind::Image,
            output_fingerprint: "batch".into(),
        };
        for (task, lease) in [(&first, "first"), (&second, "second")] {
            enqueue_thumbnail_task(&conn, task, ThumbnailLane::Fast, "fast", "run").unwrap();
            assert!(claim_thumbnail_task(&conn, task, lease, 10, 20).unwrap());
        }
        let first_result = ThumbResult {
            item_id: 7,
            thumb_status: 1,
            thumb_path: Some("256/batch/91.webp".into()),
            thumbhash: None,
            source_revision: 4,
            cache_key: 91,
        };
        let second_result = ThumbResult {
            item_id: 8,
            thumb_status: 1,
            thumb_path: Some("256/batch/92.webp".into()),
            thumbhash: None,
            source_revision: 1,
            cache_key: 92,
        };
        {
            let tx = conn.unchecked_transaction().unwrap();
            assert!(
                finish_thumbnail_task_in_transaction(&tx, &first, "first", &first_result, true)
                    .unwrap()
                    .display_updated
            );
            let mut stale = second_result.clone();
            stale.source_revision = 2;
            assert!(
                finish_thumbnail_task_in_transaction(&tx, &second, "second", &stale, true).is_err()
            );
            // 错误时整批丢弃，原 lease 可由下一次提交重用。
        }
        let status: i64 = conn
            .query_row(
                "SELECT status FROM media_derivations WHERE item_id=7 AND kind='image_thumb'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, 1);
        let tx = conn.unchecked_transaction().unwrap();
        assert!(
            finish_thumbnail_task_in_transaction(&tx, &first, "first", &first_result, true)
                .unwrap()
                .display_updated
        );
        assert!(
            finish_thumbnail_task_in_transaction(&tx, &second, "second", &second_result, true)
                .unwrap()
                .display_updated
        );
        tx.commit().unwrap();
        let published: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_items WHERE id IN (7,8) AND thumb_status=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(published, 2);

        let failed_variant = key("failed-variant");
        enqueue_thumbnail_task(
            &conn,
            &failed_variant,
            ThumbnailLane::Exception,
            "heavy",
            "run",
        )
        .unwrap();
        assert!(claim_thumbnail_task(&conn, &failed_variant, "failed", 11, 21).unwrap());
        let failed_result = ThumbResult {
            thumb_status: 2,
            thumb_path: None,
            ..first_result.clone()
        };
        let tx = conn.unchecked_transaction().unwrap();
        let finished = finish_thumbnail_task_in_transaction(
            &tx,
            &failed_variant,
            "failed",
            &failed_result,
            true,
        )
        .unwrap();
        assert!(finished.task_committed);
        assert!(
            !finished.display_updated,
            "已有成功缩略图不被失败覆盖或计作新发布"
        );
        tx.commit().unwrap();

        // 资源准入失败只结束该成员，不能清掉已有展示产物或被同轮再次领取。
        let unavailable = key("memory-limit");
        enqueue_thumbnail_task(
            &conn,
            &unavailable,
            ThumbnailLane::Fast,
            "fast",
            "memory-run",
        )
        .unwrap();
        assert!(claim_thumbnail_task(&conn, &unavailable, "memory", 30, 40).unwrap());
        assert_eq!(
            image_thumbnail_run_open_counts(&conn, "memory-run").unwrap(),
            [1, 0, 0, 0]
        );
        assert!(
            !finish_thumbnail_unavailable(&conn, &unavailable, "stale", "worker_memory_limit")
                .unwrap()
        );
        assert!(
            finish_thumbnail_unavailable(&conn, &unavailable, "memory", "worker_memory_limit")
                .unwrap()
        );
        assert!(!claim_thumbnail_task(&conn, &unavailable, "same-run", 40, 50).unwrap());
        assert_eq!(
            image_thumbnail_run_open_counts(&conn, "memory-run").unwrap(),
            [0, 0, 0, 0]
        );
        let path: String = conn
            .query_row("SELECT thumb_path FROM media_items WHERE id=7", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(path, "256/batch/91.webp");
        enqueue_thumbnail_task(&conn, &unavailable, ThumbnailLane::Fast, "fast", "next-run")
            .unwrap();
        assert!(claim_thumbnail_task(&conn, &unavailable, "retry", 50, 60).unwrap());

        let software = key("software-tail");
        enqueue_thumbnail_task(
            &conn,
            &software,
            ThumbnailLane::Fast,
            "fast",
            "software-run",
        )
        .unwrap();
        assert!(claim_thumbnail_task(&conn, &software, "discover", 60, 70).unwrap());
        assert!(!defer_thumbnail_image_rs(&conn, &software, "stale").unwrap());
        assert!(defer_thumbnail_image_rs(&conn, &software, "discover").unwrap());
        assert_eq!(
            image_thumbnail_run_open_counts(&conn, "software-run").unwrap(),
            [0, 0, 0, 1]
        );
        assert!(
            image_thumbnail_lane_page(&conn, "software-run", ThumbnailLane::Fast, 0, 10, 70)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            image_thumbnail_lane_page(&conn, "software-run", ThumbnailLane::ImageRs, 0, 10, 70)
                .unwrap()
                .len(),
            1
        );
        let unchanged: String = conn
            .query_row("SELECT thumb_path FROM media_items WHERE id=7", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(unchanged, "256/batch/91.webp");
        assert!(claim_thumbnail_task(&conn, &software, "tail", 70, 80).unwrap());
        assert!(
            !defer_thumbnail_image_rs(&conn, &software, "tail").unwrap(),
            "尾批不能再次延期"
        );
        assert!(
            is_thumbnail_exception(&conn, &software).unwrap(),
            "失败不能返回已结束的异常批"
        );
        let tx = conn.unchecked_transaction().unwrap();
        assert!(
            finish_thumbnail_task_in_transaction(&tx, &software, "tail", &first_result, true)
                .unwrap()
                .display_updated
        );
        tx.commit().unwrap();
        assert_eq!(
            image_thumbnail_run_open_counts(&conn, "software-run").unwrap(),
            [0; 4]
        );
    }

    #[test]
    fn source_change_reopens_task_and_rejects_prior_snapshot() {
        let conn = fixture();
        let old = key("size=256");
        enqueue_thumbnail_task(&conn, &old, ThumbnailLane::Fast, "small", "run-1").unwrap();
        assert!(claim_thumbnail_task(&conn, &old, "old", 10, 20).unwrap());
        conn.execute("UPDATE media_items SET source_revision=5 WHERE id=7", [])
            .unwrap();
        let mut new = old.clone();
        new.source_revision = 5;
        enqueue_thumbnail_task(&conn, &new, ThumbnailLane::Fast, "small", "run-2").unwrap();
        assert!(!claim_thumbnail_task(&conn, &old, "stale", 20, 30).unwrap());
        assert!(claim_thumbnail_task(&conn, &new, "new", 20, 30).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn viewport_video_handoff_is_atomic_and_has_one_claimable_task() {
        let conn = fixture();
        conn.execute_batch(
            "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, sort_datetime, cache_key, source_revision)
             VALUES (8, 1, 'clip.mp4', 100, 1, 'mp4', 'video', 1, 92, 3);
             INSERT INTO video_meta (item_id, video_codec) VALUES (8, 'H264');
             INSERT INTO media_derivations (item_id, kind, status)
             VALUES (8, 'video_cover', 0);",
        )
        .unwrap();
        let candidates = native_video_cover_candidates_for_ids(&conn, &[7, 8]).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].video_codec.as_deref(), Some("H264"));
        let key = ThumbnailTaskKey {
            item_id: 8,
            source_revision: 3,
            kind: ThumbnailTaskKind::VideoCover,
            output_fingerprint: "0123456789abcdef0123456789abcdef".into(),
        };
        assert_eq!(
            enqueue_thumbnail_task(&conn, &key, ThumbnailLane::Heavy, "video", "viewport").unwrap(),
            1
        );
        assert!(claim_thumbnail_task(&conn, &key, "lease", 10, 20).unwrap());
        assert!(!claim_thumbnail_task(&conn, &key, "duplicate", 11, 21).unwrap());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_derivations WHERE item_id=8 AND kind='video_cover'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn cancelled_owner_returns_lease_without_replacing_new_owner() {
        let conn = fixture();
        let task = key("fp");
        enqueue_thumbnail_task(&conn, &task, ThumbnailLane::Fast, "fast", "run").unwrap();
        assert!(claim_thumbnail_task(&conn, &task, "old", 10, 20).unwrap());
        assert!(release_thumbnail_lease(&conn, &task, "old").unwrap());
        assert!(claim_thumbnail_task(&conn, &task, "new", 11, 21).unwrap());
        assert!(!release_thumbnail_lease(&conn, &task, "old").unwrap());
        assert!(!claim_thumbnail_task(&conn, &task, "duplicate", 12, 22).unwrap());
        assert_eq!(reset_image_thumbnail_leases(&conn).unwrap(), 1);
        let stale = ThumbResult {
            item_id: 7,
            thumb_status: 1,
            thumb_path: Some("256/old.webp".into()),
            thumbhash: None,
            source_revision: 4,
            cache_key: 91,
        };
        assert!(!finish_thumbnail_task(&conn, &task, "new", &stale, true).unwrap());
        assert!(claim_thumbnail_task(&conn, &task, "after-reset", 12, 22).unwrap());
    }
}
