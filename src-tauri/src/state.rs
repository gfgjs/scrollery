// src-tauri/src/state.rs
//! 在所有 Tauri 命令之间共享的应用程序状态。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockWriteGuard};
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::ai::search_control::SearchControl;
use crate::db::{DbPool, DbWriter};
use crate::engine::EngineArena;
use crate::exotic::CatalogStore;
use crate::layout::cache::new_layout_cache;
use crate::layout::items_cache::{
    new_global_rank_cell, new_items_cache, GlobalRankCell, ItemsCache,
};
use crate::layout::LayoutCache;
use crate::thumbnail::generator::ThumbConfig;

// 去重 manager 只保存控制状态；真正的 DB 适配器在 start_with_state 时按本次 AppState
// 创建，避免形成自引用 Arc。
pub use crate::dedup::task::DedupTaskManager;

/// 全局应用程序状态。
pub struct AppState {
    /// 写入连接 — 通过 Mutex 序列化。
    pub db_writer: DbWriter,

    /// 读取连接池（WAL 并发读取）。
    pub db_read_pool: DbPool,

    /// A2:61 个设置类键的唯一真源门面(`<app_data_dir>/config.toml`)。IPC `config_commands`
    /// 与 watcher 回调(见 `config::watcher::spawn_config_watcher`)共享同一实例;`Arc` 包裹是
    /// 因为 watcher 的 tokio task 需要独立持有一份存活到应用退出,而不与 `AppState` 本体绑生死。
    pub config: Arc<crate::config::ConfigManager>,

    /// 用于扫描操作的每个根目录的取消令牌。
    pub scan_tokens: Mutex<HashMap<i64, CancellationToken>>,

    /// 前端 stop 请求携带的运行身份。root_id 只能定位一个根，不能阻止旧 stop 在新轮
    /// 安装后误取消新轮；run_id 与 `start_scan` 的一次调用一一对应。
    scan_run_ids: Mutex<HashMap<i64, String>>,

    /// 精确重复分析任务控制器。分析摘要写回使用独立 sidecar，不改变媒体位置项语义。
    pub dedup_task: Arc<DedupTaskManager>,

    /// 文件夹优先视图的只读聚合快照；键由分析代次、数据代次、哈希版本和筛选器组成。
    pub dedup_folder_stats_cache: crate::dedup::folder_cache::DedupFolderStatsCache,

    /// 去重分析/清理与全库清空之间的生命周期闸门。
    ///
    /// 分析和物理清理持读锁；`clear_database` 持写锁并先让当前分析失效，再删除媒体
    /// 与 journal，避免文件动作或迟到 sidecar 写入穿过全库清空边界。
    dedup_lifecycle_gate: RwLock<()>,

    /// 扫描令牌的按根运行代次。`scan_tokens` 保留为公开的兼容视图，实际收尾身份由这里的
    /// [`RunTokenSlot`] 判定；所有同时访问两张表的操作都先锁 `scan_tokens` 再锁本表。
    scan_token_slots: Mutex<HashMap<i64, RunTokenSlot>>,

    /// 同一扫描根的 DB 写入线性化闸门。
    ///
    /// 扫描任务的 generation 检查必须和后续 writer lock 绑定，否则新轮可能在检查之后
    /// 安装、旧轮再把批次/收尾写进去。`replace_scan_run_token` 与扫描写事务共用此闸门：
    /// 旧写入先拿闸门则在线性化点前完成，新轮先拿闸门则旧写入在闸门内被拒绝。
    scan_write_gates: Mutex<HashMap<i64, Arc<Mutex<()>>>>,

    /// 扫描生命周期管理闸门。
    ///
    /// 普通扫描/根管理操作持读锁，清空数据库持写锁。这样 `clear_database` 在取消并
    /// 等待现有根级写区后，直到数据库与内存缓存清空完成前都不会有新扫描或根变更插入。
    /// 低频生命周期操作才需要此闸门；扫描批次仍以根级 gate 做线性化。
    scan_lifecycle_gate: RwLock<()>,

    /// 数据库生命周期代次。清空/删除根/重链接进入生命周期写区时递增；后台 worker
    /// 在最终文件或 DB 写入处带上捕获的代次，过时代次只能丢弃结果。
    database_lifecycle_epoch: AtomicU64,

    /// 数据库生命周期是否接受新的根管理/扫描/派生写入。清空或其它生命周期写操作
    /// 在等待写锁前先置为 false，使已经排队但尚未取得读锁的入口在执行点快速失败。
    database_lifecycle_active: AtomicBool,

    /// 串行化生命周期写区的 active 状态切换与写锁持有期。多个写者可以同时排队在
    /// `scan_lifecycle_gate` 上，但不能各自保存 `previous_active=false` 后再错误地把
    /// active 恢复为 false。
    database_lifecycle_transition_gate: Mutex<()>,

    /// 内存中的两端对齐布局缓存。
    pub layout_cache: LayoutCache,

    /// S1 视图取数缓存（compute_layout 的百万级 SQL 段跳过器，Part2 重排提速 2026-07-04）。
    /// 命中键 = filter JSON + data_version + 序形态；详见 layout/items_cache.rs 模块文档。
    pub layout_items_cache: ItemsCache,

    /// B-file-iii：全局 **filter-invariant** filename 自然序 rank（全库一份，开机后台建一次）。
    /// filename 序是与 filter 无关的全序 → 一份服务所有筛选子集；把 B-file-i「每 filter 付一次
    /// 546ms NATURAL_CMP 查询」降为「全库一次」。消费见 [`Self::try_global_filename_ranks`]，
    /// 后台(重)建见 [`Self::spawn_global_filename_rank_build`]。详见 items_cache::GlobalFilenameRank。
    pub global_filename_rank: GlobalRankCell,
    /// 全局 rank 后台构建的「在途」标志（幂等去重：防 data_version 抖动期并发重复构建）。
    global_rank_building: std::sync::atomic::AtomicBool,

    /// 全局数据版本（S1 失效契约）：任何改变画廊视图**成员/几何/顺序**的写路径必须 bump
    /// （扫描批提交、enricher 尺寸回写、软删/恢复、文件操作、相册成员、人物指派、卷可用态
    /// 等，见 bump_data_version）；纯展示写（缩略图结果、favorite/rating/color 的非敏感
    /// 视图）走双缓存 patch 不 bump。漏 bump 的代价 = 下次重排沿用旧视图集合——宁可多
    /// bump（bump 只是让下次重排回退为全量重查，即 S1 之前的常态行为）。
    pub data_version: AtomicU64,

    /// 去重镜头视图代次（主画廊重复项浏览方案 §12.2）：镜头只消费「最近一次完整完成的
    /// 去重结果版本」。唯一发布点 = 精确去重任务状态机把一轮分析真正翻转为 `Completed`
    /// 的收尾路径（`DedupTaskManager::complete_current` 的 compare-and-clear 成功分支经
    /// `DedupStore::on_run_completed` 钩子触达）；取消/失败/stop 不发布半成品，旧代次
    /// 继续有效。分析分批写入期间不 bump——不驱动主画廊逐批重排，这是它与 `data_version`
    /// 平行存在的理由。MVP 为内存版本、重启回到 1：重启本身清空全部内存布局缓存，代次
    /// 无需跨会话保真（方案 §12.2 明确允许，需要时再落 metadata）。
    pub dedup_view_epoch: AtomicU64,

    /// H-Lab 横向画廊实验布局缓存——与 layout_cache 平行且互不可见(实验解耦契约)。
    pub h_layout_cache: crate::layout::HLayoutCache,

    /// 图像引擎容器（格式 → 引擎分发）。
    pub engine_arena: EngineArena,

    /// 冷门格式能力目录（内置 + 远程合并的只读快照）。扫描分类与缩略图路由的「能力真相」。
    /// 持 `Arc<CatalogStore>`，热路径经 `.snapshot()` 取 `Arc<CatalogSnapshot>`（一次读锁）。
    pub exotic_catalog: std::sync::Arc<CatalogStore>,

    /// 文件树「所有文件」模式的目录快照缓存（S 线 §4.2）。支撑稳定分页：首页快照一次、
    /// 后续页按下标切片，避免 10 万项目录每页重跑 `read_dir + sort`（平方级），也避免翻页
    /// 途中目录增删导致的重复/漏项。有界（8 个快照 / 20 万条目双维度封顶）。
    pub tree_snapshots: std::sync::Arc<crate::tree::cache::DirSnapshotCache>,

    /// 缩略图配置（缓存目录、大小、跳过阈值）。
    pub thumb_config: RwLock<ThumbConfig>,

    /// (2026-07-18 审查 F-02):旧轮收尾曾无条件清槽+发终态,「停止→立即重启」时会清掉新轮
    /// 刚安装的句柄并把新轮快照盖成 cancelled。语义同 `ai_analysis_token` 的 compare-and-clear。
    /// 全量缩略图生成任务的取消令牌槽(带运行代次)。
    pub thumb_gen_token: RunTokenSlot,

    /// 缩略图生成的启动、取消、终态发布与进度快照线性化闸门。它只保护短的内存/状态
    /// 操作，不包住解码、编码或文件 IO；锁序约定为先此闸门，再数据库生命周期读锁。
    thumb_gen_lifecycle_gate: Mutex<()>,

    /// 全量/增量缩略图生成的最近进度快照。进度传输为 app 级事件(`thumb:gen_progress`)——
    /// webview 刷新后经 `full_thumb_gen_status` 查此快照恢复显示(Channel 随发起它的 webview 一起死)。
    pub thumb_gen_progress: Mutex<Option<crate::ipc::thumbnail_commands::FullThumbProgressPayload>>,

    /// 多档缩略图源服务的设备像素比(2026-08-16 阶段 2,千分比 ×1000):compute_layout 时由
    /// 前端上报,出口拼装(hydrate_rows)读取做按需选档。几何不依赖 DPR,故 DPR 变化不触发
    /// 重排——仅影响后续取行批的服务档位;同尺寸跨屏 DPR 突变的边缘场景在下次 compute 收敛。
    pub thumb_serve_dpr: AtomicU32,

    /// 已解析出的日志目录路径。
    pub log_dir: PathBuf,

    /// UI 环形缓冲层的共享状态(日志能力重构 S4,方案 §9.3-D):独立日志窗口的实时流数据源。
    /// 在 tracing subscriber 建好时于 lib.rs 创建(早于 `AppState::new`),故经构造参数传入而非
    /// 在 `new()` 内部现建(与 `log_dir`/`app_data_dir` 等「外部先算好再传入」的既有字段同姿态)。
    pub log_ring: std::sync::Arc<crate::logging::LogRingBuffer>,

    /// non_blocking(lossy)后台写线程的丢弃行计数器(日志能力重构 S5 P1,方案 §3.2/§5「丢弃计数
    /// 可见化」):`tracing_appender::non_blocking::ErrorCounter` 内部即 `Arc<AtomicUsize>`,`Clone`
    /// 廉价,与 `log_ring` 同姿态在 lib.rs 建 subscriber 时一并捕获、构造参数传入。
    pub log_dropped_counter: tracing_appender::non_blocking::ErrorCounter,

    /// 应用数据根目录（Tauri `app_data_dir()` 真值）。models/documents 等子目录一律从此派生;
    /// **禁止**用 `log_dir.parent()` 反推——log_dir 是用户可配置项(设置页可改),改日志目录后
    /// 反推值随之漂移,曾导致已下载模型被判「未安装」(2026-07-10 审查 A1)。
    pub app_data_dir: PathBuf,

    /// 冷门格式插件数据根（`<app_data>/exotic`）。子目录：plugins(已装)/staging(解包)/registry(签名缓存)。
    /// 安装/卸载/修复/list_registry 命令据此定位（Part3 §6.4）。
    pub exotic_dir: PathBuf,

    /// 安装/卸载/回滚命令互斥锁（安全评审 medium）：这些命令做 quiesce + 目录原子切换，并发执行会
    /// 在 backup 清理/rename 之间产生破损窗口(无 current)。串行化保证同一时刻只一个目录变更操作。
    pub exotic_install_lock: tokio::sync::Mutex<()>,

    /// AI worker 子进程句柄(Part4-T17「AiEnginePool→worker 句柄」;`ai_backend=worker`
    /// 才实际 spawn)。std Mutex 按调用粒度持锁——worker 严格串行,批与批之间可插入
    /// 搜索请求;访问 into_inner 毒锁恢复(AI 命令族契约,同 exotic token)。
    pub ai_worker: Mutex<crate::ai::worker_client::AiWorkerClient>,

    /// 影像增强 host 服务（降噪/超分子系统 design.md §A）：直持 enhance-worker + 内存 job 队列，
    /// 由 IPC `enhance_start` 驱动。**不进** exotic 任务化调度（D-OCR-7 同型豁免）。
    pub enhance_service: std::sync::Arc<crate::enhance::EnhanceService>,

    /// 语义搜索控制面（P1-3）：常驻半精度嵌入快照的装载/失效、请求代次与结果集提交的
    /// 线性化。嵌入向量写入或重置时经 [`Self::invalidate_embedding_cache`] 作废快照；
    /// 清空搜索/切模型经其 `clear`/`model_switched` 一并吊销在途请求。
    pub ai_search: SearchControl,

    /// 后台 AI 分析流水线的取消令牌槽(带运行代次)。代次让已结束轮次的完成回调做 compare-and-clear
    /// (2026-07-10 审查 F10):旧轮迟到的完成回调不得 take 新一轮刚安装的 token——那会静默
    /// 中断刚重启的运行(「暂停→立即开始」「连点重启」可复现)。CancellationToken 无身份
    /// 比较能力,故以代次号定身份。2026-07-18 F-025 迁入 `RunTokenSlot` 统一范式;注意
    /// ai/face 的终态副作用(释放 GPU 槽/清 active 标志)门控在 `!token.is_cancelled()` 上,
    /// **不**采用 thumb/derive 的「finish 返回值门控终态发布」姿态——两者终态语义不同。
    pub ai_analysis_token: RunTokenSlot,

    /// 后台人脸识别流水线（F3）的取消令牌槽。与 `ai_analysis_token` 一一对应；存在即代表正在运行。
    pub face_analysis_token: RunTokenSlot,

    /// CLIP 与人脸共用的唯一 GPU 分析槽的单一持有者门闩（F5）。两者都吃满 GPU/显存，同一时刻
    /// 只能跑一条。`None`=空闲；`Some("ai")`/`Some("face")`=该流水线持有槽位。check-and-claim 在
    /// 这一把锁下原子完成（`try_acquire_gpu_analysis`）——这正是「两个独立 token 交叉检查」不够
    /// 的原因：两次启动各查各的 token mutex，可能都见 `None` 都启动（TOCTOU）；App.vue 启动时
    /// 不 await 地连发 `ai`+`face` 两个自动续传就是这个 race。上面的 per-pipeline 取消令牌用于
    /// 停止/暂停；本门闩是另一回事（互斥）。
    pub gpu_analysis_owner: Mutex<Option<&'static str>>,

    /// 后台派生流水线（视频封面/关键帧、文档缩略图、音频封面与元数据）的取消令牌槽(带运行代次)。
    /// 存在即代表正在运行，与 AI 令牌同构。
    /// 带运行代次的原因(2026-07-18 审查 F-01):旧轮收尾曾无条件 take+cancel,会误杀
    /// 「停止→立即重启」时新轮刚安装的 token,使刚重启的提取静默停止。
    pub derivation_token: RunTokenSlot,

    /// 后台冷门格式处理流水线（Part2）的取消令牌（R1）。存在即运行。AI/人脸经 `ai_yield_blockers`
    /// 让步给它；exotic 自身经 `should_yield_exotic` 让步给扫描/缩略图/交互（**不**让步 derivation
    /// ——二者同级、共享公平后台重活池，R4）。
    /// 访问器一律 `into_inner` 毒锁恢复(R2-6,与 exotic 子系统契约一致,见 ai/pipeline.rs
    /// 问题6):coordinator 循环须在 Pipeline panic 后存活,运行态判定不得因毒锁级联 panic。
    pub exotic_analysis_token: Mutex<Option<CancellationToken>>,

    /// 由 derivation 与 exotic 两条流水线共享的公平后台重活池（R4）：两个子系统在重活前都从**本**
    /// limiter 取 permit，故持续派生不会饿死 exotic（FIFO 公平，
    /// 等待有上界）。预算 = `available_parallelism()`：exotic 空闲时 derivation 不受影响；二者并发时
    /// 共享同一全局预算、按到达顺序公平交错。
    pub background_heavy_limiter: std::sync::Arc<crate::exotic::limiter::BackgroundHeavyLimiter>,

    /// GPU 推理令牌(Part4 D2/T11):全局额度 1 的**物理并发**闸——AI/face worker 池发
    /// 推理批前 acquire(顺序天条:先 CPU permit 后 GPU 令牌,见 `GpuToken` 文档)。与上面
    /// `gpu_analysis_owner`(**会话语义**门闩,分钟级)分层不合并;acquire 接线随 T13/T15
    /// 批派发落地,在此先建实例保证全部 GPU 消费者共享同一令牌(D2 §3.3 两形态一致)。
    pub gpu_token: std::sync::Arc<crate::exotic::limiter::GpuToken>,

    /// exotic Coordinator 句柄（setup 内创建后写入；扫描/命令经此 wake 调度）。用 `OnceLock` 因为
    /// Coordinator 需 `AppHandle`（setup 才有），晚于 AppState 构造；一次写入、多处只读。
    /// exotic 调度器句柄（晚绑定）。
    pub exotic_coordinator:
        std::sync::OnceLock<std::sync::Arc<crate::exotic::coordinator::ExoticCoordinator>>,

    /// 视频格式扩展 Service（视频格式扩展子系统 design.md §2.1）。同 `exotic_coordinator` 晚绑定
    /// （需 `Arc<AppState>` 构造真实 runner，晚于 AppState 本体）；一次写入、多处只读。消费面
    /// （backend_for 桥 / IPC）随 V5/V6 接入，本批仅注册装配点。
    pub video_worker_service:
        std::sync::OnceLock<std::sync::Arc<crate::video::worker_service::VideoWorkerService>>,

    /// 「用户正在主动交互」（重排/滚动）的截止时刻（unix 毫秒）。在其到期前，后台派生与 AI 节流，
    /// 使前台 `compute_layout` 不被重型视频解码饿死。由布局 IPC 更新、让步检查读取。0 = 从未交互。
    pub interactive_until_ms: AtomicI64,

    /// 文档存储一致性门（方案 B §3.1）：`document_versions` 表与 `appdata/documents/**` 文件是
    /// **跨 DB+文件的两处状态**，二者一致才算完整。凡同时改这两处的 blocking 闭包（`save_version` /
    /// `delete_version`）持 **read** guard；数据备份从 DB 快照到 documents 打包完毕持 **write** guard，
    /// 使备份看到的行↔文件关系来自同一逻辑时点（不会捕获插了行但文件未落、或删了行文件残留的瞬态）。
    ///
    /// `RwLock<()>`（零大小 unit）:文档写彼此不互斥（本就靠 `db_writer` Mutex 串行），仅与备份互斥。
    /// **只在 `spawn_blocking` 闭包内持有、绝不跨 `.await`**（项目硬约束:std 锁 guard 不过 await 点）。
    /// 与 `db_writer` 的锁序恒为「先 guard 后 db_writer」——读侧写侧一致,无反转。
    pub document_storage_guard: RwLock<()>,

    /// A/B 共用的文件任务单一持有者门闩(方案 B §5.2 / 方案 A):备份 / 恢复 / 导出这类重文件
    /// 任务同一时刻只允许一个。`None`=空闲;`Some("backup")`/`Some("restore")`/`Some("export")`=
    /// 该任务持有。占用时新任务返回 `file_job_busy`,**不取消旧任务**(方案 §5.2:不静默抢占)。
    /// 语义同 `gpu_analysis_owner`;check-and-claim 在单锁区原子完成。
    pub file_job_owner: Mutex<Option<&'static str>>,

    /// 数据备份任务的取消令牌槽(带运行代次,方案 B §5.1)。文件任务采 thumb/derive 的「finish
    /// 返回值门控终态发布」姿态(**非** ai/face 的 !is_cancelled 姿态,审阅线 F-013)。存在即运行。
    pub backup_token: RunTokenSlot,

    /// 最近一次备份的进度/状态快照。进度传输 = app 级事件(`backup:progress`);webview 重载后经
    /// `backup_status` 查此快照恢复显示(Channel 随发起它的 webview 一起死,方案 B §5.1)。
    pub backup_progress: Mutex<Option<crate::ipc::backup_commands::BackupProgressPayload>>,

    /// 「备份取消中」标志(审查 #13):`cancel_backup` 立即 take 空 `backup_token`(is_running→false),
    /// 但运行中的 blocking 任务尚在粗粒度取消点之间收尾。此窗口内 `backup_status` 若仅凭
    /// 「!is_running 且快照仍 running」会误报 `failed/backup_io`。以此标志区分「取消中」——真终态
    /// (cancelled)由收尾任务发布后清除。仅一次原子读写,极廉价。
    backup_cancelling: AtomicBool,

    /// 导出任务的取消令牌槽(带运行代次,方案 A §3.1)。与 `backup_token` 同姿态:文件任务采
    /// thumb/derive 的「finish 返回值门控终态发布」,非 ai/face 的 !is_cancelled 姿态。
    pub export_token: RunTokenSlot,

    /// 最近一次导出的进度/状态快照(方案 A §3.1)。导出是分块任务(逐项复制),快照含已处理/
    /// 总数以支持进度条,非备份那种单次 running→终态的粗粒度。传输 = app 级事件
    /// (`export:progress`);webview 重载后经 `export_status` 查此快照恢复(同 backup_progress
    /// 的理由:Channel 随发起它的 webview 一起死)。
    pub export_progress: Mutex<Option<crate::ipc::export_commands::ExportProgressPayload>>,

    /// 「导出取消中」标志,镜像 `backup_cancelling`:`cancel_export` 立即 take 空 `export_token`
    /// (is_running→false),运行中的 blocking 任务尚在逐项取消点之间收尾,此窗口内
    /// `export_status` 据此标志区分「取消中」与「异常终止」,不误报 `failed/export_io`。
    export_cancelling: AtomicBool,

    /// 查看器渲染色域(B 线,2026-07-23,方案 §0①)的并发去重锁表:键 = `(item_id, target_id)`,
    /// 值 = 该 key 专属的 tokio Mutex。std Mutex 只护 map 本身、取出 Arc 即释放(不跨 await);
    /// 取出的 Arc 由调用方在其上 `.lock().await` 跨越 `spawn_blocking` 渲染整段(见
    /// [`Self::viewer_render_lock`] 文档)。
    pub viewer_render_locks: Mutex<ViewerRenderLockMap>,
}

/// 查看器色域派生的 keyed 去重锁表(键 = `(item_id, target_id)`,值 = 该 key 专属 tokio Mutex)。
/// 抽 `type` 别名以避让 clippy::type_complexity(嵌套三层泛型)。
type ViewerRenderLockMap = HashMap<(i64, String), Arc<tokio::sync::Mutex<()>>>;

/// 一次交互布局操作后继续节流后台解码的时长（毫秒）。持续滚动会不断刷新该窗口；用户停手后后台恢复全速。
const INTERACTIVE_WINDOW_MS: i64 = 1500;

/// GPU 分析门闩持有者标签（见 `gpu_analysis_owner`）。用常量而非字面量，避免拼写错误悄悄破坏
/// CLIP↔人脸互斥。
pub const GPU_OWNER_AI: &str = "ai";
pub const GPU_OWNER_FACE: &str = "face";

/// A/B 共用文件任务门闩持有者标签(见 `file_job_owner`)。用常量而非字面量,拼写错不会悄悄
/// 破坏互斥/漏释放。
pub const FILE_JOB_BACKUP: &str = "backup";
pub const FILE_JOB_RESTORE: &str = "restore";
pub const FILE_JOB_EXPORT: &str = "export";
/// 图片简单编辑保存(方案 C §5/§6,与 error.rs `AppError::Edit` 文档「A/B/C 共用文件任务
/// 门闩」一致):编辑保存是前台交互式单发任务(非 job/进度事件模型),与备份/恢复/导出
/// 共用同一严格 claim-if-free 门闩——同一时刻只允许一类重文件任务在跑。
pub const FILE_JOB_EDIT: &str = "edit";
/// 目录移动/复制/恢复收尾（P0-1）。与备份/恢复/导出/编辑同属重文件任务：同一时刻只允许一个。
/// 移动自身的正确性另有源/目标扫描根闸门保护（见 `with_scan_roots_exclusive`），本门闩负责的是
/// 「别和别的重文件任务同时动磁盘」这一层。
pub const FILE_JOB_DIR_MOVE: &str = "dir_move";

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

mod run_token;

pub use run_token::RunTokenSlot;

/// 按 root_id 升序去重取一组根级写闸门句柄。
///
/// 固定顺序是跨根文件操作（目录移动/复制）不死锁的单一事实源：两次方向相反的跨根移动若各自
/// 按传入顺序取锁，就会各持一把互等。升序 + 去重后，双方拿锁顺序恒一致（同根只取一次）。
fn ordered_scan_gate_handles(
    gates: &mut HashMap<i64, Arc<Mutex<()>>>,
    root_ids: &[i64],
) -> Vec<Arc<Mutex<()>>> {
    let mut ids: Vec<i64> = root_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids.iter()
        .map(|id| {
            gates
                .entry(*id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        })
        .collect()
}

/// 这些根上是否已有扫描轮次（scan_tokens 运行态视图）。
fn any_root_running(tokens: &HashMap<i64, CancellationToken>, root_ids: &[i64]) -> bool {
    root_ids.iter().any(|id| tokens.contains_key(id))
}

/// 在同一组锁已持有时安装一个新的扫描轮次。
fn begin_scan_run(
    tokens: &mut HashMap<i64, CancellationToken>,
    slots: &mut HashMap<i64, RunTokenSlot>,
    root_id: i64,
) -> (u64, CancellationToken) {
    let (generation, token) = slots.entry(root_id).or_default().begin();
    tokens.insert(root_id, token.clone());
    (generation, token)
}

/// 在同一组锁已持有时取消旧轮并安装新轮，避免 stop/restart 间留下可被旧收尾插入的空窗。
fn replace_scan_run(
    tokens: &mut HashMap<i64, CancellationToken>,
    slots: &mut HashMap<i64, RunTokenSlot>,
    root_id: i64,
) -> (u64, CancellationToken) {
    if let Some(slot) = slots.get(&root_id) {
        slot.cancel();
    }
    if let Some(token) = tokens.remove(&root_id) {
        token.cancel();
    }
    begin_scan_run(tokens, slots, root_id)
}

/// 在同一组锁已持有时，以 generation compare-and-clear 取出本轮令牌。
///
/// `scan_tokens` 是旧调用方使用的运行态视图；它为空时，即使 `RunTokenSlot` 的通用语义
/// 允许「槽空的取消收尾」返回 true，扫描收尾也不再拥有发布权。这样 stop 后旧轮迟到的
/// 回调不会在 restart 前后发布旧终态，也不会清理后来安装的句柄。
fn take_scan_run_if_owned(
    tokens: &mut HashMap<i64, CancellationToken>,
    slots: &mut HashMap<i64, RunTokenSlot>,
    root_id: i64,
    generation: u64,
) -> Option<CancellationToken> {
    if !tokens.contains_key(&root_id) {
        return None;
    }
    let slot = slots.get(&root_id)?;
    if !slot.finish(generation) {
        return None;
    }
    tokens.remove(&root_id)
}

/// 在同一根目录的写闸门内完成「当前代次」检查与写入闭包。
///
/// 闸门本身由调用方和安装新轮共用；因此 `is_current` 返回 true 后，直到 `write` 返回
/// 之前不会有新轮完成安装。这个小原语故意不持有 DB 锁，调用方可在闭包内按既有顺序
/// 获取 `db_writer`，且不会把数据库依赖引入全局状态层。
fn with_scan_generation_gate<T, IsCurrent, Write>(
    gate: &Mutex<()>,
    is_current: IsCurrent,
    write: Write,
) -> Option<T>
where
    IsCurrent: FnOnce() -> bool,
    Write: FnOnce() -> T,
{
    let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
    if !is_current() {
        return None;
    }
    Some(write())
}

/// 生命周期写区的 RAII 守卫。`active=false` 必须在等待写锁前发布，避免新的读者在清空
/// 或删根动作排队期间继续进入；离开写区时恢复之前的状态，即使闭包返回错误也不会把
/// 后续数据库操作永久卡死。
struct DatabaseLifecycleWriteGuard<'a> {
    active: &'a AtomicBool,
    _guard: RwLockWriteGuard<'a, ()>,
    _transition_gate: MutexGuard<'a, ()>,
    previous_active: bool,
}

impl Drop for DatabaseLifecycleWriteGuard<'_> {
    fn drop(&mut self) {
        self.active.store(self.previous_active, Ordering::Release);
    }
}

/// 串行化生命周期写者的完整过渡：先锁状态转换闸门，再发布 inactive，等待写锁，最后
/// 递增 epoch。转换闸门必须贯穿整个写区生命周期，否则并发写者会把前一写者恢复的
/// `active=true` 再覆盖成自己保存的 `false`。
fn enter_database_lifecycle_write<'a>(
    transition_gate: &'a Mutex<()>,
    lifecycle_gate: &'a RwLock<()>,
    active: &'a AtomicBool,
    epoch: &'a AtomicU64,
) -> DatabaseLifecycleWriteGuard<'a> {
    let transition_guard = transition_gate.lock().unwrap_or_else(|e| e.into_inner());
    let previous_active = active.swap(false, Ordering::AcqRel);
    let guard = lifecycle_gate.write().unwrap_or_else(|e| e.into_inner());
    epoch.fetch_add(1, Ordering::AcqRel);
    DatabaseLifecycleWriteGuard {
        active,
        _guard: guard,
        _transition_gate: transition_guard,
        previous_active,
    }
}

#[cfg(test)]
mod scan_run_tests {
    use super::*;

    #[test]
    fn replace_and_finish_are_generation_safe() {
        let mut tokens = HashMap::new();
        let mut slots = HashMap::new();
        let (generation1, token1) = begin_scan_run(&mut tokens, &mut slots, 7);

        let (generation2, token2) = replace_scan_run(&mut tokens, &mut slots, 7);
        assert!(generation2 > generation1);
        assert!(token1.is_cancelled(), "替换新轮必须取消旧 token");
        assert!(!token2.is_cancelled());

        assert!(
            take_scan_run_if_owned(&mut tokens, &mut slots, 7, generation1).is_none(),
            "旧轮 finish 不得清理新轮"
        );
        assert!(tokens.contains_key(&7));
        assert!(
            take_scan_run_if_owned(&mut tokens, &mut slots, 7, generation2).is_some(),
            "新轮自己的 finish 应清理本轮"
        );
        assert!(tokens.is_empty());
    }

    #[test]
    fn stopped_scan_without_restart_cannot_be_claimed_by_late_finish() {
        let mut tokens = HashMap::new();
        let mut slots = HashMap::new();
        let (generation, token) = begin_scan_run(&mut tokens, &mut slots, 7);

        slots.get(&7).unwrap().cancel();
        tokens.remove(&7).unwrap().cancel();

        assert!(token.is_cancelled());
        assert!(
            take_scan_run_if_owned(&mut tokens, &mut slots, 7, generation).is_none(),
            "stop 后槽已无扫描句柄，迟到收尾不得再发布终态"
        );
    }

    /// 旧轮若已进入写区，新轮安装必须在线性化闸门外等待，不能插入到旧写入中间。
    #[test]
    fn generation_gate_orders_old_write_before_new_install() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{mpsc, Arc};

        let gate = Arc::new(Mutex::new(()));
        let current_generation = Arc::new(AtomicU64::new(1));
        let write_started = Arc::new(AtomicBool::new(false));
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (new_installed_tx, new_installed_rx) = mpsc::channel();
        let gate_for_old = Arc::clone(&gate);
        let generation_for_old = Arc::clone(&current_generation);
        let started_for_old = Arc::clone(&write_started);

        let old = std::thread::spawn(move || {
            with_scan_generation_gate(
                &gate_for_old,
                || generation_for_old.load(Ordering::Acquire) == 1,
                || {
                    claimed_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    started_for_old.store(true, Ordering::Release);
                },
            )
            .expect("旧轮在新轮安装前应取得写权限");
        });

        claimed_rx.recv().unwrap();
        let gate_for_new = Arc::clone(&gate);
        let generation_for_new = Arc::clone(&current_generation);
        let new = std::thread::spawn(move || {
            let _gate = gate_for_new.lock().unwrap_or_else(|e| e.into_inner());
            generation_for_new.store(2, Ordering::Release);
            new_installed_tx.send(()).unwrap();
        });

        assert!(
            new_installed_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err(),
            "旧轮写入尚未返回时，新轮不得完成安装"
        );
        release_tx.send(()).unwrap();
        old.join().unwrap();
        new.join().unwrap();
        assert!(write_started.load(Ordering::Acquire));
        assert_eq!(current_generation.load(Ordering::Acquire), 2);
    }

    /// 新轮已在线性化闸门内安装后，旧轮即使迟到也只能被拒绝，不能执行写闭包。
    #[test]
    fn stale_generation_is_rejected_before_write() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let gate = Mutex::new(());
        let current_generation = AtomicU64::new(2);
        let mut writes = 0;
        let result = with_scan_generation_gate(
            &gate,
            || current_generation.load(Ordering::Acquire) == 1,
            || {
                writes += 1;
            },
        );

        assert!(result.is_none(), "旧 generation 不得进入写闭包");
        assert_eq!(writes, 0);
    }

    /// 两个生命周期写者并发排队时，后一个写者不能带着 `previous_active=false` 离开，
    /// 否则首个写者恢复 active 后会再次被错误覆盖，令整个数据库生命周期永久失效。
    #[test]
    fn concurrent_lifecycle_writers_restore_active_after_both_finish() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let transition_gate = Arc::new(Mutex::new(()));
        let lifecycle_gate = Arc::new(RwLock::new(()));
        let active = Arc::new(AtomicBool::new(true));
        let epoch = Arc::new(AtomicU64::new(1));
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (release_second_tx, release_second_rx) = mpsc::channel();

        let first = {
            let transition_gate = Arc::clone(&transition_gate);
            let lifecycle_gate = Arc::clone(&lifecycle_gate);
            let active = Arc::clone(&active);
            let epoch = Arc::clone(&epoch);
            std::thread::spawn(move || {
                let guard = enter_database_lifecycle_write(
                    &transition_gate,
                    &lifecycle_gate,
                    &active,
                    &epoch,
                );
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
                drop(guard);
            })
        };
        let second = {
            let transition_gate = Arc::clone(&transition_gate);
            let lifecycle_gate = Arc::clone(&lifecycle_gate);
            let active = Arc::clone(&active);
            let epoch = Arc::clone(&epoch);
            std::thread::spawn(move || {
                let guard = enter_database_lifecycle_write(
                    &transition_gate,
                    &lifecycle_gate,
                    &active,
                    &epoch,
                );
                second_entered_tx.send(()).unwrap();
                release_second_rx.recv().unwrap();
                drop(guard);
            })
        };

        first_entered_rx.recv().unwrap();
        assert!(!active.load(Ordering::Acquire));
        assert!(
            second_entered_rx
                .recv_timeout(Duration::from_millis(20))
                .is_err(),
            "第二个写者必须在 transition gate 外等待"
        );

        release_first_tx.send(()).unwrap();
        second_entered_rx.recv().unwrap();
        assert!(!active.load(Ordering::Acquire));
        release_second_tx.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();
        assert!(active.load(Ordering::Acquire));
        assert_eq!(epoch.load(Ordering::Acquire), 3);
    }
}

#[allow(clippy::items_after_test_module)]
impl AppState {
    // 应用全局状态聚合构造，各依赖独立必需、无合理分组，沿用本仓库既有约定标注。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db_writer: DbWriter,
        db_read_pool: DbPool,
        config: Arc<crate::config::ConfigManager>,
        cache_dir: PathBuf,
        log_dir: PathBuf,
        log_ring: std::sync::Arc<crate::logging::LogRingBuffer>,
        log_dropped_counter: tracing_appender::non_blocking::ErrorCounter,
        app_data_dir: PathBuf,
        exotic_dir: PathBuf,
        thumb_size: u32,
        thumb_skip_max_kb: u64,
        thumb_strategy: String,
        gpu_engine: String,
        ai_hq_cache: bool,
        thumb_webp_quality: u8,
        exotic_catalog: std::sync::Arc<CatalogStore>,
    ) -> Self {
        // 批次C:ai_cache_short_edge(advanced 键)——config 已在此之前构造好(lib.rs 启动序),
        // 直接经其读一次即可,无需新增构造参数(与其余 4 键的 thumb_* 传参式不同,那些是
        // A2 既有惯例;本键是本批新增,就地读更省一次参数改动)。
        let ai_cache_short_edge: u32 = config
            .get("ai_cache_short_edge")
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::thumbnail::cache::AI_CACHE_SHORT_EDGE);
        Self {
            db_writer,
            db_read_pool,
            config,
            scan_tokens: Mutex::new(HashMap::new()),
            scan_run_ids: Mutex::new(HashMap::new()),
            dedup_task: Arc::new(DedupTaskManager::new()),
            dedup_folder_stats_cache: crate::dedup::folder_cache::DedupFolderStatsCache::new(),
            dedup_lifecycle_gate: RwLock::new(()),
            scan_token_slots: Mutex::new(HashMap::new()),
            scan_write_gates: Mutex::new(HashMap::new()),
            scan_lifecycle_gate: RwLock::new(()),
            database_lifecycle_epoch: AtomicU64::new(1),
            database_lifecycle_active: AtomicBool::new(true),
            database_lifecycle_transition_gate: Mutex::new(()),
            layout_cache: new_layout_cache(),
            layout_items_cache: new_items_cache(),
            tree_snapshots: std::sync::Arc::new(crate::tree::cache::DirSnapshotCache::new()),
            global_filename_rank: new_global_rank_cell(),
            global_rank_building: std::sync::atomic::AtomicBool::new(false),
            data_version: AtomicU64::new(1),
            dedup_view_epoch: AtomicU64::new(1),
            h_layout_cache: crate::layout::hcache::new_h_layout_cache(),
            engine_arena: EngineArena::phase1(),
            exotic_catalog,
            thumb_config: RwLock::new(ThumbConfig {
                cache_dir,
                size: thumb_size,
                skip_max_bytes: thumb_skip_max_kb * 1024,
                strategy: thumb_strategy,
                gpu_engine,
                ai_hq_cache,
                webp_quality: thumb_webp_quality,
                ai_cache_short_edge,
            }),
            thumb_gen_token: RunTokenSlot::new(),
            thumb_gen_lifecycle_gate: Mutex::new(()),
            thumb_gen_progress: Mutex::new(None),
            thumb_serve_dpr: AtomicU32::new(1000),
            log_dir,
            log_ring,
            log_dropped_counter,
            app_data_dir,
            exotic_dir,
            exotic_install_lock: tokio::sync::Mutex::new(()),
            ai_worker: Mutex::new(crate::ai::worker_client::AiWorkerClient::new()),
            enhance_service: std::sync::Arc::new(crate::enhance::EnhanceService::new()),
            ai_search: SearchControl::new(),
            ai_analysis_token: RunTokenSlot::new(),
            face_analysis_token: RunTokenSlot::new(),
            gpu_analysis_owner: Mutex::new(None),
            derivation_token: RunTokenSlot::new(),
            exotic_analysis_token: Mutex::new(None),
            // 后台重活并发预算 = 可用并行度，**下限 2**（Part3 §3.5.2 / T12）。
            // 取 max(2)：单核/受限容器（available_parallelism()==1）下，若预算=1 则派生 dispatch 线程
            // 取走唯一额度后阻塞、rayon worker 空等且 exotic 完全饿死；保底 2 让派生与 exotic 至少能交错。
            // derivation 与 exotic 共享此池（R4）。
            background_heavy_limiter: crate::exotic::limiter::BackgroundHeavyLimiter::new(
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4)
                    .max(2),
            ),
            // GPU 推理令牌额度恒 1(D2 §3.1;多 permit 放行留 T22 按 VRAM 档位实测)。
            gpu_token: crate::exotic::limiter::GpuToken::new(),
            exotic_coordinator: std::sync::OnceLock::new(),
            video_worker_service: std::sync::OnceLock::new(),
            interactive_until_ms: AtomicI64::new(0),
            document_storage_guard: RwLock::new(()),
            file_job_owner: Mutex::new(None),
            backup_token: RunTokenSlot::new(),
            backup_progress: Mutex::new(None),
            backup_cancelling: AtomicBool::new(false),
            export_token: RunTokenSlot::new(),
            export_progress: Mutex::new(None),
            export_cancelling: AtomicBool::new(false),
            viewer_render_locks: Mutex::new(HashMap::new()),
        }
    }

    /// 标记用户刚进行了一次交互布局操作（重排/滚动），使后台派生/AI 在短窗口内退让。极廉价（一次原子写）。
    pub fn note_interaction(&self) {
        self.interactive_until_ms
            .store(now_millis() + INTERACTIVE_WINDOW_MS, Ordering::Relaxed);
    }

    /// 用户是否正在主动交互（处于节流窗口内）。
    pub fn is_interactive(&self) -> bool {
        now_millis() < self.interactive_until_ms.load(Ordering::Relaxed)
    }

    /// bump 全局数据版本（S1 失效契约，调用清单见 `data_version` 字段文档）。
    pub fn bump_data_version(&self) {
        self.data_version.fetch_add(1, Ordering::Release);
    }

    /// 读全局数据版本（compute_layout 的填充/命中判定用）。
    pub fn data_version(&self) -> u64 {
        self.data_version.load(Ordering::Acquire)
    }

    /// 读去重镜头视图代次（重复镜头布局缓存的失效键之一，方案 §12.3）。
    pub fn dedup_view_epoch(&self) -> u64 {
        self.dedup_view_epoch.load(Ordering::Acquire)
    }

    /// 发布新去重镜头视图代次。唯一调用点在去重任务「完整成功完成」的收尾路径；
    /// ordering 惯例与 `data_version` 一致（写 Release / 读 Acquire）。
    pub fn bump_dedup_view_epoch(&self) {
        self.dedup_view_epoch.fetch_add(1, Ordering::Release);
    }

    /// B-file-iii：从全局 filename rank 为给定 items 构建平行 `rank` 数组（Stage 3 消费）。
    /// **三关全过才返 Some**：① 全局 rank 已就绪；② `data_version` 相符（非 stale）；③ **所有** item.id
    /// 都在 map 内（基集覆盖——非默认基集如回收站的项不在其中）。任一不满足返 `None` → 调用方退化
    /// B-file-i（§3.5，慢一次但绝不以坏序命中）。一次读锁内完成校验 + 填充（O(N) 哈希查找），
    /// 不 clone 整张 map。
    pub fn try_global_filename_ranks(
        &self,
        items: &[crate::db::models::LayoutItem],
        data_version: u64,
    ) -> Option<Vec<u32>> {
        // 校验+填充逻辑在 GlobalFilenameRank::ranks_for（纯、可单测）；此处只管取读锁委托。
        self.global_filename_rank
            .read()
            .unwrap()
            .as_ref()?
            .ranks_for(items, data_version)
    }

    /// B-file-iii：后台(重)建全局 filename rank——缺失或 stale（`data_version` 不符）且当前无构建
    /// 在途时触发。幂等：`global_rank_building` 标志防 data_version 抖动期并发重复构建。构建下沉
    /// `spawn_blocking`（全库 NATURAL_CMP 查询 ~546ms，不占 tokio worker）；仅当构建期间 dv 未再变
    /// 才写入（否则结果已 stale，丢弃、下次触发重建）。开机预建 + Stage 3 惰性触发（filename 视图见
    /// rank 未就绪时）调用；**不**挂 `bump_data_version`（扫描每批 bump，挂上会反复重建）。
    pub fn spawn_global_filename_rank_build(self: &Arc<Self>) {
        let dv = self.data_version();
        // 已就绪且非 stale → 免。
        if self
            .global_filename_rank
            .read()
            .unwrap()
            .as_ref()
            .is_some_and(|r| r.data_version == dv)
        {
            return;
        }
        // 已有构建在途 → 免（幂等）。swap 原子占位。
        if self.global_rank_building.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let build_dv = this.data_version();
            let this2 = Arc::clone(&this);
            let res = tokio::task::spawn_blocking(move || -> crate::error::Result<_> {
                let conn = this2
                    .db_read_pool
                    .get()
                    .map_err(crate::error::AppError::from)?;
                crate::layout::items_cache::build_global_filename_rank(&conn, build_dv)
            })
            .await;
            match res {
                Ok(Ok(rank)) => {
                    // 仅当构建期间 dv 未再变才写入（否则又 stale，留待下次触发重建）。写入即便偶被
                    // 抢先 bump，消费方 try_global_filename_ranks 的 dv 校验也会拒之（不坏序）。
                    if this.data_version() == build_dv {
                        *this.global_filename_rank.write().unwrap() = Some(rank);
                        tracing::info!(
                            "全局 filename rank 就绪(dv={build_dv}) | global filename rank ready"
                        );
                    } else {
                        tracing::debug!(
                            "全局 filename rank 构建完成即 stale(构建期 dv 变更),丢弃待重建"
                        );
                    }
                }
                Ok(Err(e)) => tracing::warn!("全局 filename rank 构建失败(退化 B-file-i): {e}"),
                Err(e) => tracing::warn!("全局 filename rank 构建任务 join 失败: {e}"),
            }
            this.global_rank_building.store(false, Ordering::Release);
        });
    }

    /// 缩略图结果 → items 取数缓存就地 patch（**不 bump**：缩略图不改视图成员/几何——
    /// 浏览期的持续缩略图生成若走失效，取数缓存将长期冰冷）。S3 后布局行仅存几何，
    /// 出口拼装自 items 缓存取载荷，patch 单点即达（D3 布局侧 patch 已退役）。
    pub fn apply_thumb_results(&self, results: &[crate::db::models::ThumbResult]) {
        crate::layout::items_cache::apply_thumb_results(&self.layout_items_cache, results);
    }

    /// 可视区尺寸回填 → items 缓存就地 patch（布局行几何须经重排产生，不 patch layout_cache）。
    pub fn set_dimensions_cached(&self, dims: &[(i64, i64, i64)]) {
        crate::layout::items_cache::set_dimensions(&self.layout_items_cache, dims);
    }

    /// 收藏写 → items 缓存就地 patch（S3 单点，滚出滚回新鲜度由出口拼装保证）；
    /// favoritedOnly 视图（写改成员）由 items_cache 内部降级不可复用（下次 compute 重查）。
    pub fn set_favorite_cached(&self, ids: &[i64], value: bool) {
        crate::layout::items_cache::set_favorite(&self.layout_items_cache, ids, value);
    }

    /// 评分写 → items 缓存就地 patch（S3 单点）；minRating 过滤视图降级不可复用。
    pub fn set_rating_cached(&self, ids: &[i64], rating: i64) {
        crate::layout::items_cache::set_rating(&self.layout_items_cache, ids, rating);
    }

    /// 色标写 → items 缓存就地 patch（S3 单点）；colorLabel 过滤视图降级不可复用。
    pub fn set_color_label_cached(&self, ids: &[i64], color_label: i64) {
        crate::layout::items_cache::set_color_label(&self.layout_items_cache, ids, color_label);
    }

    /// 丢弃常驻嵌入快照，使下次语义搜索重新加载。在嵌入向量写入或重置时调用。
    /// 只作废缓存,不吊销在途请求:分析落库期的搜索仍可提交(其结果是装载那一刻的自洽读)。
    pub fn invalidate_embedding_cache(&self) {
        self.ai_search.invalidate_cache();
    }

    /// 为 AI 分析流水线创建新的取消令牌,返回 (运行代次, token)。运行代次供完成回调
    /// compare-and-clear 用(F10)。
    pub fn new_ai_analysis_token(&self) -> (u64, CancellationToken) {
        self.ai_analysis_token.begin()
    }

    /// 如果正在运行，取消 AI 分析流水线。
    pub fn cancel_ai_analysis(&self) {
        self.ai_analysis_token.cancel();
    }

    /// Compare-and-clear for the AI run's completion handler (2026-07-10 审查 F10,F-025 迁
    /// [`RunTokenSlot`])。返回值是「本轮是否仍持终态发布权」;ai 侧完成回调**刻意不用它**——
    /// 终态副作用(释放 GPU 槽/清 active 标志)门控在 `!token.is_cancelled()` 上,见 pipeline.rs。
    pub fn finish_ai_analysis(&self, generation: u64) -> bool {
        self.ai_analysis_token.finish(generation)
    }

    /// 为人脸识别流水线创建新的取消令牌,返回(运行代次, token)。
    pub fn new_face_analysis_token(&self) -> (u64, CancellationToken) {
        self.face_analysis_token.begin()
    }

    /// 如果正在运行，取消人脸识别流水线。
    pub fn cancel_face_analysis(&self) {
        self.face_analysis_token.cancel();
    }

    /// face 侧完成回调的 compare-and-clear(语义同 [`Self::finish_ai_analysis`],返回值同样
    /// 刻意不用——终态门控在 `!token.is_cancelled()`)。
    pub fn finish_face_analysis(&self, generation: u64) -> bool {
        self.face_analysis_token.finish(generation)
    }

    /// 原子地为 `owner`（`GPU_OWNER_AI`/`_FACE`）占用唯一 GPU 分析槽。空闲（占用成功）或已由
    /// `owner` 持有（可重入——如 restart 先取消再重启同一流水线）返回 `true`；被**对方**持有返回
    /// `false`。check-and-set 在单一锁区内完成，堵住两个独立 token 检查会留下的 TOCTOU 窗口
    /// （见 `gpu_analysis_owner`）。
    pub fn try_acquire_gpu_analysis(&self, owner: &'static str) -> bool {
        let mut slot = self
            .gpu_analysis_owner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *slot {
            None => {
                *slot = Some(owner);
                true
            }
            Some(cur) => cur == owner,
        }
    }

    /// 释放 GPU 分析槽，但仅当 `owner` 仍持有时（否则为空操作，使过期的完成回调不会释放更新一次
    /// 运行已重新占用的槽）。
    pub fn release_gpu_analysis(&self, owner: &'static str) {
        let mut slot = self
            .gpu_analysis_owner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *slot == Some(owner) {
            *slot = None;
        }
    }

    /// 原子占用 A/B 共用文件任务门闩(方案 B §5.2)。**严格 claim-if-free**:仅空闲时占用成功返回
    /// `true`;已被任意 owner(含同名)持有均返回 `false`(调用方回 `file_job_busy`,不抢占)。
    ///
    /// 审查 #9:此处**不采用** `try_acquire_gpu_analysis` 的可重入姿态(那里重启前显式 cancel 旧轮,
    /// 同名再取是续接同一逻辑任务)。文件任务无此「先 cancel 再重取」协议——begin_backup / restore
    /// 各自独立取一次、收尾各自释放一次。若同名可重入,则手动备份进行中自动调度器每小时的 begin_backup
    /// 会**再拿到同一 BACKUP 槽**并叠跑第二个备份:第二轮 backup_token.begin() 顶掉第一轮世代,第一轮
    /// finish 失去发布权(UI 永卡 running)、第一轮收尾又把门闩提前让出。严格 claim 从源头堵死双开。
    pub fn try_acquire_file_job(&self, owner: &'static str) -> bool {
        let mut slot = self
            .file_job_owner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(owner);
            true
        } else {
            false
        }
    }

    /// 释放文件任务门闩,仅当 `owner` 仍持有时(否则空操作,防过期收尾误释放别人已占的槽)。
    pub fn release_file_job(&self, owner: &'static str) {
        let mut slot = self
            .file_job_owner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *slot == Some(owner) {
            *slot = None;
        }
    }

    /// 取消数据备份任务(若在运行)。先置「取消中」标志(审查 #13:cancel 立即 take 空 token,
    /// backup_status 在收尾窗口据此报「取消中」而非误报 failed),再 cancel token。
    pub fn cancel_backup(&self) {
        self.backup_cancelling.store(true, Ordering::Relaxed);
        self.backup_token.cancel();
    }

    /// 备份是否处于「取消中」窗口(cancel 已发、真终态未落)。backup_status 用。
    pub fn is_backup_cancelling(&self) -> bool {
        self.backup_cancelling.load(Ordering::Relaxed)
    }

    /// 清「取消中」标志(新一轮 begin 前、终态发布后调用)。
    pub fn clear_backup_cancelling(&self) {
        self.backup_cancelling.store(false, Ordering::Relaxed);
    }

    /// 取消导出任务(若在运行),镜像 [`Self::cancel_backup`]:先置「取消中」标志,再 cancel token。
    pub fn cancel_export(&self) {
        self.export_cancelling.store(true, Ordering::Relaxed);
        self.export_token.cancel();
    }

    /// 导出是否处于「取消中」窗口(cancel 已发、真终态未落)。`export_status` 用。
    pub fn is_export_cancelling(&self) -> bool {
        self.export_cancelling.load(Ordering::Relaxed)
    }

    /// 清「取消中」标志(新一轮 begin 前、终态发布后调用)。
    pub fn clear_export_cancelling(&self) {
        self.export_cancelling.store(false, Ordering::Relaxed);
    }

    /// 分级优先级阶梯（高 → 低）：扫描 > 缩略图 > 派生 > AI。
    ///
    /// 每个低优先级层在任一高优先级层活动时让步（sleep），使前台关键工作（扫描/缩略图）
    /// 不会被后台派生/AI 抢占。
    ///
    /// 扫描或缩略图生成是否正在运行。
    /// （pub：派生流水线的「硬让步」谓词 —— 见 derive/pipeline.rs,扫描/缩略图运行中派生全暂停;
    /// 交互让步已与之解耦为涓流,不再共用单一 should_yield 谓词。）
    pub fn is_scan_or_thumb_running(&self) -> bool {
        !self
            .scan_tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
            || self.thumb_gen_token.is_running()
    }

    /// 派生流水线当前是否正在运行（令牌存在）。
    pub fn is_derivation_running(&self) -> bool {
        self.derivation_token.is_running()
    }

    /// **AI** 流水线是否应让步？AI 是最低层 —— 让步给扫描、缩略图、派生与用户主动交互（保持浏览流畅）。
    pub fn ai_yield_blockers(&self) -> Vec<&'static str> {
        let mut blockers = Vec::new();

        // 这里返回具体阻塞源，避免 AI 只能反复打印“高优先级任务”却看不出是谁。
        if !self
            .scan_tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            blockers.push("scan");
        }
        if self.thumb_gen_token.is_running() {
            blockers.push("thumbnail");
        }
        if self.is_derivation_running() {
            blockers.push("derivation");
        }
        // R1：exotic 活动时 AI/人脸让步给它（exotic 优先级高于 AI/face）。逐把锁 → 读 → drop，
        // 不与上面的 token 锁同时持有，避免锁顺序反转。
        if self
            .exotic_analysis_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        {
            blockers.push("exotic");
        }
        if self.is_interactive() {
            blockers.push("interaction");
        }

        blockers
    }

    pub fn should_yield_to_higher_priority(&self) -> bool {
        !self.ai_yield_blockers().is_empty()
    }

    // （2026-07-17 视频性能线 T3）原 should_yield_derivation() = 扫描/缩略图 ∨ 交互,已拆分:
    // 扫描/缩略图 = 硬让步(全暂停,is_scan_or_thumb_running);交互 = 涓流(保留 1 个 in-flight
    // 派生任务,is_interactive)。合并谓词导致持续浏览期间派生彻底饿死(封面迟迟不出);拆分后
    // 前台 compute_layout 仍有 N-1 个核可用,派生以单任务速率推进。语义落点见 derive/pipeline.rs。

    /// 为派生流水线创建新的取消令牌,返回 (运行代次, token)。运行代次供完成回调
    /// compare-and-clear 用(审查 F-01)。
    pub fn new_derivation_token(&self) -> (u64, CancellationToken) {
        self.derivation_token.begin()
    }

    /// 如果正在运行，取消派生流水线。
    pub fn cancel_derivation(&self) {
        self.derivation_token.cancel();
    }

    /// 派生流水线完成回调的 compare-and-clear(语义同 [`Self::finish_ai_analysis`],审查 F-01)。
    /// 返回本轮是否仍是「当前轮」——false 表示已被新一轮取代,收尾不得再动全局状态。
    pub fn finish_derivation(&self, generation: u64) -> bool {
        self.derivation_token.finish(generation)
    }

    /// exotic 流水线在派发**新任务**前是否应让步?(R1/R4)
    ///
    /// exotic 解码发生在 Worker **子进程**——主进程线程 sleep 无法令子进程让出 CPU，故「让步」
    /// 不是 sleep 抢占，而是：① Claimer/Dispatcher 派发**新任务前**用本判断暂缓领取；
    /// ② Worker 子进程以低优先级创建（OS 软让步，见 Part2 §3.6）。已派发的在途解码不可中断让步，
    /// 只能自然完成或超时 kill。
    ///
    /// 让步集 = scan / thumbnail / interaction。**不含** derivation：exotic 与 derivation 同级、
    /// 共享公平后台重活池（R4）；若硬让步 derivation，大视频库下 derivation 长期运行会饿死 exotic。
    /// 同一 item 不会被两者同时处理（exotic 认领的格式不进主派生），故同级不产生同 item 互等。
    pub fn should_yield_exotic(&self) -> bool {
        self.is_scan_or_thumb_running() || self.is_interactive()
    }

    /// 冷门格式流水线当前是否正在运行（令牌存在）。
    pub fn is_exotic_running(&self) -> bool {
        self.exotic_analysis_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// 绑定 exotic Coordinator（setup 内一次性写入）。
    pub fn set_exotic_coordinator(
        &self,
        c: std::sync::Arc<crate::exotic::coordinator::ExoticCoordinator>,
    ) {
        let _ = self.exotic_coordinator.set(c);
    }

    /// 注册视频格式扩展 Service（晚绑定；同 [`Self::set_exotic_coordinator`] 姿态）。
    pub fn set_video_worker_service(
        &self,
        s: std::sync::Arc<crate::video::worker_service::VideoWorkerService>,
    ) {
        let _ = self.video_worker_service.set(s);
    }

    /// 构造运行期 [`crate::exotic::ExoticHost`]：组合 catalog（能力真相）+ 只读连接池（安装真相）+ keyring（授权真相）。
    /// 廉价（Arc/Pool clone），命令与调度按需新建、不缓存——确保读到最新
    /// 安装/授权状态（安装、激活后立即生效）。
    pub fn exotic_host(&self) -> crate::exotic::ExoticHost {
        // 组合根按渠道装配授权 provider；for_runtime 只消费统一接口。
        crate::exotic::ExoticHost::for_runtime(
            self.exotic_catalog.clone(),
            self.db_read_pool.clone(),
            crate::exotic::default_entitlement_provider(),
        )
    }

    /// 命令层取授权 provider 的统一入口（审查 R1-1）：激活 / 撤销与 evaluate 全走
    /// [`crate::exotic::default_entitlement_provider`] 装配，确保全部授权路径使用同一信任根。
    /// 与 [`Self::exotic_host`] 同理按需新建、不缓存（廉价，读到最新态）。
    pub fn entitlement_provider(&self) -> std::sync::Arc<dyn crate::exotic::EntitlementProvider> {
        crate::exotic::default_entitlement_provider()
    }

    /// 已装插件根目录（`<exotic_dir>/plugins`；各插件装到其下 `<plugin_id>`）。
    pub fn exotic_install_dir(&self) -> std::path::PathBuf {
        self.exotic_dir.join("plugins")
    }
    /// 解包暂存根（`<exotic_dir>/staging`）。
    pub fn exotic_staging_dir(&self) -> std::path::PathBuf {
        self.exotic_dir.join("staging")
    }
    /// 签名 Registry 本地缓存目录（`<exotic_dir>/registry`）。
    pub fn exotic_registry_dir(&self) -> std::path::PathBuf {
        self.exotic_dir.join("registry")
    }

    /// 静默 exotic 子系统以便替换/移除安装目录（§6.4 第 9 步前置）：置 paused 阻止新一轮启动、
    /// 取消在途 Pipeline（Supervisor kill→wait Worker，释放 exe 句柄），轮询直至不再运行或超时。
    /// 返回 `(prev_paused, quiesced)`：`prev_paused`=操作前 paused 原值（据此 [`Self::resume_after_quiesce`]
    /// 恢复）；`quiesced`=是否在超时内真正停住（false=Pipeline 仍在跑）。**调用方必须检查 `quiesced`**——
    /// 为 false 时**不得**执行目录 rename/删除（Windows 下被占用的 worker.exe 无法改名，强行操作会留破损态，
    /// 安全评审 medium），应 resume 后向前端报错。
    /// **必须**在原子切换/删目录前调用。
    /// 2026-07-06 审查 P1-5:取 `self: &Arc<Self>` 使 DB 段能下沉 spawn_blocking——`db_writer.lock()`
    /// 会等待在途大写(扫描批提交/缩略图批写)释放,直跑 async 正文会阻塞 tokio worker 数秒
    /// (「不做逐条估时豁免」硬化条款)。且 `exotic_paused` 写失败原先被 `let _` 静默——它正是堵
    /// 「取消后立即重启」竞态的闸门,写失败则 quiesce 防护形同虚设,故改为失败记 error。
    pub async fn quiesce_exotic(self: &Arc<Self>, timeout: std::time::Duration) -> (bool, bool) {
        let this = Arc::clone(self);
        let prev_paused = tokio::task::spawn_blocking(move || {
            let conn = this.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            let p = crate::db::queries::get_config(&conn, "exotic_paused")
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false);
            // 先置 paused：evaluate_run 见 paused 即不再启动新一轮（堵住取消后立即重启的竞态）。
            if let Err(e) = crate::db::queries::set_config(&conn, "exotic_paused", "true") {
                tracing::error!("quiesce_exotic 置 paused 失败,取消后重启竞态防护失效 | set exotic_paused failed: {e}");
            }
            p
        })
        .await
        .unwrap_or(false);
        self.cancel_exotic_analysis(); // 取消在途 → Supervisor kill→wait
        let start = Instant::now();
        while self.is_exotic_running() && start.elapsed() < timeout {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let quiesced = !self.is_exotic_running();
        if !quiesced {
            tracing::warn!("quiesce_exotic 超时：Pipeline 仍在运行，拒绝执行目录操作");
        }
        (prev_paused, quiesced)
    }

    /// 恢复 quiesce 前的 paused 状态并唤醒（安装/卸载完成后）。
    /// 2026-07-06 审查 P1-5:DB 段下沉 spawn_blocking;写失败原先静默会让 exotic 流水线永久停摆
    /// (paused 卡在 true 无人清),改为 error 留痕。
    pub async fn resume_after_quiesce(self: &Arc<Self>, prev_paused: bool) {
        let this = Arc::clone(self);
        let _ = tokio::task::spawn_blocking(move || {
            let conn = this.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = crate::db::queries::set_config(
                &conn,
                "exotic_paused",
                if prev_paused { "true" } else { "false" },
            ) {
                tracing::error!("resume_after_quiesce 复位 paused 失败,exotic 流水线可能停摆 | restore exotic_paused failed: {e}");
            }
        })
        .await;
        self.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
    }

    /// 幂等唤醒 exotic 调度（Coordinator 未绑定则静默忽略）。扫描提交/命令/配置变更后调用。
    pub fn wake_exotic(&self, reason: crate::exotic::coordinator::WakeReason) {
        if let Some(c) = self.exotic_coordinator.get() {
            c.wake(reason);
        }
    }

    /// 为冷门格式流水线创建一个新的取消令牌。
    pub fn new_exotic_analysis_token(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self
            .exotic_analysis_token
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        token
    }

    /// 如果正在运行，取消冷门格式流水线。
    pub fn cancel_exotic_analysis(&self) {
        if let Some(token) = self
            .exotic_analysis_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            token.cancel();
        }
    }

    /// 为扫描根目录创建一个新的取消令牌，替换任何现有的令牌。
    ///
    /// 旧调用方只需要令牌时继续使用此入口；需要把运行身份传给后台任务的调用方使用
    /// [`Self::new_scan_run_token`]。代次槽按根保留，不能随取消而移除，否则旧轮迟到的收尾
    /// 可能与新轮复用同一个代次。
    pub fn new_scan_token(&self, root_id: i64) -> CancellationToken {
        self.replace_scan_run_token(root_id).1
    }

    /// 为扫描根目录创建新的 `(generation, token)`。
    pub fn new_scan_run_token(&self, root_id: i64) -> (u64, CancellationToken) {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let write_gate = self.scan_generation_write_gate(root_id);
        let _write_gate = write_gate.lock().unwrap_or_else(|e| e.into_inner());
        // 与 cancel/finish 保持同一锁顺序：先锁兼容视图，再锁代次槽，避免两张表之间的竞态。
        let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let mut slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let result = begin_scan_run(&mut tokens, &mut slots, root_id);
        self.scan_run_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&root_id);
        result
    }

    /// 原子取消并替换同一根目录的扫描轮次。
    ///
    /// `start_scan` 使用此入口把「取消旧轮」与「安装新轮」放在同一锁区；旧轮迟到的
    /// completion 只能在锁区后看到新 generation，不能观察到一个可重新 claim 的空槽。
    /// 同时与扫描批次/收尾写入共用根级写闸门，避免检查—写入间隙让旧轮覆盖新轮。
    pub fn replace_scan_run_token(&self, root_id: i64) -> (u64, CancellationToken) {
        self.replace_scan_run_token_with_id(root_id, None)
    }

    /// 原子取消并替换扫描轮次，同时登记前端可用于 stop 的运行身份。
    pub fn replace_scan_run_token_with_id(
        &self,
        root_id: i64,
        run_id: Option<&str>,
    ) -> (u64, CancellationToken) {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let write_gate = self.scan_generation_write_gate(root_id);
        let _write_gate = write_gate.lock().unwrap_or_else(|e| e.into_inner());
        let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let mut slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let result = replace_scan_run(&mut tokens, &mut slots, root_id);
        let mut run_ids = self.scan_run_ids.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(run_id) = run_id {
            run_ids.insert(root_id, run_id.to_string());
        } else {
            run_ids.remove(&root_id);
        }
        result
    }

    /// 在数据库生命周期仍 active 时安装扫描轮次；清空/删根/重链接开始后拒绝安装。
    ///
    /// `start_scan` 在 IPC 入口使用此版本，避免前端的 Promise barrier 结束后，后端仍在
    /// 生命周期写区内把新 token 安装到即将清空的库中。普通兼容调用继续使用上方的
    /// `replace_scan_run_token_with_id`，其读锁会等待当前生命周期写操作完成。
    pub(crate) fn try_replace_scan_run_token_with_id(
        &self,
        root_id: i64,
        run_id: Option<&str>,
    ) -> Option<(u64, CancellationToken)> {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        let write_gate = self.scan_generation_write_gate(root_id);
        let _write_gate = write_gate.lock().unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }

        let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let mut slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let result = replace_scan_run(&mut tokens, &mut slots, root_id);
        let mut run_ids = self.scan_run_ids.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(run_id) = run_id {
            run_ids.insert(root_id, run_id.to_string());
        } else {
            run_ids.remove(&root_id);
        }
        Some(result)
    }

    /// 取消根目录的扫描令牌（如果存在）。
    pub fn cancel_scan(&self, root_id: i64) {
        let _ = self.with_scan_root_exclusive(root_id, || {
            self.cancel_scan_under_root_gate(root_id);
        });
    }

    /// 在已持有指定根级闸门时取消当前扫描。
    ///
    /// 只供 `with_scan_root_exclusive` 的闭包调用；不能单独使用，否则会绕过闸门破坏
    /// generation 写入的线性化顺序。
    pub(crate) fn cancel_scan_under_root_gate(&self, root_id: i64) {
        let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(slot) = slots.get(&root_id) {
            slot.cancel();
        }
        if let Some(token) = tokens.remove(&root_id) {
            // `RunTokenSlot::cancel` 已取消当前 token；此处再 cancel 是幂等的，且保留对
            // 兼容视图中异常遗留令牌的兜底语义。
            token.cancel();
        }
        self.scan_run_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&root_id);
    }

    /// 只取消仍对应指定前端运行身份的扫描；旧 stop 在 restart 后会变成 no-op。
    pub fn cancel_scan_run(&self, root_id: i64, run_id: &str) -> bool {
        self.with_scan_root_exclusive(root_id, || {
            let current = self
                .scan_run_ids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&root_id)
                .is_some_and(|current| current == run_id);
            if !current {
                return false;
            }
            self.cancel_scan_under_root_gate(root_id);
            true
        })
        .unwrap_or(false)
    }

    /// 只取消并清除指定扫描代次。返回值表示该代次仍拥有当前槽；旧轮不得触碰新轮。
    pub fn cancel_scan_generation(&self, root_id: i64, generation: u64) -> bool {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let write_gate = self.scan_generation_write_gate(root_id);
        let _write_gate = write_gate.lock().unwrap_or_else(|e| e.into_inner());
        let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let mut slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(token) = take_scan_run_if_owned(&mut tokens, &mut slots, root_id, generation) {
            token.cancel();
            self.scan_run_ids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&root_id);
            true
        } else {
            false
        }
    }

    /// 后台 enrichment 收尾的 compare-and-clear。仅指定代次仍是当前轮时才移除兼容视图。
    pub fn finish_scan(&self, root_id: i64, generation: u64) -> bool {
        self.finish_scan_with_action(root_id, generation, || {})
    }

    /// 原子地 compare-and-clear 指定扫描代次，并在清理 token 前后不允许新轮插入任何
    /// 外部终态动作。
    ///
    /// 扫描收尾需要把「移除旧 token」和「发出旧轮终态事件」作为一个线性化单元；否则
    /// 新轮可以在两者之间安装，旧轮随后仍会把取消/失败事件送到新轮。
    pub(crate) fn finish_scan_with_action<F>(
        &self,
        root_id: i64,
        generation: u64,
        action: F,
    ) -> bool
    where
        F: FnOnce(),
    {
        self.with_scan_root_exclusive(root_id, || {
            let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
            let mut slots = self
                .scan_token_slots
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if take_scan_run_if_owned(&mut tokens, &mut slots, root_id, generation).is_some() {
                self.scan_run_ids
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&root_id);
                action();
                true
            } else {
                false
            }
        })
        .unwrap_or(false)
    }

    /// 只读判断指定扫描代次是否仍是当前活动轮。
    pub fn is_scan_generation_current(&self, root_id: i64, generation: u64) -> bool {
        let tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        if !tokens.contains_key(&root_id) {
            return false;
        }
        let slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        slots
            .get(&root_id)
            .is_some_and(|slot| slot.is_current(generation))
    }

    /// 在指定扫描代次仍为当前轮时，在线性化闸门内运行一个写闭包。
    ///
    /// `replace_scan_run_token` 取得同一根目录的闸门后才会安装新轮，所以调用方可把
    /// generation 检查和 DB writer lock/事务放在同一闭包边界内；返回 `None` 表示旧轮已
    /// 失去写入资格。闭包只应执行短的 DB 状态写入，不应在其中做文件 IO。
    pub(crate) fn with_scan_generation_write<T, F>(
        &self,
        root_id: i64,
        generation: u64,
        write: F,
    ) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        let write_gate = self.scan_generation_write_gate(root_id);
        with_scan_generation_gate(
            &write_gate,
            || self.is_scan_generation_current(root_id, generation),
            write,
        )
    }

    /// 独占一个扫描根的整段操作；适用于 relink 这类必须把文件抽样、路径切换和取消
    /// 活动扫描串成一个动作的低频管理操作。闭包可做文件 IO，但不得在其中跨 await，
    /// 也不得把 `db_writer` 锁与文件 IO 交叠。
    pub(crate) fn with_scan_root_exclusive<T, F>(&self, root_id: i64, action: F) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        let write_gate = self.scan_generation_write_gate(root_id);
        let _write_gate = write_gate.lock().unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        Some(action())
    }

    /// 以固定顺序（root_id 升序）独占多个扫描根的写闸门；同一根只取一次。
    ///
    /// 跨根目录移动要同时挡住源根与目标根上的扫描：两把根级闸门必须按 root_id 升序获取，
    /// 否则两次方向相反的跨根移动会各持一把互等（死锁）。锁序与 `new_scan_run_token` 一致
    /// （先扫描生命周期读锁，再根级闸门），故持闸期间新的扫描轮次安装不进来——这正是
    /// 「操作期间不允许扫描竞争」的实现点，调用方只需在闭包开头检查是否已有扫描在跑。
    /// 闭包不得跨 await，也不得把 `db_writer` 锁与文件 IO 交叠。
    pub(crate) fn with_scan_roots_exclusive<T, F>(&self, root_ids: &[i64], action: F) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        let gates: Vec<Arc<Mutex<()>>> = {
            let mut map = self
                .scan_write_gates
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            ordered_scan_gate_handles(&mut map, root_ids)
        };
        let _guards: Vec<_> = gates
            .iter()
            .map(|gate| gate.lock().unwrap_or_else(|e| e.into_inner()))
            .collect();
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        Some(action())
    }

    /// 这些根上是否已有扫描轮次在跑（`scan_tokens` 运行态视图）。
    ///
    /// 供文件级管理操作（目录移动）在做破坏性动作前拒绝与扫描竞争：必须与
    /// `with_scan_roots_exclusive` 配合使用——只有持着闸门时读到的空集才意味着
    /// 「检查到动作之间不会有新扫描插进来」。
    pub(crate) fn any_scan_running(&self, root_ids: &[i64]) -> bool {
        let tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        any_root_running(&tokens, root_ids)
    }

    /// 独占扫描生命周期并锁住一个根的代次闸门。
    ///
    /// 根删除需要在清理缩略图文件期间阻止新的根管理/扫描操作，否则同一个 cache key
    /// 可能在旧文件异步删除和新根重建之间发生碰撞。调用方仍须在 DB writer 锁释放后
    /// 执行文件 IO；此方法只负责生命周期与根级线性化，不替调用方持有数据库锁。
    pub(crate) fn with_scan_root_and_lifecycle_exclusive<T, F>(&self, root_id: i64, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self.enter_database_lifecycle_write();
        let write_gate = self.scan_generation_write_gate(root_id);
        let _write_gate = write_gate.lock().unwrap_or_else(|e| e.into_inner());
        action()
    }

    /// 在数据库生命周期写区内运行同步闭包，并使所有已经捕获的 worker epoch 失效。
    /// 适用于全局清缓存等不绑定单一扫描根的动作。
    pub(crate) fn with_scan_lifecycle_exclusive<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self.enter_database_lifecycle_write();
        action()
    }

    /// 在不涉及具体扫描根的低频生命周期操作中持有扫描生命周期读锁。
    ///
    /// 例如添加/隐藏扫描根需要在数据库操作期间阻止 `clear_database`，但尚未必然有
    /// 可用的根级 gate；调用方必须把同步操作放进此闭包，且不得跨异步边界。
    pub(crate) fn with_scan_lifecycle_read<T, F>(&self, action: F) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        Some(action())
    }

    /// 取当前数据库生命周期 epoch。读锁同时保证 active 与 epoch 的快照相互一致；调用方
    /// 只应把该值带到后台任务，最终写入仍须使用 [`Self::with_database_lifecycle_read`]。
    pub(crate) fn current_database_epoch(&self) -> Option<u64> {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        Some(self.database_lifecycle_epoch.load(Ordering::Acquire))
    }

    /// 只在生命周期仍 active 且 epoch 未变化时运行最终缩略图文件/DB 写入闭包。
    /// 闭包不得包含解码、UI 等长任务；它只覆盖最终原子文件写入或短 DB 事务。
    pub(crate) fn with_database_lifecycle_read<T, F>(&self, epoch: u64, action: F) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire)
            || self.database_lifecycle_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        Some(action())
    }

    /// 在生命周期读锁内验证一个已捕获 epoch，供解码后清理/状态更新等非持久化动作使用。
    pub(crate) fn is_database_epoch_current(&self, epoch: u64) -> bool {
        self.with_database_lifecycle_read(epoch, || ()).is_some()
    }

    /// 在去重分析/清理的生命周期读区内运行同步闭包；不得跨 await。
    pub(crate) fn with_dedup_lifecycle_read<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _dedup_lifecycle_gate = self
            .dedup_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        action()
    }

    /// 独占去重生命周期；全库清空使用此原语。
    pub(crate) fn with_dedup_lifecycle_write<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _dedup_lifecycle_gate = self
            .dedup_lifecycle_gate
            .write()
            .unwrap_or_else(|e| e.into_inner());
        action()
    }

    /// 获取某扫描根的 DB 写线性化闸门。闸门对象按根稳定复用，避免旧轮与新轮拿到不同锁。
    fn scan_generation_write_gate(&self, root_id: i64) -> Arc<Mutex<()>> {
        let mut gates = self
            .scan_write_gates
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        gates
            .entry(root_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// 取消所有正在运行的扫描。
    pub fn cancel_all_scans(&self) {
        self.with_all_scans_exclusive(|| self.cancel_all_scans_under_exclusive());
    }

    /// 在已持有扫描生命周期写锁和全部根级 gate 时取消所有扫描。
    ///
    /// `clear_database` 复用此原语，把「取消 → 删除 DB → 清缓存」放在同一个独占区，
    /// 防止新一轮扫描在取消和删库之间重新安装。
    pub(crate) fn cancel_all_scans_under_exclusive(&self) {
        // 调用方已持有全部根级 gate；已进入某根写区的旧批次已完成，且新 start 被
        // 生命周期写锁阻止，因此此处只需清除令牌视图。
        let mut tokens = self.scan_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let slots = self
            .scan_token_slots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for slot in slots.values() {
            slot.cancel();
        }
        for token in tokens.values() {
            token.cancel();
        }
        tokens.clear();
        self.scan_run_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// 独占整个扫描生命周期，阻止新的扫描/根管理操作进入。
    ///
    /// 先持生命周期写锁，再锁住当前所有根级 gate；因此已进入根级写区的批次会先完成，
    /// 闸门收集后也不会再出现新根。闭包只能执行同步操作，不得跨 await。
    pub(crate) fn with_all_scans_exclusive<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _lifecycle_gate = self.enter_database_lifecycle_write();
        let gates_map = self
            .scan_write_gates
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _gates = gates_map
            .values()
            .map(|gate| gate.lock().unwrap_or_else(|e| e.into_inner()))
            .collect::<Vec<_>>();
        action()
    }

    /// 标记生命周期 inactive、取得全局写锁并递增 epoch。inactive 的发布在等待写锁前完成，
    /// 所以新的根管理/扫描/缩略图写入即使排在写锁后面，也会在执行点被拒绝。
    fn enter_database_lifecycle_write(&self) -> DatabaseLifecycleWriteGuard<'_> {
        enter_database_lifecycle_write(
            &self.database_lifecycle_transition_gate,
            &self.scan_lifecycle_gate,
            &self.database_lifecycle_active,
            &self.database_lifecycle_epoch,
        )
    }

    /// 为全量缩略图生成创建新的取消令牌,返回 (运行代次, token)。运行代次供完成回调
    /// compare-and-clear 用(审查 F-02)。
    pub fn new_thumb_gen_token(&self) -> (u64, CancellationToken) {
        self.with_thumb_generation_gate(|| self.thumb_gen_token.begin())
    }

    /// 在当前数据库生命周期内安装全库缩略图生成轮次，并返回其 epoch。清空/删根已经
    /// 发布 inactive 后，调用方不会启动一个必然向旧数据库写回的 worker。
    pub(crate) fn try_new_thumb_gen_token(&self) -> Option<(u64, CancellationToken, u64)> {
        let _thumb_gate = self
            .thumb_gen_lifecycle_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _lifecycle_gate = self
            .scan_lifecycle_gate
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !self.database_lifecycle_active.load(Ordering::Acquire) {
            return None;
        }
        self.thumb_gen_token.cancel();
        let epoch = self.database_lifecycle_epoch.load(Ordering::Acquire);
        let (generation, token) = self.thumb_gen_token.begin();
        Some((generation, token, epoch))
    }

    /// 如果正在运行，取消全量缩略图生成任务。
    pub fn cancel_thumb_gen(&self) {
        self.with_thumb_generation_gate(|| self.thumb_gen_token.cancel_keep_generation());
    }

    /// 在线性化闸门内运行缩略图生成的短状态操作。闭包不得包含解码、编码、文件 IO 或
    /// 其它可能阻塞的工作；若同时需要数据库生命周期读锁，必须在闭包内再获取它。
    pub(crate) fn with_thumb_generation_gate<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _thumb_gate = self
            .thumb_gen_lifecycle_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        action()
    }

    /// 撤销所有缩略图 worker，并在同一线性化区内执行需要重置缩略图状态的动作。
    ///
    /// 锁序固定为 thumb generation → database lifecycle。调用方可以在闭包内先做缓存
    /// 文件 IO，再用短暂的 `db_writer` 事务复位状态；旧 full/batch worker 的 epoch 或
    /// generation 检查都会在该写区之后失效。
    pub(crate) fn with_thumbnail_reset_exclusive<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.with_thumb_generation_gate(|| {
            self.thumb_gen_token.cancel();
            self.with_scan_lifecycle_exclusive(action)
        })
    }

    /// 查看器渲染色域(B 线,方案 §0①)的 keyed 并发去重锁:取出(或安装)`(item_id, target_id)`
    /// 对应的 `Arc<tokio::sync::Mutex<()>>`。std Mutex 仅护 map 本身,取出 Arc 后立即释放——
    /// 不跨 `.await`(硬约束)。调用方随后在返回的 Arc 上 `.lock().await`,持锁横跨
    /// `spawn_blocking` 的整段渲染(per-key tokio Mutex 跨 await 持有,是硬约束允许的
    /// "unavoidable" 场景:同一 item+target 的并发请求必须单渲,又不能用 std Mutex 卡住
    /// 整个 tokio worker)。
    pub fn viewer_render_lock(&self, key: (i64, String)) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self
            .viewer_render_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        map.entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// 渲染完成后回收 keyed lock 条目:仅当 `Arc::strong_count==1`(除 map 自身持有的一份外
    /// 再无调用方等待同一 key)才移除,避免 map 无界增长,也避免误删仍有并发等待者的条目。
    pub fn release_viewer_render_lock(&self, key: &(i64, String)) {
        let mut map = self
            .viewer_render_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(arc) = map.get(key) {
            if Arc::strong_count(arc) == 1 {
                map.remove(key);
            }
        }
    }
}
