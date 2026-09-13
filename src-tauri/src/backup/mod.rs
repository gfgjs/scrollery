//! 数据备份与恢复(方案 B)。
//!
//! v1 交付 = **完整 catalog DB + `documents/**` + manifest** 的一致快照(方案 §2.1):
//! - `manifest`:包格式 / 版本 / 计数 / payload SHA-256 类型(自描述元数据)。
//! - `core`:备份引擎——preflight → VACUUM INTO 一致快照 → 文档一致性校验 → zip 流式打包 +
//!   逐条 SHA-256 → `*.tmp` 同卷 rename → 自动包 retention。设计为**纯函数**(时间戳/id 由调用方
//!   注入),便于单测确定性;IPC/状态/取消/进度事件的 app 层接线在 `ipc::backup_commands`(阶段 2b)。
//!
//! 恢复状态机(validate→stage→arm→restart→swap,方案 §6)在阶段 3 落地。
//!
//! 前置不变量(方案 §3.1):`core::run_backup` **须在持 `AppState.document_storage_guard` write guard
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
pub use restore::{is_safe_backup_id, restore_stage, RestoreStageResult};
pub use swap::{
    arm_restore, finalize_restore_verified, perform_swap_at_boot, rollback_restore_at_boot,
    PendingRestore, RestorePhase,
};
