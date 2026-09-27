// src-tauri/src/layout/justified.rs
//! 两端对齐布局算法（Rust 实现，§ 10.1）。
//!
//! Input: flat list of `LayoutItem` sorted by `sort_datetime DESC`.
//! 输入：按 `sort_datetime DESC` 排序的 `LayoutItem` 扁平列表。
//! 输出：`Vec<LayoutRow>` — 每一行是普通图像行或日期分隔符。
//!
//! 算法（Google Photos / Flickr 两端对齐布局）：
//!   - 对于每个项目，计算宽高比。
//!   - 将项目缩放到公共高度，将它们打包成一行。
//!   - 当行宽达到 `container_width ± tolerance` 时，提交该行。
//!   - 当 `sort_datetime` 跨越日期边界时，首先插入分隔符行。
//!
//! 共享骨架(输出类型/`LayoutParams`/并行组骨架/分组辅助/出口拼装/宽高比辅助)已迁至
//! `layout::geometry`;`compute_grid_layout` 已迁至 `layout::grid_pack`(tierB-4 简案)。

use std::borrow::Borrow;
use std::collections::HashMap;

use crate::db::models::{DirLabel, LayoutItem};

use super::geometry::{
    aspect_ratio, group_label, layout_groups_parallel, median_measured_aspect, LayoutParams,
    LayoutRow, SlimRowItem, SEPARATOR_HEIGHT,
};

// 既有外部调用路径(items_cache.rs/cache.rs/horizontal.rs/ipc/layout_commands.rs)已直接改指
// `layout::geometry`/`layout::grid_pack`,本文件不再兼容 re-export(避免「拆分又暗中重新打通」)。

/// 在我们将最后一行"两端对齐"与保持原样之前,允许其短多少。
const LAST_ROW_JUSTIFY_THRESHOLD: f64 = 0.6;
/// Maximum row height multiplier — prevents a single portrait image from
/// stretching to fill the entire container width (e.g. 1200 / 0.2 = 6000px).
/// 最大行高乘数 — 防止单张肖像图像拉伸以填充整个容器宽度（例如 1200 / 0.2 = 6000px）。
/// 如果计算的 row_h > target_h * MAX_ROW_HEIGHT_FACTOR，则将其限制在该系数。
const MAX_ROW_HEIGHT_FACTOR: f64 = 2.0;

// ── Main algorithm ────────────────────────────────────────────────────────────
// ── 主算法 ────────────────────────────────────────────────────────────

/// justified 行打包核心（原 pack_group 闭包体提取，2026-09-02 镜头布局复用）：对一段
/// 连续 items 做满行检测 + 逐行提交 + 末尾不满行（is_last 语义）。分隔符由调用方先行
/// 发射（普通路径 = date/folder 组头，镜头路径 = duplicateGroup 组头），`start_y` 为
/// 首行局部 y。返回 (行集, 局部终 y)。行内几何与提取前逐位一致（表征测试守护）。
pub(super) fn pack_justified_rows<I: Borrow<LayoutItem>>(
    items: &[I],
    start_y: f64,
    params: &LayoutParams,
    placeholder_aspect: f64,
) -> (Vec<LayoutRow>, f64) {
    let mut rows: Vec<LayoutRow> = Vec::new();
    let mut y = start_y;
    let mut pending: Vec<&LayoutItem> = Vec::new();
    let mut ar_sum = 0.0f64;
    // S3.5：宽度暂存单缓冲复用（跨行 clear）——真机行密度下数百万次堆分配的大头。
    let mut widths_scratch: Vec<f64> = Vec::new();

    let commit_row = |pending: &mut Vec<&LayoutItem>,
                      ar_sum: &mut f64,
                      y: &mut f64,
                      target_h: f64,
                      rows: &mut Vec<LayoutRow>,
                      params: &LayoutParams,
                      is_last: bool,
                      widths: &mut Vec<f64>| {
        if pending.is_empty() {
            return;
        }

        // 计算实际行高
        let total_gaps = params.gap * (pending.len().saturating_sub(1)) as f64;
        let available_w = params.container_width - total_gaps;

        // 确定该行是实际填满了宽度，还是不完整的最后一行。
        let is_incomplete =
            is_last && *ar_sum * target_h < available_w * LAST_ROW_JUSTIFY_THRESHOLD;
        let ideal_h = available_w / *ar_sum;

        let row_h = if is_incomplete {
            // 最后一行 — 不要拉伸；使用目标高度
            target_h
        } else {
            // 普通行：缩放以填充宽度，但上限为 MAX_ROW_HEIGHT_FACTOR
            ideal_h.min(target_h * MAX_ROW_HEIGHT_FACTOR)
        };

        let hit_cap = ideal_h > target_h * MAX_ROW_HEIGHT_FACTOR;
        let should_snap_last = !is_incomplete && !hit_cap;

        widths.clear();
        widths.extend(
            pending
                .iter()
                .map(|item| aspect_ratio(item, placeholder_aspect) * row_h),
        );

        // 仅在完全两端对齐的行时调整以精确填充容器
        if should_snap_last && pending.len() > 1 {
            let total_unrounded: f64 = widths.iter().sum();
            let target_total_w = available_w;
            // 按比例分配差异，以避免将舍入误差倾倒在最后一个项目上
            if total_unrounded > 0.0 {
                let scale = target_total_w / total_unrounded;
                for w in widths.iter_mut() {
                    *w *= scale;
                }
            }
        }

        // 就地舍入并分配剩余整数像素差异（S3.5：未舍入值此后不再使用，单缓冲复用）
        for w in widths.iter_mut() {
            *w = w.round();
        }

        if should_snap_last && pending.len() > 1 {
            let current_total: f64 = widths.iter().sum();
            let mut diff = (available_w.round() - current_total) as i32;

            // 在项目之间分配 1px 差异，直到 diff 为 0
            // 我们可以从最大到最小进行分配以最小化视觉影响，
            // 或者只是从左到右。从左到右也可以。
            let mut i = 0;
            let len = widths.len();
            while diff != 0 {
                if diff > 0 {
                    widths[i % len] += 1.0;
                    diff -= 1;
                } else {
                    widths[i % len] -= 1.0;
                    diff += 1;
                }
                i += 1;
            }
        }

        let mut x = 0.0f64;
        let mut row_items: Vec<SlimRowItem> = Vec::with_capacity(pending.len());

        for (i, item) in pending.iter().enumerate() {
            let item_w = widths[i];

            // x/w/h 语义不变（x 取整、w 保底 1、h 取整）；S3：行内仅存 id + 几何，零载荷克隆。
            row_items.push(SlimRowItem {
                id: item.id,
                x: x.round(),
                w: item_w.max(1.0),
                h: row_h.round(),
            });

            x += item_w + params.gap;
        }

        rows.push(LayoutRow::Normal {
            y: *y,
            height: row_h.ceil(),
            items: row_items,
        });

        *y += row_h.ceil() + params.gap;
        pending.clear();
        *ar_sum = 0.0;
    };

    for item in items {
        let item = item.borrow();
        pending.push(item);
        ar_sum += aspect_ratio(item, placeholder_aspect);

        // 检查行是否已满
        let total_gaps = params.gap * (pending.len().saturating_sub(1)) as f64;
        let available_w = params.container_width - total_gaps;
        if ar_sum * params.target_row_height >= available_w {
            commit_row(
                &mut pending,
                &mut ar_sum,
                &mut y,
                params.target_row_height,
                &mut rows,
                params,
                false,
                &mut widths_scratch,
            );
        }
    }
    // 提交组末的非满行
    if !pending.is_empty() {
        commit_row(
            &mut pending,
            &mut ar_sum,
            &mut y,
            params.target_row_height,
            &mut rows,
            params,
            true,
            &mut widths_scratch,
        );
    }
    (rows, y)
}

pub fn compute_justified_layout<I: Borrow<LayoutItem> + Sync>(
    items: &[I],
    params: &LayoutParams,
    dir_labels: &HashMap<i64, DirLabel>,
    placeholder_aspect: Option<f64>,
) -> Vec<LayoutRow> {
    // Placeholder aspect for not-yet-measured (0×0) items: the median of the
    // measured items, so deferred-dimension photos render at a plausible shape
    // (not a square) and reflow only slightly when their real dims arrive.
    // 未测量(0×0)项的占位宽高比：取已测量项的中位数，使延后取尺寸的照片以合理形状
    //(而非正方形)渲染,真实尺寸到达时只发生轻微重排。
    // S3.5：中位数只依赖项集、不依赖布局参数——调用方传快照缓存值（OnceLock）免每次
    // 重排 O(N) 重算；None = 现算（测试/独立调用）。
    let placeholder_aspect = placeholder_aspect.unwrap_or_else(|| median_measured_aspect(items));

    let emit_separator = |label: &str,
                          group_id: Option<String>,
                          epoch_day: Option<i64>,
                          y: &mut f64,
                          rows: &mut Vec<LayoutRow>| {
        rows.push(LayoutRow::Separator {
            y: *y,
            height: SEPARATOR_HEIGHT,
            separator_label: label.to_string(),
            group_id,
            epoch_day,
            // date/folder 分隔符不填镜头类别：出口透传 None，普通画廊线上 JSON
            // 不含 separatorKind 键（wire 位级不变，P1 最小增量）。
            separator_kind: None,
            lens_folder: None,
            lens_group: None,
        });
        *y += SEPARATOR_HEIGHT + params.gap;
    };

    // S3.4 单组打包（组内 y 从局部 0 起算）：组首发分隔符（none 分组无），组内行打包
    // 委托 pack_justified_rows（与 2026-09-02 镜头布局共用同一核心，几何逐位一致）。
    let pack_group = |group: &[I]| -> (Vec<LayoutRow>, f64) {
        let mut rows: Vec<LayoutRow> = Vec::new();
        let mut y = 0.0f64;
        if params.group_by != "none" && !params.seamless {
            let (label, group_id, epoch_day) =
                group_label(group[0].borrow(), &params.group_by, dir_labels);
            emit_separator(&label, group_id, epoch_day, &mut y, &mut rows);
        }
        let (body, y_end) = pack_justified_rows(group, y, params, placeholder_aspect);
        rows.extend(body);
        (rows, y_end)
    };

    // 无缝模式打包按 none 语义(单段、行跨组连续);排序聚合仍由取数侧按真实 group_by 保证。
    let packing_group_by = if params.seamless {
        "none"
    } else {
        params.group_by.as_str()
    };
    layout_groups_parallel(items, packing_group_by, pack_group)
}
