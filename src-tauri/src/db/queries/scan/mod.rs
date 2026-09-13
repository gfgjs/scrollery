//! 扫描域 DAO:scan root、目录树、fast scan 写入、source-change 失效、缺失标记
//! (T 线拆分自 queries.rs,SQL/事务边界不变;`invalidate_derived_for_item` 由
//! source-change 事务 owner 持有,§4.1)。
//!
//! 目录再拆分(D 线,`scan.rs` → `scan/` 4 域文件):roots / directories /
//! media_upsert / mark_missing,各自职责见对应文件头注释。本文件是纯 facade——
//! 不放函数体,只做 `mod` 声明与 `pub use` 重导出,`queries.rs` 侧的
//! `mod scan; pub use scan::*;` 一行不变。

mod directories;
mod mark_missing;
mod media_upsert;
mod roots;

pub use directories::*;
pub use mark_missing::*;
pub use media_upsert::*;
pub use roots::*;
