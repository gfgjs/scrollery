// src-tauri/src/layout/geometry.rs
//! justified / grid 两种打包算法共享的骨架:输出类型、布局参数、组间并行骨架
//! (`layout_groups_parallel`)、分组边界辅助(`group_mark`/`group_label`)、出口
//! 拼装(`hydrate_item`/`placeholder_item`)、几何辅助(`aspect_ratio`/
//! `median_measured_aspect`)。拆分自 justified.rs(tierB-4 简案)。

use std::borrow::Borrow;
use std::collections::HashMap;

use chrono::{Datelike, TimeZone, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::models::{DirLabel, DuplicateBucket, LayoutItem};
use crate::thumbnail::serve::ThumbServe;
use crate::thumbnail::thumbhash::average_color_hex;

// ── Output types ─────────────────────────────────────────────────────────────
// ── 输出类型 ─────────────────────────────────────────────────────────────

/// 常驻布局行（S3 几何/载荷分离）：Normal 行仅存瘦行项（id + 几何），厚载荷在出口
///(get_layout_rows 系命令)经 items 取数缓存按可视区拼装为 [`HydratedRow`]。
/// 百万项下常驻 ~32B/项（原 ~200B + 5 段堆载荷），重排零载荷克隆、旧代 drop 近零。
#[derive(Debug, Clone)]
pub enum LayoutRow {
    Separator {
        y: f64,
        height: f64,
        separator_label: String,
        group_id: Option<String>,
        /// date 分组该日的 epoch 天数（`sort_datetime.div_euclid(86400)`，与 [`GroupMark::Day`]
        /// 同源）；folder/none 分组 = None。物化进 [`crate::layout::cache::SeparatorInfo`] 供 P3
        /// 时间比例坐标定位（见该字段注释）。
        epoch_day: Option<i64>,
        /// 分隔符类别（方案 §11.2）。P1 最小增量：镜头组头填 `Some(DuplicateGroup)`；
        /// date/folder 路径恒 None——出口 hydrate 原样透传，普通画廊线上 JSON 不含
        /// separatorKind 键（wire 位级不变）。`DuplicateFolder` 的生产者是 P3 folders。
        separator_kind: Option<GallerySeparatorKind>,
        /// folders 镜头文件夹头增量（方案 §7.1/§7.2/§11.2，P3）：关联簇信息 + 三桶计数，
        /// 打包成单 struct——现有构造点只补一个 None，出口 hydrate 展开到 wire 的
        /// parentGroup*/计数平铺字段。普通画廊/镜头组头恒 None。
        lens_folder: Option<LensFolderSeparator>,
        /// groups 镜头组头数值；组头文案由前端按 locale 生成。
        lens_group: Option<LensGroupSeparator>,
    },
    Normal {
        y: f64,
        height: f64,
        items: Vec<SlimRowItem>,
    },
}

impl LayoutRow {
    pub fn y(&self) -> f64 {
        match self {
            LayoutRow::Separator { y, .. } => *y,
            LayoutRow::Normal { y, .. } => *y,
        }
    }

    pub fn height(&self) -> f64 {
        match self {
            LayoutRow::Separator { height, .. } => *height,
            LayoutRow::Normal { height, .. } => *height,
        }
    }
}

/// 常驻瘦行项（S3）：id + 行内几何，Copy、零堆载荷。
#[derive(Debug, Clone, Copy)]
pub struct SlimRowItem {
    pub id: i64,
    pub x: f64,
    pub w: f64,
    pub h: f64,
}

/// 线上（IPC）布局行：serde 形状与 S3 前的 LayoutRow 完全一致（rowType 标签、camelCase），
/// **前端零改动**。由出口拼装产生（[`crate::layout::items_cache::hydrate_rows`]），不常驻。
///
/// 重复镜头增量（2026-09-02 主画廊重复项浏览方案 §11.2）：Separator 变体的镜头字段全部
/// 可选 + `skip_serializing_if`——普通画廊（P0 起的既有路径）序列化时这些键**不存在**，
/// wire 形状与方案前逐字节一致；P1 镜头布局实现时才由镜头出口填值。`default` 使旧 JSON
/// （无新键）仍可反序列化（双向兼容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "rowType")]
pub enum HydratedRow {
    #[serde(rename = "separator")]
    Separator {
        y: f64,
        height: f64,
        #[serde(rename = "separatorLabel")]
        separator_label: String,
        #[serde(rename = "groupId")]
        group_id: Option<String>,
        /// 分隔符类别（普通画廊现状 = date/folder；镜头 = duplicateGroup/duplicateFolder）。
        #[serde(
            rename = "separatorKind",
            skip_serializing_if = "Option::is_none",
            default
        )]
        separator_kind: Option<GallerySeparatorKind>,
        /// 关联文件夹簇（folders 模式）：该文件夹头所属簇的组 id / 是否簇内首个文件夹。
        #[serde(
            rename = "parentGroupId",
            skip_serializing_if = "Option::is_none",
            default
        )]
        parent_group_id: Option<String>,
        #[serde(
            rename = "parentGroupStart",
            skip_serializing_if = "Option::is_none",
            default
        )]
        parent_group_start: Option<bool>,
        /// 文件夹头/组头的三桶计数（方案 §3.4）与独有项是否被顶栏开关隐藏。
        #[serde(
            rename = "duplicateCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        duplicate_count: Option<u32>,
        #[serde(
            rename = "unconfirmedCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        unconfirmed_count: Option<u32>,
        #[serde(
            rename = "uniqueCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        unique_count: Option<u32>,
        #[serde(
            rename = "uniqueHidden",
            skip_serializing_if = "Option::is_none",
            default
        )]
        unique_hidden: Option<bool>,
        /// groups 镜头组头数值；`separatorLabel` 对该类别为空，前端按 locale 生成文案。
        #[serde(
            rename = "duplicateGroupOrdinal",
            skip_serializing_if = "Option::is_none",
            default
        )]
        duplicate_group_ordinal: Option<u32>,
        #[serde(
            rename = "duplicateMemberCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        duplicate_member_count: Option<u32>,
        #[serde(
            rename = "duplicateFolderCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        duplicate_folder_count: Option<u32>,
        #[serde(
            rename = "duplicateUnitSize",
            skip_serializing_if = "Option::is_none",
            default
        )]
        duplicate_unit_size: Option<i64>,
        /// folders 簇头数值；文件夹 `separatorLabel` 仍是原始显示路径。
        #[serde(
            rename = "parentGroupOrdinal",
            skip_serializing_if = "Option::is_none",
            default
        )]
        parent_group_ordinal: Option<u32>,
        #[serde(
            rename = "parentGroupFolderCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        parent_group_folder_count: Option<u32>,
        #[serde(
            rename = "parentGroupGroupCount",
            skip_serializing_if = "Option::is_none",
            default
        )]
        parent_group_group_count: Option<u32>,
    },
    #[serde(rename = "normal")]
    Normal {
        y: f64,
        height: f64,
        items: Vec<LayoutRowItem>,
    },
}

/// 分隔符类别（方案 §11.2）：date/folder 为普通画廊既有分组；
/// duplicateGroup=重复镜头组头，duplicateFolder=镜头文件夹头。
/// wire: camelCase 变体名（"duplicateGroup"/"duplicateFolder"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GallerySeparatorKind {
    Date,
    Folder,
    DuplicateGroup,
    DuplicateFolder,
}

/// folders 镜头文件夹头增量（方案 §7.1/§7.2/§11.2）：关联簇（parentGroup*）与
/// 文件夹三桶统计。常驻布局行携带、出口 hydrate 展开到 wire 平铺字段。
#[derive(Debug, Clone)]
pub struct LensFolderSeparator {
    /// 所属关联簇键（§7.1；同时是跳转轴的簇身份）。
    pub parent_group_id: String,
    /// 簇头序号（仅簇首文件夹头渲染——§7.2 首个文件夹头同时显示关联簇信息）。
    pub parent_group_ordinal: u32,
    pub parent_group_folder_count: u32,
    pub parent_group_group_count: u32,
    /// 是否簇内首个文件夹（前端据此把簇头并入该文件夹头，避免双层 sticky）。
    pub parent_group_start: bool,
    pub duplicate_count: u32,
    pub unconfirmed_count: u32,
    /// 独有项总数（含被开关隐藏的——§7.2 数量恒准确）。
    pub unique_count: u32,
    /// 独有项当前是否被顶栏开关隐藏。
    pub unique_hidden: bool,
}

/// groups 镜头组头数值。文本不在 Rust 生成，避免把某个 locale 固定进布局缓存。
#[derive(Debug, Clone, Copy)]
pub struct LensGroupSeparator {
    pub ordinal: u32,
    pub member_count: u32,
    pub folder_count: u32,
    pub unit_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 线上（IPC）逐项行数据（S3 后不再常驻）：由出口拼装（[`hydrate_item`]）按可视区从
/// items 取数缓存现场组装，serde 形状与 S3 前完全一致（前端零改动）。重型元数据
///(文件名、目录路径、EXIF、GPS)仍在 `MediaMeta`,仅为可视区按需拉取。
pub struct LayoutRowItem {
    pub id: i64,
    pub x: f64,
    pub w: f64,
    pub h: f64,
    pub file_size: i64,
    pub file_format: String,
    pub media_type: String,
    pub is_live_photo: bool,
    pub duration_ms: Option<i64>,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    /// 占位平均色 CSS `#rrggbb`（None → 前端回退 CSS 变量占位）。原字段 `thumbhash: Vec<u8>`
    /// 每项过桥 ~28 字节数组(JSON 化 ~100B)、前端每帧逐格现算均色;改由后端 hydrate 时算一次
    ///(`average_color_hex`),线上载荷暴跌 + 消灭渲染热路径逐格均色计算(乙:载荷瘦身)。
    pub placeholder_color: Option<String>,
    pub is_favorited: bool,
    /// 用户评分 0-5（0 = 未评分）。与 is_favorited 同类的逐项小标量，供网格星级显示 + hover 快捷评分。
    pub rating: i64,
    /// 用户颜色标签 0-7（0 = 未标）。与 rating 同类的逐项小标量，供网格 swatch 显示 + 按色筛选（T16）。
    pub color_label: i64,
    /// 系统可用态 'online'|'offline'|'missing'（前端置灰+角标；缺失检测 Part2 §3.2）。
    pub availability: String,
    pub similarity: Option<f64>,
    pub original_width: i64,
    pub original_height: i64,
    pub sort_datetime: i64,
    /// 重复镜头逐项增量（方案 §11.1，P1 起才有生产者）：分类桶 + 组内/组间序号 + 组成员数。
    /// 全部可选 + skip：普通画廊线上 JSON 不含这些键（wire 不膨胀）；常驻 `LayoutRowItem`
    /// 不加字段——P1 镜头布局在出口拼装层按需填值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_bucket: Option<DuplicateBucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_group_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_member_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_member_count: Option<u32>,
}

// ── Layout parameters ─────────────────────────────────────────────────────────
// ── 布局参数 ─────────────────────────────────────────────────────────

pub struct LayoutParams {
    pub container_width: f64,
    pub target_row_height: f64,
    pub gap: f64,
    pub group_by: String,
    pub sort_within_group: String,
    /// 无缝分组(#1,2026-07-17):item 序仍按 `group_by` 聚合排序(SQL/derive_order 不受影响),
    /// 但打包按 `none` 语义——单段连续行、不发分隔符、组末不强制断行。视觉上如不分组,
    /// 数据上保留组聚合。`group_by == "none"` 时本标志无效果。
    pub seamless: bool,
}

/// 分隔符固定高度(justified / grid 共用)。
pub(super) const SEPARATOR_HEIGHT: f64 = 36.0;

// ── Shared helpers (justified + grid) ──────────────────────────────────────────
// ── 共享辅助（justified + grid 两种布局共用）────────────────────────────────────

/// 出口拼装（S3 几何/载荷分离）：瘦行项几何 + `LayoutItem` 载荷 → 线上行项。字段集与
/// S3 前的常驻行完全一致（serde 形状不变，前端零改动）；仅对可视区行调用（10^2 级），
/// 载荷克隆成本无关紧要。新增 LayoutRowItem 字段只需改这一处（与 placeholder_item 成对）。
///
/// `serve`(多档源,2026-08-16 阶段 2):Some 时对 thumb_status=1 的项按「格尺寸×DPR 的最小
/// 满足且磁盘存在的档位」重写 thumb_path(缺档回退 DB 路径);None = 不改写(测试/兼容)。
pub fn hydrate_item(
    item: &LayoutItem,
    slot: &SlimRowItem,
    serve: Option<&ThumbServe>,
) -> LayoutRowItem {
    let thumb_path = match (&item.thumb_path, serve) {
        (Some(p), Some(s)) if item.thumb_status == 1 && !p.is_empty() => s
            .rewrite_path(p, slot.w, slot.h, item.cache_key)
            .or_else(|| item.thumb_path.clone()),
        _ => item.thumb_path.clone(),
    };
    LayoutRowItem {
        id: slot.id,
        x: slot.x,
        w: slot.w,
        h: slot.h,
        file_size: item.file_size,
        file_format: item.file_format.clone(),
        media_type: item.media_type.clone(),
        is_live_photo: item.is_live_photo,
        duration_ms: item.duration_ms,
        thumb_status: item.thumb_status,
        thumb_path,
        // 占位色在此算一次(仅可视区行调用),替代 thumbhash 数组过桥 + 前端逐格逐帧现算。
        placeholder_color: item.thumbhash.as_deref().and_then(average_color_hex),
        is_favorited: item.is_favorited,
        rating: item.rating,
        color_label: item.color_label,
        availability: item.availability.clone(),
        similarity: item.similarity,
        original_width: item.width,
        original_height: item.height,
        sort_datetime: item.sort_datetime,
        // P0 契约冻结：镜头字段无生产者，恒 None（P1 镜头出口才填值）。
        duplicate_bucket: None,
        duplicate_group_ordinal: None,
        duplicate_member_ordinal: None,
        duplicate_member_count: None,
    }
}

/// items 取数缓存查无此 id 时的占位行项（布局与快照换代的竞态窗口/清库后未重算的瞬态）：
/// 几何保留使行形不塌（虚拟滚动行高稳定），载荷置空，thumb_status=0 → 前端按待生成
/// 骨架渲染，下次布局换代自愈。availability 置空串（≠ "offline"，不触发置灰样式）。
pub fn placeholder_item(slot: &SlimRowItem) -> LayoutRowItem {
    LayoutRowItem {
        id: slot.id,
        x: slot.x,
        w: slot.w,
        h: slot.h,
        file_size: 0,
        file_format: String::new(),
        media_type: String::new(),
        is_live_photo: false,
        duration_ms: None,
        thumb_status: 0,
        thumb_path: None,
        placeholder_color: None,
        is_favorited: false,
        rating: 0,
        color_label: 0,
        availability: String::new(),
        similarity: None,
        original_width: 0,
        original_height: 0,
        sort_datetime: 0,
        // 占位行项与镜头语义无关（竞态窗口瞬态），镜头字段恒 None。
        duplicate_bucket: None,
        duplicate_group_ordinal: None,
        duplicate_member_ordinal: None,
        duplicate_member_count: None,
    }
}

/// 分组边界的逐项廉价键（S2 布局消脂）：date = UTC 日序数（`div_euclid(86400)` 与
/// `timestamp_to_date_label` 的 Utc 日界严格一致），folder = 目录 id。百万项下逐项
/// chrono 格式化 + 2 个 String 分配曾占布局段大头，现降为纯整数比较，标签仅在边界
/// 变化处构造（调用量级 10^6 → 10^3，见 group_label）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupMark {
    Day(i64),
    Dir(i64),
    /// folder 分组但 dir_id 缺失（列 NOT NULL，理论死分支，防御保留）：标签走 "Unknown"。
    DirUnknown,
}

pub(super) fn group_mark(item: &LayoutItem, group_by: &str) -> GroupMark {
    match group_by {
        "folder" => item
            .dir_id
            .map(GroupMark::Dir)
            .unwrap_or(GroupMark::DirUnknown),
        _ => GroupMark::Day(item.sort_datetime.div_euclid(86400)),
    }
}

/// 分组标签：`(分隔符标签, group_id)`。justified 与 grid 共用，确保两种布局的分隔符/月桶
/// 边界严格一致（grid 复用同样的 `YYYY-MM` group_id → get_summary 月桶推导零改动）。
/// **仅在 GroupMark 边界变化时调用**（原 group_key 逐项调用，S2 优化）。
/// - folder：展示路径经 DirLabel 映射还原（语义 = 原 SQL 逐行拼接的 dir_path；空路径回退
///   目录名/Root，查无映射回退 "Unknown"），group_id = 目录 id 字符串。
/// - none：空标签、无 group_id（调用方据此不产分隔符）。
/// - date（默认）：中文日标签 + `YYYY-MM` group_id（T14 §3.8.2）。
///
/// 返回 `(label, group_id, epoch_day)`。epoch_day 仅 date 分组非空（该日 UTC 天数，供 P3
/// 时间比例坐标），folder/none = None。
pub(super) fn group_label(
    item: &LayoutItem,
    group_by: &str,
    dir_labels: &HashMap<i64, DirLabel>,
) -> (String, Option<String>, Option<i64>) {
    match group_by {
        "folder" => {
            let Some(dir_id) = item.dir_id else {
                return ("Unknown".to_string(), None, None);
            };
            let name = match dir_labels.get(&dir_id) {
                Some(dl) if dl.display.is_empty() => {
                    if dl.name.is_empty() {
                        "Root".to_string()
                    } else {
                        dl.name.clone()
                    }
                }
                Some(dl) => dl.display.clone(),
                None => "Unknown".to_string(),
            };
            (name, Some(dir_id.to_string()), None)
        }
        "none" => ("".to_string(), None, None),
        // date 分组：epoch_day 与 `GroupMark::Day` 同算式（div_euclid 保证 UTC 日界一致）。
        _ => (
            timestamp_to_date_label(item.sort_datetime),
            Some(timestamp_to_year_month(item.sort_datetime)),
            Some(item.sort_datetime.div_euclid(86400)),
        ),
    }
}

// ── 并行骨架（S3.4）───────────────────────────────────────────────────────────

/// S3.4 组间并行布局骨架（justified / grid 共用）：按 GroupMark 切连续段 → rayon 并行
/// 逐组打包（组内 y 从局部 0 起）→ 组高前缀和缝合绝对 y。
///
/// **顺序等价**：分组语义/行几何与原顺序实现逐行相同；y 均为整数和（分隔符 36 + gap、
/// 行高 ceil + gap），「基线 + 局部 y」与顺序累加**位级一致**（整数在 f64 中精确，
/// 特征化测试锁定）。none 分组 = 单段——行打包链式依赖（断行取决于前一行终点），
/// 组内不可切分，该轴无并行收益（维持原顺序耗时）。
///
/// ⚠️ 泛型签名 `I: Borrow<LayoutItem> + Sync` 与传入闭包 `F: Fn(&[I]) -> (Vec<LayoutRow>, f64)`
/// 须逐字保持——不得为「兼容拆分」改成 `Box<dyn Fn>`/trait object,百万项热路径不得引入
/// 间接调用与堆分配(tierB-4 简案硬约束)。
pub(super) fn layout_groups_parallel<I, F>(
    items: &[I],
    group_by: &str,
    pack_group: F,
) -> Vec<LayoutRow>
where
    I: Borrow<LayoutItem> + Sync,
    F: Fn(&[I]) -> (Vec<LayoutRow>, f64) + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }
    // 段表：连续同 GroupMark 区间（整数比较 O(N) 轻扫，百万项数 ms 级）。
    let mut segments: Vec<(usize, usize)> = Vec::new();
    if group_by == "none" {
        segments.push((0, items.len()));
    } else {
        let mut start = 0usize;
        let mut last = group_mark(items[0].borrow(), group_by);
        for (i, it) in items.iter().enumerate().skip(1) {
            let mark = group_mark(it.borrow(), group_by);
            if mark != last {
                segments.push((start, i));
                start = i;
                last = mark;
            }
        }
        segments.push((start, items.len()));
    }

    // 并行逐组打包（rayon 全局池；调用方已运行在 spawn_blocking 阻塞线程上）。
    let outs: Vec<(Vec<LayoutRow>, f64)> = segments
        .par_iter()
        .map(|&(start, end)| pack_group(&items[start..end]))
        .collect();
    stitch_group_rows(outs)
}

/// 组高前缀和缝合（layout_groups_parallel 与镜头布局 `compute_lens_layout_*` 共用）：
/// 各组局部行集 + 局部高 → 绝对 y 偏移 + 顺序拼接。
///
/// **y 语义**：均为整数和（分隔符 36 + gap、行高 ceil + gap），「基线 + 局部 y」与顺序
/// 累加**位级一致**（整数在 f64 中精确，特征化测试 parallel_stitch_yields_sequential_y_positions
/// 锁定）。2026-09-02 镜头布局自 layout_groups_parallel 提取此段——镜头组是预切片
/// （无 GroupMark 扫描），但缝合语义完全同型。
pub(super) fn stitch_group_rows(mut outs: Vec<(Vec<LayoutRow>, f64)>) -> Vec<LayoutRow> {
    // 组高前缀和 → 并行偏移各组局部 y → 顺序拼接（容量一次预留）。
    let mut bases: Vec<f64> = Vec::with_capacity(outs.len());
    let mut acc = 0.0f64;
    for (_, h) in &outs {
        bases.push(acc);
        acc += *h;
    }
    outs.par_iter_mut()
        .zip(bases.par_iter())
        .for_each(|((rows, _), &base)| {
            if base != 0.0 {
                for row in rows.iter_mut() {
                    match row {
                        LayoutRow::Separator { y, .. } | LayoutRow::Normal { y, .. } => *y += base,
                    }
                }
            }
        });
    let total_rows: usize = outs.iter().map(|(rows, _)| rows.len()).sum();
    let mut all_rows: Vec<LayoutRow> = Vec::with_capacity(total_rows);
    for (rows, _) in outs {
        all_rows.extend(rows);
    }
    all_rows
}

// ── Helpers ───────────────────────────────────────────────────────────────────
// ── 辅助函数 ───────────────────────────────────────────────────────────────────

// pub(crate) 仅为 horizontal.rs(H-Lab 实验布局)复用纯几何辅助,零行为变更。
pub(crate) fn aspect_ratio(item: &LayoutItem, placeholder_aspect: f64) -> f64 {
    // Not-yet-measured items (deferred dimensions) carry 0×0 → use the supplied
    // placeholder aspect rather than 1.0 (square).
    // 未测量项（延后取尺寸）为 0×0 → 使用传入的占位宽高比，而非 1.0（正方形）。
    if item.width <= 0 || item.height <= 0 {
        return placeholder_aspect.clamp(0.2, 5.0);
    }
    let w = item.width.max(1) as f64;
    let h = item.height.max(1) as f64;
    (w / h).clamp(0.2, 5.0) // clamp to prevent extreme ratios
                            // 限制以防止极端的比例
}

/// Median aspect ratio of the measured items in the set (those with real w/h),
/// used as the placeholder shape for 0×0 items. Falls back to 3:2 when nothing
/// is measured yet. O(n) via `select_nth_unstable` (no full sort).
/// 集合中已测量项（有真实宽高）的中位宽高比，作为 0×0 项的占位形状。尚无测量时
/// 回退到 3:2。借 `select_nth_unstable` 实现 O(n)（无需完整排序）。
pub(crate) fn median_measured_aspect<I: Borrow<LayoutItem>>(items: &[I]) -> f64 {
    let mut ars: Vec<f64> = items
        .iter()
        .map(Borrow::borrow)
        .filter(|it| it.width > 0 && it.height > 0)
        .map(|it| (it.width as f64 / it.height as f64).clamp(0.2, 5.0))
        .collect();
    if ars.is_empty() {
        return 1.5; // 3:2 landscape default | 默认 3:2 横向
    }
    let mid = ars.len() / 2;
    ars.select_nth_unstable_by(mid, |a, b| {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    });
    ars[mid]
}

pub(super) fn timestamp_to_date_label(ts: i64) -> String {
    let dt = Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now);

    // Format: "2024年3月15日"  (Chinese date, as specified in the plan)
    // 格式: "2024年3月15日" (中文日期，如计划所指定)
    // 调整：使用本地时区进行显示 — 为简单起见，此处使用 UTC。
    format!("{}年{}月{}日", dt.year(), dt.month(), dt.day())
}

/// 时间戳 → `"YYYY-MM"`（月零填充）。date 分组日分隔符的 `group_id`（T14 §3.8.2）。
///
/// **与 `timestamp_to_date_label` 同用 `Utc` 基准**——确保「月」边界与「日」分隔符严格对齐
///(否则同一张照片可能落在 UTC 的 3 月 1 日却被算进 2 月桶)。这也契合 T9 核验结论:
/// `sort_datetime` 的 EXIF 分量按「墙钟时间当 UTC 存」，故 UTC 格式化对绝大多数照片即正确。
pub(super) fn timestamp_to_year_month(ts: i64) -> String {
    let dt = Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now);
    format!("{}-{:02}", dt.year(), dt.month())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表征锁（方案 §11.1，P0 契约冻结）：新镜头字段全 None 时线上 JSON **不含**新键——
    /// 普通画廊 wire 不膨胀，前端零改动。任何出口路径意外填值/漏 skip 都在此显式失败。
    #[test]
    fn layout_row_item_wire_omits_lens_keys_when_none() {
        let item = placeholder_item(&SlimRowItem {
            id: 7,
            x: 1.0,
            w: 2.0,
            h: 3.0,
        });
        let json = serde_json::to_string(&item).unwrap();
        for key in [
            "duplicateBucket",
            "duplicateGroupOrdinal",
            "duplicateMemberOrdinal",
            "duplicateMemberCount",
        ] {
            assert!(!json.contains(key), "None 字段不得出现在线上 JSON: {key}");
        }
    }

    /// 表征锁：镜头字段填值时的键名（camelCase）与桶值（"duplicate"/"unconfirmed"/"unique"）
    /// 逐键冻结——前端 TS 的 `DuplicateBucket`/`LayoutRowItem` 联合类型以此为准。
    #[test]
    fn layout_row_item_wire_locks_lens_key_names_and_values() {
        let mut item = placeholder_item(&SlimRowItem {
            id: 7,
            x: 1.0,
            w: 2.0,
            h: 3.0,
        });
        item.duplicate_bucket = Some(DuplicateBucket::Duplicate);
        item.duplicate_group_ordinal = Some(2);
        item.duplicate_member_ordinal = Some(1);
        item.duplicate_member_count = Some(3);
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["duplicateBucket"], "duplicate");
        assert_eq!(v["duplicateGroupOrdinal"], 2);
        assert_eq!(v["duplicateMemberOrdinal"], 1);
        assert_eq!(v["duplicateMemberCount"], 3);

        // 三桶枚举 wire 值逐变体锁定。
        assert_eq!(
            serde_json::to_value(DuplicateBucket::Unconfirmed).unwrap(),
            "unconfirmed"
        );
        assert_eq!(
            serde_json::to_value(DuplicateBucket::Unique).unwrap(),
            "unique"
        );

        // round-trip：键名与值能原样解析回同一分类。
        let parsed: LayoutRowItem = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.duplicate_bucket, Some(DuplicateBucket::Duplicate));
        assert_eq!(parsed.duplicate_member_count, Some(3));
    }

    /// 表征锁（方案 §11.2）：Separator 新镜头字段全 None 时线上 JSON 不含新键
    /// （普通画廊分隔符 wire 形状不变）。
    #[test]
    fn separator_wire_omits_lens_keys_when_none() {
        let row = HydratedRow::Separator {
            y: 0.0,
            height: SEPARATOR_HEIGHT,
            separator_label: "2026年9月1日".into(),
            group_id: Some("2026-09".into()),
            separator_kind: None,
            parent_group_id: None,
            parent_group_start: None,
            duplicate_count: None,
            unconfirmed_count: None,
            unique_count: None,
            unique_hidden: None,
            duplicate_group_ordinal: None,
            duplicate_member_count: None,
            duplicate_folder_count: None,
            duplicate_unit_size: None,
            parent_group_ordinal: None,
            parent_group_folder_count: None,
            parent_group_group_count: None,
        };
        let json = serde_json::to_string(&row).unwrap();
        for key in [
            "separatorKind",
            "parentGroupId",
            "parentGroupStart",
            "duplicateCount",
            "unconfirmedCount",
            "uniqueCount",
            "uniqueHidden",
            "duplicateGroupOrdinal",
            "duplicateMemberCount",
            "duplicateFolderCount",
            "duplicateUnitSize",
            "parentGroupOrdinal",
            "parentGroupFolderCount",
            "parentGroupGroupCount",
        ] {
            assert!(!json.contains(key), "None 字段不得出现在线上 JSON: {key}");
        }
    }

    /// 表征锁：Separator 镜头字段填值时的键名、类别 wire 值（"date"/"folder"/
    /// "duplicateGroup"/"duplicateFolder"）与 round-trip 逐键冻结。
    #[test]
    fn separator_wire_locks_lens_key_names_and_kind_values() {
        let row = HydratedRow::Separator {
            y: 0.0,
            height: SEPARATOR_HEIGHT,
            separator_label: String::new(),
            group_id: Some("g1".into()),
            separator_kind: Some(GallerySeparatorKind::DuplicateGroup),
            parent_group_id: Some("c1".into()),
            parent_group_start: Some(true),
            duplicate_count: Some(3),
            unconfirmed_count: Some(1),
            unique_count: Some(2),
            unique_hidden: Some(false),
            duplicate_group_ordinal: Some(4),
            duplicate_member_count: Some(3),
            duplicate_folder_count: Some(2),
            duplicate_unit_size: Some(24 * 1024 * 1024),
            parent_group_ordinal: Some(2),
            parent_group_folder_count: Some(3),
            parent_group_group_count: Some(5),
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["rowType"], "separator", "rowType 标签不因镜头增量改变");
        assert_eq!(v["separatorKind"], "duplicateGroup");
        assert_eq!(v["parentGroupId"], "c1");
        assert_eq!(v["parentGroupStart"], true);
        assert_eq!(v["duplicateCount"], 3);
        assert_eq!(v["unconfirmedCount"], 1);
        assert_eq!(v["uniqueCount"], 2);
        assert_eq!(v["uniqueHidden"], false);
        assert_eq!(v["duplicateGroupOrdinal"], 4);
        assert_eq!(v["duplicateMemberCount"], 3);
        assert_eq!(v["duplicateFolderCount"], 2);
        assert_eq!(v["duplicateUnitSize"], 24 * 1024 * 1024);
        assert_eq!(v["parentGroupOrdinal"], 2);
        assert_eq!(v["parentGroupFolderCount"], 3);
        assert_eq!(v["parentGroupGroupCount"], 5);

        // 四类分隔符的 wire 变体名逐个锁定（camelCase：duplicateGroup/duplicateFolder）。
        assert_eq!(
            serde_json::to_value(GallerySeparatorKind::Date).unwrap(),
            "date"
        );
        assert_eq!(
            serde_json::to_value(GallerySeparatorKind::Folder).unwrap(),
            "folder"
        );
        assert_eq!(
            serde_json::to_value(GallerySeparatorKind::DuplicateFolder).unwrap(),
            "duplicateFolder"
        );

        // round-trip + 旧 JSON 兼容：新键缺省反序列化为 None。
        let parsed: HydratedRow = serde_json::from_value(v).unwrap();
        let HydratedRow::Separator {
            separator_kind,
            parent_group_id,
            unique_hidden,
            ..
        } = parsed
        else {
            panic!("应解析回 Separator");
        };
        assert_eq!(separator_kind, Some(GallerySeparatorKind::DuplicateGroup));
        assert_eq!(parent_group_id, Some("c1".into()));
        assert_eq!(unique_hidden, Some(false));

        let legacy: HydratedRow = serde_json::from_str(
            r#"{"rowType":"separator","y":0.0,"height":36.0,"separatorLabel":"d","groupId":"g"}"#,
        )
        .unwrap();
        let HydratedRow::Separator { separator_kind, .. } = legacy else {
            panic!("旧 JSON 应解析回 Separator");
        };
        assert_eq!(separator_kind, None, "旧 JSON 缺新键应落 default(None)");
    }
}
