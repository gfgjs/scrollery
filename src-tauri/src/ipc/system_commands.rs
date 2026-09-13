//! 系统级命令（§ 6.1 — 系统）。

use std::sync::Arc;

use serde::Deserialize;
use tauri::{Manager, State};
use tracing::info;

use super::blocking::read_blocking;
use super::reveal::reveal_path;
use crate::db::queries::get_item_path_info;
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::utils::path::resolve_media_path;

/// 在系统文件管理器中显示媒体项（选中该项）。
///
/// 实现统一走 [`crate::ipc::reveal::reveal_path`]（F-015）。此前这里手写三平台
/// `Command::spawn`，与 `tree_commands::reveal_tree_entry` 并存为两套，且行为不等价 ——
/// Linux 分支只 `xdg-open` 父目录、**不选中**目标文件。统一后 Linux 走 D-Bus
/// `FileManager1.ShowItems`，与 Windows/macOS 一致。
///
/// 顺带的行为变化：旧的 `.spawn()` 发射后不管，打开器起不来也返回 `Ok`；opener 同步等结果，
/// 故本命令现在**可能真的返回错误**（映射为稳定码 `reveal_failed` / `unsupported_platform`）。
///
/// 路径由 DB 行拼出、不经 WebView，故走 `resolve_media_path` 即可，无需 `resolve_within_root`
/// 的根边界校验（那是给 WebView 传入的 `rel_path` 用的）。
#[tauri::command]
pub async fn show_in_explorer(item_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    let (root, rel, name) = read_blocking(&state, move |c| get_item_path_info(c, item_id)).await?;
    // resolve_media_path 返回正斜杠字符串；Windows 上 opener 内部先过 GetFullPathNameW
    // 规范化分隔符再 ILCreateFromPathW，故无需像旧代码那样手动 replace('/', "\\")。
    let abs_path = resolve_media_path(&root, &rel, &name);
    info!("show_in_explorer: {abs_path} | 在资源管理器中显示: {abs_path}");
    reveal_path(std::path::PathBuf::from(abs_path)).await
}

/// 校验:path 必须解析为一个**存在的真实目录**,不接受任意字符串(2026-07-16 安全审查 #5)。
/// 此前 `open_directory` 零校验直呼三家 OS 打开器——它们都会把「不是真实文件系统路径的形态」
/// 转发给已注册的协议处理器:`explorer.exe http://evil` 会启动默认浏览器打开该 URL,
/// `open`/`xdg-open` 同理可被指向任意已注册 URL scheme handler。canonicalize + is_dir 同时挡三类:
///   ① URL(http://…、ms-settings:…等):不是文件系统路径,canonicalize 直接失败;
///   ② 不存在的路径:同样 canonicalize 失败;
///   ③ 存在但是文件而非目录(如可执行文件):canonicalize 成功但 is_dir() 为假,单独拒。
/// canonicalize 顺带解析符号链接、判的是链接指向的真实位置,不被链接伪装绕过。
///
/// 同步阻塞 IO,抽成独立函数以便单测(命令体不便测——会真的 spawn 系统打开器弹窗)。
fn validate_openable_dir(path: &str) -> Result<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| AppError::PathResolution("目录不存在 | directory does not exist".into()))?;
    if !canonical.is_dir() {
        return Err(AppError::PathResolution(
            "目标不是目录 | target is not a directory".into(),
        ));
    }
    // dunce:Windows 上 std::fs::canonicalize 返回 \\?\ 前缀的 verbatim 路径,explorer.exe 接
    // 这种形态时行为不稳(部分版本报「找不到」);dunce 只做「能安全去前缀就去」,非 Windows 平台
    // 是恒等函数,故此调用无需 cfg 分叉。
    Ok(dunce::simplified(&canonical).to_path_buf())
}

/// 在操作系统文件资源管理器中打开任意目录。
#[tauri::command]
pub async fn open_directory(path: String) -> Result<()> {
    // 离开 tokio worker(与本文件其余磁盘 IO 命令一致的 spawn_blocking 取舍)。
    let canonical = tokio::task::spawn_blocking(move || validate_openable_dir(&path))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
    let path = canonical.to_string_lossy().to_string();

    info!("open_directory: {path} | 打开目录: {path}");

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(AppError::from)?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(AppError::from)?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(AppError::from)?;
    }

    Ok(())
}

// 2026-07-10 审查 B18:删除僵尸命令 move_to_trash——前端从未调用(仅常量登记),实现是
// 「软删 stub 却不 bump_data_version」的潜伏雷,一旦被接线即产生陈旧视图 bug。软删正路
// 走 media_commands::delete_media_items(带 bump);真回收站语义留待需求出现时重建。

/// 记录前端存活心跳，供主窗口关闭拦截器判断 WebView 是否还能消费关闭事件。
#[tauri::command]
pub async fn frontend_heartbeat() {
    crate::lifecycle::mark_frontend_heartbeat();
}

/// 清除所有日志文件。
#[tauri::command]
pub async fn clear_logs(state: State<'_, Arc<AppState>>) -> Result<()> {
    let log_dir = &state.log_dir;
    if log_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(log_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "log") {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
    // 重置 error_log_dedup_check 的重复压缩窗口(方案 §3.5),避免清空前的旧窗口计数污染清空后首条日志。
    crate::error::reset_error_log_dedup();
    tracing::info!("Logs cleared by user | 用户清除了日志文件");
    Ok(())
}

/// 单条前端日志事件(日志能力重构 S3,方案 §4/§9.4)。字段对应 `src/utils/logger.ts` 的 `LogEvent`。
///
/// `target` 不可携带(tracing target 是编译期 `&'static str`,不能塞运行时字符串 ——
/// derive/pipeline.rs 已因同一约束改为分支各写字面量,S2 教训)。前端日志统一挂固定
/// `target: "scrollery::frontend"`,来源用 `source` 字段区分(如 `window.onerror`/组件名)。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEvent {
    /// `debug`/`info`/`warn`/`error`;未识别值按 `info` 处理。
    pub level: String,
    pub msg: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    /// 事件来源:组件/模块名,或 `window.onerror`/`unhandledrejection` 等全局兜底标识。
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub col: Option<u32>,
    #[serde(default)]
    pub stack: Option<String>,
    /// 调用方附带的自由结构化上下文(`logger.debug/info/warn/error(msg, fields?)` 的 `fields`)。
    /// 经 `logging.rs` 的 `FieldCollector` 特判解回真对象,落 `attributes.context`(而非转义字符串),
    /// 与 `error_chain`/`error_code` 同一惯例(方案 §3.3)。
    #[serde(default)]
    pub fields: Option<serde_json::Value>,
}

/// 把单条前端事件按 `level` 分发到对应 tracing 宏(方案 §3.3「任务级/事件级」惯例:前端桥不做
/// 聚合,一比一转发)。`Option<T>` 字段是 tracing 合法 `Value`(为 `None` 时该 field 不落信封,
/// 不会在 JSONL 里留 `null`/空字符串噪声),与 derive/pipeline.rs 的 `operation_id` 用法同构。
fn emit_frontend_log_event(ev: &FrontendLogEvent) {
    let fields_json = ev.fields.as_ref().map(|v| v.to_string());
    macro_rules! emit {
        ($lvl:ident) => {
            tracing::$lvl!(
                target: "scrollery::frontend",
                operation_id = ev.operation_id.as_deref(),
                source = ev.source.as_deref(),
                url = ev.url.as_deref(),
                line = ev.line,
                col = ev.col,
                stack = ev.stack.as_deref(),
                frontend_context = fields_json.as_deref(),
                "{}",
                ev.msg
            )
        };
    }
    match ev.level.as_str() {
        "debug" => emit!(debug),
        "warn" => emit!(warn),
        "error" => emit!(error),
        _ => emit!(info),
    }
}

/// 批量接收前端日志事件(方案 §4 L5 前端桥):`src/utils/logger.ts` 的队列按 2s/50 条阈值或
/// onerror/unhandledrejection 立即触发,单一 IPC command 汇入即可复用文件/UI/压缩治理全部既有 Layer。
/// 不做批级早退——`off` 档的丢弃在前端侧完成(`logger.ts` 读 `configStore.logLevel`,off 时直接不入队,
/// 不白付这趟 IPC),后端收到即视为「值得记」,照单全收走既有 EnvFilter 过滤路径。
#[tauri::command]
pub async fn log_frontend_events(events: Vec<FrontendLogEvent>) -> Result<()> {
    for ev in &events {
        emit_frontend_log_event(ev);
    }
    Ok(())
}

/// 明确退出应用程序。
#[tauri::command]
pub async fn exit_app(app: tauri::AppHandle) {
    tracing::info!(
        "exit_app called from frontend, terminating process. | 前端调用了 exit_app，正在终止进程。"
    );
    app.exit(0);
}

/// 隐藏主窗口（最小化到托盘）。
#[tauri::command]
pub async fn hide_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// 将媒体项设置为桌面壁纸。
#[tauri::command]
pub async fn set_as_wallpaper(item_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    let (root, rel, name) = read_blocking(&state, move |c| get_item_path_info(c, item_id)).await?;
    let abs_path = resolve_media_path(&root, &rel, &name);
    info!("set_as_wallpaper: {abs_path} | 设为壁纸: {abs_path}");

    // 壁纸设置是系统调用 + 文件 IO，同样离开 tokio worker（R1-3 顺带）。
    tokio::task::spawn_blocking(move || -> Result<()> {
        wallpaper::set_from_path(&abs_path)
            .map_err(|e| AppError::os("设置壁纸失败 | Failed to set wallpaper", e))?;
        wallpaper::set_mode(wallpaper::Mode::Crop)
            .map_err(|e| AppError::os("设置壁纸模式失败 | Failed to set wallpaper mode", e))?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 复制媒体项图像到系统剪贴板。
#[tauri::command]
pub async fn copy_image_to_clipboard(item_id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    let (root, rel, name) = read_blocking(&state, move |c| get_item_path_info(c, item_id)).await?;
    let abs_path = resolve_media_path(&root, &rel, &name);
    info!("copy_image_to_clipboard: {abs_path} | 复制图像到剪贴板: {abs_path}");

    // 全尺寸解码 + 剪贴板写入是重 CPU/IO —— 整段离开 tokio worker（R1-3 顺带，
    // 此前大图在 async 线程上解码会卡住并发 IPC 数百毫秒级）。
    tokio::task::spawn_blocking(move || -> Result<()> {
        let img = image::open(&abs_path).map_err(AppError::Engine)?;
        let rgba = img.into_rgba8();
        let (width, height) = rgba.dimensions();
        let img_data = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Borrowed(rgba.as_raw()),
        };

        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| AppError::os("初始化剪贴板失败 | Failed to initialize clipboard", e))?;
        clipboard
            .set_image(img_data)
            .map_err(|e| AppError::os("写入剪贴板失败 | Failed to set clipboard image", e))?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法目录 → 放行,且返回值就是（去 UNC 前缀后的）真实路径,不是原样回声。
    #[test]
    fn validate_openable_dir_accepts_real_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let result = validate_openable_dir(dir.path().to_str().expect("utf8 path"));
        assert!(
            result.is_ok(),
            "real directory must be accepted: {result:?}"
        );
        assert!(result.unwrap().is_dir());
    }

    /// 不存在的路径 → 拒(canonicalize 失败),这条同时挡住 URL scheme
    /// （http://…、ms-settings:… 都不是真实文件系统路径,同样在此落网）。
    #[test]
    fn validate_openable_dir_rejects_nonexistent_path() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let missing = dir.path().join("does-not-exist");
        let err = validate_openable_dir(missing.to_str().expect("utf8 path"))
            .expect_err("nonexistent path must be rejected");
        assert!(matches!(err, AppError::PathResolution(_)));
    }

    /// URL scheme 字符串直接判不存在 —— 补一条不依赖临时目录布局的独立断言,
    /// 钉住「防 URL scheme 转发给系统协议处理器」这条本安全项的核心动机。
    #[test]
    fn validate_openable_dir_rejects_url_scheme() {
        let err = validate_openable_dir("http://evil.example/payload")
            .expect_err("URL scheme is not a filesystem path");
        assert!(matches!(err, AppError::PathResolution(_)));
    }

    /// 存在但是**文件**（如可执行文件）→ 单独拒绝,不能靠 canonicalize 成功就放行。
    #[test]
    fn validate_openable_dir_rejects_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("not-a-directory.txt");
        std::fs::write(&file_path, b"x").expect("write temp file");
        let err = validate_openable_dir(file_path.to_str().expect("utf8 path"))
            .expect_err("a file must not be accepted as an openable directory");
        assert!(matches!(err, AppError::PathResolution(_)));
    }

    /// 前端日志桥(方案 §4/§9.4 S3):batched 事件按 level 分发,信封字段 + `context`/`operation_id`
    /// 落盘齐全;`fields=None`/未设置的可选字段不应在 attributes 里留 null/空字符串噪声
    /// (方案 §3.3「丢弃可见化」精神的镜像 —— 该有的不缺,不该有的不凑)。
    #[test]
    fn emit_frontend_log_event_writes_envelope_and_context() {
        use tracing_subscriber::layer::SubscriberExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::NEVER)
            .filename_prefix("frontend-test")
            .filename_suffix("log")
            .build(dir.path())
            .expect("build rolling appender");
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);

        let env_filter = tracing_subscriber::EnvFilter::new("info");
        let session_id: Arc<str> = Arc::from("s-test");
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(crate::logging::EnvelopeFormat { session_id })
            .with_ansi(false)
            .with_writer(non_blocking);
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer);

        tracing::subscriber::with_default(subscriber, || {
            emit_frontend_log_event(&FrontendLogEvent {
                level: "error".to_string(),
                msg: "boom".to_string(),
                operation_id: Some("op-1".to_string()),
                source: Some("window.onerror".to_string()),
                url: Some("app://index.html".to_string()),
                line: Some(12),
                col: Some(3),
                stack: Some("Error: boom\n  at x".to_string()),
                fields: Some(serde_json::json!({"componentName": "MediaGrid"})),
            });
            emit_frontend_log_event(&FrontendLogEvent {
                level: "info".to_string(),
                msg: "plain".to_string(),
                operation_id: None,
                source: None,
                url: None,
                line: None,
                col: None,
                stack: None,
                fields: None,
            });
        });
        drop(guard);

        let content =
            std::fs::read_to_string(dir.path().join("frontend-test.log")).expect("read log file");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "两条事件都应落盘: {content}");

        let first: serde_json::Value =
            serde_json::from_str(lines[0]).expect("first line is valid JSON");
        assert_eq!(first["level"], "ERROR");
        assert_eq!(first["target"], "scrollery::frontend");
        assert_eq!(first["msg"], "boom");
        assert_eq!(first["operation_id"], "op-1");
        assert_eq!(first["attributes"]["source"], "window.onerror");
        assert_eq!(first["attributes"]["url"], "app://index.html");
        assert_eq!(first["attributes"]["line"], 12);
        assert_eq!(first["attributes"]["col"], 3);
        assert_eq!(first["attributes"]["context"]["componentName"], "MediaGrid");

        let second: serde_json::Value =
            serde_json::from_str(lines[1]).expect("second line is valid JSON");
        assert_eq!(second["level"], "INFO");
        assert!(
            second["operation_id"].is_null(),
            "无 operationId 时信封字段应为 null(未记录): {second}"
        );
        assert!(
            second["attributes"].get("context").is_none(),
            "fields=None 不应在 attributes 里留 context 键: {second}"
        );
        assert!(
            second["attributes"].get("source").is_none(),
            "未设置的可选字段不应出现在 attributes: {second}"
        );
    }
}
