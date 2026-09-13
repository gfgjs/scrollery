//! F6 人物墙 + 人物管理:人物列表(含隐藏根排除 V21)、单图人脸框、命名/隐藏/忽略/合并
//! (T 线拆分自 queries.rs,原 faces.rs B 群;二拆自 faces/mod.rs facade,SQL/事务边界不变)。

use rusqlite::{params, Connection};

use crate::db::queries::scan::EXCLUDE_HIDDEN_ROOTS_M;
use crate::error::{AppError, Result};

use super::in_clause;

// ── 人物墙 / 详情画框（F6）─────────────────────────────────────────────────────

/// 列出人物墙的人物簇（F6）。把每个人物的封面脸 → 其图像缩略图（status=3 vs 分档的约定同
/// `get_search_results_by_ids`）。排除"忽略"桶（误检/非人脸）；已命名优先，再按 face_count。
pub fn list_persons(
    conn: &Connection,
    model_name: &str,
) -> Result<Vec<crate::db::models::PersonSummary>> {
    list_persons_by_ignored(conn, model_name, false)
}

/// 列出「已忽略」(误检/非人脸)人物簇,供误检桶管理视图查看/恢复(镜像隐藏人物切换)。
/// 投影同 `list_persons`,仅 `is_ignored` 过滤翻转为 1(已命名优先在此无意义但无害)。
pub fn list_ignored_persons(
    conn: &Connection,
    model_name: &str,
) -> Result<Vec<crate::db::models::PersonSummary>> {
    list_persons_by_ignored(conn, model_name, true)
}

/// `list_persons`(墙,`is_ignored`=0)与 `list_ignored_persons`(误检桶,`is_ignored`=1)共享体:
/// 投影/排序相同,仅 `is_ignored` 过滤不同——抽共享体避免重复 ~40 行 SQL 与封面映射。
fn list_persons_by_ignored(
    conn: &Connection,
    model_name: &str,
    ignored: bool,
) -> Result<Vec<crate::db::models::PersonSummary>> {
    // 隐藏根排除(V21):人物墙封面/计数须与 Person 视图内列表(经 layout 排除)同口径,否则
    // 隐藏根照片会当墙封面泄漏、face_count 与点开后内容不符。无隐藏根(常态)走原 SQL 逐字节
    // 不变(face_count 用反规范化列,零开销);有隐藏根时切换变体:
    //  - face_count 改**可见**活算(排隐藏根与软删——反规范化列无法按根拆分;人物量级 ~10^2,
    //    每人物一条不相关索引子查询,代价可忽略);
    //  - 封面 JOIN 加排除:封面落隐藏根 → 置空走前端占位(不自动改选他脸,cover_face_id 不动,
    //    取消隐藏即原样回归)。cover_item_id 投影用 m.id 而非 f.item_id——后者在 m 被 JOIN
    //    条件排除后仍非空,会把隐藏项 id 漏给前端;
    //  - 可见脸为零的人物整行隐去(EXISTS 守卫),避免点开是空墙。
    let hidden = crate::db::queries::scan::hidden_root_ids(conn)?;
    let sql = if hidden.is_empty() {
        "SELECT p.id, p.name, p.face_count, p.is_named, p.is_hidden,
                f.item_id,
                CASE
                    WHEN m.thumb_status = 3 OR m.thumb_path IS NULL THEN
                        CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                             ELSE r.path || '/' || d.rel_path || '/' || m.file_name END
                    ELSE m.thumb_path
                END AS cover_thumb_path,
                CASE WHEN m.thumb_path IS NULL THEN 3 ELSE m.thumb_status END AS cover_thumb_status,
                f.bbox_x, f.bbox_y, f.bbox_w, f.bbox_h
         FROM persons p
         LEFT JOIN faces f ON f.id = p.cover_face_id
         LEFT JOIN media_items m ON m.id = f.item_id AND m.is_deleted = 0
         LEFT JOIN directories d ON m.directory_id = d.id
         LEFT JOIN scan_roots r ON d.root_id = r.id
         WHERE p.is_ignored = ?2 AND p.model_name = ?1
         ORDER BY p.is_named DESC, p.face_count DESC, p.id ASC"
            .to_string()
    } else {
        // `vm` 别名版隐藏根谓词(库内 EXCLUDE_HIDDEN_ROOTS 仅有裸/`m` 两变体)。
        const EXCLUDE_HIDDEN_ROOTS_VM: &str = "AND vm.directory_id NOT IN (
                SELECT id FROM directories WHERE root_id IN (
                    SELECT id FROM scan_roots WHERE is_hidden=1))";
        format!(
            "SELECT p.id, p.name,
                (SELECT COUNT(*) FROM faces vf JOIN media_items vm ON vm.id = vf.item_id
                 WHERE vf.person_id = p.id AND vm.is_deleted = 0 {EXCLUDE_HIDDEN_ROOTS_VM})
                    AS face_count,
                p.is_named, p.is_hidden,
                m.id AS cover_item_id,
                CASE
                    WHEN m.thumb_status = 3 OR m.thumb_path IS NULL THEN
                        CASE WHEN d.rel_path = '' THEN r.path || '/' || m.file_name
                             ELSE r.path || '/' || d.rel_path || '/' || m.file_name END
                    ELSE m.thumb_path
                END AS cover_thumb_path,
                CASE WHEN m.thumb_path IS NULL THEN 3 ELSE m.thumb_status END AS cover_thumb_status,
                f.bbox_x, f.bbox_y, f.bbox_w, f.bbox_h
         FROM persons p
         LEFT JOIN faces f ON f.id = p.cover_face_id
         LEFT JOIN media_items m ON m.id = f.item_id AND m.is_deleted = 0 {EXCLUDE_HIDDEN_ROOTS_M}
         LEFT JOIN directories d ON m.directory_id = d.id
         LEFT JOIN scan_roots r ON d.root_id = r.id
         WHERE p.is_ignored = ?2 AND p.model_name = ?1
           AND EXISTS(SELECT 1 FROM faces vf JOIN media_items vm ON vm.id = vf.item_id
                      WHERE vf.person_id = p.id AND vm.is_deleted = 0 {EXCLUDE_HIDDEN_ROOTS_VM})
         ORDER BY p.is_named DESC, face_count DESC, p.id ASC"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![model_name, ignored as i64], |row| {
        let bx: Option<f64> = row.get(8)?;
        let by: Option<f64> = row.get(9)?;
        let bw: Option<f64> = row.get(10)?;
        let bh: Option<f64> = row.get(11)?;
        let cover_bbox = match (bx, by, bw, bh) {
            (Some(x), Some(y), Some(w), Some(h)) => Some([x as f32, y as f32, w as f32, h as f32]),
            _ => None,
        };
        Ok(crate::db::models::PersonSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            face_count: row.get(2)?,
            is_named: row.get::<_, i64>(3)? != 0,
            is_hidden: row.get::<_, i64>(4)? != 0,
            cover_item_id: row.get(5)?,
            cover_thumb_path: row.get(6)?,
            cover_thumb_status: row.get(7)?,
            cover_bbox,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

// ── 隐藏根排除(V21 人物墙)────────────────────────────────────────────────────
// 锁三件事:隐藏时封面不泄漏(cover_item_id/thumb 置空)、face_count 只算可见、
// 可见脸为零的人物整簇隐去;无隐藏根时走原 SQL(反规范化 face_count)且 unhide 全量回归。
#[cfg(test)]
mod hidden_root_person_wall_tests {
    use super::*;
    use crate::db::queries::scan::set_scan_root_hidden;

    /// 两根各一图;p1 两脸(跨两根,封面钉 root2 的图 2),p2 单脸(仅 root2)。
    fn two_roots_persons() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::migration::run_migrations(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r1', 'R1'), (2, '/r2', 'R2');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES
                 (10, 1, '', 'r1'), (20, 2, '', 'r2');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime, file_format, media_type, width, height, sort_datetime, cache_key) VALUES
                 (1, 10, 'a.jpg', 1, 1, 'jpg', 'image', 10, 10, 100, 0),
                 (2, 20, 'b.jpg', 1, 1, 'jpg', 'image', 10, 10, 200, 0);
             INSERT INTO persons (id, name, face_count, is_named, model_name) VALUES
                 (1, 'p1', 2, 1, 'm'),
                 (2, 'p2', 1, 1, 'm');
             INSERT INTO faces (id, item_id, person_id, model_name, bbox_x, bbox_y, bbox_w, bbox_h, det_score, quality, embedding) VALUES
                 (101, 1, 1, 'm', 0.1, 0.1, 0.2, 0.2, 0.9, 0.5, x'00'),
                 (102, 2, 1, 'm', 0.1, 0.1, 0.2, 0.2, 0.9, 0.9, x'00'),
                 (103, 2, 2, 'm', 0.5, 0.5, 0.2, 0.2, 0.9, 0.9, x'00');
             UPDATE persons SET cover_face_id=102 WHERE id=1;
             UPDATE persons SET cover_face_id=103 WHERE id=2;",
        )
        .unwrap();
        c
    }

    #[test]
    fn wall_excludes_hidden_root_and_unhide_restores() {
        let c = two_roots_persons();
        let wall = list_persons(&c, "m").unwrap();
        assert_eq!(wall.len(), 2);
        assert_eq!(
            (wall[0].id, wall[0].face_count, wall[0].cover_item_id),
            (1, 2, Some(2))
        );
        assert_eq!(
            (wall[1].id, wall[1].face_count, wall[1].cover_item_id),
            (2, 1, Some(2))
        );

        set_scan_root_hidden(&c, 2, true).unwrap();
        let wall = list_persons(&c, "m").unwrap();
        assert_eq!(wall.len(), 1, "可见脸为零的 p2 整簇隐去");
        let p1 = &wall[0];
        assert_eq!(p1.id, 1);
        assert_eq!(p1.face_count, 1, "face_count 只算可见脸");
        assert_eq!(p1.cover_item_id, None, "隐藏根封面不泄漏 id");
        assert_eq!(p1.cover_thumb_path, None, "隐藏根封面不泄漏路径");

        set_scan_root_hidden(&c, 2, false).unwrap();
        let wall = list_persons(&c, "m").unwrap();
        assert_eq!(wall.len(), 2, "unhide 全量回归");
        assert_eq!((wall[0].face_count, wall[0].cover_item_id), (2, Some(2)));
    }
}

/// 一张图中检测到的所有人脸，用于详情查看器叠加框（F6）。
#[allow(clippy::items_after_test_module)]
pub fn get_faces_for_item(
    conn: &Connection,
    item_id: i64,
) -> Result<Vec<crate::db::models::FaceBox>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.person_id, p.name, f.bbox_x, f.bbox_y, f.bbox_w, f.bbox_h, f.det_score
         FROM faces f
         LEFT JOIN persons p ON p.id = f.person_id
         WHERE f.item_id = ?1
         ORDER BY f.det_score DESC",
    )?;
    let rows = stmt.query_map(params![item_id], |row| {
        Ok(crate::db::models::FaceBox {
            id: row.get(0)?,
            person_id: row.get(1)?,
            person_name: row.get(2)?,
            bbox: [
                row.get::<_, f64>(3)? as f32,
                row.get::<_, f64>(4)? as f32,
                row.get::<_, f64>(5)? as f32,
                row.get::<_, f64>(6)? as f32,
            ],
            det_score: row.get::<_, f64>(7)? as f32,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 给人物命名（置 `is_named=1`）。空白名字则清回未命名。
pub fn rename_person(conn: &Connection, person_id: i64, name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        conn.execute(
            "UPDATE persons SET name=NULL, is_named=0, updated_at=strftime('%s','now') WHERE id=?1",
            params![person_id],
        )?;
    } else {
        conn.execute(
            "UPDATE persons SET name=?2, is_named=1, updated_at=strftime('%s','now') WHERE id=?1",
            params![person_id, trimmed],
        )?;
    }
    Ok(())
}

/// 在人物墙上显示/隐藏某人物（`is_hidden`）。
pub fn set_person_hidden(conn: &Connection, person_id: i64, hidden: bool) -> Result<()> {
    conn.execute(
        "UPDATE persons SET is_hidden=?2, updated_at=strftime('%s','now') WHERE id=?1",
        params![person_id, hidden as i64],
    )?;
    Ok(())
}

/// 误检桶开关(2026-07-10 审查 G1):`is_ignored` 的 schema(V8)/列表过滤/recluster 锚定
/// 保护全链早已建成,唯独无任何写入口——整条特性此前不可达。语义:非人脸误检(雕像/
/// 海报/玩偶),置位后不上人物墙(list_persons 过滤)、全量重建按锚定保护(不删、成员
/// 脸留在桶内不再被吸去别处)。
pub fn set_person_ignored(conn: &Connection, person_id: i64, ignored: bool) -> Result<()> {
    conn.execute(
        "UPDATE persons SET is_ignored=?2, updated_at=strftime('%s','now') WHERE id=?1",
        params![person_id, ignored as i64],
    )?;
    Ok(())
}

/// Merge `src_ids` person clusters INTO `dst_id` in one transaction: reassign their faces to
/// `dst`, recompute `dst`'s centroid as a face_count-weighted average of all merged centroids
/// (re-normalized to unit length), bump face_count, then delete the now-empty src persons.
///
/// The weighted-average centroid is an APPROXIMATION (the true centroid is the mean of all member
/// embeddings) — deliberately consistent with F4's incremental running-average, and cheap (reads
/// only `persons`, not every face embedding). `dst`'s cover face is kept (the user merged others
/// INTO dst, so dst's identity/cover is authoritative). Named/`is_confirmed` faces aren't split.
///
/// 在单个事务中把 `src_ids` 人物簇并入 `dst_id`：把它们的脸改派给 `dst`，将 `dst` 质心重算为所有
/// 被并簇质心的 face_count 加权平均（重归一化为单位长度），累加 face_count，再删除已空的 src。
///
/// 加权平均质心是**近似**（真质心是所有成员嵌入的均值）——刻意与 F4 增量滑动平均一致，且廉价
///（只读 `persons`，不读每张脸的嵌入）。保留 `dst` 的封面脸（用户把别人并入 dst，dst 身份/封面
/// 权威）。已命名/`is_confirmed` 的脸不被打散。
pub fn merge_persons(conn: &Connection, src_ids: &[i64], dst_id: i64) -> Result<()> {
    let src: Vec<i64> = src_ids.iter().copied().filter(|&id| id != dst_id).collect();
    if src.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;

    // 收集 dst + src 的质心/计数做加权平均。f32 小端，长度 = embed_dim。
    let mut all_ids = vec![dst_id];
    all_ids.extend_from_slice(&src);

    // 同模型守卫(2026-07-10 审查 F1,对齐 reassign/create 的既有守卫):跨模型合并会让
    // 下方 zip 加权按短者静默截断、产出混维垃圾质心,且 faces 改派后 dst 簇混入异空间向量。
    // UI 只列当前模型难触发,但 IPC 边界是不可信输入(项目红线),三命令守卫必须对称。
    {
        let (ph, refs) = in_clause(&all_ids);
        let sql = format!("SELECT COUNT(DISTINCT model_name) FROM persons WHERE id IN ({ph})");
        let distinct_models: i64 = tx.query_row(&sql, refs.as_slice(), |row| row.get(0))?;
        if distinct_models > 1 {
            return Err(AppError::Internal(
                "跨模型合并被拒:被合并人物的模型不一致 | cross-model merge rejected".into(),
            ));
        }
    }

    let placeholders: Vec<String> = (1..=all_ids.len()).map(|i| format!("?{i}")).collect();
    let sel = format!(
        "SELECT centroid, face_count FROM persons WHERE id IN ({})",
        placeholders.join(",")
    );
    let mut acc: Vec<f64> = Vec::new();
    let mut total_count: i64 = 0;
    {
        let mut stmt = tx.prepare(&sel)?;
        let refs: Vec<&dyn rusqlite::ToSql> = all_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        let mut rows = stmt.query(refs.as_slice())?;
        while let Some(row) = rows.next()? {
            let blob: Option<Vec<u8>> = row.get(0)?;
            let count: i64 = row.get(1)?;
            if let Some(bytes) = blob {
                let centroid: Vec<f32> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                if acc.is_empty() {
                    acc = vec![0.0; centroid.len()];
                }
                // 加权累加（按 face_count）。
                let w = count.max(1) as f64;
                for (a, &c) in acc.iter_mut().zip(centroid.iter()) {
                    *a += c as f64 * w;
                }
            }
            total_count += count;
        }
    }

    // 把 src 的脸改派给 dst。
    let src_ph: Vec<String> = (1..=src.len()).map(|i| format!("?{}", i + 1)).collect();
    let upd = format!(
        "UPDATE faces SET person_id=?1 WHERE person_id IN ({})",
        src_ph.join(",")
    );
    let mut upd_refs: Vec<&dyn rusqlite::ToSql> = vec![&dst_id];
    for id in &src {
        upd_refs.push(id as &dyn rusqlite::ToSql);
    }
    tx.execute(&upd, upd_refs.as_slice())?;

    // 重归一化加权质心并连同新 face_count 写回。
    if !acc.is_empty() {
        let norm = acc.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-12);
        let centroid_bytes: Vec<u8> = acc
            .iter()
            .flat_map(|&x| ((x / norm) as f32).to_le_bytes())
            .collect();
        tx.execute(
            "UPDATE persons SET centroid=?2, face_count=?3, updated_at=strftime('%s','now') WHERE id=?1",
            params![dst_id, centroid_bytes, total_count],
        )?;
    } else {
        tx.execute(
            "UPDATE persons SET face_count=?2, updated_at=strftime('%s','now') WHERE id=?1",
            params![dst_id, total_count],
        )?;
    }

    // 删除已空的 src 人物。
    let del_ph: Vec<String> = (1..=src.len()).map(|i| format!("?{i}")).collect();
    let del_sql = format!("DELETE FROM persons WHERE id IN ({})", del_ph.join(","));
    let del_refs: Vec<&dyn rusqlite::ToSql> =
        src.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    tx.execute(&del_sql, del_refs.as_slice())?;

    tx.commit()?;
    Ok(())
}
