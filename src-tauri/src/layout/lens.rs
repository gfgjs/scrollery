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

    fn member(
        digest: &[u8],
        size: i64,
        item_id: i64,
        dir: i64,
        ts: i64,
        path: &str,
        name: &str,
    ) -> DuplicateLensMemberRow {
        DuplicateLensMemberRow {
            unit_digest: digest.to_vec(),
            unit_size: size,
            item_id,
            directory_id: dir,
            sort_datetime: ts,
            normalized_dir_path: path.into(),
            file_name: name.into(),
        }
    }

    fn index_of(items: &[LayoutItem]) -> rustc_hash::FxHashMap<i64, u32> {
        items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.id, i as u32))
            .collect()
    }

    /// 组序：组内最新 sort_datetime DESC，平局按 (unit_digest, unit_size) 字节 ASC。
    #[test]
    fn group_order_latest_desc_then_digest_bytes_asc() {
        let items = vec![
            mk_item(1, 100, 100, 10),
            mk_item(2, 100, 100, 10),
            mk_item(3, 100, 100, 10),
            mk_item(4, 100, 100, 10),
        ];
        let idx = index_of(&items);
        // 组 X：最新 300；组 A：最新 300（平局）→ 字节序 b"A" < b"X" → A 在前。
        // 组 Z：最新 100 → 最后。
        let rows = vec![
            member(b"X", 10, 1, 1, 300, "/p", "a.jpg"),
            member(b"X", 10, 2, 1, 100, "/p", "b.jpg"),
            member(b"Z", 10, 3, 1, 100, "/p", "c.jpg"),
            member(b"A", 10, 4, 1, 300, "/p", "d.jpg"),
        ];
        let slices = assemble_lens_groups(&rows, &items, &idx);
        let keys: Vec<&str> = slices.iter().map(|s| s.group_key.as_str()).collect();
        let a = encode_group_key(b"A", 10);
        let x = encode_group_key(b"X", 10);
        let z = encode_group_key(b"Z", 10);
        assert_eq!(keys, vec![a.as_str(), x.as_str(), z.as_str()]);
        // ordinal 按组序 1 起。
        assert_eq!(
            slices.iter().map(|s| s.ordinal).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    /// 组内四键：时间 DESC → 路径 ASC → 文件名 ASCII-NOCASE → id。
    #[test]
    fn member_order_four_keys_with_ascii_nocase() {
        let items: Vec<LayoutItem> = (1..=5).map(|i| mk_item(i, 100, 100, 0)).collect();
        let idx = index_of(&items);
        let rows = vec![
            member(b"K", 1, 1, 1, 100, "/b", "a.jpg"),
            // 2 号时间最新 → 第一。
            member(b"K", 1, 2, 1, 200, "/a", "z.jpg"),
            // 3 号与 1 号同时间，路径 /a < /b → 第二（NOCASE 折叠 "b.jpg" 比较）。
            member(b"K", 1, 3, 1, 100, "/a", "B.JPG"),
            // 5 号与 1/4 同时间同路径，NOCASE "a.jpg" < "a.txt" → jpg 在前。
            member(b"K", 1, 5, 1, 100, "/b", "A.txt"),
            // 1/4 同时间同路径同名（NOCASE 平）→ item_id ASC。
            member(b"K", 1, 4, 1, 100, "/b", "a.jpg"),
        ];
        let slices = assemble_lens_groups(&rows, &items, &idx);
        assert_eq!(slices.len(), 1);
        let got: Vec<i64> = slices[0].members.iter().map(|m| m.id).collect();
        assert_eq!(got, vec![2, 3, 1, 4, 5]);
    }

    /// folder_count = 组内不同 directory_id 数；查无 id 的成员跳过但组保留（竞态自愈）。
    #[test]
    fn folder_count_and_missing_id_skip() {
        let items = vec![mk_item(1, 100, 100, 10), mk_item(2, 100, 100, 10)];
        let idx = index_of(&items);
        let rows = vec![
            member(b"P", 5, 1, 7, 10, "/a", "a.jpg"),
            member(b"P", 5, 2, 9, 10, "/b", "b.jpg"),
            member(b"P", 5, 3, 9, 10, "/b", "c.jpg"), // 3 号不在 items → 跳过
        ];
        let slices = assemble_lens_groups(&rows, &items, &idx);
        assert_eq!(slices.len(), 1, "跳过后仍有 2 名成员，组保留");
        let s = &slices[0];
        assert_eq!(s.member_count, 2, "member_count 以实际入布局的成员为准");
        assert_eq!(s.folder_count, 2);
        // 全员查无的组不产生空切片（无可渲染内容）。
        let rows2 = vec![member(b"Q", 5, 99, 1, 10, "/a", "x.jpg")];
        assert!(assemble_lens_groups(&rows2, &items, &idx).is_empty());
    }

    /// 组头仅输出结构化数值，文案由前端按 locale 生成。
    #[test]
    fn group_header_values_are_preserved() {
        let s = LensGroupSlice {
            group_key: "k".into(),
            ordinal: 12,
            member_count: 3,
            folder_count: 2,
            unit_size: 24 * 1024 * 1024,
            members: vec![],
        };
        let values = LensGroupSeparator {
            ordinal: s.ordinal,
            member_count: s.member_count,
            folder_count: s.folder_count,
            unit_size: s.unit_size,
        };
        assert_eq!(values.ordinal, 12);
        assert_eq!(values.member_count, 3);
        assert_eq!(values.folder_count, 2);
        assert_eq!(values.unit_size, 24 * 1024 * 1024);
    }

    /// 投影：ordinal 连续、member_ordinal 1 起、member_count 与切片一致。
    #[test]
    fn projection_is_one_based_and_consistent() {
        let items: Vec<LayoutItem> = (1..=4).map(|i| mk_item(i, 100, 100, i)).collect();
        let idx = index_of(&items);
        let rows = vec![
            member(b"A", 1, 1, 1, 10, "/a", "a.jpg"),
            member(b"A", 1, 2, 1, 20, "/a", "b.jpg"),
            member(b"B", 1, 3, 2, 30, "/b", "c.jpg"),
            member(b"B", 1, 4, 2, 40, "/b", "d.jpg"),
        ];
        let slices = assemble_lens_groups(&rows, &items, &idx);
        let p = build_lens_projection(&slices);
        assert_eq!(p.len(), 4);
        // 组 B（最新 40）在前 → ordinal 1；组内时间 DESC → member_ordinal 4→1, 3→2。
        assert_eq!(p[&4].group_ordinal, 1);
        assert_eq!(p[&4].member_ordinal, Some(1));
        assert_eq!(p[&3].member_ordinal, Some(2));
        assert_eq!(p[&1].group_ordinal, 2);
        assert_eq!(p[&2].member_ordinal, Some(1), "组 A 首位 = 最新成员 2 号");
        assert_eq!(p[&1].member_count, Some(2));
        assert_eq!(p[&1].bucket, DuplicateBucket::Duplicate);
    }

    /// justified 镜头布局：每组先发 DuplicateGroup 分隔符，组内行不跨组、成员序保持。
    #[test]
    fn justified_layout_emits_group_separators_and_keeps_order() {
        let items: Vec<LayoutItem> = (1..=6).map(|i| mk_item(i, 100, 100, i)).collect();
        let idx = index_of(&items);
        let rows = vec![
            member(b"A", 1, 1, 1, 10, "/a", "a.jpg"),
            member(b"A", 1, 2, 1, 20, "/a", "b.jpg"),
            member(b"A", 1, 3, 1, 30, "/a", "c.jpg"),
            member(b"B", 1, 4, 2, 40, "/b", "d.jpg"),
            member(b"B", 1, 5, 2, 50, "/b", "e.jpg"),
            member(b"B", 1, 6, 2, 60, "/b", "f.jpg"),
        ];
        let slices = assemble_lens_groups(&rows, &items, &idx);
        let params = LayoutParams {
            container_width: 400.0,
            target_row_height: 100.0,
            gap: 4.0,
            // 镜头忽略 group_by（打包核心不读它）；seamless 不支持恒 false。
            group_by: "none".into(),
            sort_within_group: "none".into(),
            seamless: false,
        };
        let (rows, total) = compute_lens_layout_justified(&slices, &params, Some(1.0));
        // 序列：Sep(B) Row(4,5,6) Sep(A) Row(1,2,3)。3 项 ar=3×100=300 < 392 → 不满行。
        let kinds: Vec<&str> = rows
            .iter()
            .map(|r| match r {
                LayoutRow::Separator { .. } => "sep",
                LayoutRow::Normal { .. } => "row",
            })
            .collect();
        assert_eq!(kinds, vec!["sep", "row", "sep", "row"]);
        match &rows[0] {
            LayoutRow::Separator {
                separator_label,
                group_id,
                separator_kind,
                epoch_day,
                lens_group,
                ..
            } => {
                assert_eq!(separator_kind, &Some(GallerySeparatorKind::DuplicateGroup));
                assert_eq!(group_id.as_deref(), Some(slices[0].group_key.as_str()));
                assert!(separator_label.is_empty());
                let values = lens_group.as_ref().expect("组头应携带结构化数值");
                assert_eq!(values.ordinal, slices[0].ordinal);
                assert_eq!(values.member_count, slices[0].member_count);
                assert_eq!(values.folder_count, slices[0].folder_count);
                assert_eq!(values.unit_size, slices[0].unit_size);
                assert_eq!(epoch_day, &None);
            }
            _ => panic!("首行应为镜头组头"),
        }
        // 成员序 = 组内序（时间 DESC）；y 单调递增。
        let b_ids: Vec<i64> = match &rows[1] {
            LayoutRow::Normal { items, .. } => items.iter().map(|it| it.id).collect(),
            _ => panic!(),
        };
        assert_eq!(b_ids, vec![6, 5, 4]);
        let ys: Vec<f64> = rows.iter().map(|r| r.y()).collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "y 必须严格递增");
        // 行几何与普通 justified 同源：3 项 ar 和 3.0 > 392×0.6 → 走撑满缩放，
        // 行高 = ceil((400-8)/3) = 131；总高 = 2 × (36+4+131+4)。
        let row_h = match &rows[1] {
            LayoutRow::Normal { height, .. } => *height,
            _ => panic!(),
        };
        assert_eq!(row_h, 131.0);
        let expected = (36.0 + 4.0 + row_h + 4.0) * 2.0;
        assert_eq!(total, expected);
    }

    /// grid 镜头布局：列数/单元与 compute_grid_layout 同源；组边界不跨行。
    #[test]
    fn grid_layout_packs_within_groups() {
        let items: Vec<LayoutItem> = (1..=4).map(|i| mk_item(i, 100, 100, i)).collect();
        let idx = index_of(&items);
        let rows = vec![
            member(b"A", 1, 1, 1, 10, "/a", "a.jpg"),
            member(b"A", 1, 2, 1, 20, "/a", "b.jpg"),
            member(b"A", 1, 3, 1, 30, "/a", "c.jpg"),
            member(b"B", 1, 4, 2, 40, "/b", "d.jpg"),
        ];
        let slices = assemble_lens_groups(&rows, &items, &idx);
        let params = LayoutParams {
            container_width: 300.0,
            target_row_height: 100.0,
            gap: 0.0,
            group_by: "none".into(),
            sort_within_group: "none".into(),
            seamless: false,
        };
        let (rows, total) = compute_lens_layout_grid(&slices, &params);
        let kinds: Vec<&str> = rows
            .iter()
            .map(|r| match r {
                LayoutRow::Separator { .. } => "sep",
                LayoutRow::Normal { .. } => "row",
            })
            .collect();
        // 组 B（最新 40）在前：1 项 1 行；组 A：3 项满 1 行（3 列）。
        assert_eq!(kinds, vec!["sep", "row", "sep", "row"]);
        match &rows[1] {
            LayoutRow::Normal { items, height, .. } => {
                assert_eq!(items.len(), 1);
                assert_eq!(*height, 100.0, "方格边长 = 单元 100");
            }
            _ => panic!(),
        }
        match &rows[3] {
            LayoutRow::Normal { items, .. } => assert_eq!(items.len(), 3),
            _ => panic!(),
        }
        assert_eq!(total, (36.0 + 0.0 + 100.0 + 0.0) * 2.0);
    }
    /// S-P5 基准(非门禁,--release + --ignored 手动跑;方案 §15 性能预算由 100K/1M 合成库
    /// 与真实库共同校准——此处为纯函数层下界,不含 SQL 取数段)。用法:
    /// cargo test --release --lib layout::lens::tests:bench_lens_groups_1m -- --ignored --nocapture
    /// 预算参考:1M 冷 groups 布局目标 ≤3s(§15);本基准覆盖 assemble+projection+pack 段。
    #[test]
    #[ignore]
    fn bench_lens_groups_1m() {
        use std::time::Instant;
        const N: i64 = 1_000_000; // 500K 组 × 2 成员
        let items: Vec<LayoutItem> = (0..N).map(|i| mk_item(i + 1, 1600, 1200, N - i)).collect();
        let idx = index_of(&items);
        let rows: Vec<DuplicateLensMemberRow> = (0..N)
            .map(|i| {
                let gid = i / 2;
                crate::db::queries::DuplicateLensMemberRow {
                    unit_digest: vec![(gid % 251) as u8, (gid / 251 % 251) as u8],
                    unit_size: 1024,
                    item_id: i + 1,
                    directory_id: (i % 1000) + 1,
                    sort_datetime: N - i,
                    normalized_dir_path: format!("/root/dir{:04}", i % 1000),
                    file_name: format!("f{i}.jpg"),
                }
            })
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
        let slices = assemble_lens_groups(&rows, &items, &idx);
        println!(
            "assemble 1M (500K groups): {:.0}ms",
            t.elapsed().as_secs_f64() * 1e3
        );
        let t = Instant::now();
        let projection = build_lens_projection(&slices);
        println!(
            "projection: {:.0}ms ({} ids)",
            t.elapsed().as_secs_f64() * 1e3,
            projection.len()
        );
        let t = Instant::now();
        let (rows_out, total) = compute_lens_layout_justified(&slices, &params, Some(1.333));
        println!(
            "pack justified: {:.0}ms ({} rows, h={total:.0})",
            t.elapsed().as_secs_f64() * 1e3,
            rows_out.len()
        );
    }
}
