//! 独立日志窗口命令(日志能力重构 S4,方案 §5/§9.4 S4)。
//! 窗口生命周期(open_log_window)+ 历史 JSONL 按日分页读取(list_log_files/read_log_file_page)。
//! 实时流数据源是 logging.rs::LogRingBuffer,经 lib.rs 的 100ms 抽干任务 emit("log:batch",..) 推送,
//! 不在本文件——本文件只管「开窗」与「翻旧账」。

use std::sync::Arc;

use serde::Serialize;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::error::{AppError, Result};
use crate::state::AppState;

/// 打开(或聚焦既有)独立日志窗口。订阅标志的翻转是本命令的**副作用**而非单独一步:建窗即订阅
/// (方案 §9.3-D「标志由日志窗口 open/close 的 IPC command 翻转」——这里的「open/close」取窗口
/// 生命周期本身,而非窗口内容再发一轮 IPC:`WindowEvent::Destroyed` 回调覆盖一切关闭路径〔原生
/// 关闭按钮/Alt+F4/程序化关闭〕,比指望前端在卸载竞态里可靠地补一次 IPC 更稳(同 §9.2 陷阱表
/// 「靠 beforeunload flush」的教训——异步收尾在页面销毁面前不可靠)。
#[tauri::command]
pub async fn open_log_window(app: tauri::AppHandle, state: State<'_, Arc<AppState>>) -> Result<()> {
    if let Some(win) = app.get_webview_window("logs") {
        win.show()
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
        win.set_focus()
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
        return Ok(());
    }

    let ring = state.log_ring.clone();
    // 先以隐藏态建窗:窗口几何要先按 config.toml 的记录落位,再展示——否则会先闪一下创建默认
    // 尺寸的位置。恢复逻辑与主窗口共用同一套(缺记录或离屏时回该窗口的创建默认值并居中)。
    let win = WebviewWindowBuilder::new(&app, "logs", WebviewUrl::App("index.html".into()))
        .title("Scrollery — Logs")
        .inner_size(960.0, 640.0)
        .min_inner_size(640.0, 420.0)
        .visible(false)
        .build()
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    crate::config::window::restore_before_show(&app, crate::config::window::LOGS_LABEL);
    win.show()
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    win.set_focus()
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;

    ring.set_subscribed(true);
    win.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            ring.set_subscribed(false);
        }
    });

    Ok(())
}

/// 单个日志文件的元信息(方案 §5 MVP「历史:读 JSONL 文件按日分页加载」的目录列表)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    pub name: String,
    pub size_bytes: u64,
    /// 最后修改时间(unix 毫秒)——前端按此排序/展示,不依赖文件名必然含日期。
    pub modified_ms: i64,
}

/// 列出日志目录下全部 `.log` 文件,按修改时间**降序**(最新在前,匹配「默认看最近」的浏览习惯)。
#[tauri::command]
pub async fn list_log_files(state: State<'_, Arc<AppState>>) -> Result<Vec<LogFileInfo>> {
    let log_dir = state.log_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<LogFileInfo>> {
        let mut files = Vec::new();
        let entries = match std::fs::read_dir(&log_dir) {
            Ok(e) => e,
            Err(_) => return Ok(files),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "log") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            files.push(LogFileInfo {
                name: name.to_string(),
                size_bytes: meta.len(),
                modified_ms,
            });
        }
        files.sort_by_key(|f| std::cmp::Reverse(f.modified_ms));
        Ok(files)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilePage {
    /// 每行已解析为 JSON 值,按文件内原始顺序(旧→新)排列。
    pub lines: Vec<serde_json::Value>,
    pub total_lines: usize,
    /// 是否还有更早的行(前端"加载更早"按钮的显隐依据)。
    pub has_more_older: bool,
    /// 本页最旧一行在文件内的绝对行号(0-based,旧→新序)。前端把它原样带回下一次
    /// `before_line` 请求「更早一页」——见 `read_log_file_page` 文档,防止文件持续被写入时
    /// 分页锚点跟着"当前末尾"漂移导致跨页重叠/丢行。
    pub oldest_loaded_line: usize,
}

/// 按绝对行号锚点算出本页的 `[start, end)` 切片边界(方案 §5,reviewer 深审 2026-07-20 修复)。
/// 抽成纯函数以便单测「文件在两次调用之间被追加写入」场景不致跨页重叠——见 `read_log_file_page` 文档。
fn slice_page_bounds(
    total_lines: usize,
    before_line: Option<usize>,
    limit: usize,
) -> (usize, usize) {
    let end = before_line.unwrap_or(total_lines).min(total_lines);
    let start = end.saturating_sub(limit);
    (start, end)
}

/// 校验 `file_name` 解析后仍落在 `log_dir` 内(防目录穿越),且必须是已存在的普通文件。
/// 同 `system_commands::validate_openable_dir` 的姿态,只是这里校验的是文件而非目录。
fn resolve_log_file(log_dir: &std::path::Path, file_name: &str) -> Result<std::path::PathBuf> {
    let candidate = log_dir.join(file_name);
    let canonical_dir = std::fs::canonicalize(log_dir).map_err(|_| {
        AppError::PathResolution("日志目录不存在 | log directory does not exist".into())
    })?;
    let canonical_file = std::fs::canonicalize(&candidate)
        .map_err(|_| AppError::PathResolution("日志文件不存在 | log file does not exist".into()))?;
    if !canonical_file.starts_with(&canonical_dir) {
        return Err(AppError::PathResolution(
            "非法文件名 | file name resolves outside the log directory".into(),
        ));
    }
    if !canonical_file.is_file() {
        return Err(AppError::PathResolution(
            "目标不是文件 | target is not a file".into(),
        ));
    }
    Ok(canonical_file)
}

/// 把一行原始文本解析为 JSON 值;解析失败(如本次重构落地前的旧纯文本日志行)时降级为一条
/// 合成信封,`msg` 携带原始文本——不让史前日志行直接让历史视图崩掉或整页报错。
fn parse_log_line(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|_| {
        serde_json::json!({
            "ts": "", "level": "INFO", "target": "", "session_id": null,
            "operation_id": null, "msg": raw, "attributes": {},
        })
    })
}

/// 读取单个日志文件的一页(方案 §5 MVP)。
/// `before_line`:`None` = 从文件**当前末尾**取最后 `limit` 行(仅首次打开用,匹配「先看最新」的
/// 默认视角);`Some(n)` = 取绝对行号 `[n-limit, n)` 的那一页(`n` 必须是上一页响应里的
/// `oldest_loaded_line` 原样带回)。
///
/// **为何不用「距末尾偏移量」**(旧设计,reviewer 深审 2026-07-20 指出的真实 bug):被浏览的往往是
/// `list_log_files` 排序里最新、很可能仍在持续写入的当日文件——若偏移量按"每次重读时的当前末尾"
/// 计算,两次分页调用之间新追加的 Δ 行会让下一页窗口整体前移 Δ,与上一页产生 Δ 行重叠(前端
/// `loadOlderHistory` 用 concat 累加、不去重,历史视图出现重复行)。改成绝对行号锚点后,一旦首页
/// 确定了 `oldest_loaded_line`,后续每页都钉在这个固定坐标上,不随文件持续增长漂移。
///
/// 整文件读入内存后按行切片——单日文件受 §3.5 保留策略(14 天 + 512MB 总量兜底)约束,MVP 简化
/// 实现;若未来出现超大单日文件的真实痛点,再改为流式/索引读取(P2 分析进阶范畴)。
#[tauri::command]
pub async fn read_log_file_page(
    state: State<'_, Arc<AppState>>,
    file_name: String,
    before_line: Option<usize>,
    limit: usize,
) -> Result<LogFilePage> {
    // span 埋点(W1,D-312 info 档:整文件读入内存后切片,真实 IO/CPU 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:read_log_file_page");
    let log_dir = state.log_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<LogFilePage> {
        let path = resolve_log_file(&log_dir, &file_name)?;
        let content = std::fs::read_to_string(&path)?;
        let all_lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        let total_lines = all_lines.len();
        let (start, end) = slice_page_bounds(total_lines, before_line, limit);
        let lines = all_lines[start..end]
            .iter()
            .map(|l| parse_log_line(l))
            .collect();
        Ok(LogFilePage {
            lines,
            total_lines,
            has_more_older: start > 0,
            oldest_loaded_line: start,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 日志系统自身可观测性快照(方案 §5 P1「丢弃计数/压缩计数可见」):non_blocking 丢弃行数 +
/// 当前正在压缩中的错误签名列表。轻量纯内存读取(原子读 + 小 Mutex<HashMap> 遍历),不 spawn_blocking。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogDiagnostics {
    pub dropped_lines: usize,
    pub dedup_active: Vec<crate::error::ErrorDedupSnapshotEntry>,
}

#[tauri::command]
pub async fn get_log_diagnostics(state: State<'_, Arc<AppState>>) -> Result<LogDiagnostics> {
    Ok(LogDiagnostics {
        dropped_lines: state.log_dropped_counter.dropped_lines(),
        dedup_active: crate::error::error_dedup_snapshot(),
    })
}

/// 时间桶粒度 → SQLite `strftime` 格式串(方案 §5 P2「时间线直方图」)。白名单而非透传用户输入
/// 直接拼 SQL——虽然经绑定参数已无注入风险,但未知取值静默退化为按小时,好过报错打断分析流程。
fn histogram_bucket_format(bucket: &str) -> &'static str {
    match bucket {
        "day" => "%Y-%m-%dT00:00:00",
        _ => "%Y-%m-%dT%H:00:00",
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramBucket {
    /// 桶起点(朴素本地时间字符串,见下方 `strip_tz_suffix` 文档;不带时区后缀)。
    pub bucket: String,
    pub level: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogHistogram {
    pub buckets: Vec<HistogramBucket>,
    pub total_lines: usize,
    /// 成功解析出 ts+level 的行数(史前纯文本日志行/损坏行被跳过,不计入直方图;
    /// 前端据此判断本次分桶对该文件的覆盖率,而非静默漏计)。
    pub parsed_lines: usize,
}

/// 裁掉 RFC3339 时间戳的显式数字时区后缀(`EnvelopeFormat` 恒用 `to_rfc3339_opts(.., false)`,
/// 本地时间固定输出 `+HH:MM`/`-HH:MM` 六字符、从不落 `Z`),得到朴素本地时间字符串再交给 SQLite
/// `strftime` 分桶——若带偏移量直接喂给 SQLite,其日期函数会先转换到 UTC 再分桶,桶边界相对
/// 用户在日志窗口里看到的本地时间挪移一个时区差,读起来对不上。字符串过短(史前纯文本行解析失败
/// 留下的合成信封 ts=""）时原样返回,交由 SQLite 对无效值返回 NULL(该行不进任何桶,不 panic)。
fn strip_tz_suffix(ts: &str) -> &str {
    &ts[..ts.len().saturating_sub(6)]
}

/// 核心分桶逻辑(方案 §5 P2「JSONL 按需导入 rusqlite 临时表,Rust 侧 GROUP BY strftime 分桶」):
/// 抽成纯函数(输入日志文件全文,输出直方图)供单测直接构造样例 JSONL 断言,不必搭 tauri State。
/// 每次调用开一个独立于主库连接池的短生命周期内存连接,用完即弃,不占主库任何资源
/// (与本仓 DB 硬约束"rusqlite + spawn_blocking"同线,调用方需在 spawn_blocking 内执行)。
fn compute_histogram_from_content(content: &str, bucket: &str) -> rusqlite::Result<LogHistogram> {
    let conn = rusqlite::Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TEMP TABLE log_entries (ts_naive TEXT NOT NULL, level TEXT NOT NULL)",
    )?;

    let mut total_lines = 0usize;
    let mut parsed_lines = 0usize;
    {
        let mut insert =
            conn.prepare("INSERT INTO log_entries (ts_naive, level) VALUES (?1, ?2)")?;
        for line in content.lines() {
            if line.is_empty() {
                continue;
            }
            total_lines += 1;
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let (Some(ts), Some(level)) = (v["ts"].as_str(), v["level"].as_str()) else {
                continue;
            };
            insert.execute(rusqlite::params![strip_tz_suffix(ts), level])?;
            parsed_lines += 1;
        }
    }

    // WHERE 过滤桶为 NULL 的行(reviewer 深审 2026-07-20 修复):`strftime` 对无法解析的
    // ts_naive(如史前纯文本行解析失败留下的合成信封 ts="")返回 SQL NULL——上面 `row.get::<_, String>(0)`
    // 读到 NULL 会整条 Err,经 `collect::<rusqlite::Result<_>>()` 短路成**整份查询失败**,不是"该行
    // 不进桶"这个原意图的降级,而是所有正常桶也一并报错。显式排除 NULL 桶,让无法分桶的行安静地
    // 不出现在结果里(仍计入 parsed_lines,只是不落在任何 bucket——与文档注释原意一致)。
    let mut select = conn.prepare(
        "SELECT strftime(?1, ts_naive) AS bucket, level, COUNT(*) AS cnt \
         FROM log_entries WHERE strftime(?1, ts_naive) IS NOT NULL GROUP BY bucket, level ORDER BY bucket",
    )?;
    let buckets = select
        .query_map(rusqlite::params![histogram_bucket_format(bucket)], |row| {
            Ok(HistogramBucket {
                bucket: row.get(0)?,
                level: row.get(1)?,
                count: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(LogHistogram {
        buckets,
        total_lines,
        parsed_lines,
    })
}

#[tauri::command]
pub async fn compute_log_histogram(
    state: State<'_, Arc<AppState>>,
    file_name: String,
    bucket: String,
) -> Result<LogHistogram> {
    // span 埋点(W1,D-312 info 档:读文件+内存 SQLite 分桶,真实 IO/CPU 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:compute_log_histogram");
    let log_dir = state.log_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<LogHistogram> {
        let path = resolve_log_file(&log_dir, &file_name)?;
        let content = std::fs::read_to_string(&path)?;
        Ok(compute_histogram_from_content(&content, &bucket)?)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 诊断包导出结果(方案 §5 P2)。`dir` 与 `zip_path` 分开返回——前端拿 `dir` 直接调既有
/// `open_directory` 命令揭示,不必自己拆分隔符解析父目录(Windows 路径拼接的常见坑)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsPackageResult {
    pub dir: String,
    pub zip_path: String,
    pub size_bytes: u64,
    /// 脱敏扫描命中次数(方案 §5 P2「导出前脱敏扫描」——命中数可见化,不静默处理)。
    pub redacted_matches: u64,
}

/// 日志尾部截断行数(方案 §5 P2「日志尾部」):诊断包只带最近一段,不整份最新日志文件打包,
/// 防单日文件很大时诊断包本身也跟着膨胀。
const DIAGNOSTICS_TAIL_LINES: usize = 2000;

/// 导出诊断包(方案 §5 P2):system-info(版本/OS/架构/缩略图策略设置/DB 大小/扫描根数量)+
/// 最新日志文件尾部(脱敏后)打成一个 zip。仅落盘,不自动上传(方案 §5 明文要求)。
#[tauri::command]
pub async fn export_diagnostics_package(
    state: State<'_, Arc<AppState>>,
) -> Result<DiagnosticsPackageResult> {
    // span 埋点(W1,D-312 info 档:system-info 查询+日志尾脱敏+zip 打包,真实 IO/CPU 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:export_diagnostics_package");
    let thumb_strategy = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .strategy
        .clone();
    let db_pool = state.db_read_pool.clone();
    let log_dir = state.log_dir.clone();

    tokio::task::spawn_blocking(move || -> Result<DiagnosticsPackageResult> {
        // ── system-info ──────────────────────────────────────────────────
        // 缩略图策略是用户偏好；诊断包不把它写成已确认的 GPU 执行事实。
        let conn = db_pool.get()?;
        let page_count: u64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: u64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let scan_root_count = crate::db::queries::list_scan_roots(&conn)?.len();
        drop(conn);

        let system_info = serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "thumb_strategy_setting": thumb_strategy,
            "db_size_bytes": page_count * page_size,
            "scan_root_count": scan_root_count,
            "generated_at": chrono::Local::now().to_rfc3339(),
        });

        // ── 日志尾部(脱敏)──────────────────────────────────────────────
        let mut log_files: Vec<_> = std::fs::read_dir(&log_dir)?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
            .collect();
        log_files.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        let tail_text = match log_files.last() {
            Some(latest) => {
                let content = std::fs::read_to_string(latest.path())?;
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(DIAGNOSTICS_TAIL_LINES);
                lines[start..].join("\n")
            }
            None => String::new(),
        };
        let (redacted_tail, redacted_matches) = crate::logging::redact_diagnostics_text(&tail_text);

        // ── 打包 ──────────────────────────────────────────────────────────
        let diagnostics_dir = log_dir.join("diagnostics");
        std::fs::create_dir_all(&diagnostics_dir)?;
        // 毫秒精度(而非秒):reviewer 深审存疑项——同一秒内两次调用(如误触重复点击)会撞同名
        // tmp/final 路径,两次 spawn_blocking 并发写同一 tmp 文件存在竞态。UI 层 `exporting` 状态
        // 已让按钮在导出期间禁用(常规单击无法触发),这里补毫秒精度进一步缩小理论窗口,
        // 不做计数器/随机后缀(collision 概率已足够低,不值当为此加复杂度)。
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
        let final_path = diagnostics_dir.join(format!("scrollery-diagnostics-{stamp}.zip"));
        let tmp_path = diagnostics_dir.join(format!("scrollery-diagnostics-{stamp}.zip.tmp"));

        {
            use std::io::Write as _;
            let file = std::fs::File::create(&tmp_path)?;
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("system-info.json", opts)
                .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
            let system_info_text = serde_json::to_string_pretty(&system_info)
                .map_err(|e| AppError::internal("数据序列化失败 | serialization failed", e))?;
            zip.write_all(system_info_text.as_bytes())?;
            zip.start_file("log-tail.jsonl", opts)
                .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
            zip.write_all(redacted_tail.as_bytes())?;
            zip.finish()
                .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
        }
        // 派生文件写 tmp 再同卷 rename(项目硬约束),防导出中途中断留半份 zip。
        std::fs::rename(&tmp_path, &final_path)?;
        let size_bytes = std::fs::metadata(&final_path)?.len();

        Ok(DiagnosticsPackageResult {
            dir: diagnostics_dir.to_string_lossy().to_string(),
            zip_path: final_path.to_string_lossy().to_string(),
            size_bytes,
            redacted_matches,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}
