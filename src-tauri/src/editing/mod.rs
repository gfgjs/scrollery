//! 图片简单编辑(方案 C):90° 旋转 / 翻转 / 裁剪,另存副本不覆盖原图。
//!
//! 施工序见 `docs/worklogs/2026-07-19-图片简单编辑施工/task_plan.md`:先过 `metadata`(P0 元数据/
//! 色彩 spike)与内存基准两道 P0 门槛,再进入 `save_edited_image` 命令与几何处理链。

pub mod adjust;
pub mod color;
pub mod entitlement;
pub mod geometry;
pub mod ingest;
pub mod io;
pub mod memory_budget;
pub mod metadata;
pub mod naming;
pub mod preview;
