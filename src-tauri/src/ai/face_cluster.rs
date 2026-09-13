// src-tauri/src/ai/face_cluster.rs
//! 增量最近质心人脸聚类（F4）。
//!
//! # 范围：仅增量，没有全量复核
//! 早期设计笔记设想"增量 nearest-centroid + 周期全量 union-find"两段。本文件只做前半段：
//! 每张新脸一来就贪心匹配最近的既有人物质心，命中则并入、否则新建。**没有**全量复核阶段去
//! 修正贪心算法的碎片化（同一人因早期样本不足被分成几个不同人物）。
//!
//! 不做的原因：全量复核要在所有"未命名/未确认"的脸上做两两相似度（或对其质心做并查集），
//! 这是 O(n²)——库越大越贵，且与本文件的增量结果是同一份数据的两次决策，每次 pipeline 跑完
//! 都重新复核一遍等于把增量阶段算的东西扔掉重算。这两件事要么只选一个，要么让全量复核**罕见
//! 地**跑（显式命令触发，而非每次自动跑）。该取舍留给 F5（那里也是命名人物/已确认脸的"不可
//! 被自动拆散"保护规则的合适落点——这里完全不需要，因为贪心增量从不移动已分配的脸，只决定
//! 新脸去哪）。
//!
//! 已知 v1 局限：早期样本不足导致的碎片化（同一人被分成几个未命名人物）在 F6（人物墙）里要靠
//! 用户手动合并；不是 bug，是这一刀切掉的复杂度的直接后果。
//!
//! # `persons` 已按模型隔离(Part4-T6,2026-07-02)
//! `persons.model_name`(V11)与 `faces.model_name` 同为向量空间键:名册读取
//! (`get_all_persons_for_clustering`)、新人物落库(`apply_face_clusters`)、全量重建
//! (`rebuild_person_clusters`)与按模型 reset 全部按当前模型过滤——不同模型的名册互不
//! 可见、互不销毁(旧轨标注是用户资产)。切换轨经 gated `set_active_face_model`
//! (face_commands):写 `face_model_active` + `sync_face_status_for_model` 把 face_status
//! 指向新轨覆盖。维度混算的最后防线在 `cosine_similarity`:长度不匹配 → 0.0(永不判同)
//! 而非 panic。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tracing::warn;

use crate::ai::clip::{bytes_to_embedding, embedding_to_bytes};
use crate::ai::face_profile::FaceProfile;
use crate::ai::search::cosine_similarity;
use crate::db::queries::{
    apply_face_clusters, get_all_persons_for_clustering, get_clusterable_faces, get_config,
    ClusterableFaceRow, PersonClusterUpdate, PersonRow,
};
use crate::state::AppState;

/// 运行期阈值 override：用 `app_config` 的可选键覆盖 profile 的人脸阈值，让阈值在
/// **不重编译**下可调，服务「实测比较」（用户约束:未经实测/市场验证之物不定死）。
/// config 键缺省或解析失败 → 回退 profile 默认值，无 override 时行为与改动前完全一致。
///
/// ⚠️ profile 默认阈值是上游（OpenCV/InsightFace）搬来的、**未经本项目百万级增量聚类实测标定**
/// （见 [`FaceProfile::same_face_threshold`] / [`FaceProfile::min_quality`] 字段注释）。
/// - `face_same_threshold`：同人 cosine 阈值 override
/// - `face_min_quality`：参与聚类的质量下限 override
pub fn effective_thresholds(state: &Arc<AppState>, profile: &FaceProfile) -> (f32, f32) {
    let Ok(conn) = state.db_read_pool.get() else {
        return (profile.same_face_threshold, profile.min_quality);
    };
    let same = read_f32_config(&conn, "face_same_threshold").unwrap_or(profile.same_face_threshold);
    let min_q = read_f32_config(&conn, "face_min_quality").unwrap_or(profile.min_quality);
    (same, min_q)
}

/// 读取一个 f32 配置项；缺省/空/解析失败统一返回 None（让调用方回退默认）。
fn read_f32_config(conn: &rusqlite::Connection, key: &str) -> Option<f32> {
    get_config(conn, key)
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<f32>().ok())
}

/// In-memory roster entry for one person, decoded from `PersonRow` for cosine-similarity math.
/// `id < 0` means "created during this flush, not yet persisted" (see `assign_face`).
/// 一个人物的内存名册条目，从 `PersonRow` 解码出来用于余弦相似度计算。`id < 0` 表示
/// "本次刷新中新建、尚未落库"（见 `assign_face`）。
struct RosterEntry {
    id: i64,
    centroid: Vec<f32>,
    face_count: i64,
    cover_face_id: i64,
    cover_quality: f32,
}

impl From<PersonRow> for RosterEntry {
    fn from(p: PersonRow) -> Self {
        RosterEntry {
            id: p.id,
            centroid: bytes_to_embedding(&p.centroid),
            face_count: p.face_count,
            cover_face_id: p.cover_face_id,
            cover_quality: p.cover_quality,
        }
    }
}

/// 一张就绪可聚类的人脸（已解码嵌入向量）。
struct ClusterableFace {
    id: i64,
    embedding: Vec<f32>,
    quality: f32,
}

impl From<ClusterableFaceRow> for ClusterableFace {
    fn from(r: ClusterableFaceRow) -> Self {
        ClusterableFace {
            id: r.id,
            embedding: bytes_to_embedding(&r.embedding),
            quality: r.quality,
        }
    }
}

/// 为一张新脸在内存 `roster` 中决定归属，原地修改（命中则更新既有条目的质心/计数/封面，未命中
/// 则追加一条占位条目）。返回归属 id（负数=占位，尚未落库）。
///
/// 贪心且终局：一张脸一旦在此归入某人，本模块里没有任何东西会重新考虑它（没有全量复核——见
/// 模块头）。质心更新是重归一化到单位长度的滑动平均，按到达顺序会有轻微漂移，对仅增量的设计
/// 而言无妨。
fn assign_face(
    roster: &mut Vec<RosterEntry>,
    face: &ClusterableFace,
    threshold: f32,
    next_placeholder_id: &mut i64,
    rejected: &HashSet<i64>,
) -> i64 {
    // 跳过用户为此脸拒绝过的**真实** person id 的名册条目（负样本守卫，Part4 T3 StageB）。占位条目
    //（id<0，本趟新簇）从不被拒，仍可作候选——被拒脸仍能另立/并入新簇。
    let best = roster
        .iter()
        .enumerate()
        .filter(|(_, p)| !rejected.contains(&p.id))
        .map(|(i, p)| (i, cosine_similarity(&face.embedding, &p.centroid)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((i, sim)) = best {
        if sim >= threshold {
            let p = &mut roster[i];
            let n = p.face_count as f32;
            let mut merged: Vec<f32> = p
                .centroid
                .iter()
                .zip(&face.embedding)
                .map(|(c, e)| (c * n + e) / (n + 1.0))
                .collect();
            let norm = merged.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            for x in &mut merged {
                *x /= norm;
            }
            p.centroid = merged;
            p.face_count += 1;
            // 封面升级：后来一张质量更高的脸替换人物墙缩略图。
            if face.quality > p.cover_quality {
                p.cover_face_id = face.id;
                p.cover_quality = face.quality;
            }
            return p.id;
        }
    }

    let id = *next_placeholder_id;
    *next_placeholder_id -= 1;
    roster.push(RosterEntry {
        id,
        centroid: face.embedding.clone(),
        face_count: 1,
        cover_face_id: face.id,
        cover_quality: face.quality,
    });
    id
}

/// Incrementally cluster the faces just written for `item_ids` under `model_name`.
///
/// 增量聚类刚为 `item_ids`（`model_name` 下）写入的人脸。
///
/// 必须在调用方释放了用于主人脸写入的 `db_writer` 锁**之后**调用——本函数自己做一次短读
/// （经 `db_read_pool`）再做一次短写（经 `db_writer`），不能嵌套在更长的写临界区里（会拉长
/// scan/缩略图生成也要争用的锁）。质量低于 `min_quality` 的脸不参与聚类（`person_id` 保持
/// NULL），符合 `FaceProfile::min_quality`"跳过低质量、减噪"的约定。
pub fn cluster_new_faces(
    state: &Arc<AppState>,
    item_ids: &[i64],
    model_name: &str,
    threshold: f32,
    min_quality: f32,
) {
    if item_ids.is_empty() {
        return;
    }

    let (mut roster, faces) = {
        let conn = match state.db_read_pool.get() {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "DB pool error in face clustering | 人脸聚类 DB 池错误: {}",
                    e
                );
                return;
            }
        };
        let roster = match get_all_persons_for_clustering(&conn, model_name) {
            Ok(r) => r.into_iter().map(RosterEntry::from).collect::<Vec<_>>(),
            Err(e) => {
                warn!("Failed to load person roster | 加载人物名册失败: {}", e);
                return;
            }
        };
        let faces = match get_clusterable_faces(&conn, item_ids, model_name) {
            Ok(f) => f.into_iter().map(ClusterableFace::from).collect::<Vec<_>>(),
            Err(e) => {
                warn!(
                    "Failed to load clusterable faces | 加载待聚类人脸失败: {}",
                    e
                );
                return;
            }
        };
        (roster, faces)
    };

    // 名册 id（真实或占位）→ 本次刷新归入它的脸 id 列表。
    let mut touched: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut next_placeholder_id = -1i64;
    // 增量刷新不消费拒绝对（负样本仅在显式全量重聚类时生效，见 plan_recluster）。空集免每次新建。
    let no_rejections: HashSet<i64> = HashSet::new();
    for face in &faces {
        if face.quality < min_quality {
            continue;
        }
        let pid = assign_face(
            &mut roster,
            face,
            threshold,
            &mut next_placeholder_id,
            &no_rejections,
        );
        touched.entry(pid).or_default().push(face.id);
    }

    if touched.is_empty() {
        return;
    }

    let by_id: HashMap<i64, &RosterEntry> = roster.iter().map(|p| (p.id, p)).collect();
    let updates: Vec<PersonClusterUpdate> = touched
        .into_iter()
        .filter_map(|(pid, face_ids)| {
            by_id.get(&pid).map(|p| PersonClusterUpdate {
                id: p.id,
                centroid: embedding_to_bytes(&p.centroid),
                face_count: p.face_count,
                cover_face_id: p.cover_face_id,
                face_ids,
            })
        })
        .collect();

    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = apply_face_clusters(&conn, model_name, &updates) {
        warn!(
            "Failed to apply face cluster assignments | 应用人脸聚类分配失败: {}",
            e
        );
    }
}

// ── 全量重新聚类（显式命令：修增量碎片化，但护用户劳动）──────────────────────────
// ── Full re-clustering (explicit command: fixes incremental fragmentation, protects labor) ──

/// 一组嵌入的 L2 归一化均值（真质心）。空输入返回空。
fn mean_centroid(embeddings: &[&[f32]]) -> Vec<f32> {
    if embeddings.is_empty() {
        return Vec::new();
    }
    let dim = embeddings[0].len();
    let mut acc = vec![0f32; dim];
    for e in embeddings {
        for (a, v) in acc.iter_mut().zip(e.iter()) {
            *a += v;
        }
    }
    let n = embeddings.len() as f32;
    for a in acc.iter_mut() {
        *a /= n;
    }
    let norm = acc.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for a in acc.iter_mut() {
        *a /= norm;
    }
    acc
}

/// 待重建的一行 person 数据。
struct ReclusterPerson {
    id: i64,
    is_named: bool,
    is_ignored: bool,
    centroid: Vec<f32>,
}

/// 待重建的一行 face 数据。
struct ReclusterFace {
    id: i64,
    person_id: Option<i64>,
    embedding: Vec<f32>,
    quality: f32,
    is_confirmed: bool,
}

/// 全量重聚类的纯核心：从零决定每张脸归属，同时锁定用户劳动。与 DB I/O 分离以便单测。
///
/// 保护契约（按脸 `is_confirmed`，对应 schema 注释「重聚类不打散」）：
/// - **锁定脸**（永不移动）：`is_confirmed` 的脸，以及归属 `is_ignored` 人物下的任何脸。
/// - **锚定人物**（得以存续）：已命名、已忽略、或持有 ≥1 张锁定脸的人物。
/// - **Free 脸** 按最近质心贪心重新分配（质量降序，强脸优先占位）；低于 `min_quality` 者
///   不参与聚类（`person_id` 保持 NULL）。
///
/// 返回供 `rebuild_person_clusters` 用的 `PersonClusterUpdate` 列表（占位 id 为负）。
fn plan_recluster(
    persons: Vec<ReclusterPerson>,
    faces: Vec<ReclusterFace>,
    threshold: f32,
    min_quality: f32,
    rejections: &HashMap<i64, HashSet<i64>>,
) -> Vec<PersonClusterUpdate> {
    let ignored_ids: HashSet<i64> = persons
        .iter()
        .filter(|p| p.is_ignored)
        .map(|p| p.id)
        .collect();

    // 锁定脸 = 已确认 或 落在忽略桶里——前提是它确有所属人物（已确认但未分配的脸落入 free）。
    let is_pinned = |f: &ReclusterFace| -> bool {
        match f.person_id {
            Some(pid) => f.is_confirmed || ignored_ids.contains(&pid),
            None => false,
        }
    };

    // 按所属（固定）人物分组锁定脸。
    let mut pinned_by_person: HashMap<i64, Vec<&ReclusterFace>> = HashMap::new();
    for f in &faces {
        if is_pinned(f) {
            if let Some(pid) = f.person_id {
                pinned_by_person.entry(pid).or_default().push(f);
            }
        }
    }

    // 聚合一组脸 → (质心, 封面脸id)，取质量最高者为封面。
    let aggregate = |fs: &[&ReclusterFace]| -> (Vec<f32>, i64, f32) {
        let embs: Vec<&[f32]> = fs.iter().map(|f| f.embedding.as_slice()).collect();
        let centroid = mean_centroid(&embs);
        let cover = fs
            .iter()
            .max_by(|a, b| {
                a.quality
                    .partial_cmp(&b.quality)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        (centroid, cover.id, cover.quality)
    };

    let mut updates: Vec<PersonClusterUpdate> = Vec::new();

    // 1. 忽略桶：原样重挂其锁定脸；绝不吸附 free 脸。
    for p in persons.iter().filter(|p| p.is_ignored) {
        if let Some(fs) = pinned_by_person.get(&p.id) {
            let (centroid, cover_id, _) = aggregate(fs);
            updates.push(PersonClusterUpdate {
                id: p.id,
                centroid: embedding_to_bytes(&centroid),
                face_count: fs.len() as i64,
                cover_face_id: cover_id,
                face_ids: fs.iter().map(|f| f.id).collect(),
            });
        }
        // 空忽略桶：无 update，靠 is_ignored=1 在删除步骤存活。
    }

    // 2. Matchable roster: named persons + unnamed persons holding pinned (confirmed) faces.
    //    Seeded so `assign_face` continues each person's running average correctly.
    //    可匹配名册：已命名人物 + 含锁定（已确认）脸的未命名人物。预种好使 assign_face 续算滑动均值。
    let mut roster: Vec<RosterEntry> = Vec::new();
    let mut touched: HashMap<i64, Vec<i64>> = HashMap::new();
    for p in persons.iter().filter(|p| !p.is_ignored) {
        let pinned = pinned_by_person.get(&p.id);
        let anchored = p.is_named || pinned.is_some();
        if !anchored {
            continue;
        }
        let entry = match pinned {
            Some(fs) => {
                let (centroid, cover_id, cover_q) = aggregate(fs);
                touched
                    .entry(p.id)
                    .or_default()
                    .extend(fs.iter().map(|f| f.id));
                RosterEntry {
                    id: p.id,
                    centroid,
                    face_count: fs.len() as i64,
                    cover_face_id: cover_id,
                    cover_quality: cover_q,
                }
            }
            None => {
                // Named-but-no-confirmed-face: seed from stored centroid so it still attracts. A
                // person with neither pinned faces nor a stored centroid can't match → skip (its
                // row survives untouched via is_named=1).
                // 已命名但无确认脸：用库存质心作种子以仍能吸附。既无锁定脸又无库存质心者无法匹配→
                // 跳过（其行靠 is_named=1 原样存活）。
                if p.centroid.is_empty() {
                    continue;
                }
                RosterEntry {
                    id: p.id,
                    centroid: p.centroid.clone(),
                    face_count: 0,
                    cover_face_id: 0,
                    cover_quality: 0.0,
                }
            }
        };
        roster.push(entry);
    }

    // 3. Free 脸（非锁定、质量达标），强脸优先，贪心分配。
    let mut free: Vec<&ReclusterFace> = faces
        .iter()
        .filter(|f| !is_pinned(f) && f.quality >= min_quality)
        .collect();
    free.sort_by(|a, b| {
        b.quality
            .partial_cmp(&a.quality)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut next_placeholder_id = -1i64;
    let empty_rejected: HashSet<i64> = HashSet::new();
    for f in free {
        let cf = ClusterableFace {
            id: f.id,
            embedding: f.embedding.clone(),
            quality: f.quality,
        };
        // 该脸被拒绝的 person 集（无则空）——assign_face 据此跳过这些既有 person。
        let rejected = rejections.get(&f.id).unwrap_or(&empty_rejected);
        let pid = assign_face(
            &mut roster,
            &cf,
            threshold,
            &mut next_placeholder_id,
            rejected,
        );
        touched.entry(pid).or_default().push(f.id);
    }

    // 4. 为最终有脸的每个名册条目产出 update（可匹配 + 新占位）。
    let by_id: HashMap<i64, &RosterEntry> = roster.iter().map(|p| (p.id, p)).collect();
    for (pid, face_ids) in touched {
        if let Some(p) = by_id.get(&pid) {
            updates.push(PersonClusterUpdate {
                id: p.id,
                centroid: embedding_to_bytes(&p.centroid),
                face_count: p.face_count,
                cover_face_id: p.cover_face_id,
                face_ids,
            });
        }
    }

    updates
}

/// 全量重聚类 `model_name` 下所有脸：加载 → `plan_recluster` → 落库。同 `cluster_new_faces`，必须在
/// 任何长写临界区**之外**运行——自做一次短读再一次短写事务。调用方须保证人脸流水线未并发写入
///（命令以分析令牌守卫）。最坏 O(n²) 但罕见（显式命令），不同于每批增量。
pub fn recluster_all(state: &Arc<AppState>, model_name: &str, threshold: f32, min_quality: f32) {
    use crate::db::queries::{
        get_all_faces_for_recluster, get_face_rejections, get_persons_for_recluster,
    };

    let (persons, faces, rejections) = {
        let conn = match state.db_read_pool.get() {
            Ok(c) => c,
            Err(e) => {
                warn!("DB pool error in re-clustering | 重聚类 DB 池错误: {}", e);
                return;
            }
        };
        let persons = match get_persons_for_recluster(&conn, model_name) {
            Ok(rows) => rows
                .into_iter()
                .map(|p| ReclusterPerson {
                    id: p.id,
                    is_named: p.is_named,
                    is_ignored: p.is_ignored,
                    centroid: bytes_to_embedding(&p.centroid),
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                warn!(
                    "Failed to load persons for recluster | 加载重聚类人物失败: {}",
                    e
                );
                return;
            }
        };
        let faces = match get_all_faces_for_recluster(&conn, model_name) {
            Ok(rows) => rows
                .into_iter()
                .map(|f| ReclusterFace {
                    id: f.id,
                    person_id: f.person_id,
                    embedding: bytes_to_embedding(&f.embedding),
                    quality: f.quality,
                    is_confirmed: f.is_confirmed,
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                warn!(
                    "Failed to load faces for recluster | 加载重聚类人脸失败: {}",
                    e
                );
                return;
            }
        };
        // 负样本对 → HashMap<face_id, HashSet<person_id>>；加载失败降级为空（不阻断重聚类）。
        let rejections: HashMap<i64, HashSet<i64>> = match get_face_rejections(&conn, model_name) {
            Ok(pairs) => {
                let mut map: HashMap<i64, HashSet<i64>> = HashMap::new();
                for (face_id, person_id) in pairs {
                    map.entry(face_id).or_default().insert(person_id);
                }
                map
            }
            Err(e) => {
                warn!(
                    "Failed to load face rejections (continuing without) | 加载人脸负样本失败（按无处理）: {}",
                    e
                );
                HashMap::new()
            }
        };
        (persons, faces, rejections)
    };

    let updates = plan_recluster(persons, faces, threshold, min_quality, &rejections);

    let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = crate::db::queries::rebuild_person_clusters(&conn, model_name, &updates) {
        warn!("Failed to rebuild person clusters | 重建人物簇失败: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(id: i64, embedding: Vec<f32>, quality: f32) -> ClusterableFace {
        ClusterableFace {
            id,
            embedding,
            quality,
        }
    }

    /// T_t3(Part4-T6):跨向量空间兜底——名册质心 2 维、新脸 3 维(模拟切换后残留),
    /// assign_face 不 panic、不判同,新脸另立占位新簇。
    #[test]
    fn assign_face_dim_mismatch_no_panic_no_match() {
        let mut roster = vec![RosterEntry {
            id: 1,
            centroid: vec![1.0, 0.0],
            face_count: 3,
            cover_face_id: 10,
            cover_quality: 0.9,
        }];
        let f = face(99, vec![1.0, 0.0, 0.0], 0.9);
        let mut next_id = -1i64;
        let pid = assign_face(&mut roster, &f, 0.363, &mut next_id, &HashSet::new());
        assert_eq!(pid, -1, "维度不符 → 相似度 0.0 → 不并入旧簇,另立占位新簇");
        assert_eq!(roster.len(), 2);
    }

    #[test]
    fn first_face_creates_a_new_placeholder_person() {
        let mut roster: Vec<RosterEntry> = Vec::new();
        let mut next_id = -1i64;
        let f = face(1, vec![1.0, 0.0], 0.5);

        let pid = assign_face(&mut roster, &f, 0.9, &mut next_id, &HashSet::new());

        assert_eq!(pid, -1);
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].face_count, 1);
        assert_eq!(roster[0].cover_face_id, 1);
    }

    #[test]
    fn similar_face_joins_the_existing_person() {
        let mut roster: Vec<RosterEntry> = Vec::new();
        let mut next_id = -1i64;
        let a = face(1, vec![1.0, 0.0], 0.5);
        let b = face(2, vec![0.99, 0.14], 0.5); // cosine ≈ 0.99 with a

        let pid_a = assign_face(&mut roster, &a, 0.9, &mut next_id, &HashSet::new());
        let pid_b = assign_face(&mut roster, &b, 0.9, &mut next_id, &HashSet::new());

        assert_eq!(pid_a, pid_b);
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].face_count, 2);
    }

    #[test]
    fn dissimilar_face_creates_a_separate_person() {
        let mut roster: Vec<RosterEntry> = Vec::new();
        let mut next_id = -1i64;
        let a = face(1, vec![1.0, 0.0], 0.5);
        let b = face(2, vec![0.0, 1.0], 0.5); // orthogonal → cosine 0

        let pid_a = assign_face(&mut roster, &a, 0.9, &mut next_id, &HashSet::new());
        let pid_b = assign_face(&mut roster, &b, 0.9, &mut next_id, &HashSet::new());

        assert_ne!(pid_a, pid_b);
        assert_eq!(roster.len(), 2);
    }

    #[test]
    fn higher_quality_face_upgrades_the_cover() {
        let mut roster: Vec<RosterEntry> = Vec::new();
        let mut next_id = -1i64;
        let low = face(1, vec![1.0, 0.0], 0.3);
        let high = face(2, vec![1.0, 0.0], 0.9);

        assign_face(&mut roster, &low, 0.9, &mut next_id, &HashSet::new());
        assign_face(&mut roster, &high, 0.9, &mut next_id, &HashSet::new());

        assert_eq!(roster[0].cover_face_id, 2);
        assert_eq!(roster[0].cover_quality, 0.9);
    }

    // ── plan_recluster ──────────────────────────────────────────────────────

    fn person(id: i64, is_named: bool, is_ignored: bool, centroid: Vec<f32>) -> ReclusterPerson {
        ReclusterPerson {
            id,
            is_named,
            is_ignored,
            centroid,
        }
    }

    fn rface(
        id: i64,
        person_id: Option<i64>,
        embedding: Vec<f32>,
        quality: f32,
        is_confirmed: bool,
    ) -> ReclusterFace {
        ReclusterFace {
            id,
            person_id,
            embedding,
            quality,
            is_confirmed,
        }
    }

    #[test]
    fn recluster_merges_two_unnamed_fragments() {
        // 两个未命名碎片簇（互相相似），无确认/命名/忽略 → 应合并为一个新占位簇。
        let persons = vec![
            person(1, false, false, vec![1.0, 0.0]),
            person(2, false, false, vec![0.99, 0.14]),
        ];
        let faces = vec![
            rface(10, Some(1), vec![1.0, 0.0], 0.5, false),
            rface(20, Some(2), vec![0.99, 0.14], 0.5, false), // cosine ≈ 0.99 with face 10
        ];

        let updates = plan_recluster(persons, faces, 0.9, 0.0, &HashMap::new());

        // 单簇：一个新占位人物（id<0）收下两张脸；旧的两个空未命名人物不在 updates（落库时被删）。
        assert_eq!(updates.len(), 1);
        assert!(updates[0].id < 0);
        assert_eq!(updates[0].face_count, 2);
        let mut ids = updates[0].face_ids.clone();
        ids.sort();
        assert_eq!(ids, vec![10, 20]);
    }

    #[test]
    fn recluster_pins_confirmed_face_and_separates_dissimilar() {
        // person 1 有一张已确认锚脸；另有一张正交的未确认 free 脸 → 锚脸不动，free 脸另立新簇。
        let persons = vec![person(1, false, false, vec![1.0, 0.0])];
        let faces = vec![
            rface(10, Some(1), vec![1.0, 0.0], 0.5, true), // confirmed anchor
            rface(20, None, vec![0.0, 1.0], 0.5, false),   // orthogonal free face
        ];

        let updates = plan_recluster(persons, faces, 0.9, 0.0, &HashMap::new());

        // person 1 仍持有且仅持有确认脸 10；free 脸 20 落到一个新占位簇。
        let anchor = updates
            .iter()
            .find(|u| u.id == 1)
            .expect("anchored person 1 must remain");
        assert_eq!(anchor.face_ids, vec![10]);
        let other = updates
            .iter()
            .find(|u| u.id < 0)
            .expect("free face must form a new cluster");
        assert_eq!(other.face_ids, vec![20]);
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn recluster_keeps_named_person_via_stored_centroid() {
        // 已命名人物无确认脸，但库存质心能吸附其未确认成员 → 该人物（正 id）保留并收下成员。
        let persons = vec![person(7, true, false, vec![1.0, 0.0])];
        let faces = vec![rface(30, Some(7), vec![0.99, 0.14], 0.5, false)]; // unconfirmed member

        let updates = plan_recluster(persons, faces, 0.9, 0.0, &HashMap::new());

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].id, 7); // 命名人物身份保留，未变成新占位
        assert_eq!(updates[0].face_ids, vec![30]);
    }

    #[test]
    fn recluster_rejection_blocks_reattraction_to_named_person() {
        // 已命名人物 7（库存质心吸附力强）；脸 30 与其几乎同向，本会被吸入——但 (30,7) 已被拒绝，
        // 故 30 必须改立新占位簇，而非回到 person 7。验证负样本在全量重聚类生效（Part4 T3 StageB）。
        let persons = vec![person(7, true, false, vec![1.0, 0.0])];
        let faces = vec![rface(30, None, vec![0.99, 0.14], 0.5, false)]; // free，高度相似但被拒
        let mut rejections: HashMap<i64, HashSet<i64>> = HashMap::new();
        rejections.insert(30, HashSet::from([7]));

        let updates = plan_recluster(persons, faces, 0.9, 0.0, &rejections);

        // 脸 30 落到新占位簇（id<0），未回到 person 7。
        let new_cluster = updates.iter().find(|u| u.id < 0).expect("被拒脸应另立新簇");
        assert_eq!(new_cluster.face_ids, vec![30]);
        assert!(
            !updates
                .iter()
                .any(|u| u.id == 7 && u.face_ids.contains(&30)),
            "被拒脸绝不回到拒绝它的 person 7"
        );
    }

    #[test]
    fn recluster_rejection_allows_other_person() {
        // 脸 30 被拒于 person 7，但与 person 8 同样相似 → 应改投 person 8（拒绝只挡特定对，不挡全部）。
        let persons = vec![
            person(7, true, false, vec![1.0, 0.0]),
            person(8, true, false, vec![0.99, 0.14]),
        ];
        let faces = vec![rface(30, None, vec![1.0, 0.0], 0.5, false)];
        let mut rejections: HashMap<i64, HashSet<i64>> = HashMap::new();
        rejections.insert(30, HashSet::from([7]));

        let updates = plan_recluster(persons, faces, 0.9, 0.0, &rejections);

        // 30 进 person 8（正 id，非新占位），且不在 person 7。
        let p8 = updates
            .iter()
            .find(|u| u.id == 8)
            .expect("应改投未被拒的 person 8");
        assert!(p8.face_ids.contains(&30));
        assert!(
            !updates
                .iter()
                .any(|u| u.id == 7 && u.face_ids.contains(&30)),
            "不进被拒的 person 7"
        );
    }
}
