//! 布局查询域 DAO:LayoutItem mapper、canonical 布局查询、filter/sort SQL builder
//! (push_query_body/push_where_predicates/push_order_by)、view_to_sql、selection 解析、
//! 目录标签与格式 facet(T 线拆分自 queries.rs,SQL 与行为不变;mapper+SELECT 列清单+
//! 三 builder+排序等价测试必须同模块,§4.1)。
//!
//! (本域已按 item_queries / query_builder / selection / facets 四子模块拆分,测试树见
//! `tests/`;本文件仅作 facade,对外路径 `crate::db::queries::layout::*` 与拆分前一致。)

mod facets;
mod item_queries;
mod query_builder;
mod selection;

#[cfg(test)]
mod tests;

pub use facets::{list_library_formats, query_dir_labels};
pub use item_queries::{
    query_item_ids_filename_order, query_layout_items, query_layout_items_canonical,
    query_layout_items_filename_baseline,
};
pub use query_builder::view_to_sql;
pub use selection::{count_selection, resolve_selection, SELECTION_BATCH_CHUNK};
