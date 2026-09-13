// src-tauri/src/layout/horizontal.rs
//! 横向画廊实验室(H-Lab)布局算法族(docs/designs/2026-07-02-horizontal-gallery-lab.md §3)。
//!
//! 三种候选模式统一为纯函数 `(items, params) → Vec<HBlock>`,块沿 x 主轴单调排布:
//!   - `paged`:分屏 justified——页宽 = 视口宽 × factor,页内等高行装配,页高恰纳视口;
//!   - `lanes`:等高泳道——k 条固定等高泳道,列主序(item i → 泳道 i mod k)或 balance 指派;
//!   - `columns`:转置 justified——列宽浮动、列内同宽异高、列高恰满视口。
//!
//! 与生产 justified.rs 的关系:刻意**不**抽象合并(实验期不动生产代码优先于 DRY),仅复用
//! 其纯几何辅助 `aspect_ratio` / `median_measured_aspect`;某模式毕业转正时再统一。
//! `HItem` 坐标一律**全局绝对坐标**——渲染层不感知模式差异,块仅是取数分组 + 虚拟化单元。

use serde::{Deserialize, Serialize};

use crate::db::models::LayoutItem;
use crate::layout::geometry::{aspect_ratio, median_measured_aspect};

// ── 契约类型(跨 IPC,camelCase 序列化)─────────────────────────────────────────

/// 实验布局模式。internally-tagged:前端传 `{ mode: 'paged', pageFactor: 1.2, ... }`。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode")]
pub enum HLayoutMode {
    /// A:分屏 justified。`page_factor` 刻意 >1 露出残页作滚动线索(用户原案 1.2)。
    #[serde(rename = "paged", rename_all = "camelCase")]
    Paged {
        page_factor: f64,
        target_row_height: f64,
    },
    /// B:等高泳道,列主序填充。`balance=true` 时改为「放最落后泳道」抑制游标漂移
    /// (代价:视觉列内顺序轻微乱序),两种指派并列供真人调研对比。
    #[serde(rename = "lanes", rename_all = "camelCase")]
    Lanes { lane_count: usize, balance: bool },
    /// C:转置 justified(列内同宽异高,列高恰满视口)。
    #[serde(rename = "columns", rename_all = "camelCase")]
    Columns { target_col_width: f64 },
}

pub struct HLayoutParams {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub gap: f64,
    pub mode: HLayoutMode,
}

/// 实验项:仅缩略图渲染所需字段(无收藏/评分/可用态——附加能力显式推迟,plan §6)。
/// x/y/w/h 为全局绝对坐标。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HItem {
    pub id: i64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub media_type: String,
    pub file_format: String,
    pub file_size: i64,
    pub is_live_photo: bool,
    pub duration_ms: Option<i64>,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
}

/// 虚拟化单元:bbox 覆盖其全部子项(lanes 模式相邻块 bbox 可轻微重叠,取块按 bbox 相交)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HBlock {
    pub x: f64,
    pub width: f64,
    pub items: Vec<HItem>,
}

// ── 公共常量(对偶生产 justified.rs)──────────────────────────────────────────

/// 末行/末列「不满则不拉伸」阈值(对偶生产 LAST_ROW_JUSTIFY_THRESHOLD)。
const LAST_UNIT_JUSTIFY_THRESHOLD: f64 = 0.6;
/// 行高/列宽相对目标值的上限倍数(对偶生产 MAX_ROW_HEIGHT_FACTOR,防御性钳制)。
const MAX_UNIT_FACTOR: f64 = 2.0;

// ── 入口 ──────────────────────────────────────────────────────────────────────

pub fn compute_horizontal_layout(items: &[LayoutItem], params: &HLayoutParams) -> Vec<HBlock> {
    if items.is_empty() {
        return vec![];
    }
    let placeholder = median_measured_aspect(items);
    let vw = params.viewport_width.max(200.0);
    let vh = params.viewport_height.max(200.0);
    let gap = params.gap.max(0.0);

    match params.mode {
        HLayoutMode::Paged {
            page_factor,
            target_row_height,
        } => layout_paged(
            items,
            vw,
            vh,
            gap,
            page_factor,
            target_row_height,
            placeholder,
        ),
        HLayoutMode::Lanes {
            lane_count,
            balance,
        } => layout_lanes(items, vh, gap, lane_count, balance, placeholder),
        HLayoutMode::Columns { target_col_width } => {
            layout_columns(items, vh, gap, target_col_width, placeholder)
        }
    }
}

fn make_h_item(item: &LayoutItem, x: f64, y: f64, w: f64, h: f64) -> HItem {
    HItem {
        id: item.id,
        x,
        y,
        w,
        h,
        media_type: item.media_type.clone(),
        file_format: item.file_format.clone(),
        file_size: item.file_size,
        is_live_photo: item.is_live_photo,
        duration_ms: item.duration_ms,
        thumb_status: item.thumb_status,
        thumb_path: item.thumb_path.clone(),
        thumbhash: item.thumbhash.clone(),
    }
}

// ── C:转置 justified(columns)────────────────────────────────────────────────

fn layout_columns(
    items: &[LayoutItem],
    vh: f64,
    gap: f64,
    target_col_width: f64,
    placeholder: f64,
) -> Vec<HBlock> {
    let target_w = target_col_width.clamp(60.0, 2000.0);

    let mut blocks: Vec<HBlock> = Vec::new();
    let mut cur_x = 0.0f64;
    let mut pending: Vec<&LayoutItem> = Vec::new();
    let mut inv_ar_sum = 0.0f64; // Σ(h/w)——转置后的 packing ratio 累加

    // 提交一列。几何与生产 commit_row 严格对偶(宽↔高互换):
    // 列宽 = 可用高 / Σ(1/ar);完整列先比例缩放各项高、再取整并逐项 ±1 摊派像素差,
    // 保证列底恰贴视口底;不满末列不拉伸(用目标宽,顶对齐)。
    #[allow(clippy::too_many_arguments)]
    fn commit_column(
        pending: &mut Vec<&LayoutItem>,
        inv_sum: &mut f64,
        cur_x: &mut f64,
        blocks: &mut Vec<HBlock>,
        vh: f64,
        gap: f64,
        target_w: f64,
        placeholder: f64,
        is_last: bool,
    ) {
        if pending.is_empty() {
            return;
        }
        let gaps = gap * (pending.len().saturating_sub(1)) as f64;
        let avail_h = vh - gaps;

        let is_incomplete = is_last && *inv_sum * target_w < avail_h * LAST_UNIT_JUSTIFY_THRESHOLD;
        let ideal_w = avail_h / *inv_sum;
        let col_w = if is_incomplete {
            target_w
        } else {
            ideal_w.min(target_w * MAX_UNIT_FACTOR)
        };
        let hit_cap = ideal_w > target_w * MAX_UNIT_FACTOR;
        let snap = !is_incomplete && !hit_cap;

        let mut heights: Vec<f64> = pending
            .iter()
            .map(|it| col_w / aspect_ratio(it, placeholder))
            .collect();

        if snap && pending.len() > 1 {
            let total: f64 = heights.iter().sum();
            if total > 0.0 {
                let scale = avail_h / total;
                for hh in heights.iter_mut() {
                    *hh *= scale;
                }
            }
        }

        let mut final_h: Vec<f64> = heights.iter().map(|v| v.round()).collect();
        if snap && pending.len() > 1 {
            let current_total: f64 = final_h.iter().sum();
            let mut diff = (avail_h.round() - current_total) as i32;
            let len = final_h.len();
            let mut i = 0;
            while diff != 0 {
                if diff > 0 {
                    final_h[i % len] += 1.0;
                    diff -= 1;
                } else {
                    final_h[i % len] -= 1.0;
                    diff += 1;
                }
                i += 1;
            }
        }

        let cw = col_w.round().max(1.0);
        let mut y = 0.0f64;
        let mut col_items: Vec<HItem> = Vec::with_capacity(pending.len());
        for (i, item) in pending.iter().enumerate() {
            col_items.push(make_h_item(
                item,
                cur_x.round(),
                y.round(),
                cw,
                final_h[i].max(1.0),
            ));
            y += final_h[i] + gap;
        }

        blocks.push(HBlock {
            x: cur_x.round(),
            width: cw,
            items: col_items,
        });
        *cur_x += cw + gap;
        pending.clear();
        *inv_sum = 0.0;
    }

    for item in items {
        let inv = 1.0 / aspect_ratio(item, placeholder);
        pending.push(item);
        inv_ar_sum += inv;

        let gaps = gap * (pending.len().saturating_sub(1)) as f64;
        // 判满:目标列宽下累计高度触达可用高即提交(对偶生产的行宽判满)。
        if inv_ar_sum * target_w >= vh - gaps {
            commit_column(
                &mut pending,
                &mut inv_ar_sum,
                &mut cur_x,
                &mut blocks,
                vh,
                gap,
                target_w,
                placeholder,
                false,
            );
        }
    }
    commit_column(
        &mut pending,
        &mut inv_ar_sum,
        &mut cur_x,
        &mut blocks,
        vh,
        gap,
        target_w,
        placeholder,
        true,
    );

    blocks
}

// ── B:等高泳道(lanes)───────────────────────────────────────────────────────

fn layout_lanes(
    items: &[LayoutItem],
    vh: f64,
    gap: f64,
    lane_count: usize,
    balance: bool,
    placeholder: f64,
) -> Vec<HBlock> {
    let k = lane_count.clamp(1, 8);
    let lane_h = ((vh - gap * (k as f64 - 1.0)) / k as f64).max(40.0);
    // 泳道 y 预先取整固定;项高取 floor(lane_h) 防取整后相邻泳道重叠。
    let lane_ys: Vec<f64> = (0..k)
        .map(|l| (l as f64 * (lane_h + gap)).round())
        .collect();
    let item_h = lane_h.floor().max(1.0);

    let mut cursors = vec![0.0f64; k];
    let mut blocks: Vec<HBlock> = Vec::new();

    // 按「轮」分块:每轮 k 项(末轮不满),块 bbox 覆盖本轮全部项。泳道游标只增,
    // 故块 x 单调;泳道漂移时相邻块 bbox 允许重叠,取块按 bbox 相交(hcache)。
    let mut round_items: Vec<HItem> = Vec::new();
    let mut round_min_x = f64::MAX;
    let mut round_max_right = 0.0f64;

    for (i, item) in items.iter().enumerate() {
        let lane = if balance {
            // 漂移抑制:放游标最小的泳道;并列取小序号,保证确定性。
            let mut best = 0usize;
            for l in 1..k {
                if cursors[l] < cursors[best] {
                    best = l;
                }
            }
            best
        } else {
            i % k
        };

        let ar = aspect_ratio(item, placeholder);
        let w = (ar * item_h).round().max(1.0);
        let x = cursors[lane].round();
        round_items.push(make_h_item(item, x, lane_ys[lane], w, item_h));
        round_min_x = round_min_x.min(x);
        round_max_right = round_max_right.max(x + w);
        cursors[lane] += w + gap;

        if round_items.len() == k || i == items.len() - 1 {
            blocks.push(HBlock {
                x: round_min_x,
                width: (round_max_right - round_min_x).max(1.0),
                items: std::mem::take(&mut round_items),
            });
            round_min_x = f64::MAX;
            round_max_right = 0.0;
        }
    }

    blocks
}

// ── A:分屏 justified(paged)────────────────────────────────────────────────

/// 阶段 1 产物:一行(页内几何)。`h` 为行的自然高(justified 结果);
/// items 的 x 为页内偏移、w 已定,y/h 置 0 占位——分页阶段统一落定(含纵向缩放)。
struct RawRow {
    h: f64,
    items: Vec<HItem>,
}

/// A(paged)两阶段(2026-07-02 用户反馈修订:每屏必须拉满视口高):
///   1. 全量贪心装行(页宽 justified,几何同生产 commit_row);
///   2. 分页 + **纵向 justify**——断行决策取「压缩进本页 vs 挤到下页后拉伸」中伸缩因子
///      更接近 1(失真更小)者,随后把整页行高统一缩放到恰满视口高。
///
/// 行高缩放不回改项宽(页宽已精确),引入的小幅纵横比偏差由前端 `object-fit: cover`
/// 裁切吸收(与生产 grid 模式方图裁切同哲学);末页不足视口高 0.6 时不拉伸(顶对齐)。
fn layout_paged(
    items: &[LayoutItem],
    vw: f64,
    vh: f64,
    gap: f64,
    page_factor: f64,
    target_row_height: f64,
    placeholder: f64,
) -> Vec<HBlock> {
    let page_w = (vw * page_factor.clamp(0.5, 3.0)).round();
    let target_h = target_row_height.clamp(60.0, 600.0);

    // ── 阶段 1:全量装行(与页无关)──────────────────────────────────────────
    let mut rows: Vec<RawRow> = Vec::new();
    let mut pending: Vec<&LayoutItem> = Vec::new();
    let mut ar_sum = 0.0f64;

    #[allow(clippy::too_many_arguments)]
    fn commit_row(
        rows: &mut Vec<RawRow>,
        pending: &mut Vec<&LayoutItem>,
        ar_sum: &mut f64,
        page_w: f64,
        vh: f64,
        gap: f64,
        target_h: f64,
        placeholder: f64,
        is_last: bool,
    ) {
        if pending.is_empty() {
            return;
        }
        let gaps = gap * (pending.len().saturating_sub(1)) as f64;
        let avail_w = page_w - gaps;

        let is_incomplete = is_last && *ar_sum * target_h < avail_w * LAST_UNIT_JUSTIFY_THRESHOLD;
        let ideal_h = avail_w / *ar_sum;
        let row_h = if is_incomplete {
            target_h
        } else {
            ideal_h.min(target_h * MAX_UNIT_FACTOR)
        }
        // 单行不得高于页高(极端参数防御:target×2 > 视口高时仍可放置)。
        .min(vh);
        let hit_cap = ideal_h > target_h * MAX_UNIT_FACTOR;
        let snap = !is_incomplete && !hit_cap;

        let mut widths: Vec<f64> = pending
            .iter()
            .map(|it| aspect_ratio(it, placeholder) * row_h)
            .collect();
        if snap && pending.len() > 1 {
            let total: f64 = widths.iter().sum();
            if total > 0.0 {
                let scale = avail_w / total;
                for w in widths.iter_mut() {
                    *w *= scale;
                }
            }
        }
        let mut final_w: Vec<f64> = widths.iter().map(|v| v.round()).collect();
        if snap && pending.len() > 1 {
            let current_total: f64 = final_w.iter().sum();
            let mut diff = (avail_w.round() - current_total) as i32;
            let len = final_w.len();
            let mut i = 0;
            while diff != 0 {
                if diff > 0 {
                    final_w[i % len] += 1.0;
                    diff -= 1;
                } else {
                    final_w[i % len] -= 1.0;
                    diff += 1;
                }
                i += 1;
            }
        }

        let mut x = 0.0f64;
        let mut row_items: Vec<HItem> = Vec::with_capacity(pending.len());
        for (i, item) in pending.iter().enumerate() {
            row_items.push(make_h_item(item, x.round(), 0.0, final_w[i].max(1.0), 0.0));
            x += final_w[i] + gap;
        }
        rows.push(RawRow {
            h: row_h,
            items: row_items,
        });
        pending.clear();
        *ar_sum = 0.0;
    }

    for item in items {
        let ar = aspect_ratio(item, placeholder);
        pending.push(item);
        ar_sum += ar;

        let gaps = gap * (pending.len().saturating_sub(1)) as f64;
        if ar_sum * target_h >= page_w - gaps {
            commit_row(
                &mut rows,
                &mut pending,
                &mut ar_sum,
                page_w,
                vh,
                gap,
                target_h,
                placeholder,
                false,
            );
        }
    }
    commit_row(
        &mut rows,
        &mut pending,
        &mut ar_sum,
        page_w,
        vh,
        gap,
        target_h,
        placeholder,
        true,
    );

    // ── 阶段 2:分页 + 纵向 justify ────────────────────────────────────────────
    let mut blocks: Vec<HBlock> = Vec::new();
    let mut page_rows: Vec<RawRow> = Vec::new();
    let mut sum_h = 0.0f64;

    // 封页:整页行高缩放到恰满视口高;取整后把像素差逐行 ±1 摊派,页底逐像素贴齐。
    // 末页不足阈值(0.6)则保持自然行高、顶对齐(对偶末行不拉伸规则)。
    fn close_page(
        page_rows: &mut Vec<RawRow>,
        blocks: &mut Vec<HBlock>,
        page_w: f64,
        vh: f64,
        gap: f64,
        is_last_page: bool,
    ) {
        if page_rows.is_empty() {
            return;
        }
        let n = page_rows.len();
        let gaps = gap * (n - 1) as f64;
        let avail = vh - gaps;
        let sum_h: f64 = page_rows.iter().map(|r| r.h).sum();
        let justify = !is_last_page || sum_h >= avail * LAST_UNIT_JUSTIFY_THRESHOLD;

        let mut heights: Vec<f64> = if justify {
            let f = avail / sum_h;
            page_rows.iter().map(|r| (r.h * f).round()).collect()
        } else {
            page_rows.iter().map(|r| r.h.round()).collect()
        };
        if justify {
            let current: f64 = heights.iter().sum();
            let mut diff = (avail.round() - current) as i32;
            let len = heights.len();
            let mut i = 0;
            while diff != 0 {
                if diff > 0 {
                    heights[i % len] += 1.0;
                    diff -= 1;
                } else {
                    heights[i % len] -= 1.0;
                    diff += 1;
                }
                i += 1;
            }
        }

        let base_x = blocks.len() as f64 * (page_w + gap);
        let mut y = 0.0f64;
        let mut page_items: Vec<HItem> = Vec::new();
        for (ri, row) in page_rows.drain(..).enumerate() {
            for mut it in row.items {
                it.x += base_x;
                it.y = y.round();
                it.h = heights[ri].max(1.0);
                page_items.push(it);
            }
            y += heights[ri] + gap;
        }
        blocks.push(HBlock {
            x: base_x,
            width: page_w,
            items: page_items,
        });
    }

    for row in rows {
        if !page_rows.is_empty() {
            let n_excl = page_rows.len() as f64;
            let avail_incl = vh - gap * n_excl;
            let f_incl = avail_incl / (sum_h + row.h);
            if f_incl < 1.0 {
                // 断行决策(经典 line-breaking 代价):比较「本行压缩进本页」|ln 1/f_incl|
                // 与「本行挤到下页、本页拉伸」|ln f_excl|,取更接近 1(失真更小)者。
                let avail_excl = vh - gap * (n_excl - 1.0);
                let f_excl = avail_excl / sum_h; // ≥1:累积时已保证放得下
                if (1.0 / f_incl).ln() <= f_excl.ln() {
                    page_rows.push(row);
                    close_page(&mut page_rows, &mut blocks, page_w, vh, gap, false);
                    sum_h = 0.0;
                    continue;
                }
                close_page(&mut page_rows, &mut blocks, page_w, vh, gap, false);
                sum_h = 0.0;
            }
        }
        sum_h += row.h;
        page_rows.push(row);
    }
    close_page(&mut page_rows, &mut blocks, page_w, vh, gap, true);

    blocks
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小 LayoutItem fixture(同 justified.rs 测试惯例):仅关心 id/宽高,其余无害默认。
    fn mk_item(id: i64, w: i64, h: i64) -> LayoutItem {
        LayoutItem {
            id,
            width: w,
            height: h,
            file_size: 0,
            sort_datetime: id,
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

    fn params(mode: HLayoutMode) -> HLayoutParams {
        HLayoutParams {
            viewport_width: 1000.0,
            viewport_height: 800.0,
            gap: 0.0,
            mode,
        }
    }

    #[test]
    fn empty_items_yield_empty_blocks() {
        let p = params(HLayoutMode::Columns {
            target_col_width: 200.0,
        });
        assert!(compute_horizontal_layout(&[], &p).is_empty());
    }

    /// C:完整列恰满视口高(摊派后逐像素),末列不满不拉伸(用目标宽、顶对齐)。
    #[test]
    fn columns_fill_height_exactly_and_last_col_not_stretched() {
        // 10 张 160×90(ar≈1.778,1/ar=0.5625),H=800,target=200:
        // 判满 inv_sum×200 ≥ 800 → 第 8 张提交(4.5),末列剩 2 张不满。
        let items: Vec<LayoutItem> = (1..=10).map(|i| mk_item(i, 160, 90)).collect();
        let p = params(HLayoutMode::Columns {
            target_col_width: 200.0,
        });
        let blocks = compute_horizontal_layout(&items, &p);

        assert_eq!(blocks.len(), 2, "8 + 2 两列");
        let full = &blocks[0];
        assert_eq!(full.items.len(), 8);
        // 完整列:列宽 = 800/4.5 ≈ 177.78 → 178;列底恰贴 800(±1 舍入容差)。
        assert_eq!(full.width, 178.0);
        for it in &full.items {
            assert_eq!(it.w, full.width, "列内同宽");
        }
        let bottom = full.items.last().map(|it| it.y + it.h).unwrap();
        assert!(
            (bottom - 800.0).abs() <= 1.0,
            "完整列底应贴视口底,实际 {bottom}"
        );

        // 末列:不满(2×0.5625×200=225 < 480)→ 列宽=target,顶对齐,不贴底。
        let last = &blocks[1];
        assert_eq!(last.items.len(), 2);
        assert_eq!(last.width, 200.0);
        assert_eq!(last.items[0].y, 0.0);
        let last_bottom = last.items.last().map(|it| it.y + it.h).unwrap();
        assert!(last_bottom < 800.0 * LAST_UNIT_JUSTIFY_THRESHOLD + 1.0);
    }

    /// C:x 单调推进且 gap 记账正确(块间距 = 列宽 + gap)。
    #[test]
    fn columns_x_monotonic_with_gap() {
        let items: Vec<LayoutItem> = (1..=24).map(|i| mk_item(i, 100, 100)).collect();
        let mut p = params(HLayoutMode::Columns {
            target_col_width: 200.0,
        });
        p.gap = 4.0;
        let blocks = compute_horizontal_layout(&items, &p);
        assert!(blocks.len() >= 3);
        for w in blocks.windows(2) {
            let expected = w[0].x + w[0].width + 4.0;
            assert!(
                (w[1].x - expected).abs() <= 1.0,
                "相邻列 x 步距应为 列宽+gap:{} vs {}",
                w[1].x,
                expected
            );
        }
        // 列内 gap:相邻项 y 间距 = 上项高 + gap。
        let col = &blocks[0];
        for pair in col.items.windows(2) {
            assert!((pair[1].y - (pair[0].y + pair[0].h + 4.0)).abs() <= 1.0);
        }
    }

    /// C:单张尾随全景图 → 不满末列,列宽=target,项高=target/ar。
    #[test]
    fn columns_trailing_panorama_uses_target_width() {
        let items = vec![mk_item(1, 5000, 1000)]; // ar 钳制到 5.0
        let p = params(HLayoutMode::Columns {
            target_col_width: 200.0,
        });
        let blocks = compute_horizontal_layout(&items, &p);
        assert_eq!(blocks.len(), 1);
        let it = &blocks[0].items[0];
        assert_eq!((it.w, it.h), (200.0, 40.0), "200/5=40 高,顶对齐留白");
        assert_eq!(it.y, 0.0);
    }

    /// B:严格列主序(item i → 泳道 i mod k),泳道 y 固定,同泳道 x 递增且不重叠。
    #[test]
    fn lanes_strict_round_robin_geometry() {
        // k=3,H=904,gap=2 → lane_h=(904-4)/3=300;方图 w=floor(300)=300。
        let items: Vec<LayoutItem> = (0..7).map(|i| mk_item(i + 1, 100, 100)).collect();
        let p = HLayoutParams {
            viewport_width: 1000.0,
            viewport_height: 904.0,
            gap: 2.0,
            mode: HLayoutMode::Lanes {
                lane_count: 3,
                balance: false,
            },
        };
        let blocks = compute_horizontal_layout(&items, &p);
        assert_eq!(blocks.len(), 3, "7 项按轮分块:3+3+1");
        assert_eq!(blocks[2].items.len(), 1);

        let all: Vec<&HItem> = blocks.iter().flat_map(|b| &b.items).collect();
        // 泳道 y:round(l×302) = 0/302/604;item i 落泳道 i%3。
        for (i, it) in all.iter().enumerate() {
            let lane = i % 3;
            assert_eq!(it.y, (lane as f64 * 302.0).round(), "item {i} 泳道错位");
            assert_eq!(it.h, 300.0);
        }
        // 同泳道(0):x 依次 0, 302, 604(w=300 + gap=2),不重叠。
        assert_eq!((all[0].x, all[3].x, all[6].x), (0.0, 302.0, 604.0));
        // 块 bbox 覆盖其全部项。
        for b in &blocks {
            for it in &b.items {
                assert!(it.x >= b.x && it.x + it.w <= b.x + b.width + 0.5);
            }
        }
        // 块 x 单调不减。
        for w in blocks.windows(2) {
            assert!(w[1].x >= w[0].x);
        }
    }

    /// B:balance=true 把后续项放最落后泳道(全景图不再拖着自己的泳道漂移)。
    #[test]
    fn lanes_balance_assigns_shortest_lane() {
        // k=2:先放一张全景(ar 5 → 极宽),strict 下 item2 会回泳道 0;
        // balance 下 item1/2/3 应全落泳道 1(其游标一直更短)。
        let items = vec![
            mk_item(1, 5000, 1000),
            mk_item(2, 100, 100),
            mk_item(3, 100, 100),
            mk_item(4, 100, 100),
        ];
        let p = HLayoutParams {
            viewport_width: 1000.0,
            viewport_height: 800.0,
            gap: 0.0,
            mode: HLayoutMode::Lanes {
                lane_count: 2,
                balance: true,
            },
        };
        let blocks = compute_horizontal_layout(&items, &p);
        let all: Vec<&HItem> = blocks.iter().flat_map(|b| &b.items).collect();
        let lane1_y = all[1].y;
        assert!(lane1_y > 0.0, "item2 应在泳道 1");
        assert_eq!(all[2].y, lane1_y, "item3 应继续泳道 1(仍最短)");
        assert_eq!(all[3].y, lane1_y, "item4 应继续泳道 1(400+400 < 2000)");
    }

    /// 页内某行的项集合(按 y 分组的辅助)。
    fn rows_of(block: &HBlock) -> Vec<(f64, Vec<&HItem>)> {
        let mut ys: Vec<f64> = block.items.iter().map(|it| it.y).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ys.dedup();
        ys.into_iter()
            .map(|y| (y, block.items.iter().filter(|it| it.y == y).collect()))
            .collect()
    }

    /// A(2026-07-02 修订):每页(含够高末页)纵向 justify 恰满视口高;页 x 步距 = 页宽 + gap;
    /// 完整行恰满页宽;断行决策在压缩失真更小时把溢出行压进本页。
    #[test]
    fn paged_pages_fill_viewport_height_exactly() {
        // 视口 1000×500,factor 1.2 → 页宽 1200;方图 target_h=200 → 每行 6 张、自然行高 200。
        // 26 张 → 4 整行 + 末行 2 张。分页:页1 累到第 3 行时 f_incl=500/600≈0.833(|ln|=0.182)
        // 优于 f_excl=1.25(|ln|=0.223)→ 3 行压入,缩至 166/167/167;页2 = 整行+末行拉伸 1.25。
        let items: Vec<LayoutItem> = (1..=26).map(|i| mk_item(i, 100, 100)).collect();
        let p = HLayoutParams {
            viewport_width: 1000.0,
            viewport_height: 500.0,
            gap: 0.0,
            mode: HLayoutMode::Paged {
                page_factor: 1.2,
                target_row_height: 200.0,
            },
        };
        let blocks = compute_horizontal_layout(&items, &p);
        assert_eq!(blocks.len(), 2, "断行决策应把第 3 行压进页 1 → 18+8 两页");
        assert_eq!(blocks[0].items.len(), 18);
        assert_eq!(blocks[1].items.len(), 8);

        for (pi, b) in blocks.iter().enumerate() {
            assert_eq!(b.x, pi as f64 * 1200.0, "页 x 步距");
            assert_eq!(b.width, 1200.0);
            // 用户反馈修订核心:页底逐像素贴齐视口底。
            let bottom = b.items.iter().map(|it| it.y + it.h).fold(0.0, f64::max);
            assert_eq!(bottom, 500.0, "页 {pi} 底缘应恰贴视口底");
            for it in &b.items {
                assert!(
                    it.x >= b.x && it.x + it.w <= b.x + b.width + 1.0,
                    "项应落在本页 x 范围内"
                );
            }
        }
        // 页 1:3 行,压缩后行高 ∈ {166,167} 且总和恰 500。
        let p0_rows = rows_of(&blocks[0]);
        assert_eq!(p0_rows.len(), 3);
        let h_sum: f64 = p0_rows.iter().map(|(_, its)| its[0].h).sum();
        assert_eq!(h_sum, 500.0);
        // 页 1 首行:6 张恰满 1200。
        let (_, row0) = &p0_rows[0];
        assert_eq!(row0.len(), 6);
        let right = row0.iter().map(|it| it.x + it.w).fold(0.0, f64::max);
        assert!((right - 1200.0).abs() <= 1.0, "完整行右缘应贴页宽");

        // 页 2(末页):Σ自然高 400 ≥ 0.6×500 → 拉伸 1.25:整行与不满末行均 250 高;
        // 不满末行宽度保持自然(2×200,不横向拉伸),纵横比偏差由前端 cover 裁切。
        let p1_rows = rows_of(&blocks[1]);
        assert_eq!(p1_rows.len(), 2);
        assert_eq!(p1_rows[0].1[0].h, 250.0);
        let (_, last_row) = &p1_rows[1];
        assert_eq!(last_row.len(), 2);
        assert_eq!((last_row[0].w, last_row[0].h), (200.0, 250.0));
    }

    /// A 断行决策反向用例:压缩失真更大时,溢出行挤到下页、本页拉伸;
    /// 极不满的末页(Σ高 < 0.6×视口高)保持自然行高、顶对齐不拉伸。
    #[test]
    fn paged_page_break_prefers_stretch_when_compression_worse() {
        // 视口高 450:两行 400 后第 3 行 f_incl=450/600=0.75(|ln|=0.288)
        // 劣于 f_excl=450/400=1.125(|ln|=0.118)→ 挤到下页;页 1 两行拉伸至 225。
        // 页 2 仅一行(200 < 0.6×450=270)→ 不拉伸。
        let items: Vec<LayoutItem> = (1..=18).map(|i| mk_item(i, 100, 100)).collect();
        let p = HLayoutParams {
            viewport_width: 1000.0,
            viewport_height: 450.0,
            gap: 0.0,
            mode: HLayoutMode::Paged {
                page_factor: 1.2,
                target_row_height: 200.0,
            },
        };
        let blocks = compute_horizontal_layout(&items, &p);
        assert_eq!(blocks.len(), 2, "3 整行应分为 2+1 两页");

        let p0_bottom = blocks[0]
            .items
            .iter()
            .map(|it| it.y + it.h)
            .fold(0.0, f64::max);
        assert_eq!(p0_bottom, 450.0, "页 1 拉伸后应贴底");
        assert!(blocks[0].items.iter().all(|it| it.h == 225.0));

        let p1_bottom = blocks[1]
            .items
            .iter()
            .map(|it| it.y + it.h)
            .fold(0.0, f64::max);
        assert_eq!(p1_bottom, 200.0, "极不满末页应保持自然高、顶对齐");
        assert!(blocks[1]
            .items
            .iter()
            .all(|it| (it.y, it.h) == (0.0, 200.0)));
    }

    /// 0×0 未测量项使用已测项中位 ar 占位(形状合理,非正方形)。
    #[test]
    fn unmeasured_items_use_median_placeholder() {
        // 已测两张 ar=2.0 → 中位 2.0;0×0 项在 lanes 下应得 w = 2×item_h。
        let items = vec![mk_item(1, 200, 100), mk_item(2, 200, 100), mk_item(3, 0, 0)];
        let p = HLayoutParams {
            viewport_width: 1000.0,
            viewport_height: 400.0,
            gap: 0.0,
            mode: HLayoutMode::Lanes {
                lane_count: 1,
                balance: false,
            },
        };
        let blocks = compute_horizontal_layout(&items, &p);
        let all: Vec<&HItem> = blocks.iter().flat_map(|b| &b.items).collect();
        assert_eq!(all[2].w, all[2].h * 2.0, "占位 ar 应为中位 2.0");
    }
}
