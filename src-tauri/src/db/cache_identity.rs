//! 一次性修复旧库中跨目录共享的派生缓存身份。

use rusqlite::{params, Connection, Transaction, TransactionBehavior};

const MARKER: &str = "media_cache_namespace_v1";

fn completed(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM app_config WHERE key=?1 AND value='1')",
        [MARKER],
        |row| row.get(0),
    )
}

/// 隔离旧库中共享缓存键的资产；完成标记与全部失效写入在同一事务提交。
pub fn reconcile_cache_key_collisions(conn: &Connection) -> crate::error::Result<usize> {
    if completed(conn)? {
        return Ok(0);
    }
    // 多实例同时启动时，取得写锁后再检查标记，避免重复推进源代次。
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if completed(&tx)? {
        return Ok(0);
    }
    let candidates = {
        let mut stmt = tx.prepare(
            "SELECT m.id,m.directory_id,d.rel_path,m.file_name,m.file_mtime,
                    COALESCE(m.file_mtime_ns,0)
             FROM media_items m JOIN directories d ON d.id=m.directory_id
             WHERE m.cache_key IN (
                 SELECT cache_key FROM media_items GROUP BY cache_key HAVING COUNT(*)>1
             ) ORDER BY m.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    // 先收集完整冲突集合：改第一项的键后，其余成员仍必须获得独立身份。
    for (id, directory_id, rel_path, file_name, file_mtime, file_mtime_ns) in &candidates {
        let key = crate::utils::hash::compute_cache_key_with_mtime_ns(
            *directory_id,
            rel_path,
            file_name,
            *file_mtime,
            *file_mtime_ns,
        );
        tx.execute(
            "UPDATE media_items SET cache_key=?2,source_revision=source_revision+1,
                thumb_status=0,thumb_path=NULL,thumbhash=NULL,ai_status=0,
                updated_at=strftime('%s','now') WHERE id=?1",
            params![id, key],
        )?;
        for sql in [
            "DELETE FROM image_artifacts WHERE item_id=?1",
            "DELETE FROM ai_embeddings WHERE item_id=?1",
            "DELETE FROM ai_search_results WHERE file_id=?1",
            "UPDATE media_derivations SET status=0,payload_path=NULL,error=NULL,
                updated_at=strftime('%s','now') WHERE item_id=?1",
        ] {
            tx.execute(sql, [id])?;
        }
        crate::db::queries::invalidate_exotic_tasks_for_item(&tx, *id)?;
        // 源文件没有改变。人脸重扫会删除人工确认/拒绝关系，因此保留脸记录和状态；
        // 源元数据与直接读取原文件的 dedup 摘要也保留原代次，不伪造重新分析结果。
    }
    tx.execute(
        "INSERT INTO app_config(key,value) VALUES (?1,'1')
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [MARKER],
    )?;
    tx.commit()?;
    Ok(candidates.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, types::Value};

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots(id,path) VALUES (1,'root-a'),(2,'root-b');
             INSERT INTO directories(id,root_id,rel_path,name)
                 VALUES (1,1,'photos','photos'),(2,2,'photos','photos');
             INSERT INTO tags(id,name) VALUES (1,'保留标签');
             INSERT INTO albums(id,name) VALUES (100,'保留图库');
             INSERT INTO persons(id,name,face_count,is_named,is_ignored)
                 VALUES (1,'已确认的人物',1,1,0),(2,'忽略的人物',1,1,1);",
        )
        .unwrap();
        for (id, directory_id, file_name, cache_key) in [
            (1, 1, "same.bmp", 42),
            (2, 2, "same.bmp", 42),
            (3, 1, "other.bmp", 99),
        ] {
            conn.execute(
                "INSERT INTO media_items(id,directory_id,file_name,file_size,file_mtime,
                    file_mtime_ns,file_format,sort_datetime,cache_key,source_revision,
                    thumb_status,thumb_path,thumbhash,ai_status,face_status,is_favorited,
                    rating,color_label,content_hash,view_rotation,playback_position_ms)
                 VALUES (?1,?2,?3,246,1700000000,1700000000123456700,'bmp',1700000000,
                    ?4,7,1,'shared.webp',X'0102',1,1,1,5,3,'source-hash',90,1234)",
                params![id, directory_id, file_name, cache_key],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO image_artifacts(item_id,source_revision,cache_key,kind,
                    fingerprint,status,path,thumbhash)
                 VALUES (?1,7,?2,'display','display-policy',1,'shared.webp',X'0102'),
                    (?1,7,?2,'ai','ai-policy',1,'shared-ai.webp',NULL)",
                params![id, cache_key],
            )
            .unwrap();
            for sql in [
                "INSERT INTO ai_embeddings(item_id,model_name,embedding) VALUES (?1,'clip',X'0102')",
                "INSERT INTO ai_search_results(file_id,similarity) VALUES (?1,0.9)",
                "INSERT INTO media_derivations(item_id,kind,status,payload_path,error) VALUES (?1,'video_cover',1,'shared.webp','old error')",
                "INSERT INTO exotic_tasks(item_id,plugin_id,capability,status,input_fingerprint,attempts,next_retry_at,claimed_at,lease_owner,last_error_code,last_error_message,output_path,worker_version) VALUES (?1,'psd','thumbnail',1,'old-input',2,10,20,'worker','old-code','old-message','shared.webp','1')",
                "INSERT INTO image_meta(item_id,exif_make) VALUES (?1,'camera')",
                "INSERT INTO dedup_index(item_id,source_revision,hash_version,exact_digest,status,checked_at) VALUES (?1,7,1,X'AABB','ready',1)",
                "INSERT INTO item_tags(item_id,tag_id) VALUES (?1,1)",
                "INSERT INTO album_items(album_id,item_id) VALUES (100,?1)",
                "INSERT INTO reading_progress(item_id,position) VALUES (?1,'cfi:unchanged')",
                "INSERT INTO reader_book_prefs(item_id,prefs) VALUES (?1,'{\"theme\":\"dark\"}')",
                "INSERT INTO reader_bookmarks(item_id,locator,label) VALUES (?1,'cfi:bookmark','保留书签')",
                "INSERT INTO document_versions(item_id,label,storage,abs_path,source,is_current) VALUES (?1,'保留修订','external','original/version.md','user',1)",
            ] {
                conn.execute(sql, [id]).unwrap();
            }
        }
        conn.execute_batch(
            "INSERT INTO faces(id,item_id,person_id,model_name,bbox_x,bbox_y,bbox_w,bbox_h,
                det_score,embedding,is_confirmed)
             VALUES (1,1,1,'yunet-sface',0.1,0.1,0.5,0.5,0.9,X'0102',1),
                (2,2,2,'yunet-sface',0.2,0.2,0.4,0.4,0.8,X'0304',1);
             UPDATE persons SET cover_face_id=id;
             INSERT INTO face_coverage(item_id,model_name) VALUES (1,'yunet-sface'),(2,'yunet-sface');
             INSERT INTO face_rejections(face_id,person_id) VALUES (1,2);",
        )
        .unwrap();
        conn
    }

    fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
        let mut stmt = conn.prepare(sql).unwrap();
        let count = stmt.column_count();
        stmt.query_map([], |row| (0..count).map(|i| row.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn preserved_data(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
        [
            "SELECT * FROM scan_roots ORDER BY id",
            "SELECT * FROM directories ORDER BY id",
            "SELECT id,directory_id,file_name,file_size,file_mtime,file_mtime_ns,file_format,
                sort_datetime,is_favorited,is_deleted,rating,color_label,content_hash,
                view_rotation,playback_position_ms,face_status FROM media_items ORDER BY id",
            "SELECT * FROM persons ORDER BY id",
            "SELECT * FROM faces ORDER BY id",
            "SELECT * FROM face_coverage ORDER BY item_id",
            "SELECT * FROM face_rejections ORDER BY face_id",
            "SELECT * FROM image_meta ORDER BY item_id",
            "SELECT * FROM dedup_index ORDER BY item_id",
            "SELECT * FROM tags ORDER BY id",
            "SELECT * FROM item_tags ORDER BY item_id",
            "SELECT * FROM albums ORDER BY id",
            "SELECT * FROM album_items ORDER BY item_id",
            "SELECT * FROM reading_progress ORDER BY item_id",
            "SELECT * FROM reader_book_prefs ORDER BY item_id",
            "SELECT * FROM reader_bookmarks ORDER BY id",
            "SELECT * FROM document_versions ORDER BY id",
        ]
        .map(|sql| rows(conn, sql))
        .to_vec()
    }

    fn cache_data(conn: &Connection, only_unaffected: bool) -> Vec<Vec<Vec<Value>>> {
        let item_predicate = if only_unaffected {
            " WHERE item_id=3"
        } else {
            ""
        };
        let mut snapshot = vec![rows(
            conn,
            if only_unaffected {
                "SELECT * FROM media_items WHERE id=3"
            } else {
                "SELECT * FROM media_items ORDER BY id"
            },
        )];
        for table in [
            "image_artifacts",
            "ai_embeddings",
            "media_derivations",
            "exotic_tasks",
        ] {
            snapshot.push(rows(
                conn,
                &format!("SELECT * FROM {table}{item_predicate} ORDER BY rowid"),
            ));
        }
        snapshot.push(rows(
            conn,
            if only_unaffected {
                "SELECT * FROM ai_search_results WHERE file_id=3"
            } else {
                "SELECT * FROM ai_search_results ORDER BY file_id"
            },
        ));
        snapshot
    }

    #[test]
    fn repairs_only_colliding_cache_identities_preserving_user_data_and_is_idempotent() {
        let conn = fixture();
        let user_before = preserved_data(&conn);
        let unaffected_before = cache_data(&conn, true);
        let config = crate::thumbnail::generator::ThumbConfig {
            cache_dir: std::path::PathBuf::new(),
            size: 256,
            skip_max_bytes: 0,
            strategy: "cpu".into(),
            gpu_engine: "auto".into(),
            ai_hq_cache: true,
            webp_quality: 80,
            ai_cache_short_edge: 336,
        };
        assert_eq!(reconcile_cache_key_collisions(&conn).unwrap(), 2);
        let keys = (1..=2)
            .map(|id| {
                let key = conn
                    .query_row("SELECT cache_key FROM media_items WHERE id=?1", [id], |r| {
                        r.get::<_, i64>(0)
                    })
                    .unwrap();
                assert_eq!(
                    key,
                    crate::utils::hash::compute_cache_key_with_mtime_ns(
                        id,
                        "photos",
                        "same.bmp",
                        1700000000,
                        1700000000123456700
                    )
                );
                key
            })
            .collect::<Vec<_>>();
        assert_ne!(keys[0], keys[1]);
        assert_ne!(
            config.display_path(1, 8, keys[0]),
            config.display_path(2, 8, keys[1])
        );
        assert_ne!(config.ai_path(1, 8, keys[0]), config.ai_path(2, 8, keys[1]));
        assert_eq!(rows(&conn,"SELECT source_revision,thumb_status,thumb_path,thumbhash,ai_status FROM media_items WHERE id<3 ORDER BY id"),vec![vec![Value::Integer(8),Value::Integer(0),Value::Null,Value::Null,Value::Integer(0)];2]);
        assert!(rows(&conn, "SELECT * FROM image_artifacts WHERE item_id<3").is_empty());
        assert!(rows(&conn, "SELECT * FROM ai_embeddings WHERE item_id<3").is_empty());
        assert!(rows(&conn, "SELECT * FROM ai_search_results WHERE file_id<3").is_empty());
        assert_eq!(rows(&conn,"SELECT status,payload_path,error FROM media_derivations WHERE item_id<3 ORDER BY item_id"),vec![vec![Value::Integer(0),Value::Null,Value::Null];2]);
        assert_eq!(rows(&conn,"SELECT status,input_fingerprint,attempts,next_retry_at,claimed_at,lease_owner,last_error_code,last_error_message,output_path,worker_version FROM exotic_tasks WHERE item_id<3 ORDER BY item_id"),vec![vec![Value::Integer(0),Value::Null,Value::Integer(0),Value::Null,Value::Null,Value::Null,Value::Null,Value::Null,Value::Null,Value::Null];2]);
        assert_eq!(preserved_data(&conn), user_before);
        assert_eq!(cache_data(&conn, true), unaffected_before);
        assert_eq!(
            conn.query_row("SELECT value FROM app_config WHERE key=?1", [MARKER], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "1"
        );
        let repaired = cache_data(&conn, false);
        assert_eq!(reconcile_cache_key_collisions(&conn).unwrap(), 0);
        assert_eq!(cache_data(&conn, false), repaired);
        assert_eq!(preserved_data(&conn), user_before);
    }

    #[test]
    fn failed_collision_repair_rolls_back_cache_changes_and_completion_marker() {
        let conn = fixture();
        let cache_before = cache_data(&conn, false);
        let user_before = preserved_data(&conn);
        conn.execute_batch("CREATE TRIGGER fail_cache_repair BEFORE UPDATE OF cache_key ON media_items WHEN OLD.id=2 BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        assert!(reconcile_cache_key_collisions(&conn).is_err());
        assert_eq!(cache_data(&conn, false), cache_before);
        assert_eq!(preserved_data(&conn), user_before);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM app_config WHERE key=?1",
                [MARKER],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        conn.execute_batch("DROP TRIGGER fail_cache_repair;")
            .unwrap();
        assert_eq!(reconcile_cache_key_collisions(&conn).unwrap(), 2);
    }
}
