//! H-Lab(横向画廊实验)IPC 命令(docs/designs/2026-07-02-horizontal-gallery-lab.md §4)。
//! 与生产 layout_commands 平行:同款「查询 + CPU 布局同入一个 spawn_blocking」纪律、
//! 同款 AppError 错误契约;缓存/版本完全独立,互不可见。

use std::sync::Arc;

use tauri::State;

use crate::db::models::MediaFilter;
use crate::db::queries::query_layout_items;
use crate::error::{AppError, Result};
use crate::layout::hcache::{get_h_blocks_by_x as cache_get_blocks, store_h_layout};
use crate::layout::horizontal::{compute_horizontal_layout, HBlock, HLayoutMode, HLayoutParams};
use crate::state::AppState;

/// H 布局计算参数。filters/directory_id 预留视图镜像能力,实验 v1 前端不传(全库)。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeHLayoutParams {
    pub directory_id: Option<i64>,
    pub filters: Option<MediaFilter>,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub gap: f64,
    /// 时间方向('asc' | 'desc'):算法不感知,由查询层排序承担(plan §3 公共约定)。
    pub sort_order: Option<String>,
    pub mode: HLayoutMode,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HLayoutSummary {
    pub total_width: f64,
    pub block_count: usize,
    pub total_items: usize,
    pub layout_version: u64,
    /// 查询 + 布局的后端耗时(实验控制条展示,供参数调优参考)。
    pub compute_ms: u64,
}

/// 计算横向实验布局并存入独立缓存,返回摘要。
#[tauri::command]
pub async fn compute_h_layout(
    params: ComputeHLayoutParams,
    state: State<'_, Arc<AppState>>,
) -> Result<HLayoutSummary> {
    // span 埋点(W1,D-312 debug 档:compute_layout 同类视口热路径查询,默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:compute_h_layout");
    state.note_interaction();

    let filter = {
        let mut f = params.filters.unwrap_or_default();
        if let Some(dir_id) = params.directory_id {
            f.directory_id = Some(dir_id);
        }
        f
    };

    let state_arc = state.inner().clone();
    let sort_order = params.sort_order.clone();
    let h_params = HLayoutParams {
        viewport_width: params.viewport_width,
        viewport_height: params.viewport_height,
        gap: params.gap,
        mode: params.mode,
    };

    let started = std::time::Instant::now();
    // 查询与 CPU 布局同入一个阻塞任务(「async command 内 rusqlite 一律 spawn_blocking」硬化纪律);
    // 读连接仅查询期间持有,布局计算前释放(同生产 compute_layout 的池连接纪律)。
    let (blocks, total_items): (Vec<HBlock>, usize) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<HBlock>, usize)> {
            let items = {
                let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
                query_layout_items(
                    &pool,
                    &filter,
                    Some("none"), // 实验 v1 无分组/分隔符(plan §6 显式推迟)
                    Some("datetime"),
                    sort_order.as_deref(),
                    false,
                )?
            };
            let total = items.len();
            Ok((compute_horizontal_layout(&items, &h_params), total))
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
    let compute_ms = started.elapsed().as_millis() as u64;

    let block_count = blocks.len();
    // lanes 模式泳道漂移时最右缘未必在末块 → 总宽取全块 bbox 右缘的最大值。
    let total_width = blocks.iter().map(|b| b.x + b.width).fold(0.0f64, f64::max);

    let layout_version = store_h_layout(&state.h_layout_cache, blocks, total_width);

    Ok(HLayoutSummary {
        total_width,
        block_count,
        total_items,
        layout_version,
        compute_ms,
    })
}

/// 取与 [left_x, right_x] 相交的块。版本不符/无布局 → LayoutNotReady(前端按码重算)。
#[tauri::command]
pub async fn get_h_blocks_by_x(
    left_x: f64,
    right_x: f64,
    layout_version: Option<u64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HBlock>> {
    // 滚动 = 主动交互 → 节流后台解码(同生产取行命令)。
    state.note_interaction();
    cache_get_blocks(&state.h_layout_cache, left_x, right_x, layout_version)
        .ok_or(AppError::LayoutNotReady)
}
