// src-tauri/src/scanner/fast_scan/tests.rs
//! run_fast_scan 单测四域(自 fast_scan.rs 结构性拆分,tierB-1)。

use super::*;

#[cfg(test)]
mod finalize_tests {
    use super::*;

    /// 内存库 + 一个 scan_root(id=1)/目录(id=10)/卷=5，一项在线媒体(id=100，未在 seen)。
    fn db_with_one_missing_candidate() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, '', 'r1');
             INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key, volume_id, availability)
             VALUES (100, 10, 'a.jpg', 0,0,'jpg','image',0,0,0,0, 5, 'online');",
        )
        .unwrap();
        c
    }

    fn avail(c: &Connection, id: i64) -> String {
        c.query_row(
            "SELECT availability FROM media_items WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 完整门闩拦截：扫描不完整（有遍历错误）→ 即便项真缺失也**不标**（最关键红线）。
    #[test]
    fn incomplete_walk_blocks_deletion() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::new();
        let n = finalize_missing_detection(&c, 1, false, 3, true, Some(5), &seen).unwrap();
        assert_eq!(n, 0, "不完整扫描绝不删除");
        assert_eq!(avail(&c, 100), "online");
    }

    /// TOCTOU 拦截：写删前复查卷已离线 → 不标（防中途拔盘误删）。
    #[test]
    fn offline_at_recheck_blocks_deletion() {
        let c = db_with_one_missing_candidate();
        let seen = HashSet::new();
        let n = finalize_missing_detection(&c, 1, true, 0, false, Some(5), &seen).unwrap();
        assert_eq!(n, 0, "卷离线绝不删除");
        assert_eq!(avail(&c, 100), "online");
    }
}

#[cfg(test)]
mod p1_4_freshness_tests {
    //! P1-4 扫描新鲜度契约（真临时目录 → 真扫描入口 → 真 DB，非单点 mock）：
    //!  - 完整扫描必须发现「父目录 mtime 不变」的**就地文件变更**（同 size、内容变），推进
    //!    `source_revision`，并使旧缩略图/派生/AI/人脸/去重结果全部失效；
    //!  - 携带旧 `source_revision` 的迟到写（缩略图回写、去重摘要）必须在真库上被拒绝，
    //!    且换当前代次即放行（拒绝来自代次而非其它字段被写坏）；
    //!  - 上一轮未成功收尾（取消/中断）后，下一轮即使请求 quick 也强制全量（F-001）。
    use super::*;
    use std::io::Write;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("scrollery_p14_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 内存库 + scan_root(id=1) 指向 `root`。FK 关闭：本域不构造卷/父目录链（与既有扫描测试同款）。
    fn db_for_root(root: &Path) -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'R')",
            params![root.to_string_lossy()],
        )
        .unwrap();
        c
    }

    /// 生产同款 Channel：回调即丢（本域只断言 DB 终态）。
    fn silent_channel() -> Channel<ScanChannelPayload> {
        Channel::new(|_| Ok(()))
    }

    /// 走生产入口 `run_fast_scan`（无 generation 闸门的真实扫描路径）。
    fn scan(writer: &Mutex<Connection>, root: &Path, run_id: &str, quick: bool) -> Result<u64> {
        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        let channel = silent_channel();
        run_fast_scan(
            writer,
            1,
            run_id,
            &root.to_string_lossy(),
            &catalog,
            &channel,
            &cancel,
            &|| {},
            quick,
        )
    }

    fn config_value(conn: &Connection, key: &str) -> Option<String> {
        conn.query_row(
            "SELECT value FROM app_config WHERE key=?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
    }

    fn item_count(writer: &Mutex<Connection>) -> i64 {
        writer
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM media_items", [], |r| r.get(0))
            .unwrap()
    }

    fn scalar_i64(conn: &Connection, sql: &str, id: i64) -> i64 {
        conn.query_row(sql, params![id], |r| r.get(0)).unwrap()
    }

    fn move_mixed_and_rescan(quick: bool) {
        let tmp = tempfile::Builder::new()
            .prefix("scrollery-sync-")
            .tempdir()
            .unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("source/folder/nested")).unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("source/loose.psd"), b"loose").unwrap();
        std::fs::write(root.join("source/folder/nested/image.jpg"), b"nested").unwrap();
        let writer = Mutex::new(db_for_root(root));
        assert_eq!(scan(&writer, root, "baseline", false).unwrap(), 2);
        let conn = writer.lock().unwrap();
        conn.execute_batch(
            "UPDATE media_items SET is_favorited=1,rating=5;
            INSERT INTO tags(id,name) VALUES(1,'keep');
            INSERT INTO item_tags(item_id,tag_id) SELECT id,1 FROM media_items;
            INSERT INTO albums(id,name) VALUES(100,'keep');
            INSERT INTO album_items(album_id,item_id) SELECT 100,id FROM media_items;",
        )
        .unwrap();
        let original: Vec<(i64, String)> = conn
            .prepare("SELECT id,file_name FROM media_items ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM exotic_tasks", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1,
            "迁移前已按 PSD 格式播种插件任务"
        );
        drop(conn);
        std::fs::rename(root.join("source/folder"), root.join("target/folder")).unwrap();
        std::fs::rename(
            root.join("source/loose.psd"),
            root.join("target/renamed.txt"),
        )
        .unwrap();
        assert_eq!(
            scan(&writer, root, "moved", quick).unwrap(),
            0,
            "移动复用原资产"
        );
        let conn = writer.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM media_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        for (id, old_name) in original {
            let (rel,name,favorite,rating,availability,volume_path):(String,String,i64,i64,String,String)=conn.query_row("SELECT d.rel_path,m.file_name,m.is_favorited,m.rating,m.availability,m.volume_relative_path FROM media_items m JOIN directories d ON d.id=m.directory_id WHERE m.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap();
            let expected = if old_name == "loose.psd" {
                ("target", "renamed.txt")
            } else {
                ("target/folder/nested", "image.jpg")
            };
            assert_eq!((rel.as_str(), name.as_str()), expected);
            assert_eq!((favorite, rating, availability.as_str()), (1, 5, "online"));
            assert_eq!(volume_path, format!("{rel}/{name}"));
            if old_name == "loose.psd" {
                let classification: (String, String) = conn
                    .query_row(
                        "SELECT file_format,media_type FROM media_items WHERE id=?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(classification, ("txt".into(), "document".into()));
                assert_eq!(
                    scalar_i64(
                        &conn,
                        "SELECT count(*) FROM exotic_tasks WHERE item_id=?1",
                        id
                    ),
                    0,
                    "旧格式的任务不能继续阻塞当前格式的派生"
                );
            }
            assert_eq!(
                scalar_i64(&conn, "SELECT count(*) FROM item_tags WHERE item_id=?1", id),
                1
            );
            assert_eq!(
                scalar_i64(
                    &conn,
                    "SELECT count(*) FROM album_items WHERE item_id=?1",
                    id
                ),
                1
            );
        }
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM directories WHERE rel_path LIKE 'source/folder%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn full_rescan_preserves_identity_after_mixed_external_moves() {
        move_mixed_and_rescan(false);
    }

    #[test]
    fn quick_rescan_preserves_identity_after_mixed_external_moves() {
        move_mixed_and_rescan(true);
    }

    /// 完整扫描检出「目录 mtime 不变」的就地编辑，并让旧缩略图/派生/AI/人脸/去重结果失效，
    /// 同时拒绝携带旧代次的迟到写。quick 作为对照漏掉同一处变更（明示的设计取舍）。
    #[test]
    fn full_scan_detects_in_place_edit_and_rejects_stale_derived_writes() {
        use crate::db::queries as q;

        let root = tmp_root("inplace");
        let file = root.join("a.jpg");
        // 16B ≤ 64MB → content_fingerprint 走**全文** sha256（不带抽样漏检边界）。
        std::fs::write(&file, b"AAAAAAAAAAAAAAAA").unwrap();
        let writer = Mutex::new(db_for_root(&root));

        // ── 第一轮全量：入库 + 建立目录 mtime 基线 ────────────────────────────────
        assert_eq!(scan(&writer, &root, "run-1", false).unwrap(), 1);
        let (item_id, cache_key, baseline_mtime) = {
            let c = writer.lock().unwrap();
            let (id, revision, cache_key): (i64, i64, i64) = c
                .query_row(
                    "SELECT id, source_revision, cache_key FROM media_items WHERE file_name='a.jpg'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(revision, 1, "新项从 source_revision=1 起算");
            let mtime: i64 = c
                .query_row(
                    "SELECT mtime FROM directories WHERE root_id=1 AND rel_path=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            (id, cache_key, mtime)
        };
        assert_eq!(
            read_dir_mtime(&root),
            Some(baseline_mtime),
            "首轮全量应写入当前目录 mtime 基线（quick 剪枝的前置条件）"
        );

        // ── 就地改写：同 size、内容不同；文件 mtime 前移，目录 mtime 还原为基线值 ──
        // 「目录 mtime ≡ 基线」正是 quick 的剪枝判据；本用例要证明完整扫描不据它跳过。
        let dir_baseline =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&root).unwrap());
        {
            let mut f = std::fs::File::create(&file).unwrap();
            f.write_all(b"BBBBBBBBBBBBBBBB").unwrap();
            f.sync_all().unwrap();
        }
        filetime::set_file_mtime(
            &file,
            filetime::FileTime::from_unix_time(dir_baseline.unix_seconds() + 7, 0),
        )
        .unwrap();
        filetime::set_file_mtime(&root, dir_baseline).unwrap();
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&root).unwrap()),
            dir_baseline,
            "前置条件：就地改写后父目录 mtime 必须与基线完全一致"
        );
        assert_eq!(read_dir_mtime(&root), Some(baseline_mtime));

        // ── 对照（设计取舍，非缺陷）：目录 mtime 未变 → quick 剪掉本目录 → 检出不到 ──
        assert_eq!(scan(&writer, &root, "run-quick", true).unwrap(), 0);
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT source_revision FROM media_items WHERE id=?1",
                    item_id
                ),
                1,
                "quick 剪枝目录内的就地编辑按设计不检出（完整扫描兜底）"
            );
        }

        // ── 播种「旧版本（rev=1）」派生结果：缩略图/EXIF/派生任务/AI/人脸/去重摘要 ──
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                q::update_thumb_result_if_current(
                    &c,
                    item_id,
                    1,
                    cache_key,
                    1,
                    Some("thumb/old.webp"),
                    None
                )
                .unwrap(),
                1,
                "前置条件：rev=1 + 当前 cache_key 的缩略图回写应当生效"
            );
            c.execute_batch(&format!(
                "INSERT INTO image_meta (item_id, orientation) VALUES ({item_id}, 6);
                 INSERT INTO media_derivations (item_id, kind, status, payload_path)
                    VALUES ({item_id}, 'video_cover', 2, 'deriv/old.webp');
                 INSERT INTO ai_embeddings (item_id, model_name, embedding)
                    VALUES ({item_id}, 'clip-test', x'0001');
                 INSERT INTO faces (item_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h, det_score, embedding)
                    VALUES ({item_id}, 'yunet-sface', 0.1, 0.1, 0.2, 0.2, 0.9, x'0001');
                 INSERT INTO face_coverage (item_id, model_name) VALUES ({item_id}, 'yunet-sface');
                 UPDATE media_items SET ai_status=2, face_status=2 WHERE id={item_id};"
            ))
            .unwrap();
            assert!(q::begin_dedup_run(&c, 1, true).unwrap());
            let (size, mtime_ns): (i64, i64) = c
                .query_row(
                    "SELECT file_size, file_mtime_ns FROM media_items WHERE id=?1",
                    params![item_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert!(
                q::write_dedup_index_if_generation_current(
                    &c,
                    &q::DedupIndexUpdate {
                        item_id,
                        source_revision: 1,
                        file_size: size,
                        file_mtime_ns: Some(mtime_ns),
                        hash_version: 1,
                        quick_digest: Some(b"old"),
                        exact_digest: Some(b"old"),
                        unit_digest: Some(b"old"),
                        unit_size: Some(size),
                        physical_key: None,
                        status: "ready",
                        error_code: None,
                    },
                    1,
                )
                .unwrap(),
                "前置条件：rev=1 的去重摘要应当写入"
            );
        }

        // ── 第二轮完整扫描：检出就地变更 → 推进代次 + 全链失效 ───────────────────
        assert_eq!(
            scan(&writer, &root, "run-2", false).unwrap(),
            0,
            "就地编辑是原地更新，不是新增"
        );
        let c = writer.lock().unwrap();
        #[allow(clippy::type_complexity)]
        let (new_revision, thumb_status, thumb_path, ai_status, face_status, new_cache_key): (
            i64,
            i64,
            Option<String>,
            i64,
            i64,
            i64,
        ) = c
            .query_row(
                "SELECT source_revision, thumb_status, thumb_path, ai_status, face_status, cache_key
                   FROM media_items WHERE id=?1",
                params![item_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            new_revision, 2,
            "同 size 内容变 → SourceChanged 必须推进 source_revision"
        );
        assert_ne!(
            new_cache_key, cache_key,
            "换内容必须换 cache_key（含纳秒 mtime）"
        );
        assert_eq!(thumb_status, 0);
        assert!(thumb_path.is_none());
        assert_eq!(ai_status, 0);
        assert_eq!(face_status, 0);
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM image_meta WHERE item_id=?1",
                item_id
            ),
            0,
            "旧 EXIF 必须失效重算"
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM ai_embeddings WHERE item_id=?1",
                item_id
            ),
            0
        );
        assert_eq!(
            scalar_i64(&c, "SELECT COUNT(*) FROM faces WHERE item_id=?1", item_id),
            0
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM face_coverage WHERE item_id=?1",
                item_id
            ),
            0
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT COUNT(*) FROM dedup_index WHERE item_id=?1",
                item_id
            ),
            0,
            "旧代次的精确摘要必须整行作废"
        );
        let (deriv_status, deriv_path): (i64, Option<String>) = c
            .query_row(
                "SELECT status, payload_path FROM media_derivations
                  WHERE item_id=?1 AND kind='video_cover'",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(deriv_status, 0, "源变后派生任务必须退回 pending");
        assert!(deriv_path.is_none(), "旧派生产物路径必须清空");
        assert_eq!(
            config_value(&c, &baseline_incomplete_key(1)),
            Some("0".to_string()),
            "成功收尾应清零未完成标记"
        );

        // ── 拒旧版本结果：携带旧 source_revision 的迟到写一律不得落库 ─────────────
        assert_eq!(
            q::update_thumb_result_if_current(
                &c,
                item_id,
                1,
                cache_key,
                1,
                Some("thumb/late.webp"),
                None
            )
            .unwrap(),
            0,
            "旧代次缩略图回写必须被拒绝"
        );
        let (thumb_status, thumb_path): (i64, Option<String>) = c
            .query_row(
                "SELECT thumb_status, thumb_path FROM media_items WHERE id=?1",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(thumb_status, 0);
        assert!(thumb_path.is_none(), "被拒绝的回写不得留下产物路径");

        // 唯一变量是代次：size/mtime_ns 用**当前**值，旧代次仍必须被拒；换当前代次即放行。
        let (size_now, mtime_ns_now): (i64, i64) = c
            .query_row(
                "SELECT file_size, file_mtime_ns FROM media_items WHERE id=?1",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            !q::dedup_write_batch_current(
                &c,
                1,
                &[q::DedupWriteCheck {
                    item_id,
                    source_revision: 1,
                    file_size: size_now,
                    file_mtime_ns: Some(mtime_ns_now),
                    companion_of: None,
                    physical_key: None,
                }]
            )
            .unwrap(),
            "旧代次去重摘要写回必须被拒绝"
        );
        assert!(
            q::dedup_write_batch_current(
                &c,
                1,
                &[q::DedupWriteCheck {
                    item_id,
                    source_revision: 2,
                    file_size: size_now,
                    file_mtime_ns: Some(mtime_ns_now),
                    companion_of: None,
                    physical_key: None,
                }]
            )
            .unwrap(),
            "对照组：同快照换当前代次应放行（证明拒绝来自代次）"
        );
        assert_eq!(
            q::update_thumb_result_if_current(
                &c,
                item_id,
                2,
                new_cache_key,
                1,
                Some("thumb/new.webp"),
                None
            )
            .unwrap(),
            1,
            "对照组：当前代次 + 当前 cache_key 的回写应当生效"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 同大小源变更的定案须整体提交；末尾 Exotic 失效失败时保留旧基线，使重扫还能重试。
    #[test]
    fn suspect_change_failure_rolls_back_source_and_derivations_for_retry() {
        let root = tmp_root("suspect-rollback");
        let file = root.join("a.psd");
        std::fs::write(&file, b"AAAAAAAAAAAAAAAA").unwrap();
        filetime::set_file_mtime(&file, filetime::FileTime::from_unix_time(1700000000, 0)).unwrap();
        let writer = Mutex::new(db_for_root(&root));
        assert_eq!(scan(&writer, &root, "baseline", false).unwrap(), 1);
        let item_id: i64 = writer
            .lock()
            .unwrap()
            .query_row("SELECT id FROM media_items", [], |r| r.get(0))
            .unwrap();
        let source_state = |c: &Connection| -> (i64, i64, i64) {
            c.query_row(
                "SELECT file_mtime_ns, source_revision, cache_key FROM media_items WHERE id=?1",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        let before = {
            let c = writer.lock().unwrap();
            c.execute(
                "INSERT INTO image_meta (item_id, orientation) VALUES (?1, 6)",
                params![item_id],
            )
            .unwrap();
            c.execute(
                "INSERT INTO dedup_index (item_id, source_revision, hash_version, status, checked_at)
                 VALUES (?1, 1, 1, 'done', 1)",
                params![item_id],
            )
            .unwrap();
            c.execute(
                "UPDATE media_items SET thumb_status=1, thumb_path='old.webp' WHERE id=?1",
                params![item_id],
            )
            .unwrap();
            assert!(
                c.execute(
                    "UPDATE exotic_tasks SET status=2, output_path='old.webp' WHERE item_id=?1",
                    params![item_id],
                )
                .unwrap()
                    > 0
            );
            c.execute_batch(
                "CREATE TRIGGER t_block_exotic_invalidation BEFORE UPDATE ON exotic_tasks
                 BEGIN SELECT RAISE(ABORT, 'blocked-exotic-invalidation'); END;",
            )
            .unwrap();
            source_state(&c)
        };

        std::fs::write(&file, b"BBBBBBBBBBBBBBBB").unwrap();
        filetime::set_file_mtime(&file, filetime::FileTime::from_unix_time(1700000001, 0)).unwrap();
        assert!(scan(&writer, &root, "failed-invalidation", false).is_err());
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                source_state(&c),
                before,
                "末尾失败不能提前提交新 mtime/源代次"
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT count(*) FROM image_meta WHERE item_id=?1",
                    item_id
                ),
                1
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT count(*) FROM dedup_index WHERE item_id=?1",
                    item_id
                ),
                1
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT status FROM exotic_tasks WHERE item_id=?1",
                    item_id
                ),
                2
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT thumb_status FROM media_items WHERE id=?1",
                    item_id
                ),
                1
            );
            assert_eq!(
                config_value(&c, &baseline_incomplete_key(1)).as_deref(),
                Some("1")
            );
            c.execute_batch("DROP TRIGGER t_block_exotic_invalidation;")
                .unwrap();
        }

        // 同一真实文件再次扫描，必须重新完成失效，不能因上次已写新 mtime 而永久走 Unchanged。
        assert_eq!(scan(&writer, &root, "retry-invalidation", true).unwrap(), 0);
        {
            let c = writer.lock().unwrap();
            let after = source_state(&c);
            assert_ne!(after.0, before.0);
            assert_eq!(after.1, before.1 + 1);
            assert_ne!(after.2, before.2);
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT count(*) FROM image_meta WHERE item_id=?1",
                    item_id
                ),
                0
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT count(*) FROM dedup_index WHERE item_id=?1",
                    item_id
                ),
                0
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT status FROM exotic_tasks WHERE item_id=?1",
                    item_id
                ),
                0
            );
            assert_eq!(
                scalar_i64(
                    &c,
                    "SELECT thumb_status FROM media_items WHERE id=?1",
                    item_id
                ),
                0
            );
            assert_eq!(
                config_value(&c, &baseline_incomplete_key(1)).as_deref(),
                Some("0")
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// F-001：上一轮未成功收尾（持久标记置位）→ 本轮即使请求 quick 也强制全量。
    /// 场景即残留态本身：目录 mtime 基线已「治愈」为当前值，而该目录的在盘文件尚未入库
    /// （中断前基线先写、文件后入的窗口）。quick 单独跑必然剪掉它 → 漏扫；标记兜底 → 全量必扫到。
    #[test]
    fn interrupted_scan_forces_next_quick_to_full() {
        let root = tmp_root("interrupted");
        std::fs::write(root.join("a.jpg"), b"xxxxxxxx").unwrap();
        let writer = Mutex::new(db_for_root(&root));
        {
            let c = writer.lock().unwrap();
            let mtime = read_dir_mtime(&root).expect("临时目录应可读 mtime");
            c.execute(
                "INSERT INTO directories (root_id, rel_path, name, mtime)
                   VALUES (1, '', 'R', ?1)",
                params![mtime],
            )
            .unwrap();
        }

        // quick 单独跑：基线与目录 mtime 一致 → 剪枝 → 盘上文件不入库（残留态确实会漏）。
        assert_eq!(scan(&writer, &root, "run-pruned", true).unwrap(), 0);
        assert_eq!(item_count(&writer), 0);

        // 模拟「上一轮中断」留下的持久标记（生产由扫描入口在遍历前自行置位；此处直写等价态）。
        writer
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, '1')",
                params![baseline_incomplete_key(1)],
            )
            .unwrap();

        // 同一请求（quick=true）必须降级为全量 → 在盘文件入库，且成功收尾后标记清零。
        assert_eq!(scan(&writer, &root, "run-forced-full", true).unwrap(), 1);
        assert_eq!(
            item_count(&writer),
            1,
            "中断后下一轮必须强制全量，不得再据被治愈的基线剪枝"
        );
        {
            let c = writer.lock().unwrap();
            assert_eq!(
                config_value(&c, &baseline_incomplete_key(1)),
                Some("0".to_string()),
                "成功收尾应清零标记"
            );
        }

        // 不永久降级：基线已重建，下一轮 quick 恢复剪枝（在盘内容与库已一致）。
        assert_eq!(scan(&writer, &root, "run-quick-after", true).unwrap(), 0);
        assert_eq!(item_count(&writer), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 收尾落库是**一次事务**：目录计数 / 根状态 / dirty 标记要么一起生效、要么一起不留痕。
    ///
    /// 两处故障注入（trigger RAISE 打断写库）：① 目录计数 UPDATE 就失败 → 三者都未写；
    /// ② 计数已写、根状态 UPDATE 才失败 → 已写入的计数必须一起回滚（三者各自 autocommit 时它会留在 12）。
    /// 两种情况下 dirty 都必须仍置位，使下一轮 quick 强制全量重建基线。
    #[test]
    fn finalize_write_failure_rolls_back_counts_status_and_dirty() {
        let root = tmp_root("finalize-rollback");
        std::fs::write(root.join("a.jpg"), b"x").unwrap();
        let writer = Mutex::new(db_for_root(&root));
        assert_eq!(scan(&writer, &root, "baseline", false).unwrap(), 1);

        let counts = {
            let c = writer.lock().unwrap();
            let dir_id: i64 = c
                .query_row(
                    "SELECT id FROM directories WHERE root_id=1 AND rel_path=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            // 收尾前哨兵状态：计数 99、根状态 scanning、未写过 last_scan_at、dirty 置位。
            c.execute(
                "UPDATE directories SET media_count=99 WHERE id=?1",
                params![dir_id],
            )
            .unwrap();
            c.execute(
                "UPDATE scan_roots SET scan_status='scanning', scan_progress=3,
                        total_files=3, last_scan_at=NULL WHERE id=?1",
                params![1i64],
            )
            .unwrap();
            set_baseline_incomplete(&c, 1).unwrap();
            std::collections::HashMap::from([(dir_id, 12i64)])
        };

        let c = writer.lock().unwrap();
        let dir_id = *counts.keys().next().unwrap();
        // 收尾失败后必须原样保持收尾前哨兵状态 —— 两种故障注入共用同一组断言。
        let assert_untouched = |c: &Connection| {
            assert_eq!(
                scalar_i64(c, "SELECT media_count FROM directories WHERE id=?1", dir_id),
                99,
                "目录计数写入必须整体回滚，不得留下部分提交"
            );
            let (status, progress, last_scan_at): (String, i64, Option<i64>) = c
                .query_row(
                    "SELECT scan_status, scan_progress, last_scan_at FROM scan_roots WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(status, "scanning", "根状态不得被单独推进为 idle");
            assert_eq!(progress, 3, "根进度不得被单独推进");
            assert!(last_scan_at.is_none(), "收尾未成功不得写 last_scan_at");
            assert_eq!(
                config_value(c, &baseline_incomplete_key(1)).as_deref(),
                Some("1"),
                "收尾失败必须保留 dirty，下一轮强制全量重建基线"
            );
            let availability: String = c
                .query_row("SELECT availability FROM media_items", [], |r| r.get(0))
                .unwrap();
            assert_eq!(availability, "online", "既有项不得被标缺失");
        };

        // ① 目录计数 UPDATE 就失败 → 三者都未写。
        c.execute_batch(
            "CREATE TRIGGER t_block_dir_count BEFORE UPDATE OF media_count ON directories
             BEGIN SELECT RAISE(ABORT, 'blocked-dir-count'); END;",
        )
        .unwrap();
        init_seen_table_for_run(&c, 1, 7).unwrap();
        let err = finalize_scan_root(&c, 1, 7, true, 0, true, None, &counts, 12).unwrap_err();
        assert!(
            err.to_string().contains("blocked-dir-count"),
            "前置条件：失败必须来自目录计数 UPDATE 的 trigger，而非其它路径: {err}"
        );
        assert_untouched(&c);

        // ② 计数已写、根状态 UPDATE 才失败 → 已写入的计数必须一起回滚。
        c.execute_batch(
            "DROP TRIGGER t_block_dir_count;
             CREATE TRIGGER t_block_root_finish BEFORE UPDATE OF scan_status ON scan_roots
             BEGIN SELECT RAISE(ABORT, 'blocked-root-finish'); END;",
        )
        .unwrap();
        init_seen_table_for_run(&c, 1, 7).unwrap();
        let err = finalize_scan_root(&c, 1, 7, true, 0, true, None, &counts, 12).unwrap_err();
        assert!(
            err.to_string().contains("blocked-root-finish"),
            "前置条件：失败必须来自根状态 UPDATE 的 trigger: {err}"
        );
        assert_untouched(&c);

        c.execute_batch("DROP TRIGGER t_block_root_finish;")
            .unwrap();
        // 失败只是回滚，不是永久损伤：去掉故障后同一轮收尾正常落地，三项一起生效。
        init_seen_table_for_run(&c, 1, 7).unwrap();
        assert_eq!(
            finalize_scan_root(&c, 1, 7, true, 0, true, None, &counts, 12).unwrap(),
            0
        );
        assert_eq!(
            scalar_i64(
                &c,
                "SELECT media_count FROM directories WHERE id=?1",
                dir_id
            ),
            12,
            "重试收尾应写回本轮计数"
        );
        assert_eq!(
            config_value(&c, &baseline_incomplete_key(1)).as_deref(),
            Some("0"),
            "重试成功后 dirty 才清零"
        );

        drop(c);
        let _ = std::fs::remove_dir_all(&root);
    }
}
