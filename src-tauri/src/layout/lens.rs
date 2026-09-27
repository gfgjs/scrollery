// src-tauri/src/layout/lens.rs
//! 重复镜头布局管线（2026-09-02 主画廊重复项浏览方案 §6/§11/§12，P1 按重复组模式）。
//!
//! 纯函数主战场：dedup 成员行（[`DuplicateLensMemberRow`]，SQL 无序、只投影排序键）×
//! 全库 canonical items 快照 → 组切片（§6.3 稳定序）→ 组头分隔符 + 既有 justified/grid
//! 打包核心 → 常驻 [`LayoutRow`]。组身份编码与 dedup IPC 共用单一事实源
//! （[`encode_group_key`]）；组间缝合复用 [`stitch_group_rows`]，行几何与普通画廊逐位一致。
//! 出口逐项投影（[`build_lens_projection`]）由 layout cache 常驻、出口拼装（hydrate_rows）
//! 按需填值——wire 字段位见 P0 契约（方案 §11.1/§11.2）。

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::db::models::{encode_group_key, DuplicateBucket, LayoutItem};
use crate::db::queries::DuplicateLensMemberRow;

use super::geometry::{
    median_measured_aspect, stitch_group_rows, GallerySeparatorKind, LayoutParams, LayoutRow,
    LensGroupSeparator, SEPARATOR_HEIGHT,
};
use super::grid_pack::{grid_metrics, pack_grid_rows};
use super::justified::pack_justified_rows;

/// 逐项镜头投影（方案 §11.1）：item id → 分类桶 + 组序号 + 组内序号 + 组成员数。
/// 常驻于 layout cache（[`super::cache::LayoutCacheData::lens_projection`]），出口拼装
/// 按可视区查表填 wire 字段——不在每项上复制路径或完整 group key。
pub type LensProjection = HashMap<i64, LensItemProjection>;

/// 组装期的组桶：成员证据行 × canonical item 引用（assemble_lens_groups 内部结构）。
type LensGroupBucket<'a> = Vec<(&'a DuplicateLensMemberRow, &'a LayoutItem)>;

/// 单项投影值（方案 §11.1）。`group_ordinal` 从 1 起；`member_ordinal`/`member_count`
/// 仅 groups 模式有值（组内第 M/共 N 项）——folders 模式组员跨目录分散、不保证组内
/// 连续（§16），组徽标只显示「组 N」，这两字段为 None（wire 不出键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LensItemProjection {
    /// 分类桶。groups 模式成员恒 `Duplicate`；folders 模式三桶皆有。
    pub bucket: DuplicateBucket,
    /// 布局内组短编号，1 起。
    pub group_ordinal: u32,
    /// 组内序号，1 起（groups 模式）。
    pub member_ordinal: Option<u32>,
    /// 组成员总数（groups 模式）。
    pub member_count: Option<u32>,
}

/// 镜头组切片：一组连续成员 + 组头元数据（方案 §6.1）。
pub struct LensGroupSlice<'a> {
    /// 重复组稳定 key（`encode_group_key(unit_digest, unit_size)`）：separator groupId +
    /// 选择契约的内部身份，不暴露摘要原文。
    pub group_key: String,
    /// 布局内组短编号，1 起（按组序分配，不暴露摘要原文）。
    pub ordinal: u32,
    /// 组成员数（组装后口径 = `members.len()`：查无 id 的成员已跳过，标签与投影的
    /// 「M/总数」以实际入布局的成员为准，竞态瞬态随后续刷新自愈）。
    pub member_count: u32,
    /// 组内不同 directory_id 数。
    pub folder_count: u32,
    /// 组内每份文件大小（unit_size，字节）。
    pub unit_size: i64,
    /// 已按组内稳定序排好（方案 §6.3）。
    pub members: Vec<&'a LayoutItem>,
}

/// ASCII 大写折叠比较（SQLite `COLLATE NOCASE` 等价）：仅折叠 `A-Z`，逐字节比较。
/// groups（§6.3 组内序）与 folders（§7.5 未确认/独有桶内序）两模式共用。
pub(crate) fn ascii_nocase_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn fold(b: u8) -> u8 {
        if b.is_ascii_uppercase() {
            b + 32
        } else {
            b
        }
    }
    a.bytes().map(fold).cmp(b.bytes().map(fold))
}

/// 组序比较（方案 §6.3，groups/folders 两模式共用的单一事实源，§7.3 组徽标「组 N」
/// 跨模式可追踪）：组内最新 `sort_datetime` DESC → `(unit_digest, unit_size)` 字节 ASC。
/// `latest_*` = 组内成员最大 sort_datetime；摘要按原始字节比较（非 base64 编码串——
/// 编码字符序 ≠ 摘要字节序）。
pub(crate) fn cmp_group_order(
    latest_a: i64,
    digest_a: &[u8],
    size_a: i64,
    latest_b: i64,
    digest_b: &[u8],
    size_b: i64,
) -> std::cmp::Ordering {
    Reverse(latest_a)
        .cmp(&Reverse(latest_b))
        .then_with(|| digest_a.cmp(digest_b))
        .then_with(|| size_a.cmp(&size_b))
}

/// 组装（方案 §6.3/§12.1）。组序：组内最新 `sort_datetime` DESC，`(unit_digest,unit_size)`
/// 字节 ASC；组内：`sort_datetime` DESC，`normalized_dir_path` ASC，`file_name`
/// ASCII-NOCASE ASC，`item_id` ASC。
///
/// 成员 `item_id` 在 `id_to_idx` 查无（dedup 行与布局换代的竞态瞬态、或成员落在隐藏根等
/// 视图排除集内）→ 跳过该成员不 panic。**跳过后成员数 <2 的组仍保留**：组头证据（摘要
/// 代次）随后续 `dedup_view_epoch` 刷新自愈，此处不丢组头以免布局 y 坐标抖动。
pub fn assemble_lens_groups<'a>(
    rows: &'a [DuplicateLensMemberRow],
    items: &'a [LayoutItem],
    id_to_idx: &rustc_hash::FxHashMap<i64, u32>,
) -> Vec<LensGroupSlice<'a>> {
    // 组身份 = (unit_digest, unit_size)，按**原始字节**做键：组序 tie-break 也要求原始
    // 字节序，不能基于 base64url 编码串（编码字符序 ≠ 摘要字节序）。
    let mut groups: HashMap<(Vec<u8>, i64), LensGroupBucket<'a>> = HashMap::new();
    for row in rows {
        // 查无即跳过（不 panic）：见函数注释。items.get 双重防御（越界不应发生）。
        let Some(&idx) = id_to_idx.get(&row.item_id) else {
            continue;
        };
        let Some(item) = items.get(idx as usize) else {
            continue;
        };
        groups
            .entry((row.unit_digest.clone(), row.unit_size))
            .or_default()
            .push((row, item));
    }

    // 组序（§6.3）：组内最新 sort_datetime DESC → (unit_digest,unit_size) 字节 ASC
    // （比较器与 folders 模式共用 cmp_group_order）。
    let mut acc: Vec<((Vec<u8>, i64), LensGroupBucket<'a>)> = groups.into_iter().collect();
    acc.sort_unstable_by(|(ka, ga), (kb, gb)| {
        let latest = |g: &LensGroupBucket| {
            g.iter()
                .map(|(r, _)| r.sort_datetime)
                .max()
                .unwrap_or(i64::MIN)
        };
        cmp_group_order(latest(ga), &ka.0, ka.1, latest(gb), &kb.0, kb.1)
    });

    acc.into_iter()
        .enumerate()
        .map(|(gi, (key, mut members))| {
            // 组内序（§6.3）：时间 DESC → 路径 ASC → 文件名 NOCASE ASC → id ASC。
            members.sort_unstable_by(|(ra, _), (rb, _)| {
                rb.sort_datetime
                    .cmp(&ra.sort_datetime)
                    .then_with(|| ra.normalized_dir_path.cmp(&rb.normalized_dir_path))
                    .then_with(|| ascii_nocase_cmp(&ra.file_name, &rb.file_name))
                    .then_with(|| ra.item_id.cmp(&rb.item_id))
            });
            LensGroupSlice {
                group_key: encode_group_key(&key.0, key.1),
                ordinal: gi as u32 + 1,
                member_count: members.len() as u32,
                folder_count: members
                    .iter()
                    .map(|(r, _)| r.directory_id)
                    .collect::<HashSet<i64>>()
                    .len() as u32,
                unit_size: key.1,
                members: members.into_iter().map(|(_, item)| item).collect(),
            }
        })
        .collect()
}

/// 逐项投影（方案 §11.1）：`member_ordinal` 从 1 起。P1 组内成员恒
/// [`DuplicateBucket::Duplicate`]——unique/unconfirmed 的发放点是 P3 folders 模式。
pub fn build_lens_projection(slices: &[LensGroupSlice]) -> LensProjection {
    let cap = slices.iter().map(|s| s.members.len()).sum();
    let mut out: LensProjection = HashMap::with_capacity(cap);
    for s in slices {
        for (i, m) in s.members.iter().enumerate() {
            out.insert(
                m.id,
                LensItemProjection {
                    bucket: DuplicateBucket::Duplicate,
                    group_ordinal: s.ordinal,
                    member_ordinal: Some(i as u32 + 1),
                    member_count: Some(s.member_count),
                },
            );
        }
    }
    out
}

/// 镜头布局公共骨架：每组先发 duplicateGroup 组头（局部 y=0），再委托对应打包核心装
/// 组内行，最后 [`stitch_group_rows`] 前缀和缝合。镜头不支持 seamless（§6.1）——组边界
/// 恒存在，组间不共享行。返回 (行集, 总高)；总高 = Σ(SEPARATOR_HEIGHT + gap + 行高 + gap)
/// （各组局部终 y 之和，含末组尾随 gap，与缝合的组高前缀和同源）。
fn compute_lens_layout(
    slices: &[LensGroupSlice],
    params: &LayoutParams,
    pack: impl Fn(&[&LayoutItem], f64) -> (Vec<LayoutRow>, f64),
) -> (Vec<LayoutRow>, f64) {
    let gap = params.gap.max(0.0);
    let outs: Vec<(Vec<LayoutRow>, f64)> = slices
        .iter()
        .map(|s| {
            let mut rows = Vec::new();
            let mut y = 0.0f64;
            rows.push(LayoutRow::Separator {
                y: 0.0,
                height: SEPARATOR_HEIGHT,
                // 组头文本由前端按当前 locale 生成；此字段仅为普通分隔行协议兼容而保留。
                separator_label: String::new(),
                group_id: Some(s.group_key.clone()),
                // 镜头组头无「日」概念（P3 时间比例坐标只服务 date 分隔符）。
                epoch_day: None,
                separator_kind: Some(GallerySeparatorKind::DuplicateGroup),
                // groups 模式组头无文件夹头语义。
                lens_folder: None,
                lens_group: Some(LensGroupSeparator {
                    ordinal: s.ordinal,
                    member_count: s.member_count,
                    folder_count: s.folder_count,
                    unit_size: s.unit_size,
                }),
            });
            y += SEPARATOR_HEIGHT + gap;
            let (body, y_end) = pack(&s.members, y);
            rows.extend(body);
            (rows, y_end)
        })
        .collect();
    let total_height = outs.iter().map(|(_, h)| h).sum();
    (stitch_group_rows(outs), total_height)
}

/// 镜头 justified 布局（等高行，方案 §11）：`placeholder_aspect` 语义同
/// `compute_justified_layout`——None = 对镜头成员集现算已测量项中位数（测试/独立调用）。
pub fn compute_lens_layout_justified(
    slices: &[LensGroupSlice],
    params: &LayoutParams,
    placeholder_aspect: Option<f64>,
) -> (Vec<LayoutRow>, f64) {
    let ph = placeholder_aspect.unwrap_or_else(|| {
        let flat: Vec<&LayoutItem> = slices
            .iter()
            .flat_map(|s| s.members.iter().copied())
            .collect();
        median_measured_aspect(&flat)
    });
    compute_lens_layout(slices, params, |members, y| {
        pack_justified_rows(members, y, params, ph)
    })
}

/// 镜头 grid 布局（均匀宫格，T20）：列数/单元边长公式与 `compute_grid_layout` 同源
/// （[`grid_metrics`]），保证两种入口的行几何一致。
pub fn compute_lens_layout_grid(
    slices: &[LensGroupSlice],
    params: &LayoutParams,
) -> (Vec<LayoutRow>, f64) {
    let (cols, cell) = grid_metrics(params.container_width, params.target_row_height, params.gap);
    let gap = params.gap.max(0.0);
    compute_lens_layout(slices, params, |members, y| {
        pack_grid_rows(members, y, cols, cell, gap)
    })
}
