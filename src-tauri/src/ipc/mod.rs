pub mod ai_commands;
pub mod audio_commands;
pub mod backup_commands;
pub mod blocking;
pub mod collection_commands;
pub mod config_commands;
pub mod dedup_commands;
pub mod derive_commands;
/// 目录移动引擎（物理搬运 + 身份重写 + 阶段日志 + 幂等恢复），见 dir_move.rs 模块文档。
pub mod dir_move;
pub mod doc_commands;
pub mod edit_commands;
pub mod enhance_commands;
pub mod exotic_commands;
pub mod export_commands;
pub mod face_commands;
pub mod file_ops_commands;
pub mod hgallery_commands;
pub mod layout_commands;
pub mod log_commands;
pub mod media_commands;
pub mod model_download;
pub mod ocr_commands;
pub mod player_commands;
pub mod proofread_commands;
pub mod registry;
pub mod reveal;
pub mod scan_commands;
pub mod search_commands;
pub mod storage_commands;
pub mod system_commands;
pub mod thumbnail_commands;
/// 全库缩略图生成流水线（thumbnail_commands.rs 拆出，见超长文件拆分方案 tierB-3）；
/// 只经 `thumbnail_commands` 的 `pub use *` 转发对外可见，故本身不 `pub`。
mod thumbnail_full_gen;
pub mod tree_commands;
pub mod video_commands;
pub mod viewer_color_commands;
pub mod volume_commands;
