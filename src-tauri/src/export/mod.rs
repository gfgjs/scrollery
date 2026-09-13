//! 导出整理成果(方案 A)。
//!
//! v1 交付 = 当前选区/相册/视图的**有序媒体集合**原样字节复制到用户指定父目录下的独立子目录,
//! 可选生成 manifest(标签/评级/相册/相对路径,方案 §2.4)。不重编码、不改源文件、不改库内状态。
//!
//! - `naming`:命名档(original/sequence/date)+ 文件名合规化,纯函数,穷举边界单测。
//! - `manifest`:manifest 结构与原子写入。
//! - `core`:导出引擎——staging 内逐项 `.tmp`→rename → manifest → 整目录 rename 落正式目录。
//!   设计为纯函数(时间戳/job_id/取消令牌由调用方注入,同 `backup::core` 姿态),便于单测确定性;
//!   IPC/状态/门闩/进度事件的 app 层接线在 `ipc::export_commands`。

pub mod core;
pub mod manifest;
pub mod naming;

pub use core::{
    ensure_target_writable, is_inside_library, run_export, ExportConflict, ExportItemResult,
    ExportOutcome, ExportParams,
};
pub use manifest::ExportSource;
pub use naming::NamingScheme;
