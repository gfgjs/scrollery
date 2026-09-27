//! Part4 T3 / §3.5.1 批量审批命令:确认/改派/移出/拒绝/建人 + likely-match 列表 +
//! face 写入(insert/finish/guarded)。(T 线拆分自 queries.rs,原 faces.rs C 群;
//! 二拆自 faces/mod.rs facade,SQL/事务边界不变)。

use rusqlite::{params, Connection};

use crate::error::{AppError, Result};

use super::{in_clause, recompute_person_aggregates};

// ── 人脸批量审批（Part4 T3 / §3.5.1）───────────────────────────────────────────
// ── Face batch approval (Part4 T3 / §3.5.1) ──────────────────────────────────
//
// 这些命令让用户校正聚类结果（确认/改派/移出/拒绝/建人），并写入 `faces.is_confirmed`
// （recluster 锁定不打散）与 `face_rejections`（负样本）。每个改动 person 归属的命令都在同一
// 事务内连带重算受影响 person 的派生字段（质心/封面/计数），避免计数与质心陈旧（§3.5.1a）。
// These commands let the user correct clustering (confirm/reassign/unassign/reject/create) by
// writing `faces.is_confirmed` (pinned across recluster) and `face_rejections` (negative samples).
// Every command that changes a person's membership recomputes the affected persons' derived
// fields (centroid/cover/count) in the SAME transaction, so counts/centroids never go stale.

/// 收集 `face_ids` 当前所属的去重非空 `person_id` —— 这些脸移动/离开后须重算其派生字段的 person 集。
fn persons_of_faces(conn: &Connection, face_ids: &[i64]) -> Result<Vec<i64>> {
    if face_ids.is_empty() {
        return Ok(Vec::new());
    }
    let ph: Vec<String> = (1..=face_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT DISTINCT person_id FROM faces WHERE id IN ({}) AND person_id IS NOT NULL",
        ph.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = face_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(refs.as_slice(), |row| row.get::<_, i64>(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 确认（锁定）这些脸的当前归属：`is_confirmed=1`。重聚类不再移动它们（见 `face_cluster` 的
/// `is_pinned`）。纯锁定——不改归属，故无须重算质心。
pub fn confirm_face_assignment(conn: &Connection, face_ids: &[i64]) -> Result<()> {
    if face_ids.is_empty() {
        return Ok(());
    }
    let (ph, refs) = in_clause(face_ids);
    let sql = format!("UPDATE faces SET is_confirmed=1 WHERE id IN ({ph})");
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

/// 手动把 `face_ids` 改派给 `person_id` 并锁定（`is_confirmed=1`）——用户纠正聚类错误。🔴 同模型
/// 守卫（§3.5.1a）：每张被改派脸的 `model_name` 必须等于目标 person 的 `model_name`，否则会把 128
/// 维脸塞进 512 维 person、质心重算混维（panic/算错）——拒绝。在同一事务内重算目标 + 所有源 person。
pub fn reassign_face_to_person(conn: &Connection, face_ids: &[i64], person_id: i64) -> Result<()> {
    if face_ids.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;

    // 目标 person 的 model_name（不存在 → QueryReturnedNoRows 经 ? 透出 Db 错误）。
    let target_model: String = tx.query_row(
        "SELECT model_name FROM persons WHERE id=?1",
        params![person_id],
        |row| row.get(0),
    )?;

    // 同模型守卫：任一被改派脸的 model_name 与目标不符即拒绝（防混维质心）。
    let (ph, mut refs) = in_clause(face_ids);
    let mismatch_sql = format!(
        "SELECT COUNT(*) FROM faces WHERE id IN ({ph}) AND model_name <> ?{}",
        face_ids.len() + 1
    );
    refs.push(&target_model);
    let mismatch: i64 = tx.query_row(&mismatch_sql, refs.as_slice(), |row| row.get(0))?;
    if mismatch > 0 {
        return Err(AppError::Internal(
            "跨模型改派被拒：人脸与目标人物的模型不一致 | cross-model reassign rejected".into(),
        ));
    }

    // 改派前先取源 person 集（改派后这些脸已不在源下）。
    let mut affected = persons_of_faces(&tx, face_ids)?;

    // 🔴 占位符顺序：IN 子句先占 ?1..?k，绑定的 person_id 放末位 ?{k+1}——否则 `person_id=?1 …
    // IN (?1)` 会与 IN 占位冲突（rusqlite 按最大索引计数，给 2 个值却只认 1 个 → InvalidParameterCount）。
    let (ph2, refs2) = in_clause(face_ids);
    // is_unassigned=0:归位即清「用户移出」判别位(F3)。
    let upd_sql = format!(
        "UPDATE faces SET person_id=?{}, is_confirmed=1, is_unassigned=0 WHERE id IN ({ph2})",
        face_ids.len() + 1
    );
    let mut upd_refs = refs2;
    upd_refs.push(&person_id);
    tx.execute(&upd_sql, upd_refs.as_slice())?;

    // 重算目标 + 所有源 person（去重，含目标）。
    if !affected.contains(&person_id) {
        affected.push(person_id);
    }
    for pid in affected {
        recompute_person_aggregates(&tx, pid)?;
    }

    tx.commit()?;
    Ok(())
}

/// 移出 `face_ids`（误检/非人脸或归错）：`person_id=NULL` 且 `is_confirmed=0`。🔴 必须清
/// `is_confirmed`（§3.5.1a）：残留 `is_confirmed=1 + person_id=NULL` 会让下次重聚类把它当 free
/// 锚脸重新吸附，等于悄悄撤销 unassign。在同一事务内重算所有源 person。
pub fn unassign_face(conn: &Connection, face_ids: &[i64]) -> Result<()> {
    if face_ids.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let affected = persons_of_faces(&tx, face_ids)?;
    let (ph, refs) = in_clause(face_ids);
    // is_unassigned=1(2026-07-10 审查 F3 判别位):使孤儿对账能把「用户移出」与
    // 「崩溃丢聚类」区分开,不把用户刚移出的脸自动吸回。
    let sql = format!(
        "UPDATE faces SET person_id=NULL, is_confirmed=0, is_unassigned=1 WHERE id IN ({ph})"
    );
    tx.execute(&sql, refs.as_slice())?;
    for pid in affected {
        recompute_person_aggregates(&tx, pid)?;
    }
    tx.commit()?;
    Ok(())
}

/// Record "these faces are NOT `person_id`" negative samples in `face_rejections` AND remove the
/// faces from that person right now (`person_id=NULL`, `is_confirmed=0` if they were assigned to
/// it). The rejection rows are what a later full-recluster consults to skip already-rejected
/// (face, person) pairs (Stage B; prevents a near centroid from re-attracting them). Recomputes
/// the rejected person's aggregates. `INSERT OR IGNORE` makes repeat rejections idempotent.
///
/// 在 `face_rejections` 记录「这些脸不是 `person_id`」负样本，并立即把（当前归在该 person 的）脸
/// 移出（`person_id=NULL`、`is_confirmed=0`）。负样本行供后续全量重聚类查阅以跳过已拒绝
///（face, person）对（Stage B；防相近质心反复吸附）。重算该 person 的派生字段。`INSERT OR IGNORE`
/// 使重复拒绝幂等。
pub fn reject_face_candidate(conn: &Connection, face_ids: &[i64], person_id: i64) -> Result<()> {
    if face_ids.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;

    // 1. 记负样本（幂等）。
    for &fid in face_ids {
        tx.execute(
            "INSERT OR IGNORE INTO face_rejections (face_id, person_id) VALUES (?1, ?2)",
            params![fid, person_id],
        )?;
    }

    // 2. 当前归在该 person 的被拒脸立即移出（仅限确实归在它的，避免误清其它 person 的脸）。
    //    is_unassigned=1:同 unassign,孤儿对账不得自动吸回(负样本行本身也会挡)。
    let (ph, refs) = in_clause(face_ids);
    let detach_sql = format!(
        "UPDATE faces SET person_id=NULL, is_confirmed=0, is_unassigned=1 WHERE id IN ({ph}) AND person_id=?{}",
        face_ids.len() + 1
    );
    let mut detach_refs = refs;
    detach_refs.push(&person_id);
    tx.execute(&detach_sql, detach_refs.as_slice())?;

    // 3. 重算被拒 person（可能减员）。
    recompute_person_aggregates(&tx, person_id)?;

    tx.commit()?;
    Ok(())
}

/// Create a brand-new person from `face_ids` (one-tap "make a person" from a likely-match group),
/// binding them with `is_confirmed=1`. Optional `name` sets `is_named=1`. The new person's
/// aggregates are computed from the bound faces; every source person the faces left is recomputed.
/// 🔴 All faces must share ONE `model_name` (the new person's vector space); mismatch is rejected.
/// Returns the new `person_id`.
///
/// 从 `face_ids` 新建一个 person（从 likely-match 组一键「建人」），以 `is_confirmed=1` 绑定。可选
/// `name` 置 `is_named=1`。新 person 派生字段由绑定脸算出；脸离开的每个源 person 都重算。🔴 所有脸
/// 必须同一 `model_name`（新 person 的向量空间）；不一致则拒绝。返回新 `person_id`。
pub fn create_person_from_faces(
    conn: &Connection,
    face_ids: &[i64],
    name: Option<&str>,
) -> Result<i64> {
    if face_ids.is_empty() {
        return Err(AppError::Internal(
            "建人需至少一张人脸 | create_person needs at least one face".into(),
        ));
    }
    let tx = conn.unchecked_transaction()?;

    // 取这批脸的 model_name 去重——必须唯一（新 person 的向量空间身份）。
    let (ph, refs) = in_clause(face_ids);
    let model_sql = format!("SELECT DISTINCT model_name FROM faces WHERE id IN ({ph})");
    let mut models: Vec<String> = {
        let mut stmt = tx.prepare(&model_sql)?;
        let rows = stmt.query_map(refs.as_slice(), |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    if models.len() != 1 {
        return Err(AppError::Internal(
            "建人失败：所选人脸跨多个模型 | faces span multiple models".into(),
        ));
    }
    let model_name = models.pop().unwrap();

    // 源 person 集（绑定后这些脸已离开）。
    let affected = persons_of_faces(&tx, face_ids)?;

    let trimmed = name.map(str::trim).filter(|s| !s.is_empty());
    tx.execute(
        "INSERT INTO persons (name, is_named, model_name, face_count) VALUES (?1, ?2, ?3, 0)",
        params![trimmed, trimmed.is_some() as i64, model_name],
    )?;
    let new_id = tx.last_insert_rowid();

    // 占位符顺序同 reassign：IN 先占 ?1..?k，new_id 末位 ?{k+1}（避免与 IN 占位冲突）。
    let (ph2, refs2) = in_clause(face_ids);
    let bind_sql = format!(
        "UPDATE faces SET person_id=?{}, is_confirmed=1, is_unassigned=0 WHERE id IN ({ph2})",
        face_ids.len() + 1
    );
    let mut bind_refs = refs2;
    bind_refs.push(&new_id);
    tx.execute(&bind_sql, bind_refs.as_slice())?;

    // 新 person + 所有源 person 重算。
    recompute_person_aggregates(&tx, new_id)?;
    for pid in affected {
        recompute_person_aggregates(&tx, pid)?;
    }

    tx.commit()?;
    Ok(new_id)
}

/// 列出批量审批 UI 的「likely match」组（Part4 §3.5.1 / Part5 T10）：未确认脸（`is_confirmed=0`）
/// 暂归于某（非忽略）person，按候选 person 分组。每张脸带源图缩略图（status=3 vs 分档路径约定同
/// `list_persons`）+ bbox 裁剪框，及其与 person 质心的余弦相似度；组 `confidence` 为这些相似度的
/// 均值。可选 `person_id` 限定单人；可选 `limit` 限组数（confidence 高者优先）。
pub fn list_likely_matches(
    conn: &Connection,
    person_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<crate::db::models::LikelyMatchGroup>> {
    use crate::db::models::{FaceThumb, LikelyMatchGroup};

    // person_id 过滤是可选的；用 (?1 IS NULL OR f.person_id=?1) 避免动态拼 SQL。
    let mut stmt = conn.prepare(
        "SELECT f.id, f.item_id, f.person_id, p.name, p.centroid, f.embedding,
                f.bbox_x, f.bbox_y, f.bbox_w, f.bbox_h,
                CASE
                    WHEN m.thumb_status = 3 OR m.thumb_path IS NULL THEN
                        CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                             ELSE r.path || '/' || d.rel_path || '/' || m.file_name END
                    ELSE m.thumb_path
                END AS thumb_path,
                CASE WHEN m.thumb_path IS NULL THEN 3 ELSE m.thumb_status END AS thumb_status
         FROM faces f
         JOIN persons p ON p.id = f.person_id
         LEFT JOIN media_items m ON m.id = f.item_id AND m.is_deleted = 0
         LEFT JOIN directories d ON m.directory_id = d.id
         LEFT JOIN scan_roots r ON d.root_id = r.id
         WHERE f.is_confirmed = 0 AND f.person_id IS NOT NULL AND p.is_ignored = 0
           AND (?1 IS NULL OR f.person_id = ?1)
         ORDER BY f.person_id, f.det_score DESC",
    )?;

    struct Row {
        face_id: i64,
        item_id: i64,
        person_id: i64,
        person_name: Option<String>,
        centroid: Option<Vec<u8>>,
        embedding: Vec<u8>,
        bbox: [f32; 4],
        thumb_path: Option<String>,
        thumb_status: Option<i64>,
    }
    let rows = stmt.query_map(params![person_id], |row| {
        Ok(Row {
            face_id: row.get(0)?,
            item_id: row.get(1)?,
            person_id: row.get(2)?,
            person_name: row.get(3)?,
            centroid: row.get(4)?,
            embedding: row.get(5)?,
            bbox: [
                row.get::<_, f64>(6)? as f32,
                row.get::<_, f64>(7)? as f32,
                row.get::<_, f64>(8)? as f32,
                row.get::<_, f64>(9)? as f32,
            ],
            thumb_path: row.get(10)?,
            thumb_status: row.get(11)?,
        })
    })?;

    // 按 person 顺序分组（SQL 已 ORDER BY person_id）。centroid/embedding 解码后算余弦（两者均
    // L2 归一化 → 余弦=点积）；centroid 为 NULL 时相似度记 0。
    let mut groups: Vec<LikelyMatchGroup> = Vec::new();
    for r in rows {
        let r = r?;
        let sim = match &r.centroid {
            Some(c) => cosine_from_le_bytes(c, &r.embedding),
            None => 0.0,
        };
        let thumb = FaceThumb {
            face_id: r.face_id,
            item_id: r.item_id,
            thumb_path: r.thumb_path,
            thumb_status: r.thumb_status,
            bbox: r.bbox,
            similarity: sim,
        };
        match groups.last_mut() {
            Some(g) if g.person_id == r.person_id => g.candidate_faces.push(thumb),
            _ => groups.push(LikelyMatchGroup {
                person_id: r.person_id,
                person_name: r.person_name,
                candidate_faces: vec![thumb],
                confidence: 0.0,
            }),
        }
    }

    // 组 confidence = 成员相似度均值；按 confidence 降序，limit 限组数。
    for g in groups.iter_mut() {
        let n = g.candidate_faces.len().max(1) as f32;
        g.confidence = g.candidate_faces.iter().map(|f| f.similarity).sum::<f32>() / n;
    }
    groups.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(lim) = limit {
        if lim >= 0 {
            groups.truncate(lim as usize);
        }
    }
    Ok(groups)
}

/// 两个 f32 小端编码嵌入的余弦相似度。两者在流水线中均已单位归一化，故实为点积；仍除以范数以
/// 防库存质心漂移出单位球。长度不一致 → 0（跨模型安全）。
fn cosine_from_le_bytes(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0f64;
    let mut na = 0f64;
    let mut nb = 0f64;
    for (ca, cb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let va = f32::from_le_bytes([ca[0], ca[1], ca[2], ca[3]]) as f64;
        let vb = f32::from_le_bytes([cb[0], cb[1], cb[2], cb[3]]) as f64;
        dot += va * vb;
        na += va * va;
        nb += vb * vb;
    }
    let denom = (na.sqrt() * nb.sqrt()).max(1e-12);
    (dot / denom) as f32
}

/// 一条待写入的人脸行。`bbox`/`landmarks` 须已按**解码图自身**宽高归一化（非数据库存的原图尺寸——
/// 解码源可能是更小的缩略图），见 `ai::face_pipeline`。
pub struct NewFace {
    pub item_id: i64,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    /// 5 关键点拍平为 10 个 f32，小端（复用 `ai::clip::embedding_to_bytes`）。
    pub landmarks: Vec<u8>,
    pub det_score: f32,
    pub quality: f32,
    pub embedding: Vec<u8>,
}

/// 单条 faces 行 INSERT(batch_finish_face_items 的插入半段)。
fn insert_face_row(conn: &Connection, model_name: &str, r: &NewFace) -> Result<()> {
    conn.execute(
        "INSERT INTO faces
            (item_id, person_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h, landmarks, det_score, quality, embedding, is_confirmed)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
        params![
            r.item_id,
            model_name,
            r.bbox_x,
            r.bbox_y,
            r.bbox_w,
            r.bbox_h,
            r.landmarks,
            r.det_score,
            r.quality,
            r.embedding
        ],
    )?;
    Ok(())
}

/// Face 条件完成写(2026-07-10 审查 X1,语义同 `batch_finish_ai_items`):仅 `cache_key`
/// 仍等于领取时快照的项才「先删后插 faces + face_status=Done」;失效项整体跳过(其旧 faces
/// 已被 invalidate_derived_for_item 删除,新结果又是旧内容算的,一并丢弃)。
/// 返回真正完成(仍新鲜)的 item_id 集——调用方只把这些排进聚类队列。
pub fn batch_finish_face_items(
    conn: &Connection,
    items: &[(i64, i64)], // (item_id, cache_key 快照)
    model_name: &str,
    rows: &[NewFace],
) -> Result<Vec<i64>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let tx = conn.unchecked_transaction()?;
    let mut fresh_ids: Vec<i64> = Vec::with_capacity(items.len());
    for (id, cache_key) in items {
        let fresh = tx.execute(
            "UPDATE media_items SET face_status=2, updated_at=strftime('%s','now')
              WHERE id=?1 AND cache_key=?2",
            params![id, cache_key],
        )?;
        if fresh == 0 {
            continue;
        }
        // 先删后插:零脸图也要删旧行,使上次运行的陈旧人脸被清除。
        tx.execute(
            "DELETE FROM faces WHERE item_id=?1 AND model_name=?2",
            params![id, model_name],
        )?;
        for r in rows.iter().filter(|r| r.item_id == *id) {
            insert_face_row(&tx, model_name, r)?;
        }
        // 覆盖记账与脸行/status 同事务(V17,加固批 B-3):零脸图也落账,
        // 「Done 必有本模型覆盖行」自此是事务级不变量——迟到 writer 也不破坏。
        tx.execute(
            "INSERT OR REPLACE INTO face_coverage (item_id, model_name, analyzed_at)
              VALUES (?1, ?2, strftime('%s','now'))",
            params![id, model_name],
        )?;
        fresh_ids.push(*id);
    }
    tx.commit()?;
    Ok(fresh_ids)
}

/// X1 条件状态写(face 侧 Error 等失败路径,语义同 `batch_update_ai_status_guarded`)。
pub fn batch_update_face_status_guarded(
    conn: &Connection,
    items: &[(i64, i64)], // (item_id, cache_key 快照)
    status: i64,
) -> Result<usize> {
    if items.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut n = 0usize;
    for (id, cache_key) in items {
        n += tx.execute(
            "UPDATE media_items SET face_status=?1, updated_at=strftime('%s','now')
              WHERE id=?2 AND cache_key=?3",
            params![status, id, cache_key],
        )?;
    }
    tx.commit()?;
    Ok(n)
}
