//! F3 人脸检测队列状态机:待检测项查询、状态位批量更新/复位、按模型 reset/sync
//! (T 线拆分自 queries.rs,原 faces.rs A 群;二拆自 faces/mod.rs facade,SQL/事务边界不变)。

use rusqlite::{params, Connection};

// 共用 helper 定向引用(§4.3):错误复位归 ai 域、exotic 门控谓词归 exotic 域、隐藏根排除归 scan 域。
use crate::db::queries::ai::reset_error_items_batched;
use crate::db::queries::exotic::{NOT_BLOCKED_BY_EXOTIC, NOT_BLOCKED_BY_EXOTIC_M};
use crate::db::queries::scan::{EXCLUDE_HIDDEN_ROOTS, EXCLUDE_HIDDEN_ROOTS_M};
use crate::error::{AppError, Result};

/// `face_status=3`(Error)项数(2026-07-10 审查 A11/F9)。
pub fn count_error_face_items(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM media_items WHERE face_status=3 AND is_deleted=0 AND media_type='image'",
        [],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 非破坏重试(F9,face 侧):Error → Pending。对照:此前唯一恢复手段是**销毁全部
/// 人物命名与确认**的 restart_face_analysis——一批图因临时原因(卷离线/解码抽风)标
/// Error,补跑代价=全部标注劳动。本函数零破坏。
pub fn reset_error_face_items(db: &std::sync::Mutex<Connection>) -> Result<usize> {
    reset_error_items_batched(db, "face_status", 10_000)
}

// ── 人脸识别（Face Recognition，F3）─────────────────────────────────────────

/// One pending image awaiting face detection — same decode-source hints as `PendingAiItem`.
/// `cache_key` addresses the face-specific 640px cache (`face_thumbs/`), NOT the 336px CLIP
/// `ai_thumbs/` (still never used here — see `ai::face_pipeline` module header for why).
/// 一个待人脸检测的图像项 —— 解码源提示与 `PendingAiItem` 相同。`cache_key` 用于寻址 face
/// 专属的 640px 缓存(`face_thumbs/`,T16-R2 方案 A),而非 336px 的 CLIP `ai_thumbs/`
/// (后者对 YuNet 输入太小,依旧不用——原因见 `ai::face_pipeline` 模块头)。
pub struct PendingFaceItem {
    pub id: i64,
    pub abs_path: String,
    pub file_format: String,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    pub width: i64,
    pub height: i64,
    pub cache_key: i64,
}

/// 取一批待人脸检测的图像（`face_status=0`）。
pub fn get_pending_face_items(conn: &Connection, limit: i64) -> Result<Vec<PendingFaceItem>> {
    let sql = format!(
        "SELECT m.id,
                CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                     ELSE r.path || '/' || d.rel_path || '/' || m.file_name
                END,
                m.file_format, m.thumb_status, m.thumb_path, m.width, m.height, m.cache_key
         FROM media_items m
         JOIN directories d ON m.directory_id = d.id
         JOIN scan_roots r ON d.root_id = r.id
         WHERE m.face_status=0 AND m.is_deleted=0 AND m.media_type='image' {NOT_BLOCKED_BY_EXOTIC_M} {EXCLUDE_HIDDEN_ROOTS_M}
         ORDER BY m.created_at DESC
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(PendingFaceItem {
            id: row.get(0)?,
            abs_path: row.get(1)?,
            file_format: row.get(2)?,
            thumb_status: row.get(3)?,
            thumb_path: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            cache_key: row.get(7)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 统计待人脸检测的项数量。
pub fn count_pending_face_items(conn: &Connection) -> Result<i64> {
    let sql = format!(
        "SELECT COUNT(*) FROM media_items
         WHERE face_status=0 AND is_deleted=0 AND media_type='image' {NOT_BLOCKED_BY_EXOTIC} {EXCLUDE_HIDDEN_ROOTS}"
    );
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(AppError::from)
}

/// 批量更新多个媒体项的 `face_status`。
pub fn batch_update_face_status(conn: &Connection, item_ids: &[i64], status: i64) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for &id in item_ids {
        tx.execute(
            "UPDATE media_items SET face_status=?1, updated_at=strftime('%s','now') WHERE id=?2",
            params![status, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 释放人脸流水线已领取（`face_status`=Processing）但未完成的项——镜像 `reset_processing_ai_items`（问题7）。
pub fn reset_processing_face_items(conn: &Connection) -> Result<usize> {
    conn.execute(
        "UPDATE media_items SET face_status=0 WHERE face_status=1 AND media_type='image'",
        [],
    )
    .map_err(AppError::from)
}

/// 统计已处理完成的人脸项——`face_status IN (2,3)`，即 完成 或 错误。刻意把错误项也算作
/// "已处理"（不同于 CLIP 基于 `count_embeddings` 的进度），使部分图解码失败时进度条仍能到 100%。
pub fn count_processed_face_items(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM media_items WHERE face_status IN (2,3) AND is_deleted=0 AND media_type='image'",
        [],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 统计某模型已聚类的人物数(人物墙的名册规模;Part4-T6 按模型隔离)。
pub fn count_persons(conn: &Connection, model_name: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM persons WHERE model_name=?1",
        params![model_name],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 统计某模型下已存的人脸数（跨所有图像）。
pub fn count_faces_for_model(conn: &Connection, model_name: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM faces WHERE model_name=?1",
        params![model_name],
        |row| row.get(0),
    )
    .map_err(AppError::from)
}

/// 按模型重置人脸数据以全量重来:删除该模型的 faces 与该模型的 persons(Part4-T6
/// `persons.model_name` 隔离——其他模型的名册是用户资产,保留不删),并把 `face_status`
/// 重置为待处理,使流水线全量重跑。警告仍会销毁**本模型**的用户劳动:其已命名人物与
/// `is_confirmed` 指派将丢失(调用方须提示用户)。
///
/// R2-6 分批化(签名改 Mutex 的理由同 reset_ai_embeddings):faces→persons 两个 DELETE
/// 保持单事务(FK 顺序;persons 行数小,不构成长事务),仅 face_status 全表 UPDATE 分批。
pub fn reset_face_data(db: &std::sync::Mutex<Connection>, model_name: &str) -> Result<()> {
    reset_face_data_batched(db, model_name, 10_000)
}

fn reset_face_data_batched(
    db: &std::sync::Mutex<Connection>,
    model_name: &str,
    batch: i64,
) -> Result<()> {
    {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM faces WHERE model_name=?1", params![model_name])?;
        tx.execute(
            "DELETE FROM persons WHERE model_name=?1",
            params![model_name],
        )?;
        // V17:重扫语义连带销账——覆盖行不删,后续 sync 会把这些图误标回 Done(账在人不在)。
        tx.execute(
            "DELETE FROM face_coverage WHERE model_name=?1",
            params![model_name],
        )?;
        tx.commit()?;
    }
    loop {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            "UPDATE media_items SET face_status=0, updated_at=strftime('%s','now')
             WHERE rowid IN (SELECT rowid FROM media_items
                             WHERE media_type='image' AND face_status<>0 LIMIT ?1)",
            params![batch],
        )?;
        if (n as i64) < batch {
            break;
        }
    }
    Ok(())
}

/// 把全局 `face_status` 重新指向 `model_name` 的扫描覆盖(仿 `sync_ai_status_for_model`):
/// 有该模型 `face_coverage` 行的项 → 已完成(2),其余 → 待处理(0)。已同步行零写;分批执行。
///
/// V17(2026-07-11 加固批 B-3)语义修正:覆盖真相从「有无 faces 行」改为 `face_coverage`
/// 记账表——零检出图也有账,切轨/流水线启动 sync 不再把无脸图误归 0 全量重扫(X2 撤销的
/// 病灶就此关死);由此本函数可安全用于流水线启动自愈(A3/F11 竞态误标的无损承接)。
pub fn sync_face_status_for_model(
    db: &std::sync::Mutex<Connection>,
    model_name: &str,
) -> Result<()> {
    sync_face_status_batched(db, model_name, 10_000)
}

fn sync_face_status_batched(
    db: &std::sync::Mutex<Connection>,
    model_name: &str,
    batch: i64,
) -> Result<()> {
    loop {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            "UPDATE media_items SET
                face_status = CASE
                    WHEN id IN (SELECT item_id FROM face_coverage WHERE model_name=?1) THEN 2
                    ELSE 0 END,
                updated_at = strftime('%s','now')
             WHERE rowid IN (
                 SELECT rowid FROM media_items
                 WHERE media_type='image' AND is_deleted=0
                   AND face_status <> (CASE WHEN id IN
                        (SELECT item_id FROM face_coverage WHERE model_name=?1) THEN 2 ELSE 0 END)
                 LIMIT ?2)",
            params![model_name, batch],
        )?;
        if (n as i64) < batch {
            break;
        }
    }
    Ok(())
}

/// T18 S0：`view_to_sql` 编译器快照测试。不依赖 DB —— 断言各 scope/filter 组合编译出的 SQL
/// 片段与参数个数符合预期，间接锁住 `push_query_body`（与 `query_layout_items` 共用）的行为。
#[cfg(test)]
mod r2_6_query_tests {
    use std::sync::Mutex;

    use super::*;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // root1 → A(顶层) → A/B(子);C(顶层,无子)。
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name, depth) VALUES
                 (10, 1, NULL, 'A', 'A', 0),
                 (11, 1, 10, 'A/B', 'B', 1),
                 (12, 1, NULL, 'C', 'C', 0);",
        )
        .unwrap();
        c
    }

    #[allow(clippy::too_many_arguments)]
    fn add_item(
        c: &Connection,
        id: i64,
        dir: i64,
        mtype: &str,
        fav: i64,
        del: i64,
        live: i64,
        companion: Option<i64>,
    ) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format, media_type,
                 width, height, sort_datetime, cache_key, is_favorited, is_deleted,
                 is_live_photo, companion_of)
             VALUES (?1, ?2, ?3, 0, 0, 'jpg', ?4, 0, 0, 0, 0, ?5, ?6, ?7, ?8)",
            params![
                id,
                dir,
                format!("{id}.jpg"),
                mtype,
                fav,
                del,
                live,
                companion
            ],
        )
        .unwrap();
    }

    /// face reset 分批:face_status 循环清零(batch=1 迫使多轮),非 image 不动。
    #[test]
    fn reset_face_data_batched_clears_face_status() {
        let c = mem_db();
        for id in 1..=3 {
            add_item(&c, id, 10, "image", 0, 0, 0, None);
        }
        add_item(&c, 9, 10, "video", 0, 0, 0, None);
        c.execute_batch("UPDATE media_items SET face_status=2 WHERE id IN (1,2,3,9);")
            .unwrap();

        let db = Mutex::new(c);
        super::reset_face_data_batched(&db, "yunet-sface", 1).unwrap();

        let c = db.lock().unwrap();
        let fs = |id: i64| -> i64 {
            c.query_row(
                "SELECT face_status FROM media_items WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(fs(1), 0);
        assert_eq!(fs(2), 0);
        assert_eq!(fs(3), 0);
        assert_eq!(fs(9), 2, "非 image 不动");
    }

    /// T_t3(Part4-T6)+ V17 改义(2026-07-11 加固批 B-3):sync_face_status 把全局
    /// face_status 指向目标模型的 **face_coverage 记账**——有账→2(含零脸图!)、
    /// 无账→0、只有他模型账→0(向量空间不同不算覆盖);软删行不动。
    /// 零脸图有账即 Done 是 V17 的核心改义:X2 时代按 faces 行判定会把它们误归 0 重扫。
    #[test]
    fn sync_face_status_batched_mirrors_model_coverage() {
        let c = mem_db();
        add_item(&c, 1, 10, "image", 0, 0, 0, None); // m1 有脸有账,face=0 → 2
        add_item(&c, 2, 10, "image", 0, 0, 0, None); // m1 零脸有账,face=0 → 2(V17 关键例)
        add_item(&c, 3, 10, "image", 0, 0, 0, None); // 仅 m2 有账,face=2 → 0
        add_item(&c, 4, 10, "image", 0, 1, 0, None); // 软删,不动
        add_item(&c, 5, 10, "image", 0, 0, 0, None); // 无任何账但误标 2(A3/F11 竞态残留)→ 0
        c.execute_batch(
            "INSERT INTO faces (id, item_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h,
                                det_score, quality, embedding, is_confirmed)
             VALUES (1, 1, 'm1', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, X'0000803F', 0);
             INSERT INTO face_coverage (item_id, model_name)
             VALUES (1, 'm1'), (2, 'm1'), (3, 'm2');
             UPDATE media_items SET face_status=2 WHERE id IN (3,4,5);",
        )
        .unwrap();

        let db = Mutex::new(c);
        super::sync_face_status_batched(&db, "m1", 1).unwrap();

        let c = db.lock().unwrap();
        let fs = |id: i64| -> i64 {
            c.query_row(
                "SELECT face_status FROM media_items WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(fs(1), 2, "有本模型账(有脸)→ Done");
        assert_eq!(fs(2), 2, "有本模型账(零脸)→ Done——V17 前会被误归 0 重扫");
        assert_eq!(fs(3), 0, "他模型账不算本模型覆盖");
        assert_eq!(fs(4), 2, "软删行不参与同步");
        assert_eq!(
            fs(5),
            0,
            "无账误标 Done → 归 0 自愈(启动 sync 的 A3/F11 承接)"
        );
    }

    /// T_t3(Part4-T6):reset 按模型隔离——目标模型 faces+persons 删净,他模型名册保留。
    #[test]
    fn reset_face_data_keeps_other_models_roster() {
        let c = mem_db();
        add_item(&c, 1, 10, "image", 0, 0, 0, None);
        c.execute_batch(
            "INSERT INTO persons (id, name, model_name, centroid, face_count)
             VALUES (1, NULL, 'm1', NULL, 1), (2, NULL, 'm2', NULL, 1);
             INSERT INTO faces (id, item_id, person_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h,
                                det_score, quality, embedding, is_confirmed)
             VALUES (1, 1, 1, 'm1', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, X'0000803F', 0),
                    (2, 1, 2, 'm2', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, X'0000803F', 0);
             INSERT INTO face_coverage (item_id, model_name) VALUES (1, 'm1'), (1, 'm2');",
        )
        .unwrap();

        let db = Mutex::new(c);
        super::reset_face_data_batched(&db, "m1", 10).unwrap();

        let c = db.lock().unwrap();
        let n = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(n("SELECT COUNT(*) FROM faces WHERE model_name='m1'"), 0);
        assert_eq!(
            n("SELECT COUNT(*) FROM faces WHERE model_name='m2'"),
            1,
            "他模型脸保留"
        );
        assert_eq!(n("SELECT COUNT(*) FROM persons WHERE model_name='m1'"), 0);
        assert_eq!(
            n("SELECT COUNT(*) FROM persons WHERE model_name='m2'"),
            1,
            "他模型名册保留(用户资产)"
        );
        // V17:重扫销账按模型隔离——m1 账清净,m2 账保留(否则 sync 会把重扫图误翻回 Done)。
        assert_eq!(
            n("SELECT COUNT(*) FROM face_coverage WHERE model_name='m1'"),
            0,
            "重扫模型的覆盖账清净"
        );
        assert_eq!(
            n("SELECT COUNT(*) FROM face_coverage WHERE model_name='m2'"),
            1,
            "他模型覆盖账保留"
        );
    }

    /// T_t3(Part4-T6):增量聚类新建人物随向量空间落 model_name;全量重建的碎片清理只
    /// 触达本模型——他模型的空名册人物不被误删。
    #[test]
    fn cluster_writes_stamp_model_and_rebuild_scoped() {
        let c = mem_db();
        add_item(&c, 1, 10, "image", 0, 0, 0, None);
        c.execute_batch(
            "INSERT INTO persons (id, name, model_name, centroid, face_count)
             VALUES (7, NULL, 'm2', NULL, 0);
             INSERT INTO faces (id, item_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h,
                                det_score, quality, embedding, is_confirmed)
             VALUES (1, 1, 'm1', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, X'0000803F', 0);",
        )
        .unwrap();

        // 增量:新人物(id<0 占位)落 m1。
        let upd = crate::db::queries::PersonClusterUpdate {
            id: -1,
            centroid: vec![0, 0, 128, 63],
            face_count: 1,
            cover_face_id: 1,
            face_ids: vec![1],
        };
        crate::db::queries::apply_face_clusters(&c, "m1", &[upd]).unwrap();
        let model: String = c
            .query_row(
                "SELECT model_name FROM persons WHERE id=(SELECT person_id FROM faces WHERE id=1)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "m1", "增量新建人物落其向量空间");

        // 全量重建 m1(空 updates → m1 未命名人物全按碎片清理);m2 的空名册人物保留。
        crate::db::queries::rebuild_person_clusters(&c, "m1", &[]).unwrap();
        let m2: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM persons WHERE model_name='m2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(m2, 1, "他模型名册不被重建碎片清理误删");
        let m1: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM persons WHERE model_name='m1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(m1, 0, "本模型未命名空人物按碎片清走");
    }
}
