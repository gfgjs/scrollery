// src-tauri/src/scanner/enricher.rs
//! 阶段 2：后台信息丰富 — EXIF、XMP 动态照片、实况照片配对、sort_datetime 修正。
//!
//! 在快速扫描完成后异步运行。
//! 发送 `db:media_enriched` 和 `enrichment:completed` Tauri 事件。

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::db::models::ImageMeta;
use crate::db::queries::{
    get_audios_needing_meta, get_metadata_workload_with_video_retry,
    get_videos_needing_meta_with_retry, update_live_photo_flags, update_media_dimensions,
    update_sort_datetime, update_video_dimensions, upsert_audio_meta, upsert_image_meta,
    upsert_video_meta,
};
use crate::error::{AppError, Result};
use crate::scanner::live_photo::pair_live_photos;
use crate::scanner::metadata::{
    apply_orientation_swap, detect_motion_photo_xmp, detect_motion_photo_xmp_buf,
    parse_exif_with_context, read_dimensions_with_context, read_header_buf_for_ext_with_file_size,
    ExifParsePath, HeaderRead, ImageReadContext,
};
use crate::state::AppState;
use crate::utils::path::resolve_media_path;

mod image_pipeline;
use image_pipeline::{
    run_bounded_header_parse, PipelineTiming, HEADER_CHUNK_ITEMS, MAX_INFLIGHT_HEADER_CHUNKS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnrichmentSourceSnapshot {
    source_revision: i64,
    file_size: i64,
    file_mtime_ns: Option<i64>,
}

type ImageEnrichmentPathInfo = (i64, String, i64, i64, i64, i64);
type MediaEnrichmentPathInfo = (i64, String, String, i64);

/// 在文件解析开始前抓取 DB 源快照；写回事务内会再次比对，避免编辑/扫描失效在
/// 解析期间发生后，旧元数据重新覆盖当前项。该查询只读数据库，不触碰文件。
fn load_enrichment_source_snapshots(
    conn: &Connection,
    item_ids: impl IntoIterator<Item = i64>,
) -> Result<HashMap<i64, EnrichmentSourceSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT source_revision, file_size, file_mtime_ns, is_deleted
           FROM media_items WHERE id=?1",
    )?;
    let mut snapshots = HashMap::new();
    for item_id in item_ids {
        let row = stmt
            .query_row([item_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .optional()?;
        if let Some((source_revision, file_size, file_mtime_ns, is_deleted)) = row {
            if is_deleted == 0 {
                snapshots.insert(
                    item_id,
                    EnrichmentSourceSnapshot {
                        source_revision,
                        file_size,
                        file_mtime_ns,
                    },
                );
            }
        }
    }
    Ok(snapshots)
}

fn enrichment_source_is_current(
    conn: &Connection,
    item_id: i64,
    expected: EnrichmentSourceSnapshot,
) -> Result<bool> {
    let current = conn
        .query_row(
            "SELECT source_revision, file_size, file_mtime_ns, is_deleted
               FROM media_items WHERE id=?1",
            [item_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(
        current.is_some_and(|(source_revision, file_size, file_mtime_ns, is_deleted)| {
            is_deleted == 0
                && source_revision == expected.source_revision
                && file_size == expected.file_size
                && file_mtime_ns == expected.file_mtime_ns
        }),
    )
}
use crate::video::{VideoProbeOutcome, VideoProbeRuntime, VideoProbeStatus};

use serde::{Deserialize, Serialize};

const ENRICHMENT_BATCH: i64 = 500;
// 1,000 是图片段的 **DB 选批/写回**粒度：减少事务/事件次数，同时不让视频/音频这种单项更重的
// 任务跟着放大批次。批内的头读不是整批收齐——64 项一块、最多 3 块在途（生产 + 排队 + 消费，
// 见 image_pipeline），故同时在途的头缓冲与文件句柄上限是 192 项（最坏 192×256 KiB = 48 MiB，
// 通常更低）。
const IMAGE_ENRICHMENT_BATCH: i64 = 1_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct VideoProbeFormatStats {
    attempted: i64,
    populated: i64,
    failed: i64,
    unsupported: i64,
    fallback_attempted: i64,
    fallback_populated: i64,
}

#[derive(Debug, Default)]
struct VideoProbeSummary {
    attempted: i64,
    populated: i64,
    failed: i64,
    unsupported: i64,
    fallback_attempted: i64,
    fallback_populated: i64,
    by_format: BTreeMap<String, VideoProbeFormatStats>,
}

impl VideoProbeSummary {
    fn observe(&mut self, ext: &str, outcome: &VideoProbeOutcome) {
        let populated = matches!(outcome.status, VideoProbeStatus::Succeeded)
            && outcome
                .info
                .as_ref()
                .is_some_and(|info| info.width > 0 && info.height > 0);

        self.attempted += 1;
        let format = self.by_format.entry(ext.to_ascii_lowercase()).or_default();
        format.attempted += 1;

        if outcome.fallback_attempted {
            self.fallback_attempted += 1;
            format.fallback_attempted += 1;
        }

        if populated {
            self.populated += 1;
            format.populated += 1;
            if outcome.fallback_attempted {
                self.fallback_populated += 1;
                format.fallback_populated += 1;
            }
        } else {
            match outcome.status {
                VideoProbeStatus::Unsupported => {
                    self.unsupported += 1;
                    format.unsupported += 1;
                }
                VideoProbeStatus::Failed | VideoProbeStatus::Succeeded => {
                    // A successful backend call with zero dimensions is still not usable metadata.
                    self.failed += 1;
                    format.failed += 1;
                }
            }
        }
    }

    fn format_breakdown(&self) -> String {
        self.by_format
            .iter()
            .map(|(ext, stats)| {
                format!(
                    "{ext}:attempted={},populated={},failed={},unsupported={},fallback={}/{}",
                    stats.attempted,
                    stats.populated,
                    stats.failed,
                    stats.unsupported,
                    stats.fallback_populated,
                    stats.fallback_attempted,
                )
            })
            .collect::<Vec<_>>()
            .join(";")
    }
}

/// 一个**为前台保留一个 CPU 核**的 rayon 池，使单线程的 `compute_layout` 在导入期（并行、且部分受 IO
/// 限制的）媒体探测运行时仍跟手（对应「布局被视频任务阻塞」的扫描期分支）。与派生流水线（用户交互即完全暂停）不同：enrichment 是用户正在等待的
/// 导入工作，且其进度事件会自动触发重排，故「交互即让步」会饿死它。改为保留一个核，既保 UI 流畅又不
/// 拖慢导入。`None` → 调用方回退到全局池。
fn reserved_core_pool() -> Option<rayon::ThreadPool> {
    let n = std::thread::available_parallelism()
        .map(|c| c.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    rayon::ThreadPoolBuilder::new().num_threads(n).build().ok()
}

/// 头读阶段单独使用偏 IO 的有界线程池。
///
/// 解析阶段仍保留一个 CPU 核给前台；头读则允许略高于逻辑核数的并发，用来覆盖
/// Windows 下多文件 open/stat/seek 和杀毒软件过滤造成的等待。上限是硬边界，避免把
/// 「提高并发」变成无界文件句柄和随机 IO 放大。
fn header_read_pool() -> Option<rayon::ThreadPool> {
    let n = crate::scanner::hdd_io::header_worker_count();
    rayon::ThreadPoolBuilder::new().num_threads(n).build().ok()
}

// ── 图片批的单项读头 / 解析 ─────────────────────────────────────────────

/// 单项头读结果：小写扩展名 + 头缓冲与保留句柄（`None` = open/读头失败，解析走原 per-fn 路径）。
type ImageHeaderRead = (String, Option<HeaderRead>);

/// 单项解析结果：(id, EXIF/尺寸解析结果, is_live, has_embedded_video, 像素尺寸, EXIF 解析路径)。
#[allow(clippy::type_complexity)]
type ImageParseResult = (
    i64,
    Result<ImageMeta>,
    bool,
    bool,
    Option<(i64, i64)>,
    ExifParsePath,
);

/// 读一项文件头并按扩展名阶梯取上限，保留句柄供 EXIF/尺寸/回退复用（省一次 stat 与重复 open）。
fn read_image_header(path: &std::path::Path, file_size: i64) -> ImageHeaderRead {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let known_file_size = u64::try_from(file_size).ok();
    let header = read_header_buf_for_ext_with_file_size(path, &ext, known_file_size).ok();
    (ext, header)
}

/// 解析一项已读好的头缓冲：EXIF +（JPEG）XMP 动态照片标记 + 占位项真实尺寸。
///
/// 与旧整批路径逐项同语义（同一解析函数、同一方向处理、同一回退选择），只是从内联闭包提取成
/// 函数：新分块流水线与旧整批路径共用它，表征测试才能逐项对照两条路径的结果。
#[cfg(test)]
fn parse_image_item(
    path_info: &ImageEnrichmentPathInfo,
    ext: String,
    header_read: Option<HeaderRead>,
) -> ImageParseResult {
    parse_image_item_measured(
        path_info,
        ext,
        header_read,
        &mut ImageReadContext::default(),
        std::time::Duration::ZERO,
        None,
    )
    .unwrap()
}

fn parse_image_item_measured(
    path_info: &ImageEnrichmentPathInfo,
    ext: String,
    mut header_read: Option<HeaderRead>,
    context: &mut ImageReadContext,
    header_elapsed: std::time::Duration,
    run: Option<(i64, &str)>,
) -> Result<ImageParseResult> {
    let (id, abs_path, w, h, _sort_dt, _file_size) = path_info;
    let started = std::time::Instant::now();
    let header_bytes = header_read
        .as_ref()
        .map_or(0, |read| read.header.bytes.len());
    let container_format = header_read
        .as_ref()
        .and_then(|read| image::guess_format(&read.header.bytes).ok());
    let mut dimensions_elapsed = std::time::Duration::ZERO;
    // panic 伞(2026-07-10 审查 B8,P1-1 同族):kamadak-exif/image 对畸形文件的 panic 会经
    // rayon 传播中止整个 run_enrichment,且该项不落 minimal 行 → 下轮同一文件重新入选再炸,
    // 库级补全永不完成。panic 降级为「EXIF 失败」,与解析失败同路写 minimal 行,保住前进性。
    let guarded = crate::thumbnail::generator::panic_guard("enrich_parse_item", || {
        let path = std::path::Path::new(abs_path);
        // #10 单次头读:一次 open 读头,EXIF/XMP/尺寸三消费者复用(原实现最多 open 3 次,
        // Windows 下每次 open 伴随 Defender 扫描,是本 pass 的首要 I/O 成本)。阶段5 按扩展名
        // 阶梯读:JPEG 128KB、TIFF/HEIC 系保持 256KB、其余 64KB；截断时先做格式级判断,
        // 只有不能确认“无 EXIF”时才回退整文件路径。open 失败走原 per-fn 路径,同语义报错。
        let (meta, exif_path) = parse_exif_with_context(path, header_read.as_mut(), context);
        let (is_live, has_embedded) = if matches!(ext.as_str(), "jpg" | "jpeg") {
            match header_read.as_ref() {
                Some(read) => detect_motion_photo_xmp_buf(&read.header),
                None if context.ensure_file_access().is_ok() => detect_motion_photo_xmp(path),
                None => (false, false),
            }
        } else {
            (false, false)
        };
        // 仅对占位项读取尺寸 — 保持首屏即时尺寸（及其方向）不变（不双重翻转）。
        // 复用上面刚解析出的方向（meta），而不是为读 Orientation 再开一次 JPEG。
        let dims = if *w == 0 || *h == 0 {
            let dimension_started = std::time::Instant::now();
            let raw = read_dimensions_with_context(
                path,
                ext.as_str(),
                header_read.as_ref().map(|read| &read.header),
                context,
            );
            dimensions_elapsed = dimension_started.elapsed();
            if raw.0 > 0 && raw.1 > 0 {
                let oriented = if matches!(ext.as_str(), "jpg" | "jpeg") {
                    let orientation = meta.as_ref().map(|m| m.orientation as u32).unwrap_or(1);
                    apply_orientation_swap(raw, orientation)
                } else {
                    raw
                };
                Some(oriented)
            } else {
                None
            }
        } else {
            None
        };
        Ok((meta, is_live, has_embedded, dims, exif_path))
    });
    let result = match guarded {
        Ok((meta, is_live, has_embedded, dims, exif_path)) => {
            (*id, meta, is_live, has_embedded, dims, exif_path)
        }
        // panic → 与 EXIF 解析失败同路(meta=Err),下游写 minimal 行。
        Err(e) => (*id, Err(e), false, false, None, ExifParsePath::Failed),
    };
    let parse_elapsed = started.elapsed();
    if let Some((root_id, run_id)) = run {
        if header_elapsed + parse_elapsed >= std::time::Duration::from_millis(500) {
            info!(root_id, run_id, item_id = id, format = %ext, container_format = ?container_format,
                header_ms = header_elapsed.as_secs_f64() * 1000.0,
                parse_ms = parse_elapsed.as_secs_f64() * 1000.0,
                exif_memory_ms = context.exif_memory.as_secs_f64() * 1000.0,
                exif_probe_ms = context.exif_probe.as_secs_f64() * 1000.0,
                exif_fallback_ms = context.exif_fallback.as_secs_f64() * 1000.0,
                dimensions_ms = dimensions_elapsed.as_secs_f64() * 1000.0,
                dimensions_file = context.dimensions_file,
                file_access_wait_ms = context.file_access_wait.as_secs_f64() * 1000.0, header_bytes,
                exif_path = ?result.5, exif_ok = result.1.is_ok(), dimensions = ?result.4,
                "Slow image enrichment item");
        }
    }
    context.check_io_error()?;
    Ok(result)
}

// ── IPC 事件负载 ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaEnrichedPayload {
    pub root_id: i64,
    /// root_id=0 的画廊刷新哨兵不属于扫描任务，因此不携带 run_id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub enriched_count: i64,
    pub total: i64,
    pub processed_bytes: i64,
    pub total_bytes: i64,
}

impl MediaEnrichedPayload {
    /// 仅通知画廊刷新，不驱动扫描进度。
    pub fn refresh_signal() -> Self {
        Self {
            root_id: 0,
            run_id: None,
            enriched_count: 0,
            total: 0,
            processed_bytes: 0,
            total_bytes: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentCompletedPayload {
    pub root_id: i64,
    pub run_id: String,
    pub elapsed_ms: u64,
    /// 终态错误码：`None` 表示正常完成；`Some(稳定码)` 表示后台补全**异常终止**
    /// （出错 / panic）。前端据此弹 warning，告知用户部分元数据可能缺失——避免
    /// 失败「伪装成正常完成」只进日志、对用户不可见。
    ///
    /// 携带**稳定粗粒度码**（如 `"enrich_failed"` / `"enrich_panicked"`）而非原始
    /// 错误串，遵循 IPC 边界错误契约；细分原因（EXIF/XMP/视频…）已在后端日志中。
    pub error_code: Option<String>,
}

impl EnrichmentCompletedPayload {
    /// 正常完成的终态事件。
    pub fn ok(root_id: i64, run_id: &str, elapsed_ms: u64) -> Self {
        Self {
            root_id,
            run_id: run_id.to_string(),
            elapsed_ms,
            error_code: None,
        }
    }

    /// 异常终止的终态事件，携带稳定错误码。`elapsed_ms` 置 0（耗时对失败无意义）。
    pub fn failed(root_id: i64, run_id: &str, code: &str) -> Self {
        Self {
            root_id,
            run_id: run_id.to_string(),
            elapsed_ms: 0,
            error_code: Some(code.to_string()),
        }
    }
}

// ── 丰富信息入口点 ────────────────────────────────────────────────────

/// 扫描富化阶段使用的根级代次上下文。
///
/// 编辑保存触发的富化没有扫描轮次，继续使用 `None` 兼容入口；扫描命令则必须把所有
/// DB 写入交给 `AppState` 的同一根级 generation 闸门，避免 stop 后立即 restart 时旧轮
/// 在新轮安装后继续发布尺寸/元数据。
#[derive(Clone, Copy)]
struct ScanGeneration<'a> {
    state: &'a AppState,
    root_id: i64,
    generation: u64,
}

fn ensure_scan_generation(context: Option<ScanGeneration<'_>>) -> Result<()> {
    if context.is_some_and(|context| {
        !context
            .state
            .is_scan_generation_current(context.root_id, context.generation)
    }) {
        return Err(AppError::Cancelled);
    }
    Ok(())
}

/// 在扫描代次闸门内完成一段短 DB 写入；文件解析必须发生在调用方进入此函数之前。
fn with_scan_db_write<T>(
    writer: &Mutex<Connection>,
    context: Option<ScanGeneration<'_>>,
    write: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    if let Some(context) = context {
        return context
            .state
            .with_scan_generation_write(context.root_id, context.generation, || {
                let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
                write(&conn)
            })
            .transpose()?
            .ok_or(AppError::Cancelled);
    }

    let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
    write(&conn)
}

/// 只把极短的 bump/事件发布也绑定到当前代次，防止旧轮在新轮安装后刷新前端状态。
fn with_scan_action(context: Option<ScanGeneration<'_>>, action: impl FnOnce()) -> Result<()> {
    if let Some(context) = context {
        if context
            .state
            .with_scan_generation_write(context.root_id, context.generation, action)
            .is_none()
        {
            return Err(AppError::Cancelled);
        }
        return Ok(());
    }

    action();
    Ok(())
}

/// 运行扫描根目录的后台信息丰富。
///
/// 此函数旨在从 `tokio::task::spawn_blocking` 调用
/// 因此异步运行时不会被阻塞。
/// S1：enrichment 批提交后 bump 数据版本——尺寸回写与 EXIF 时间修正改变布局的几何与
/// 顺序输入，items 取数缓存必须失效。经 AppHandle 取 AppState（测试无 managed state 时
/// 静默跳过，行为即「无缓存可失效」）。
fn bump_layout_data_version(app: &AppHandle) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<std::sync::Arc<crate::state::AppState>>() {
        state.bump_data_version();
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_enrichment(
    app: &AppHandle,
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    group_by: &str,
    sort_within_group: &str,
    sort_order: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    run_enrichment_inner(
        app,
        writer,
        root_id,
        run_id,
        group_by,
        sort_within_group,
        sort_order,
        cancel,
        None,
    )
}

/// 运行绑定扫描代次的后台信息丰富。
#[allow(clippy::too_many_arguments)]
pub fn run_enrichment_with_generation(
    app: &AppHandle,
    state: &AppState,
    root_id: i64,
    run_id: &str,
    group_by: &str,
    sort_within_group: &str,
    sort_order: &str,
    cancel: &CancellationToken,
    generation: u64,
) -> Result<()> {
    run_enrichment_inner(
        app,
        &state.db_writer,
        root_id,
        run_id,
        group_by,
        sort_within_group,
        sort_order,
        cancel,
        Some(ScanGeneration {
            state,
            root_id,
            generation,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_enrichment_inner(
    app: &AppHandle,
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    group_by: &str,
    sort_within_group: &str,
    sort_order: &str,
    cancel: &CancellationToken,
    scan_generation: Option<ScanGeneration<'_>>,
) -> Result<()> {
    let started = std::time::Instant::now();
    ensure_scan_generation(scan_generation)?;
    info!("Enrichment started: root_id={root_id} | 增量补全开始: root_id={root_id}");
    // 同一富化阶段只检查一次 worker 能力；若存在替代后端，也允许重试历史最小视频元数据。
    let probe_runtime = VideoProbeRuntime::new();

    // 按画廊当前视图顺序处理，使占位尺寸自上而下补全 —— 贴合用户可能的滚动 ——
    // 而非按插入 id。与 `query_layout_geometry` 的 ORDER BY 对齐。
    let order_clause = enrichment_order_clause(group_by, sort_within_group, sort_order);
    // 阶段1(2026-08-21):默认「日期/时间」序改用 keyset 分页。原实现每批对全部剩余未富化项
    // ORDER BY + LIMIT,SQLite 无法用旧索引消序(EXPLAIN 呈 TEMP B-TREE),批循环整体近
    // O(N²/500)。keyset 查询消费 V24 `idx_media_type_sort(media_type, sort_datetime, id)`,
    // 每批从上一游标直接 seek。
    // 2026-08-23:不可 keyset 的序(folder / *+filename)改走 TEMP 队列表——首批判次一次性
    // 按视图序灌入,此后按 seq 游标续取,同样脱离 O(N²/批)(吸收对照线方案;表名带调用
    // 代次,同根重扫的旧任务收尾不会误删新任务刚建的表)。
    let keyset_enabled = image_keyset_applicable(group_by, sort_within_group);
    // #10(2026-07-17):批选直接带路径三段(r.path/d.rel_path/m.file_name)——原实现批选后
    // 逐项 get_item_path_info ×500,串行 3 表 JOIN 且**持写锁**,500k 库约 1000 批纯浪费。
    // 本查询本就 JOIN directories,多 JOIN scan_roots 一表即免掉整个逐项阶段。
    let keyset_first_sql = image_batch_sql(true, false, &order_clause, sort_order);
    let keyset_next_sql = image_batch_sql(true, true, &order_clause, sort_order);
    let fallback_sql = image_batch_sql(false, false, &order_clause, sort_order);
    // keyset 游标:(上一批末尾 sort_datetime, id)。`None` = 尚未取过 keyset 批。
    let mut image_cursor: Option<ImageBatchCursor> = None;
    // 队列表(不可 keyset 序):惰性建于首个队列表批,seq 游标从 0 起;RAII 守卫负责收尾 DROP。
    let mut queue: Option<QueueGuard> = None;
    // 主源(keyset/队列)耗尽后切一次 fallback,接住扫描开始后并发插入、且序值落在已走过
    // 游标之前的行,以及本轮写库失败需重试的行。
    let mut image_fallback = false;

    // ── Count total unenriched items ──────────────────────────────────────
    // ── 计算未丰富信息的项目总数 ──────────────────────────────────────
    ensure_scan_generation(scan_generation)?;
    let workload = {
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        get_metadata_workload_with_video_retry(&conn, root_id, probe_runtime.has_worker_fallback())?
    };
    let total = workload.files;
    let total_bytes = workload.bytes;

    info!(
        "Enrichment: {total} items / {total_bytes} bytes to process for root_id={root_id} | 增量补全: root_id={root_id} 共有 {total} 项 / {total_bytes} 字节待处理"
    );

    let mut enriched_total: i64 = 0;
    let mut enriched_bytes: i64 = 0;
    let mut exif_header_total: i64 = 0;
    let mut exif_chunk_total: i64 = 0;
    let mut exif_fallback_total: i64 = 0;
    let mut exif_no_metadata_total: i64 = 0;
    let mut exif_unsupported_total: i64 = 0;
    let mut exif_direct_file_total: i64 = 0;
    let mut exif_failed_total: i64 = 0;
    // 批循环各段的累计服务时间（纳秒）：select/pipeline_wall/write/publish 逐步相加，
    // 收尾日志用结构化字段一次性输出，便于机械汇总。
    let mut select_ns_total: u64 = 0;
    let mut pipeline_wall_ns_total: u64 = 0;
    let mut write_wait_ns_total: u64 = 0;
    let mut write_hold_ns_total: u64 = 0;
    let mut publish_ns_total: u64 = 0;

    // T13（§3.7.2）：图片段并行 EXIF/尺寸提取也跑在**保留核**的池上（与视频/音频段一致），
    // 为前台 `compute_layout` 留一个核——否则海量图片导入会把全局 rayon 池占满、与「2s 自动重排」
    // 反馈循环互相饿死。一次性建池、整段复用；`None`（建池失败）→ 回退全局池。
    let root_path: Option<String> = {
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT path FROM scan_roots WHERE id=?1",
            [root_id],
            |row| row.get(0),
        )
        .optional()?
    };
    // 设备查询不能占着数据库 writer；同盘多个根从进程级注册表取得同一预算。
    let disk_budget = root_path
        .as_deref()
        .and_then(crate::scanner::hdd_io::for_root);
    let image_io_mode = if disk_budget.is_some() {
        "hdd_adaptive"
    } else {
        "header_pipeline"
    };
    let img_pool = reserved_core_pool();
    let header_pool = header_read_pool();
    // 批内流水线的累计分段计时与在途块峰值。口径是单根：多根同时富化时各自独立统计，
    // 这里的峰值不是全局上界。header/parse 在块级真重叠，累计服务时间不可相加当 wall。
    let pipeline_timing = PipelineTiming::default();
    let header_workers = header_pool
        .as_ref()
        .map(rayon::ThreadPool::current_num_threads)
        .unwrap_or_else(rayon::current_num_threads);
    let parse_workers = img_pool
        .as_ref()
        .map(rayon::ThreadPool::current_num_threads)
        .unwrap_or_else(rayon::current_num_threads);
    info!(
        root_id, run_id, image_io_mode,
        disk = ?disk_budget.as_ref().map(|budget| budget.disk),
        header_limit = ?disk_budget.as_ref().map(|budget| budget.header_limit),
        supplemental_limit = ?disk_budget.as_ref().map(|_| 1),
        "Image enrichment pipeline: batch={IMAGE_ENRICHMENT_BATCH}, header_chunk={HEADER_CHUNK_ITEMS}, max_inflight_header_chunks={MAX_INFLIGHT_HEADER_CHUNKS}, header_workers={header_workers}, parse_workers={parse_workers}"
    );

    loop {
        if cancel.is_cancelled() {
            warn!("Enrichment cancelled at root_id={root_id}");
            return Err(AppError::Cancelled);
        }
        ensure_scan_generation(scan_generation)?;

        // 获取下一批未丰富信息的项目（在该根目录下），并带上当前尺寸，
        // 以便补全快速扫描"延后尺寸"路径留下的 0×0 占位。
        // #10:路径随批选一次取回(见上方 SQL 注释),写锁只占这一段、不再有逐项查询。
        // 第 6 列为游标键(keyset=m.sort_datetime,queue=q.seq),解析/写回不消费。
        // 选批（含 source_snapshot 读取）计时：写锁只覆盖这一段，文件解析不持锁。
        let select_started = std::time::Instant::now();
        let (path_infos, source_snapshots): (
            Vec<ImageEnrichmentPathInfo>,
            HashMap<i64, EnrichmentSourceSnapshot>,
        ) = {
            let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
            let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ImageEnrichmentPathInfo> {
                let id = row.get::<_, i64>(0)?;
                let w = row.get::<_, i64>(1)?;
                let h = row.get::<_, i64>(2)?;
                let root_p = row.get::<_, String>(3)?;
                let rel_p = row.get::<_, String>(4)?;
                let name = row.get::<_, String>(5)?;
                let sort_dt = row.get::<_, i64>(6)?;
                let file_size = row.get::<_, i64>(7)?;
                Ok((
                    id,
                    resolve_media_path(&root_p, &rel_p, &name),
                    w,
                    h,
                    sort_dt,
                    file_size,
                ))
            };
            let rows = if image_fallback {
                let mut stmt = conn.prepare(&fallback_sql)?;
                let rows = stmt
                    .query_map(rusqlite::params![root_id, IMAGE_ENRICHMENT_BATCH], map_row)?
                    .filter_map(|r| r.ok())
                    .collect::<Vec<_>>();
                rows
            } else if keyset_enabled {
                match image_cursor {
                    None => {
                        let mut stmt = conn.prepare(&keyset_first_sql)?;
                        let rows = stmt
                            .query_map(rusqlite::params![root_id, IMAGE_ENRICHMENT_BATCH], map_row)?
                            .filter_map(|r| r.ok())
                            .collect::<Vec<_>>();
                        rows
                    }
                    Some(cur) => {
                        let mut stmt = conn.prepare(&keyset_next_sql)?;
                        let rows = stmt
                            .query_map(
                                rusqlite::params![
                                    root_id,
                                    IMAGE_ENRICHMENT_BATCH,
                                    cur.sort_datetime,
                                    cur.id
                                ],
                                map_row,
                            )?
                            .filter_map(|r| r.ok())
                            .collect::<Vec<_>>();
                        rows
                    }
                }
            } else {
                // 队列表分支(不可 keyset 的视图序):首个批判次一次性按视图序灌入 TEMP
                // 队列表,此后按 seq 游标续取——每批免重排,脱离 O(N²/批)。
                if queue.is_none() {
                    let table = format!("_enrich_q_r{root_id}_g{}", next_queue_gen());
                    conn.execute_batch(&image_queue_create_sql(&table))?;
                    conn.prepare(&image_queue_fill_sql(&order_clause, sort_order, &table))?
                        .execute(rusqlite::params![root_id])?;
                    queue = Some(QueueGuard::new(writer, table));
                }
                let q = queue.as_ref().expect("queue built above");
                let mut stmt = conn.prepare(&image_queue_fetch_sql(&q.table))?;
                let rows = stmt
                    .query_map(rusqlite::params![IMAGE_ENRICHMENT_BATCH, q.seq], map_row)?
                    .filter_map(|r| r.ok())
                    .collect::<Vec<_>>();
                rows
            };
            let source_snapshots = load_enrichment_source_snapshots(
                &conn,
                rows.iter().map(|(item_id, _, _, _, _, _)| *item_id),
            )?;
            (rows, source_snapshots)
        };
        let select_ns = image_pipeline::duration_ns(select_started.elapsed());
        select_ns_total += select_ns;

        if path_infos.is_empty() {
            if !image_fallback {
                // 主源(keyset/队列)已走完 → 补一次 fallback 接住并发插入与本轮失败重试
                // (通常为空,只多一条空查询)。
                image_fallback = true;
                continue;
            }
            break;
        }

        if !image_fallback {
            if keyset_enabled {
                // 本批按 (sort_datetime,id) 升/降序返回,末尾即下一游标。
                image_cursor =
                    path_infos
                        .last()
                        .map(|(id, _, _, _, sort_dt, _)| ImageBatchCursor {
                            sort_datetime: *sort_dt,
                            id: *id,
                        });
            } else if let Some(q) = queue.as_mut() {
                // 队列批按 seq 升序返回,末行 seq 即下一游标。
                if let Some((_, _, _, _, seq, _)) = path_infos.last() {
                    q.seq = *seq;
                }
            }
        }

        // 批内流水线（2026-09-12 性能调查 F-001/F-002）：按 HEADER_CHUNK_ITEMS 项一块读头，
        // 块一到就交给解析池，使下一块头读与当前块解析真重叠，并把在途 HeaderRead（头缓冲 +
        // 文件句柄）峰值从整批降到 MAX_INFLIGHT_HEADER_CHUNKS 块。选批与写回仍是每
        // IMAGE_ENRICHMENT_BATCH 项一次，视图顺序/source_snapshot 校验/generation 语义不变；
        // 取消时整批放弃（不提前写部分结果，也不提前发布事件）。
        let timing_before = pipeline_timing.snapshot();
        let pipeline_started = std::time::Instant::now();
        let parsed: Vec<ImageParseResult> = run_bounded_header_parse(
            path_infos.len(),
            cancel,
            header_pool.as_ref(),
            img_pool.as_ref(),
            &pipeline_timing,
            |index| -> Result<_> {
                let (_, abs_path, _, _, _, file_size) = &path_infos[index];
                let started = std::time::Instant::now();
                // 守卫只覆盖头读；随缓冲进入队列会与消费侧的独占补读互相等待。
                let _permit = disk_budget
                    .as_ref()
                    .map(|budget| budget.acquire_header(cancel))
                    .transpose()?;
                let header = read_image_header(std::path::Path::new(abs_path), *file_size);
                Ok((header, started.elapsed()))
            },
            |index, header| -> Result<_> {
                let ((ext, header_read), elapsed) = header?;
                let mut context = ImageReadContext::with_disk_budget(disk_budget.clone(), cancel);
                parse_image_item_measured(
                    &path_infos[index],
                    ext,
                    header_read,
                    &mut context,
                    elapsed,
                    Some((root_id, run_id)),
                )
            },
        )?
        .into_iter()
        .collect::<Result<_>>()?;
        let pipeline_wall_ns = image_pipeline::duration_ns(pipeline_started.elapsed());
        pipeline_wall_ns_total += pipeline_wall_ns;

        // 在单个事务中写入结果
        let write_started = std::time::Instant::now();
        let mut tx_hold_ns = 0u64;
        with_scan_db_write(writer, scan_generation, |conn| {
            let tx_started = std::time::Instant::now();
            let tx = conn.unchecked_transaction()?;

            for (item_id, meta_result, is_live, has_embedded, dims, _) in &parsed {
                let Some(source_snapshot) = source_snapshots.get(item_id).copied() else {
                    continue;
                };
                if !enrichment_source_is_current(&tx, *item_id, source_snapshot)? {
                    debug!(
                        "Skipping stale image enrichment id={item_id} | 跳过已失效图片富化 id={item_id}"
                    );
                    continue;
                }
                // 为占位（0×0）项补全真实尺寸。
                if let Some((w, h)) = dims {
                    if let Err(e) = update_media_dimensions(&tx, *item_id, *w, *h) {
                        warn!("Failed to backfill dimensions for id={item_id}: {e}");
                    }
                }

                match meta_result {
                    Ok(meta) => {
                        let mut m = meta.clone();
                        m.item_id = *item_id;

                        if let Err(e) = upsert_image_meta(&tx, &m) {
                            warn!("Failed to upsert image_meta for id={item_id}: {e}");
                        }

                        // 修正 sort_datetime = COALESCE(exif_datetime, file_mtime)
                        if let Some(exif_dt) = m.exif_datetime {
                            let _ = update_sort_datetime(&tx, *item_id, exif_dt);
                        }
                        // 注意：宽度/高度方向修正由 fast_scan 处理
                        // 针对 JPEG（最常见的情况）。不要在这里再次交换以避免
                        // 双重翻转。如果将来需要非 JPEG 方向支持，
                        // 添加一个 media_items.dims_corrected 标志并仅在其为 0 时进行交换。
                    }
                    Err(e) => {
                        debug!("EXIF parse skipped id={item_id}: {e}");
                        // 插入最小行，以便我们不会再次尝试此项目
                        let minimal = ImageMeta {
                            item_id: *item_id,
                            orientation: 1,
                            ..Default::default()
                        };
                        let _ = upsert_image_meta(&tx, &minimal);
                    }
                }

                if *is_live {
                    let _ = update_live_photo_flags(&tx, *item_id, true, *has_embedded);
                }
            }

            tx.commit()?;
            tx_hold_ns = image_pipeline::duration_ns(tx_started.elapsed());
            Ok(())
        })?;
        // write_wait = 等 writer 锁（含代次闸门），write_hold = 事务实际持锁时间。
        let write_wait_ns =
            image_pipeline::duration_ns(write_started.elapsed()).saturating_sub(tx_hold_ns);
        write_wait_ns_total += write_wait_ns;
        write_hold_ns_total += tx_hold_ns;
        // S1：本批尺寸/EXIF 时间已提交（几何与顺序输入变化）→ bump；事件也绑定当前代次。
        let publish_started = std::time::Instant::now();
        with_scan_action(scan_generation, || {
            bump_layout_data_version(app);
            let _ = app.emit(
                "db:media_enriched",
                MediaEnrichedPayload {
                    root_id,
                    run_id: Some(run_id.to_string()),
                    enriched_count: enriched_total,
                    total,
                    processed_bytes: enriched_bytes,
                    total_bytes,
                },
            );
        })?;
        let publish_ns = image_pipeline::duration_ns(publish_started.elapsed());
        publish_ns_total += publish_ns;

        for (_, _, _, _, _, exif_path) in &parsed {
            match exif_path {
                ExifParsePath::HeaderBuffer => exif_header_total += 1,
                ExifParsePath::ContainerChunk => exif_chunk_total += 1,
                ExifParsePath::FullFileFallback => exif_fallback_total += 1,
                ExifParsePath::NoMetadata => exif_no_metadata_total += 1,
                ExifParsePath::Unsupported => exif_unsupported_total += 1,
                ExifParsePath::DirectFile => exif_direct_file_total += 1,
                ExifParsePath::Failed => exif_failed_total += 1,
            }
        }

        enriched_total += parsed.len() as i64;
        enriched_bytes += path_infos
            .iter()
            .map(|(_, _, _, _, _, file_size)| *file_size)
            .sum::<i64>();
        debug!(
            "Enrichment batch done: {enriched_total}/{total} files, {enriched_bytes}/{total_bytes} bytes; EXIF full-file fallbacks so far: {exif_fallback_total}"
        );
        // 单批分段计时（单根口径，不是全局 cap）：header_read/parse 是累计服务时间，两者在块级
        // 真重叠，不可相加当 wall；queue_wait=消费侧等块，producer_backpressure=生产侧等槽位，
        // write_wait=等 writer 锁，write_hold=事务持锁。字段全为纳秒整数，便于汇总。
        let batch_stages = pipeline_timing.snapshot().delta_since(timing_before);
        debug!(
            run_id,
            root_id,
            image_io_mode,
            items = parsed.len(),
            select_ns,
            pipeline_wall_ns,
            write_wait_ns,
            write_hold_ns = tx_hold_ns,
            publish_ns,
            header_read_ns = batch_stages.header_read_ns,
            parse_ns = batch_stages.parse_ns,
            queue_wait_ns = batch_stages.consumer_wait_ns,
            producer_backpressure_ns = batch_stages.producer_backpressure_ns,
            chunks_produced = batch_stages.chunks_produced,
            chunks_consumed = batch_stages.chunks_consumed,
            peak_inflight_chunks = batch_stages.peak_inflight_chunks,
            "Enrichment batch stages (service times overlap; do not sum as wall) | 富化单批分段:累计服务时间不可相加当 wall"
        );
    }

    // ── 实况照片配对 ────────────────────────────────────────────────
    if !cancel.is_cancelled() {
        match with_scan_db_write(writer, scan_generation, |conn| {
            pair_live_photos(conn, root_id)
        }) {
            Ok(_) => {}
            Err(AppError::Cancelled) => return Err(AppError::Cancelled),
            Err(e) => error!("Live Photo pairing error: {e}"),
        }
    }

    // ── 视频探测（§2.1 / §3.2）：真实宽高 + 旋转 + video_meta ───────────────────────
    // 布局关键：用真实比例（含旋转修正）替换 16:9 占位，使竖拍视频不躺倒、Justified Layout 正确。
    if !cancel.is_cancelled() {
        if let Err(e) = enrich_videos(
            app,
            writer,
            root_id,
            run_id,
            total,
            total_bytes,
            &mut enriched_total,
            &mut enriched_bytes,
            cancel,
            &probe_runtime,
            scan_generation,
        ) {
            if !matches!(e, AppError::Cancelled) {
                warn!("Video enrichment error: {e}");
            }
        }
    }

    // ── 音频探测（§3.6）：lofty 标签/歌词 + 时长 → audio_meta ───────────────────────
    // 音频比例固定 400×400（封面位，fast_scan 已设默认），无需回填尺寸；这里仅补元数据。
    if !cancel.is_cancelled() {
        if let Err(e) = enrich_audios(
            app,
            writer,
            root_id,
            run_id,
            total,
            total_bytes,
            &mut enriched_total,
            &mut enriched_bytes,
            cancel,
            scan_generation,
        ) {
            if !matches!(e, AppError::Cancelled) {
                warn!("Audio enrichment error: {e}");
            }
        }
    }

    // 富化收尾的分阶段汇总（每轮一次，全为结构化字段，可直接机械汇总）：EXIF 路径计数 +
    // 图片流水线的累计服务时间、空转等待与在途块峰值 + 批循环各段累计。单根口径：多根同时富化时
    // 各自独立统计，此峰值不构成全局上限；header/parse 在块级真重叠，累计服务时间不可相加当 wall。
    let stages = pipeline_timing.snapshot();
    info!(
        run_id,
        root_id,
        image_io_mode,
        exif_header = exif_header_total,
        exif_container_chunk = exif_chunk_total,
        exif_full_file_fallback = exif_fallback_total,
        exif_no_metadata = exif_no_metadata_total,
        exif_unsupported = exif_unsupported_total,
        exif_direct_file = exif_direct_file_total,
        exif_failed = exif_failed_total,
        header_read_ns = stages.header_read_ns,
        parse_ns = stages.parse_ns,
        queue_wait_ns = stages.consumer_wait_ns,
        producer_backpressure_ns = stages.producer_backpressure_ns,
        chunks_produced = stages.chunks_produced,
        chunks_consumed = stages.chunks_consumed,
        peak_inflight_chunks = stages.peak_inflight_chunks,
        select_ns_total,
        pipeline_wall_ns_total,
        write_wait_ns_total,
        write_hold_ns_total,
        publish_ns_total,
        "Enrichment stage summary (single root; stage times overlap) | 富化分段汇总(单根;累计服务时间不可相加当 wall)"
    );

    let elapsed_ms = started.elapsed().as_millis() as u64;
    info!("Enrichment complete: root_id={root_id} enriched={enriched_total} elapsed={elapsed_ms}ms | 增量补全完成: root_id={root_id} 补全={enriched_total} 耗时={elapsed_ms}ms");

    with_scan_action(scan_generation, || {
        let _ = app.emit(
            "enrichment:completed",
            EnrichmentCompletedPayload::ok(root_id, run_id, elapsed_ms),
        );
    })?;

    Ok(())
}

/// 探测 `root_id` 下缺 `video_meta` 的视频，回填真实宽高/旋转/时长 + 一行 `video_meta`（§2.1 / §3.2）。
/// 使用 `VideoBackend` 注册表（Windows 下为 Media Foundation）；无后端的平台/格式写一行最小
/// `video_meta`，避免反复探测（保留 16:9 占位，§9）。
#[allow(clippy::too_many_arguments)]
fn enrich_videos(
    app: &AppHandle,
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    total: i64,
    total_bytes: i64,
    enriched_total: &mut i64,
    enriched_bytes: &mut i64,
    cancel: &CancellationToken,
    probe_runtime: &VideoProbeRuntime,
    scan_generation: Option<ScanGeneration<'_>>,
) -> Result<()> {
    use std::path::Path;

    // 为前台保留一个核，使导入期布局保持跟手（见 helper）。
    let probe_pool = reserved_core_pool();
    let mut probe_summary = VideoProbeSummary::default();

    // keyset 游标:每批按 m.id 升序返回,末项 id 即下一批下界(首批 i64::MIN 覆盖全部正 id)。
    let mut after_id = i64::MIN;
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        ensure_scan_generation(scan_generation)?;

        let (batch, source_snapshots): (
            Vec<MediaEnrichmentPathInfo>,
            HashMap<i64, EnrichmentSourceSnapshot>,
        ) = {
            let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
            let batch = get_videos_needing_meta_with_retry(
                &conn,
                root_id,
                ENRICHMENT_BATCH,
                after_id,
                probe_runtime.has_worker_fallback(),
            )?;
            let source_snapshots =
                load_enrichment_source_snapshots(&conn, batch.iter().map(|(id, _, _, _)| *id))?;
            (batch, source_snapshots)
        };
        if batch.is_empty() {
            break;
        }
        if let Some((last_id, _, _, _)) = batch.last() {
            after_id = *last_id;
        }

        // 并行探测 —— 每个 MF 调用在各自的 rayon 线程上初始化 COM。跑在保留核的池上，使前台留有一个核（「布局被视频任务阻塞」的扫描期分支）。
        let probe = || -> Vec<(i64, String, VideoProbeOutcome)> {
            batch
                .par_iter()
                .map(|(id, abs, ext, _file_size)| {
                    (*id, ext.clone(), probe_runtime.probe(ext, Path::new(abs)))
                })
                .collect()
        };
        let probed: Vec<(i64, String, VideoProbeOutcome)> = match &probe_pool {
            Some(p) => p.install(probe),
            None => probe(),
        };

        with_scan_db_write(writer, scan_generation, |conn| {
            let tx = conn.unchecked_transaction()?;
            for (id, ext, outcome) in &probed {
                let Some(source_snapshot) = source_snapshots.get(id).copied() else {
                    continue;
                };
                if !enrichment_source_is_current(&tx, *id, source_snapshot)? {
                    debug!("Skipping stale video enrichment id={id} | 跳过已失效视频富化 id={id}");
                    continue;
                }
                probe_summary.observe(ext, outcome);
                match outcome.info.as_ref() {
                    Some(vi) if vi.width > 0 && vi.height > 0 => {
                        let dur = if vi.duration_ms > 0 {
                            Some(vi.duration_ms as i64)
                        } else {
                            None
                        };
                        let _ = update_video_dimensions(
                            &tx,
                            *id,
                            vi.width as i64,
                            vi.height as i64,
                            dur,
                        );
                        let _ = upsert_video_meta(
                            &tx,
                            *id,
                            vi.codec.as_deref(),
                            if vi.fps > 0.0 {
                                Some(vi.fps as f64)
                            } else {
                                None
                            },
                            if vi.bitrate > 0 {
                                Some(vi.bitrate as i64)
                            } else {
                                None
                            },
                            vi.rotation as i64,
                            vi.has_audio,
                        );
                    }
                    // 探测失败 / 不支持的容器：写最小行使 LEFT JOIN 不再选中（不无限重探）。与 image_meta 同理。
                    _ => {
                        let _ = upsert_video_meta(&tx, *id, None, None, None, 0, false);
                    }
                }
            }
            tx.commit()?;
            Ok(())
        })?;

        *enriched_total += probed.len() as i64;
        *enriched_bytes += batch
            .iter()
            .map(|(_, _, _, file_size)| *file_size)
            .sum::<i64>();
        // S1：视频真实宽高已回填（几何输入变化）→ bump；通知也绑定当前代次。
        with_scan_action(scan_generation, || {
            bump_layout_data_version(app);
            let _ = app.emit(
                "db:media_enriched",
                MediaEnrichedPayload {
                    root_id,
                    run_id: Some(run_id.to_string()),
                    enriched_count: *enriched_total,
                    total,
                    processed_bytes: *enriched_bytes,
                    total_bytes,
                },
            );
        })?;
    }

    if probe_summary.attempted > 0 {
        info!(
            "Video enrichment complete: root_id={root_id} attempted={} populated={} failed={} unsupported={} fallback_attempted={} fallback_populated={} formats={} | 视频探测完成: root_id={root_id} 尝试={} 成功={} 失败={} 不支持={} 回退尝试={} 回退成功={} 格式={}",
            probe_summary.attempted,
            probe_summary.populated,
            probe_summary.failed,
            probe_summary.unsupported,
            probe_summary.fallback_attempted,
            probe_summary.fallback_populated,
            probe_summary.format_breakdown(),
            probe_summary.attempted,
            probe_summary.populated,
            probe_summary.failed,
            probe_summary.unsupported,
            probe_summary.fallback_attempted,
            probe_summary.fallback_populated,
            probe_summary.format_breakdown(),
        );
    }
    Ok(())
}

/// 探测 `root_id` 下缺 `audio_meta` 的音频，用 `lofty` 回填标签/歌词来源 + 时长（§3.6）。与视频探测
/// 同理：即便失败也写行，使 LEFT JOIN 不再选中（不无限重探）。封面是独立派生（`kind=audio_cover`）；
/// 歌词**文本**由 `get_audio_detail` 懒加载，故此处仅持久化来源（embedded/lrc/none）+ `.lrc` 路径。
#[allow(clippy::too_many_arguments)]
fn enrich_audios(
    app: &AppHandle,
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    total: i64,
    total_bytes: i64,
    enriched_total: &mut i64,
    enriched_bytes: &mut i64,
    cancel: &CancellationToken,
    scan_generation: Option<ScanGeneration<'_>>,
) -> Result<()> {
    use crate::audio::{find_lrc, lyrics_source, read_tags, AudioTags};
    use std::path::Path;

    // 为前台保留一个核，使导入期布局保持跟手（见 helper）。
    let probe_pool = reserved_core_pool();

    // keyset 游标:同视频段,末项 id 即下一批下界。
    let mut after_id = i64::MIN;
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        ensure_scan_generation(scan_generation)?;

        let (batch, source_snapshots): (
            Vec<MediaEnrichmentPathInfo>,
            HashMap<i64, EnrichmentSourceSnapshot>,
        ) = {
            let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
            let batch = get_audios_needing_meta(&conn, root_id, ENRICHMENT_BATCH, after_id)?;
            let source_snapshots =
                load_enrichment_source_snapshots(&conn, batch.iter().map(|(id, _, _, _)| *id))?;
            (batch, source_snapshots)
        };
        if batch.is_empty() {
            break;
        }
        if let Some((last_id, _, _, _)) = batch.last() {
            after_id = *last_id;
        }

        // 并行读取标签（lofty 受 CPU/IO 限制，无共享状态）；跑在保留核的池上，使前台在导入期留有一个核。
        let probe = || -> Vec<(i64, AudioTags, &'static str, Option<String>)> {
            batch
                .par_iter()
                .map(|(id, abs, _ext, _file_size)| {
                    let path = Path::new(abs);
                    let tags = read_tags(path).unwrap_or_default();
                    let lrc = find_lrc(path);
                    let src = lyrics_source(&tags, &lrc);
                    let lrc_path = lrc.map(|p| p.to_string_lossy().replace('\\', "/"));
                    (*id, tags, src, lrc_path)
                })
                .collect()
        };
        let probed: Vec<(i64, AudioTags, &'static str, Option<String>)> = match &probe_pool {
            Some(p) => p.install(probe),
            None => probe(),
        };

        with_scan_db_write(writer, scan_generation, |conn| {
            let tx = conn.unchecked_transaction()?;
            for (id, tags, lyrics_src, lrc_path) in &probed {
                let Some(source_snapshot) = source_snapshots.get(id).copied() else {
                    continue;
                };
                if !enrichment_source_is_current(&tx, *id, source_snapshot)? {
                    debug!("Skipping stale audio enrichment id={id} | 跳过已失效音频富化 id={id}");
                    continue;
                }
                let _ = upsert_audio_meta(
                    &tx,
                    *id,
                    tags.codec.as_deref(),
                    tags.artist.as_deref(),
                    tags.album.as_deref(),
                    tags.title.as_deref(),
                    tags.track_no,
                    tags.year,
                    tags.genre.as_deref(),
                    Some(*lyrics_src),
                    lrc_path.as_deref(),
                );
                // 在主表回填时长（音频尺寸保持 400×400 默认）。
                if let Some(dur) = tags.duration_ms {
                    let _ = tx
                        .prepare_cached(
                            "UPDATE media_items SET duration_ms=?1 WHERE id=?2 AND media_type='audio'",
                        )
                        .and_then(|mut stmt| stmt.execute(rusqlite::params![dur, id]));
                }
            }
            tx.commit()?;
            Ok(())
        })?;

        *enriched_total += probed.len() as i64;
        *enriched_bytes += batch
            .iter()
            .map(|(_, _, _, file_size)| *file_size)
            .sum::<i64>();
        // S1：音频时长已回填（duration 徽标随布局行下发）→ bump；通知也绑定当前代次。
        with_scan_action(scan_generation, || {
            bump_layout_data_version(app);
            let _ = app.emit(
                "db:media_enriched",
                MediaEnrichedPayload {
                    root_id,
                    run_id: Some(run_id.to_string()),
                    enriched_count: *enriched_total,
                    total,
                    processed_bytes: *enriched_bytes,
                    total_bytes,
                },
            );
        })?;
    }

    if *enriched_total > 0 {
        info!("Audio enrichment complete for root_id={root_id} | 音频探测累计: root_id={root_id} 共 {} 个", *enriched_total);
    }
    Ok(())
}

/// 图片富化 keyset 游标:(本批末尾 sort_datetime, id)。两列共同组成稳定序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImageBatchCursor {
    sort_datetime: i64,
    id: i64,
}

/// 队列表调用代次:进程内单调递增,保证同根重扫/并发任务各自表名唯一。
fn next_queue_gen() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GEN: AtomicU64 = AtomicU64::new(1);
    GEN.fetch_add(1, Ordering::Relaxed)
}

/// 队列表 RAII 守卫:持有本调用代次的表名与 seq 游标,任务结束(正常/取消/出错)DROP
/// 自有表,免 TEMP(temp_store=MEMORY)滞留。表名含代次后缀——同根重扫时旧任务的收尾
/// DROP 绝不误删新任务刚建的表(固定表名在此有对照线实证过的窄窗口竞态)。
struct QueueGuard<'a> {
    writer: &'a Mutex<Connection>,
    table: String,
    seq: i64,
}

impl QueueGuard<'_> {
    fn new<'a>(writer: &'a Mutex<Connection>, table: String) -> QueueGuard<'a> {
        QueueGuard {
            writer,
            table,
            seq: 0,
        }
    }
}

impl Drop for QueueGuard<'_> {
    fn drop(&mut self) {
        let conn = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = conn.execute_batch(&format!("DROP TABLE IF EXISTS {};", self.table)) {
            warn!(
                "enrichment queue cleanup failed, TEMP 表留待连接关闭 | 队列表清理失败: table={} err={e}",
                self.table
            );
        }
    }
}

/// 构造队列表建表 SQL。seq 为 rowid 别名,随插入序递增。
fn image_queue_create_sql(table: &str) -> String {
    format!("CREATE TEMP TABLE {table}(seq INTEGER PRIMARY KEY, item_id INTEGER NOT NULL);")
}

/// 构造队列表灌入 SQL(不可 keyset 的视图序用):一次性按视图序 INSERT..SELECT(**含 ?1 =
/// root_id,须 prepare 绑定,不能走 execute_batch**——后者把未绑参数当 NULL,静默灌入空表)。
/// 此后取批按 seq 游标续取,每批不再重排。追加 `m.id {dir}` tiebreaker 与 fallback 排序口径一致。
fn image_queue_fill_sql(order_clause: &str, sort_order: &str, table: &str) -> String {
    let dir = if sort_order == "asc" { "ASC" } else { "DESC" };
    format!(
        "INSERT INTO {table}(item_id)
         SELECT m.id FROM media_items m
         LEFT JOIN image_meta im ON im.item_id = m.id
         JOIN directories d ON d.id = m.directory_id
         WHERE d.root_id=?1 AND m.is_deleted=0 AND m.media_type='image' AND im.item_id IS NULL
         {order_clause}, m.id {dir}"
    )
}

/// 构造队列表取批 SQL:按 seq 游标续取(主键序,无排序);重查 `is_deleted=0`,
/// 建表后软删的项跳过——与逐批查询的旧语义一致。第 6 列输出 q.seq 作游标键。
fn image_queue_fetch_sql(table: &str) -> String {
    format!(
        "SELECT m.id, m.width, m.height, r.path, d.rel_path, m.file_name, q.seq, m.file_size
         FROM {table} q
         JOIN media_items m ON m.id = q.item_id
         JOIN directories d ON d.id = m.directory_id
         JOIN scan_roots r ON r.id = d.root_id
         WHERE m.is_deleted=0 AND q.seq > ?2
         ORDER BY q.seq
         LIMIT ?1"
    )
}

/// 当前 ORDER BY 是否可用 media_items 的 `(sort_datetime,id)` keyset:
/// 仅「非 folder 轴 + 组内按 datetime」可表达为纯 sort_datetime/id 序;
/// folder 轴和 filename 次键需要目录树键/自然排序键,暂走 fallback。
fn image_keyset_applicable(group_by: &str, sort_within_group: &str) -> bool {
    group_by != "folder" && sort_within_group != "filename"
}

/// 构造图片富化选批 SQL。
///
/// - `keyset=true, with_cursor=true`:带 `(sort_datetime,id)` 游标谓词,消费
///   V24 `idx_media_type_sort` 直接 seek;
/// - `keyset=true, with_cursor=false`:首批,同序但不带游标谓词;
/// - `keyset=false`:原 fallback 查询,保留传入 `order_clause`,统一补 `m.id` 同向 tiebreaker。
fn image_batch_sql(
    keyset: bool,
    with_cursor: bool,
    order_clause: &str,
    sort_order: &str,
) -> String {
    let dir = if sort_order == "asc" { "ASC" } else { "DESC" };
    let cmp = if sort_order == "asc" { ">" } else { "<" };
    if keyset {
        let cursor = if with_cursor {
            format!(" AND (m.sort_datetime {cmp} ?3 OR (m.sort_datetime = ?3 AND m.id {cmp} ?4))")
        } else {
            String::new()
        };
        format!(
            "SELECT m.id, m.width, m.height, r.path, d.rel_path, m.file_name, m.sort_datetime, m.file_size
             FROM media_items m
             LEFT JOIN image_meta im ON im.item_id = m.id
             JOIN directories d ON d.id = m.directory_id
             JOIN scan_roots r ON r.id = d.root_id
             WHERE d.root_id=?1 AND m.is_deleted=0 AND m.media_type='image' AND im.item_id IS NULL
             {cursor}
             ORDER BY m.sort_datetime {dir}, m.id {dir}
             LIMIT ?2"
        )
    } else {
        format!(
            "SELECT m.id, m.width, m.height, r.path, d.rel_path, m.file_name, m.sort_datetime, m.file_size
             FROM media_items m
             LEFT JOIN image_meta im ON im.item_id = m.id
             JOIN directories d ON d.id = m.directory_id
             JOIN scan_roots r ON r.id = d.root_id
             WHERE d.root_id=?1 AND m.is_deleted=0 AND m.media_type='image' AND im.item_id IS NULL
             {order_clause}, m.id {dir}
             LIMIT ?2"
        )
    }
}

/// 构建 enrichment 批次的 ORDER BY，近似画廊视图顺序（对齐 `query_layout_geometry`，去掉
/// 导入期无数据的 AI 相似度分支）。入参取自固定选项集 → 无注入风险。
///
/// 注：folder 轴按持久列 `d.tree_sort_key`（方案 B），与画廊 folder 目录序在**同一根内**逐位
/// 一致的前序 DFS。enricher 按 root 分批（查询 `WHERE d.root_id=?1`），单根内 rel_path 唯一 →
/// tree_sort_key 唯一，故无需 scan_roots 根序前缀。升级前按 `d.rel_path` 字符串序是**近似**（会把
/// `A/Z` 排到 `A-` 之后而与画廊 DFS 割裂），现已从"就近"升为"根内精确"。仍**不在**内存/SQL 刚性
/// 等价契约内（背景富化优先级序、无选区语义），故 root 序前缀可省。
fn enrichment_order_clause(group_by: &str, sort_within_group: &str, sort_order: &str) -> String {
    let dir = if sort_order == "asc" { "ASC" } else { "DESC" };
    let secondary = if sort_within_group == "filename" {
        format!("m.file_name COLLATE NATURAL_CMP {dir}")
    } else {
        // 'datetime' (or 'similarity', which has no scores at import) → sort_datetime
        format!("m.sort_datetime {dir}")
    };
    match group_by {
        "folder" => format!("ORDER BY d.tree_sort_key ASC, {secondary}"),
        "date" => {
            if sort_within_group == "filename" {
                // B-file-iii / D-018 A′：去 `'localtime'` 改 UTC 日界，与画廊 `push_order_by` date+filename
                // 同源（sort_datetime = EXIF 墙钟当 UTC 存，UTC 桶才与显示标签/分组一致）。本子句是背景富化
                // 优先级近似、不在刚性等价契约内，此处对齐仅为消除 localtime 双重偏移的口径漂移。
                format!("ORDER BY date(m.sort_datetime,'unixepoch') {dir}, {secondary}")
            } else {
                format!("ORDER BY m.sort_datetime {dir}")
            }
        }
        _ => format!("ORDER BY {secondary}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_disk_is_a_batch_error_instead_of_minimal_metadata() {
        let budget = crate::scanner::hdd_io::test_budget(1);
        let cancel = CancellationToken::new();
        let permit = budget.acquire(&cancel).unwrap();
        permit.mark_timed_out();
        let mut context = ImageReadContext::with_disk_budget(Some(budget), &cancel);
        let path_info = (1, String::from("missing.jpg"), 0, 0, 0, 0);
        let result = parse_image_item_measured(
            &path_info,
            String::from("jpg"),
            None,
            &mut context,
            std::time::Duration::ZERO,
            None,
        );
        assert!(matches!(result, Err(AppError::ImageReadTimeout)));
    }

    fn video_info(width: u32, height: u32) -> crate::video::VideoInfo {
        crate::video::VideoInfo {
            width,
            height,
            duration_ms: 0,
            rotation: 0,
            fps: 0.0,
            bitrate: 0,
            has_audio: false,
            codec: None,
        }
    }

    #[test]
    fn video_probe_summary_separates_fallback_and_failure_counts() {
        let mut summary = VideoProbeSummary::default();
        summary.observe(
            "TS",
            &VideoProbeOutcome {
                info: Some(video_info(1920, 1080)),
                status: VideoProbeStatus::Succeeded,
                fallback_attempted: true,
            },
        );
        summary.observe(
            "TS",
            &VideoProbeOutcome {
                info: None,
                status: VideoProbeStatus::Failed,
                fallback_attempted: true,
            },
        );
        summary.observe(
            "MTS",
            &VideoProbeOutcome {
                info: None,
                status: VideoProbeStatus::Unsupported,
                fallback_attempted: false,
            },
        );
        summary.observe(
            "MP4",
            &VideoProbeOutcome {
                info: Some(video_info(0, 0)),
                status: VideoProbeStatus::Succeeded,
                fallback_attempted: false,
            },
        );

        assert_eq!(summary.attempted, 4);
        assert_eq!(summary.populated, 1);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.unsupported, 1);
        assert_eq!(summary.fallback_attempted, 2);
        assert_eq!(summary.fallback_populated, 1);
        assert_eq!(summary.by_format["ts"].failed, 1);
        assert_eq!(summary.by_format["mts"].unsupported, 1);
    }

    fn mem_db() -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        crate::db::register_custom_collations(&c).unwrap();

        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name) VALUES (10, 1, NULL, '', 'r');",
        )
        .unwrap();
        c
    }

    fn insert_image(c: &rusqlite::Connection, id: i64, sort_dt: i64) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, 10, ?2, 0, ?3, 'jpg', 'image', 0, 0, ?3, 0)",
            rusqlite::params![id, format!("{id}.jpg"), sort_dt],
        )
        .unwrap();
    }

    fn query_batch(
        c: &rusqlite::Connection,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Vec<(i64, i64)> {
        let mut stmt = c.prepare(sql).unwrap();
        stmt.query_map(params, |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(6)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    #[test]
    fn keyset_applies_only_to_datetime_axes() {
        assert!(image_keyset_applicable("date", "datetime"));
        assert!(image_keyset_applicable("none", "datetime"));
        assert!(!image_keyset_applicable("folder", "datetime"));
        assert!(!image_keyset_applicable("date", "filename"));

        let sql = image_batch_sql(true, true, "ORDER BY m.sort_datetime DESC", "desc");
        assert!(sql.contains("m.sort_datetime < ?3"));
        assert!(sql.contains("ORDER BY m.sort_datetime DESC, m.id DESC"));
        let fallback = image_batch_sql(false, false, "ORDER BY m.sort_datetime DESC", "desc");
        assert!(fallback.contains("ORDER BY m.sort_datetime DESC, m.id DESC"));

        // 计划回归:keyset 查询必须用 V24 复合索引直接 seek,不得再落 TEMP B-TREE 排序。
        let c = mem_db();
        let mut plan_stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan: Vec<String> = plan_stmt
            .query_map([&1i64, &500i64, &i64::MAX, &i64::MAX], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            plan.iter().any(|p| p.contains("idx_media_type_sort")),
            "keyset SQL 未使用 idx_media_type_sort: {plan:?}"
        );
        assert!(
            !plan.iter().any(|p| p.contains("TEMP B-TREE")),
            "keyset SQL 不应落 TEMP B-TREE: {plan:?}"
        );
    }

    #[test]
    fn keyset_pages_all_images_in_stable_order() {
        let c = mem_db();
        // 含同 sort_datetime 的同分项,验证 (sort,id) 双键不重不漏。
        insert_image(&c, 1, 300);
        insert_image(&c, 2, 300);
        insert_image(&c, 3, 200);
        insert_image(&c, 4, 100);
        insert_image(&c, 5, 50);
        insert_image(&c, 6, 50);

        let first = image_batch_sql(true, false, "", "desc");
        let next = image_batch_sql(true, true, "", "desc");

        let mut cursor: Option<(i64, i64)> = None;
        let mut got = Vec::new();
        for _ in 0..5 {
            let batch = match cursor {
                None => query_batch(&c, &first, &[&1i64, &2i64]),
                Some(cur) => query_batch(&c, &next, &[&1i64, &2i64, &cur.0, &cur.1]),
            };
            if batch.is_empty() {
                break;
            }
            got.extend(batch.iter().map(|(id, _)| *id));
            let (last_id, last_sort) = *batch.last().unwrap();
            cursor = Some((last_sort, last_id));
        }
        // 同 sort_datetime 的项按 id DESC 稳定裁决(与画廊布局 tiebreaker 同向)。
        assert_eq!(got, vec![2, 1, 3, 4, 6, 5], "keyset 应稳定分页且不重不漏");

        // 同集合旧 fallback 查询对照:结果序列一致(仅处理优先级序,不改变成员)。
        let fallback = image_batch_sql(false, false, "ORDER BY m.sort_datetime DESC", "desc");
        let all = query_batch(&c, &fallback, &[&1i64, &100i64]);
        assert_eq!(all.len(), 6);
        assert_eq!(all.iter().map(|(id, _)| *id).collect::<Vec<_>>(), got);
    }

    fn insert_image_in(c: &rusqlite::Connection, id: i64, dir: i64, name: &str, sort_dt: i64) {
        c.execute(
            "INSERT INTO media_items
                (id, directory_id, file_name, file_size, file_mtime, file_format,
                 media_type, width, height, sort_datetime, cache_key)
             VALUES (?1, ?2, ?3, 0, ?4, 'jpg', 'image', 0, 0, ?4, 0)",
            rusqlite::params![id, dir, name, sort_dt],
        )
        .unwrap();
    }

    /// ground truth:与批选同谓词的全量同序查询(等价性对拍基准)。candidate 集须在
    /// drain 写 image_meta 之前取(写后谓词集变空)。
    fn full_order_ids(c: &rusqlite::Connection, order_sql: &str) -> Vec<i64> {
        let sql = format!(
            "SELECT m.id FROM media_items m
             LEFT JOIN image_meta im ON im.item_id = m.id
             JOIN directories d ON d.id = m.directory_id
             WHERE d.root_id=1 AND m.is_deleted=0 AND m.media_type='image' AND im.item_id IS NULL
             {order_sql}"
        );
        let mut stmt = c.prepare(&sql).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    /// 按生产口径建队:CREATE 走 batch,INSERT..SELECT 经 prepare 绑定 root_id
    /// (execute_batch 把未绑参数当 NULL,会静默灌入空表——本测试就是要锁住这条生产路径)。
    fn fill_queue(c: &rusqlite::Connection, order_clause: &str, sort_order: &str, table: &str) {
        c.execute_batch(&image_queue_create_sql(table)).unwrap();
        c.prepare(&image_queue_fill_sql(order_clause, sort_order, table))
            .unwrap()
            .execute(rusqlite::params![1i64])
            .unwrap();
    }

    /// 不可 keyset 序(folder)走队列表:按 seq 游标取批的完整序列 == 同序全量查询(对拍);
    /// 多目录 tree_sort_key 真实编码 + 同分项 id 裁决 + 取批计划无 TEMP B-TREE。
    #[test]
    fn queue_pages_folder_order_matching_full_view_order() {
        let c = mem_db();
        let tsk = crate::utils::path::encode_tree_sort_key;
        // 三个目录:根('')、a、b——真实 tree_sort_key 编码决定 folder 视图序(根 < a < b)。
        for (id, rel) in [(10u64, ""), (11u64, "a"), (12u64, "b")] {
            c.execute(
                "INSERT OR REPLACE INTO directories (id, root_id, parent_id, rel_path, name, tree_sort_key)
                 VALUES (?1, 1, NULL, ?2, ?2, ?3)",
                rusqlite::params![id as i64, rel, tsk(rel)],
            )
            .unwrap();
        }
        insert_image_in(&c, 1, 10, "root.jpg", 300);
        insert_image_in(&c, 2, 11, "a2.jpg", 100);
        insert_image_in(&c, 3, 11, "a3.jpg", 100); // 同目录同 sort → id DESC 裁决
        insert_image_in(&c, 4, 12, "b1.jpg", 200);
        insert_image_in(&c, 5, 12, "b2.jpg", 999); // 已删 → 不入队
        c.execute("UPDATE media_items SET is_deleted=1 WHERE id=5", [])
            .unwrap();
        c.execute("INSERT INTO image_meta (item_id) VALUES (1)", []) // 已富化 → 不入队
            .unwrap();

        let order_clause = enrichment_order_clause("folder", "datetime", "desc");
        assert!(
            !image_keyset_applicable("folder", "datetime"),
            "前置:folder 序不可 keyset,须走队列表"
        );
        let table = "_enrich_q_test1";
        fill_queue(&c, &order_clause, "desc", table);

        // 对拍基准在建队后、写回前取。
        let expected = full_order_ids(&c, &format!("{order_clause}, m.id DESC"));

        let fetch_sql = image_queue_fetch_sql(table);
        let mut seq = 0i64;
        let mut got = Vec::new();
        for _ in 0..10 {
            let batch = query_batch(&c, &fetch_sql, &[&2i64, &seq]); // 小批压多轮推进
            if batch.is_empty() {
                break;
            }
            for (id, _) in &batch {
                // 模拟生产写回:该项退出候选谓词。
                c.execute("INSERT INTO image_meta (item_id) VALUES (?1)", [*id])
                    .unwrap();
                got.push(*id);
            }
            seq = batch.last().unwrap().1;
        }
        assert_eq!(got, expected, "队列表 seq 游标分页应与全量视图序逐项一致");
        assert_eq!(
            got,
            vec![3, 2, 4],
            "folder 序:树键根 < a < b;id=1 已富化、id=5 已删均不入队;a 内同 sort 按 id DESC"
        );

        // 计划回归:取批走队列表主键序,不得落 TEMP B-TREE 排序。
        let mut plan_stmt = c
            .prepare(&format!("EXPLAIN QUERY PLAN {fetch_sql}"))
            .unwrap();
        let plan: Vec<String> = plan_stmt
            .query_map(rusqlite::params![2i64, 0i64], |row| row.get::<_, String>(3))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            !plan.iter().any(|p| p.contains("TEMP B-TREE")),
            "队列表取批不应落 TEMP B-TREE: {plan:?}"
        );
    }

    /// date+filename 序走队列表:建队后软删的项在取批时跳过(与逐批查询的旧语义一致)。
    #[test]
    fn queue_fetch_skips_items_soft_deleted_after_build() {
        let c = mem_db();
        insert_image_in(&c, 1, 10, "a.jpg", 100);
        insert_image_in(&c, 2, 10, "b.jpg", 100);
        insert_image_in(&c, 3, 10, "c.jpg", 100);

        let order_clause = enrichment_order_clause("date", "filename", "desc");
        assert!(!image_keyset_applicable("date", "filename"));
        let table = "_enrich_q_test2";
        fill_queue(&c, &order_clause, "desc", table);

        // 建队后软删 id=2:队列行仍在,取批必须跳过。
        c.execute("UPDATE media_items SET is_deleted=1 WHERE id=2", [])
            .unwrap();
        let expected = full_order_ids(&c, &format!("{order_clause}, m.id DESC"));

        let fetch_sql = image_queue_fetch_sql(table);
        let mut seq = 0i64;
        let mut got = Vec::new();
        loop {
            let batch = query_batch(&c, &fetch_sql, &[&1i64, &seq]);
            if batch.is_empty() {
                break;
            }
            for (id, _) in &batch {
                c.execute("INSERT INTO image_meta (item_id) VALUES (?1)", [*id])
                    .unwrap();
                got.push(*id);
            }
            seq = batch.last().unwrap().1;
        }
        assert_eq!(got, expected);
        assert!(!got.contains(&2), "建队后软删的项不得被取批");
    }

    /// QueueGuard RAII:作用域结束 DROP 自有表——正常/取消/出错路径统一的清理契约。
    #[test]
    fn queue_guard_drops_own_table_on_scope_exit() {
        let c = mem_db();
        let writer = Mutex::new(c);
        let table = "_enrich_q_guard_t";
        {
            let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
            conn.execute_batch(&format!("CREATE TEMP TABLE {table}(x);"))
                .unwrap();
        }
        {
            let _g = QueueGuard::new(&writer, table.to_string());
            let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name=?1",
                    rusqlite::params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "守卫存活期间表在");
        }
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name=?1",
                rusqlite::params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "守卫离开作用域后自有表应被 DROP");
    }

    // ── 图片批流水线：真实文件表征与对照 ──────────────────────────────────────

    /// 最小 EXIF/TIFF 块（II 小端，IFD0 仅一条 Orientation=SHORT），供 image 编码器写进 JPEG/PNG。
    fn orientation_tiff(orientation: u16) -> Vec<u8> {
        let mut tiff = b"II".to_vec();
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0u8; 2]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff
    }

    fn write_oriented_jpeg(path: &std::path::Path, orientation: u16) {
        use image::codecs::jpeg::JpegEncoder;
        use image::{ExtendedColorType, ImageEncoder, RgbImage};
        let img = RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 5) as u8, 90])
        });
        let mut out = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut out, 85);
        enc.set_exif_metadata(orientation_tiff(orientation))
            .unwrap();
        enc.write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn write_oriented_png(path: &std::path::Path, orientation: u16) {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder, RgbImage};
        let img = RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([40, (y * 4) as u8, (x * 2) as u8])
        });
        let mut out = Vec::new();
        let mut enc = PngEncoder::new(&mut out);
        enc.set_exif_metadata(orientation_tiff(orientation))
            .unwrap();
        enc.write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn enrich_pool(threads: usize) -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
    }

    /// 头读载荷的活跃统计：进入流水线 +1/+bytes，解析完成（载荷 Drop）归还，并记录峰值。
    #[derive(Default)]
    struct HeaderPeak {
        live: std::sync::atomic::AtomicUsize,
        live_bytes: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
        peak_bytes: std::sync::atomic::AtomicUsize,
        max_item_bytes: std::sync::atomic::AtomicUsize,
        produced: std::sync::atomic::AtomicUsize,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct HeaderPeakSnapshot {
        live: usize,
        live_bytes: usize,
        peak: usize,
        peak_bytes: usize,
        max_item_bytes: usize,
        produced: usize,
    }

    impl HeaderPeak {
        fn add(&self, bytes: usize) {
            use std::sync::atomic::Ordering::SeqCst;
            let live = self.live.fetch_add(1, SeqCst) + 1;
            let live_bytes = self.live_bytes.fetch_add(bytes, SeqCst) + bytes;
            self.peak.fetch_max(live, SeqCst);
            self.peak_bytes.fetch_max(live_bytes, SeqCst);
            self.max_item_bytes.fetch_max(bytes, SeqCst);
            self.produced.fetch_add(1, SeqCst);
        }

        fn snapshot(&self) -> HeaderPeakSnapshot {
            use std::sync::atomic::Ordering::SeqCst;
            HeaderPeakSnapshot {
                live: self.live.load(SeqCst),
                live_bytes: self.live_bytes.load(SeqCst),
                peak: self.peak.load(SeqCst),
                peak_bytes: self.peak_bytes.load(SeqCst),
                max_item_bytes: self.max_item_bytes.load(SeqCst),
                produced: self.produced.load(SeqCst),
            }
        }
    }

    /// 包住一项头读结果，解析完（Drop）即记一次释放——用于核对全部退出路径的句柄/缓冲归还。
    struct TrackedHeader {
        inner: Option<ImageHeaderRead>,
        bytes: usize,
        stats: std::sync::Arc<HeaderPeak>,
    }

    impl TrackedHeader {
        fn read(
            index: usize,
            path_infos: &[ImageEnrichmentPathInfo],
            stats: &std::sync::Arc<HeaderPeak>,
        ) -> Self {
            let (_, abs_path, _, _, _, file_size) = &path_infos[index];
            let inner = read_image_header(std::path::Path::new(abs_path), *file_size);
            let bytes = inner.1.as_ref().map(|r| r.header.bytes.len()).unwrap_or(0);
            stats.add(bytes);
            Self {
                inner: Some(inner),
                bytes,
                stats: std::sync::Arc::clone(stats),
            }
        }

        fn take(&mut self) -> ImageHeaderRead {
            self.inner.take().expect("每项头读只被解析一次")
        }
    }

    impl Drop for TrackedHeader {
        fn drop(&mut self) {
            use std::sync::atomic::Ordering::SeqCst;
            self.stats.live.fetch_sub(1, SeqCst);
            self.stats.live_bytes.fetch_sub(self.bytes, SeqCst);
        }
    }

    fn parse_fingerprints(results: &[ImageParseResult]) -> Vec<String> {
        results
            .iter()
            .map(|(id, meta, is_live, embedded, dims, path)| {
                let meta = match meta {
                    Ok(m) => format!("ok:{m:?}"),
                    Err(e) => format!("err:{e}"),
                };
                format!("{id}|{meta}|{is_live}|{embedded}|{dims:?}|{path:?}")
            })
            .collect()
    }

    /// 用同一套 read/parse（都带 [`TrackedHeader`] 统计包装）分别跑两条批形态，避免只给其中一组
    /// 加统计开销的不公平对照。
    fn run_bulk(
        path_infos: &[ImageEnrichmentPathInfo],
        header_pool: &rayon::ThreadPool,
        parse_pool: &rayon::ThreadPool,
        stats: &std::sync::Arc<HeaderPeak>,
    ) -> Vec<ImageParseResult> {
        super::image_pipeline::run_bulk_read_then_parse(
            path_infos.len(),
            Some(header_pool),
            Some(parse_pool),
            |index| TrackedHeader::read(index, path_infos, stats),
            |index, mut tracked| {
                let (ext, header_read) = tracked.take();
                parse_image_item(&path_infos[index], ext, header_read)
            },
        )
    }

    /// 真实 JPEG/PNG（含 EXIF Orientation）+ 缺失文件 + 坏 JPEG 的混合批次：
    /// 新有界流水线与旧整批路径逐项等价，且在途头读项数/字节有界、收工后全部释放。
    /// 两条路径都套同一 [`TrackedHeader`] 包装，在途峰值对比才可比。
    #[test]
    fn bounded_pipeline_matches_bulk_reference_on_real_files() {
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let jpeg = dir.path().join("oriented.jpg");
        let png = dir.path().join("oriented.png");
        let broken = dir.path().join("broken.jpg");
        let missing = dir.path().join("missing.jpg");
        write_oriented_jpeg(&jpeg, 6);
        write_oriented_png(&png, 8);
        std::fs::write(&broken, b"not a real jpeg at all").unwrap();

        // 200 项 = 4 块(64+64+64+8)：在途头读上界(3 块)应严格低于整批。
        let sources = [jpeg.clone(), png.clone(), missing.clone(), broken.clone()];
        let path_infos: Vec<ImageEnrichmentPathInfo> = (0..200)
            .map(|i| {
                let path = &sources[i % sources.len()];
                let file_size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
                let placeholder = i % 7 == 0;
                (
                    i as i64 + 1,
                    path.to_string_lossy().to_string(),
                    if placeholder { 0 } else { 320 },
                    if placeholder { 0 } else { 240 },
                    i as i64,
                    file_size,
                )
            })
            .collect();

        let header_pool = enrich_pool(4);
        let parse_pool = enrich_pool(4);
        let timing = PipelineTiming::default();
        let bounded_stats = Arc::new(HeaderPeak::default());
        let bulk_stats = Arc::new(HeaderPeak::default());
        let read_stats = Arc::clone(&bounded_stats);
        let cancel = CancellationToken::new();

        let piped: Vec<ImageParseResult> = run_bounded_header_parse(
            path_infos.len(),
            &cancel,
            Some(&header_pool),
            Some(&parse_pool),
            &timing,
            |index| TrackedHeader::read(index, &path_infos, &read_stats),
            |index, mut tracked| {
                let (ext, header_read) = tracked.take();
                parse_image_item(&path_infos[index], ext, header_read)
            },
        )
        .expect("真实文件批不应取消");
        let bulk = run_bulk(&path_infos, &header_pool, &parse_pool, &bulk_stats);

        assert_eq!(
            parse_fingerprints(&piped),
            parse_fingerprints(&bulk),
            "流水线与整批路径必须逐项等价"
        );

        // 自动调度复用同组真实文件，逐项核对元数据、尺寸和失败路径。
        let budget = crate::scanner::hdd_io::test_budget(4);
        let adaptive = run_bounded_header_parse(
            path_infos.len(),
            &cancel,
            Some(&header_pool),
            Some(&parse_pool),
            &PipelineTiming::default(),
            |index| {
                let _permit = budget.acquire_header(&cancel).unwrap();
                let (_, path, _, _, _, file_size) = &path_infos[index];
                read_image_header(std::path::Path::new(path), *file_size)
            },
            |index, (ext, header)| {
                let mut context = ImageReadContext::with_disk_budget(Some(budget.clone()), &cancel);
                parse_image_item_measured(
                    &path_infos[index],
                    ext,
                    header,
                    &mut context,
                    std::time::Duration::ZERO,
                    None,
                )
            },
        )
        .unwrap()
        .into_iter()
        .collect::<Result<Vec<_>>>()
        .unwrap();
        assert_eq!(parse_fingerprints(&adaptive), parse_fingerprints(&bulk));

        // 有效样本确实被解析出 EXIF：JPEG(EXIF APP1)方向为 6。
        let jpeg_parsed = piped
            .iter()
            .filter(|(id, ..)| (*id - 1) % 4 == 0)
            .collect::<Vec<_>>();
        assert_eq!(jpeg_parsed.len(), 50);
        assert!(
            jpeg_parsed
                .iter()
                .all(|(_, meta, ..)| meta.as_ref().is_ok_and(|m| m.orientation == 6)),
            "JPEG 样本应解析出 EXIF Orientation=6"
        );
        // 缺失与坏文件仍走失败/minimal 路径且不 panic（两者结果已在上方对照中一致）。
        assert!(
            piped
                .iter()
                .filter(|(id, ..)| (*id - 1) % 4 == 3)
                .all(|(_, meta, ..)| meta.is_err()),
            "坏 JPEG 应记为解析失败"
        );

        let snapshot = timing.snapshot();
        assert_eq!(snapshot.in_flight_chunks, 0, "收工后在途块必须归零");
        assert!(snapshot.peak_inflight_chunks <= MAX_INFLIGHT_HEADER_CHUNKS);
        let headers = bounded_stats.snapshot();
        assert_eq!(headers.produced, path_infos.len(), "每项恰好读一次头");
        assert_eq!(headers.live, 0, "收工后不得保留头缓冲与文件句柄");
        assert_eq!(headers.live_bytes, 0);
        assert!(
            headers.peak <= MAX_INFLIGHT_HEADER_CHUNKS * HEADER_CHUNK_ITEMS,
            "在途头读项数不得超过三块: {}",
            headers.peak
        );
        assert!(
            headers.peak < path_infos.len(),
            "在途头读项数应严格低于整批: {}",
            headers.peak
        );
        assert!(
            headers.peak_bytes
                <= MAX_INFLIGHT_HEADER_CHUNKS * HEADER_CHUNK_ITEMS * headers.max_item_bytes,
            "在途头读字节不得超过三块上限: {}",
            headers.peak_bytes
        );
        // 旧整批路径的在途峰值等于整批（全部头读先收齐再解析），这是本次改动的对照基线。
        let bulk_headers = bulk_stats.snapshot();
        assert_eq!(
            bulk_headers.peak,
            path_infos.len(),
            "整批路径在途头读等于批量"
        );
        assert_eq!(bulk_headers.live, 0, "整批路径收工后同样不保留头读");
    }

    /// 微基准（手动运行，不作速度断言）：默认批量（[`IMAGE_ENRICHMENT_BATCH`] 项）下对照
    /// 「旧整批头读→解析」与「有界小块流水线」的多轮墙钟与在途头读峰值，供判断收益来源。
    ///
    /// 语料是 32 个真实文件（16 JPEG + 16 PNG，均带 EXIF Orientation）在临时目录里的**暖缓存重复**：
    /// 同一文件被多次读头，缓存命中下 I/O 成本远低于首次入库的冷缓存真实库，故这里只反映批内形态
    /// 差异，不能外推成整库提速倍数。两组套同一 [`TrackedHeader`] 统计包装，轮次交替先后顺序。
    /// 运行：`cargo test -p scrollery --lib microbench_bulk_vs_bounded -- --ignored --nocapture`
    #[test]
    #[ignore = "微基准:手动运行,打印暖缓存对照"]
    fn microbench_bulk_vs_bounded_pipeline_warm_cache() {
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let mut sources = Vec::new();
        for i in 0..16 {
            let jpeg = dir.path().join(format!("bench-{i}.jpg"));
            let png = dir.path().join(format!("bench-{i}.png"));
            write_oriented_jpeg(&jpeg, 6);
            write_oriented_png(&png, 8);
            sources.push(jpeg);
            sources.push(png);
        }
        let items = IMAGE_ENRICHMENT_BATCH as usize;
        let path_infos: Vec<ImageEnrichmentPathInfo> = (0..items)
            .map(|i| {
                let path = &sources[i % sources.len()];
                let file_size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
                (
                    i as i64 + 1,
                    path.to_string_lossy().to_string(),
                    0,
                    0,
                    i as i64,
                    file_size,
                )
            })
            .collect();

        let header_pool = enrich_pool(8);
        let parse_pool = enrich_pool(8);
        let bulk_stats = Arc::new(HeaderPeak::default());
        let bounded_stats = Arc::new(HeaderPeak::default());
        let timing = PipelineTiming::default();
        const ROUNDS: usize = 3;
        let mut bulk_ms = Vec::new();
        let mut bounded_ms = Vec::new();

        let run_bounded = |stats: &Arc<HeaderPeak>| -> Vec<ImageParseResult> {
            let read_stats = Arc::clone(stats);
            run_bounded_header_parse(
                path_infos.len(),
                &CancellationToken::new(),
                Some(&header_pool),
                Some(&parse_pool),
                &timing,
                |index| TrackedHeader::read(index, &path_infos, &read_stats),
                |index, mut tracked| {
                    let (ext, header_read) = tracked.take();
                    parse_image_item(&path_infos[index], ext, header_read)
                },
            )
            .expect("微基准批不应取消")
        };

        // 第 0 轮为暖缓存预热，不计入统计；其余轮次两组交替先后，避免固定顺序吃亏。
        for round in 0..=ROUNDS {
            let bulk_first = round % 2 == 0;
            // 两组用同一 TrackedHeader 包装，只比批形态；先后顺序按轮次交替。
            let timing_of = |stats: &Arc<HeaderPeak>, bounded: bool| {
                let started = std::time::Instant::now();
                let results = if bounded {
                    run_bounded(stats)
                } else {
                    run_bulk(&path_infos, &header_pool, &parse_pool, stats)
                };
                (started.elapsed().as_secs_f64() * 1000.0, results)
            };
            let (first, second) = if bulk_first {
                (
                    timing_of(&bulk_stats, false),
                    timing_of(&bounded_stats, true),
                )
            } else {
                (
                    timing_of(&bounded_stats, true),
                    timing_of(&bulk_stats, false),
                )
            };
            let (bulk_round, bounded_round) = if bulk_first {
                (first, second)
            } else {
                (second, first)
            };
            let (bulk_ms_round, bulk_results) = bulk_round;
            let (bounded_ms_round, bounded_results) = bounded_round;
            assert_eq!(
                parse_fingerprints(&bulk_results),
                parse_fingerprints(&bounded_results),
                "微基准:两条路径结果必须逐项一致"
            );
            assert_eq!(bulk_stats.snapshot().live, 0, "每轮结束不得保留头读");
            assert_eq!(bounded_stats.snapshot().live, 0, "每轮结束不得保留头读");
            if round > 0 {
                bulk_ms.push(bulk_ms_round);
                bounded_ms.push(bounded_ms_round);
            }
        }

        let median = |mut values: Vec<f64>| {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap());
            values[values.len() / 2]
        };
        let stages = timing.snapshot();
        println!(
            "microbench(items={}, sources={}, rounds={ROUNDS}, warm synthetic repeated files, order alternated): bulk_rounds_ms={bulk_ms:?} bounded_rounds_ms={bounded_ms:?}",
            path_infos.len(),
            sources.len(),
        );
        println!(
            "microbench median_ms: bulk={:.2} bounded={:.2} (ratio={:.2}x)",
            median(bulk_ms.clone()),
            median(bounded_ms.clone()),
            median(bulk_ms) / median(bounded_ms),
        );
        println!(
            "microbench in-flight headers: bulk_peak={} bounded_peak={} (batch={}) | bounded stages: header_read_ms={:.1} parse_ms={:.1} queue_wait_ms={:.1} producer_backpressure_ms={:.1} peak_chunks={}",
            bulk_stats.snapshot().peak,
            bounded_stats.snapshot().peak,
            path_infos.len(),
            stages.header_read_ns as f64 / 1e6,
            stages.parse_ns as f64 / 1e6,
            stages.consumer_wait_ns as f64 / 1e6,
            stages.producer_backpressure_ns as f64 / 1e6,
            stages.peak_inflight_chunks,
        );
    }
}
