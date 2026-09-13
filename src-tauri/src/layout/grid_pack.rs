// src-tauri/src/layout/grid_pack.rs
//! 均匀宫格布局算法(T20,方案 a「后端 uniform-packing」)。拆分自 justified.rs
//!(tierB-4 简案)——与 justified 共享分组/分隔符/月桶骨架(见 `layout::geometry`),
//! 仅"行内打包"不同。

use std::borrow::Borrow;
use std::collections::HashMap;

use crate::db::models::{DirLabel, LayoutItem};

use super::geometry::{
    group_label, layout_groups_parallel, LayoutParams, LayoutRow, SlimRowItem, SEPARATOR_HEIGHT,
};

/// 均匀宫格布局（T20，方案 a「后端 uniform-packing」）。与 justified 共享分组/分隔符/月桶逻辑
///(group_key + SEPARATOR_HEIGHT),仅"行内打包"不同:固定列数、方格单元、等高行;单元撑满
/// 容器宽（消除右侧空隙），方图由前端 `object-fit: cover` 裁切。产出同一 `LayoutRow` 枚举，故
/// 时间轴 / 虚拟滚动 / 分隔符联动全部原样工作。
///
/// 列数 = `⌊(W+gap) / (cell_target+gap)⌋`（至少 1）；实际单元边长 = `(W - (cols-1)·gap)/cols`
/// 撑满宽。`target_row_height` 复用作单元目标边长（即工具栏 gridRowHeight 密度滑块）。
/// 列数与单元边长（T20 撑满宽公式）：列数 = `⌊(W+gap)/(cell_target+gap)⌋`（至少 1），
/// 实际单元边长 = `(W - (cols-1)·gap)/cols`。compute_grid_layout 与镜头 grid 布局
/// （`layout::lens::compute_lens_layout_grid`）共用，保证两种入口的行几何一致。
pub(super) fn grid_metrics(container_width: f64, target_row_height: f64, gap: f64) -> (usize, f64) {
    let gap = gap.max(0.0);
    let target = target_row_height.max(1.0);
    let width = container_width.max(1.0);
    let cols = (((width + gap) / (target + gap)).floor() as usize).max(1);
    let cell = ((width - gap * (cols as f64 - 1.0)) / cols as f64).max(1.0);
    (cols, cell)
}

/// grid 行打包核心（原 pack_group 闭包体提取，2026-09-02 镜头布局复用）：固定列数、
/// 方格单元、x 按列均布、满行即提交 + 末尾不满行。分隔符由调用方先行发射（普通路径 =
/// date/folder 组头，镜头路径 = duplicateGroup 组头），`start_y` 为首行局部 y。
/// 返回 (行集, 局部终 y)。行内几何与提取前逐位一致（表征测试守护）。
pub(super) fn pack_grid_rows<I: Borrow<LayoutItem>>(
    items: &[I],
    start_y: f64,
    cols: usize,
    cell: f64,
    gap: f64,
) -> (Vec<LayoutRow>, f64) {
    let mut rows: Vec<LayoutRow> = Vec::new();
    let mut y = start_y;
    let mut pending: Vec<&LayoutItem> = Vec::new();

    // 提交一整（或末尾不满）行：方格、x 按列均布。
    let commit_grid_row =
        |pending: &mut Vec<&LayoutItem>, y: &mut f64, rows: &mut Vec<LayoutRow>| {
            if pending.is_empty() {
                return;
            }
            let mut row_items: Vec<SlimRowItem> = Vec::with_capacity(pending.len());
            for (col, item) in pending.iter().enumerate() {
                let x = (col as f64) * (cell + gap);
                // 方格：w = h = cell；x/w/h 取整与 justified 同款，避免亚像素缝。S3：仅 id + 几何。
                row_items.push(SlimRowItem {
                    id: item.id,
                    x: x.round(),
                    w: cell.round(),
                    h: cell.round(),
                });
            }
            rows.push(LayoutRow::Normal {
                y: *y,
                height: cell.ceil(),
                items: row_items,
            });
            *y += cell.ceil() + gap;
            pending.clear();
        };

    for item in items {
        pending.push(item.borrow());
        if pending.len() >= cols {
            commit_grid_row(&mut pending, &mut y, &mut rows);
        }
    }
    // 末尾不满行
    if !pending.is_empty() {
        commit_grid_row(&mut pending, &mut y, &mut rows);
    }
    (rows, y)
}

pub fn compute_grid_layout<I: Borrow<LayoutItem> + Sync>(
    items: &[I],
    params: &LayoutParams,
    dir_labels: &HashMap<i64, DirLabel>,
) -> Vec<LayoutRow> {
    // 列数：容器宽内能放下几个 (target+gap)，至少 1 列；随后把单元放大到精确撑满宽。
    // 提取为 grid_metrics 供镜头 grid 布局共用（同一公式，几何逐位一致）。
    let (cols, cell) = grid_metrics(params.container_width, params.target_row_height, params.gap);
    let gap = params.gap.max(0.0);

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
        *y += SEPARATOR_HEIGHT + gap;
    };

    // S3.4 单组打包（同 justified 的组间并行骨架）：组间不共享行、组末冲掉不满行,
    // 与顺序版逐行相同；行打包委托 pack_grid_rows（与镜头布局共用同一核心）。
    let pack_group = |group: &[I]| -> (Vec<LayoutRow>, f64) {
        let mut rows: Vec<LayoutRow> = Vec::new();
        let mut y = 0.0f64;
        if params.group_by != "none" && !params.seamless {
            let (label, group_id, epoch_day) =
                group_label(group[0].borrow(), &params.group_by, dir_labels);
            emit_separator(&label, group_id, epoch_day, &mut y, &mut rows);
        }
        let (body, y_end) = pack_grid_rows(group, y, cols, cell, gap);
        rows.extend(body);
        (rows, y_end)
    };

    // 无缝模式同 justified:打包按 none 语义,排序聚合由取数侧保证。
    let packing_group_by = if params.seamless {
        "none"
    } else {
        params.group_by.as_str()
    };
    layout_groups_parallel(items, packing_group_by, pack_group)
}
