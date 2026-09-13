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

// wildcard `pub use` 不会把非 `pub` 项提级暴露:谓词构造器定义处即为
// `pub(in crate::db::queries)`,须在此按同等可见性具名重导出(不可放宽),
// 保 `queries::search` 现有 `use super::layout::push_in_predicate;` 零改动可编译。
pub(in crate::db::queries) use query_builder::push_in_predicate;
// `push_root_exclusion` 当前查无跨域调用点,但其 `pub(in crate::db::queries)` 可见性本身
// 即设计上预留的跨域复用面——同等重导出以保持对称,免未来复用时二次改动。
#[allow(unused_imports)]
pub(in crate::db::queries) use query_builder::push_root_exclusion;
