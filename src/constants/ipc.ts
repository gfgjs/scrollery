// IPC 命令和事件名称常量

export const IPC = {
  // ── 扫描 ──────────────────────────────────────────────────────────────
  ADD_SCAN_ROOT: 'add_scan_root',
  REMOVE_SCAN_ROOT: 'remove_scan_root',
  // 移除扫描根并可选清理其缩略图（侧栏文件夹管理用）。
  REMOVE_SCAN_ROOT_WITH_OPTIONS: 'remove_scan_root_with_options',
  // 设置页控制某扫描根的显隐（V21，库级排除：隐藏后其媒体从画廊/时间轴/搜索/统计/树全部消失）。
  SET_SCAN_ROOT_HIDDEN: 'set_scan_root_hidden',
  // 新增扫描根前检测与既有根的路径包含/重叠冲突。
  CHECK_FOLDER_OVERLAP: 'check_folder_overlap',
  // 文件夹整体迁移后（如 D 盘→C 盘）把扫描根改指新路径：抽样校验通过即改根路径 + 重绑卷，
  // 免重扫、免重生成缩略图（cache_key 不含盘符，派生产物原样有效）。
  RELINK_SCAN_ROOT: 'relink_scan_root',
  LIST_SCAN_ROOTS: 'list_scan_roots',
  START_SCAN: 'start_scan',
  STOP_SCAN: 'stop_scan',
  CLEAR_DATABASE: 'clear_database',
  CLEAR_SETTINGS: 'clear_settings',

  // ── 布局 ────────────────────────────────────────────────────────────
  COMPUTE_LAYOUT: 'compute_layout',
  GET_LAYOUT_ROWS_BY_Y: 'get_layout_rows_by_y',
  // T16 方案B:按段取行(半开区间 [startY,endY) 精确归属,区别于 by_y 的视口相交语义)
  GET_BUCKET_ROWS: 'get_bucket_rows',
  GET_ITEM_Y_BY_ID: 'get_item_y_by_id',
  GET_SUBTREE_SCROLL_TARGET: 'get_subtree_scroll_target',
  // 按布局序的视图全集 id（Part5 T4 选区脱离 DOM 的顺序来源；后端 layout_commands.rs:128 已注册）
  GET_VIEW_IDS: 'get_view_ids',

  // ── H-Lab 横向画廊实验(docs/designs/2026-07-02-horizontal-gallery-lab.md)────────
  // 独立缓存/版本,与生产布局命令平行互不可见。
  COMPUTE_H_LAYOUT: 'compute_h_layout',
  GET_H_BLOCKS_BY_X: 'get_h_blocks_by_x',

  // ── 媒体 ─────────────────────────────────────────────────────────────
  GET_MEDIA_DETAIL: 'get_media_detail',
  GET_ADJACENT_MEDIA: 'get_adjacent_media',
  // 镜头查看器按布局版本取一项相邻媒体；不物化完整 flat_ids。
  GET_LENS_ADJACENT_MEDIA: 'get_lens_adjacent_media',
  GET_META_FOR_VIEWPORT: 'get_meta_for_viewport',
  GET_COMPANION_VIDEO_URL: 'get_companion_video_url',
  GET_KEYFRAME_SPRITE: 'get_keyframe_sprite',
  GET_AUDIO_DETAIL: 'get_audio_detail',
  TOGGLE_FAVORITE: 'toggle_favorite',
  // 批量设/取消收藏（选区批量操作用）。
  BATCH_TOGGLE_FAVORITE: 'batch_toggle_favorite',
  SET_RATING: 'set_rating',
  // 批量评分（0-5,选区键盘 1-5 快捷评分用,单条 UPDATE+IN,避免逐项 N 次 IPC）。
  BATCH_SET_RATING: 'batch_set_rating',
  // 颜色标签（0=无 / 1-7 色档,T16）。set 单项,batch 选区批量(单条 UPDATE+IN)。
  SET_COLOR_LABEL: 'set_color_label',
  BATCH_SET_COLOR_LABEL: 'batch_set_color_label',
  // 看图台展示旋转持久化（归一化 0/90/180/270,V20;仅大图查看用,不进网格）。
  SET_VIEW_ROTATION: 'set_view_rotation',
  // 播放器播放位置记忆(ms,V23;镜像 SET_VIEW_ROTATION,不进网格)。
  SET_PLAYBACK_POSITION: 'set_playback_position',
  // 播放器字幕加载(同目录同名探测,或用户指定路径)。
  LOAD_VIDEO_SUBTITLE: 'load_video_subtitle',
  // 播放器截帧保存为 PNG。
  SAVE_FRAME_PNG: 'save_frame_png',
  // ── 查看器渲染色域(自定义 ICC 与色域切换,方案 B §0①)──────────────────────
  // 查看器大图换源色域:None=直显原图(target=srgb/移动端/非派生适用),Some=派生文件绝对路径。
  // target 不由前端传参,后端从 ConfigManager 单源读取(防前后端口径分叉)。
  GET_VIEWER_COLOR_URL: 'get_viewer_color_url',
  // 自定义 ICC 导入/枚举/删除(设置页 ICC 管理列表用)。
  IMPORT_ICC_PROFILE: 'import_icc_profile',
  LIST_ICC_PROFILES: 'list_icc_profiles',
  DELETE_ICC_PROFILE: 'delete_icc_profile',
  SOFT_DELETE_ITEMS: 'soft_delete_items',
  RESTORE_ITEMS: 'restore_items',
  GET_STATS: 'get_stats',
  // ── 格式注册表与 facet（S 线 §6/§7）────────────────────────────────────
  // 已注册格式全集（内置 ∪ exotic Catalog）的 UI 投影；随插件安装变化，故不缓存跨会话。
  LIST_REGISTERED_FORMATS: 'list_registered_formats',
  // 库内**实际存在**的格式（DISTINCT file_format + 基础谓词）。实测 0.0ms → 每次开弹层现查。
  LIST_LIBRARY_FORMATS: 'list_library_formats',
  GET_DIRECTORY_TREE: 'get_directory_tree',
  GET_DIRECTORY_CHILDREN: 'get_directory_children',
  // 由目标项 id 反查其目录祖先链（定位/展开到指定项用）。
  GET_DIRECTORY_ANCESTORS: 'get_directory_ancestors',
  // ── 文件树「所有文件」两态（S 线 §4）：受限 FS 枚举，不经媒体库 ──────────────
  // 「已注册格式」模式**不走**这三个命令，仍是上面的 DB 快路径（零回归）。
  LIST_TREE_ENTRIES: 'list_tree_entries',
  REVEAL_TREE_ENTRY: 'reveal_tree_entry',
  // path-based 只读文本预览(问题②方案 B v1):白名单纯文本,后端 resolve_within_root 校验。
  GET_TREE_TEXT_PREVIEW: 'get_tree_text_preview',
  INVALIDATE_TREE_CACHE: 'invalidate_tree_cache',
  LIST_DIRECTORY_FILES: 'list_directory_files',

  // ── 缩略图 ─────────────────────────────────────────────────────────
  BATCH_REQUEST_THUMBNAILS: 'batch_request_thumbnails',
  START_FULL_THUMBNAIL_GENERATION: 'start_full_thumbnail_generation',
  // 增量生成:只补 thumb_status=0(缺失/被复位)的项,不做全表重置;与全量共用停止命令与进度通道。
  START_INCREMENTAL_THUMBNAIL_GENERATION: 'start_incremental_thumbnail_generation',
  STOP_FULL_THUMBNAIL_GENERATION: 'stop_full_thumbnail_generation',
  /** 缩略图生成状态快照(webview 刷新后恢复进度用;进度流走 EVENTS.THUMB_GEN_PROGRESS)。 */
  FULL_THUMB_GEN_STATUS: 'full_thumb_gen_status',
  // 「在途项保留后端单飞——同项大概率重回视口」的设计(useRequestQueue.cancel)不调用它。
  // 优先测量给定项的真实尺寸（滚动到锚点前抢先算其 y，避免跳动）。
  PRIORITIZE_DIMENSIONS: 'prioritize_dimensions',
  // 清空全部缩略图缓存。
  CLEAR_ALL_THUMBNAILS: 'clear_all_thumbnails',
  // 懒自愈：某项 thumb_status=1 但封面文件已被 LRU 驱逐（404）→ 复位待重生成。
  REGENERATE_MISSING_THUMB: 'regenerate_missing_thumb',

  // ── 派生流水线（P2/P3/P4，§3.2/§3.3/§3.6：视频封面/关键帧、音频封面、epub 封面） ────
  START_DERIVATION: 'start_derivation',
  STOP_DERIVATION: 'stop_derivation',
  DERIVATION_STATUS: 'derivation_status',

  // ── 视频格式扩展 · 播放链路（V6，§5）────────────────────────────────────
  /** 解析视频播放路径 → { mode: direct|derived|preparing|needsComponent|needsHevcExt|needsConfirm } */
  RESOLVE_VIDEO_PLAYBACK: 'resolve_video_playback',
  /** 单文件超池 50% 护栏放行后复调（confirmed=true 跳过护栏）。 */
  CONFIRM_VIDEO_PLAYBACK: 'confirm_video_playback',
  /** 取消在途/待产出的播放准备（硬取消：kill 在途/出队，行退回 pending，允许下次重触发）。 */
  CANCEL_VIDEO_PLAYBACK: 'cancel_video_playback',
  /** 播放准备进度快照（webview 刷新后恢复进度；无在途 job 回 null）。进度流走 VIDEO_PLAYBACK_PROGRESS_EVENT。 */
  VIDEO_PLAYBACK_PROGRESS_SNAPSHOT: 'video_playback_progress_snapshot',
  /** 触发下载+安装 FFmpeg 视频扩展组件（幂等）。 */
  DOWNLOAD_VIDEO_COMPONENT: 'download_video_component',
  /** 视频可播产物独立池实时占用统计 → { bytes, files, limitMb }（design §5.4，V7 补遗）。 */
  VIDEO_CACHE_STATS: 'video_cache_stats',

  // ── 收藏夹（需求7） ─────────────────────────────────────────────────────
  LIST_COLLECTIONS: 'list_collections',
  /** 软删收藏夹的读路径（收藏夹页「已删除」捞回入口）；恢复仍走 RESTORE_COLLECTION。 */
  LIST_DELETED_COLLECTIONS: 'list_deleted_collections',
  RECENT_COLLECTIONS: 'recent_collections',
  CREATE_COLLECTION: 'create_collection',
  DELETE_COLLECTION: 'delete_collection',
  RESTORE_COLLECTION: 'restore_collection',
  RENAME_COLLECTION: 'rename_collection',
  ADD_TO_COLLECTION: 'add_to_collection',
  REMOVE_FROM_COLLECTION: 'remove_from_collection',

  // ── 文档（P4, §3.4/§3.5） ─────────────────────────────────────────────
  ENSURE_DOC_THUMB_QUEUE: 'ensure_doc_thumb_queue',
  LIST_PENDING_DOC_THUMBS: 'list_pending_doc_thumbs',
  STORE_DOC_THUMBNAIL: 'store_doc_thumbnail',
  GET_READING_PROGRESS: 'get_reading_progress',
  SET_READING_PROGRESS: 'set_reading_progress',
  // 阅读器 R1(§6.3):txt 编码检测 + 分章(索引)/ 分段(单章内容)。字节偏移不出 Rust。
  GET_TEXT_BOOK_INDEX: 'get_text_book_index',
  GET_TEXT_CHAPTER: 'get_text_chapter',
  // 阅读器(§5.10):简繁转换(ferrous-opencc)。前端在替换规则之后按章批量调用。
  CONVERT_CHINESE: 'convert_chinese',
  // 阅读器(§6.1):每书阅读偏好(R1 承载手动编码覆盖 prefs.encoding,R3 扩充竖排/主题/字号)。
  GET_READER_BOOK_PREFS: 'get_reader_book_prefs',
  SET_READER_BOOK_PREFS: 'set_reader_book_prefs',
  // 阅读器 R4(§6.2):书签 CRUD。locator 现用 foliate CFI（cfi:...），与阅读进度同源。
  LIST_READER_BOOKMARKS: 'list_reader_bookmarks',
  ADD_READER_BOOKMARK: 'add_reader_bookmark',
  DELETE_READER_BOOKMARK: 'delete_reader_bookmark',
  LIST_REPLACEMENTS: 'list_replacements',
  GET_EFFECTIVE_REPLACEMENTS: 'get_effective_replacements',
  UPSERT_REPLACEMENT: 'upsert_replacement',
  DELETE_REPLACEMENT: 'delete_replacement',
  LIST_VERSIONS: 'list_versions',
  GET_CURRENT_VERSION: 'get_current_version',
  GET_DOCUMENT_TEXT: 'get_document_text',
  SAVE_VERSION: 'save_version',
  SET_CURRENT_VERSION: 'set_current_version',
  DELETE_VERSION: 'delete_version',
  DIFF_VERSIONS: 'diff_versions',
  DIFF_TEXTS: 'diff_texts',
  GET_PROOFREAD_CONFIG: 'get_proofread_config',
  SET_PROOFREAD_CONFIG: 'set_proofread_config',
  SET_PROOFREAD_KEY: 'set_proofread_key',
  CLEAR_PROOFREAD_KEY: 'clear_proofread_key',
  PROOFREAD_CHUNK: 'proofread_chunk',

  // ── 存储后端（网络盘, 需求8 8B, §3.8） ─────────────────────────────────
  LIST_BACKENDS: 'list_backends',
  ADD_BACKEND: 'add_backend',
  TEST_BACKEND: 'test_backend',
  REMOVE_BACKEND: 'remove_backend',


  // ── 配置 ────────────────────────────────────────────────────────────
  GET_APP_CONFIG: 'get_app_config',
  SET_APP_CONFIG: 'set_app_config',
  // ── 中央设置(设置集中保存,2026-09-16)──────────────────────────────────
  // 全部已注册设置的生效值 + 本进程 revision / generation,供运行时刷新与重置后同步。
  GET_SETTINGS_SNAPSHOT: 'get_settings_snapshot',
  // 一次性批量提交具名键值 patch:写盘一次,返回最新快照、变化键、需重启键与应用失败键。
  // 必须携带 generation:旧代次(重置前)的写入被拒绝,防其他窗口迟到回写。
  SET_APP_SETTINGS: 'set_app_settings',
  // 退出前 flush 回报:后端发 SETTINGS_FLUSH_REQUESTED,前端落盘完成后经此回执。
  // ok=false 时后端保留窗口不退出,由前端给出重试/放弃选择。
  SETTINGS_FLUSH_DONE: 'settings_flush_done',
  // 启动时一次性取设置快照与内部状态(首启/引导标记),合并为 1 次往返。
  GET_STARTUP_CONFIG: 'get_startup_config',
  // 应用日志目录路径（设置页"打开日志目录"用）。
  GET_LOG_DIR: 'get_log_dir',
  // ── 外置配置文件（config.toml，批次B）───────────────────────────────
  // 用系统默认编辑器打开外置配置文件；保存后由后端热应用并广播 CONFIG_FILE_CHANGED。
  OPEN_CONFIG_FILE: 'open_config_file',
  // 查询外置配置文件路径 / 是否存在 / 最近一次解析错误。
  GET_CONFIG_STATUS: 'get_config_status',

  // ── Logs window（日志能力重构 S4，方案 §5/§9.4 S4）───────────────────────
  // ── 独立日志窗口 ─────────────────────────────────────────────────────
  // 打开（或聚焦既有）独立日志窗口；建窗即在后端翻转环形缓冲订阅标志。
  OPEN_LOG_WINDOW: 'open_log_window',
  // 列出日志目录下全部 .log 文件（按修改时间降序），供历史分页的日期选择用。
  LIST_LOG_FILES: 'list_log_files',
  // 读取单个日志文件的一页（从文件末尾按 offsetFromEnd/limit 切片）。
  READ_LOG_FILE_PAGE: 'read_log_file_page',

  // ── Logs analysis（日志能力重构 S5 P1/P2，方案 §5/§9.4 S5）───────────────
  // ── 日志分析 ──────────────────────────────────────────────────────────
  // 日志系统自身可观测性快照：non_blocking 丢弃行数 + 当前正在压缩中的错误签名列表。
  GET_LOG_DIAGNOSTICS: 'get_log_diagnostics',
  // 对某个历史日志文件按小时/天分桶，返回各桶各级别的计数（rusqlite 临时表 + GROUP BY strftime）。
  COMPUTE_LOG_HISTOGRAM: 'compute_log_histogram',
  // 导出诊断包（system-info + 最新日志尾部，脱敏后打 zip，仅落盘不自动上传）。
  EXPORT_DIAGNOSTICS_PACKAGE: 'export_diagnostics_package',

  // ── 文件操作 ──────────────────────────────────────────────────────────
  RELOCATE_MEDIA_ITEMS: 'relocate_media_items',
  COPY_MEDIA_ITEMS_DB: 'copy_media_items_db',
  REMOVE_MEDIA_ITEMS_HARD: 'remove_media_items_hard',
  MOVE_DIRECTORY: 'move_directory',
  COPY_DIRECTORY: 'copy_directory',
  DELETE_DIRECTORY_TO_TRASH: 'delete_directory_to_trash',
  // 目录移动的半完成收尾（P0-1）：只读列出未完成项（读清单本身不触发任何物理动作）。
  LIST_PENDING_DIRECTORY_MOVES: 'list_pending_directory_moves',
  // 按阶段日志 id 重试一条收尾（幂等，允许重做物理搬运与整树校验）。
  RETRY_DIRECTORY_MOVE: 'retry_directory_move',
  // 在磁盘上新建物理文件夹（侧栏新建文件夹对话框用）。
  CREATE_PHYSICAL_FOLDER: 'create_physical_folder',

  // ── 系统 ────────────────────────────────────────────────────────────
  SHOW_IN_EXPLORER: 'show_in_explorer',
  // 用系统文件管理器打开指定目录。
  OPEN_DIRECTORY: 'open_directory',
  // 关闭启动闪屏并显示主窗口（前端 onMounted 就绪后调用）。
  // 隐藏主窗口（关闭行为=最小化到托盘时）。
  HIDE_WINDOW: 'hide_window',
  // 退出应用进程。
  EXIT_APP: 'exit_app',
  // 前端存活心跳：Rust 关闭拦截器用它判断 WebView 是否仍能消费关闭事件。
  FRONTEND_HEARTBEAT: 'frontend_heartbeat',
  // 清空应用日志文件。
  CLEAR_LOGS: 'clear_logs',
  // 把指定图片复制到系统剪贴板。
  COPY_IMAGE_TO_CLIPBOARD: 'copy_image_to_clipboard',
  // 把指定图片设为桌面壁纸。
  SET_AS_WALLPAPER: 'set_as_wallpaper',
  // 前端日志桥(日志能力重构 S3,方案 §4/§9.4):批量上报前端日志事件,汇入后端 tracing。
  LOG_FRONTEND_EVENTS: 'log_frontend_events',

  // ── AI ────────────────────────────────────────────────────────────────
  DETECT_AI_PROVIDER: 'detect_ai_provider',
  GET_AI_STATUS: 'get_ai_status',
  SEMANTIC_SEARCH_CMD: 'semantic_search_cmd',
  // 清空语义搜索结果集与在途请求(P1-3 两段式:入口吊销 + 工作段擦库)。
  CLEAR_SEMANTIC_SEARCH: 'clear_semantic_search',
  START_AI_ANALYSIS: 'start_ai_analysis',
  STOP_AI_ANALYSIS: 'stop_ai_analysis',
  PAUSE_AI_ANALYSIS: 'pause_ai_analysis',
  RESTART_AI_ANALYSIS: 'restart_ai_analysis',
  RETRY_FAILED_AI_ITEMS: 'retry_failed_ai_items',
  REBUILD_EMBEDDINGS: 'rebuild_embeddings',
  RELOAD_AI_ENGINE: 'reload_ai_engine',
  LIST_MODEL_REGISTRY: 'list_model_registry',
  SET_ACTIVE_MODEL: 'set_active_model',
  DOWNLOAD_MODEL: 'download_model',

  // ── OCR 文字提取（B′ 路线，2026-07-23）───────────────────────────────────
  // 门控 + 安装态查询（前端 useOcr 判门、设置页 OCR 分节共用）。
  OCR_STATUS: 'ocr_status',
  // 画廊图片 OCR 提取（按 item_id）。
  OCR_EXTRACT_IMAGE: 'ocr_extract_image',
  // 视频帧 OCR 提取（前端 canvas 截帧回传 base64 PNG）。
  OCR_EXTRACT_FRAME: 'ocr_extract_frame',
  // 下载 OCR 档位模型（mobile/server 二选）。
  DOWNLOAD_OCR_MODELS: 'download_ocr_models',

  // ── 影像增强（降噪/超分子系统，2026-07-24，P0 批 5）────────────────────────
  // 门控 + 模型态查询（设置页「影像增强」分节 / EnhanceDialog 判门共用）。
  ENHANCE_STATUS: 'enhance_status',
  // 下载某增强模型档位（复用 download_assets 进度事件；下载免费，不过授权门）。
  DOWNLOAD_ENHANCE_MODEL: 'download_enhance_model',
  // 删除某增强模型档位的两文件（fp32 + fp16）。
  DELETE_ENHANCE_MODEL: 'delete_enhance_model',
  // 前后对比预览（P0 后端返 enhance_not_implemented 占位，实现归 4.5 批）。
  ENHANCE_PREVIEW: 'enhance_preview',
  // 发起增强（多选入队，顺序执行），返回 job_id。
  ENHANCE_START: 'enhance_start',
  // 取消某 job。
  ENHANCE_CANCEL: 'enhance_cancel',
  // 队列全量快照（事件后拉取）。
  GET_ENHANCE_QUEUE: 'get_enhance_queue',

  // ── Face（人脸分析 / 人物管理）─────────────────────────────────────────
  // ── 人脸 ─────────────────────────────────────────────────────────────
  START_FACE_ANALYSIS: 'start_face_analysis',
  STOP_FACE_ANALYSIS: 'stop_face_analysis',
  PAUSE_FACE_ANALYSIS: 'pause_face_analysis',
  RESTART_FACE_ANALYSIS: 'restart_face_analysis',
  RETRY_FAILED_FACE_ITEMS: 'retry_failed_face_items',
  GET_FACE_STATUS: 'get_face_status',
  GET_ITEM_FACES: 'get_item_faces',
  LIST_FACE_PERSONS: 'list_face_persons',
  LIST_IGNORED_FACE_PERSONS: 'list_ignored_face_persons',
  RENAME_FACE_PERSON: 'rename_face_person',
  MERGE_FACE_PERSONS: 'merge_face_persons',
  SET_FACE_PERSON_HIDDEN: 'set_face_person_hidden',
  SET_FACE_PERSON_IGNORED: 'set_face_person_ignored',
  RECLUSTER_FACES: 'recluster_faces',
  LIST_FACE_MODEL_REGISTRY: 'list_face_model_registry',
  DOWNLOAD_FACE_MODEL: 'download_face_model',
  // 批量审批（T10）：likely-match 分组 + 整组确认/改派/移出/拒绝/建新人物。
  LIST_LIKELY_FACE_MATCHES: 'list_likely_face_matches',
  CONFIRM_FACES: 'confirm_faces',
  REASSIGN_FACES: 'reassign_faces',
  UNASSIGN_FACES: 'unassign_faces',
  REJECT_FACES: 'reject_faces',
  CREATE_PERSON: 'create_person',

  // ── Exotic 插件平台（Part5 T11/T12，消费 Part6）────────────────────────
  // 某插件的授权判定（gate / 购买引导用）；判定全在后端 EntitlementProvider，前端不持验签逻辑。
  GET_OFFICIAL_ENTITLEMENT: 'get_official_entitlement',
  LIST_FEATURE_OFFERINGS: 'list_feature_offerings',
  ACTIVATE_OFFICIAL_LICENSE: 'activate_official_license',
  DEACTIVATE_OFFICIAL_LICENSE: 'deactivate_official_license',
  GET_PLUGIN_ENTITLEMENT: 'get_plugin_entitlement',
  // 单个媒体项的 exotic 状态（可用态 + 任务态）；resolution=null 即普通格式，触点据此决定是否 gate。
  GET_EXOTIC_ITEM_STATE: 'get_exotic_item_state',
  // Catalog 全部格式解析（前端据此缓存"哪些格式属 exotic"，避免为普通格式空跑 item-state IPC）。
  LIST_EXOTIC_FORMAT_RESOLUTIONS: 'list_exotic_format_resolutions',
  // ── 插件商店（T11）：registry 浏览 / 安装生命周期 / 处理进度 ──────────────
  // 拉取远程签名 Registry（验签+防回滚+原子写缓存）；返回本次可装条目摘要。
  FETCH_EXOTIC_REGISTRY: 'fetch_exotic_registry',
  // 列出本地缓存的可安装条目（无缓存→空列表）。
  LIST_EXOTIC_REGISTRY: 'list_exotic_registry',
  // 列出已安装插件（安装真相投影）。
  LIST_INSTALLED_EXOTIC_PLUGINS: 'list_installed_exotic_plugins',
  // 安装 / 卸载 / 修复 / 回滚（参数只接受已校验 pluginId，绝不接受 URL/路径/hash——后端红线）。
  INSTALL_EXOTIC_PLUGIN: 'install_exotic_plugin',
  UNINSTALL_EXOTIC_PLUGIN: 'uninstall_exotic_plugin',
  REPAIR_EXOTIC_PLUGIN: 'repair_exotic_plugin',
  ROLLBACK_EXOTIC_PLUGIN: 'rollback_exotic_plugin',
  // 处理进度摘要 + 控制（恢复/暂停/停止本次运行/重试某插件全部失败）。
  GET_EXOTIC_PROCESSING_STATUS: 'get_exotic_processing_status',
  /** 处理详情列表（进度区「展开详情」：文件级任务 + 状态/错误码，桶筛选 + 分页）。 */
  LIST_EXOTIC_TASK_DETAILS: 'list_exotic_task_details',
  START_EXOTIC_PROCESSING: 'start_exotic_processing',
  PAUSE_EXOTIC_PROCESSING: 'pause_exotic_processing',
  STOP_EXOTIC_PROCESSING: 'stop_exotic_processing',
  RETRY_EXOTIC_PLUGIN_FAILURES: 'retry_exotic_plugin_failures',

  // ── Volume（已知卷面板，T13 离线 UX）──────────────────────────────────
  // 列出已知卷（含在线态 + 媒体数）/ 重命名 / 忘记（FK SET NULL，媒体保留仅解绑）。
  LIST_VOLUMES: 'list_volumes',
  RENAME_VOLUME: 'rename_volume',
  FORGET_VOLUME: 'forget_volume',

  // ── Thumbnail cache（缩略图缓存目录）────────────────
  // 注:SET_WINDOW_THEME 已随自绘标题栏(顶栏重构 L1)废弃——Windows DWM 染色链移除,
  // 窗口明暗同步改前端 getCurrentWindow().setTheme()(uiStore.applyAppearance)。
  GET_THUMB_CACHE_DIR: 'get_thumb_cache_dir',
  /** 缓存占用统计(分类目字节+文件数;设置页缩略图卡统计行,点击刷新)。 */
  GET_CACHE_STATS: 'get_cache_stats',

  // ── Export（导出整理成果，方案 A §3/§4）────────────────────────────────
  /** 预检:选区规模/估算体积/离线缺失数/非零旋转数/目的地可写性/是否命中库内。 */
  PREFLIGHT_EXPORT: 'preflight_export',
  /** 启动导出,立即返回 jobId;前端订阅 EVENTS.EXPORT_PROGRESS + 轮询/恢复用 EXPORT_STATUS。 */
  START_EXPORT: 'start_export',
  /** 导出状态查询(webview 重载后经此恢复快照,同 thumb/backup 的 app 事件 + 快照姿态)。 */
  EXPORT_STATUS: 'export_status',
  /** 取消导出;jobId 须匹配当前运行任务,不匹配静默忽略(过期引用保护)。 */
  STOP_EXPORT: 'stop_export',

  // ── Backup / Restore（数据备份与恢复，方案 B §5/§8）────────────────────
  /** 备份预检:目的地可写性、同卷警告、文档一致性与估算体积。 */
  PREFLIGHT_BACKUP: 'preflight_backup',
  /** 启动一次手动完整备份；实际任务与当前 webview 生命周期分离。 */
  START_BACKUP: 'start_backup',
  /** 备份状态快照(webview 重载后恢复,与 BACKUP_PROGRESS 事件配套)。 */
  BACKUP_STATUS: 'backup_status',
  /** 列出目的目录内 manifest 可验证的备份包。 */
  LIST_BACKUPS: 'list_backups',
  /** 取消当前备份；已正式落名的旧包不受影响。 */
  STOP_BACKUP: 'stop_backup',
  /** 校验并暂存恢复包，不改动活库。 */
  RESTORE_STAGE: 'restore_stage',
  /** 生成恢复前回滚包并写入重启交换 marker。 */
  RESTORE_ARM: 'restore_arm',
  /** 恢复 arm 成功后重启应用，由启动期状态机完成交换。 */
  RELAUNCH_APP: 'relaunch_app',

  // ── 图片简单编辑（方案 C §6/§7）────────────────────────────────
  /** 获取 orientation 烤入、sRGB、长边受限的 raw 编辑预览 packet。 */
  GET_EDIT_PREVIEW: 'get_edit_preview',
  /** 保存编辑副本:旋转/翻转/裁剪 + 编码落盘 + 单文件入库,直接返回终态(非 job/事件模型)。 */
  SAVE_EDITED_IMAGE: 'save_edited_image',

  // ── 精确内容去重（P2-P4）────────────────────────────────────────────
  START_DEDUP_ANALYSIS: 'start_dedup_analysis',
  STOP_DEDUP_ANALYSIS: 'stop_dedup_analysis',
  DEDUP_STATUS: 'dedup_status',
} as const

// ── Tauri 事件 ──────────────────────────────────────────────────────────
export const EVENTS = {
  OFFICIAL_LICENSE_CHANGED: 'official-license-changed',
  // 退出前 flush 请求，载荷 { requestId }；前端落盘后经 IPC.SETTINGS_FLUSH_DONE 回执。
  SETTINGS_FLUSH_REQUESTED: 'settings-flush-requested',
  MEDIA_ENRICHED: 'db:media_enriched',
  ENRICHMENT_COMPLETED: 'enrichment:completed',
  /** 卷插拔监听（Part2 T2）：卷在线态变化 → 画廊刷新离线徽标显隐。 */
  VOLUMES_CHANGED: 'volumes:changed',
  /** exotic 处理进度/状态变化（后端 Pipeline 每批进度 + 租约清扫时发）→ 商店进度区实时刷新。 */
  EXOTIC_STATUS_CHANGED: 'exotic:status-changed',
  /** 缩略图全量/增量生成进度(app 级广播,替代 Channel——Channel 随发起它的 webview 一起死,
      刷新页面即丢进度;事件对重建后的 webview 照常送达)。 */
  THUMB_GEN_PROGRESS: 'thumb:gen_progress',
  /** 视频播放准备进度/状态（V6，§5.3）：payload { itemId, status: preparing|ready|error|cancelled, src?, code? }。
      app 级广播 + resolve 命令快照恢复（缩略图进度先例：Channel 随 webview 死，事件对重建 webview 照送）。 */
  VIDEO_PLAYBACK_PROGRESS: 'video:playback_progress',
  /** 导出进度(方案 A §3.1,同上,app 级广播 + export_status 快照恢复)。 */
  EXPORT_PROGRESS: 'export:progress',
  /** 备份进度(方案 B §5.1,app 级广播 + backup_status 快照恢复)。 */
  BACKUP_PROGRESS: 'backup:progress',
  /** 日志窗口实时流批推(日志能力重构 S4,方案 §9.3-D):UI 环形缓冲层 ~100ms 一批,app 级广播——
      只有日志窗口会监听,其余窗口忽略无成本。 */
  LOG_BATCH: 'log:batch',
  /** 外置配置文件被外部编辑器改动并热应用成功后广播（批次B）；payload 含变更键名,
      前端不按 keys 做选择性刷新——复用既有加载路径整体重取（见 useConfigFile.ts）。 */
  CONFIG_FILE_CHANGED: 'config-file-changed',
  /** 外置配置文件保存但解析失败时广播（批次B）；后端保持旧值生效,前端仅更新错误展示。 */
  CONFIG_FILE_ERROR: 'config-file-error',
  /** 影像增强队列/进度变化（enhance_start 驱动，每 item 完成 + 终态时发）→ 前端拉全量队列刷新。 */
  ENHANCE_QUEUE_CHANGED: 'enhance:queue-changed',
  /** 精确去重分析进度：app 级广播，刷新后由 dedup_status 恢复快照。 */
  DEDUP_PROGRESS: 'dedup:progress',
} as const
