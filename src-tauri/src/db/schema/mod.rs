//! DDL：CREATE TABLE / CREATE INDEX 语句。
//! 在模式引导期间从 `migration.rs` 调用一次。
//!
//! 按版本纪元切分为三个子文件（`early`/`mid`/`late`），本文件仅做 `pub use` 全量重导出，
//! 外部路径 `crate::db::schema::SCHEMA_Vn` 保持不变；拆分断点严格落在版本号之间。

mod early;
mod late;
mod mid;

pub use early::*;
pub use late::*;
pub use mid::*;
