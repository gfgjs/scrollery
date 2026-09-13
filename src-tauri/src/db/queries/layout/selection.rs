//! 选择解析:`SelectionDescriptor` → 实际 id 集合 / 精确计数,经 `view_to_sql` 在 SQL 层取全集。

use rusqlite::Connection;

use crate::db::models::SelectionDescriptor;
use crate::error::{AppError, Result};

use super::query_builder::view_to_sql;

/// 选择规模上限（T18 D4 初值，可调）。
pub(in crate::db::queries::layout) const SELECTION_EXPLICIT_MAX: usize = 100_000;
/// 批量写分块大小（S3 消费 + count 交集分块）：平衡单事务大小与往返次数。
pub const SELECTION_BATCH_CHUNK: usize = 5_000;

/// 把 `SelectionDescriptor` 展开为实际 id 集合（按视图布局序）。
///
/// - `Explicit{ids}`：上限校验后原样返回。
/// - `SelectAll{view, excluded}`：先以 `current_layout_version` 守门（不一致 → `ViewStale`），
///   再经 `view_to_sql` 流式取全集 id，扣除 `excluded_ids`（HashSet 过滤）。
///
/// `current_layout_version` 由调用方（IPC 命令）从 `AppState` 的 LayoutCache 读出传入 —— 本 DB 层
/// 不依赖全局状态，保持纯函数可测。
pub fn resolve_selection(
    conn: &Connection,
    sel: &SelectionDescriptor,
    current_layout_version: u64,
) -> Result<Vec<i64>> {
    match sel {
        SelectionDescriptor::Explicit { ids } => {
            if ids.len() > SELECTION_EXPLICIT_MAX {
                return Err(AppError::Internal(format!(
                    "显式选择 {} 项超过上限 {SELECTION_EXPLICIT_MAX}，请改用全选（SelectAll）",
                    ids.len()
                )));
            }
            Ok(ids.clone())
        }
        SelectionDescriptor::SelectAll { view, excluded_ids } => {
            if view.layout_version != current_layout_version {
                return Err(AppError::ViewStale);
            }
            // 隐藏根排除（V21）：全选须与画廊可见集一致，不含隐藏根媒体。
            let (sql, params) = view_to_sql(view, &super::super::scan::hidden_root_ids(conn)?)?;
            let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(refs.as_slice(), |row| row.get::<_, i64>(0))?;

            if excluded_ids.is_empty() {
                return rows.map(|r| r.map_err(AppError::from)).collect();
            }
            // excluded 通常很小，HashSet 过滤即可（无需把 NOT IN 灌进 SQL）。
            let excluded: std::collections::HashSet<i64> = excluded_ids.iter().copied().collect();
            let mut out = Vec::new();
            for r in rows {
                let id = r?;
                if !excluded.contains(&id) {
                    out.push(id);
                }
            }
            Ok(out)
        }
    }
}

/// 仅计数 `SelectionDescriptor`（UI「将操作 N 项」），SelectAll 走 `COUNT(*)` 不取全 id。
///
/// 精确计数（T18 D3）：`COUNT(*) − (excluded ∩ view)`。不近似为 `total − excluded.len()`，因
/// `excluded` 可能含已不在视图的 id；分块统计其与视图的真实交集，保「已选 N 项」与实际一致。
pub fn count_selection(
    conn: &Connection,
    sel: &SelectionDescriptor,
    current_layout_version: u64,
) -> Result<u64> {
    match sel {
        SelectionDescriptor::Explicit { ids } => {
            if ids.len() > SELECTION_EXPLICIT_MAX {
                return Err(AppError::Internal(format!(
                    "显式选择 {} 项超过上限 {SELECTION_EXPLICIT_MAX}，请改用全选（SelectAll）",
                    ids.len()
                )));
            }
            Ok(ids.len() as u64)
        }
        SelectionDescriptor::SelectAll { view, excluded_ids } => {
            if view.layout_version != current_layout_version {
                return Err(AppError::ViewStale);
            }
            // 隐藏根排除（V21）：计数与全选同集，不含隐藏根媒体。
            let (view_sql, params) =
                view_to_sql(view, &super::super::scan::hidden_root_ids(conn)?)?;
            // 包成 COUNT(*) 子查询：内层 ?1..?k 仍按位绑定 params。子查询 ORDER BY 对 COUNT 无意义
            // 但无害（v1 容忍微小浪费）。
            let count_sql = format!("SELECT COUNT(*) FROM ({view_sql})");
            let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
            let total: i64 = conn.query_row(&count_sql, refs.as_slice(), |row| row.get(0))?;

            if excluded_ids.is_empty() {
                return Ok(total as u64);
            }
            // 交集（excluded ∩ view）：分块 IN 统计（占位从 params.len()+1 起，接续内层绑定）。
            let mut intersect: i64 = 0;
            for chunk in excluded_ids.chunks(SELECTION_BATCH_CHUNK) {
                let placeholders: Vec<String> = (0..chunk.len())
                    .map(|i| format!("?{}", params.len() + i + 1))
                    .collect();
                let in_sql = format!(
                    "SELECT COUNT(*) FROM ({view_sql}) WHERE id IN ({})",
                    placeholders.join(",")
                );
                let mut all_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|b| b.as_ref()).collect();
                for v in chunk {
                    all_refs.push(v as &dyn rusqlite::ToSql);
                }
                let c: i64 = conn.query_row(&in_sql, all_refs.as_slice(), |row| row.get(0))?;
                intersect += c;
            }
            Ok((total - intersect).max(0) as u64)
        }
    }
}
