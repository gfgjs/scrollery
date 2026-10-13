//! 数据备份与恢复。
//!
//! 完整 catalog DB + `documents/**` + manifest 的一致快照:
//! - `manifest`:包格式 / 版本 / 计数 / payload SHA-256 类型(自描述元数据)。
//! - `core`:VACUUM INTO 快照、文档校验、流式 ZIP 与逐条 SHA-256，临时包同卷发布后轮转自动包。
//!   时间戳与 id 由调用方注入；IPC、状态、取消与进度事件由 `ipc::backup_commands` 编排。
//!
//! 恢复状态机 validate→stage→arm→restart→swap 见 [`restore`] 与 [`swap`]。
//!
//! 前置不变量:`core::run_backup` **须在持 `AppState.document_storage_guard` write guard
//! 的 blocking 上下文调用**——保证 VACUUM 快照与 documents 文件来自同一逻辑时点,文档写不穿插。

pub mod core;
mod dbread;
pub mod manifest;
pub mod restore;
pub mod swap;

pub use core::{run_backup, BackupOutcome, BackupParams, ManifestMeta};
pub use manifest::{
    BackupKind, Counts, Manifest, PayloadEntry, RootEntry, BACKUP_FILE_EXT, BACKUP_FORMAT_VERSION,
};
pub use restore::{is_safe_backup_id, restore_discard, restore_stage, RestoreStageResult};
pub use swap::{
    arm_restore, finalize_restore_verified, perform_swap_at_boot, rollback_restore_at_boot,
    PendingRestore, RestorePhase,
};
