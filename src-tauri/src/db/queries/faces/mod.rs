//! 人脸域 DAO:face 状态机、persons、人工审批、增量聚类、全量重聚类、rejection,
//! 以及全文件唯一真跨域事务 `delete_media_item_hard`(硬删 + person 聚合重算,§4.3)
//! (T 线拆分自 queries.rs,SQL/事务边界/aggregate 不变量不变;第一轮不二拆,D-005)。
//! (二拆 facade,2026-07-25:faces.rs 单文件换成本目录,子模块按职责群拆分,
//! 外部调用方 `crate::db::queries::<symbol>` 零改动,见拆分方案 analysis/faces-rs.md。)

mod approval;
mod clustering;
mod recluster;
mod status;
mod wall;

pub use approval::*;
pub use clustering::*;
pub use recluster::*;
pub use status::*;
pub use wall::*;

use rusqlite::{params, Connection};

use crate::error::Result;

/// Recompute one person's derived fields from its CURRENT member faces, within an open
/// transaction (`conn` may be a `&Transaction`). Centroid = L2-normalized TRUE MEAN of member
/// embeddings (more accurate than the incremental running-average; affordable here because batch
/// approval is rare and a person holds at most a few hundred faces). Cover = the max-quality
/// member. If the person ends up with ZERO faces, mirror `rebuild_person_clusters`' cleanup
/// policy: DELETE it when unnamed & non-ignored (fragment), else keep it as an empty roster slot
/// (centroid/cover cleared) to preserve a named/ignored person the user cares about.
///
/// 在一个已开启的事务内，按 person **当前**成员脸重算其派生字段（`conn` 可为 `&Transaction`）。
/// 质心 = 成员嵌入的 L2 归一化**真均值**（比增量滑动均值更准；批量审批罕见且单 person 至多数百脸，
/// 开销可接受）。封面 = 质量最高的成员。若归零，复刻 `rebuild_person_clusters` 的清理策略：未命名
/// 且非忽略者删除（碎片），否则保留为空槽（清空质心/封面）以护命名/忽略人物。
pub(in crate::db::queries) fn recompute_person_aggregates(
    conn: &Connection,
    person_id: i64,
) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, embedding, quality FROM faces WHERE person_id=?1")?;
    let mut embeddings: Vec<Vec<f32>> = Vec::new();
    let mut best_cover: Option<(i64, f32)> = None;
    {
        let rows = stmt.query_map(params![person_id], |row| {
            let id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let quality: f64 = row.get(2)?;
            Ok((id, blob, quality as f32))
        })?;
        for r in rows {
            let (id, blob, quality) = r?;
            let emb: Vec<f32> = blob
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            embeddings.push(emb);
            // 取质量最高者为封面（与聚类/重聚类 cover 升级规则一致）。
            if best_cover.is_none_or(|(_, q)| quality > q) {
                best_cover = Some((id, quality));
            }
        }
    }

    if embeddings.is_empty() {
        // 归零：删未命名非忽略碎片；命名/忽略者留空槽。两条语句互斥（删了就更新不到行）。
        conn.execute(
            "DELETE FROM persons WHERE id=?1 AND is_named=0 AND is_ignored=0",
            params![person_id],
        )?;
        conn.execute(
            "UPDATE persons SET centroid=NULL, cover_face_id=NULL, face_count=0,
                                updated_at=strftime('%s','now')
             WHERE id=?1",
            params![person_id],
        )?;
        return Ok(());
    }

    // 真均值质心，L2 归一化（f64 累加减小数值误差，写回 f32 LE）。
    let dim = embeddings[0].len();
    let mut acc = vec![0f64; dim];
    for emb in &embeddings {
        for (a, &v) in acc.iter_mut().zip(emb.iter()) {
            *a += v as f64;
        }
    }
    let n = embeddings.len() as f64;
    for a in acc.iter_mut() {
        *a /= n;
    }
    let norm = acc.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-12);
    let centroid_bytes: Vec<u8> = acc
        .iter()
        .flat_map(|&x| ((x / norm) as f32).to_le_bytes())
        .collect();
    let cover_id = best_cover.map(|(id, _)| id).unwrap_or(0);
    conn.execute(
        "UPDATE persons SET centroid=?2, cover_face_id=?3, face_count=?4,
                            updated_at=strftime('%s','now')
         WHERE id=?1",
        params![person_id, centroid_bytes, cover_id, embeddings.len() as i64],
    )?;
    Ok(())
}

/// 为 `face_ids` 的 `IN (…)` 构造 `"?1,?2,…?k"` 占位符与对应 `ToSql` 引用。
pub(super) fn in_clause(face_ids: &[i64]) -> (String, Vec<&dyn rusqlite::ToSql>) {
    let ph: Vec<String> = (1..=face_ids.len()).map(|i| format!("?{i}")).collect();
    let refs: Vec<&dyn rusqlite::ToSql> = face_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    (ph.join(","), refs)
}
