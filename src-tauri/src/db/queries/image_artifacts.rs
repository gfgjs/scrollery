//! 普通图片已完成产物、原子条件发布与单轮固定候选快照。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior};

use super::media::{
    get_image_sources_batch, ImageSource, IMAGE_SOURCE_ELIGIBILITY, IMAGE_SOURCE_PAGE_SIZE,
};
use crate::db::schema::{read_schema_version, SCHEMA_VERSION};
use crate::error::{AppError, Result};

/// 一个完整产物身份的持久终态；1=成功、2=确定的内容失败、3=直显。
/// 执行域暂不可用、取消和排队状态不进入本表；成功/直显须携带路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageArtifact {
    pub item_id: i64,
    pub source_revision: i64,
    pub cache_key: i64,
    pub kind: String,
    pub fingerprint: String,
    pub status: i64,
    pub path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
}

/// 与发布输入逐项对应；仅在整个事务成功后返回。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactPublication {
    /// 源仍有效，且失败没有覆盖同身份或媒体当前展示的成功结果。
    pub accepted: bool,
    /// 本事务中新插入了该身份；重复发布或更新已有行均为 false。
    pub inserted: bool,
}

fn map_artifact(row: &Row<'_>) -> rusqlite::Result<ImageArtifact> {
    Ok(ImageArtifact {
        item_id: row.get(0)?,
        source_revision: row.get(1)?,
        cache_key: row.get(2)?,
        kind: row.get(3)?,
        fingerprint: row.get(4)?,
        status: row.get(5)?,
        path: row.get(6)?,
        thumbhash: row.get(7)?,
    })
}

/// 硬删除前读取该项可回收文件的缓存根相对路径；直显原图不属于缓存。
/// 路径含缓存键；其它媒体项仍持有该键时保留所有变体，包括未发布或软删的副本。
/// 调用方须持有同一宿主写锁直至删行与文件回收结束，阻止检查后复制或发布新引用。
pub fn image_artifact_cache_paths(conn: &Connection, item_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT CASE a.kind WHEN 'display' THEN 'thumbnails/' || a.path ELSE a.path END
         FROM image_artifacts a WHERE a.item_id=?1 AND a.status=1 AND a.path IS NOT NULL
           AND a.kind IN ('display','ai')
           AND NOT EXISTS(SELECT 1 FROM media_items m
                          WHERE m.cache_key=a.cache_key AND m.id<>?1)",
    )?;
    let rows = stmt.query_map([item_id], |row| row.get(0))?;
    rows.map(|row| row.map_err(AppError::from)).collect()
}

/// 查找精确身份的持久结果（包括旧源）；该查询本身不证明源当前仍有效。
pub fn find_image_artifact(
    conn: &Connection,
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    kind: &str,
    fingerprint: &str,
) -> Result<Option<ImageArtifact>> {
    Ok(conn
        .prepare_cached(
            "SELECT item_id, source_revision, cache_key, kind, fingerprint, status, path, thumbhash
         FROM image_artifacts WHERE item_id=?1 AND source_revision=?2 AND cache_key=?3
           AND kind=?4 AND fingerprint=?5",
        )?
        .query_row(
            params![item_id, source_revision, cache_key, kind, fingerprint],
            map_artifact,
        )
        .optional()?)
}

/// 按当前源、种类和配置指纹批量查找完成结果，最多 256 个 ID；包含持久内容失败。
pub fn find_image_artifacts(
    conn: &Connection,
    ids: &[i64],
    kind: &str,
    fingerprint: &str,
) -> Result<Vec<ImageArtifact>> {
    if ids.len() > IMAGE_SOURCE_PAGE_SIZE {
        return Err(AppError::Internal(
            "image artifact batch exceeds 256".into(),
        ));
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (3..ids.len() + 3)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT a.item_id, a.source_revision, a.cache_key, a.kind, a.fingerprint,
                a.status, a.path, a.thumbhash
         FROM image_artifacts a JOIN media_items m ON m.id=a.item_id
         JOIN directories d ON d.id=m.directory_id JOIN scan_roots r ON r.id=d.root_id
         WHERE a.kind=?1 AND a.fingerprint=?2 AND m.id IN ({placeholders})
           AND a.source_revision=m.source_revision AND a.cache_key=m.cache_key
           AND {IMAGE_SOURCE_ELIGIBILITY} ORDER BY m.id"
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = vec![&kind, &fingerprint];
    values.extend(ids.iter().map(|id| id as &dyn rusqlite::ToSql));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(values.as_slice(), map_artifact)?;
    rows.map(|row| row.map_err(AppError::from)).collect()
}

/// 批量条件发布；源代次、缓存键、可处理状态在短写事务内校验。
///
/// 任一 SQL/约束错误回滚整批，绝不返回部分成功。`update_display` 仅允许 display
/// 写媒体展示引用；调用方在此之前校验 epoch、配置、所有权及订阅，并先原子发布文件。
/// 新生成计数须同时判断 accepted、inserted 和输入的成功状态，不把失败或重复身份计入。
pub fn publish_image_artifacts(
    conn: &mut Connection,
    artifacts: &[ImageArtifact],
    update_display: bool,
) -> Result<Vec<ArtifactPublication>> {
    if artifacts.is_empty() {
        return Ok(Vec::new());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut results = Vec::with_capacity(artifacts.len());
    {
        let mut exists = tx.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM image_artifacts
             WHERE item_id=?1 AND source_revision=?2 AND cache_key=?3 AND kind=?4 AND fingerprint=?5)",
        )?;
        let mut publish = tx.prepare_cached(&format!(
            "INSERT INTO image_artifacts(item_id,source_revision,cache_key,kind,fingerprint,status,path,thumbhash)
             SELECT m.id,?2,?3,?4,?5,?6,?7,?8 FROM media_items m
             JOIN directories d ON d.id=m.directory_id JOIN scan_roots r ON r.id=d.root_id
             WHERE m.id=?1 AND m.source_revision=?2 AND m.cache_key=?3 AND {IMAGE_SOURCE_ELIGIBILITY}
               AND NOT (?4='display' AND ?6=2 AND m.thumb_status IN (1,3))
             ON CONFLICT(item_id,source_revision,cache_key,kind,fingerprint) DO UPDATE
             SET status=excluded.status,path=excluded.path,thumbhash=excluded.thumbhash
             WHERE NOT (excluded.status=2 AND image_artifacts.status IN (1,3))"
        ))?;
        let mut display = tx.prepare_cached(
            "UPDATE media_items SET thumb_status=?4,thumb_path=?5,thumbhash=?6,
                    updated_at=strftime('%s','now')
             WHERE id=?1 AND source_revision=?2 AND cache_key=?3 AND is_deleted=0
               AND NOT (?4=2 AND thumb_status IN (1,3))",
        )?;
        for a in artifacts {
            let existed: bool = exists.query_row(
                params![
                    a.item_id,
                    a.source_revision,
                    a.cache_key,
                    a.kind,
                    a.fingerprint
                ],
                |row| row.get(0),
            )?;
            let accepted = publish.execute(params![
                a.item_id,
                a.source_revision,
                a.cache_key,
                a.kind,
                a.fingerprint,
                a.status,
                a.path,
                a.thumbhash
            ])? == 1;
            if accepted && update_display && a.kind == "display" {
                display.execute(params![
                    a.item_id,
                    a.source_revision,
                    a.cache_key,
                    a.status,
                    a.path,
                    a.thumbhash
                ])?;
            }
            results.push(ArtifactPublication {
                accepted,
                inserted: accepted && !existed,
            });
        }
    }
    tx.commit()?;
    Ok(results)
}

/// 一轮的固定候选成员；`source=None` 表示源已变化、删除或不再归普通图片管线。
/// 过期成员仍返回原身份，调用方须把它计入本轮已处理项。
#[derive(Debug)]
pub struct ImageCandidate {
    pub item_id: i64,
    pub source_revision: i64,
    pub cache_key: i64,
    pub source: Option<ImageSource>,
}

/// 独占专用连接的固定候选集；TEMP 仅存身份，不保存调度或媒体实体。
/// 创建和每页读取结束即释放主库读快照，不跨图片生成持有读事务。
pub struct ImageCandidateSnapshot {
    conn: Connection,
    total: usize,
    after_id: Option<i64>,
}

impl ImageCandidateSnapshot {
    /// 打开已有当前格式库，按当前 display/可选 AI 指纹一次固定缺项成员。
    /// `ai_fingerprint=None` 表示本轮不要求 AI 产物；当前 display 确定失败也不再要求 AI。
    /// 展示引用已重置的项也须入选，以便复用历史产物或检查新缓存目录中的文件。
    /// 文件是否存在由缓存校验后调用失效接口。
    pub fn open(
        db_path: &Path,
        display_fingerprint: &str,
        ai_fingerprint: Option<&str>,
    ) -> Result<Self> {
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(5))?;
        if read_schema_version(&conn) != SCHEMA_VERSION {
            return Err(AppError::SchemaIncompatible);
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA temp_store=FILE;
             CREATE TEMP TABLE image_candidates(
                 item_id INTEGER PRIMARY KEY,source_revision INTEGER NOT NULL,cache_key INTEGER NOT NULL);",
        )?;
        conn.execute(&format!(
            "INSERT INTO temp.image_candidates
             SELECT m.id,m.source_revision,m.cache_key FROM media_items m
             JOIN directories d ON d.id=m.directory_id JOIN scan_roots r ON r.id=d.root_id
             LEFT JOIN image_artifacts display ON display.item_id=m.id
                 AND display.source_revision=m.source_revision AND display.cache_key=m.cache_key
                 AND display.kind='display' AND display.fingerprint=?1
             WHERE {IMAGE_SOURCE_ELIGIBILITY} AND (
                  m.thumb_status=0 OR display.item_id IS NULL
                 OR (display.status<>2 AND ?2 IS NOT NULL AND NOT EXISTS(SELECT 1 FROM image_artifacts a WHERE a.item_id=m.id
                     AND a.source_revision=m.source_revision AND a.cache_key=m.cache_key
                     AND a.kind='ai' AND a.fingerprint=?2)))"
        ), params![display_fingerprint, ai_fingerprint])?;
        let total = conn.query_row("SELECT COUNT(*) FROM temp.image_candidates", [], |row| {
            row.get::<_, i64>(0)
        })? as usize;
        Ok(Self {
            conn,
            total,
            after_id: None,
        })
    }

    /// 创建快照时的固定总数，不因生成期间的插入、删除或源变化而改变。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 消费下一个 keyset 页，`limit` 必须为 1..=256；空页表示完成。
    /// 游标只在整页读取成功后推进，不丢弃查源时已过期的固定成员。
    pub fn next_page(&mut self, limit: usize) -> Result<Vec<ImageCandidate>> {
        if limit == 0 || limit > IMAGE_SOURCE_PAGE_SIZE {
            return Err(AppError::Internal(
                "image candidate page must be 1..=256".into(),
            ));
        }
        let mut page = {
            let mut stmt = self.conn.prepare(
                "SELECT item_id,source_revision,cache_key FROM temp.image_candidates
                 WHERE item_id > ?1 ORDER BY item_id LIMIT ?2",
            )?;
            // 无初始游标时独立 SQL，既保留 INTEGER PRIMARY KEY 的范围查找，也覆盖负 ID。
            let mut first = self.conn.prepare(
                "SELECT item_id,source_revision,cache_key FROM temp.image_candidates ORDER BY item_id LIMIT ?1",
            )?;
            let map = |row: &Row<'_>| {
                Ok(ImageCandidate {
                    item_id: row.get(0)?,
                    source_revision: row.get(1)?,
                    cache_key: row.get(2)?,
                    source: None,
                })
            };
            let rows = match self.after_id {
                Some(after) => stmt.query_map(params![after, limit as i64], map)?,
                None => first.query_map(params![limit as i64], map)?,
            };
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let ids: Vec<i64> = page.iter().map(|candidate| candidate.item_id).collect();
        let mut sources: HashMap<i64, ImageSource> = get_image_sources_batch(&self.conn, &ids)?
            .into_iter()
            .map(|source| (source.item.id, source))
            .collect();
        for candidate in &mut page {
            candidate.source = sources.remove(&candidate.item_id).filter(|source| {
                source.item.source_revision == candidate.source_revision
                    && source.item.cache_key == candidate.cache_key
            });
        }
        if let Some(last) = page.last() {
            self.after_id = Some(last.item_id);
        }
        Ok(page)
    }
}

/// 使指定图片的所有产物失效；返回被复位展示引用的 ID，不删除文件。
pub fn invalidate_image_artifacts_for_items(
    conn: &mut Connection,
    ids: &[i64],
) -> Result<Vec<i64>> {
    invalidate_artifacts(conn, "item_id", ids)
}

/// 缓存驱逐按完整产物路径失效，保留其它尺寸/指纹；返回被复位展示引用的 ID。
pub fn invalidate_image_artifacts_by_paths(
    conn: &mut Connection,
    paths: &[String],
) -> Result<Vec<i64>> {
    invalidate_artifacts(conn, "path", paths)
}

/// 清除所有普通图片产物及其当前展示引用；非图片与 exotic 的独立结果不受影响。
pub fn clear_image_artifacts(conn: &mut Connection) -> Result<Vec<i64>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids = reset_artifact_display(&tx, "1", &[])?;
    tx.execute("DELETE FROM image_artifacts", [])?;
    tx.commit()?;
    Ok(ids)
}

fn invalidate_artifacts<T: rusqlite::ToSql>(
    conn: &mut Connection,
    column: &str,
    values: &[T],
) -> Result<Vec<i64>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut ids = Vec::new();
    for chunk in values.chunks(IMAGE_SOURCE_PAGE_SIZE) {
        // 列名仅来自上述两条内部固定调用；输入值一律绑定。
        let predicate = format!("a.{column} IN ({})", vec!["?"; chunk.len()].join(","));
        let values: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        ids.extend(reset_artifact_display(&tx, &predicate, &values)?);
        tx.execute(
            &format!("DELETE FROM image_artifacts AS a WHERE {predicate}"),
            values.as_slice(),
        )?;
    }
    tx.commit()?;
    Ok(ids)
}

fn reset_artifact_display(
    conn: &Connection,
    predicate: &str,
    values: &[&dyn rusqlite::ToSql],
) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(&format!(
        "UPDATE media_items SET thumb_status=0,thumb_path=NULL,thumbhash=NULL
         WHERE media_type='image' AND EXISTS(SELECT 1 FROM image_artifacts a
             WHERE a.item_id=media_items.id AND a.source_revision=media_items.source_revision
               AND a.cache_key=media_items.cache_key AND a.kind='display'
               AND a.status=media_items.thumb_status AND a.path IS media_items.thumb_path
               AND {predicate}) RETURNING id"
    ))?;
    let rows = stmt.query_map(values, |row| row.get(0))?;
    rows.map(|row| row.map_err(AppError::from)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries::media::get_media_item;
    use crate::db::schema::initialize_schema;

    fn seed(conn: &Connection) {
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        initialize_schema(conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots(id,path) VALUES(1,'C:/photos');
             INSERT INTO directories(id,root_id,rel_path,name) VALUES(1,1,'album','album');",
        )
        .unwrap();
    }

    fn add(conn: &Connection, id: i64) {
        conn.execute(
            "INSERT INTO media_items(id,directory_id,file_name,file_size,file_mtime,
                 file_format,sort_datetime,cache_key) VALUES(?1,1,?2,10,11,'jpg',12,?3)",
            params![id, format!("{id}.jpg"), id * 10],
        )
        .unwrap();
    }

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        add(&conn, 1);
        conn
    }

    fn artifact(id: i64, kind: &str, fingerprint: &str) -> ImageArtifact {
        ImageArtifact {
            item_id: id,
            source_revision: 1,
            cache_key: id * 10,
            kind: kind.into(),
            fingerprint: fingerprint.into(),
            status: 1,
            path: Some(format!("{id}/{kind}/{fingerprint}.webp")),
            thumbhash: Some(vec![7, 8]),
        }
    }

    fn count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM image_artifacts", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn deletion_paths_include_all_committed_variants_but_never_direct_sources() {
        let mut conn = memory();
        add(&conn, 2);
        let mut direct = artifact(1, "display", "direct");
        direct.status = 3;
        direct.path = Some("C:/photos/album/1.jpg".into());
        let mut failed = artifact(1, "display", "failed");
        failed.status = 2;
        failed.path = None;
        let display = artifact(1, "display", "64");
        let other_size = artifact(1, "display", "128");
        let mut ai = artifact(1, "ai", "336");
        ai.path = Some("ai_thumbs/v2/336/00/r1/000000000000000a.webp".into());
        publish_image_artifacts(
            &mut conn,
            &[
                display,
                other_size,
                ai,
                direct,
                failed,
                artifact(2, "display", "64"),
            ],
            true,
        )
        .unwrap();
        let mut paths = image_artifact_cache_paths(&conn, 1).unwrap();
        paths.sort();
        assert_eq!(
            paths,
            [
                "ai_thumbs/v2/336/00/r1/000000000000000a.webp",
                "thumbnails/1/display/128.webp",
                "thumbnails/1/display/64.webp",
            ]
        );
        conn.execute("DELETE FROM media_items WHERE id=?1", [1])
            .unwrap();
        assert!(image_artifact_cache_paths(&conn, 1).unwrap().is_empty());
        assert_eq!(
            image_artifact_cache_paths(&conn, 2).unwrap(),
            ["thumbnails/2/display/64.webp"]
        );
        assert_eq!(paths.len(), 3, "删行前保留的路径仍可用于文件回收");
    }

    #[test]
    fn deletion_preserves_inherited_and_pending_cache_owners() {
        let mut conn = memory();
        let display = artifact(1, "display", "64");
        publish_image_artifacts(&mut conn, &[display], true).unwrap();
        conn.execute(
            "INSERT INTO directories(id,root_id,rel_path,name) VALUES(2,1,'copy','copy')",
            [],
        )
        .unwrap();
        // 旧版复制曾共用源项缓存键；清理仍须保护尚未修复的历史共享引用。
        let copy: i64 = conn
            .query_row(
                "INSERT INTO media_items(directory_id,file_name,file_size,file_mtime,file_format,
                sort_datetime,cache_key,thumb_status,thumb_path)
             SELECT 2,file_name,file_size,file_mtime,file_format,sort_datetime,cache_key,
                thumb_status,thumb_path FROM media_items WHERE id=1 RETURNING id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count(&conn), 1, "副本尚无自己的 artifact 行");
        assert!(image_artifact_cache_paths(&conn, 1).unwrap().is_empty());
        conn.execute(
            "UPDATE media_items SET thumb_status=0,thumb_path=NULL,is_deleted=1 WHERE id=?1",
            [copy],
        )
        .unwrap();
        assert!(
            image_artifact_cache_paths(&conn, 1).unwrap().is_empty(),
            "保留尚未发布及可恢复的副本所用缓存键"
        );
        conn.execute("DELETE FROM media_items WHERE id=?1", [copy])
            .unwrap();
        assert_eq!(image_artifact_cache_paths(&conn, 1).unwrap().len(), 1);
    }

    #[test]
    fn publication_cas_rejects_changed_deleted_and_non_image_sources() {
        let mut conn = memory();
        for id in 2..=5 {
            add(&conn, id);
        }
        conn.execute_batch(
            "UPDATE media_items SET source_revision=2 WHERE id=2;
             UPDATE media_items SET cache_key=999 WHERE id=3;
             UPDATE media_items SET is_deleted=1 WHERE id=4;
             UPDATE media_items SET media_type='video' WHERE id=5;",
        )
        .unwrap();
        let inputs: Vec<_> = (1..=6).map(|id| artifact(id, "display", "480")).collect();
        let results = publish_image_artifacts(&mut conn, &inputs, true).unwrap();
        assert_eq!(results.len(), inputs.len());
        assert_eq!(
            results[0],
            ArtifactPublication {
                accepted: true,
                inserted: true
            }
        );
        assert!(results[1..].iter().all(|r| !r.accepted && !r.inserted));
        assert_eq!(count(&conn), 1);
        assert_eq!(get_media_item(&conn, 4).unwrap().thumb_status, 0);
        conn.execute("DELETE FROM media_items WHERE id=1", [])
            .unwrap();
        assert_eq!(count(&conn), 0, "物理删除必须级联删除产物");
        assert!(!publish_image_artifacts(&mut conn, &inputs[..1], true).unwrap()[0].accepted);
    }

    #[test]
    fn display_sql_error_rolls_back_artifact_and_earlier_rows() {
        let mut conn = memory();
        add(&conn, 2);
        conn.execute_batch(
            "CREATE TRIGGER fail_display BEFORE UPDATE OF thumb_path ON media_items
             WHEN new.id=2 BEGIN SELECT RAISE(ABORT, 'forced display failure'); END;",
        )
        .unwrap();
        assert!(publish_image_artifacts(
            &mut conn,
            &[artifact(1, "display", "480"), artifact(2, "display", "480")],
            true
        )
        .is_err());
        assert_eq!(count(&conn), 0);
        assert_eq!(get_media_item(&conn, 1).unwrap().thumb_path, None);
    }

    #[test]
    fn invalidation_sql_failure_keeps_artifacts_and_display() {
        let mut conn = memory();
        let display = artifact(1, "display", "480");
        publish_image_artifacts(&mut conn, std::slice::from_ref(&display), true).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON image_artifacts BEGIN SELECT RAISE(ABORT,'forced'); END;").unwrap();
        assert!(clear_image_artifacts(&mut conn).is_err());
        assert_eq!(count(&conn), 1);
        assert_eq!(get_media_item(&conn, 1).unwrap().thumb_path, display.path);
    }
}
