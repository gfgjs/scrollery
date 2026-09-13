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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::geometry::{hydrate_item, timestamp_to_date_label, timestamp_to_year_month};
    use crate::layout::grid_pack::compute_grid_layout;
    use chrono::{TimeZone, Utc};

    /// `timestamp_to_year_month`：UTC 基准 + 月零填充（date 分组 group_id 的事实源，T14 §3.8.2）。
    #[test]
    fn year_month_is_utc_and_zero_padded() {
        // epoch → 1970-01（验证个位月零填充）。
        assert_eq!(timestamp_to_year_month(0), "1970-01");
        // 已知 UTC 时刻：用 chrono 构造，避免硬编码脆弱的魔数。
        let mar = Utc
            .with_ymd_and_hms(2024, 3, 5, 10, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(timestamp_to_year_month(mar), "2024-03");
        let dec = Utc
            .with_ymd_and_hms(2023, 12, 31, 23, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(timestamp_to_year_month(dec), "2023-12", "两位月不补零");
        // 与同基准的日标签同月——锁住「月桶边界 = 日分隔符边界」的对齐前提。
        assert!(timestamp_to_date_label(mar).starts_with("2024年3月"));
    }

    /// 最小 LayoutItem fixture：仅关心 id / 宽高 / 时间戳，其余取无害默认。
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

    /// 取出 Normal 行的 (y, height, items)，分隔符行返回 None——测试断言用。
    fn as_normal(row: &LayoutRow) -> Option<(f64, f64, &Vec<SlimRowItem>)> {
        match row {
            LayoutRow::Normal { y, height, items } => Some((*y, *height, items)),
            LayoutRow::Separator { .. } => None,
        }
    }

    fn grid_params(container_width: f64, target: f64, gap: f64, group_by: &str) -> LayoutParams {
        LayoutParams {
            container_width,
            target_row_height: target,
            gap,
            group_by: group_by.to_string(),
            sort_within_group: "datetime".to_string(),
            seamless: false,
        }
    }

    /// grid（none 分组）：固定列数、方格单元、撑满宽、x 按列均布、y 逐行推进。
    #[test]
    fn grid_none_packs_uniform_square_rows() {
        // 容器 300 / 目标 100 / gap 0 → cols=3, cell=100。7 项 → 行 [3,3,1]，无分隔符。
        let items: Vec<LayoutItem> = (1..=7).map(|i| mk_item(i, 160, 90, i)).collect();
        let rows = compute_grid_layout(
            &items,
            &grid_params(300.0, 100.0, 0.0, "none"),
            &HashMap::new(),
        );

        // 全是 Normal 行（none 分组无分隔符）。
        assert!(
            rows.iter().all(|r| as_normal(r).is_some()),
            "none 分组不应有分隔符"
        );
        assert_eq!(rows.len(), 3, "7 项 / 3 列 = 3 行");

        let (y0, h0, r0) = as_normal(&rows[0]).unwrap();
        assert_eq!(r0.len(), 3);
        assert_eq!((y0, h0), (0.0, 100.0));
        // 方格：w == h == cell(100)；x 按列 0/100/200。
        for it in r0 {
            assert_eq!((it.w, it.h), (100.0, 100.0), "单元应为方格");
        }
        assert_eq!((r0[0].x, r0[1].x, r0[2].x), (0.0, 100.0, 200.0));

        // 第二行 y 推进到 100；末行仅 1 项、x=0。
        let (y1, _, r1) = as_normal(&rows[1]).unwrap();
        assert_eq!((y1, r1.len()), (100.0, 3));
        let (y2, _, r2) = as_normal(&rows[2]).unwrap();
        assert_eq!((y2, r2.len()), (200.0, 1));
        assert_eq!(r2[0].x, 0.0);
    }

    /// grid 单元撑满容器宽：cols·cell + (cols-1)·gap ≈ 容器宽（消除右侧空隙）。
    #[test]
    fn grid_cell_fills_width_with_gap() {
        // 容器 320 / 目标 100 / gap 10 → cols=⌊330/110⌋=3, cell=(320-20)/3=100。
        let items: Vec<LayoutItem> = (1..=3).map(|i| mk_item(i, 100, 100, i)).collect();
        let rows = compute_grid_layout(
            &items,
            &grid_params(320.0, 100.0, 10.0, "none"),
            &HashMap::new(),
        );
        let (_, _, r0) = as_normal(&rows[0]).unwrap();
        assert_eq!(r0.len(), 3);
        // x: 0 / (100+10)=110 / 220；末单元右缘 = 220+100 = 320 = 容器宽。
        assert_eq!((r0[0].x, r0[1].x, r0[2].x), (0.0, 110.0, 220.0));
        assert_eq!(r0[2].x + r0[2].w, 320.0, "末单元右缘应贴容器右沿");
    }

    /// grid（date 分组）：跨天插分隔符、组间不共享行，group_id 为 "YYYY-MM"（与 justified 同源）。
    #[test]
    fn grid_inserts_separators_between_days() {
        // 2 项 1970-01-01 + 2 项约 2 天后；cols≥3 故每组 1 个不满行。
        let day2 = 2 * 86_400;
        let items = vec![
            mk_item(1, 100, 100, 10),
            mk_item(2, 100, 100, 20),
            mk_item(3, 100, 100, day2 + 10),
            mk_item(4, 100, 100, day2 + 20),
        ];
        let rows = compute_grid_layout(
            &items,
            &grid_params(400.0, 100.0, 0.0, "date"),
            &HashMap::new(),
        );

        // 期望序列：Sep, Normal(2), Sep, Normal(2)。
        let sep_count = rows
            .iter()
            .filter(|r| matches!(r, LayoutRow::Separator { .. }))
            .count();
        assert_eq!(sep_count, 2, "两天 → 两个日分隔符");
        // 首行是分隔符且带 YYYY-MM group_id。
        match &rows[0] {
            LayoutRow::Separator { group_id, .. } => {
                assert_eq!(group_id.as_deref(), Some("1970-01"))
            }
            _ => panic!("首行应为分隔符"),
        }
        // 两个 Normal 行各 2 项（组间不共享行）。
        let normal_lens: Vec<usize> = rows
            .iter()
            .filter_map(as_normal)
            .map(|(_, _, its)| its.len())
            .collect();
        assert_eq!(normal_lens, vec![2, 2]);
    }

    /// 无缝分组(#1,grid):date 分组 + seamless —— 无分隔符、行跨组连续、保输入序,
    /// 且与 group_by="none" 的打包位级一致(排序聚合由取数侧负责,本层只管打包)。
    #[test]
    fn seamless_grid_packs_like_none_keeps_input_order() {
        let day2 = 2 * 86_400;
        let items = vec![
            mk_item(1, 100, 100, 10),
            mk_item(2, 100, 100, 20),
            mk_item(3, 100, 100, day2 + 10),
            mk_item(4, 100, 100, day2 + 20),
        ];
        let mut params = grid_params(300.0, 100.0, 0.0, "date");
        params.seamless = true;
        let rows = compute_grid_layout(&items, &params, &HashMap::new());

        // 无分隔符;4 项 / 3 列 = 2 行,第 3 项(次日)与前两项同行 = 行跨组。
        assert!(
            rows.iter().all(|r| as_normal(r).is_some()),
            "seamless 不应有分隔符"
        );
        assert_eq!(rows.len(), 2);
        let (_, _, r0) = as_normal(&rows[0]).unwrap();
        assert_eq!(
            r0.iter().map(|it| it.id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "行应跨组连续且保持输入序"
        );

        // 与 none 打包位级一致。
        let rows_none = compute_grid_layout(
            &items,
            &grid_params(300.0, 100.0, 0.0, "none"),
            &HashMap::new(),
        );
        assert_eq!(rows.len(), rows_none.len());
        for (a, b) in rows.iter().zip(rows_none.iter()) {
            let (ya, ha, ra) = as_normal(a).unwrap();
            let (yb, hb, rb) = as_normal(b).unwrap();
            assert_eq!((ya, ha), (yb, hb));
            assert_eq!(
                ra.iter().map(|i| (i.id, i.x, i.w)).collect::<Vec<_>>(),
                rb.iter().map(|i| (i.id, i.x, i.w)).collect::<Vec<_>>()
            );
        }
    }

    /// 无缝分组(#1,justified):同 grid——无分隔符、与 none 打包位级一致。
    #[test]
    fn seamless_justified_matches_none_packing() {
        let day2 = 2 * 86_400;
        let items = vec![
            mk_item(1, 160, 90, 10),
            mk_item(2, 90, 160, 20),
            mk_item(3, 100, 100, day2 + 10),
            mk_item(4, 400, 100, day2 + 20),
        ];
        let mut p_seam = grid_params(500.0, 120.0, 8.0, "date");
        p_seam.seamless = true;
        let rows = compute_justified_layout(&items, &p_seam, &HashMap::new(), None);
        assert!(
            rows.iter().all(|r| as_normal(r).is_some()),
            "seamless 不应有分隔符"
        );

        let rows_none = compute_justified_layout(
            &items,
            &grid_params(500.0, 120.0, 8.0, "none"),
            &HashMap::new(),
            None,
        );
        assert_eq!(rows.len(), rows_none.len());
        for (a, b) in rows.iter().zip(rows_none.iter()) {
            let (ya, ha, ra) = as_normal(a).unwrap();
            let (yb, hb, rb) = as_normal(b).unwrap();
            assert_eq!((ya, ha), (yb, hb));
            assert_eq!(
                ra.iter().map(|i| (i.id, i.x, i.w)).collect::<Vec<_>>(),
                rb.iter().map(|i| (i.id, i.x, i.w)).collect::<Vec<_>>()
            );
        }
    }

    /// P3 epoch_day 端到端接线（评审 R6）：此前唯一相关测试用手搓 LayoutRow 走缓存 round-trip，
    /// 从 compute_*_layout 出发的产生链（group_label → emit_separator）零锁定——若回归误传
    /// None，前端 time 坐标只会静默退化（hasTime=false 按钮消失）而无任何报错。
    /// 含负时间戳（<1970）：div_euclid 向负无穷取整与 chrono UTC 日界一致（截断除法会 off-by-one），
    /// 及 folder 分组恒 None。
    #[test]
    fn layouts_emit_epoch_day_from_sort_datetime() {
        // DESC 序：1970-01-02（ts=86_400+10 → epoch_day 1）在前，1969-12-31 23:59:59
        //(ts=-1 → epoch_day -1,若用 `/` 截断则错为 0)在后。
        let items = vec![mk_item(1, 100, 100, 86_400 + 10), mk_item(2, 100, 100, -1)];
        let expect_days = vec![Some(1), Some(-1)];

        let sep_days = |rows: &[LayoutRow]| -> Vec<Option<i64>> {
            rows.iter()
                .filter_map(|r| match r {
                    LayoutRow::Separator { epoch_day, .. } => Some(*epoch_day),
                    _ => None,
                })
                .collect()
        };

        let grid = compute_grid_layout(
            &items,
            &grid_params(400.0, 100.0, 0.0, "date"),
            &HashMap::new(),
        );
        assert_eq!(
            sep_days(&grid),
            expect_days,
            "grid：epoch_day = sort_datetime.div_euclid(86400)，负日界不 off-by-one"
        );

        let just = compute_justified_layout(
            &items,
            &grid_params(400.0, 100.0, 0.0, "date"),
            &HashMap::new(),
            None,
        );
        assert_eq!(sep_days(&just), expect_days, "justified 与 grid 同源");

        // folder 分组：无「日」概念，epoch_day 恒 None。
        let mut f1 = mk_item(1, 100, 100, 86_400 + 10);
        f1.dir_id = Some(7);
        let mut f2 = mk_item(2, 100, 100, -1);
        f2.dir_id = Some(7);
        let folder = compute_grid_layout(
            &[f1, f2],
            &grid_params(400.0, 100.0, 0.0, "folder"),
            &HashMap::new(),
        );
        assert_eq!(
            sep_days(&folder),
            vec![None],
            "folder 分组 epoch_day 恒 None"
        );
    }

    /// S3.4 并行缝合特征化：多组绝对 y 与顺序累加位级一致（组高前缀和 + 组内局部 y）。
    #[test]
    fn parallel_stitch_yields_sequential_y_positions() {
        let day2 = 2 * 86_400;
        // grid/date/gap0:Sep(0,36)+Normal(36,100)+Sep(136,36)+Normal(172,100)。
        let items = vec![
            mk_item(1, 100, 100, 10),
            mk_item(2, 100, 100, 20),
            mk_item(3, 100, 100, day2 + 10),
            mk_item(4, 100, 100, day2 + 20),
        ];
        let rows = compute_grid_layout(
            &items,
            &grid_params(400.0, 100.0, 0.0, "date"),
            &HashMap::new(),
        );
        let ys: Vec<f64> = rows.iter().map(|r| r.y()).collect();
        assert_eq!(ys, vec![0.0, 36.0, 136.0, 172.0]);

        // justified/date/gap4(单张 ar2 不满行 → 行高=目标 100):
        // Sep(0)→y40 + Row(40,100)→y144 + Sep(144)→y184 + Row(184)。
        let items = vec![mk_item(1, 200, 100, 10), mk_item(2, 200, 100, day2 + 10)];
        let rows = compute_justified_layout(
            &items,
            &grid_params(400.0, 100.0, 4.0, "date"),
            &HashMap::new(),
            None,
        );
        let ys: Vec<f64> = rows.iter().map(|r| r.y()).collect();
        assert_eq!(ys, vec![0.0, 40.0, 144.0, 184.0]);
    }

    /// justified 几何特征化：锁住 make_row_item / group_key 提取后的输出不变（该路径此前无单测）。
    /// 单张 ar=2.0 图、容器 400 / 目标 100 / none：不满末行不拉伸 → 行高=目标，宽=ar·行高。
    #[test]
    fn justified_geometry_unchanged_characterization() {
        let items = vec![mk_item(1, 200, 100, 5)]; // ar = 2.0
        let rows = compute_justified_layout(
            &items,
            &grid_params(400.0, 100.0, 0.0, "none"),
            &HashMap::new(),
            None,
        );
        assert_eq!(rows.len(), 1);
        let (y, h, its) = as_normal(&rows[0]).unwrap();
        assert_eq!((y, h), (0.0, 100.0));
        assert_eq!(its.len(), 1);
        // is_incomplete（200 < 400·0.6=240）→ row_h=target=100；w=ar·row_h=200；x=0。
        assert_eq!((its[0].x, its[0].w, its[0].h), (0.0, 200.0, 100.0));
        // 字段透传职责已随 S3 移到出口拼装：hydrate_item 保 slot 几何 + item 载荷逐字段还原
        //（original 尺寸 = 原始宽高、rating/color_label 默认 0）——继续锁同一契约。
        let wire = hydrate_item(&items[0], &its[0], None);
        assert_eq!((wire.id, wire.x, wire.w, wire.h), (1, 0.0, 200.0, 100.0));
        assert_eq!((wire.original_width, wire.original_height), (200, 100));
        assert_eq!((wire.rating, wire.color_label), (0, 0));
        assert_eq!(wire.file_format, "jpg");
    }

    /// S3.5 拆帐基准(非门控,--release + --ignored 手动跑):1M 合成项,分相计时定位
    /// layout 段的 150ms 地板。用法:
    /// cargo test --release --lib layout::justified::tests::bench_layout_1m -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_layout_1m() {
        use std::time::Instant;
        const N: i64 = 1_000_000;
        // 1000 目录 × 连续块(与真实序一致:folder 轴按目录聚簇);时间戳跨 ~200 天;
        // 4 种宽高比轮转 + 5% 未测量(0×0)项走占位比。
        let items: Vec<LayoutItem> = (0..N)
            .map(|i| {
                let (w, h) = match i % 20 {
                    0 => (0, 0),
                    x if x % 4 == 1 => (1600, 1200),
                    x if x % 4 == 2 => (1200, 1600),
                    x if x % 4 == 3 => (1920, 1080),
                    _ => (1500, 1000),
                };
                let mut it = mk_item(i + 1, w, h, (N - i) * 17);
                it.dir_id = Some(i / 1000);
                it
            })
            .collect();
        let dir_labels: HashMap<i64, DirLabel> = (0..1000)
            .map(|d| {
                (
                    d,
                    DirLabel {
                        rel_path: format!("dir/{d:04}"),
                        display: format!("D:/photos/dir/{d:04}"),
                        name: format!("{d:04}"),
                        root_created_at: 0,
                        root_id: 0,
                    },
                )
            })
            .collect();

        let t = Instant::now();
        let med = median_measured_aspect(&items);
        println!(
            "median_measured_aspect: {:.1}ms (med={med:.3})",
            t.elapsed().as_secs_f64() * 1e3
        );

        // 复现真机行密度(1.2-2.4 项/行 → 41-83 万行):target 拉大。
        for (axis, target) in [
            ("folder", 100.0),
            ("folder", 240.0),
            ("folder", 400.0),
            ("folder", 700.0),
            ("date", 100.0),
            ("none", 100.0),
        ] {
            let p = grid_params(1200.0, target, 4.0, axis);
            // 预热一次(rayon 池/页错误),再取三次最小值。
            let _ = compute_justified_layout(&items, &p, &dir_labels, Some(med));
            let mut best = f64::MAX;
            let mut rows_n = 0usize;
            for _ in 0..3 {
                let t = Instant::now();
                let rows = compute_justified_layout(&items, &p, &dir_labels, Some(med));
                best = best.min(t.elapsed().as_secs_f64() * 1e3);
                rows_n = rows.len();
                drop(rows);
            }
            println!("justified axis={axis} target={target}: {best:.1}ms ({rows_n} rows)");
        }
        for (axis, target) in [("folder", 100.0), ("date", 100.0)] {
            let p = grid_params(1200.0, target, 4.0, axis);
            let _ = compute_grid_layout(&items, &p, &dir_labels);
            let mut best = f64::MAX;
            let mut rows_n = 0usize;
            for _ in 0..3 {
                let t = Instant::now();
                let rows = compute_grid_layout(&items, &p, &dir_labels);
                best = best.min(t.elapsed().as_secs_f64() * 1e3);
                rows_n = rows.len();
                drop(rows);
            }
            println!("grid axis={axis} target={target}: {best:.1}ms ({rows_n} rows)");
        }
    }
}
