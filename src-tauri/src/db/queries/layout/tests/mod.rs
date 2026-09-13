//! 布局查询域测试树(整体迁自原 layout.rs 的五个 `#[cfg(test)] mod`,用例内容未改)。
//!
//! 本模块统一持有各测试文件共用的 import,子文件沿用原来的 `use super::*;` 一行拿全。

use super::item_queries::{canonical_layout_sql, map_layout_item, sort_canonical};
use super::selection::SELECTION_EXPLICIT_MAX;
use super::*;

use rusqlite::{params, Connection};

use crate::db::models::MediaFilter;
use crate::error::{AppError, Result};

mod canonical_plan;
mod format_facet;
mod hidden_root_exclusion;
mod lens_lowering;
mod selection_resolve;
mod view_layout_parity;
mod view_to_sql;
