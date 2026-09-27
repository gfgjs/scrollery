//! F4 增量聚类(仅增量)+ 派生字段对账。(T 线拆分自 queries.rs,原 faces.rs D 群;
//! 二拆自 faces/mod.rs facade,SQL/事务边界不变)。

use rusqlite::{params, Connection};

use crate::error::{AppError, Result};

use super::recompute_person_aggregates;

// ── 人脸聚类（Face Clustering，F4，仅增量）─────────────────────────────────────

/// 一个既有人物，已解码出增量最近质心匹配所需的字段。`cover_quality` 来自 LEFT JOIN
/// （`cover_face_id` 悬空时默认 0.0——`persons.cover_face_id` 无 FK 约束，见 schema）。
pub struct PersonRow {
    pub id: i64,
    pub centroid: Vec<u8>,
    pub face_count: i64,
    pub cover_face_id: i64,
    pub cover_quality: f32,
}

/// 加载所有已有质心的人物（即已有 ≥1 张脸的人物），供与新写入人脸做内存中最近质心匹配。
pub fn get_all_persons_for_clustering(
    conn: &Connection,
    model_name: &str,
) -> Result<Vec<PersonRow>> {
    let mut stmt = conn.prepare(
        // 只取本模型名册(Part4-T6):跨向量空间的质心绝不进入最近质心匹配。
        "SELECT p.id, p.centroid, p.face_count, p.cover_face_id, COALESCE(f.quality, 0.0)
         FROM persons p
         LEFT JOIN faces f ON f.id = p.cover_face_id
         WHERE p.centroid IS NOT NULL AND p.model_name = ?1",
    )?;
    let rows = stmt.query_map(params![model_name], |row| {
        Ok(PersonRow {
            id: row.get(0)?,
            centroid: row.get(1)?,
            face_count: row.get(2)?,
            cover_face_id: row.get(3)?,
            cover_quality: row.get(4)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 一张刚写入的人脸，已就绪可供增量聚类。
pub struct ClusterableFaceRow {
    pub id: i64,
    pub embedding: Vec<u8>,
    pub quality: f32,
}

/// 重新按 `item_ids` + `model_name` 取回刚写入的人脸——`batch_finish_face_items` 不返回自增 id，
/// 聚类按同样的键重新查询，而不是把 id 一路串回写入路径。
pub fn get_clusterable_faces(
    conn: &Connection,
    item_ids: &[i64],
    model_name: &str,
) -> Result<Vec<ClusterableFaceRow>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=item_ids.len()).map(|i| format!("?{i}")).collect();
    // 谓词收窄(2026-07-10 审查 F2/F3):只取「未归属 + 非用户移出 + 无负样本」的脸。
    // 对正常增量流是零影响(新脸恒 person_id=NULL/is_unassigned=0/无 rejection——新自增 id
    // 不可能有拒绝记录),但堵住两类误伤:① 分析运行中用户已改派/确认的脸不再被随后的
    // 聚类 flush 冲掉;② F3 孤儿对账复用本查询时,不会吸回用户移出的脸,也不会在增量
    // 空拒绝集下把被拒脸吸回原簇(带 rejection 的脸交给全量 recluster 按负样本正规处理)。
    let sql = format!(
        "SELECT id, embedding, quality FROM faces
          WHERE item_id IN ({}) AND model_name=?{}
            AND person_id IS NULL AND is_unassigned=0
            AND NOT EXISTS (SELECT 1 FROM face_rejections fr WHERE fr.face_id = faces.id)",
        placeholders.join(","),
        item_ids.len() + 1
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut refs: Vec<&dyn rusqlite::ToSql> = item_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    refs.push(&model_name);
    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok(ClusterableFaceRow {
            id: row.get(0)?,
            embedding: row.get(1)?,
            quality: row.get(2)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 硬删单个媒体项并连带对账人脸派生字段(2026-07-10 审查 F7):`DELETE FROM media_items`
/// 会 CASCADE 删 faces,但 persons.face_count/cover_face_id/centroid 不会自己重算——
/// 计数虚高、封面悬挂(前端裂头像)、照片删光的人物以旧计数挂墙。删前收集受影响
/// person,删后同事务 recompute(空簇按既有策略:未命名碎片删除、命名/忽略者留空槽)。
pub fn delete_media_item_hard(conn: &Connection, item_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let affected: Vec<i64> = {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT person_id FROM faces WHERE item_id=?1 AND person_id IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![item_id], |row| row.get::<_, i64>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    tx.execute("DELETE FROM media_items WHERE id=?1", params![item_id])?;
    for pid in affected {
        recompute_person_aggregates(&tx, pid)?;
    }
    tx.commit()?;
    Ok(())
}

/// 全库人脸计数对账(2026-07-10 审查 F7 兜底半边,幂等):凡 `persons.face_count` 与实际
/// 引用数不符者按库内真相 recompute。捕获任何绕过 [`delete_media_item_hard`] 的删除路径
/// (历史遗留/扫描端裁剪等)累积的陈旧派生字段;face_pipeline 启动时调用。返回修复数。
pub fn reconcile_person_face_counts(conn: &Connection) -> Result<usize> {
    let stale: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT p.id FROM persons p
             LEFT JOIN (SELECT person_id, COUNT(*) AS n FROM faces
                         WHERE person_id IS NOT NULL GROUP BY person_id) f
               ON f.person_id = p.id
             WHERE COALESCE(f.n, 0) <> p.face_count",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    if stale.is_empty() {
        return Ok(0);
    }
    let n = stale.len();
    let tx = conn.unchecked_transaction()?;
    for pid in &stale {
        recompute_person_aggregates(&tx, *pid)?;
    }
    tx.commit()?;
    Ok(n)
}

/// 孤儿未聚类脸的宿主 item 集(2026-07-10 审查 F3):face_status=Done 而脸仍未归属、
/// 且**并非**「低质量有意不聚 / 用户移出 / 有负样本」的项。这些脸只可能来自崩溃丢
/// cluster_pending(或历史上的 F2 丢批),增量聚类按本轮 item_ids 取件永远不会再碰它们,
/// 旧实现的唯一救济是销毁性的全量 recluster。face_pipeline 启动时据此对账(幂等:归簇
/// 后不再命中)。
pub fn get_orphan_cluster_item_ids(
    conn: &Connection,
    model_name: &str,
    min_quality: f32,
) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT f.item_id
           FROM faces f
           JOIN media_items m ON m.id = f.item_id
          WHERE f.model_name=?1 AND f.person_id IS NULL AND f.is_unassigned=0
            AND f.quality >= ?2
            AND m.face_status=2 AND m.is_deleted=0
            AND NOT EXISTS (SELECT 1 FROM face_rejections fr WHERE fr.face_id = f.id)",
    )?;
    let rows = stmt.query_map(params![model_name, min_quality as f64], |row| row.get(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 本次刷新聚类涉及的一个人物：`id < 0` 是未落库的占位（插入后解析真实 id）；`id > 0` 是要原地
/// 更新的既有人物。`face_ids` 列出本批归入它的所有脸——人物行就位（新人物 id 已知）后用于设置
/// `faces.person_id`。
pub struct PersonClusterUpdate {
    pub id: i64,
    pub centroid: Vec<u8>,
    pub face_count: i64,
    pub cover_face_id: i64,
    pub face_ids: Vec<i64>,
}

/// 在单个事务中应用增量聚类决策：插入新人物（经 `last_insert_rowid` 解析真实 id）、更新既有
/// 人物的质心/计数/封面，然后为每条更新里的所有脸设置 `faces.person_id`。
pub fn apply_face_clusters(
    conn: &Connection,
    model_name: &str,
    updates: &[PersonClusterUpdate],
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for u in updates {
        // F2 并发防御(2026-07-10 审查):增量聚类是「快照读 → 内存决策 → 短写」,与
        // merge/reassign/unassign 等人工命令之间**没有互斥**。两个具体病灶在此闭合:
        // ① 快照后人物被 merge 删除 → 旧实现 UPDATE persons 影响 0 行仍照改派 faces
        //   → FK 违约 → 整批(≤512 项)事务回滚静默丢失。现改为把这些脸转挂新占位人物。
        // ② 快照后人工命令做过精确重算 → 内存滑动均值/旧计数是陈旧值,整行覆盖即
        //   lost update。现把 UPDATE 条件化在「face_count 仍等于快照推导值」上,不符即
        //   放弃内存值、改派后按库内真相重算(recompute 只在真冲突时付费)。
        let (person_id, recompute) = if u.id < 0 {
            // 新人物随其向量空间落 model_name(Part4-T6:名册按模型隔离)。
            tx.execute(
                "INSERT INTO persons (cover_face_id, centroid, face_count, model_name) VALUES (?1, ?2, ?3, ?4)",
                params![u.cover_face_id, u.centroid, u.face_count, model_name],
            )?;
            (tx.last_insert_rowid(), false)
        } else {
            // 快照时的旧 face_count = 新 face_count − 本批归入数(滑动均值的推导前值)。
            let expected_old = u.face_count - u.face_ids.len() as i64;
            let n = tx.execute(
                "UPDATE persons SET centroid=?1, face_count=?2, cover_face_id=?3, updated_at=strftime('%s','now')
                 WHERE id=?4 AND face_count=?5",
                params![u.centroid, u.face_count, u.cover_face_id, u.id, expected_old],
            )?;
            if n == 1 {
                (u.id, false)
            } else {
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM persons WHERE id=?1",
                    params![u.id],
                    |row| row.get(0),
                )?;
                if exists == 0 {
                    // ① 人物已被删除:脸转挂新占位(质心/封面先用内存值占位,随后按真相重算)。
                    tx.execute(
                        "INSERT INTO persons (cover_face_id, centroid, face_count, model_name) VALUES (?1, ?2, ?3, ?4)",
                        params![u.cover_face_id, u.centroid, u.face_ids.len() as i64, model_name],
                    )?;
                    (tx.last_insert_rowid(), true)
                } else {
                    // ② 人物被并发修改:放弃陈旧的内存值,改派后按库内真相重算。
                    (u.id, true)
                }
            }
        };
        for &face_id in &u.face_ids {
            // 归簇即清「用户移出」位(F3 判别位契约;增量路径的新脸恒为 0,此处是保险)。
            tx.execute(
                "UPDATE faces SET person_id=?1, is_unassigned=0 WHERE id=?2",
                params![person_id, face_id],
            )?;
        }
        if recompute {
            recompute_person_aggregates(&tx, person_id)?;
        }
    }
    tx.commit()?;
    Ok(())
}
