// src-tauri/src/layout/lens_folder.rs
//! 重复镜头 folders 模式的纯函数组装（2026-09-02 主画廊重复项浏览方案
//! §3.3 关联文件夹簇、§7.4 文件夹簇计算、§7.5 文件夹内稳定顺序）。
//!
//! 输入 = 三桶分类行（`list_duplicate_folder_lens_rows`）× 全库 canonical items，
//! 输出 = 关联文件夹簇序列（簇 → 目录 → 桶 → 行），供布局打包层（compute_layout
//! 的 Folders 分支）直接消费。簇关系用 Union-Find 的**组节点 star 连接**表达
//!（同组全部目录并入同一分量，O(k)——§7.4 明令不做 O(k²) pair）。
//!
//! 全局组序与 groups 模式共用 [`super::lens::cmp_group_order`] 单一事实源，使同一
//! 重复组在两种模式与跨文件夹块中的「组 N」可追踪（§7.3）。

use std::collections::{HashMap, HashSet};

use crate::db::models::LayoutItem;
use crate::db::queries::{DuplicateFolderLensRow, FolderLensBucket};

use super::geometry::{GallerySeparatorKind, LayoutParams, LayoutRow, SEPARATOR_HEIGHT};
use super::lens::{ascii_nocase_cmp, cmp_group_order};

/// 簇内一个直接文件夹块（§7.2 文件夹头的数据源）。
pub struct LensFolderBlock<'a> {
    pub directory_id: i64,
    /// 文件夹头 label（方案 §7.2 `{relativePath}`；来源 = 输入的显示路径映射）。
    pub display_path: String,
    /// 实际入块的重复行数（跳过查无 id 行后；§7.2「N 重复」）。
    pub duplicate_count: u32,
    /// 实际入块的尚未确认行数（§7.2「M 尚未确认」）。
    pub unconfirmed_count: u32,
    /// 实际入块的独有行数。**统计恒全量口径**：`showUniqueItems` 关闭时不进布局
    ///（打包层跳过），但此处计数不随开关变化——§7.2「K 独有（已隐藏）」要求
    /// 关闭开关时数量仍准确。
    pub unique_count: u32,
    /// 该目录 duplicate 成员所属全局组序，升序去重。
    pub group_ordinals: Vec<u32>,
    /// 目录内已按 §7.5 稳定序排好的行 × canonical item 引用。
    pub rows: Vec<(&'a DuplicateFolderLensRow, &'a LayoutItem)>,
}

/// 关联文件夹簇：通过一个或多个重复组直接或间接连通的文件夹集合（§3.3）。
pub struct LensFolderCluster<'a> {
    /// 确定性簇键 = 分量内最小 directory_id 的十进制字符串（separator `parentGroupId`）。
    pub component_key: String,
    /// 布局内簇短编号，1 起（簇头「关联文件夹簇 N」）。
    pub ordinal: u32,
    /// 簇内目录块（已按 §7.4 确定性遍历排序）。
    pub folders: Vec<LensFolderBlock<'a>>,
    /// 簇内重复组数（簇头统计）。
    pub group_count: u32,
}

/// 目录贪心遍历的元数据：重复位置数 / 目录内最新重复项时间 / 所属组集合。
type DirDupMeta<'a> = (u32, i64, HashSet<(&'a [u8], i64)>);

/// 小型并查集（路径压缩 + 按大小合并）。手写而非引依赖：仅本模块使用、约 40 行，
/// 引入 union-find crate 的收益撑不起依赖面。
struct Dsu {
    parent: Vec<usize>,
    size: Vec<u32>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // 路径压缩：沿途节点全部直挂根。
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        // 按大小合并（小树挂大树）。
        let (small, large) = if self.size[ra] < self.size[rb] {
            (ra, rb)
        } else {
            (rb, ra)
        };
        self.parent[small] = large;
        self.size[large] += self.size[small];
    }
}

/// folders 模式完整组装（纯函数）：三桶行 × canonical items → 簇序列
///（簇序 §7.4 → 簇内目录 §7.4 遍历 → 目录内行 §7.5 稳定序）。
///
/// - 查无 id 的行跳过（与 groups 组装同哲学：布局是证据的快照渲染，竞态瞬态随后续
///   刷新自愈）；三类计数随**实际入块**口径，与布局/flat 序严格一致。
/// - `showUniqueItems` 过滤不在此层：组装恒含 Unique 行（布局打包层按开关跳过 Unique，
///   `unique_count` 恒全量——§7.2「独有（已隐藏）」）。
pub struct LensFolderAssembly<'a> {
    /// 关联文件夹簇序列（簇序 §7.4）。
    pub clusters: Vec<LensFolderCluster<'a>>,
    /// 全局组序表：(unit_digest, unit_size) → 布局内组短编号（1 起）。
    /// 打包层投影「组 N」徽标复用（与 groups 模式 cmp_group_order 同源的产物）。
    pub group_ordinal_of: HashMap<(&'a [u8], i64), u32>,
}

pub fn assemble_lens_folder_clusters<'a>(
    rows: &'a [DuplicateFolderLensRow],
    items: &'a [LayoutItem],
    id_to_idx: &rustc_hash::FxHashMap<i64, u32>,
    dir_display: &HashMap<i64, String>,
) -> LensFolderAssembly<'a> {
    // ① 解析 canonical item 引用（查无即跳过）。
    let resolved: Vec<(&DuplicateFolderLensRow, &LayoutItem)> = rows
        .iter()
        .filter_map(|r| {
            id_to_idx
                .get(&r.item_id)
                .and_then(|&i| items.get(i as usize))
                .map(|it| (r, it))
        })
        .collect();
    if resolved.is_empty() {
        return LensFolderAssembly {
            clusters: Vec::new(),
            group_ordinal_of: HashMap::new(),
        };
    }

    // ② 全局组序（跨块稳定，§7.3）：Duplicate 行按 (unit_digest,unit_size) 分组，
    //    组序 = 组内最新 sort_datetime DESC → (digest,size) 字节 ASC（cmp_group_order，
    //    与 groups 模式同源）。
    let mut duplicate_groups: HashMap<(&[u8], i64), Vec<&DuplicateFolderLensRow>> = HashMap::new();
    for (r, _) in resolved
        .iter()
        .filter(|(r, _)| r.bucket == FolderLensBucket::Duplicate)
    {
        if let (Some(d), Some(s)) = (&r.unit_digest, r.unit_size) {
            duplicate_groups
                .entry((d.as_slice(), s))
                .or_default()
                .push(r);
        }
    }
    let mut ordered_groups: Vec<(&[u8], i64, i64)> = duplicate_groups
        .iter()
        .map(|(k, g)| {
            (
                k.0,
                k.1,
                g.iter().map(|r| r.sort_datetime).max().unwrap_or(i64::MIN),
            )
        })
        .collect();
    ordered_groups.sort_unstable_by(|a, b| cmp_group_order(a.2, a.0, a.1, b.2, b.0, b.1));
    let group_ordinal_of: HashMap<(&[u8], i64), u32> = ordered_groups
        .iter()
        .enumerate()
        .map(|(i, (d, s, _))| ((*d, *s), i as u32 + 1))
        .collect();

    // ③ DSU：先收集全部涉及目录建满索引，再按 Duplicate 组 star union（组内全部
    //    目录并入同一分量——同组即连通，O(组员数)）。
    let mut dir_index: HashMap<i64, usize> = HashMap::new();
    for (r, _) in resolved.iter() {
        dir_index.entry(r.directory_id).or_insert(usize::MAX);
    }
    let mut dir_ids: Vec<i64> = Vec::with_capacity(dir_index.len());
    for id in dir_index.keys().copied().collect::<Vec<_>>() {
        let idx = dir_ids.len();
        dir_ids.push(id);
        dir_index.insert(id, idx);
    }
    let mut dsu = Dsu::new(dir_ids.len());
    for g in duplicate_groups.values() {
        let mut iter = g
            .iter()
            .filter_map(|r| dir_index.get(&r.directory_id).copied());
        if let Some(first) = iter.next() {
            for other in iter {
                dsu.union(first, other);
            }
        }
    }

    // ④ 分量收集：DSU 根 → 目录集。
    let mut components: HashMap<usize, Vec<i64>> = HashMap::new();
    for (id, &node) in &dir_index {
        components.entry(dsu.find(node)).or_default().push(*id);
    }

    // ⑤ 每目录的行分组与贪心遍历元数据（重复位置数 / 最新重复时间 / 组集合）。
    let mut rows_by_dir: HashMap<i64, Vec<(&DuplicateFolderLensRow, &LayoutItem)>> = HashMap::new();
    for (r, it) in resolved {
        rows_by_dir.entry(r.directory_id).or_default().push((r, it));
    }
    let dup_meta: HashMap<i64, DirDupMeta> = rows_by_dir
        .iter()
        .map(|(dir, rs)| {
            let dups: Vec<&DuplicateFolderLensRow> = rs
                .iter()
                .filter(|(r, _)| r.bucket == FolderLensBucket::Duplicate)
                .map(|(r, _)| *r)
                .collect();
            let latest = dups
                .iter()
                .map(|r| r.sort_datetime)
                .max()
                .unwrap_or(i64::MIN);
            let groups: HashSet<(&[u8], i64)> = dups
                .iter()
                .filter_map(|r| match (&r.unit_digest, r.unit_size) {
                    (Some(d), Some(s)) => Some((d.as_slice(), s)),
                    _ => None,
                })
                .collect();
            (*dir, (dups.len() as u32, latest, groups))
        })
        .collect();

    // ⑥ 逐分量构建簇（当前分量全部展示后再进下一簇，§7.4 第 4 条）。
    // 组 → 目录倒排表（构建一次 O(行数)）：贪心的「与已展示集合共享组更多」用增量
    // 计数维护——shared[dir] 在组进入已展示集时对其余成员目录 +1。若在比较器里现场
    // 做集合交集，O(D²) 次比较 × 大组集交集在 1M 行域实测 37s（§15 预算 5s 爆表）；
    // 增量后总代价 O(D² + 总行数)，1M 域毫秒级。
    let mut group_members: HashMap<(&[u8], i64), Vec<i64>> = HashMap::new();
    for (dir, (_, _, groups)) in &dup_meta {
        for g in groups {
            group_members.entry(*g).or_default().push(*dir);
        }
    }
    let mut clusters: Vec<(String, i64, Vec<LensFolderBlock>, u32, i64)> = components
        .into_values()
        .map(|dirs| {
            let component_min_id = *dirs.iter().min().expect("分量至少含一目录");
            // 簇内目录确定性遍历（§7.4 第 1-3 条）：贪心每轮选
            // 重复位置数最多 → 与已展示集合共享组更多（增量计数）→ 最新重复项时间 →
            // 路径 → id。起点即首轮（已展示集为空、shared 恒 0）= 重复位置数最多者。
            let mut remaining: HashSet<i64> = dirs.iter().copied().collect();
            let mut shared: HashMap<i64, u32> = HashMap::new();
            let mut ordered_dirs: Vec<i64> = Vec::with_capacity(dirs.len());
            while !remaining.is_empty() {
                let pick = *remaining
                    .iter()
                    .max_by(|&&a, &&b| {
                        let (count_a, latest_a) = dup_meta
                            .get(&a)
                            .map(|(c, l, _)| (*c, *l))
                            .unwrap_or((0, i64::MIN));
                        let (count_b, latest_b) = dup_meta
                            .get(&b)
                            .map(|(c, l, _)| (*c, *l))
                            .unwrap_or((0, i64::MIN));
                        let shared_a = shared.get(&a).copied().unwrap_or(0);
                        let shared_b = shared.get(&b).copied().unwrap_or(0);
                        count_a
                            .cmp(&count_b)
                            .then_with(|| shared_a.cmp(&shared_b))
                            .then_with(|| latest_a.cmp(&latest_b))
                            .then_with(|| {
                                dir_display
                                    .get(&a)
                                    .map(String::as_str)
                                    .unwrap_or("")
                                    .cmp(dir_display.get(&b).map(String::as_str).unwrap_or(""))
                            })
                            .then_with(|| a.cmp(&b))
                    })
                    .expect("remaining 非空");
                remaining.remove(&pick);
                if let Some((_, _, groups)) = dup_meta.get(&pick) {
                    for g in groups {
                        for &m in group_members.get(g).into_iter().flatten() {
                            if m != pick {
                                *shared.entry(m).or_insert(0) += 1;
                            }
                        }
                    }
                }
                ordered_dirs.push(pick);
            }

            // 目录块：§7.5 目录内排序 + 三桶计数 + 组序号收集。
            let mut group_ids_in_cluster: HashSet<(&[u8], i64)> = HashSet::new();
            let mut latest_dup_in_cluster = i64::MIN;
            let blocks: Vec<LensFolderBlock> = ordered_dirs
                .into_iter()
                .map(|dir| {
                    let mut rs = rows_by_dir.remove(&dir).unwrap_or_default();
                    let group_ordinal =
                        |r: &DuplicateFolderLensRow| match (&r.unit_digest, r.unit_size) {
                            (Some(d), Some(s)) => group_ordinal_of
                                .get(&(d.as_slice(), s))
                                .copied()
                                .unwrap_or(u32::MAX),
                            _ => u32::MAX,
                        };
                    rs.sort_unstable_by(|(ra, _), (rb, _)| {
                        // bucket_rank = FolderLensBucket 判别序（Ord 派生，
                        // Duplicate=0 < Unconfirmed=1 < Unique=2，§7.5）。
                        ra.bucket
                            .cmp(&rb.bucket)
                            .then_with(|| match (ra.bucket, rb.bucket) {
                                (FolderLensBucket::Duplicate, FolderLensBucket::Duplicate) => {
                                    group_ordinal(ra)
                                        .cmp(&group_ordinal(rb))
                                        .then_with(|| rb.sort_datetime.cmp(&ra.sort_datetime))
                                        .then_with(|| ra.item_id.cmp(&rb.item_id))
                                }
                                _ => rb
                                    .sort_datetime
                                    .cmp(&ra.sort_datetime)
                                    .then_with(|| ascii_nocase_cmp(&ra.file_name, &rb.file_name))
                                    .then_with(|| ra.item_id.cmp(&rb.item_id)),
                            })
                    });
                    let count = |b: FolderLensBucket| {
                        rs.iter().filter(|(r, _)| r.bucket == b).count() as u32
                    };
                    let mut ordinals: Vec<u32> = rs
                        .iter()
                        .filter(|(r, _)| r.bucket == FolderLensBucket::Duplicate)
                        .map(|(r, _)| group_ordinal(r))
                        .collect();
                    ordinals.sort_unstable();
                    ordinals.dedup();
                    for (r, _) in rs
                        .iter()
                        .filter(|(r, _)| r.bucket == FolderLensBucket::Duplicate)
                    {
                        latest_dup_in_cluster = latest_dup_in_cluster.max(r.sort_datetime);
                        if let (Some(d), Some(s)) = (&r.unit_digest, r.unit_size) {
                            group_ids_in_cluster.insert((d.as_slice(), s));
                        }
                    }
                    LensFolderBlock {
                        directory_id: dir,
                        display_path: dir_display
                            .get(&dir)
                            .cloned()
                            .unwrap_or_else(|| "Unknown".into()),
                        duplicate_count: count(FolderLensBucket::Duplicate),
                        unconfirmed_count: count(FolderLensBucket::Unconfirmed),
                        unique_count: count(FolderLensBucket::Unique),
                        group_ordinals: ordinals,
                        rows: rs,
                    }
                })
                .collect();
            (
                component_min_id.to_string(),
                component_min_id,
                blocks,
                group_ids_in_cluster.len() as u32,
                latest_dup_in_cluster,
            )
        })
        .collect();

    // ⑦ 簇序（§7.4）：簇内最新重复项 DESC → component_key 数值 ASC。
    //    component_key 字符串是键/显示身份；排序取数值序（避免 "10" < "9" 字典序
    //    反直觉）——确定性不受影响，方案只要求稳定 tie-break。
    clusters.sort_unstable_by(|(_, min_a, _, _, latest_a), (_, min_b, _, _, latest_b)| {
        latest_b.cmp(latest_a).then_with(|| min_a.cmp(min_b))
    });

    let clusters = clusters
        .into_iter()
        .enumerate()
        .map(|(i, (key, _, folders, group_count, _))| LensFolderCluster {
            component_key: key,
            ordinal: i as u32 + 1,
            folders,
            group_count,
        })
        .collect();
    LensFolderAssembly {
        clusters,
        group_ordinal_of,
    }
}

/// folders 镜头布局（方案 §7/§11.2）：逐簇逐目录先发 `duplicateFolder` 文件夹头
///（label=显示路径、groupId=目录 id、`lens_folder` 增量携带关联簇与三桶统计、
/// `parent_group_start` 标记簇首），目录内按桶过滤后复用既有行打包核心，目录块间
/// `stitch_group_rows` 缝合（每目录一个缝合单元，簇/目录边界永不跨行）。
/// 返回 (rows, 总高, 逐项投影)。
///
/// `show_unique_items=false` 时 Unique 行不进布局（不进 flat_ids），但文件夹头的
/// `unique_count` 恒全量、`unique_hidden=true`（§7.2「独有（已隐藏）」）。
#[allow(clippy::too_many_arguments)]
pub fn compute_lens_folder_layout(
    assembly: &LensFolderAssembly,
    show_unique_items: bool,
    params: &LayoutParams,
    placeholder_aspect: Option<f64>,
    grid: bool,
) -> (Vec<LayoutRow>, f64, super::lens::LensProjection) {
    use super::geometry::LensFolderSeparator;
    use super::grid_pack::{grid_metrics, pack_grid_rows};
    use super::justified::pack_justified_rows;
    use super::lens::LensItemProjection;

    let gap = params.gap.max(0.0);
    let aspect = placeholder_aspect.unwrap_or_else(|| {
        let flat: Vec<&LayoutItem> = assembly
            .clusters
            .iter()
            .flat_map(|c| c.folders.iter())
            .flat_map(|b| b.rows.iter().map(|(_, it)| *it))
            .collect();
        super::geometry::median_measured_aspect(&flat)
    });
    let (cols, cell) = grid_metrics(params.container_width, params.target_row_height, gap);
    let bucket = |r: &DuplicateFolderLensRow| match r.bucket {
        FolderLensBucket::Duplicate => crate::db::models::DuplicateBucket::Duplicate,
        FolderLensBucket::Unconfirmed => crate::db::models::DuplicateBucket::Unconfirmed,
        FolderLensBucket::Unique => crate::db::models::DuplicateBucket::Unique,
    };

    let mut projection: super::lens::LensProjection = HashMap::new();
    let mut outs: Vec<(Vec<LayoutRow>, f64)> = Vec::new();
    for cluster in &assembly.clusters {
        for (fi, block) in cluster.folders.iter().enumerate() {
            let mut rows = vec![LayoutRow::Separator {
                y: 0.0,
                height: SEPARATOR_HEIGHT,
                separator_label: block.display_path.clone(),
                group_id: Some(block.directory_id.to_string()),
                epoch_day: None,
                separator_kind: Some(GallerySeparatorKind::DuplicateFolder),
                lens_folder: Some(LensFolderSeparator {
                    parent_group_id: cluster.component_key.clone(),
                    parent_group_ordinal: cluster.ordinal,
                    parent_group_folder_count: cluster.folders.len() as u32,
                    parent_group_group_count: cluster.group_count,
                    parent_group_start: fi == 0,
                    duplicate_count: block.duplicate_count,
                    unconfirmed_count: block.unconfirmed_count,
                    unique_count: block.unique_count,
                    unique_hidden: !show_unique_items,
                }),
                lens_group: None,
            }];
            let members: Vec<&LayoutItem> = block
                .rows
                .iter()
                .filter(|(r, _)| show_unique_items || r.bucket != FolderLensBucket::Unique)
                .map(|(_, it)| *it)
                .collect();
            let (body, y_end) = if grid {
                pack_grid_rows(&members, SEPARATOR_HEIGHT + gap, cols, cell, gap)
            } else {
                pack_justified_rows(&members, SEPARATOR_HEIGHT + gap, params, aspect)
            };
            rows.extend(body);
            for (r, it) in &block.rows {
                if !show_unique_items && r.bucket == FolderLensBucket::Unique {
                    continue;
                }
                let group_ordinal = if r.bucket == FolderLensBucket::Duplicate {
                    match (&r.unit_digest, r.unit_size) {
                        (Some(d), Some(s)) => assembly
                            .group_ordinal_of
                            .get(&(d.as_slice(), s))
                            .copied()
                            .unwrap_or(u32::MAX),
                        _ => u32::MAX,
                    }
                } else {
                    u32::MAX
                };
                projection.insert(
                    it.id,
                    LensItemProjection {
                        bucket: bucket(r),
                        group_ordinal,
                        // folders 模式组员跨目录分散，无组内序（§16）。
                        member_ordinal: None,
                        member_count: None,
                    },
                );
            }
            outs.push((rows, y_end));
        }
    }
    let rows = super::geometry::stitch_group_rows(outs);
    let total_height = rows.last().map(|r| r.y() + r.height()).unwrap_or(0.0);
    (rows, total_height, projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_item(id: i64, w: i64, h: i64, ts: i64) -> LayoutItem {
        LayoutItem {
            id,
            width: w,
            height: h,
            file_size: 0,
            sort_datetime: ts,
            file_format: "jpg".into(),
            media_type: "image".into(),
            is_live_photo: false,
            duration_ms: None,
            thumb_status: 1,
            thumb_path: None,
            thumbhash: None,
            is_favorited: false,
            rating: 0,
            color_label: 0,
            availability: "online".into(),
            dir_id: None,
            similarity: None,
            cache_key: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        item_id: i64,
        dir: i64,
        ts: i64,
        path: &str,
        name: &str,
        bucket: FolderLensBucket,
        digest: Option<&[u8]>,
        size: Option<i64>,
    ) -> DuplicateFolderLensRow {
        DuplicateFolderLensRow {
            item_id,
            directory_id: dir,
            sort_datetime: ts,
            normalized_dir_path: path.into(),
            file_name: name.into(),
            bucket,
            unit_digest: digest.map(|d| d.to_vec()),
            unit_size: size,
        }
    }

    fn dup(
        item_id: i64,
        dir: i64,
        ts: i64,
        path: &str,
        name: &str,
        digest: &[u8],
    ) -> DuplicateFolderLensRow {
        row(
            item_id,
            dir,
            ts,
            path,
            name,
            FolderLensBucket::Duplicate,
            Some(digest),
            Some(10),
        )
    }

    fn index(items: &[LayoutItem]) -> rustc_hash::FxHashMap<i64, u32> {
        items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.id, i as u32))
            .collect()
    }

    fn display(dirs: &[i64]) -> HashMap<i64, String> {
        dirs.iter().map(|&d| (d, format!("/dir{d}"))).collect()
    }

    /// §14「两个互不相关簇」：A-B 共享 X、B-C 共享 Y → 同簇；无关组 Z 单独成簇。
    #[test]
    fn bridging_and_separate_components() {
        let items: Vec<LayoutItem> = (1..=5).map(|i| mk_item(i, 100, 100, i)).collect();
        let rows = vec![
            dup(1, 1, 10, "/a", "a.jpg", b"X"),
            dup(2, 2, 20, "/b", "b.jpg", b"X"),
            dup(3, 2, 30, "/b", "c.jpg", b"Y"),
            dup(4, 3, 40, "/c", "d.jpg", b"Y"),
            dup(5, 9, 50, "/z", "e.jpg", b"Z"),
        ];
        let clusters =
            assemble_lens_folder_clusters(&rows, &items, &index(&items), &display(&[1, 2, 3, 9]))
                .clusters;
        assert_eq!(
            clusters.len(),
            2,
            "目录 1/2/3 经 X/Y 桥接为一簇，目录 9 独立"
        );
        let big = clusters.iter().find(|c| c.folders.len() == 3).unwrap();
        let dirs: Vec<i64> = big.folders.iter().map(|f| f.directory_id).collect();
        assert_eq!(
            dirs,
            vec![2, 3, 1],
            "起点=重复位最多的 dir2，其次与已展示共享组的 dir3（更新），末 dir1"
        );
        assert_eq!(big.group_count, 2);
        assert_eq!(
            clusters
                .iter()
                .map(|c| c.component_key.as_str())
                .collect::<Vec<_>>(),
            vec!["9", "1"],
            "簇序按簇内最新重复项 DESC：dir9(50) 更于桥接簇(dir3=40)"
        );
    }

    /// §7.4：无共享组的目录不连通 → 分簇；簇内起点 = 重复位置更多的目录。
    #[test]
    fn separate_cluster_when_no_shared_group() {
        let items: Vec<LayoutItem> = (1..=4).map(|i| mk_item(i, 100, 100, i)).collect();
        let rows = vec![
            dup(1, 8, 10, "/8", "a.jpg", b"S1"),
            dup(2, 8, 20, "/8", "b.jpg", b"S2"),
            dup(3, 9, 30, "/9", "c.jpg", b"S1"),
            dup(4, 7, 40, "/7", "e.jpg", b"T1"),
        ];
        let clusters =
            assemble_lens_folder_clusters(&rows, &items, &index(&items), &display(&[7, 8, 9]))
                .clusters;
        assert_eq!(clusters.len(), 2);
        let c89 = clusters.iter().find(|c| c.folders.len() == 2).unwrap();
        let dirs: Vec<i64> = c89.folders.iter().map(|f| f.directory_id).collect();
        assert_eq!(dirs, vec![8, 9], "起点 = 重复位置更多的 dir8");
        assert_eq!(c89.group_count, 2);
    }

    /// §7.3/§7.5：全局组序 = 组内最新时间 DESC；同一组在簇内不同目录块中 ordinal
    /// 一致（组徽标跨块可追踪）。
    #[test]
    fn global_group_order_stable_across_blocks() {
        let items: Vec<LayoutItem> = (1..=4).map(|i| mk_item(i, 100, 100, i)).collect();
        // 组 OLDER 最新 60 > 组 LATEST 最新 40 → OLDER ordinal 1。
        let rows = vec![
            dup(1, 1, 10, "/a", "a.jpg", b"OLDER"),
            dup(2, 2, 60, "/b", "b.jpg", b"OLDER"),
            dup(3, 1, 30, "/a", "c.jpg", b"LATEST"),
            dup(4, 2, 40, "/b", "d.jpg", b"LATEST"),
        ];
        let clusters =
            assemble_lens_folder_clusters(&rows, &items, &index(&items), &display(&[1, 2]))
                .clusters;
        assert_eq!(clusters.len(), 1);
        let c = &clusters[0];
        assert_eq!(
            c.folders[0].group_ordinals,
            vec![1, 2],
            "group_ordinals 语义 = 涉及的全局组序升序去重（OLDER=1、LATEST=2）"
        );
        assert_eq!(
            c.folders[1].group_ordinals,
            vec![1, 2],
            "同组跨块同 ordinal"
        );
        assert_eq!(c.group_count, 2);
    }

    /// §7.5 目录内三桶次序（duplicate 组序 ASC → unconf → unique）；
    /// 查无 id 的行跳过且计数随实际入块。
    #[test]
    fn in_folder_bucket_order_and_skip_counts() {
        let items: Vec<LayoutItem> = (1..=4).map(|i| mk_item(i, 100, 100, i)).collect();
        let rows = vec![
            row(
                3,
                1,
                30,
                "/a",
                "z.jpg",
                FolderLensBucket::Unique,
                Some(b"U1"),
                Some(5),
            ),
            dup(1, 1, 10, "/a", "a.jpg", b"G2"),
            row(
                4,
                1,
                40,
                "/a",
                "b.txt",
                FolderLensBucket::Unconfirmed,
                None,
                None,
            ),
            dup(2, 1, 20, "/a", "B.JPG", b"G1"),
            dup(9, 1, 50, "/a", "ghost.jpg", b"G3"), // 9 号查无 → 跳过
        ];
        let clusters =
            assemble_lens_folder_clusters(&rows, &items, &index(&items), &display(&[1])).clusters;
        let f = &clusters[0].folders[0];
        let ids: Vec<i64> = f.rows.iter().map(|(r, _)| r.item_id).collect();
        // duplicate 桶按组序（G1 最新 20 → ordinal 1；G2 → 2）→ [2, 1]，再 unconf(4)，unique(3)。
        assert_eq!(ids, vec![2, 1, 4, 3]);
        assert_eq!(
            (f.duplicate_count, f.unconfirmed_count, f.unique_count),
            (2, 1, 1),
            "跳过的幽灵行不计数"
        );
    }

    /// §7.4 簇序：簇内最新重复项 DESC；簇头数值稳定（§7.1）。
    #[test]
    fn cluster_order_and_label() {
        let items: Vec<LayoutItem> = (1..=3).map(|i| mk_item(i, 100, 100, i)).collect();
        let rows = vec![
            dup(1, 1, 10, "/a", "a.jpg", b"X"),
            dup(2, 3, 90, "/c", "c.jpg", b"Y"),
            dup(3, 4, 20, "/d", "d.jpg", b"Y"),
        ];
        let clusters =
            assemble_lens_folder_clusters(&rows, &items, &index(&items), &display(&[1, 3, 4]))
                .clusters;
        // 簇 {3,4} 最新 90 > 簇 {1} 的 10 → 在前。
        assert_eq!(clusters[0].component_key, "3");
        assert_eq!(clusters[1].component_key, "1");
        assert_eq!(clusters[0].ordinal, 1);
        assert_eq!(clusters[0].folders.len(), 2);
        assert_eq!(clusters[0].group_count, 1);
    }
    /// 测试断言用文件夹头元组（目录 id / 类别 / 是否簇首 / 三桶+隐藏标记）。
    type FolderHeadTuple<'a> = (
        &'a String,
        &'a GallerySeparatorKind,
        bool,
        (u32, u32, u32, bool),
    );

    /// compute_lens_folder_layout 端到端：文件夹头 separator（kind/簇首/计数/隐藏标记）、
    /// 独有项开关（关→不进布局且 unique_hidden=true）、投影（dup 组徽标/unconf 无组内序）、
    /// y 单调。
    #[test]
    fn folder_layout_end_to_end() {
        use super::super::geometry::{GallerySeparatorKind, LayoutParams};
        use crate::db::models::DuplicateBucket;

        let items: Vec<LayoutItem> = (1..=5).map(|i| mk_item(i, 100, 100, i)).collect();
        // 簇 {1,2}（组 G）；独立簇 {3}（组 H）；3 号是 unique、4 号 unconf。
        let rows = vec![
            dup(1, 1, 10, "/a", "a.jpg", b"G"),
            dup(2, 2, 20, "/b", "b.jpg", b"G"),
            row(
                3,
                1,
                30,
                "/a",
                "u.jpg",
                FolderLensBucket::Unique,
                Some(b"U"),
                Some(7),
            ),
            row(
                4,
                1,
                40,
                "/a",
                "n.jpg",
                FolderLensBucket::Unconfirmed,
                None,
                None,
            ),
            dup(5, 3, 50, "/c", "c.jpg", b"H"),
        ];
        let assembly =
            assemble_lens_folder_clusters(&rows, &items, &index(&items), &display(&[1, 2, 3]));
        // 容器 200：dir1 块 2 项 1 行、加 unique 后 3 项 2 行——总高随开关变化可断言。
        let params = LayoutParams {
            container_width: 200.0,
            target_row_height: 100.0,
            gap: 0.0,
            group_by: "none".into(),
            sort_within_group: "none".into(),
            seamless: false,
        };

        // 关独有项：dir1 块只装 dup+unconf，unique_count 恒 1 且 unique_hidden=true。
        let (rows_off, total_off, proj_off) =
            compute_lens_folder_layout(&assembly, false, &params, Some(1.0), false);
        let seps: Vec<&LayoutRow> = rows_off
            .iter()
            .filter(|r| matches!(r, LayoutRow::Separator { .. }))
            .collect();
        assert_eq!(seps.len(), 3, "三个文件夹头");
        let heads: Vec<FolderHeadTuple> = seps
            .iter()
            .filter_map(|r| match r {
                LayoutRow::Separator {
                    group_id,
                    separator_kind,
                    lens_folder,
                    ..
                } => lens_folder.as_ref().map(|f| {
                    (
                        group_id.as_ref().unwrap(),
                        separator_kind.as_ref().unwrap(),
                        f.parent_group_start,
                        (
                            f.duplicate_count,
                            f.unconfirmed_count,
                            f.unique_count,
                            f.unique_hidden,
                        ),
                    )
                }),
                _ => None,
            })
            .collect();
        assert!(heads
            .iter()
            .all(|(_, k, ..)| **k == GallerySeparatorKind::DuplicateFolder));
        let cluster_values: Vec<(String, bool, u32, u32, u32)> = seps
            .iter()
            .filter_map(|r| match r {
                LayoutRow::Separator {
                    separator_label,
                    lens_folder: Some(folder),
                    ..
                } => Some((
                    separator_label.clone(),
                    folder.parent_group_start,
                    folder.parent_group_ordinal,
                    folder.parent_group_folder_count,
                    folder.parent_group_group_count,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            cluster_values,
            vec![
                ("/dir3".into(), true, 1, 1, 1),
                ("/dir2".into(), true, 2, 2, 1),
                ("/dir1".into(), false, 2, 2, 1),
            ],
            "簇数值独立于路径标签，路径标签保持原样"
        );
        // 簇序 = 簇内最新重复项 DESC：簇 {3}(50) 在簇 {1,2}(20) 前。
        // heads[0]=dir3（独立簇首）：1 dup。
        assert_eq!(heads[0].0, "3");
        assert_eq!(
            (heads[0].1, heads[0].2, heads[0].3),
            (
                &GallerySeparatorKind::DuplicateFolder,
                true,
                (1, 0, 0, true)
            )
        );
        // heads[1]=dir2（簇 {1,2} 簇首：重复位数平局 → 最新重复时间 20>10 → dir2 起）。
        assert_eq!(heads[1].0, "2");
        assert_eq!(
            (heads[1].1, heads[1].2, heads[1].3),
            (
                &GallerySeparatorKind::DuplicateFolder,
                true,
                (1, 0, 0, true)
            )
        );
        // heads[2]=dir1（同簇非首）：1 dup + 1 unconf + 1 unique（隐藏）。
        assert_eq!(heads[2].0, "1");
        assert_eq!(
            (heads[2].1, heads[2].2, heads[2].3),
            (
                &GallerySeparatorKind::DuplicateFolder,
                false,
                (1, 1, 1, true)
            )
        );

        // flat 序：dir1(dup2? 组内序——dir1 内组 G 行 1 号) …具体：dir1 [1(unconf 后)…]——
        // 桶序 dup(1) → unconf(4)；dir2 dup(2)；dir3 dup(5)。unique(3) 不进。
        let flat: Vec<i64> = rows_off
            .iter()
            .filter_map(|r| match r {
                LayoutRow::Normal { items, .. } => {
                    Some(items.iter().map(|it| it.id).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(
            flat,
            vec![5, 2, 1, 4],
            "簇3 在前；簇内 dir2 起；unique 行被开关跳过"
        );
        // 投影：dup 组徽标、unconf 无组内序、被跳过的 unique 无投影。
        assert_eq!(proj_off[&1].bucket, DuplicateBucket::Duplicate);
        assert_eq!(
            proj_off[&1].group_ordinal, 2,
            "组 H(最新 50)=1、组 G(最新 20)=2"
        );
        assert_eq!(proj_off[&4].bucket, DuplicateBucket::Unconfirmed);
        assert_eq!(proj_off[&4].member_ordinal, None);
        assert!(!proj_off.contains_key(&3), "被隐藏的 unique 不进投影");
        let ys: Vec<f64> = rows_off.iter().map(|r| r.y()).collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "y 严格递增");

        // 开独有项：unique(3) 进 dir1 块末尾；unique_hidden=false。
        let (rows_on, total_on, proj_on) =
            compute_lens_folder_layout(&assembly, true, &params, Some(1.0), false);
        let flat_on: Vec<i64> = rows_on
            .iter()
            .filter_map(|r| match r {
                LayoutRow::Normal { items, .. } => {
                    Some(items.iter().map(|it| it.id).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(
            flat_on,
            vec![5, 2, 1, 4, 3],
            "簇3 在前；unique 行在 dir1 块末尾"
        );
        assert!(total_on > total_off);
        assert_eq!(proj_on[&3].bucket, DuplicateBucket::Unique);
    }
    /// S-P5 基准(非门禁,--release + --ignored 手动跑;方案 §15)。folders 纯函数层下界:
    /// 250K 组 × 2 成员(500K dup 行)+ 各目录 250 unconf/250 unique ≈ 1M 行域。
    /// 预算参考:1M 冷 folders(隐藏独有)布局目标 ≤5s(§15);本基准不含 SQL 取数段。
    /// cargo test --release --lib layout::lens_folder::tests:bench_lens_folders_1m -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_lens_folders_1m() {
        use std::time::Instant;
        const DIRS: i64 = 1_000;
        const DUP_TOTAL: i64 = 500_000; // 250K 组 × 2 成员，均匀分布 1000 目录
        let mut id = 0i64;
        let mut items: Vec<LayoutItem> = Vec::new();
        let mut rows: Vec<DuplicateFolderLensRow> = Vec::new();
        let mk = |dir: i64,
                  bucket: FolderLensBucket,
                  digest: Option<Vec<u8>>,
                  ts: i64,
                  items: &mut Vec<LayoutItem>,
                  rows: &mut Vec<DuplicateFolderLensRow>,
                  id: &mut i64| {
            *id += 1;
            items.push(mk_item(*id, 1600, 1200, ts));
            rows.push(DuplicateFolderLensRow {
                item_id: *id,
                directory_id: dir,
                sort_datetime: ts,
                normalized_dir_path: format!("/root/dir{dir:04}"),
                file_name: format!("f{id}.jpg"),
                bucket,
                unit_digest: digest,
                unit_size: Some(1024),
            });
        };
        for i in 0..DUP_TOTAL {
            let dir = i % DIRS + 1;
            let gid = i / 2;
            let digest = vec![(gid % 251) as u8, (gid / 251 % 251) as u8];
            mk(
                dir,
                FolderLensBucket::Duplicate,
                Some(digest),
                DUP_TOTAL - i,
                &mut items,
                &mut rows,
                &mut id,
            );
        }
        for i in 0..DUP_TOTAL / 2 {
            let dir = i % DIRS + 1;
            mk(
                dir,
                FolderLensBucket::Unconfirmed,
                None,
                DUP_TOTAL - i,
                &mut items,
                &mut rows,
                &mut id,
            );
        }
        for i in 0..DUP_TOTAL / 2 {
            let dir = i % DIRS + 1;
            mk(
                dir,
                FolderLensBucket::Unique,
                Some(vec![255, (i % 251) as u8]),
                DUP_TOTAL - i,
                &mut items,
                &mut rows,
                &mut id,
            );
        }
        let idx = index(&items);
        let dir_display: HashMap<i64, String> = (1..=DIRS)
            .map(|d| (d, format!("/root/dir{d:04}")))
            .collect();
        let params = LayoutParams {
            container_width: 1200.0,
            target_row_height: 240.0,
            gap: 4.0,
            group_by: "none".into(),
            sort_within_group: "none".into(),
            seamless: false,
        };
        let t = Instant::now();
        let assembly = assemble_lens_folder_clusters(&rows, &items, &idx, &dir_display);
        println!(
            "assemble folders 1M: {:.0}ms ({} clusters)",
            t.elapsed().as_secs_f64() * 1e3,
            assembly.clusters.len()
        );
        let t = Instant::now();
        let (rows_out, total, projection) =
            compute_lens_folder_layout(&assembly, false, &params, Some(1.333), false);
        println!(
            "pack folders (hidden unique): {:.0}ms ({} rows, {} proj, h={total:.0})",
            t.elapsed().as_secs_f64() * 1e3,
            rows_out.len(),
            projection.len()
        );
    }
}
