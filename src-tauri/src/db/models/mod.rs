//! 镜像数据库行的 Rust 结构体。
//! 所有结构体都实现 `serde::{Serialize, Deserialize}` 以用于 IPC。
//!
//! 按域拆分(tierB-4 简案):`scan`(扫描根/目录树)/`media`(媒体项/元数据)/
//! `view`(过滤器/视图描述符/选择契约)/`ai_face`(AI/人脸)/`storage`(存储后端/卷)/
//! `derived`(派生结果/收藏夹/文档/杂项)。本文件 `pub use` 全量再导出,
//! `crate::db::models::{...}` 既有调用路径零改动。

mod ai_face;
mod derived;
mod media;
mod scan;
mod storage;
mod view;

pub use ai_face::*;
pub use derived::*;
pub use media::*;
pub use scan::*;
pub use storage::*;
pub use view::*;
