//! 布局查询核心回归：选区解析、批量写入边界与隐藏目录过滤。
//!
//! 本模块统一持有各测试文件共用的 import,子文件沿用原来的 `use super::*;` 一行拿全。

use super::item_queries::canonical_layout_sql;
use super::*;

use rusqlite::{params, Connection};

use crate::db::models::MediaFilter;
use crate::error::AppError;

mod hidden_root_exclusion;
mod selection_resolve;
