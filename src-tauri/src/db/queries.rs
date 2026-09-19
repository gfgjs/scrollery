//! 可重用的参数化 SQL 查询函数。
//! 所有 SQL 均使用参数绑定 — 绝不使用字符串拼接。

// ── 领域子模块与稳定 re-export(T 线拆分,本文件为 facade;调用方路径 ────────────
// ── `crate::db::queries::<symbol>` 不变,子模块私有、由编译器禁止直引)──────────
mod ai;
mod collections;
mod config;
mod dedup;
mod derivations;
mod documents;
mod exotic;
mod export;
mod faces;
mod layout;
mod media;
mod metadata;
mod move_journal;
mod scan;
mod storage;
mod thumbnail;

pub use ai::*;
pub use collections::*;
pub use config::*;
pub use dedup::*;
pub use derivations::*;
pub use documents::*;
pub use exotic::*;
pub use export::*;
pub use faces::*;
pub use layout::*;
pub use media::*;
pub use metadata::*;
pub use move_journal::*;
pub use scan::*;
pub use storage::*;
pub use thumbnail::*;
