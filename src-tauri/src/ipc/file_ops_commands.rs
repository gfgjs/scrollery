use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tracing::info;

use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::ipc::dir_move;
pub use crate::ipc::dir_move::DirectoryCopyProgress;
use crate::ipc::scan_commands::add_scan_root;
use crate::state::{AppState, FILE_JOB_DIR_MOVE};
use crate::utils::fs::rename_no_replace;
use crate::utils::path::{resolve_media_path, resolve_within_root_optional};

/// Drop 守卫：把「布局失效 + bump 数据版本」放到 early-return 跳不过的位置。
///
/// 2026-07-06 审查 P1-6：本文件三个批量命令（relocate/copy_db/remove_hard）逐条自动提交，
/// 第 N 项失败时前 N-1 项的删行/改行**已生效**，而原实现把失效放在循环之后——中途 `?` 返回即
/// 跳过 bump，items 取数缓存继续按旧 data_version 命中，画廊持续端出已删/已移项（点击报
/// MediaNotFound），直到下次任意写路径 bump 才恢复。守卫在 Drop 时按 `dirty` 决定失效，
/// 成功与报错路径共用同一收尾。
struct InvalidateOnWrite {
    state: Arc<AppState>,
    /// 至少一条写入已生效（调用方在每次 DB 写成功后置 true）。
    dirty: bool,
}

impl InvalidateOnWrite {
    fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: Arc::clone(state),
            dirty: false,
        }
    }
}

impl Drop for InvalidateOnWrite {
    fn drop(&mut self) {
        if !self.dirty {
            return;
        }
        // Drop 内不可 panic：锁中毒用 into_inner 恢复（保护的是可整体置 None 的纯数据）。
        *self
            .state
            .layout_cache
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.state.bump_data_version();
        // 文件树目录快照一并失效（S 线审查 R-05）：本守卫覆盖的批量命令多数真实改写扫描根内
        // 磁盘，「所有文件」模式的快照会继续端出旧页直到手动刷新。全清而非逐根失效：批量项可
        // 跨多根，从每条 DB 行反查 root_id 只为省几份快照不值当——缓存双维度封顶（≤8 快照），
        // 重建成本 = 下次展开时一次 read_dir+sort。媒体复制也会先发布文件再登记索引，
        // 即使最终提交失败，文件树仍须反映已经落盘的副本。
        self.state.tree_snapshots.clear();
    }
}

#[tauri::command]
pub async fn create_physical_folder(
    app: AppHandle,
    base_path: String,
    folder_name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String> {
    // span 埋点(W1,D-312 info 档:目录创建+扫描根登记,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:create_physical_folder");
    let target_path = if base_path.is_empty() {
        PathBuf::from(&folder_name)
    } else {
        PathBuf::from(&base_path).join(&folder_name)
    };

    let path_str = target_path.to_string_lossy().to_string();
    info!("create_physical_folder: path={}", path_str);

    let created = tokio::fs::create_dir_all(&target_path).await;
    // 失效树快照(R-05):尝试即清,不等成功——create_dir_all 失败前可能已建出中间层目录。
    // 全清成本 ≤8 份快照,见 InvalidateOnWrite::drop 的注。
    state.tree_snapshots.clear();
    created.map_err(|e| AppError::CreateFolder(e.to_string()))?;

    let norm = crate::utils::path::normalize_db_path(&path_str);

    // R1-3：读池查询走 read_blocking。
    let is_within_existing = super::blocking::read_blocking(&state, move |conn| {
        let roots = q::list_scan_roots(conn)?;
        let target_norm = format!("{norm}/");
        Ok(roots.into_iter().any(|r| {
            let r_norm = format!("{}/", r.path);
            target_norm.starts_with(&r_norm)
        }))
    })
    .await?;

    if !is_within_existing {
        // 自动纳入 Scan Roots
        add_scan_root(app.clone(), path_str.clone(), None, None, state.clone()).await?;
    }

    Ok(path_str)
}

/// 一次重定位请求：将媒体项 `id` 移动到目录 `target_dir_id`。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRelocation {
    pub id: i64,
    pub target_dir_id: i64,
}

/// 重定位结果 — 带上原目录，使调用方能构造精确的逆操作以供撤销/重做。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRelocationResult {
    pub id: i64,
    pub from_dir_id: i64,
    pub target_dir_id: i64,
}

/// 批量文件操作的首个失败项；错误沿用 AppError 的稳定码与对外序列化。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFileOperationFailure {
    pub id: i64,
    pub error: AppError,
}

/// 已实际移动的前缀与首个失败项；已在目标目录的无操作项不计入 completed。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRelocationBatchResult {
    pub completed: Vec<MediaRelocationResult>,
    pub failure: Option<MediaFileOperationFailure>,
}

fn relocate_media_file(
    conn: &mut rusqlite::Connection,
    id: i64,
    target_dir_id: i64,
    source: &std::path::Path,
    target: &std::path::Path,
) -> Result<()> {
    // 先在事务中校验唯一键和外键；目标残留索引不能让文件先搬走、随后改库失败。
    let tx = conn.transaction().map_err(AppError::Db)?;
    let cache_key = tx.query_row(
        "SELECT d.rel_path,m.file_name,m.file_mtime,COALESCE(m.file_mtime_ns,0)
         FROM media_items m JOIN directories d ON d.id=?2 WHERE m.id=?1",
        rusqlite::params![id, target_dir_id],
        |row| {
            Ok(crate::utils::hash::compute_cache_key_with_mtime_ns(
                target_dir_id,
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        },
    )?;
    // 腾出的原位置可能立即导入同名同时间的新文件；移动项必须切换缓存身份，不能继续
    // 引用旧位置的派生路径。源内容不变，保留已完成人脸/AI结果及人工确认，仅重排在途任务。
    tx.execute(
        "UPDATE media_items SET directory_id=?1,cache_key=?3,source_revision=source_revision+1,
             thumb_status=0,thumb_path=NULL,thumbhash=NULL,
             ai_status=CASE WHEN ai_status=1 THEN 0 ELSE ai_status END,
             face_status=CASE WHEN face_status=1 THEN 0 ELSE face_status END,
             updated_at=strftime('%s','now') WHERE id=?2",
        rusqlite::params![target_dir_id, id, cache_key],
    )
    .map_err(AppError::Db)?;
    tx.execute("DELETE FROM image_artifacts WHERE item_id=?1", [id])?;
    tx.execute(
        "UPDATE media_derivations SET status=0,payload_path=NULL,error=NULL,
             updated_at=strftime('%s','now') WHERE item_id=?1",
        [id],
    )?;
    q::invalidate_exotic_tasks_for_item(&tx, id)?;
    // 移动失败时事务随返回回滚；这里仅做同卷原子移动，不执行跨卷复制或等待异步任务。
    rename_no_replace(source, target).map_err(|error| {
        tracing::error!(
            media_id = id,
            source = %source.display(),
            target = %target.display(),
            error = %error,
            "relocate_media_items: 文件移动失败"
        );
        AppError::MoveFile(
            "移动文件失败，请检查源文件和目标目录 | Failed to move file; check the source and destination"
                .into(),
        )
    })?;
    if let Err(error) = tx.commit() {
        // commit 失败会回滚索引；文件也须回原处，否则失败项不进入撤销记录却已被搬走。
        // 原位置若已被重新占用，原子拒绝覆盖并保留目标文件，不能为恢复索引丢掉新文件。
        if let Err(restore_error) = rename_no_replace(target, source) {
            tracing::error!(
                media_id = id,
                source = %source.display(),
                target = %target.display(),
                %error,
                %restore_error,
                "移动索引提交失败，恢复文件原位置也失败"
            );
            return Err(AppError::MoveFile(format!(
                "文件已移动到 {}，索引提交失败且无法恢复原位置，请重扫目标目录 | File moved; catalog commit and restore failed; rescan the destination",
                target.display()
            )));
        }
        return Err(AppError::Db(error));
    }
    Ok(())
}

#[cfg(test)]
mod move_file_tests {
    use super::*;

    fn seed_catalog(root: &std::path::Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        seed_catalog_into(&conn, root);
        conn
    }

    fn seed_catalog_into(conn: &rusqlite::Connection, root: &std::path::Path) {
        crate::db::schema::initialize_schema(conn).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'Root')",
            rusqlite::params![root.to_string_lossy()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES
                (10, 1, 'source', 'source'), (20, 1, 'target', 'target');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                file_format, sort_datetime, cache_key)
             VALUES (101, 10, 'asset.jpg', 12, 1, 'jpg', 1, 101);",
        )
        .unwrap();
    }

    fn item_directory(conn: &rusqlite::Connection, id: i64) -> i64 {
        conn.query_row(
            "SELECT directory_id FROM media_items WHERE id=?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn relocate_directory_after_plan(state: &Arc<AppState>, directory_id: i64) {
        let root = {
            let conn = state.db_writer.lock().unwrap();
            let root: String = conn
                .query_row("SELECT path FROM scan_roots WHERE id=1", [], |r| r.get(0))
                .unwrap();
            conn.execute_batch(
                "INSERT INTO directories(id,root_id,rel_path,name) VALUES
                (30,1,'','Root'),(40,1,'parent','parent');
                UPDATE directories SET parent_id=30 WHERE id IN(10,20);",
            )
            .unwrap();
            root
        };
        std::fs::create_dir_all(PathBuf::from(root).join("parent")).unwrap();
        let plan = dir_move::load_plan(&state.db_writer.lock().unwrap(), directory_id, 40).unwrap();
        assert!(state.try_acquire_file_job(FILE_JOB_DIR_MOVE));
        state
            .with_scan_roots_exclusive(&[1], || {
                dir_move::verify_plan_current(&state.db_writer.lock().unwrap(), &plan).unwrap();
                dir_move::execute_move(
                    dir_move::MoveDb::State(state),
                    &plan,
                    &state.thumb_config.read().unwrap().cache_dir,
                )
                .unwrap();
            })
            .unwrap();
        state.release_file_job(FILE_JOB_DIR_MOVE);
    }

    #[test]
    fn relocation_rejects_target_directory_moved_after_path_read() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        std::fs::write(tmp.path().join("source/asset.jpg"), b"preserved asset").unwrap();
        let state = batch_state(tmp.path());
        let outcome = relocate_media_items_with(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
            || relocate_directory_after_plan(&state, 20),
        );
        assert!(outcome.completed.is_empty());
        assert!(matches!(
            outcome.failure.unwrap().error,
            AppError::InvalidMove(_)
        ));
        assert!(
            !tmp.path().join("target").exists(),
            "过期计划不得重建旧目录"
        );
        assert_eq!(
            std::fs::read(tmp.path().join("source/asset.jpg")).unwrap(),
            b"preserved asset"
        );
        assert_eq!(item_directory(&state.db_writer.lock().unwrap(), 101), 10);
        let retried = relocate_media_items_blocking(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(retried.failure.is_none());
        assert_eq!(retried.completed.len(), 1);
        assert_eq!(
            std::fs::read(tmp.path().join("parent/target/asset.jpg")).unwrap(),
            b"preserved asset"
        );
    }

    #[test]
    fn relocation_rejects_source_directory_moved_after_path_read() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        std::fs::write(tmp.path().join("source/asset.jpg"), b"preserved asset").unwrap();
        let state = batch_state(tmp.path());
        let outcome = relocate_media_items_with(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
            || relocate_directory_after_plan(&state, 10),
        );
        assert!(outcome.completed.is_empty());
        assert!(matches!(
            outcome.failure.unwrap().error,
            AppError::InvalidMove(_)
        ));
        assert_eq!(
            std::fs::read(tmp.path().join("parent/source/asset.jpg")).unwrap(),
            b"preserved asset"
        );
        assert!(!tmp.path().join("target/asset.jpg").exists());
        assert_eq!(item_directory(&state.db_writer.lock().unwrap(), 101), 10);
    }

    #[test]
    fn late_image_write_cannot_replace_reoccupied_path_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        std::fs::create_dir_all(tmp.path().join("cache")).unwrap();
        std::fs::write(tmp.path().join("source/asset.jpg"), b"old source").unwrap();
        let state = batch_state(tmp.path());
        let config = state.thumb_config.read().unwrap().clone();
        let old_key =
            crate::utils::hash::compute_cache_key_with_mtime_ns(10, "source", "asset.jpg", 1, 0);
        state
            .db_writer
            .lock()
            .unwrap()
            .execute(
                "UPDATE media_items SET cache_key=?1,rating=5 WHERE id=101",
                [old_key],
            )
            .unwrap();
        let old_display = config.display_path(101, 1, old_key);
        let old_ai = config.ai_path(101, 1, old_key);
        state.db_writer.lock().unwrap().execute_batch(
            "INSERT INTO tags(id,name) VALUES(1,'人工标签');
             INSERT INTO item_tags(item_id,tag_id) VALUES(101,1);
             INSERT INTO persons(id,name,is_named) VALUES(1,'确认人物',1);
             INSERT INTO faces(id,item_id,person_id,model_name,bbox_x,bbox_y,bbox_w,bbox_h,det_score,embedding,is_confirmed)
                VALUES(1,101,1,'face',0,0,1,1,1,X'0102',1);"
        ).unwrap();
        // 编码后的旧任务停在写盘之前；主线程先移动并重新占位、发布新任务。
        let (release, pending) = std::sync::mpsc::channel();
        let old_state = Arc::clone(&state);
        let old_config = config.clone();
        let old_display_write = old_display.clone();
        let old_ai_write = old_ai.clone();
        let old_writer = std::thread::spawn(move || {
            pending.recv().unwrap();
            old_state
                .artifact_writer
                .write(
                    &old_config.cache_dir,
                    &format!("thumbnails/{old_display_write}"),
                    b"OLD_RED_ENCODED",
                )
                .unwrap();
            old_state
                .artifact_writer
                .write(&old_config.cache_dir, &old_ai_write, b"OLD_RED_AI")
                .unwrap();
        });
        let moved = relocate_media_items_blocking(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(moved.failure.is_none());
        std::fs::write(tmp.path().join("source/asset.jpg"), b"new source").unwrap();
        let new_id = {
            let mut conn = state.db_writer.lock().unwrap();
            let tx = conn.transaction().unwrap();
            let id = q::duplicate_media_item_into_dir(
                &tx,
                101,
                10,
                &q::MediaCopyFileStat {
                    file_size: 10,
                    file_mtime: 1,
                    file_mtime_ns: 0,
                },
            )
            .unwrap();
            tx.commit().unwrap();
            id
        };
        let new_display = config.display_path(new_id, 1, old_key);
        let new_ai = config.ai_path(new_id, 1, old_key);
        state
            .artifact_writer
            .write(
                &config.cache_dir,
                &format!("thumbnails/{new_display}"),
                b"NEW_BLUE_ENCODED",
            )
            .unwrap();
        state
            .artifact_writer
            .write(&config.cache_dir, &new_ai, b"NEW_BLUE_AI")
            .unwrap();
        let display = q::ImageArtifact {
            item_id: new_id,
            source_revision: 1,
            cache_key: old_key,
            kind: "display".into(),
            fingerprint: config.display_fingerprint(),
            status: 1,
            path: Some(new_display.clone()),
            thumbhash: None,
        };
        let ai = q::ImageArtifact {
            kind: "ai".into(),
            fingerprint: config.ai_fingerprint(),
            path: Some(new_ai.clone()),
            ..display.clone()
        };
        let published = q::publish_image_artifacts(
            &mut state.db_writer.lock().unwrap(),
            &[display.clone(), ai.clone()],
            true,
        )
        .unwrap();
        assert!(published[0].accepted && published[1].accepted);
        release.send(()).unwrap();
        old_writer.join().unwrap();
        let stale = q::publish_image_artifacts(
            &mut state.db_writer.lock().unwrap(),
            &[
                q::ImageArtifact {
                    item_id: 101,
                    path: Some(old_display),
                    ..display
                },
                q::ImageArtifact {
                    item_id: 101,
                    path: Some(old_ai),
                    ..ai
                },
            ],
            true,
        )
        .unwrap();
        assert!(!stale[0].accepted && !stale[1].accepted);
        assert_eq!(
            std::fs::read(config.cache_dir.join("thumbnails").join(new_display)).unwrap(),
            b"NEW_BLUE_ENCODED"
        );
        assert_eq!(
            std::fs::read(config.cache_dir.join(new_ai)).unwrap(),
            b"NEW_BLUE_AI"
        );
        assert_eq!(
            state
                .db_writer
                .lock()
                .unwrap()
                .query_row("SELECT rating FROM media_items WHERE id=101", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            5
        );
        let conn = state.db_writer.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT tag_id FROM item_tags WHERE item_id=101", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT person_id,is_confirmed FROM faces WHERE item_id=101",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (1, 1)
        );
    }

    #[test]
    fn move_commit_failure_restores_source_and_cache_identity() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        std::fs::write(&source, b"preserved asset").unwrap();
        let mut conn = seed_catalog(temp.path());
        // 更新阶段产生延迟外键违规，使错误明确落在文件移动之后的提交阶段。
        conn.execute_batch(
            "CREATE TABLE commit_failure(item_id INTEGER REFERENCES media_items(id)
                DEFERRABLE INITIALLY DEFERRED);
             CREATE TRIGGER fail_move_commit AFTER UPDATE OF directory_id ON media_items
                BEGIN INSERT INTO commit_failure VALUES(999999); END;",
        )
        .unwrap();

        let error = relocate_media_file(&mut conn, 101, 20, &source, &target).unwrap_err();
        assert!(matches!(error, AppError::Db(_)));
        assert_eq!(std::fs::read(&source).unwrap(), b"preserved asset");
        assert!(!target.exists(), "提交失败须恢复文件原位置");
        assert_eq!(item_directory(&conn, 101), 10);
        assert_eq!(
            conn.query_row(
                "SELECT cache_key,source_revision FROM media_items WHERE id=101",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            )
            .unwrap(),
            (101, 1)
        );
        conn.execute_batch("DROP TRIGGER fail_move_commit;")
            .unwrap();
        relocate_media_file(&mut conn, 101, 20, &source, &target).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"preserved asset");
        assert_eq!(item_directory(&conn, 101), 20);
    }

    #[test]
    fn move_commit_failure_preserves_reoccupied_source_and_reports_destination() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let database = temp.path().join("catalog.db");
        std::fs::write(&source, b"preserved moved asset").unwrap();
        let mut conn = rusqlite::Connection::open(&database).unwrap();
        seed_catalog_into(&conn, temp.path());
        conn.busy_timeout(std::time::Duration::from_millis(500))
            .unwrap();
        let reader = rusqlite::Connection::open(&database).unwrap();
        // rollback journal 的读事务允许 UPDATE 和文件移动，但阻止写事务 COMMIT。
        reader
            .execute_batch("BEGIN; SELECT id FROM media_items;")
            .unwrap();
        let source_again = source.clone();
        let published = target.clone();
        let replacer = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !published.exists() {
                assert!(std::time::Instant::now() < deadline, "文件移动未发生");
                std::thread::yield_now();
            }
            std::fs::write(source_again, b"new source occupant").unwrap();
        });

        let error = relocate_media_file(&mut conn, 101, 20, &source, &target).unwrap_err();
        replacer.join().unwrap();
        reader.execute_batch("ROLLBACK;").unwrap();
        let AppError::MoveFile(message) = error else {
            panic!("恢复受阻须报告文件实际位置: {error:?}");
        };
        assert!(message.contains("重扫目标目录"));
        assert!(message.contains(target.to_string_lossy().as_ref()));
        assert_eq!(std::fs::read(&source).unwrap(), b"new source occupant");
        assert_eq!(std::fs::read(&target).unwrap(), b"preserved moved asset");
        assert_eq!(item_directory(&conn, 101), 10);
    }

    #[test]
    fn move_rejects_target_created_after_precheck_and_preserves_both_assets() {
        let tmp = tempfile::Builder::new()
            .prefix("scrollery-move-regression-")
            .tempdir()
            .unwrap()
            .keep();
        let parent = tmp.join("nested".repeat(22));
        let directory = parent.join("deeper".repeat(22));
        let source_dir = directory.join("source");
        let target_dir = directory.join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source = source_dir.join("asset.jpg");
        let target = target_dir.join("asset.jpg");
        let mut conn = seed_catalog(&directory);
        std::fs::write(&source, b"source asset").unwrap();
        assert!(!target.try_exists().unwrap());
        // 检查通过后，另一次移动或外部程序先占用了目标文件名。
        std::fs::write(&target, b"existing target asset").unwrap();
        assert!(relocate_media_file(&mut conn, 101, 20, &source, &target).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT cache_key,source_revision FROM media_items WHERE id=101",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (101, 1),
            "文件移动失败后缓存身份和代次必须一起回滚"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"source asset");
        assert_eq!(std::fs::read(&target).unwrap(), b"existing target asset");
        assert_eq!(item_directory(&conn, 101), 10);

        // 用户明确腾空目标后，同一路径仍应完成正常同卷移动。
        std::fs::remove_file(&target).unwrap();
        relocate_media_file(&mut conn, 101, 20, &source, &target).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"source asset");
        assert_eq!(item_directory(&conn, 101), 20);
        std::fs::remove_file(&target).unwrap();
        std::fs::remove_dir(&source_dir).unwrap();
        std::fs::remove_dir(&target_dir).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        std::fs::remove_dir(&parent).unwrap();
        std::fs::remove_dir(&tmp).unwrap();
    }

    #[test]
    fn move_rejects_stale_target_index_before_moving_source() {
        let tmp = tempfile::Builder::new()
            .prefix("scrollery-move-index-regression-")
            .tempdir()
            .unwrap()
            .keep();
        let source_dir = tmp.join("source");
        let target_dir = tmp.join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source = source_dir.join("asset.jpg");
        let target = target_dir.join("asset.jpg");
        let mut conn = seed_catalog(&tmp);
        // 外部删除文件后，目标目录中的索引行可能仍在；磁盘冲突检查无法发现它。
        conn.execute_batch(
            "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                file_format, sort_datetime, cache_key)
             VALUES (102, 20, 'asset.jpg', 12, 1, 'jpg', 1, 102);",
        )
        .unwrap();
        std::fs::write(&source, b"source asset").unwrap();
        assert!(!target.exists());

        let error = relocate_media_file(&mut conn, 101, 20, &source, &target).unwrap_err();
        assert!(matches!(error, AppError::Db(_)));
        assert!(source.exists(), "索引拒绝移动时，源文件必须仍在原处");
        assert_eq!(std::fs::read(&source).unwrap(), b"source asset");
        assert!(!target.exists());
        assert_eq!(item_directory(&conn, 101), 10);
        assert_eq!(item_directory(&conn, 102), 20);

        std::fs::remove_file(&source).unwrap();
        std::fs::remove_dir(&source_dir).unwrap();
        std::fs::remove_dir(&target_dir).unwrap();
        std::fs::remove_dir(&tmp).unwrap();
    }

    fn batch_state(tmp: &std::path::Path) -> Arc<AppState> {
        let db_path = tmp.join("test.db");
        let writer = crate::db::create_write_connection(&db_path).unwrap();
        {
            let conn = writer.lock().unwrap();
            seed_catalog_into(&conn, tmp);
            conn.execute_batch(
                "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                    file_format, sort_datetime, cache_key) VALUES
                 (100, 20, 'already.jpg', 13, 1, 'jpg', 1, 100),
                 (102, 10, 'blocked.jpg', 7, 1, 'jpg', 1, 102),
                 (103, 10, 'later.jpg', 7, 1, 'jpg', 1, 103),
                 (104, 20, 'blocked.jpg', 7, 1, 'jpg', 1, 104);",
            )
            .unwrap();
        }
        let pool = crate::db::create_read_pool(&db_path, 1).unwrap();
        let (config, _) =
            crate::config::ConfigManager::load_or_init(tmp.join("config.toml")).unwrap();
        let (log_writer, _log_guard) = tracing_appender::non_blocking(std::io::sink());
        Arc::new(AppState::new(
            writer,
            pool,
            Arc::new(config),
            tmp.join("cache"),
            tmp.join("logs"),
            crate::logging::LogRingBuffer::new(Arc::from("move-batch-test"), 1),
            log_writer.error_counter(),
            tmp.to_path_buf(),
            tmp.join("exotic"),
            256,
            0,
            "balanced".into(),
            "cpu".into(),
            false,
            80,
            Arc::new(crate::exotic::catalog::CatalogStore::with_snapshot(
                crate::exotic::catalog::CatalogSnapshot::empty(),
            )),
        ))
    }

    #[test]
    fn offline_media_copy_keeps_catalog_until_root_returns() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mounted");
        std::fs::create_dir_all(root.join("source")).unwrap();
        std::fs::write(root.join("source/asset.jpg"), b"copied asset").unwrap();
        let state = batch_state(tmp.path());
        state
            .db_writer
            .lock()
            .unwrap()
            .execute(
                "UPDATE scan_roots SET path=?1 WHERE id=1",
                [root.to_string_lossy()],
            )
            .unwrap();
        let offline = tmp.path().join("offline");
        std::fs::rename(&root, &offline).unwrap();
        let version = state.data_version();
        let result =
            remove_media_items_hard_with(vec![101], &state, |_| panic!("离线时不能尝试回收"));
        assert!(
            matches!(result, Err(AppError::PathResolution(_))),
            "根暂时不可用必须拒绝清库"
        );
        assert_eq!(item_directory(&state.db_writer.lock().unwrap(), 101), 10);
        assert_eq!(state.data_version(), version);
        std::fs::rename(&offline, &root).unwrap();
        remove_media_items_hard_with(vec![101], &state, |path| {
            std::fs::remove_file(path).map_err(AppError::Io)
        })
        .unwrap();
        assert!(!root.join("source/asset.jpg").exists());
        assert_eq!(
            state
                .db_writer
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM media_items WHERE id=101", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }

    #[test]
    fn media_copy_removal_retries_partial_success_and_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("asset.jpg"), b"copy one").unwrap();
        std::fs::write(source.join("blocked.jpg"), b"copy two").unwrap();
        let state = batch_state(tmp.path());
        let version = state.data_version();
        let result = remove_media_items_hard_with(vec![101, 102, 103], &state, |path| {
            if path.file_name().unwrap() == "blocked.jpg" {
                return Err(AppError::CopyFile("模拟回收失败".into()));
            }
            std::fs::remove_file(path).map_err(AppError::Io)
        });
        assert!(result.is_err());
        assert!(!source.join("asset.jpg").exists());
        assert_eq!(
            std::fs::read(source.join("blocked.jpg")).unwrap(),
            b"copy two"
        );
        assert_eq!(state.data_version(), version + 1);
        assert_eq!(item_directory(&state.db_writer.lock().unwrap(), 102), 10);
        let mut recycled = 0;
        // 保留原 ID 重试：已完成项跳过，根可访问而文件已缺失的第三项仍可清理残留索引。
        remove_media_items_hard_with(vec![101, 102, 103], &state, |path| {
            recycled += 1;
            std::fs::remove_file(path).map_err(AppError::Io)
        })
        .unwrap();
        assert_eq!(recycled, 1);
        assert_eq!(state.data_version(), version + 2);
        assert_eq!(
            state
                .db_writer
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM media_items WHERE id IN(101,102,103)",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn copy_rejects_target_directory_moved_after_path_read() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        std::fs::write(tmp.path().join("source/asset.jpg"), b"preserved asset").unwrap();
        let state = batch_state(tmp.path());
        let outcome = copy_media_items_with(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
            || relocate_directory_after_plan(&state, 20),
        );
        assert!(outcome.completed.is_empty());
        assert!(matches!(
            outcome.failure.unwrap().error,
            AppError::CopyFile(_)
        ));
        assert!(
            !tmp.path().join("target").exists(),
            "过期复制计划不得重建已搬走的目录"
        );
        assert_eq!(
            std::fs::read(tmp.path().join("source/asset.jpg")).unwrap(),
            b"preserved asset"
        );
        assert!(!tmp.path().join("parent/target/asset.jpg").exists());
        let retried = copy_media_items_blocking(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(retried.failure.is_none());
        assert_eq!(retried.completed.len(), 1);
        assert_eq!(
            std::fs::read(tmp.path().join("parent/target/asset.jpg")).unwrap(),
            b"preserved asset"
        );
    }

    #[test]
    fn copy_rejects_busy_file_job_without_releasing_owner_and_retries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        std::fs::write(tmp.path().join("source/asset.jpg"), b"preserved asset").unwrap();
        let state = batch_state(tmp.path());
        let version = state.data_version();
        assert!(state.try_acquire_file_job(crate::state::FILE_JOB_RESTORE));
        let outcome = copy_media_items_blocking(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(outcome.completed.is_empty());
        assert!(matches!(
            outcome.failure.unwrap().error,
            AppError::CopyFile(_)
        ));
        assert_eq!(
            *state.file_job_owner.lock().unwrap(),
            Some(crate::state::FILE_JOB_RESTORE)
        );
        assert_eq!(
            std::fs::read_dir(tmp.path().join("target"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(state.data_version(), version);
        state.release_file_job(crate::state::FILE_JOB_RESTORE);
        let retried = copy_media_items_blocking(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(retried.failure.is_none());
        assert_eq!(retried.completed.len(), 1);
        assert_eq!(
            std::fs::read(tmp.path().join("target/asset.jpg")).unwrap(),
            b"preserved asset"
        );
        assert_eq!(*state.file_job_owner.lock().unwrap(), None);
    }

    #[test]
    fn copy_index_failure_does_not_publish_orphan_and_allows_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("blocked.jpg"), b"complete copied content").unwrap();
        let source_permissions = std::fs::metadata(source.join("blocked.jpg"))
            .unwrap()
            .permissions();
        let mut readonly = source_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(source.join("blocked.jpg"), readonly).unwrap();
        let state = batch_state(tmp.path());
        let version = state.data_version();
        let result = copy_media_items_blocking(
            vec![MediaRelocation {
                id: 102,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(result.completed.is_empty());
        assert!(matches!(result.failure.unwrap().error, AppError::Db(_)));
        assert_eq!(state.data_version(), version);
        assert_eq!(
            std::fs::read(source.join("blocked.jpg")).unwrap(),
            b"complete copied content"
        );
        assert!(
            !target.join("blocked.jpg").exists(),
            "落库冲突不得把未登记副本发布到最终文件名"
        );
        assert_eq!(
            std::fs::read_dir(&target).unwrap().count(),
            0,
            "失败应清理本次暂存"
        );
        state
            .db_writer
            .lock()
            .unwrap()
            .execute("DELETE FROM media_items WHERE id=104", [])
            .unwrap();
        let retried = copy_media_items_blocking(
            vec![MediaRelocation {
                id: 102,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(retried.failure.is_none());
        assert_eq!(retried.completed.len(), 1);
        assert_eq!(retried.completed[0].src_id, 102);
        assert_eq!(
            std::fs::read(target.join("blocked.jpg")).unwrap(),
            b"complete copied content"
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
        assert!(std::fs::metadata(target.join("blocked.jpg"))
            .unwrap()
            .permissions()
            .readonly());
        std::fs::set_permissions(source.join("blocked.jpg"), source_permissions.clone()).unwrap();
        std::fs::set_permissions(target.join("blocked.jpg"), source_permissions).unwrap();
        assert_eq!(state.data_version(), version + 1);
    }

    #[test]
    fn copy_commit_failure_keeps_published_file_and_reports_rescan() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("asset.jpg"), b"preserved source").unwrap();
        let state = batch_state(tmp.path());
        {
            let conn = state.db_writer.lock().unwrap();
            // 用延迟约束在副本插行时注入故障；前置位置复核的读事务不会提前消耗它。
            conn.execute_batch(
                "CREATE TABLE copy_commit_fault (
                    item_id INTEGER REFERENCES media_items(id) DEFERRABLE INITIALLY DEFERRED
                 );
                 CREATE TRIGGER fail_copy_commit AFTER INSERT ON media_items
                 BEGIN INSERT INTO copy_commit_fault(item_id) VALUES(999999); END;",
            )
            .unwrap();
        }
        let version = state.data_version();
        let outcome = copy_media_items_blocking(
            vec![MediaRelocation {
                id: 101,
                target_dir_id: 20,
            }],
            &state,
        );
        assert!(outcome.completed.is_empty());
        let failure = outcome.failure.unwrap();
        assert_eq!(failure.id, 101);
        let payload = serde_json::to_value(failure.error).unwrap();
        assert_eq!(payload["code"], "CopyFile");
        assert!(payload["message"]
            .as_str()
            .unwrap()
            .contains("重扫目标目录"));
        assert!(!payload.to_string().contains("FOREIGN KEY"));
        assert_eq!(
            std::fs::read(source.join("asset.jpg")).unwrap(),
            b"preserved source"
        );
        assert_eq!(
            std::fs::read(target.join("asset.jpg")).unwrap(),
            b"preserved source"
        );
        assert_eq!(state.data_version(), version + 1);
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
        assert_eq!(state.db_writer.lock().unwrap().query_row("SELECT count(*) FROM media_items WHERE directory_id=20 AND file_name='asset.jpg'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn relocation_batch_preserves_success_before_index_failure_and_stops() {
        let tmp = tempfile::Builder::new()
            .prefix("scrollery-move-batch-regression-")
            .tempdir()
            .unwrap()
            .keep();
        let source_dir = tmp.join("source");
        let target_dir = tmp.join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(source_dir.join("asset.jpg"), b"asset A").unwrap();
        std::fs::write(source_dir.join("blocked.jpg"), b"asset B").unwrap();
        std::fs::write(source_dir.join("later.jpg"), b"asset C").unwrap();
        std::fs::write(target_dir.join("already.jpg"), b"already moved").unwrap();

        let state = batch_state(&tmp);
        let version = state.data_version();
        // 同批有已在目标的项、成功项、残留索引冲突项，以及失败后不得执行的项。
        let outcome = relocate_media_items_blocking(
            vec![
                MediaRelocation {
                    id: 100,
                    target_dir_id: 20,
                },
                MediaRelocation {
                    id: 101,
                    target_dir_id: 20,
                },
                MediaRelocation {
                    id: 102,
                    target_dir_id: 20,
                },
                MediaRelocation {
                    id: 103,
                    target_dir_id: 20,
                },
            ],
            &state,
        );
        let payload = serde_json::to_value(&outcome).unwrap();
        assert_eq!(
            payload["completed"],
            serde_json::json!([{ "id": 101, "fromDirId": 10, "targetDirId": 20 }]),
            "中途失败必须保留真实成功项，且不得将 no-op 报为成功移动"
        );
        assert_eq!(payload["failure"]["id"], 102);
        assert_eq!(
            payload["failure"]["error"],
            serde_json::json!({ "code": "Db", "message": "数据库访问异常 | Database error" })
        );
        assert!(!payload.to_string().contains("UNIQUE"));
        assert_eq!(state.data_version(), version + 1);
        assert!(!source_dir.join("asset.jpg").exists());
        assert_eq!(
            std::fs::read(target_dir.join("asset.jpg")).unwrap(),
            b"asset A"
        );
        assert_eq!(
            std::fs::read(source_dir.join("blocked.jpg")).unwrap(),
            b"asset B"
        );
        assert_eq!(
            std::fs::read(source_dir.join("later.jpg")).unwrap(),
            b"asset C"
        );
        assert_eq!(
            std::fs::read(target_dir.join("already.jpg")).unwrap(),
            b"already moved"
        );
        assert!(!target_dir.join("blocked.jpg").exists());
        assert!(!target_dir.join("later.jpg").exists());
        let conn = state.db_writer.lock().unwrap();
        assert_eq!(item_directory(&conn, 100), 20);
        assert_eq!(item_directory(&conn, 101), 20);
        assert_eq!(item_directory(&conn, 102), 10);
        assert_eq!(item_directory(&conn, 103), 10);
        assert_eq!(item_directory(&conn, 104), 20);
    }
}

/// 通过重设 `directory_id` 并移动磁盘文件来重定位媒体项。
/// 保留同一 item id，使缩略图与 AI 嵌入向量仍有效；返回原目录 `from_dir_id`，
/// 供调用方记录逆操作并执行撤销/重做。遇首个项级错误停止，仍返回此前的实际成功项。
#[tauri::command]
pub async fn relocate_media_items(
    moves: Vec<MediaRelocation>,
    state: State<'_, Arc<AppState>>,
) -> Result<MediaRelocationBatchResult> {
    // span 埋点(W1,D-312 info 档:逐条 fs rename + DB 改行,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:relocate_media_items");
    // R1-3：逐条 SQL 与 fs rename 交织，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || relocate_media_items_blocking(moves, &state))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))
}

#[derive(PartialEq, Eq)]
struct MediaRelocationPlan {
    cur_abs: String,
    from_dir_id: i64,
    source_root_id: i64,
    target_abs_dir: PathBuf,
    target_root_id: i64,
    source_revision: i64,
    cache_key: i64,
}

fn load_media_relocation_plan(
    conn: &rusqlite::Connection,
    mv: &MediaRelocation,
) -> Result<MediaRelocationPlan> {
    // 一条查询捕获两侧身份，避免初始计划混入不同数据库快照的路径。
    Ok(conn.query_row(
        "SELECT m.directory_id,d.root_id,r.path,d.rel_path,m.file_name,
                t.root_id,tr.path,t.rel_path,m.source_revision,m.cache_key
         FROM media_items m JOIN directories d ON d.id=m.directory_id
         JOIN scan_roots r ON r.id=d.root_id JOIN directories t ON t.id=?2
         JOIN scan_roots tr ON tr.id=t.root_id WHERE m.id=?1",
        rusqlite::params![mv.id, mv.target_dir_id],
        |row| {
            let root: String = row.get(2)?;
            let rel: String = row.get(3)?;
            let name: String = row.get(4)?;
            Ok(MediaRelocationPlan {
                cur_abs: resolve_media_path(&root, &rel, &name),
                from_dir_id: row.get(0)?,
                source_root_id: row.get(1)?,
                target_abs_dir: PathBuf::from(row.get::<_, String>(6)?)
                    .join(row.get::<_, String>(7)?),
                target_root_id: row.get(5)?,
                source_revision: row.get(8)?,
                cache_key: row.get(9)?,
            })
        },
    )?)
}

fn relocate_media_items_blocking(
    moves: Vec<MediaRelocation>,
    state: &Arc<AppState>,
) -> MediaRelocationBatchResult {
    relocate_media_items_with(moves, state, || {})
}

fn relocate_media_items_with(
    moves: Vec<MediaRelocation>,
    state: &Arc<AppState>,
    mut after_plan: impl FnMut(),
) -> MediaRelocationBatchResult {
    let mut guard = InvalidateOnWrite::new(state);
    let mut batch = MediaRelocationBatchResult {
        completed: Vec::new(),
        failure: None,
    };

    for mv in moves {
        // 查询、路径和磁盘错误都属于当前项，不能让早期 ? 丢掉已提交的前缀。
        let outcome = (|| -> Result<Option<MediaRelocationResult>> {
            let plan = {
                let pool = state.db_read_pool.get().map_err(AppError::from)?;
                load_media_relocation_plan(&pool, &mv)?
            };
            after_plan();
            if mv.target_dir_id == plan.from_dir_id {
                return Ok(None); // 已在目标目录，不登记一次新的移动。
            }
            // 与目录搬运共用文件任务和根闸，覆盖路径复核到 rename/提交/恢复的窗口。
            if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
                return Err(AppError::InvalidMove(
                    "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
                ));
            }
            let roots = [plan.source_root_id, plan.target_root_id];
            let outcome = state.with_scan_roots_exclusive(
                &roots,
                || -> Result<Option<MediaRelocationResult>> {
                    if state.any_scan_running(&roots) {
                        return Err(AppError::InvalidMove(
                            "扫描进行中，请稍后再试 | a scan is running, try again later".into(),
                        ));
                    }
                    {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        if load_media_relocation_plan(&conn, &mv)? != plan {
                            return Err(AppError::InvalidMove(
                                "文件或目录位置已变化，请重试 | file or directory location changed, retry".into(),
                            ));
                        }
                    }
                    let file_name = std::path::Path::new(&plan.cur_abs)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if file_name.is_empty() {
                        return Ok(None);
                    }

                    let target_path = plan.target_abs_dir.join(&file_name);
                    if target_path == std::path::Path::new(&plan.cur_abs) {
                        return Ok(None);
                    }
                    if target_path.try_exists().unwrap_or(false) {
                        return Err(AppError::MoveFile(format!(
                            "目标已存在同名文件 | Destination file already exists: {}",
                            file_name
                        )));
                    }

                    std::fs::create_dir_all(&plan.target_abs_dir).map_err(|error| {
                        tracing::error!(
                            media_id = mv.id,
                            target = %plan.target_abs_dir.display(),
                            error = %error,
                            "relocate_media_items: 创建目标目录失败"
                        );
                        AppError::MoveFile(
                            "创建目标目录失败 | Failed to create destination folder".into(),
                        )
                    })?;
                    {
                        let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        // 文件可能搬走后在提交/恢复阶段失败；失败项也必须失效磁盘和布局快照。
                        guard.dirty = true;
                        relocate_media_file(
                            &mut conn,
                            mv.id,
                            mv.target_dir_id,
                            std::path::Path::new(&plan.cur_abs),
                            &target_path,
                        )?;
                    }
                    Ok(Some(MediaRelocationResult {
                        id: mv.id,
                        from_dir_id: plan.from_dir_id,
                        target_dir_id: mv.target_dir_id,
                    }))
                },
            );
            state.release_file_job(FILE_JOB_DIR_MOVE);
            outcome.ok_or_else(|| {
                AppError::InvalidMove(
                    "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                )
            })?
        })();
        match outcome {
            Ok(Some(result)) => {
                guard.dirty = true;
                batch.completed.push(result);
            }
            Ok(None) => {}
            Err(error) => {
                batch.failure = Some(MediaFileOperationFailure { id: mv.id, error });
                break;
            }
        }
    }

    // Invalidate the layout cache so the next compute_layout reflects the moved items.
    // Directory media_count is computed live by the tree query, so no count maintenance.
    // 布局失效 + bump 由 guard 在 Drop 统一执行（含中途报错路径，P1-6）。目录 media_count
    // 由树查询实时计算，无需维护计数列。
    batch
}

/// Result of a media copy — the new row id lets the caller record an exact inverse
/// (delete the copy) for undo/redo.
/// 媒体复制结果 — 新行 id 让调用方记录精确逆操作（删除副本）以撤销/重做。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCopyResult {
    pub src_id: i64,
    pub new_id: i64,
}

/// 批量复制已经落库的副本与首个失败项，供调用方登记准确的撤销记录。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCopyBatchResult {
    pub completed: Vec<MediaCopyResult>,
    pub failure: Option<MediaFileOperationFailure>,
}

#[cfg(windows)]
fn copy_file_no_replace(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::CopyFileW;

    // std::fs 会处理长路径；直接调用 Win32 时需自行取得 verbatim 路径，不能依赖系统策略。
    // 只规范化已存在的父目录，最终目标仍由 failIfExists 原子判断，不能提前跟随目标链接。
    let source = source.canonicalize()?;
    let target_name = target.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "复制目标缺少文件名 | copy target has no filename",
        )
    })?;
    let target_parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let target = target_parent.canonicalize()?.join(target_name);
    let to_wide = |path: &std::path::Path| -> std::io::Result<Vec<u16>> {
        let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "文件路径包含空字符 | path contains a null character",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let source = to_wide(&source)?;
    let target = to_wide(&target)?;
    // 目标检查只是提前提示；由系统原子拒绝同名目标，关闭检查后被其它写入者抢占的窗口。
    // 保留系统复制路径与文件属性复制行为，不把大文件复制改成用户态小块循环。
    unsafe { CopyFileW(PCWSTR(source.as_ptr()), PCWSTR(target.as_ptr()), true) }
        .map_err(std::io::Error::other)
}

#[cfg(not(windows))]
fn copy_file_no_replace(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    let mut source = std::fs::File::open(source)?;
    let metadata = source.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "复制源不是普通文件 | copy source is not a regular file",
        ));
    }
    // create_new 同时拒绝普通目标和符号链接，失败时不触碰它们。
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    std::io::copy(&mut source, &mut target)?;
    target.set_permissions(metadata.permissions())
}

#[cfg(test)]
mod copy_file_tests {
    use super::*;

    #[test]
    fn copy_rejects_target_created_after_precheck_without_changing_either_file() {
        let tmp = tempfile::tempdir().unwrap();
        let directory = tmp
            .path()
            .join("nested".repeat(22))
            .join("deeper".repeat(22));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.jpg");
        let target = directory.join("target.jpg");
        std::fs::write(&source, b"complete source content").unwrap();
        let permissions = std::fs::metadata(&source).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&source, readonly).unwrap();
        assert!(!target.exists());
        // 入口检查以后，另一次复制或外部程序先写入同名文件。
        std::fs::write(&target, b"existing user content").unwrap();
        assert!(copy_file_no_replace(&source, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"existing user content");
        assert_eq!(std::fs::read(&source).unwrap(), b"complete source content");

        // 目标明确腾空后，同一复制路径仍能完整落盘。
        std::fs::remove_file(&target).unwrap();
        copy_file_no_replace(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"complete source content");
        assert_eq!(std::fs::read(&source).unwrap(), b"complete source content");
        assert!(std::fs::metadata(&target).unwrap().permissions().readonly());
        std::fs::set_permissions(&source, permissions.clone()).unwrap();
        std::fs::set_permissions(&target, permissions).unwrap();
    }
}

/// 把媒体项复制到目标目录，按目标文件的真实属性登记独立缓存身份。
/// 返回实际完成副本的新行 id 和首个失败项，使调用方在部分失败后仍能精确撤销。
#[tauri::command]
pub async fn copy_media_items_db(
    moves: Vec<MediaRelocation>,
    state: State<'_, Arc<AppState>>,
) -> Result<MediaCopyBatchResult> {
    // span 埋点(W1,D-312 info 档:逐条 fs copy + DB 插行,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:copy_media_items_db");
    // R1-3：逐条 SQL 与 fs copy（可达 GB 级）交织，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || copy_media_items_blocking(moves, &state))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))
}

fn copy_media_items_blocking(
    moves: Vec<MediaRelocation>,
    state: &Arc<AppState>,
) -> MediaCopyBatchResult {
    copy_media_items_with(moves, state, || {})
}

fn copy_media_items_with(
    moves: Vec<MediaRelocation>,
    state: &Arc<AppState>,
    mut after_plan: impl FnMut(),
) -> MediaCopyBatchResult {
    let mut guard = InvalidateOnWrite::new(state);
    let mut batch = MediaCopyBatchResult {
        completed: Vec::new(),
        failure: None,
    };

    for mv in moves {
        // 路径解析、磁盘和落库错误都归入当前项，保留此前实际成功的副本。
        let outcome = (|| -> Result<Option<MediaCopyResult>> {
            let plan = {
                let pool = state.db_read_pool.get().map_err(AppError::from)?;
                load_media_relocation_plan(&pool, &mv)?
            };
            after_plan();
            if mv.target_dir_id == plan.from_dir_id {
                return Ok(None);
            }
            if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
                return Err(AppError::CopyFile(
                    "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
                ));
            }
            let roots = [plan.source_root_id, plan.target_root_id];
            let outcome = state.with_scan_roots_exclusive(
                &roots,
                || -> Result<Option<MediaCopyResult>> {
                    if state.any_scan_running(&roots) {
                        return Err(AppError::CopyFile(
                            "扫描进行中，请稍后再试 | a scan is running, try again later".into(),
                        ));
                    }
                    {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        // 路径读取后目录可能已被搬走；持有两侧根闸并复核后才能创建暂存。
                        if load_media_relocation_plan(&conn, &mv)? != plan {
                            return Err(AppError::CopyFile(
                                "文件或目录位置已变化，请重试 | file or directory location changed, retry".into(),
                            ));
                        }
                    }
                    let cur_abs = &plan.cur_abs;
                    let target_abs_dir = &plan.target_abs_dir;
                    let file_name = PathBuf::from(cur_abs)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if file_name.is_empty() {
                        return Ok(None);
                    }

                    let dest = target_abs_dir.join(&file_name);
                    if dest == std::path::Path::new(cur_abs) {
                        return Ok(None); // copy onto self — skip | 复制到自身 — 跳过
                    }
                    if dest.try_exists().unwrap_or(false) {
                        return Err(AppError::CopyFile(format!(
                            "目标已存在同名文件: {}",
                            file_name
                        )));
                    }

                    std::fs::create_dir_all(target_abs_dir).map_err(|error| {
                        tracing::error!(media_id = mv.id, target = %target_abs_dir.display(), %error, "复制时创建目标目录失败");
                        AppError::CopyFile("创建目标目录失败 | Failed to create destination folder".into())
                    })?;
                    // 大文件先复制到同卷私有暂存，期间不持数据库锁；扫描器会剪除点前缀目录。
                    let staging = tempfile::Builder::new()
                        .prefix(".scrollery-copy-")
                        .suffix(".tmp")
                        .tempdir_in(target_abs_dir)
                        .map_err(AppError::Io)?;
                    let staged_file = staging.path().join("payload.tmp");
                    copy_file_no_replace(std::path::Path::new(cur_abs), &staged_file).map_err(|error| {
                        tracing::error!(media_id = mv.id, source = %cur_abs, target = %dest.display(), %error, "复制文件失败");
                        AppError::CopyFile("复制文件失败，请检查源文件和目标目录 | Failed to copy file; check the source and destination".into())
                    })?;

                    let metadata = std::fs::metadata(&staged_file).map_err(AppError::Io)?;
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
                    let stat = q::MediaCopyFileStat {
                        file_size: metadata.len() as i64,
                        file_mtime: modified.map(|time| time.as_secs() as i64).unwrap_or(0),
                        file_mtime_ns: modified
                            .and_then(|time| i64::try_from(time.as_nanos()).ok())
                            .unwrap_or(0),
                    };
                    let new_id = {
                        let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        let tx =
                            conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                        if load_media_relocation_plan(&tx, &mv)? != plan {
                            return Err(AppError::CopyFile(
                                "复制期间文件位置已变化，请重试 | File location changed during copy; retry"
                                    .into(),
                            ));
                        }
                        // 唯一键/外键先落到未提交事务；发布仍由系统原子拒绝覆盖抢占目标。
                        let new_id = q::duplicate_media_item_into_dir(&tx, mv.id, mv.target_dir_id, &stat)?;
                        rename_no_replace(&staged_file, &dest).map_err(|error| {
                            tracing::error!(media_id = mv.id, target = %dest.display(), %error, "副本发布失败");
                            AppError::CopyFile("副本发布失败，请检查目标文件是否已存在 | Failed to publish copy; check the destination".into())
                        })?;
                        guard.dirty = true;
                        tx.commit().map_err(|error| {
                            tracing::error!(media_id = mv.id, target = %dest.display(), %error, "副本已发布但索引提交失败");
                            // 提交失败后不盲删最终路径，那里可能已被外部程序改写。
                            AppError::CopyFile("副本已生成但登记失败，请重扫目标目录 | Copy created but catalog commit failed; rescan the destination".into())
                        })?;
                        new_id
                    };
                    Ok(Some(MediaCopyResult {
                        src_id: mv.id,
                        new_id,
                    }))
                },
            );
            state.release_file_job(FILE_JOB_DIR_MOVE);
            outcome.ok_or_else(|| {
                AppError::CopyFile(
                    "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                )
            })?
        })();
        match outcome {
            Ok(Some(result)) => {
                guard.dirty = true;
                batch.completed.push(result);
            }
            Ok(None) => {}
            Err(error) => {
                batch.failure = Some(MediaFileOperationFailure { id: mv.id, error });
                break;
            }
        }
    }

    // S1：新行已插入（成员变化）→ 失效由 guard 在 Drop 统一执行（含报错路径）。
    batch
}

/// Hard-remove media items: move each file to the OS trash and delete its DB row. Used to
/// UNDO a drag-copy (问题2). Missing files / rows are tolerated. Invalidates the layout cache.
/// A trash failure aborts before the corresponding DB delete, so a failed physical move can
/// never silently discard the media row.
/// 硬删除媒体项：把每个文件移入系统回收站并删除其 DB 行。用于撤销拖拽复制（问题2）。
/// 容忍文件/行缺失。使布局缓存失效。
#[tauri::command]
pub async fn remove_media_items_hard(ids: Vec<i64>, state: State<'_, Arc<AppState>>) -> Result<()> {
    // span 埋点(W1,D-312 info 档:回收站 syscall+逐条 DB 删行+派生缓存清理,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:remove_media_items_hard");
    // R1-3：回收站 syscall + 逐条 SQL + 派生缓存文件删除全是阻塞，整段下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        remove_media_items_hard_with(ids, &state, |path| {
            trash::delete(path).map_err(|error| {
                tracing::warn!(path = %path.display(), %error, "副本移入回收站失败");
                AppError::os(
                    "无法移入系统回收站 | failed to move file to system trash",
                    error,
                )
            })
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

fn remove_media_items_hard_with(
    ids: Vec<i64>,
    state: &Arc<AppState>,
    mut recycle: impl FnMut(&std::path::Path) -> Result<()>,
) -> Result<()> {
    use rusqlite::OptionalExtension;

    let mut guard = InvalidateOnWrite::new(state);
    // 缓存目录（删除后按 cache_key 清派生孤儿，§3.3.2 / Q7）。
    let cache_dir = state
        .thumb_config
        .read()
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
        .cache_dir
        .clone();
    for id in ids {
        // 只有查无此行才是已完成的重试项；数据库错误和扫描根离线不能伪装成缺失。
        let path_info: Option<(String, String, String, i64)> = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            pool.query_row(
                "SELECT r.path,d.rel_path,m.file_name,m.cache_key FROM media_items m
                 JOIN directories d ON d.id=m.directory_id
                 JOIN scan_roots r ON r.id=d.root_id WHERE m.id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
        };
        let Some((root, rel, name, cache_key)) = path_info else {
            continue;
        };
        // 回收的是目录项本身：规范化父目录后保留最终文件名，避免把链接目标当成待删资产。
        let mut components = std::path::Path::new(&name).components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err(AppError::PathResolution(
                "媒体文件名无效 | invalid media filename".into(),
            ));
        }
        if let Some(parent) = resolve_within_root_optional(&root, &rel)? {
            let target = parent.join(&name);
            match std::fs::symlink_metadata(&target) {
                Ok(_) => recycle(&target)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(media_id = id, path = %target.display(), %error, "副本路径不可访问");
                    return Err(AppError::PathResolution(
                        "副本路径不可访问 | copied file inaccessible".into(),
                    ));
                }
            }
        }
        // 引用检查到回收完成共用写锁，防止并发复制/发布在检查后引用将被删除的文件。
        // 共享键通过既有索引查找；普通图片生成流程不增加锁或查询。
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let artifact_paths = q::image_artifact_cache_paths(&conn, id)?;
        let shared_cache_key = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM media_items WHERE cache_key=?1 AND id<>?2)",
            rusqlite::params![cache_key, id],
            |row| row.get::<_, bool>(0),
        )?;
        // CASCADE 删脸后同事务重算人物派生字段，防止计数与封面悬挂。
        q::delete_media_item_hard(&conn, id)?;
        // 删行已生效——即使后续项失败也必须失效（P1-6）
        guard.dirty = true;
        for relative in artifact_paths {
            match crate::thumbnail::artifact_io::remove_cached_artifact(&cache_dir, &relative) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => tracing::warn!(%relative, %error, "硬删除后的图片缓存清理失败"),
            }
        }
        // 硬删后即时清理派生缓存孤儿（§3.3.2 / Q7）：按 cache_key 删 4 档缩略图 + ai_thumb +
        // sprite + motion。共享键与新版产物一样保留；best-effort、失败不阻塞删除。软删（is_deleted）不走此路径
        // （保留缓存供恢复，与「离线≠删除」同理）。
        if !shared_cache_key {
            crate::thumbnail::cache::remove_cache_files_for_key(&cache_dir, cache_key);
        }
    }

    // S1：硬删（成员变化）→ 失效由 guard 在 Drop 统一执行（含报错路径）。
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
// Folder (directory) move / copy / undo-delete
// 文件夹（目录）移动 / 复制 / 撤销删除
// ════════════════════════════════════════════════════════════════════════════

/// 文件夹移动结果 — 足以让前端定位、记录撤销并在半完成时给出重试入口。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDirResult {
    pub dir_id: i64,
    pub root_id: i64,
    pub new_rel_path: String,
    pub affected_dirs: usize,
    pub affected_media: usize,
    /// 文件真实落点（绝对路径）。物理搬运成功时即使索引段失败，前端也能如实告知用户。
    pub target_abs_path: String,
    /// 源目录残留（跨卷删源未完成）：索引已更新，旧路径还有一份副本待清理。
    pub source_leftover: Option<String>,
    /// 未完成阶段日志 id（Some = 仍需重试收尾）。
    pub recovery_id: Option<i64>,
}

/// 文件夹复制结果 — 标识新创建的子树以供撤销，并明确「物理已复制、尚未入库」。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyDirResult {
    pub created_root_id: i64,
    pub created_rel_path: String,
    pub created_abs_path: String,
    /// 本次落盘的普通文件数。
    pub copied_files: usize,
    /// 需要调用方触发目标根重扫把新文件入库（可重试；失败不代表复制失败）。
    pub needs_rescan: bool,
}

/// Move a folder (and its whole subtree) into another folder, **preserving metadata**.
/// In-place DB update: rewrites rel_path/parent_id/depth/root_id of every descendant
/// directory plus each affected media item's cache_key / thumb_path / **volume identity**
/// (volume_id + volume_relative_path, so an offline source volume can never mark the
/// already-moved item offline), and relocates cached thumbnail files.
/// Favorites / ratings / AI embeddings (keyed by item id) are kept.
///
/// 将文件夹（及其整个子树）移动到另一个文件夹，**保留元数据**。
/// 原地更新数据库：重写每个后代目录的 rel_path/parent_id/depth/root_id，以及每个受影响媒体项的
/// cache_key / thumb_path / **卷定位**（volume_id + volume_relative_path —— 源卷离线不得影响
/// 已搬到在线目标卷的条目），并重定位已缓存的缩略图文件。收藏 / 评分 / AI 嵌入（按 item id
/// 关联）均保留。
///
/// 执行顺序与失败语义（审查 §7.1-A/B，实现见 `ipc::dir_move`）：
///   1. 校验 + 计算新旧路径；
///   2. 源/目标两个扫描根的闸门按 root_id 升序独占（期间不允许扫描竞争）；
///   3. 落一行阶段日志 → 物理搬运（同卷 rename；跨卷走独占暂存 + 校验 + rename 发布 + 删源，
///      全程不覆盖目标既有路径，源原件不丢）→ 重写身份 → 收尾日志。
/// 物理成功而 DB 失败时返回 `AppError::MoveRecovery`（稳定码 `move_db_pending` + 日志 id +
/// 目标绝对路径），前端据此显示文件真实位置并可重试；重试与启动收尾共用同一段幂等逻辑。
#[tauri::command]
pub async fn move_directory(
    source_dir_id: i64,
    target_dir_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<MoveDirResult> {
    // span 埋点(W1,D-312 info 档:子树重写事务+缓存文件重定位,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:move_directory");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<MoveDirResult> {
        // ── 1. 计划与校验 ── 读元数据、全套校验、算新前缀与规范化绝对路径 ──
        // R1-3：读池查询在下沉 blocking 段内取连接（校验失败以 Err 穿出）。
        let plan = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            dir_move::load_plan(&pool, source_dir_id, target_dir_id)?
        };

        // ── 2. 文件任务门闩 ──
        // 与备份/恢复/导出/编辑共用「同一时刻只有一个重文件任务」的门闩；忙碌即拒绝，不抢占。
        if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
            return Err(AppError::InvalidMove(
                "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
            ));
        }
        let outcome = (|| -> Result<MoveDirResult> {
            // ── 3. 两个根级闸门按固定顺序独占 ──
            // 跨根移动要同时挡住源根与目标根上的扫描：按 root_id 升序取闸（见
            // AppState::with_scan_roots_exclusive），持闸期间新的扫描轮次安装不进来；闸内再确认
            // 当前没有扫描在跑——「检查」与「动手」之间因此不存在扫描竞争者。
            let roots = [plan.source_root_id, plan.target_root_id];
            let mut guard = InvalidateOnWrite::new(&state);
            let outcome = state
                .with_scan_roots_exclusive(&roots, || -> Result<dir_move::MoveDirOutcome> {
                    if state.any_scan_running(&roots) {
                        return Err(AppError::InvalidMove(
                            "扫描进行中，请稍后再试 | a scan is running, try again later".into(),
                        ));
                    }
                    // 计划是在取闸之前算的：排队期间另一次移动/删根/重链接都可能让它过期，
                    // 动磁盘之前必须复核源与目标身份（不一致即让用户重试，不用过期计划）。
                    {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        dir_move::verify_plan_current(&conn, &plan)?;
                    }
                    // 缩略图缓存目录（缓存文件重定位用）在闸内取一次快照。
                    let cache_dir = state
                        .thumb_config
                        .read()
                        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
                        .cache_dir
                        .clone();
                    // 磁盘一定会被改写（同卷 rename 或跨卷「暂存→发布→删源」）→ 成败都失效视图。
                    guard.dirty = true;
                    state.tree_snapshots.clear(); // R-05:跨卷中途失败也已改写磁盘
                    dir_move::execute_move(dir_move::MoveDb::State(&state), &plan, &cache_dir)
                })
                .ok_or_else(|| {
                    AppError::InvalidMove(
                        "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                    )
                })??;

            info!(
                "move_directory: dir={source_dir_id} -> parent={target_dir_id} ({} dirs, {} media) | 文件夹已移动",
                outcome.affected_dirs, outcome.affected_media
            );

            Ok(MoveDirResult {
                dir_id: source_dir_id,
                root_id: plan.target_root_id,
                new_rel_path: plan.new_rel.clone(),
                affected_dirs: outcome.affected_dirs,
                affected_media: outcome.affected_media,
                target_abs_path: plan.dst_abs.to_string_lossy().to_string(),
                source_leftover: outcome.source_leftover,
                recovery_id: outcome.recovery_id,
            })
        })();
        state.release_file_job(FILE_JOB_DIR_MOVE);
        outcome
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Copy a folder (and its whole subtree) into another folder on disk. The new files
/// are ingested as fresh assets via a subsequent re-scan triggered by the frontend.
/// Returns the created subtree's location so an undo can remove exactly it, plus the
/// copied file count and an explicit `needs_rescan` flag (审查 §7.1-B：复制依赖后续扫描
/// 入库，扫描中止会留下「文件在、库未入」，调用方必须能如实显示并重试)。
///
/// 将文件夹（及其整个子树）复制到磁盘上的另一个文件夹。新文件经前端随后触发的
/// 重扫作为全新资产引入。返回新建子树的位置（以便撤销时精确移除）、落盘文件数与
/// `needsRescan` 标记——复制成功但尚未入库是一个**明确、可重试**的中间状态。
///
/// 物理复制走独占暂存 + rename 发布（`dir_move::copy_tree_published`）：目标位置要么不存在、
/// 要么是完整副本，中途失败只清我们自己的暂存目录。
#[tauri::command]
pub async fn copy_directory(
    source_dir_id: i64,
    target_dir_id: i64,
    on_progress: Option<tauri::ipc::JavaScriptChannelId>,
    webview: tauri::Webview,
    state: State<'_, Arc<AppState>>,
) -> Result<CopyDirResult> {
    // span 埋点(W1,D-312 info 档:递归复制整树,真实 IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:copy_directory");
    // Tauri 的 Channel 仅实现 CommandArg，无法嵌入 Option；官方可反序列化 ID 保留缺省参数兼容。
    let on_progress: Option<tauri::ipc::Channel<DirectoryCopyProgress>> =
        on_progress.map(|channel| channel.channel_on(webview));
    // R1-3：读池查询与整段复制（文件 IO）下沉同一 blocking 段。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<CopyDirResult> {
        // 校验口径与目录移动共用一份（不可复制扫描根 / 不可入自身 / 不可入自身子树 / 同名冲突）。
        let plan = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            dir_move::load_plan(&pool, source_dir_id, target_dir_id)?
        };
        // 文件任务门闩：与备份/恢复/导出/编辑互斥（同一时刻只允许一个重文件任务）。
        if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
            return Err(AppError::InvalidMove(
                "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
            ));
        }
        let (generation, cancel) = state.directory_copy_token.begin();
        let copied_files = (|| -> Result<usize> {
            let mut last_report: Option<std::time::Instant> = None;
            let mut report = |progress: DirectoryCopyProgress| {
                if progress.publishing
                    || last_report.is_none_or(|last| {
                        last.elapsed() >= std::time::Duration::from_millis(150)
                    })
                {
                    if let Some(channel) = &on_progress {
                        // 通道关闭不改变磁盘任务结果；终态由命令 promise 返回，不因进度失败回滚。
                        let _ = channel.send(progress);
                    }
                    last_report = Some(std::time::Instant::now());
                }
            };
            let mut control =
                dir_move::DirectoryCopyControl::new(generation, &cancel, &mut report);
            let roots = [plan.source_root_id, plan.target_root_id];
            state
                .with_scan_roots_exclusive(&roots, || -> Result<usize> {
                    if state.any_scan_running(&roots) {
                        return Err(AppError::InvalidMove(
                            "扫描进行中，请稍后再试 | a scan is running, try again later".into(),
                        ));
                    }
                    // 计划在取闸前算出：排队期间可能过期，复制前复核源与目标身份。
                    {
                        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                        dir_move::verify_plan_current(&conn, &plan)?;
                    }
                    // R-05:复制中途失败也已落盘一部分,成败都清（暂存不留垃圾）。
                    state.tree_snapshots.clear();
                    dir_move::copy_tree_published(
                        &plan.src_abs,
                        &plan.dst_abs,
                        &plan.name,
                        Some(&mut control),
                    )
                })
                .ok_or_else(|| {
                    AppError::InvalidMove(
                        "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                    )
                })?
        })();
        state.directory_copy_token.finish(generation);
        state.release_file_job(FILE_JOB_DIR_MOVE);
        let copied_files = copied_files?;

        info!(
            "copy_directory: dir={source_dir_id} -> parent={target_dir_id} ({copied_files} files) | 文件夹已复制"
        );

        Ok(CopyDirResult {
            created_root_id: plan.target_root_id,
            created_rel_path: plan.new_rel.clone(),
            created_abs_path: plan.dst_abs.to_string_lossy().replace('\\', "/"),
            copied_files,
            // 入库由调用方随后触发的目标根重扫完成；未完成时前端据此显示「已复制、未入库」并可重试。
            needs_rescan: true,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 取消指定代次的目录复制；过期请求无动作，进入发布段后由复制命令返回实际结果。
#[tauri::command]
pub fn cancel_directory_copy(generation: u64, state: State<'_, Arc<AppState>>) {
    state.directory_copy_token.cancel_generation(generation);
}

/// 列出未完成的目录移动（审查 §7.1-B：重启后用户要能看到文件真实位置与重试入口）。
///
/// 只读清单，不做任何收尾：收尾可能包含大规模跨卷拷贝，必须由用户显式触发
/// （[retry_directory_move]）或启动期轻量收尾负责。
#[tauri::command]
pub async fn list_pending_directory_moves(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<dir_move::MoveRecoveryReport>> {
    let _span = crate::logging::SpanTimer::info("ipc:list_pending_directory_moves");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<dir_move::MoveRecoveryReport>> {
        let pending = {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            q::list_pending(&conn)?
        };
        Ok(pending.iter().map(dir_move::pending_report).collect())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 重试一条未完成的目录移动（幂等，允许重做物理搬运）。
///
/// 返回收尾后的报告；日志行已不存在时返回 `None`（说明这条已经收尾干净）。
#[tauri::command]
pub async fn retry_directory_move(
    recovery_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<dir_move::MoveRecoveryReport>> {
    let _span = crate::logging::SpanTimer::info("ipc:retry_directory_move");
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Option<dir_move::MoveRecoveryReport>> {
        // 1. 读日志行拿两个根，据根取闸（固定顺序），避免与扫描竞争。
        let roots = {
            let pool = state.db_read_pool.get().map_err(AppError::from)?;
            match q::get(&pool, recovery_id)? {
                Some(entry) => [entry.source_root_id, entry.target_root_id],
                None => return Ok(None),
            }
        };
        // 2. 文件任务门闩：收尾可能整树拷贝/删源残留，同属重文件任务。
        if !state.try_acquire_file_job(FILE_JOB_DIR_MOVE) {
            return Err(AppError::InvalidMove(
                "有其它文件任务正在进行，请稍后再试 | another file task is running".into(),
            ));
        }
        let report = (|| -> Result<Option<dir_move::MoveRecoveryReport>> {
            let mut guard = InvalidateOnWrite::new(&state);
            state
                .with_scan_roots_exclusive(
                    &roots,
                    || -> Result<Option<dir_move::MoveRecoveryReport>> {
                        if state.any_scan_running(&roots) {
                            return Err(AppError::InvalidMove(
                                "扫描进行中，请稍后再试 | a scan is running, try again later"
                                    .into(),
                            ));
                        }
                        let cache_dir = state
                            .thumb_config
                            .read()
                            .map_err(|e| {
                                AppError::internal("内部任务失败 | internal task failed", e)
                            })?
                            .cache_dir
                            .clone();
                        // 收尾可能改写身份（DB）或磁盘（删源残留）→ 视图与树快照一律失效。
                        guard.dirty = true;
                        state.tree_snapshots.clear();
                        dir_move::retry_entry(
                            dir_move::MoveDb::State(&state),
                            &cache_dir,
                            recovery_id,
                        )
                    },
                )
                .ok_or_else(|| {
                    AppError::InvalidMove(
                        "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
                    )
                })?
        })();
        state.release_file_job(FILE_JOB_DIR_MOVE);
        report
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// 只有物理回收成功或确认目标已不存在，才允许清除目录索引；解析失败须保留重试信息。
fn delete_directory_copy_with(
    root_path: &str,
    rel_path: &str,
    recycle: impl FnOnce(&std::path::Path) -> Result<()>,
    purge: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if let Some(canonical) = resolve_within_root_optional(root_path, rel_path)? {
        recycle(&canonical)?;
    }
    purge()
}

fn delete_directory_to_trash_with(
    state: &AppState,
    root_id: i64,
    rel_path: &str,
    recycle: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<()> {
    // 取消后仍须持根闸直到物理回收和索引清理结束；否则新扫描可在回收期间枚举旧文件，
    // 再在清理之后按有效的新代次把已删除目录写回。磁盘 IO 期间不持 DB writer 锁。
    state
        .with_scan_root_exclusive(root_id, || {
            state.cancel_scan_under_root_gate(root_id);
            let root_path = {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                q::get_scan_root(&conn, root_id)?.path
            };
            delete_directory_copy_with(
                &root_path,
                rel_path,
                |canonical| {
                    recycle(canonical)?;
                    state.tree_snapshots.clear();
                    Ok(())
                },
                || {
                    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(dir_id) = q::find_directory_id(&conn, root_id, rel_path)? {
                        q::delete_directory_by_id(&conn, dir_id)?;
                        state.bump_data_version();
                    }
                    Ok(())
                },
            )
        })
        .ok_or_else(|| {
            AppError::InvalidMove(
                "数据库正在维护，请稍后再试 | database maintenance in progress".into(),
            )
        })?
}

#[cfg(test)]
mod directory_trash_tests {
    use super::*;
    use rusqlite::OptionalExtension;

    fn seed_catalog(root: &std::path::Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        seed_catalog_into(&conn, root);
        conn
    }

    fn seed_catalog_into(conn: &rusqlite::Connection, root: &std::path::Path) {
        crate::db::schema::initialize_schema(conn).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn.execute(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, ?1, 'Root')",
            rusqlite::params![root.to_string_lossy()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES (10, 1, 'copy', 'copy');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, sort_datetime, cache_key, rating)
             VALUES (20, 10, 'photo.jpg', 6, 1, 'jpg', 1, 20, 5);",
        )
        .unwrap();
    }

    #[test]
    fn directory_trash_blocks_restart_until_catalog_purge() {
        use std::sync::mpsc;
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("copy")).unwrap();
        std::fs::write(root.join("copy/photo.jpg"), b"photo!").unwrap();
        let db_path = tmp.path().join("test.db");
        let writer = crate::db::create_write_connection(&db_path).unwrap();
        seed_catalog_into(&writer.lock().unwrap(), &root);
        let pool = crate::db::create_read_pool(&db_path, 1).unwrap();
        let (config, _) =
            crate::config::ConfigManager::load_or_init(tmp.path().join("config.toml")).unwrap();
        let (log_writer, _log_guard) = tracing_appender::non_blocking(std::io::sink());
        let state = Arc::new(AppState::new(
            writer,
            pool,
            Arc::new(config),
            tmp.path().join("cache"),
            tmp.path().join("logs"),
            crate::logging::LogRingBuffer::new(Arc::from("directory-trash-test"), 1),
            log_writer.error_counter(),
            tmp.path().to_path_buf(),
            tmp.path().join("exotic"),
            256,
            0,
            "balanced".into(),
            "cpu".into(),
            false,
            80,
            Arc::new(crate::exotic::catalog::CatalogStore::with_snapshot(
                crate::exotic::catalog::CatalogSnapshot::empty(),
            )),
        ));
        let (old_generation, old_cancel) = state.new_scan_run_token(1);
        let (recycling_tx, recycling_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (deleted_tx, deleted_rx) = mpsc::channel();
        let recycled = tmp.path().join("recycled-copy");
        let deleting_state = Arc::clone(&state);
        let deleting = std::thread::spawn(move || {
            delete_directory_to_trash_with(&deleting_state, 1, "copy", |path| {
                recycling_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                std::fs::rename(path, recycled).map_err(AppError::Io)
            })
        });
        recycling_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let restarting_state = Arc::clone(&state);
        let restarting_root = root.clone();
        let restarting = std::thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            let (generation, _) = restarting_state
                .try_replace_scan_run_token_with_id(1, Some("restart"))
                .unwrap();
            // 模拟真实扫描先枚举磁盘、稍后按代次批量写库；删除期间不得取得这份旧快照。
            let discovered_copy = restarting_root.join("copy/photo.jpg").is_file();
            snapshot_tx.send(()).unwrap();
            deleted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            restarting_state.with_scan_generation_write(1, generation, || {
                if discovered_copy {
                    let conn = restarting_state.db_writer.lock().unwrap();
                    conn.execute(
                        "INSERT INTO directories (root_id, rel_path, name) VALUES (?1, ?2, ?3)",
                        rusqlite::params![1, "copy", "copy"],
                    )
                    .unwrap();
                }
            })
        });
        attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let restarted_during_delete = snapshot_rx.recv_timeout(Duration::from_millis(150)).is_ok();
        release_tx.send(()).unwrap();
        deleting.join().unwrap().unwrap();
        deleted_tx.send(()).unwrap();
        assert!(restarting.join().unwrap().is_some());

        let conn = state.db_writer.lock().unwrap();
        assert!(
            q::find_directory_id(&conn, 1, "copy").unwrap().is_none(),
            "回收后的目录不能被删除期间启动的扫描重新写回"
        );
        drop(conn);
        assert!(!restarted_during_delete, "新扫描须等到回收及索引清理完成");
        assert!(old_cancel.is_cancelled());
        assert!(state
            .with_scan_generation_write(1, old_generation, || ())
            .is_none());
        assert_eq!(
            std::fs::read(tmp.path().join("recycled-copy/photo.jpg")).unwrap(),
            b"photo!"
        );
        assert!(!root.join("copy").exists());
    }

    #[test]
    fn unavailable_root_keeps_catalog_and_allows_directory_trash_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("copy")).unwrap();
        std::fs::write(root.join("copy/photo.jpg"), b"photo!").unwrap();
        let conn = seed_catalog(&root);
        let root_path = q::get_scan_root(&conn, 1).unwrap().path;
        let offline = tmp.path().join("offline");
        std::fs::rename(&root, &offline).unwrap();

        let result = delete_directory_copy_with(
            &root_path,
            "copy",
            |_| panic!("扫描根不可用时不得触发回收"),
            || q::delete_directory_by_id(&conn, 10).map(|_| ()),
        );
        let metadata: Option<(i64, i64)> = conn
            .query_row(
                "SELECT directory_id, rating FROM media_items WHERE id=20",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap();
        assert_eq!(
            metadata,
            Some((10, 5)),
            "离线错误不能删掉可重试的目录与用户元数据"
        );
        assert!(result.is_err(), "应报根不可访问，不能把失败当成已完成");
        assert_eq!(
            std::fs::read(offline.join("copy/photo.jpg")).unwrap(),
            b"photo!"
        );

        std::fs::rename(&offline, &root).unwrap();
        let recycled = tmp.path().join("recycled-copy");
        delete_directory_copy_with(
            &root_path,
            "copy",
            |path| std::fs::rename(path, &recycled).map_err(AppError::Io),
            || q::delete_directory_by_id(&conn, 10).map(|_| ()),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(recycled.join("photo.jpg")).unwrap(),
            b"photo!"
        );
        assert!(q::find_directory_id(&conn, 1, "copy").unwrap().is_none());
    }
}

/// 撤销文件夹复制：先成功回收实体目录，再清除已登记目录与媒体；目标已不存在时可重复执行。
/// 根离线、路径越界或访问失败会保留数据库记录，供用户恢复访问后重试。
#[tauri::command]
pub async fn delete_directory_to_trash(
    abs_path: String,
    root_id: i64,
    rel_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 info 档:回收站 syscall(整树)+级联删除,真实 IO/DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:delete_directory_to_trash");
    // R1-3：回收站 syscall（整树入回收站，可达秒级）+ 级联删除 SQL 一并下沉 blocking。
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<()> {
        // 1. 物理：将文件夹移入系统回收站（可恢复，比硬删除更安全）。
        //
        // 路径源改为权威的 root_id + rel_path（与下方 DB 级联删同源），不再信任前端传入的裸
        // abs_path —— 那绕过了越界校验，可被诱导回收根目录之外的路径。
        // 解析仍校验 canonicalize + 授权边界，只有明确不存在的目标可跳过物理回收。
        delete_directory_to_trash_with(&state, root_id, &rel_path, |canonical| {
            trash::delete(canonical)
                .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    info!("delete_directory_to_trash: {abs_path} | 已将复制的文件夹移入回收站并清理数据库行");
    Ok(())
}
