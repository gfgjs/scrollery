//! IPC 命令注册清单(U-P1-a,2026-07-16 从 lib.rs 下沉)。
//!
//! 此后**新增 IPC 命令只碰本文件 + 对应命令文件**,lib.rs 退出高频冲突面。
//! 清单从 lib.rs `invoke_handler` 宏调用逐行原样搬迁,分组注释保留;
//! 迁移前后已做命令路径集合对拍(U 设计 §8 专项结构门)。

use crate::ipc;

/// 返回 `Builder::invoke_handler` 所需的命令分发闭包。
///
/// `generate_handler!` 展开为无捕获 `move` 闭包(tauri-macros 2.6.3
/// src/command/handler.rs),满足 `Fn(Invoke<R>) -> bool + Send + Sync + 'static`;
/// 命令名取路径末段,搬迁不改变前端可见的命令集合。
///
/// Runtime 收敛为具体 `Wry` 而非泛型 `R`:多个命令签名直接收 `AppHandle`
/// (默认 `AppHandle<Wry>`),泛型形态满足不了它们的 `CommandArg<R>` 约束;
/// 本应用桌面/移动入口均由 `Builder::default()` 构建,Wry 是唯一 runtime。
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // scan
        ipc::scan_commands::add_scan_root,
        ipc::scan_commands::remove_scan_root,
        ipc::scan_commands::remove_scan_root_with_options,
        ipc::scan_commands::set_scan_root_hidden, // V21：设置页根显隐(库级排除)
        ipc::scan_commands::check_folder_overlap,
        ipc::scan_commands::relink_scan_root, // #7 方案A：文件夹迁移后改根路径，免重扫免重生成
        ipc::scan_commands::list_scan_roots,
        ipc::scan_commands::start_scan,
        ipc::scan_commands::stop_scan,
        ipc::scan_commands::clear_database,
        ipc::scan_commands::clear_settings,
        // 精确内容去重（独立后台分析 + keyset 重复组查询）
        ipc::dedup_commands::start_dedup_analysis,
        ipc::dedup_commands::stop_dedup_analysis,
        ipc::dedup_commands::dedup_status,
        ipc::dedup_commands::list_duplicate_groups,
        ipc::dedup_commands::list_duplicate_members,
        ipc::dedup_commands::list_duplicate_group_members,
        ipc::dedup_commands::list_duplicate_folder_candidates,
        ipc::dedup_commands::list_duplicate_folder_roots,
        ipc::dedup_commands::list_duplicate_folder_children,
        ipc::dedup_commands::get_duplicate_folder_summary,
        ipc::dedup_commands::list_duplicate_folder_items,
        ipc::dedup_commands::preview_dedup_folder_cleanup,
        ipc::dedup_commands::apply_dedup_folder_soft_delete,
        ipc::dedup_commands::apply_dedup_soft_delete,
        // layout
        ipc::layout_commands::compute_layout,
        ipc::layout_commands::get_view_ids, // T14.5/T18：按布局序的视图全集 id（Part5 选区前置）
        ipc::layout_commands::get_layout_rows,
        ipc::layout_commands::get_layout_rows_by_y,
        ipc::layout_commands::get_bucket_rows, // T16 方案B:bucket 段精确取行(B0)
        ipc::layout_commands::get_separator_y_by_group_id,
        ipc::layout_commands::get_item_y_by_id,
        ipc::layout_commands::get_subtree_scroll_target,
        // H-Lab 横向画廊实验(独立缓存,与生产布局命令平行)
        ipc::hgallery_commands::compute_h_layout,
        ipc::hgallery_commands::get_h_blocks_by_x,
        // media
        ipc::media_commands::get_media_detail,
        ipc::media_commands::get_meta_for_viewport,
        ipc::media_commands::get_adjacent_media,
        ipc::media_commands::get_lens_adjacent_media,
        ipc::media_commands::get_companion_video_url,
        ipc::media_commands::get_keyframe_sprite,
        // 音频播放器（需求6, §3.6）
        ipc::audio_commands::get_audio_detail,
        // player(播放器线,2026-07-22):字幕加载 + 截帧保存
        ipc::player_commands::load_video_subtitle,
        ipc::player_commands::save_frame_png,
        ipc::media_commands::toggle_favorite,
        ipc::media_commands::batch_toggle_favorite,
        ipc::media_commands::set_rating,
        ipc::media_commands::batch_set_rating,
        ipc::media_commands::set_color_label,
        ipc::media_commands::batch_set_color_label,
        ipc::media_commands::set_view_rotation,
        ipc::media_commands::set_playback_position, // V23:播放器播放位置记忆
        ipc::media_commands::soft_delete_items,
        ipc::media_commands::restore_items,
        ipc::media_commands::resolve_selection, // Part5 S4：选择描述符 → id 列表（按视图布局序）
        ipc::media_commands::count_selection, // Part5 S4：选择描述符精确计数（SelectAll 走 COUNT(*)）
        ipc::media_commands::get_trash,
        ipc::media_commands::get_stats,
        ipc::media_commands::list_registered_formats,
        ipc::media_commands::list_library_formats,
        ipc::media_commands::get_directory_tree,
        ipc::media_commands::get_directory_children,
        ipc::media_commands::get_directory_ancestors,
        ipc::media_commands::list_directory_files,
        ipc::media_commands::prioritize_dimensions,
        // thumbnails
        ipc::thumbnail_commands::batch_request_thumbnails,
        ipc::thumbnail_commands::start_full_thumbnail_generation,
        ipc::thumbnail_commands::start_incremental_thumbnail_generation,
        ipc::thumbnail_commands::stop_full_thumbnail_generation,
        ipc::thumbnail_commands::full_thumb_gen_status,
        ipc::thumbnail_commands::cancel_thumbnail_request,
        ipc::thumbnail_commands::regenerate_missing_thumb,
        ipc::thumbnail_commands::clear_all_thumbnails,
        // 数据备份（方案 B §5/§8）
        ipc::backup_commands::preflight_backup,
        ipc::backup_commands::start_backup,
        ipc::backup_commands::stop_backup,
        ipc::backup_commands::backup_status,
        ipc::backup_commands::list_backups,
        ipc::backup_commands::restore_stage,
        ipc::backup_commands::restore_arm,
        ipc::backup_commands::relaunch_app,
        // 导出整理成果（方案 A §3/§4）
        ipc::export_commands::preflight_export,
        ipc::export_commands::start_export,
        ipc::export_commands::export_status,
        ipc::export_commands::stop_export,
        // 图片简单编辑（方案 C §6）
        ipc::edit_commands::get_editing_entitlement,
        ipc::edit_commands::activate_editing_feature,
        ipc::edit_commands::deactivate_editing_feature,
        ipc::edit_commands::get_edit_preview,
        ipc::edit_commands::save_edited_image,
        // 查看器渲染色域(B 线,方案 §0①④)
        ipc::viewer_color_commands::get_viewer_color_url,
        ipc::viewer_color_commands::import_icc_profile,
        ipc::viewer_color_commands::list_icc_profiles,
        ipc::viewer_color_commands::delete_icc_profile,
        // exotic（冷门格式插件）查询命令（Part1 §2.3）
        ipc::exotic_commands::list_exotic_format_resolutions,
        ipc::exotic_commands::get_exotic_item_state,
        ipc::exotic_commands::list_installed_exotic_plugins,
        ipc::exotic_commands::get_plugin_entitlement,
        // exotic 处理控制命令（Part2 §4.5）
        ipc::exotic_commands::start_exotic_processing,
        ipc::exotic_commands::pause_exotic_processing,
        ipc::exotic_commands::stop_exotic_processing,
        ipc::exotic_commands::get_exotic_processing_status,
        ipc::exotic_commands::list_exotic_task_details,
        ipc::exotic_commands::retry_exotic_task,
        ipc::exotic_commands::retry_exotic_plugin_failures,
        // exotic 激活 / 移除授权命令（Part3 §6.6）
        ipc::exotic_commands::activate_exotic_plugin,
        ipc::exotic_commands::deactivate_exotic_plugin,
        // exotic 安装 / 卸载 / 修复 / 回滚 / Registry 命令（Part3 §6.4-6.6）
        ipc::exotic_commands::fetch_exotic_registry,
        ipc::exotic_commands::list_exotic_registry,
        ipc::exotic_commands::install_exotic_plugin,
        ipc::exotic_commands::repair_exotic_plugin,
        ipc::exotic_commands::rollback_exotic_plugin,
        ipc::exotic_commands::uninstall_exotic_plugin,
        // volume（已知卷面板，T13 离线 UX）
        ipc::volume_commands::list_volumes,
        ipc::volume_commands::rename_volume,
        ipc::volume_commands::forget_volume,
        // search
        ipc::search_commands::search_media,
        // config
        ipc::config_commands::get_app_config,
        ipc::config_commands::get_startup_config,
        ipc::config_commands::set_app_config,
        ipc::config_commands::get_thumb_cache_dir,
        ipc::config_commands::get_log_dir,
        ipc::config_commands::get_cache_stats,
        ipc::config_commands::clear_cache,
        // A2:config.toml 外部编辑支持
        ipc::config_commands::open_config_file,
        ipc::config_commands::get_config_status,
        // 独立日志窗口(日志能力重构 S4,方案 §5/§9.4 S4)
        ipc::log_commands::open_log_window,
        ipc::log_commands::list_log_files,
        ipc::log_commands::read_log_file_page,
        // 日志分析(日志能力重构 S5 P1/P2,方案 §5/§9.4 S5)
        ipc::log_commands::get_log_diagnostics,
        ipc::log_commands::compute_log_histogram,
        ipc::log_commands::export_diagnostics_package,
        // system
        ipc::system_commands::show_in_explorer,
        ipc::system_commands::open_directory,
        ipc::system_commands::frontend_heartbeat,
        ipc::system_commands::clear_logs,
        ipc::system_commands::log_frontend_events,
        // 文件树(S 线):「所有文件」两态的受限 FS 访问
        ipc::tree_commands::list_tree_entries,
        ipc::tree_commands::reveal_tree_entry,
        ipc::tree_commands::get_tree_text_preview,
        ipc::tree_commands::invalidate_tree_cache,
        // AI
        ipc::ai_commands::detect_ai_provider,
        ipc::ai_commands::get_ai_status,
        ipc::ai_commands::semantic_search_cmd,
        ipc::ai_commands::clear_semantic_search,
        ipc::ai_commands::start_ai_analysis,
        ipc::ai_commands::restart_ai_analysis,
        ipc::ai_commands::pause_ai_analysis,
        ipc::ai_commands::stop_ai_analysis,
        ipc::ai_commands::retry_failed_ai_items,
        ipc::ai_commands::rebuild_embeddings,
        ipc::ai_commands::list_ai_models,
        ipc::ai_commands::import_ai_model,
        ipc::ai_commands::reload_ai_engine,
        ipc::ai_commands::list_model_registry,
        ipc::ai_commands::set_active_model,
        ipc::ai_commands::download_model,
        // OCR 文字提取（B′ 路线，2026-07-23，T7）
        ipc::ocr_commands::ocr_status,
        ipc::ocr_commands::ocr_extract_image,
        ipc::ocr_commands::ocr_extract_frame,
        ipc::ocr_commands::download_ocr_models,
        ipc::enhance_commands::enhance_status,
        ipc::enhance_commands::download_enhance_model,
        ipc::enhance_commands::delete_enhance_model,
        ipc::enhance_commands::enhance_preview,
        ipc::enhance_commands::enhance_start,
        ipc::enhance_commands::enhance_cancel,
        ipc::enhance_commands::get_enhance_queue,
        // 人脸识别（F5）
        ipc::face_commands::get_face_status,
        ipc::face_commands::start_face_analysis,
        ipc::face_commands::restart_face_analysis,
        ipc::face_commands::pause_face_analysis,
        ipc::face_commands::stop_face_analysis,
        ipc::face_commands::retry_failed_face_items,
        ipc::face_commands::list_face_persons,
        ipc::face_commands::list_ignored_face_persons,
        ipc::face_commands::get_item_faces,
        ipc::face_commands::rename_face_person,
        ipc::face_commands::set_face_person_hidden,
        ipc::face_commands::set_face_person_ignored,
        ipc::face_commands::merge_face_persons,
        ipc::face_commands::recluster_faces,
        // 批量审批（Part4 T3 / §3.5.1）
        ipc::face_commands::confirm_faces,
        ipc::face_commands::reassign_faces,
        ipc::face_commands::unassign_faces,
        ipc::face_commands::reject_faces,
        ipc::face_commands::create_person,
        ipc::face_commands::list_likely_face_matches,
        ipc::face_commands::list_face_model_registry,
        ipc::face_commands::download_face_model,
        ipc::face_commands::set_active_face_model,
        // 派生流水线（视频封面/关键帧、文档缩略图、音频封面/元数据）
        ipc::derive_commands::start_derivation,
        ipc::derive_commands::pause_derivation,
        ipc::derive_commands::stop_derivation,
        ipc::derive_commands::derivation_status,
        // 视频格式扩展 · 播放链路(V6):resolve/confirm/cancel + 组件下载/状态
        ipc::video_commands::resolve_video_playback,
        ipc::video_commands::confirm_video_playback,
        ipc::video_commands::cancel_video_playback,
        ipc::video_commands::video_playback_progress_snapshot,
        ipc::video_commands::video_component_status,
        ipc::video_commands::download_video_component,
        ipc::video_commands::video_cache_stats,
        // 文档（P4）：文档缩略图前端渲染回环（§3.4）
        ipc::doc_commands::ensure_doc_thumb_queue,
        ipc::doc_commands::list_pending_doc_thumbs,
        ipc::doc_commands::store_doc_thumbnail,
        ipc::doc_commands::get_reading_progress,
        ipc::doc_commands::set_reading_progress,
        // 阅读器 R1:txt 编码检测 + 分章/分段(§6.3)
        ipc::doc_commands::get_text_book_index,
        ipc::doc_commands::get_text_chapter,
        // 阅读器:简繁转换(§5.10/§6.3,ferrous-opencc)
        ipc::doc_commands::convert_chinese,
        // 阅读器:每书阅读偏好(§6.1,R1 承载手动编码覆盖)
        ipc::doc_commands::get_reader_book_prefs,
        ipc::doc_commands::set_reader_book_prefs,
        // 阅读器 R4:书签 CRUD(§6.2)
        ipc::doc_commands::list_reader_bookmarks,
        ipc::doc_commands::add_reader_bookmark,
        ipc::doc_commands::delete_reader_bookmark,
        ipc::doc_commands::list_replacements,
        ipc::doc_commands::get_effective_replacements,
        ipc::doc_commands::upsert_replacement,
        ipc::doc_commands::delete_replacement,
        ipc::doc_commands::list_versions,
        ipc::doc_commands::get_current_version,
        ipc::doc_commands::get_document_text,
        ipc::doc_commands::get_version_content,
        ipc::doc_commands::save_version,
        ipc::doc_commands::set_current_version,
        ipc::doc_commands::delete_version,
        ipc::doc_commands::diff_versions,
        ipc::doc_commands::diff_texts,
        // 文档（P4）：远程 AI 校对（§5.4）
        ipc::proofread_commands::get_proofread_config,
        ipc::proofread_commands::set_proofread_config,
        ipc::proofread_commands::set_proofread_key,
        ipc::proofread_commands::clear_proofread_key,
        ipc::proofread_commands::proofread_chunk,
        // 收藏夹（需求7）
        ipc::collection_commands::list_collections,
        ipc::collection_commands::list_deleted_collections,
        ipc::collection_commands::recent_collections,
        ipc::collection_commands::create_collection,
        ipc::collection_commands::delete_collection,
        ipc::collection_commands::restore_collection,
        ipc::collection_commands::rename_collection,
        ipc::collection_commands::add_to_collection,
        ipc::collection_commands::remove_from_collection,
        // 存储后端（网络盘, 需求8 8B, §3.8）
        ipc::storage_commands::list_backends,
        ipc::storage_commands::add_backend,
        ipc::storage_commands::test_backend,
        ipc::storage_commands::remove_backend,
        ipc::system_commands::exit_app,
        ipc::system_commands::hide_window,
        ipc::system_commands::set_as_wallpaper,
        ipc::system_commands::copy_image_to_clipboard,
        // file ops
        ipc::file_ops_commands::create_physical_folder,
        ipc::file_ops_commands::move_media_items,
        ipc::file_ops_commands::copy_media_items,
        ipc::file_ops_commands::relocate_media_items,
        ipc::file_ops_commands::copy_media_items_db,
        ipc::file_ops_commands::remove_media_items_hard,
        ipc::file_ops_commands::move_directory,
        ipc::file_ops_commands::copy_directory,
        ipc::file_ops_commands::delete_directory_to_trash,
        // 目录移动半完成状态的读取与重试（审查 §7.1-B）
        ipc::file_ops_commands::list_pending_directory_moves,
        ipc::file_ops_commands::retry_directory_move,
    ]
}
