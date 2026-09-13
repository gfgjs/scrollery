//! 全量重聚类(显式命令,非增量)。(T 线拆分自 queries.rs,原 faces.rs E 群;
//! 二拆自 faces/mod.rs facade,SQL/事务边界不变)。

use rusqlite::{params, Connection};

use crate::error::{AppError, Result};

use super::recompute_person_aggregates;
use crate::db::queries::PersonClusterUpdate;

// ── 全量重新聚类（显式命令，非增量）────────────────────────────────────────────
// ── Full re-clustering (explicit command, not the incremental flush) ─────────

/// 一个既有人物，为全量重聚类解码。带 `is_named`/`is_ignored` 以便重建保留"锚定"人物，并用已命名
/// 人物的库存 `centroid` 作种子（即便它没有已确认/锁定脸也仍能吸附成员）。`centroid` 为空表示列为 NULL。
pub struct PersonReclusterRow {
    pub id: i64,
    pub is_named: bool,
    pub is_ignored: bool,
    pub centroid: Vec<u8>,
}

/// 加载全部人物（含标志位 + 库存质心），供全量重聚类重建。
pub fn get_persons_for_recluster(
    conn: &Connection,
    model_name: &str,
) -> Result<Vec<PersonReclusterRow>> {
    let mut stmt =
        conn.prepare("SELECT id, is_named, is_ignored, centroid FROM persons WHERE model_name=?1")?;
    let rows = stmt.query_map(params![model_name], |row| {
        let centroid: Option<Vec<u8>> = row.get(3)?;
        Ok(PersonReclusterRow {
            id: row.get(0)?,
            is_named: row.get::<_, i64>(1)? != 0,
            is_ignored: row.get::<_, i64>(2)? != 0,
            centroid: centroid.unwrap_or_default(),
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 全量重聚类用的一张脸：带当前 `person_id` + `is_confirmed`，使重建能锁定已确认脸与落在"忽略"
/// 桶里的脸（永不移动它们）。
pub struct ReclusterFaceRow {
    pub id: i64,
    pub person_id: Option<i64>,
    pub embedding: Vec<u8>,
    pub quality: f32,
    pub is_confirmed: bool,
}

/// 加载 `model_name` 下的全部人脸，供全量重聚类重建。
pub fn get_all_faces_for_recluster(
    conn: &Connection,
    model_name: &str,
) -> Result<Vec<ReclusterFaceRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, person_id, embedding, quality, is_confirmed FROM faces WHERE model_name=?1",
    )?;
    let rows = stmt.query_map(params![model_name], |row| {
        Ok(ReclusterFaceRow {
            id: row.get(0)?,
            person_id: row.get(1)?,
            embedding: row.get(2)?,
            quality: row.get::<_, f64>(3)? as f32,
            is_confirmed: row.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 加载 `model_name` 下人脸的 `(face_id, person_id)` 负样本——用户经 `reject_face_candidate` 拒绝的对。
/// 全量重聚类查阅它，确保被拒脸绝不被重新吸回它被拒绝的那个 person（Part4 T3 StageB / §3.5.1）。
/// JOIN `faces` 以仅返回该模型的拒绝对（face id 按模型唯一）。
pub fn get_face_rejections(conn: &Connection, model_name: &str) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT fr.face_id, fr.person_id
         FROM face_rejections fr
         JOIN faces f ON f.id = fr.face_id
         WHERE f.model_name = ?1",
    )?;
    let rows = stmt.query_map(params![model_name], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// Apply a full re-clustering rebuild in ONE transaction:
/// 1. detach every face of this model (`person_id=NULL`) — clean slate;
/// 2. re-apply the computed `updates` (id<0 INSERT new UNNAMED person; id>0 UPDATE centroid/
///    face_count/cover **but NEVER touch name/is_named/is_hidden/is_ignored**) + set each listed
///    face's `person_id` (this re-attaches pinned faces too — they MUST be in some update);
/// 3. delete UNNAMED, non-ignored persons that ended up referenced by no face (fragmentation
///    cleanup). Named and ignored persons survive even if empty (preserve user labor).
///
/// 在单事务中应用全量重聚类：①把该模型所有脸解绑（`person_id=NULL`）做白板；②重放算出的 `updates`
///（id<0 插入新**未命名**人物；id>0 更新质心/计数/封面，**绝不动 name/is_named/is_hidden/is_ignored**）
/// 并回填每张脸的 `person_id`（锁定脸也借此重新挂回——故它们必须出现在某条 update 里）；③删除最终无任
/// 何脸指向的**未命名**非忽略人物（清碎片化遗留）。已命名/忽略人物即便空也保留（护用户劳动）。
pub fn rebuild_person_clusters(
    conn: &Connection,
    model_name: &str,
    updates: &[PersonClusterUpdate],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE faces SET person_id=NULL WHERE model_name=?1",
        params![model_name],
    )?;
    for u in updates {
        let person_id = if u.id < 0 {
            // 同 apply_face_clusters:新人物落本模型名册(Part4-T6)。
            tx.execute(
                "INSERT INTO persons (cover_face_id, centroid, face_count, model_name) VALUES (?1, ?2, ?3, ?4)",
                params![u.cover_face_id, u.centroid, u.face_count, model_name],
            )?;
            tx.last_insert_rowid()
        } else {
            tx.execute(
                "UPDATE persons SET centroid=?1, face_count=?2, cover_face_id=?3, updated_at=strftime('%s','now')
                 WHERE id=?4",
                params![u.centroid, u.face_count, u.cover_face_id, u.id],
            )?;
            u.id
        };
        for &face_id in &u.face_ids {
            // 归簇即清「用户移出」位(F3;全量重建吸附 free 脸是既定语义)。
            tx.execute(
                "UPDATE faces SET person_id=?1, is_unassigned=0 WHERE id=?2",
                params![person_id, face_id],
            )?;
        }
    }
    // 清碎片：不再被任何脸引用的"未命名非忽略"人物。子查询已滤 NULL，故 NOT IN 安全。
    // 限定本模型(Part4-T6):其他模型的名册不参与本次重建,不得被当碎片误删。
    tx.execute(
        "DELETE FROM persons WHERE is_named=0 AND is_ignored=0 AND model_name=?1
         AND id NOT IN (SELECT person_id FROM faces WHERE person_id IS NOT NULL)",
        params![model_name],
    )?;
    // 尾部对账(2026-07-10 审查 F6):白板+重放只回填 updates 内的人物——named/ignored 者
    // 既无 pinned 脸又零吸附时不在 updates,行存活但 face_count/cover_face_id 是重建前旧值
    // (人物墙「张三 · 12 张」点进去 0 张,封面指向已解绑、可能已归他人的脸)。此处对
    // 本模型未被 updates 覆盖的 named/ignored 者按库内真相补算(归零者清 cover/centroid,
    // recompute 的空槽策略与碎片清理策略一致)。
    {
        let updated: std::collections::HashSet<i64> =
            updates.iter().filter(|u| u.id > 0).map(|u| u.id).collect();
        let flagged: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM persons WHERE model_name=?1 AND (is_named=1 OR is_ignored=1)",
            )?;
            let rows = stmt.query_map(params![model_name], |row| row.get::<_, i64>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for pid in flagged {
            if !updated.contains(&pid) {
                recompute_person_aggregates(&tx, pid)?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}
