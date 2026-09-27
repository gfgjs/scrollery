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
