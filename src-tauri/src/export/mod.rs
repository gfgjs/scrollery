//! 导出整理成果。
//!
//! 将选区、相册或视图的有序媒体集合原样复制到指定目录，可选生成 manifest。
//! 不重编码，不修改源文件或库内状态。
//!
//! - `naming`:命名档(original/sequence/date)与文件名合规化。
//! - `manifest`:manifest 结构与原子写入。
//! - `core`:导出引擎——staging 内逐项 `.tmp`→rename → manifest → 整目录 rename 落正式目录。
//!   时间戳、job_id 和取消令牌由调用方注入；IPC 与进度事件由 `ipc::export_commands` 编排。

pub mod core;
pub mod manifest;
pub mod naming;
mod source;

pub use core::{
    ensure_target_writable, is_inside_library, run_export, ExportConflict, ExportItemResult,
    ExportOutcome, ExportParams,
};
pub use manifest::ExportSource;
pub use naming::NamingScheme;

/// 打开扫描根内已授权的源文件，并核验最终句柄边界。
pub(crate) use source::open as open_authorized_source;
