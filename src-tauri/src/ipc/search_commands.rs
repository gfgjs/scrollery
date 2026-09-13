//! 阶段 1：文件名 LIKE 搜索（§ 6.1 — 搜索）。

use std::sync::Arc;

use tauri::State;

use crate::db::models::{MediaFilter, SearchResult};
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::state::AppState;

/// 通过文件名搜索媒体项（LIKE 查询）。
/// 阶段 3 将迁移到 FTS5。
///
/// 前端必须进行 150 毫秒的调用防抖（在 AppToolbar.vue 中）。
#[tauri::command]
pub async fn search_media(
    query: String,
    directory_id: Option<i64>,
    filters: Option<MediaFilter>,
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<SearchResult>> {
    // span 埋点(W1,D-312 info 档:LIKE 扫库可达秒级,真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:search_media");
    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    let mut filter = filters.unwrap_or_default();
    if let Some(dir_id) = directory_id {
        filter.directory_id = Some(dir_id);
    }

    // R1-3：LIKE 扫库可达秒级，绝不能占 tokio worker（CLAUDE.md rusqlite 硬化）。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
        q::search_media(&pool, &query, &filter, limit.unwrap_or(100))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}
