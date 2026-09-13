// src-tauri/src/error.rs
//! 统一的应用程序错误类型。
//! 所有变体都是可序列化的，以便可以通过 IPC 转发到前端。

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("Connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("EXIF parse error: {0}")]
    Exif(#[from] exif::Error),

    #[error("XMP parse error: {0}")]
    Xmp(#[from] quick_xml::Error),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Image engine error: {0}")]
    Engine(#[from] image::ImageError),

    #[error("Path resolution error: {0}")]
    PathResolution(String),

    #[error("FFmpeg error: {0}")]
    FFmpeg(String),

    #[error("Audio metadata error: {0}")]
    AudioMetadata(String),

    #[error("Document render error: {0}")]
    DocumentRender(String),

    #[error("Layout cache not ready — call compute_layout first")]
    LayoutNotReady,

    /// 选择守门失效（T18）：SelectAll 携带的 `layout_version` 与当前布局不一致 → 视图在全选后已变，
    /// 拒绝按可能已漂移的集合执行批量写。前端据此**重算 layout 拿新版本 → 重发命令**。可恢复、不记错误日志。
    #[error("View stale — layout changed since selection; recompute and retry")]
    ViewStale,

    /// 重复镜头描述符未通过 MVP 校验（2026-09-02 主画廊重复项浏览方案 §10.2）。
    /// message 由 [`crate::db::models::DuplicateLensDescriptor::validate`] 按违规规则
    /// 给出固定文案（中文|英文），前端按稳定 code `DuplicateLensInvalid` 统一分流
    /// （提示重建描述符，而非匹配文案）。
    #[error("Duplicate lens descriptor invalid: {0}")]
    DuplicateLensInvalid(String),

    /// 重复镜头 **folders 模式**的 SQL lowering 尚未实现（方案 §13：groups 模式已在 P2
    /// 落地，folders 排列属 P3）：`view_to_sql` 收到 `mode=folders` 的镜头描述符时显式
    /// 拒绝，绝不静默按 groups 降维执行。稳定 code `DuplicateLensUnsupported`，
    /// P3 落地后此变体的发射点应随之消失。
    #[error("重复镜头 folders 模式解析尚未支持（P3） | Duplicate lens folders mode lowering not supported yet")]
    DuplicateLensUnsupported,

    #[error("Scan root not found: id={0}")]
    ScanRootNotFound(i64),

    #[error("Media item not found: id={0}")]
    MediaNotFound(i64),

    /// 卷离线（T13 §3.7）：打开原图/视频等需实体文件访问的操作，其所在卷当前离线（可自动恢复）。
    /// message 携带卷标签（或 stable_id 兜底）供前端弹「请插入设备 <label>」；前端按稳定 code
    /// `VolumeOffline` 分流（非破图、非硬故障，重连即恢复），不记错误日志。
    #[error("Volume offline: {0}")]
    VolumeOffline(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("AI inference error: {0}")]
    Ai(String),

    #[error("AI model not loaded: {0}")]
    AiModelNotLoaded(String),

    /// 资源竞争是可等待状态，前端按稳定码续跑，不解析本地化文案。
    #[error("Another analysis owns the GPU session")]
    AnalysisBusy,

    #[error("System error: {0}")]
    System(String),

    #[error("OS error: {0}")]
    Os(String),

    #[error("AI tokenization error: {0}")]
    AiTokenizer(String),

    #[error("Internal error: {0}")]
    Internal(String),

    /// 冷门格式插件子系统错误。`code` 为底层稳定标识——直接取自 FetchError / Registry / License /
    /// crypto 各自的 `code()`，或命令层自定的稳定字面量——并原样透到 IPC `code` 字段，让前端可
    /// **按类型**分流（如「回滚攻击被拒」`rollback` 给安全警告、「网络失败」`http` 给重试），
    /// 而非靠匹配 `message` 文案（脆弱、改文案即失效）。message 仅作展示/日志，不承担分流职责。
    #[error("exotic error [{code}]: {message}")]
    Exotic { code: &'static str, message: String },

    /// reveal（在文件管理器中显示）失败。与 [`AppError::Exotic`] 同姿态：`code` 是稳定分流标识
    /// （`unsupported_platform` = 移动端永不支持，前端据此**永久隐藏**动作而非提示重试；
    /// `reveal_failed` = 通用失败，提示即可），原样透到 IPC `code` 字段。此前稳定子码寄生在
    /// `PathResolution` 的 message 里、序列化 code 恒为 "PathResolution"——前端要分流就只能匹配
    /// 文案，改文案即失效（S 线审查 R-08）。
    #[error("reveal error [{code}]: {message}")]
    Reveal { code: &'static str, message: String },

    /// 树内文本预览(path-based 只读打开,问题②方案 B v1)失败。与 [`AppError::Reveal`]
    /// 同姿态:`code` 是稳定分流标识(`preview_unsupported_type` = 扩展名不在白名单,前端
    /// 不该发起、收到即为兜底;`preview_failed` = 读取失败,提示即可),原样透到 IPC `code`
    /// 字段。message **不携带绝对路径**(泄漏面)。
    #[error("preview error [{code}]: {message}")]
    Preview { code: &'static str, message: String },

    /// 重链接扫描根(#7 方案A:文件夹整体迁移后改根路径、免重扫免重生成)失败。与
    /// [`AppError::Preview`] 同姿态:`code` 是稳定分流标识,原样透到 IPC `code` 字段——
    /// `relink_not_a_dir`(新路径不存在或不是目录)/ `relink_path_taken`(该路径已被别的
    /// 扫描根占用,`scan_roots.path` 有 UNIQUE)/ `relink_mismatch`(抽样校验不过,目标
    /// 不是同一棵目录树)。三者前端出的话术与后续动作都不同,故必须可按 code 分流。
    /// **不复用 `InvalidMove`**:那是 file_ops 的「文件夹移动非法」,已被 8 处占用,
    /// 挤进同一 code 前端就分不开了。message 只带统计数,**不携带绝对路径**(泄漏面)。
    #[error("relink error [{code}]: {message}")]
    Relink { code: &'static str, message: String },

    /// 数据备份(方案 B §9)失败。与 [`AppError::Relink`] 同姿态:`code` 是稳定小写分流标识,
    /// 原样透到 IPC `code` 字段。稳定码集:`backup_dir_unset`(未设目的地)/ `backup_dir_not_writable`
    /// (目的地不可写)/ `backup_document_inconsistent`(文档行↔文件不一致,拒产假完整包)/
    /// `backup_cancelled`(用户取消)/ `backup_io`(读写失败)/ `file_job_busy`(A/B 文件任务占用)。
    /// message **不携带绝对路径 / SQL / 内部错误串**(泄漏面,方案 §9)。
    #[error("backup error [{code}]: {message}")]
    Backup { code: &'static str, message: String },

    /// 数据恢复(方案 B §6/§9)失败。与 [`AppError::Backup`] 同姿态:`code` 是稳定小写分流标识,
    /// 原样透到 IPC `code` 字段。稳定码集:`restore_format_unsupported`(包格式版本过新)/
    /// `restore_schema_too_new`(schema 新于本二进制)/ `restore_corrupt`(quick_check/sha256 不符/
    /// 损坏)/ `restore_path_invalid`(zip-slip/盘符/符号链接逃逸)/ `restore_size_limit`(条目数/
    /// 解压尺寸超限,zip bomb)/ `restore_document_missing`(rebase 后版本文件缺失)/
    /// `restore_rollback_failed`(预恢复回滚包失败)/ `restore_io`。message **不携带绝对路径 / SQL /
    /// 内部错误串**(泄漏面,方案 §9)。
    #[error("restore error [{code}]: {message}")]
    Restore { code: &'static str, message: String },

    /// 导出整理成果(方案 A)失败。与 [`AppError::Backup`] 同姿态:`code` 是稳定小写分流标识,
    /// 原样透到 IPC `code` 字段。稳定码集:`export_target_invalid`(目的父目录不存在/不是目录)/
    /// `export_target_not_writable`(不可写)/ `export_target_inside_library`(命中扫描根内部,
    /// 未带 `allow_inside_library` 确认)/ `export_cancelled`(用户取消)/ `export_io`(读写失败)/
    /// `file_job_busy`(A/B 文件任务占用,与 backup/restore 复用同一域共享码,前端不分
    /// backup/export 统一分流「有文件任务在跑」)。SelectAll 所据布局过期复用全局
    /// [`AppError::ViewStale`](稳定码 `"ViewStale"`),不落本变体的私有码集(2026-07-19 复审 P3:
    /// 此前误记 `export_view_stale`,全仓从无该发射点)。message **不携带绝对路径 / SQL / 内部
    /// 错误串**(泄漏面,方案 §3.3)。
    #[error("export error [{code}]: {message}")]
    Export { code: &'static str, message: String },

    /// 图片简单编辑(方案 C §6)失败。与 [`AppError::Export`] 同姿态:`code` 是稳定小写分流标识,
    /// 原样透到 IPC `code` 字段。稳定码集:`edit_source_unavailable`(源文件不可读)/
    /// `edit_format_unsupported`(GIF/HEIC/AVIF 等 v1 不承诺的格式)/ `edit_decode_failed` /
    /// `edit_encode_failed` / `edit_image_too_large`(`memory_budget::CODE_TOO_LARGE`,峰值预算超限)/
    /// `edit_crop_empty`(`geometry::CODE_CROP_EMPTY`,裁剪区域为空)/
    /// `edit_invalid_ops`(`geometry::CODE_INVALID_OPS`,rotate 等编辑参数非法,D-010 新增)/
    /// `edit_target_conflict` /
    /// `edit_io` / `edit_saved_needs_index`(文件已落盘、单文件 ingest 失败的 partial 恢复态)/
    /// `file_job_busy`(A/B/C 共用文件任务门闩)。message **不携带绝对路径 / SQL / 内部错误串**
    /// (泄漏面,同 Backup/Export)。
    #[error("edit error [{code}]: {message}")]
    Edit { code: &'static str, message: String },

    /// 配置文件(config.toml,A2)读写/校验失败。与 [`AppError::Backup`] 同姿态:`code` 为稳定
    /// 小写分流标识,原样透到 IPC `code` 字段。稳定码集:`config_invalid_value`(`set_app_config`
    /// 提交的值未通过 `SettingKind` 校验)/ `config_write_failed`(原子写盘失败)/
    /// `config_open_failed`(`open_config_file` 打开外部编辑器与回退定位均失败)。message
    /// 不携带绝对路径 / 内部原始异常字符串(泄漏面,硬约束)。
    #[error("config error [{code}]: {message}")]
    Config { code: &'static str, message: String },

    /// 播放器(字幕加载/截帧保存,2026-07-22 播放器线)失败。与 [`AppError::Edit`] 同姿态:
    /// `code` 是稳定小写分流标识,原样透到 IPC `code` 字段。稳定码集:`player_subtitle_not_found` /
    /// `player_subtitle_unsupported_type` / `player_subtitle_too_large` / `player_subtitle_io` /
    /// `player_frame_target_invalid` / `player_frame_decode_failed` / `player_frame_io`。
    /// message **不携带绝对路径**(泄漏面,同 Preview/Reveal 姿态)。
    #[error("player error [{code}]: {message}")]
    Player { code: &'static str, message: String },

    /// 查看器渲染色域(B 线,2026-07-23,方案 §0①)失败。与 [`AppError::Player`] 同姿态:
    /// `code` 是稳定小写分流标识,原样透到 IPC `code` 字段。稳定码集:`icc_parse_failed` /
    /// `icc_not_rgb` / `icc_not_display_class` / `icc_transform_unsupported` / `icc_too_large` /
    /// `icc_io` / `icc_not_found` / `unsupported_platform` / `viewer_render_unsupported` /
    /// `viewer_render_decode_failed` / `viewer_render_too_large` / `viewer_render_io`。message
    /// **不携带绝对路径 / 底层错误串**(泄漏面,同 Preview/Reveal/Player 姿态)。
    #[error("color error [{code}]: {message}")]
    Color { code: &'static str, message: String },

    /// OCR 文字提取(B′路线,2026-07-23)失败。与 [`AppError::Player`] 同姿态:`code` 是稳定
    /// 小写分流标识,原样透到 IPC `code` 字段。稳定码集:`ocr_unlicensed`(插件未激活)/
    /// `ocr_license_expired`(授权过期)/ `ocr_unavailable`(平台/版本/安装态不满足)/
    /// `ocr_model_missing`(档位模型未下载/未就位)/ `ocr_manifest_unready`(下载清单 URL
    /// 未钉定)/ `ocr_invalid_input`(base64/档位参数非法)/ `ocr_decode_failed`(图像读取
    /// /解码失败)/ `ocr_engine_failed`(推理资源超限/内部错误)/ `ocr_worker_failed`
    /// (worker 通路硬止损)。message **不携带绝对路径 / worker 内部错误串**(泄漏面,
    /// 同 Player/Preview/Reveal 姿态)。
    #[error("ocr error [{code}]: {message}")]
    Ocr { code: &'static str, message: String },

    /// 影像增强(降噪/超分子系统,2026-07-24)失败。与 [`AppError::Ocr`] 同姿态:`code` 是稳定
    /// 小写分流标识,原样透到 IPC `code` 字段。稳定码集:`enhance_unlicensed`(插件未激活)/
    /// `enhance_input_too_large`(输入/输出像素超准入门,J-6 草案)/ `enhance_input_unsupported`
    /// (RAW/不可解码格式)/ `enhance_model_missing`(模型未下载/未就位)/
    /// `enhance_manifest_unready`(下载清单尚未钉定)/ `enhance_busy`(队列/worker 忙)/
    /// `enhance_download_failed`(模型下载失败)/ `enhance_io`(读写/落盘失败)/
    /// `enhance_invalid_params`(strength NaN 等参数非法)/ `enhance_not_implemented`(P0 预览占位)。
    /// message **不携带绝对路径 / worker 内部错误串**(泄漏面,同 Ocr/Player/Preview 姿态)。
    #[error("enhance error [{code}]: {message}")]
    Enhance { code: &'static str, message: String },

    /// 精确内容去重错误。`code` 是稳定分流码，message 只使用固定文案，不携带路径、SQL
    /// 或底层 OS 错误；前端据此区分可重试的源漂移、任务繁忙和清理守门失败。
    #[error("dedup error [{code}]: {message}")]
    Dedup { code: &'static str, message: String },

    /// 扫描生命周期错误。`code` 是稳定分流码；数据库清空或其它库级切换期间，前端应等待
    /// 当前操作结束后重试，而不是把它当成文件扫描失败。
    #[error("scan error [{code}]: {message}")]
    Scan { code: &'static str, message: String },

    #[error("Failed to create folder: {0}")]
    CreateFolder(String),

    #[error("Failed to move file: {0}")]
    MoveFile(String),

    #[error("Failed to copy file: {0}")]
    CopyFile(String),

    #[error("Invalid folder move: {0}")]
    InvalidMove(String),

    #[error("Target already has a folder named: {0}")]
    DirectoryExists(String),

    /// 目录移动的半完成状态：物理搬运已落盘，索引未更新或源残留未清。
    ///
    /// 与 [AppError::Relink]/[AppError::Backup] 同姿态：`code` 是稳定分流标识（当前只有
    /// `move_db_pending` = 目标已落盘、库未改，可重试），并**额外**带上 `recovery_id`
    /// 与 `target_abs_path`，使前端能显示文件的真实位置并给出重试入口，而不是只报一句
    /// 「操作失败」——这正是审查 §7.1-B 指出的缺口。message **不携带 SQL / 内部错误串**。
    #[error("move recovery [{code}]: {message}")]
    MoveRecovery {
        code: &'static str,
        message: String,
        recovery_id: i64,
        target_abs_path: String,
    },
}

impl AppError {
    /// 内部错误脱敏包装:`msg` 为对外稳定文案(硬编码,可安全暴露),原始错误仅写日志、不进 IPC 载荷。
    /// 用于 JoinError / keyring / serde 等原始串可能泄露 panic 载荷、凭据或数据形态的场景。
    /// (io::Error 等通用 OS 串低敏,不强制走此路。)
    pub fn internal(msg: impl Into<String>, source: impl std::fmt::Display) -> Self {
        let msg = msg.into();
        tracing::error!("{msg} | {source}");
        AppError::System(msg)
    }

    /// OS/原生 API 错误的脱敏包装：保留既有稳定 code=`Os`，原始错误仅进日志。
    pub fn os(msg: impl Into<String>, source: impl std::fmt::Display) -> Self {
        let msg = msg.into();
        tracing::error!("{msg} | {source}");
        AppError::Os(msg)
    }
}

impl From<crate::dedup::HashError> for AppError {
    fn from(error: crate::dedup::HashError) -> Self {
        let code = error.code();
        Self::Dedup {
            code,
            message: format!("精确摘要失败（{code}） | Exact digest failed ({code})"),
        }
    }
}

/// AI 推理核错误收敛(Part4-T15/T16):scrollery-ai-core 自持 AiError,此处映射回既有
/// 变体,调用点 `?` 传播与前端 IPC code(Ai/AiTokenizer/Internal)完全不变。T16 收口后
/// host 关闭 ai-core 的 `inference` feature(ort 直依赖已拆),AiError 为 non_exhaustive:
/// 通配臂携带字符串进 Ai 变体——workspace feature 并集把 Ort 臂带回来时同样落此臂。
impl From<scrollery_ai_core::AiError> for AppError {
    fn from(e: scrollery_ai_core::AiError) -> Self {
        use scrollery_ai_core::AiError;
        match e {
            AiError::Internal(m) => AppError::Internal(m),
            AiError::Tokenizer(m) => AppError::AiTokenizer(m),
            other => AppError::Ai(other.to_string()),
        }
    }
}

/// A2:config.toml 读写失败(`ConfigManager::set_and_persist`/`reset_key`)统一映射为
/// `AppError::Config { code: "config_write_failed" }`——`ConfigFileError::to_status_message`
/// 已按硬约束脱敏(`Io` 变体不透传原始路径),此处直接复用其输出作为 message。
impl From<crate::config::ConfigFileError> for AppError {
    fn from(e: crate::config::ConfigFileError) -> Self {
        let (message, _line) = e.to_status_message();
        AppError::Config {
            code: "config_write_failed",
            message,
        }
    }
}

/// 同一 (调用点 target, 稳定 code) 签名的重复压缩(方案 §3.4/§9.3-C):30 秒窗口内的重复 error
/// 全部吞掉只计数,窗口首条(或窗口切换后的首条)才真正落盘,防同一错误短时间内刷屏。
/// 定型为本调用点(下方 `Serialize for AppError`)专用的前置辅助函数,不做成通用 Layer——
/// 复用既有 `error!` 宏调用,判断逻辑与「是否落盘」解耦即可,无需在 `on_event` 里重实现过滤。
struct ErrorDedupEntry {
    window_start: std::time::Instant,
    /// 当前窗口内已吞掉(未落盘)的次数。
    swallowed: u32,
}

/// signature = (调用点 target, AppError 稳定 code)。target 恒为本调用点的 `module_path!()`,
/// 随参数传入而非硬编码,为未来若有第二个调用点复用此辅助函数留出余地(方案 §3.2 定型理由)。
static ERROR_LOG_DEDUP: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(&'static str, &'static str), ErrorDedupEntry>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

const ERROR_DEDUP_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);

enum ErrorLogDecision {
    /// 本签名首次出现,直接放行,信封附 `repeat_count=1`。
    EmitFirstSeen,
    /// 窗口内被吞掉过 `swallowed` 次之后,窗口切换的首条放行,信封附 `suppressed=N`。
    EmitAfterSuppression(u32),
    /// 仍在窗口内且非首条:error 档不落盘,但降档以 debug 级留一条全量证据(D-307)。
    Swallow,
}

fn error_log_dedup_check(target: &'static str, code: &'static str) -> ErrorLogDecision {
    let mut map = ERROR_LOG_DEDUP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = std::time::Instant::now();
    match map.get_mut(&(target, code)) {
        None => {
            map.insert(
                (target, code),
                ErrorDedupEntry {
                    window_start: now,
                    swallowed: 0,
                },
            );
            ErrorLogDecision::EmitFirstSeen
        }
        Some(entry) if now.duration_since(entry.window_start) < ERROR_DEDUP_WINDOW => {
            entry.swallowed += 1;
            ErrorLogDecision::Swallow
        }
        Some(entry) => {
            let suppressed = entry.swallowed;
            entry.window_start = now;
            entry.swallowed = 0;
            ErrorLogDecision::EmitAfterSuppression(suppressed)
        }
    }
}

/// `clear_logs` 命令重置去重表(方案 §3.5),避免旧窗口计数影响用户清空日志后的新一轮观感。
pub fn reset_error_log_dedup() {
    ERROR_LOG_DEDUP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// 单个签名的当前压缩快照(方案 §5 P1「压缩计数可见」——日志系统自身可观测)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDedupSnapshotEntry {
    pub target: String,
    pub code: String,
    /// 本窗口内已吞掉(降档 debug 留证、error 档未落盘)的次数。
    pub swallowed_in_window: u32,
    pub window_age_secs: u64,
}

/// 压缩表快照(方案 §5 P1):只列出**当前窗口内已发生过压缩**的签名(`swallowed>0`)——刚见过一次
/// 但从未重复的签名不算噪音,不进快照,避免诊断面板被大量「swallowed=0」的正常签名淹没。
pub fn error_dedup_snapshot() -> Vec<ErrorDedupSnapshotEntry> {
    let map = ERROR_LOG_DEDUP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = std::time::Instant::now();
    map.iter()
        .filter(|(_, entry)| entry.swallowed > 0)
        .map(|((target, code), entry)| ErrorDedupSnapshotEntry {
            target: (*target).to_string(),
            code: (*code).to_string(),
            swallowed_in_window: entry.swallowed,
            window_age_secs: now.duration_since(entry.window_start).as_secs(),
        })
        .collect()
}

/// 沿 `std::error::Error::source()` 链收集每一层的展示文本(方案 §3.3):数组而非 `{:?}` 整段
/// 多行 Caused-by 文本,防撕裂 rg/jq 按行解析(底稿 C §7)。
fn error_chain_strings(err: &dyn std::error::Error) -> Vec<String> {
    let mut chain = vec![err.to_string()];
    let mut current = err;
    while let Some(source) = std::error::Error::source(current) {
        chain.push(source.to_string());
        current = source;
    }
    chain
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let (code, msg) = match self {
            AppError::Io(_) => ("Io", "文件读写异常 | IO error"),
            AppError::Db(_) => ("Db", "数据库访问异常 | Database error"),
            AppError::Pool(_) => ("Pool", "数据库连接池异常 | Connection pool error"),
            AppError::Exif(_) => ("Exif", "照片元数据解析异常 | EXIF parse error"),
            AppError::Xmp(_) => ("Xmp", "XMP 数据解析异常 | XMP parse error"),
            AppError::UnsupportedFormat(m) => ("UnsupportedFormat", m.as_str()),
            AppError::Engine(_) => ("Engine", "图像处理引擎异常 | Image engine error"),
            AppError::PathResolution(m) => ("PathResolution", m.as_str()),
            AppError::FFmpeg(m) => ("FFmpeg", m.as_str()),
            AppError::AudioMetadata(m) => ("AudioMetadata", m.as_str()),
            AppError::DocumentRender(m) => ("DocumentRender", m.as_str()),
            AppError::LayoutNotReady => (
                "LayoutNotReady",
                "布局未就绪，请先计算布局 | Layout cache not ready",
            ),
            AppError::ViewStale => (
                "ViewStale",
                "视图已更新，请重试 | View changed since selection, please retry",
            ),
            // message-passthrough：validate 的固定文案（中文|英文）直接透出，前端按 code 分流。
            AppError::DuplicateLensInvalid(m) => ("DuplicateLensInvalid", m.as_str()),
            AppError::DuplicateLensUnsupported => (
                "DuplicateLensUnsupported",
                "重复镜头 folders 模式解析尚未支持（P3） | Duplicate lens folders mode lowering not supported yet",
            ),
            AppError::ScanRootNotFound(_) => {
                ("ScanRootNotFound", "未找到扫描目录 | Scan root not found")
            }
            AppError::MediaNotFound(_) => {
                ("MediaNotFound", "未找到媒体文件 | Media item not found")
            }
            // message 即卷标签（前端拼「请插入设备 <label>」）——message-passthrough，同 System/Os。
            AppError::VolumeOffline(m) => ("VolumeOffline", m.as_str()),
            AppError::Cancelled => ("Cancelled", "操作已取消 | Operation cancelled"),
            AppError::Ai(_) => ("Ai", "AI 推理异常 | AI inference error"),
            AppError::AiModelNotLoaded(m) => ("AiModelNotLoaded", m.as_str()),
            AppError::AnalysisBusy => (
                "AnalysisBusy",
                "正在等待其他分析完成 | Waiting for another analysis to finish",
            ),
            AppError::System(m) => ("System", m.as_str()),
            AppError::Os(m) => ("Os", m.as_str()),
            AppError::AiTokenizer(m) => ("AiTokenizer", m.as_str()),
            AppError::Internal(m) => ("Internal", m.as_str()),
            // 透出底层稳定 code（而非笼统 "Internal"）：前端按 code 分流 exotic 失败。
            AppError::Exotic { code, message } => (*code, message.as_str()),
            // 同上：reveal 的稳定码（unsupported_platform / reveal_failed）供前端分流。
            AppError::Reveal { code, message } => (*code, message.as_str()),
            // 同上：预览的稳定码（preview_unsupported_type / preview_failed）供前端分流。
            AppError::Preview { code, message } => (*code, message.as_str()),
            // 同上：重链接的稳定码（relink_not_a_dir / relink_path_taken / relink_mismatch）供前端分流。
            AppError::Relink { code, message } => (*code, message.as_str()),
            // 同上：备份的稳定码（backup_dir_not_writable / backup_document_inconsistent / ...）供前端分流。
            AppError::Backup { code, message } => (*code, message.as_str()),
            // 同上：恢复的稳定码（restore_schema_too_new / restore_corrupt / restore_path_invalid / ...）供前端分流。
            AppError::Restore { code, message } => (*code, message.as_str()),
            // 同上：导出的稳定码（export_target_invalid / export_target_inside_library / file_job_busy / ...）供前端分流。
            AppError::Export { code, message } => (*code, message.as_str()),
            // 同上：图片编辑的稳定码（edit_image_too_large / edit_crop_empty / edit_saved_needs_index / ...）供前端分流。
            AppError::Edit { code, message } => (*code, message.as_str()),
            // 同上：配置文件的稳定码（config_invalid_value / config_write_failed / config_open_failed）供前端分流。
            AppError::Config { code, message } => (*code, message.as_str()),
            // 同上：播放器的稳定码（player_subtitle_not_found / player_frame_io / ...）供前端分流。
            AppError::Player { code, message } => (*code, message.as_str()),
            // 同上：查看器色域的稳定码（icc_parse_failed / viewer_render_too_large / ...）供前端分流。
            AppError::Color { code, message } => (*code, message.as_str()),
            // 同上：OCR 的稳定码（ocr_unlicensed / ocr_model_missing / ocr_engine_failed / ...）供前端分流。
            AppError::Ocr { code, message } => (*code, message.as_str()),
            // 同上：影像增强的稳定码（enhance_unlicensed / enhance_input_too_large / enhance_model_missing / ...）供前端分流。
            AppError::Enhance { code, message } => (*code, message.as_str()),
            AppError::Dedup { code, message } => (*code, message.as_str()),
            AppError::Scan { code, message } => (*code, message.as_str()),
            AppError::CreateFolder(m) => ("CreateFolder", m.as_str()),
            AppError::MoveFile(m) => ("MoveFile", m.as_str()),
            AppError::CopyFile(m) => ("CopyFile", m.as_str()),
            AppError::InvalidMove(m) => ("InvalidMove", m.as_str()),
            AppError::DirectoryExists(m) => ("DirectoryExists", m.as_str()),
            // 目录移动半完成：稳定码 move_db_pending（前端据此给「文件已在 X，可重试」的分流）。
            AppError::MoveRecovery { code, message, .. } => (*code, message.as_str()),
        };

        if !matches!(
            self,
            AppError::LayoutNotReady
                | AppError::Cancelled
                | AppError::ViewStale
                | AppError::AnalysisBusy
                // 卷离线可自动恢复（重连即好），属预期分支，不记错误日志（同 Cancelled/ViewStale）。
                | AppError::VolumeOffline(_)
        ) {
            // reviewer 深审修正:先判去重决策再算 chain——Swallow 分支(窗口内多数调用)是最热路径,
            // 不该白付 error_chain_strings + serde_json 序列化的开销;chain 只在真正要落盘的分支现算。
            match error_log_dedup_check(module_path!(), code) {
                ErrorLogDecision::EmitFirstSeen => {
                    let chain_json = serde_json::to_string(&error_chain_strings(self))
                        .unwrap_or_else(|_| "[]".to_string());
                    // 用 serde_json 直接产出合法 JSON 数组文本,经字段传给 tracing 后由 EnvelopeFormat
                    // 的 FieldCollector 按字段名解析回真正的 JSON 数组（而非 `{:?}` 的 Debug 转义文本）。
                    tracing::error!(
                        error_code = code,
                        error_chain = %chain_json,
                        repeat_count = 1u32,
                        "AppError occurred (to frontend): {:?}", self
                    );
                }
                ErrorLogDecision::EmitAfterSuppression(suppressed) => {
                    let chain_json = serde_json::to_string(&error_chain_strings(self))
                        .unwrap_or_else(|_| "[]".to_string());
                    tracing::error!(
                        error_code = code,
                        error_chain = %chain_json,
                        suppressed = suppressed,
                        "AppError occurred (to frontend): {:?}", self
                    );
                }
                ErrorLogDecision::Swallow => {
                    // D-307:被吞条目降档为 debug 而非纯吞——签名 (target,code) 在单一调用点下
                    // 事实上仅按 code 去重,不相关错误撞泛化 code(Io/Db/System 等)会被误吞;
                    // 降档后 error 档保住风暴压缩,排查时切 debug 档即可见全量证据。
                    // tracing 宏先查 callsite enabled 再求值字段表达式,默认 info 档下
                    // chain 序列化不会执行,Swallow 热路径仍只付 ~ns 级检查。
                    tracing::debug!(
                        error_code = code,
                        error_chain = %serde_json::to_string(&error_chain_strings(self))
                            .unwrap_or_else(|_| "[]".to_string()),
                        dedup_swallowed = true,
                        "AppError occurred (to frontend): {:?}", self
                    );
                }
            }
        }

        // MoveRecovery 额外带恢复定位（日志 id + 文件真实位置）；其余变体仍是 code/message 两字段。
        match self {
            AppError::MoveRecovery {
                recovery_id,
                target_abs_path,
                ..
            } => {
                let mut state = serializer.serialize_struct("AppError", 4)?;
                state.serialize_field("code", code)?;
                state.serialize_field("message", msg)?;
                state.serialize_field("recoveryId", recovery_id)?;
                state.serialize_field("targetAbsPath", target_abs_path)?;
                state.end()
            }
            _ => {
                let mut state = serializer.serialize_struct("AppError", 2)?;
                state.serialize_field("code", code)?;
                state.serialize_field("message", msg)?;
                state.end()
            }
        }
    }
}

/// 整个代码库中使用的便捷别名。
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// 锁住 exotic 错误契约：序列化后 IPC `code` 字段必须是底层稳定码（如 "rollback"），
    /// **不得**笼统回退为 "Internal"——这正是「前端按类型分流」赖以成立的前提。
    #[test]
    fn exotic_error_surfaces_stable_code_not_internal() {
        let err = AppError::Exotic {
            code: "rollback",
            message: "Registry 验签/接受失败：rollback".into(),
        };
        let v = serde_json::to_value(&err).expect("serialize AppError::Exotic");
        assert_eq!(v["code"], "rollback", "code 字段须为稳定码而非 Internal");
        assert_eq!(v["message"], "Registry 验签/接受失败：rollback");
    }

    /// 锁住预览错误契约（同 Exotic/Reveal 姿态）：IPC `code` 字段须为稳定码,前端据此分流,
    /// 不得匹配 message 文案。
    #[test]
    fn preview_error_surfaces_stable_code() {
        let err = AppError::Preview {
            code: "preview_unsupported_type",
            message: "扩展名不在预览白名单".into(),
        };
        let v = serde_json::to_value(&err).expect("serialize AppError::Preview");
        assert_eq!(v["code"], "preview_unsupported_type");
    }

    /// 锁住重链接错误契约（#7 方案A）：三个稳定码须原样透出，前端据此分流出不同话术
    /// （路径无效 → 重选；已被占用 → 指向冲突根；抽样不符 → 明确拒绝并解释）。
    /// 同时锁住「不得回退为 InvalidMove/Internal」——那正是本变体存在的理由。
    #[test]
    fn relink_error_surfaces_stable_code() {
        for code in ["relink_not_a_dir", "relink_path_taken", "relink_mismatch"] {
            let err = AppError::Relink {
                code,
                message: "sampled=100 matched=12".into(),
            };
            let v = serde_json::to_value(&err).expect("serialize AppError::Relink");
            assert_eq!(v["code"], code, "relink code 须为稳定码");
        }
    }

    /// 锁住备份错误契约（方案 B §9,同 Relink 姿态）:稳定小写码原样透出供前端分流,
    /// message 展示用。
    #[test]
    fn backup_error_surfaces_stable_code() {
        for code in [
            "backup_dir_not_writable",
            "backup_document_inconsistent",
            "backup_cancelled",
            "backup_io",
            "file_job_busy",
        ] {
            let err = AppError::Backup {
                code,
                message: "备份失败".into(),
            };
            let v = serde_json::to_value(&err).expect("serialize AppError::Backup");
            assert_eq!(v["code"], code, "backup code 须为稳定码");
        }
    }

    /// 锁住查看器色域错误契约(B 线,同 Backup 姿态):稳定小写码原样透出供前端分流。
    #[test]
    fn color_error_surfaces_stable_code() {
        for code in [
            "icc_parse_failed",
            "icc_not_rgb",
            "icc_not_display_class",
            "icc_transform_unsupported",
            "icc_too_large",
            "icc_io",
            "icc_not_found",
            "unsupported_platform",
            "viewer_render_unsupported",
            "viewer_render_decode_failed",
            "viewer_render_too_large",
            "viewer_render_io",
        ] {
            let err = AppError::Color {
                code,
                message: "查看器色域渲染失败".into(),
            };
            let v = serde_json::to_value(&err).expect("serialize AppError::Color");
            assert_eq!(v["code"], code, "color code 须为稳定码");
        }
    }

    /// 锁住恢复错误契约（方案 B §9,同 Backup 姿态）:稳定小写码原样透出供前端分流。
    #[test]
    fn restore_error_surfaces_stable_code() {
        for code in [
            "restore_format_unsupported",
            "restore_schema_too_new",
            "restore_corrupt",
            "restore_path_invalid",
            "restore_size_limit",
            "restore_document_missing",
            "restore_rollback_failed",
        ] {
            let err = AppError::Restore {
                code,
                message: "恢复失败".into(),
            };
            let v = serde_json::to_value(&err).expect("serialize AppError::Restore");
            assert_eq!(v["code"], code, "restore code 须为稳定码");
        }
    }

    /// 锁住 OCR 错误契约（T7,同 Player/Backup 姿态）:九个稳定码全遍历，序列化后 IPC
    /// `code` 字段须原样透出，前端据此分流（门控三态 + 模型/清单未就绪 + 输入/推理/worker 失败）。
    #[test]
    fn ocr_error_surfaces_stable_code() {
        for code in [
            "ocr_unlicensed",
            "ocr_license_expired",
            "ocr_unavailable",
            "ocr_model_missing",
            "ocr_manifest_unready",
            "ocr_invalid_input",
            "ocr_decode_failed",
            "ocr_engine_failed",
            "ocr_worker_failed",
        ] {
            let err = AppError::Ocr {
                code,
                message: "OCR 失败".into(),
            };
            let v = serde_json::to_value(&err).expect("serialize AppError::Ocr");
            assert_eq!(v["code"], code, "ocr code 须为稳定码");
        }
    }

    /// 锁住影像增强错误契约（降噪/超分子系统,同 Ocr 姿态）:稳定小写码原样透出供前端分流。
    #[test]
    fn enhance_error_surfaces_stable_code() {
        for code in [
            "enhance_unlicensed",
            "enhance_input_too_large",
            "enhance_input_unsupported",
            "enhance_model_missing",
            "enhance_manifest_unready",
            "enhance_busy",
            "enhance_download_failed",
            "enhance_io",
            "enhance_invalid_params",
            "enhance_not_implemented",
        ] {
            let err = AppError::Enhance {
                code,
                message: "影像增强失败".into(),
            };
            let v = serde_json::to_value(&err).expect("serialize AppError::Enhance");
            assert_eq!(v["code"], code, "enhance code 须为稳定码");
        }
    }

    /// 对照：泛化 Internal 仍序列化为 "Internal"（确认新变体未污染既有契约）。
    #[test]
    fn internal_error_still_serializes_as_internal() {
        let v = serde_json::to_value(AppError::Internal("x".into())).unwrap();
        assert_eq!(v["code"], "Internal");
    }

    /// 定向脱敏 helper 只把固定文案送进 IPC；底层错误（可能含路径/panic 载荷）只记日志。
    #[test]
    fn sanitized_helpers_do_not_serialize_source_text() {
        let source = "C:/Users/private/secret.txt: panic payload";
        let system = serde_json::to_value(AppError::internal("内部任务失败", source)).unwrap();
        assert_eq!(system["code"], "System");
        assert_eq!(system["message"], "内部任务失败");
        assert!(!system.to_string().contains("private"));

        let os = serde_json::to_value(AppError::os("系统接口失败", source)).unwrap();
        assert_eq!(os["code"], "Os");
        assert_eq!(os["message"], "系统接口失败");
        assert!(!os.to_string().contains("private"));
    }

    /// 锁住 T18 选择守门契约：ViewStale 序列化后 code 必须是稳定 "ViewStale"，前端据此重算 layout 重发。
    #[test]
    fn view_stale_surfaces_stable_code() {
        let v = serde_json::to_value(AppError::ViewStale).unwrap();
        assert_eq!(v["code"], "ViewStale");
    }

    /// 锁住 T13 离线契约：VolumeOffline 序列化后 code 稳定为 "VolumeOffline"，
    /// message 即卷标签（前端据此弹「请插入设备 <label>」而非破图）。
    #[test]
    fn volume_offline_surfaces_stable_code_and_label() {
        let v = serde_json::to_value(AppError::VolumeOffline("我的移动硬盘".into())).unwrap();
        assert_eq!(v["code"], "VolumeOffline");
        assert_eq!(v["message"], "我的移动硬盘");
    }

    #[test]
    fn scan_lifecycle_surfaces_stable_code() {
        let v = serde_json::to_value(AppError::Scan {
            code: "scan_database_clearing",
            message: "retry".into(),
        })
        .unwrap();
        assert_eq!(v["code"], "scan_database_clearing");
        assert_eq!(v["message"], "retry");
    }

    /// 锁住 error.chain 收集逻辑(方案 §3.3 S2):自身展示文本打头,沿 source() 链逐层追加,
    /// 而非只取自身 `{:?}` 整段文本。
    #[test]
    fn error_chain_strings_collects_source_chain() {
        let io_err = std::io::Error::other("disk full");
        let app_err = AppError::Io(io_err);
        let chain = error_chain_strings(&app_err);
        assert_eq!(
            chain.len(),
            2,
            "AppError::Io 的 chain 应为[自身, 内层 io::Error]两层: {chain:?}"
        );
        assert!(
            chain[0].contains("IO error"),
            "首层应为 AppError 自身展示文本: {chain:?}"
        );
        assert_eq!(chain[1], "disk full", "第二层应为内层 io::Error 的展示文本");
    }

    /// 锁住重复压缩状态机(方案 §9.3-C):首见放行、窗口内吞掉、窗口外首条放行且附正确 suppressed 计数。
    /// 用测试专属 code(不与其它用例共享签名)规避 ERROR_LOG_DEDUP 全局表的跨用例串扰。
    #[test]
    fn error_log_dedup_first_seen_then_swallow_then_reopen() {
        let target = "test::dedup_only";
        let code = "TestDedupSig1";

        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::EmitFirstSeen
        ));
        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::Swallow
        ));
        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::Swallow
        ));

        // 手动把窗口开始时间往回拨,模拟「窗口已过」而不必真的 sleep 30 秒。
        {
            let mut map = ERROR_LOG_DEDUP.lock().unwrap_or_else(|p| p.into_inner());
            let entry = map
                .get_mut(&(target, code))
                .expect("entry must exist after first_seen");
            entry.window_start -= ERROR_DEDUP_WINDOW + std::time::Duration::from_secs(1);
        }

        match error_log_dedup_check(target, code) {
            ErrorLogDecision::EmitAfterSuppression(suppressed) => {
                assert_eq!(suppressed, 2, "窗口内吞掉的 2 条应计入 suppressed");
            }
            other => panic!(
                "窗口过后应放行且附 suppressed,实得: {:?}",
                std::mem::discriminant(&other)
            ),
        }

        // 新窗口重新起算:马上再来一条应被吞(而非又被当成 EmitFirstSeen 或 EmitAfterSuppression)。
        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::Swallow
        ));
    }

    /// 锁住压缩快照(方案 §5 P1):首见(swallowed=0)不进快照,吞过至少一次后才出现且计数正确;
    /// 未出现过的签名不在快照里。测试专属 code 规避全局表跨用例串扰。
    #[test]
    fn error_dedup_snapshot_only_lists_signatures_with_swallowed_repeats() {
        let target = "test::dedup_snapshot";
        let code = "TestDedupSnapshotSig1";

        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::EmitFirstSeen
        ));
        assert!(
            !error_dedup_snapshot()
                .iter()
                .any(|e| e.target == target && e.code == code),
            "首见后从未重复的签名不应出现在快照里"
        );

        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::Swallow
        ));
        assert!(matches!(
            error_log_dedup_check(target, code),
            ErrorLogDecision::Swallow
        ));

        let snapshot = error_dedup_snapshot();
        let entry = snapshot
            .iter()
            .find(|e| e.target == target && e.code == code)
            .expect("吞过两条后应出现在快照里");
        assert_eq!(entry.swallowed_in_window, 2);
    }

    /// S2 DoD(方案 §9.4):发一个真实 AppError 事件,解析落盘 JSONL,断言信封六字段齐全 +
    /// error.chain 是数组而非 `{:?}` 整段撕裂文本(方案 §3.3)。
    #[test]
    fn app_error_serialize_emits_full_envelope_with_error_chain_array() {
        use tracing_subscriber::layer::SubscriberExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::NEVER)
            .filename_prefix("apperr-test")
            .filename_suffix("log")
            .build(dir.path())
            .expect("build rolling appender");
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);

        let env_filter = tracing_subscriber::EnvFilter::new("info");
        let (filter, _handle) = tracing_subscriber::reload::Layer::new(env_filter);
        let session_id: std::sync::Arc<str> = std::sync::Arc::from("s-test");
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(crate::logging::EnvelopeFormat { session_id })
            .with_ansi(false)
            .with_writer(non_blocking);
        let subscriber = tracing_subscriber::registry().with(filter).with(file_layer);

        // 用测试专属 code 的 AppError 变体规避 dedup 表跨用例串扰(exotic code 全局唯一)。
        let err = AppError::Exotic {
            code: "envelope_test_only",
            message: "disk full".to_string(),
        };

        tracing::subscriber::with_default(subscriber, || {
            let _ = serde_json::to_value(&err).expect("serialize AppError::Exotic");
        });
        drop(guard);

        let log_path = dir.path().join("apperr-test.log");
        let content = std::fs::read_to_string(&log_path).expect("read log file");
        let line = content
            .lines()
            .find(|l| !l.is_empty())
            .expect("at least one log line");
        let v: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");

        for field in [
            "ts",
            "level",
            "target",
            "session_id",
            "operation_id",
            "msg",
            "attributes",
        ] {
            assert!(v.get(field).is_some(), "信封字段缺失: {field} ({line})");
        }
        assert_eq!(v["level"], "ERROR");
        assert_eq!(v["attributes"]["error.code"], "envelope_test_only");
        let chain = v["attributes"]["error.chain"]
            .as_array()
            .unwrap_or_else(|| panic!("error.chain 应为数组而非字符串: {line}"));
        assert!(!chain.is_empty(), "chain 不应为空");
        assert!(
            chain.iter().all(|e| e.is_string()),
            "chain 每项应为字符串: {line}"
        );
    }

    /// 锁住 D-307:窗口内被吞条目降档 debug 留证——debug 档下可见(带 dedup_swallowed 标记 +
    /// error.chain 数组),info 档下不产生任何行(error 档风暴压缩不回潮)。
    #[test]
    fn swallowed_repeat_demoted_to_debug_level() {
        use tracing_subscriber::layer::SubscriberExt;

        // 单次搭台:tempdir + 指定档位 EnvFilter,serialize 同一 code 两次,返回落盘全部行。
        fn emit_twice_and_collect(
            filter_directive: &str,
            code: &'static str,
        ) -> Vec<serde_json::Value> {
            let dir = tempfile::tempdir().expect("tempdir");
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::NEVER)
                .filename_prefix("swallow-test")
                .filename_suffix("log")
                .build(dir.path())
                .expect("build rolling appender");
            let (non_blocking, guard) = tracing_appender::non_blocking(appender);

            let env_filter = tracing_subscriber::EnvFilter::new(filter_directive);
            let (filter, _handle) = tracing_subscriber::reload::Layer::new(env_filter);
            let session_id: std::sync::Arc<str> = std::sync::Arc::from("s-test");
            let file_layer = tracing_subscriber::fmt::layer()
                .event_format(crate::logging::EnvelopeFormat { session_id })
                .with_ansi(false)
                .with_writer(non_blocking);
            let subscriber = tracing_subscriber::registry().with(filter).with(file_layer);

            let err = AppError::Exotic {
                code,
                message: "disk full".to_string(),
            };
            tracing::subscriber::with_default(subscriber, || {
                let _ = serde_json::to_value(&err).expect("serialize first");
                let _ = serde_json::to_value(&err).expect("serialize second");
            });
            drop(guard);

            let content = std::fs::read_to_string(dir.path().join("swallow-test.log"))
                .expect("read log file");
            content
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| serde_json::from_str(l).expect("valid JSON line"))
                .collect()
        }

        // debug 档:第 1 条 ERROR(首见),第 2 条 DEBUG(降档留证)。code 测试专属防 dedup 表串扰。
        let lines = emit_twice_and_collect("debug", "swallow_debug_test_only");
        assert_eq!(lines.len(), 2, "debug 档应落 2 行: {lines:?}");
        assert_eq!(lines[0]["level"], "ERROR");
        assert_eq!(lines[0]["attributes"]["repeat_count"], 1);
        assert_eq!(lines[1]["level"], "DEBUG");
        assert_eq!(lines[1]["attributes"]["dedup_swallowed"], true);
        assert_eq!(
            lines[1]["attributes"]["error.code"],
            "swallow_debug_test_only"
        );
        assert!(
            lines[1]["attributes"]["error.chain"].is_array(),
            "降档行同样带 error.chain 数组: {:?}",
            lines[1]
        );

        // info 档:仅首见 1 行,被吞条目不落盘(debug 降档行被档位滤掉)。
        let lines = emit_twice_and_collect("info", "swallow_info_test_only");
        assert_eq!(lines.len(), 1, "info 档应仅落首见 1 行: {lines:?}");
        assert_eq!(lines[0]["level"], "ERROR");
    }
}
