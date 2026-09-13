// src-tauri/src/scanner/fast_scan.rs
//! 阶段 1 快速扫描：轻量级单文件操作，立即插入数据库。
//!
//! 单文件工作（全部为 CPU 密集型，由 rayon 处理）：
//!   1. `image::image_dimensions()` → 从文件头获取宽度/高度（无解码）
//!   2. JPEG：读取方向标签（前 ~1KB）→ 如果需要则交换宽高
//!   3. TIFF：应用 50ms 超时保护
//!   4. `compute_cache_key`
//!   5. 批量 INSERT 到 `media_items`（500 行/事务）
//!
//! 完成后，通过 Tauri 频道发送 `ScanCompletedPayload`。

use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::db::queries::{
    cleanup_scan_temp_tables_for_run, finish_scan_root, get_config, init_seen_table_for_run,
    invalidate_exotic_tasks_for_item, load_directory_mtime_snapshot,
    mark_missing_preloaded_for_run, resolve_suspect_change, seed_exotic_tasks_for_item, set_config,
    set_directory_media_counts, update_scan_root_status, upsert_directory, upsert_fast_scan_item,
    FastScanItem, SeenWriter, UpsertOutcome,
};
#[cfg(test)]
use crate::db::queries::{init_seen_table, mark_missing};
use crate::error::{AppError, Result};
use crate::exotic::catalog::CatalogSnapshot;
use crate::scanner::metadata::{
    read_header_buf_for_ext, read_image_dimensions, read_image_dimensions_buf,
};
use crate::scanner::volume_probe::{PathProber, VolumeOnlineCheck};
use crate::scanner::walker::{MediaWalker, WalkedFile};
use crate::state::AppState;
use crate::utils::format::{classify_media_type, is_phase1_image, MediaType};
use crate::utils::hash::compute_cache_key_with_mtime_ns;
use crate::utils::path::{dir_rel_path, normalize_db_path, path_depth};

mod payload;
#[cfg(test)]
mod tests;

pub use payload::{
    ScanChannelPayload, ScanCompletedPayload, ScanErrorPayload, ScanProgressPayload,
};

const BATCH_SIZE: usize = 500;

/// 即时提取真实尺寸的"首屏项"数量（覆盖前几屏）。其余以 0×0 占位入库
/// （布局按正方形渲染），稍后由 enrichment 补全 —— 这样海量导入不再被
/// "逐个文件提尺寸"阻塞，同时首屏不会发生重排。
const EAGER_DIM_COUNT: usize = 500;

// ── Per-file dimension extraction ─────────────────────────────────────────────
// ── 单文件尺寸提取 ─────────────────────────────────────────────

struct FileInfo {
    walked: WalkedFile,
    width: i64,
    height: i64,
}

/// 阶段 2 媒体（音频/文档/视频）的廉价、无需读文件的占位尺寸。
/// 阶段 1 图像返回 `None`（需要真实读取文件头）。
///
/// 按类型给「默认宽高」，避免非图片项以 0×0 进入 Justified Layout 导致布局错乱（§2.1）。
/// `query_layout_geometry` 对所有类型读取 width/height，0×0 会让布局把整屏算崩。
/// 真实宽高随后在补全/派生阶段回填（视频走 MF probe + rotation 修正、音频走 lofty、
/// 文档走子类型探测），这里只保证「先有一个合理的占位比例」。
fn cheap_phase2_dimensions(walked: &WalkedFile) -> Option<(i64, i64)> {
    if is_phase1_image(walked.extension.as_str()) {
        return None;
    }
    // 穷举所有变体（不用 `_`）：将来新增类型时编译器会强制处理默认尺寸。
    Some(match walked.media_type {
        // 视频默认 16:9，补全后回填真实值（含 rotation 交换，竖拍视频否则会躺倒）。
        MediaType::Video => (1280, 720),
        // 音频用方形封面位。
        MediaType::Audio => (400, 400),
        // 文档：PDF 用 A4 比例（595×842），其它（svg/txt/md/office…）用方形占位。
        MediaType::Document => {
            if walked.extension == "pdf" {
                (595, 842)
            } else {
                (400, 400)
            }
        }
        // 图像不会走到这里（上面已 return None）；保险给方形占位。
        MediaType::Image => (400, 400),
    })
}

/// 单文件的真实尺寸（阶段 2 → 廉价常量；阶段 1 图像 → 经方向校正的文件头读取）。
///
/// 阶段5:eager 路径对非 TIFF 图像先读一次 HeaderBuf,尺寸 + JPEG orientation 共用,
/// 消除旧路径「尺寸 open 一次 + orientation 再 open 一次」;TIFF 保持原路径(IFD 任意
/// 偏移 + 超时守卫)。头读失败回退旧路径,正确性不变。
fn extract_dimensions(walked: &WalkedFile) -> (i64, i64) {
    if let Some(dims) = cheap_phase2_dimensions(walked) {
        return dims;
    }
    let ext = walked.extension.as_str();
    if ext == "tif" || ext == "tiff" {
        return read_image_dimensions(&walked.abs_path, ext);
    }
    match read_header_buf_for_ext(&walked.abs_path, ext) {
        Ok(hb) => read_image_dimensions_buf(&walked.abs_path, ext, &hb),
        Err(_) => read_image_dimensions(&walked.abs_path, ext),
    }
}

// ── Main fast scan entry point ────────────────────────────────────────────────
// ── 快速扫描主入口点 ────────────────────────────────────────────────

/// 快扫各阶段耗时的**局部**累加器（微秒）。
///
/// 只为回答「时间花在遍历、eager 尺寸、写库还是可疑指纹」这一个问题：收尾打一行 `info!` summary，
/// 每批打一行 `debug!`。不引入全局统计框架、不改 IPC 负载，也不打印路径或逐文件日志。
/// 分段不是时间轴上的划分（一次文件 I/O 会同时计入遍历或 eager 段），除下列各段外仍有准备、
/// 状态写回与调度等未单列开销，故各段只作相对比较用。
#[derive(Default)]
struct ScanPhaseTimings {
    /// 从 walker 拉取（目录遍历与单文件 stat 的 I/O 等待都在这里）。
    walk_us: u128,
    /// eager 真实尺寸/头读（rayon 并行，仅最先入库的前 `EAGER_DIM_COUNT` 项）。
    eager_us: u128,
    /// 批事务写库：**含**等 writer 锁与 generation 闸门的时间（本轮未分离锁等待，故不等于
    /// 纯 DB 执行时间）。
    db_us: u128,
    /// 单批写库最大耗时——批间偶发慢批靠它暴露。
    db_max_us: u128,
    /// 可疑变更定案：指纹读文件 + 定案短写。
    suspect_us: u128,
    /// 收尾：缺失检测 + 目录计数/根状态/基线标记的原子提交。
    finalize_us: u128,
    batches: usize,
    /// 触发内容指纹的可疑变更项数。
    suspects: usize,
    /// 已定案的可疑变更短写次数。
    suspect_commits: usize,
}

impl ScanPhaseTimings {
    fn record_db(&mut self, batch_us: u128) {
        self.db_us += batch_us;
        self.db_max_us = self.db_max_us.max(batch_us);
    }

    /// 收尾 summary（唯一一行分段汇总；结构化字段便于机械汇总，不含路径）。
    /// 带 root_id / generation / run_id：多根并发或同根重启时，分段数据按轮次区分不混在一起。
    fn log_summary(
        &self,
        root_id: i64,
        generation: u64,
        run_id: &str,
        inserted: u64,
        elapsed_ms: u64,
    ) {
        info!(
            root_id,
            generation,
            run_id,
            inserted,
            elapsed_ms,
            batches = self.batches as u64,
            walk_us = self.walk_us as u64,
            eager_us = self.eager_us as u64,
            db_us = self.db_us as u64,
            db_max_us = self.db_max_us as u64,
            suspect_us = self.suspect_us as u64,
            suspects = self.suspects as u64,
            suspect_commits = self.suspect_commits as u64,
            finalize_us = self.finalize_us as u64,
            "Fast scan phase summary | 快速扫描分段耗时"
        );
    }
}

fn ensure_dir_chain(
    tx: &rusqlite::Transaction,
    root_id: i64,
    rel_path: &str,
    dir_cache: &mut std::collections::HashMap<String, i64>,
    root_name: &str,
    dir_mtime_cache: &std::collections::HashMap<String, Option<i64>>,
) -> Result<i64> {
    if let Some(&id) = dir_cache.get(rel_path) {
        return Ok(id);
    }
    let parent_id = if rel_path.is_empty() {
        None
    } else {
        let p = Path::new(rel_path);
        let p_rel = p
            .parent()
            .map(|p| normalize_db_path(&p.to_string_lossy()))
            .unwrap_or_default();
        Some(ensure_dir_chain(
            tx,
            root_id,
            &p_rel,
            dir_cache,
            root_name,
            dir_mtime_cache,
        )?)
    };

    let dir_name = if rel_path.is_empty() {
        root_name.to_string()
    } else {
        Path::new(rel_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    let depth = path_depth(rel_path);

    // T17a 增量剪枝基线：目录 mtime 已在取得 writer mutex 之前预取，事务内只消费
    // 快照；SQLite writer mutex 不应包住冷盘/网络卷 metadata I/O。
    // ⚠️ 已知边界（见 §3.4/T17）：目录 mtime 仅反映**直接子项的增/删/改名**，不反映文件**就地编辑**
    // （同大小 EXIF/评分写回）与**孙级**变化——故仅供 opt-in「快速扫描」剪枝、默认全量扫描不据此跳。
    let dir_mtime = dir_mtime_cache.get(rel_path).copied().flatten();

    let id = upsert_directory(
        tx, root_id, parent_id, rel_path, &dir_name, depth, dir_mtime,
    )?;
    dir_cache.insert(rel_path.to_string(), id);
    Ok(id)
}

/// 读取目录 mtime。调用方必须在取得 SQLite writer mutex 之前调用。
fn read_dir_mtime(dir_abs: &Path) -> Option<i64> {
    std::fs::metadata(dir_abs)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// 为一个文件所在目录及其祖先预取 mtime；所有文件系统读取发生在 DB 写锁之外。
fn prefetch_dir_mtimes(
    root: &Path,
    rel_path: &str,
    dir_mtime_cache: &mut std::collections::HashMap<String, Option<i64>>,
) {
    let mut rel = rel_path.to_string();
    loop {
        if !dir_mtime_cache.contains_key(&rel) {
            let abs = if rel.is_empty() {
                root.to_path_buf()
            } else {
                root.join(&rel)
            };
            dir_mtime_cache.insert(rel.clone(), read_dir_mtime(&abs));
        }
        if rel.is_empty() {
            break;
        }
        rel = Path::new(&rel)
            .parent()
            .map(|p| normalize_db_path(&p.to_string_lossy()))
            .unwrap_or_default();
    }
}

/// T17b opt-in「快速扫描」剪枝判定：某目录的 FS mtime 与基线（T17a 写入的 `directories.mtime`，
/// 经由**扫描启动时一次性加载的快照** `dir_mtime_snapshot` 比对——即「上一轮扫描终态」）一致
/// → 其**直接子项无增/删/改名** → 跳过该目录所有直接文件的 per-file 工作（metadata stat /
/// cache_key / upsert / exotic 播种）。可剪枝时把该目录**全部未删媒体 id 回填进 `seen`**，确保
/// 缺失检测差集不把它们误判为已删除（🔴 数据安全：跳过 ≠ 消失）。返回 `true` 表示该目录可剪枝。
///
/// **逐目录、非递归**：仅跳本目录直接文件，**不** `skip_current_dir`——子目录仍由 walkdir 独立下降、
/// 各自比对 mtime，故嵌套目录的新增/删除**不漏**（目录 mtime 不向上冒泡，单祖先 mtime 不能代表子树）。
///
/// **J1 修复（方案 A 快照）**：基线只读 `dir_mtime_snapshot`（扫描启动时一次性 `SELECT` 全量加载，
/// 见 `load_directory_mtime_snapshot`），**不再查活 DB 行**。原实现直接查 `directories` 表活行，
/// 而本轮扫描的祖先链 `ensure_dir_chain` 会在遍历过程中持续覆写同一行的 `mtime`（每次经过都写当前
/// FS mtime）——若某祖先目录先被下降到的子目录「顺路」upsert 过，其 `mtime` 已被改写为当前值，
/// 此后该目录自身的直接文件再触发本判定时，查到的就是「已被本轮覆写」的新值而非「上一轮终态」，
/// 导致 `cur == stored` 恒成立、误判为「未变」而漏扫新增文件，且基线被错误「治愈」、后续轮次永续漏
/// （walkdir 无序遍历，子目录条目可能先于父目录的后续直接文件被访问，见 walker.rs 无 sort）。
/// 快照在写入发生前取好，不受扫描期覆写影响，从根本上消除该失效模式。
/// 快照未命中（新目录 / 无基线）= 不剪枝（与原「mtime 为 NULL」语义一致）。
///
/// **已知且唯一的漏检边界**：文件**就地编辑**（内容变、父目录 mtime 不变，如同大小 EXIF/评分写回）
/// 会被跳过——这是快速扫描的设计取舍，全量扫描兜底。
/// 测试用薄封装:保留旧「传文件路径」调用形态,生产 quick 路径已改走目录级
/// `decide_dir_pruned_at`(walker 在 stat 之前询问)。
#[cfg(test)]
fn decide_dir_pruned(
    tx: &rusqlite::Transaction,
    root_id: i64,
    rel_path_norm: &str,
    file_abs: &Path,
    dir_mtime_snapshot: &std::collections::HashMap<String, i64>,
    seen: &mut HashSet<i64>,
) -> Result<bool> {
    let dir_abs = file_abs.parent().unwrap_or(file_abs);
    let current_mtime = read_dir_mtime(dir_abs);
    decide_dir_pruned_at(
        tx,
        root_id,
        rel_path_norm,
        current_mtime,
        dir_mtime_snapshot,
        |id| {
            seen.insert(id);
        },
    )
}

/// 目录级剪枝判定(阶段3 walker 前置版):入参改为**目录绝对路径**,供 walker 在
/// per-file `metadata()` 之前询问。判定语义与旧 `decide_dir_pruned` 完全一致。
fn decide_dir_pruned_at(
    conn: &Connection,
    root_id: i64,
    rel_path_norm: &str,
    current_mtime: Option<i64>,
    dir_mtime_snapshot: &std::collections::HashMap<String, i64>,
    record_seen: impl FnMut(i64),
) -> Result<bool> {
    // 基线只读快照（扫描启动时的「上一轮终态」），不再查活 DB 行——见上方 J1 doc comment。
    let Some(&old_mtime) = dir_mtime_snapshot.get(rel_path_norm) else {
        return Ok(false); // 快照未命中（新目录/无基线）→ 保守不剪枝
    };
    // 目录 id 仍需查活行（回填 seen 要用），此处只取 id、不取 mtime，故不受覆写影响。
    let dir_id: i64 = match conn
        .prepare_cached("SELECT id FROM directories WHERE root_id=?1 AND rel_path=?2")?
        .query_row(params![root_id, rel_path_norm], |r| r.get(0))
        .optional()?
    {
        Some(id) => id,
        None => return Ok(false), // 快照命中但活行已消失（理论不应发生），保守不剪枝
    };

    // 当前目录 FS mtime 已由调用方在 writer mutex 外读取；读不到 → 保守处理。
    if current_mtime != Some(old_mtime) {
        return Ok(false); // mtime 变（相对快照基线）→ 直接子项结构变化 → 必须处理
    }

    // 未变 → 回填该目录全部未删媒体 id（含 companion：它们都在盘上、须计入 seen 防误删）。
    let mut record_seen = record_seen;
    let mut stmt =
        conn.prepare_cached("SELECT id FROM media_items WHERE directory_id=?1 AND is_deleted=0")?;
    let rows = stmt.query_map(params![dir_id], |r| r.get::<_, i64>(0))?;
    for id in rows {
        record_seen(id?);
    }
    Ok(true)
}

/// quick 模式 walker 剪枝器:缓存每目录判定,并在可剪枝时把该目录全部未删媒体 id
/// 回填本根 seen 表。`decisions` 用 `RefCell` 承载,因 walker 迭代器持有本对象的
/// 不可变引用。
struct QuickDirPruner<'a> {
    writer: &'a Mutex<Connection>,
    root_id: i64,
    seen: SeenWriter,
    snapshot: &'a std::collections::HashMap<String, i64>,
    decisions: std::cell::RefCell<std::collections::HashMap<String, bool>>,
}

impl QuickDirPruner<'_> {
    fn should_prune_dir(&self, abs_dir: &Path, rel_path_norm: &str) -> bool {
        if let Some(&cached) = self.decisions.borrow().get(rel_path_norm) {
            return cached;
        }
        // 判定需要 DB 只读 + 一次目录 stat;任何失败都保守按「不剪枝」处理,
        // 后续正常入库路径会重新贡献 seen,数据安全不回退。
        let current_mtime = read_dir_mtime(abs_dir);
        let decided = {
            let conn = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            match decide_dir_pruned_at(
                &conn,
                self.root_id,
                rel_path_norm,
                current_mtime,
                self.snapshot,
                |id| {
                    // seen 已流式写入本根连接级 TEMP 表,供 mark_missing_preloaded 直接差集。
                    let _ = self.seen.insert(&conn, id);
                },
            ) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "quick prune decision failed, fallback to full scan | quick 剪枝判定失败,回退全量处理: dir={rel_path_norm} err={e}"
                    );
                    false
                }
            }
        };
        self.decisions
            .borrow_mut()
            .insert(rel_path_norm.to_string(), decided);
        decided
    }
}

impl crate::scanner::walker::DirPruner for QuickDirPruner<'_> {
    fn should_prune_dir(&self, abs_dir: &Path, rel_path: &str) -> bool {
        self.should_prune_dir(abs_dir, rel_path)
    }
}

/// 缺失检测收尾·四道闸判定（抽出以脱离 Channel、可单测闸门排序）。返回标记数（0=被某闸拦下/无缺失）。
///
/// 闸序（任一不过即返回 0、**绝不删除**）：
///   1. 完整门闩 `walk_complete`——不完整扫描（遍历错误）→ seen 不可信，不差集。
///   2. TOCTOU `volume_online`——写删前复查卷在线（防扫描中途拔盘误删）。
///   3. 三重守门 `mark_missing`——在线卷集（本根卷）∩本根子树∩¬seen（守门内置）。
///
/// 入口守门（第四道闸·离线即不进 fast_scan）在调用方 `run_fast_scan` 入口处。
/// 测试/兼容入口:显式传内存 HashSet(旧签名)。生产已走流式本根 seen 表。
#[cfg(test)]
fn finalize_missing_detection(
    conn: &Connection,
    root_id: i64,
    walk_complete: bool,
    walk_error_count: usize,
    volume_online: bool,
    volume_id: Option<i64>,
    seen: &HashSet<i64>,
) -> Result<usize> {
    finalize_missing_detection_with(
        conn,
        root_id,
        walk_complete,
        walk_error_count,
        volume_online,
        volume_id,
        |conn, vols| mark_missing(conn, root_id, vols, seen, false),
    )
}

/// 生产收尾：消费本轮 seen 表、写回计数，并仅在遍历完整且最终卷在线时确认基线。
/// 调用方须在 `with_scan_db_write` 的根级 generation 闸门内执行，避免旧轮清掉新轮的未完成标记。
///
/// 缺失检测成功后，「计数 + 根状态 + 清 dirty」由 [`commit_scan_finalize`] 一次事务落地。
#[allow(clippy::too_many_arguments)]
fn finalize_scan_root(
    conn: &Connection,
    root_id: i64,
    generation: u64,
    walk_complete: bool,
    walk_error_count: usize,
    volume_online: bool,
    volume_id: Option<i64>,
    dir_media_counts: &std::collections::HashMap<i64, i64>,
    inserted: i64,
) -> Result<usize> {
    let marked = finalize_missing_detection_with(
        conn,
        root_id,
        walk_complete,
        walk_error_count,
        volume_online,
        volume_id,
        |conn, vols| mark_missing_preloaded_for_run(conn, root_id, generation, vols, false),
    )?;

    // 计数保存本轮已扫描结果（遍历错误/离线时也照写）；遍历错误或最终离线时目录 mtime 可能已
    // 提前更新，这些部分结果不能当可靠剪枝基线，故保留 dirty 让下一轮 quick 强制完整扫描。
    // 缺失检测返回 0 也可能是被完整性/在线闸门拦下，不能据返回成功就清标记。
    let baseline_complete = walk_complete && volume_online;
    commit_scan_finalize(conn, root_id, dir_media_counts, inserted, baseline_complete)?;
    Ok(marked)
}

/// 收尾落库：目录计数、根状态、未完成标记**必须在一次短事务里原子提交**。
///
/// 三者同属「本轮收尾」这一件事，分开提交只会在中间态暴露裂缝（状态已 idle 而基线仍 dirty，
/// 或反之）。目录数随重扫规模增长，逐条 UPDATE 各自 autocommit 会为每个目录付一次 WAL 提交
/// （调查 F-004）；合并后只付一次。
///
/// **不含 `mark_missing`**：缺失检测自带事务且末尾 DROP 本轮 TEMP 表，本函数只承接它成功之后的
/// 纯写入，不扩它的 scope。任一步失败 → 整体回滚，`scan.baseline_incomplete` 保持置位，
/// 下一轮扫描强制全量重建基线（失败方向恒为「未完成」，与 baseline_incomplete 的约定一致）。
fn commit_scan_finalize(
    conn: &Connection,
    root_id: i64,
    dir_media_counts: &std::collections::HashMap<i64, i64>,
    inserted: i64,
    baseline_complete: bool,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    set_directory_media_counts(&tx, dir_media_counts)?;
    finish_scan_root(&tx, root_id, inserted)?;
    if baseline_complete {
        clear_baseline_incomplete(&tx, root_id)?;
    }
    tx.commit()?;
    Ok(())
}

fn finalize_missing_detection_with(
    conn: &Connection,
    root_id: i64,
    walk_complete: bool,
    walk_error_count: usize,
    volume_online: bool,
    volume_id: Option<i64>,
    run: impl FnOnce(&Connection, &[i64]) -> Result<usize>,
) -> Result<usize> {
    if !walk_complete {
        warn!(
            "跳过缺失检测：扫描不完整（{walk_error_count} 处遍历错误）→ 不差集 | root_id={root_id}"
        );
        return Ok(0);
    }
    if !volume_online {
        // TOCTOU：扫描中途拔盘 → 写删前复查发现离线 → 放弃差集，防误删。
        warn!("跳过缺失检测：卷在写删前复查时已离线（疑中途拔盘）→ 不删除 | root_id={root_id}");
        return Ok(0);
    }
    // 守门1 在线卷集 = 本根的卷（在线）。volume_id=None（未识别）→ 空集 → 不标（宁可不删，§5 不变量 4）。
    let online_vols: Vec<i64> = volume_id.into_iter().collect();
    let marked = run(conn, &online_vols)?;
    if marked > 0 {
        info!("缺失检测：标记 {marked} 项 availability=missing（在线卷差集，未碰 is_deleted）| root_id={root_id}");
    }
    Ok(marked)
}

/// 运行期 TEMP 表的错误/取消路径兜底。表名由 root+generation 派生，Drop 不会清理其它轮次。
struct ScanTempTablesGuard<'a> {
    writer: &'a Mutex<Connection>,
    root_id: i64,
    generation: u64,
}

impl Drop for ScanTempTablesGuard<'_> {
    fn drop(&mut self) {
        let conn = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = cleanup_scan_temp_tables_for_run(&conn, self.root_id, self.generation) {
            warn!(
                "scan TEMP cleanup failed: root_id={} generation={} error={} | 扫描 TEMP 清理失败",
                self.root_id, self.generation, e
            );
        }
    }
}

/// 运行单个扫描根目录的快速扫描。
///
/// 流式管道（T12，§3.7.1）——**一次只持有一批**，内存峰值 O(batch)（不再全量 `Vec` + 排序 clone）：
/// - 遍历文件系统（`MediaWalker` 单线程流式，I/O 密集型），攒满 `BATCH_SIZE` 即处理一批。
/// - 每批前 `EAGER_DIM_COUNT`（跨批累计预算）项并行提真实尺寸（rayon），其余廉价占位。
/// - 一批一事务写 `media_items`；视图序让渡布局层（`query_layout_geometry` 独立 ORDER BY）。
/// - 每批经 `channel` 发进度（流式不预扫总数 → indeterminate）。
/// - 收尾跑缺失检测四道闸（`finalize_scan_root`）：完整门闩 + TOCTOU 复查 + 三重守门。
/// - 遵循 `cancel` 令牌——触发即返回 `Err(AppError::Cancelled)`（丢弃当前批、不进差集）。
///
/// `quick`（T17b，§3.4 opt-in）：对 FS mtime 未变的目录跳过其直接文件的 per-file 工作并回填 seen，
/// 提速增量重扫；唯一漏检边界是文件**就地编辑**，由全量扫描兜底。默认 false 即全量逐文件、行为不变。
// 扫描编排参数各自独立（S1 新增批提交回调后达 8 个）、无合理分组，沿用本仓库既有约定标注。
#[allow(clippy::too_many_arguments)]
pub fn run_fast_scan(
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    root_path: &str,
    catalog: &CatalogSnapshot,
    channel: &Channel<ScanChannelPayload>,
    cancel: &CancellationToken,
    // S1：每批事务提交后回调（生产接线 = AppState::bump_data_version，使 items 取数缓存
    // 逐批失效——扫描进行中前端按进度事件逐批重排，必须看到新入库的项）。
    on_batch_committed: &(dyn Fn() + Sync),
    // T17b：opt-in「快速扫描」。true 时对 FS mtime 未变的目录跳过其直接文件的 per-file 工作
    // （仍遍历整棵树、仍处理变更目录与所有子目录），代价是漏「就地编辑」（见 decide_dir_pruned）。
    // false（默认）→ 行为与 T17b 之前完全一致（全量逐文件）。
    // 上一轮未成功收尾时（见 baseline_incomplete）本次强制全量，忽略此处的 true。
    quick: bool,
) -> Result<u64> {
    // 兼容旧调用方；生产 IPC 使用带 generation 的入口，确保同根并发轮次的 TEMP 状态隔离。
    run_fast_scan_with_generation(
        writer,
        root_id,
        run_id,
        root_path,
        catalog,
        channel,
        cancel,
        on_batch_committed,
        quick,
        0,
    )
}

/// 带扫描代次的快速扫描入口。代次由 `start_scan` 的 per-root `RunTokenSlot` 生成，贯穿
/// seen/online TEMP 表命名与错误路径清理；不等待旧轮退出即可启动新轮。
#[allow(clippy::too_many_arguments)]
pub fn run_fast_scan_with_generation(
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    root_path: &str,
    catalog: &CatalogSnapshot,
    channel: &Channel<ScanChannelPayload>,
    cancel: &CancellationToken,
    on_batch_committed: &(dyn Fn() + Sync),
    quick: bool,
    generation: u64,
) -> Result<u64> {
    run_fast_scan_inner(
        writer,
        root_id,
        run_id,
        root_path,
        catalog,
        channel,
        cancel,
        on_batch_committed,
        quick,
        generation,
        None,
    )
}

/// 生产 IPC 使用的带状态扫描入口。所有会改变扫描 DB 状态的写入都通过 `AppState` 的
/// 根级 generation 闸门；旧轮不能在新轮安装后越过检查—写入间隙。
#[allow(clippy::too_many_arguments)]
pub fn run_fast_scan_with_generation_and_state(
    state: &AppState,
    root_id: i64,
    run_id: &str,
    root_path: &str,
    catalog: &CatalogSnapshot,
    channel: &Channel<ScanChannelPayload>,
    cancel: &CancellationToken,
    on_batch_committed: &(dyn Fn() + Sync),
    quick: bool,
    generation: u64,
) -> Result<u64> {
    run_fast_scan_inner(
        &state.db_writer,
        root_id,
        run_id,
        root_path,
        catalog,
        channel,
        cancel,
        on_batch_committed,
        quick,
        generation,
        Some(state),
    )
}

/// 在扫描写闸门内获取 writer 并执行短 DB 写闭包。
///
/// 生产路径的 generation 检查与 `db_writer` 获取处于同一根级闸门内；新轮安装也必须拿
/// 该闸门，因此不存在「检查通过后新轮安装、旧轮随后写入」的线性化漏洞。兼容入口没有
/// `AppState`，保留原有无代次调用语义。
fn with_scan_db_write<T>(
    writer: &Mutex<Connection>,
    state: Option<&AppState>,
    root_id: i64,
    generation: u64,
    write: impl FnOnce(&Connection) -> Result<T>,
) -> Result<Option<T>> {
    if let Some(state) = state {
        return state
            .with_scan_generation_write(root_id, generation, || {
                let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
                write(&conn)
            })
            .transpose();
    }

    let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
    write(&conn).map(Some)
}

/// 把扫描后的全局回调和 Channel 事件也绑定到根级 generation 闸门。
///
/// 永久 DB 写入受 [`with_scan_db_write`] 保护，但批次回调/进度/完成消息同样不能在新轮
/// 安装后由旧轮发布；兼容测试入口没有 `AppState` 时保留原有无代次语义。
fn with_scan_generation_action(
    state: Option<&AppState>,
    root_id: i64,
    generation: u64,
    action: impl FnOnce(),
) -> Result<()> {
    if let Some(state) = state {
        if state
            .with_scan_generation_write(root_id, generation, action)
            .is_none()
        {
            return Err(AppError::Cancelled);
        }
        return Ok(());
    }
    action();
    Ok(())
}

// ── 扫描基线完整性标记（F-001）───────────────────────────────────────────────

/// 「上次扫描未成功收尾」标记在 `app_config` 里的键前缀；后接 `root_id`，按根分片。
const BASELINE_INCOMPLETE_KEY_PREFIX: &str = "scan.baseline_incomplete";

fn baseline_incomplete_key(root_id: i64) -> String {
    format!("{BASELINE_INCOMPLETE_KEY_PREFIX}.{root_id}")
}

/// 读取本根的「未完成」标记。已置位表示上一轮扫描在成功收尾前中断（取消/崩溃/进程被杀），
/// 其已提交批次可能已把部分 `directories.mtime` 覆写为「治愈」值却漏掉同目录的就地新增文件。
///
/// **失败方向恒为「未完成」**：读取失败（库忙/损坏）恰恰发生在最需要这层保险的场景，绝不能
/// 让「读不到」成为放开 quick 剪枝的理由；判定细节见 `marker_is_incomplete`。
fn baseline_incomplete(conn: &Connection, root_id: i64) -> bool {
    let read = get_config(conn, &baseline_incomplete_key(root_id));
    if let Err(e) = &read {
        warn!("读取扫描基线标记失败，保守按未完成（本轮强制全量）| root_id={root_id} err={e}");
    }
    marker_is_incomplete(read)
}

/// 标记读取结果 → 是否「未完成」。**只有确证干净才算干净**：
/// - `Ok(None)`（key 从未置位：新根 / 历史库尚未扫过）或 `Ok(Some("0"))` → 干净；
/// - `Ok(Some(其它))("1" 或任何异常残留值)` → 未完成；
/// - `Err`（读失败）→ 未完成。
fn marker_is_incomplete(read: Result<Option<String>>) -> bool {
    match read {
        Ok(None) => false,
        Ok(Some(value)) => value != "0",
        Err(_) => true,
    }
}

/// 置位「未完成」（进入遍历前）。持久在 DB：进程被杀后重启仍可读到。
fn set_baseline_incomplete(conn: &Connection, root_id: i64) -> Result<()> {
    set_config(conn, &baseline_incomplete_key(root_id), "1")
}

/// 清零「未完成」（成功收尾后）。写 "0" 而非删键：语义与存在性解耦，便于排查。
fn clear_baseline_incomplete(conn: &Connection, root_id: i64) -> Result<()> {
    set_config(conn, &baseline_incomplete_key(root_id), "0")
}

#[allow(clippy::too_many_arguments)]
fn run_fast_scan_inner(
    writer: &Mutex<Connection>,
    root_id: i64,
    run_id: &str,
    root_path: &str,
    catalog: &CatalogSnapshot,
    channel: &Channel<ScanChannelPayload>,
    cancel: &CancellationToken,
    on_batch_committed: &(dyn Fn() + Sync),
    quick: bool,
    generation: u64,
    write_state: Option<&AppState>,
) -> Result<u64> {
    let started = std::time::Instant::now();
    info!("Fast scan started: root_id={root_id} generation={generation} path={root_path} | 快速扫描开始: root_id={root_id} generation={generation} 路径={root_path}");

    // 取消/DB 错误都必须回收自己的 TEMP 表，但不能碰新轮同根表。成功收尾时表已由
    // `mark_missing_preloaded_for_run` 回收，guard 的 DROP 是幂等兜底。
    let _temp_tables = ScanTempTablesGuard {
        writer,
        root_id,
        generation,
    };

    let root = Path::new(root_path);

    // ── 缺失检测·入口守门（§3.1.3，第一道闸）──────────────────────────────
    // 卷离线 / 根路径不可访问 → 跳过整次扫描、**绝不动 DB**（离线 ≠ 删除）。
    let prober = PathProber;
    if !prober.is_online(root) {
        warn!("scan_root 卷离线/不可访问，跳过扫描（不动 DB）: root_id={root_id} path={root_path}");
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        with_scan_generation_action(write_state, root_id, generation, || {
            let _ = channel.send(ScanChannelPayload::Completed(ScanCompletedPayload {
                root_id,
                run_id: run_id.to_string(),
                total_items: 0,
                total_bytes: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
                marked_missing: 0,
            }));
        })?;
        return Ok(0);
    }

    // 本 scan_root 的卷 id（V10 回填）：缺失检测守门1 的「在线卷集」。
    // None（未识别卷/孤儿根）→ 守门为空集 → 不标缺失（宁可不删，§5 不变量 4）。
    // 同一把锁内读出本根的卷 id 与 F-001 的「上一轮未成功收尾」标记（都只读、都在任何写入之前）。
    let (volume_id, baseline_dirty): (Option<i64>, bool) = {
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        let volume_id = conn
            .query_row(
                "SELECT volume_id FROM scan_roots WHERE id = ?1",
                params![root_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten();
        (volume_id, baseline_incomplete(&conn, root_id))
    };

    // ── 基线完整性降级（F-001）──────────────────────────────────────────────
    // quick 剪枝的基线（directories.mtime）在**遍历期**就随每批提交写回，而扫描可能在中途被
    // 取消/崩溃终止：此时某些目录的基线已被写成本轮值，其就地新增文件却还没入库。下一轮 quick
    // 据该「已被治愈」的基线判目录未变 → 剪掉该目录 → 那些文件跨轮漏扫，直到全量扫描才兜底。
    // 用持久标记把这条罕见路径封死：进入遍历前置「未完成」，仅成功收尾清零；因此任何中断
    // （含进程被杀后重启，标记在 DB 里）都会让下一轮即使请求 quick 也强制全量重建基线。
    // 只做 quick → 全量的降级，不反向把全量误升为 quick（quick 是全量能力的严格子集）。
    // 标记的失败方向恒为「未完成」（读取失败亦然，见 baseline_incomplete）——代价只是多跑一次
    // 全量，绝不会把未完成当完成而漏扫。
    let quick = if quick && baseline_dirty {
        warn!(
            "上一轮扫描未成功收尾，本轮强制全量重建基线 | root_id={root_id} generation={generation}"
        );
        false
    } else {
        quick
    };
    match with_scan_db_write(writer, write_state, root_id, generation, |conn| {
        set_baseline_incomplete(conn, root_id)
    })? {
        Some(()) => {}
        None => return Err(AppError::Cancelled),
    }

    // ── Step 1-3：流式遍历 + 分块提尺寸 + 分批入库（T12，§3.7.1）─────────────
    // 旧实现把整棵树收进 `Vec<WalkedFile>` 再 `order_for_view` clone 一遍（峰值 ~600MB@100 万）。
    // 改为**流式**：`MediaWalker` 逐项产出，攒满 BATCH_SIZE 即提尺寸+入库，**一次只持有一批**
    // → 内存峰值降至 O(batch)。代价（设计取舍，已纳入 §5 风险表）：
    //   ① **放弃入库前全局排序**——视图序完全交给布局层 `query_layout_geometry`（独立 ORDER BY
    //      sort_datetime/file_name 重排，正确性不受影响）；导入瞬时画廊非时间序，enrichment 补完
    //      sort_datetime 后首次 relayout 即正确。
    //   ② **eager-dim 对齐降级**——首批（前 EAGER_DIM_COUNT 项，按**遍历序**）做真实头读取，
    //      不再保证恰是「最先展示」的项（date 分组下两者不一致）；folder 分组下遍历序≈视图序，
    //      仍大体对齐。残留首屏占位由 enrichment 秒级回填，仅导入态可见。
    //   ③ **进度转 indeterminate**——不预扫总数，故扫描期 `total` 未知（发 0）；完成事件携带准确计数。
    // J1 修复（方案 A 快照）：quick 模式下，扫描启动时（本轮任何 upsert 改写之前）一次性加载
    // 全根 `rel_path → mtime` 快照 = 上一轮扫描终态。decide_dir_pruned_at 只读此快照做基线比对，
    // 不再受本轮扫描中祖先链 ensure_dir_chain 持续覆写活 DB 行影响（见该函数 doc comment）。
    // quick=false 时不查询，零开销。
    // 原「中断残留窗」（本轮已封，F-001）：中途取消/出错时已提交批次的 ensure_dir_chain 可能
    // 已把某祖先的活 mtime 覆写为当前值而其直接新文件尚未入库，下一轮 quick 的快照即含该
    // 「治愈」值而跨轮漏扫。现由上方持久「未完成」标记兜底：中断即留痕，下一轮强制全量重建
    // 基线（全量逐文件、不读基线），故该残留态不可能再被 quick 继承。
    let dir_mtime_snapshot: std::collections::HashMap<String, i64> = if quick {
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        load_directory_mtime_snapshot(&conn, root_id)?
    } else {
        std::collections::HashMap::new()
    };

    // 阶段4:seen 集改为连接级 TEMP 表流式累积(边扫边插),不再持有内存 HashSet、
    // 收尾也不再整体搬运。表按 root+generation 后缀隔离:同根 stop→restart 也可并发收尾,
    // 后启动者的清空/收尾只作用于自己的表,绝不误清旧轮或其它根(P0 修复);
    // 真实差集完成后由 mark_missing_preloaded_for_run DROP,免 temp_store=MEMORY 常驻。
    match with_scan_db_write(writer, write_state, root_id, generation, |conn| {
        init_seen_table_for_run(conn, root_id, generation)
    })? {
        Some(()) => {}
        None => return Err(AppError::Cancelled),
    }
    let seen_writer = SeenWriter::new_for_run(root_id, generation);
    // 阶段3:quick 剪枝器在 walker 进入目录时询问,命中即跳过该目录直接文件——发生在
    // WalkedFile stat **之前**,增量重扫不再为未变目录逐文件付 metadata 成本。
    let quick_pruner = quick.then(|| QuickDirPruner {
        writer,
        root_id,
        seen: SeenWriter::new_for_run(root_id, generation),
        snapshot: &dir_mtime_snapshot,
        decisions: RefCell::new(std::collections::HashMap::new()),
    });
    let mut walker = MediaWalker::new(root, catalog, cancel).with_dir_pruner(
        quick_pruner
            .as_ref()
            .map(|p| p as &dyn crate::scanner::walker::DirPruner),
    );

    // 我们需要一个目录缓存来避免对同一目录的重复更新插入 (upsert)
    let mut dir_cache: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    // 目录 mtime 预取缓存；所有 metadata 读取均在 writer mutex 外完成。
    let mut dir_mtime_cache: std::collections::HashMap<String, Option<i64>> =
        std::collections::HashMap::new();
    // T17a：每目录「直接媒体计数」累积器（dir_id → 本次扫描在该目录直接命中的媒体文件数）。
    // 跨批累积，收尾时随根状态/基线标记同事务写回 directories.media_count 作增量剪枝基线
    // （详见 set_directory_media_counts）。
    let mut dir_media_counts: std::collections::HashMap<i64, i64> =
        std::collections::HashMap::new();
    let mut inserted = 0u64;
    let mut batch_count = 0usize;
    let mut processed_bytes = 0u64;
    // eager-dim 预算（跨批累计）：仅最先入库的前 EAGER_DIM_COUNT 项做真实头读取，其余占位。
    let mut eager_remaining = EAGER_DIM_COUNT;
    // 复用的当前批缓冲（drain 后保留容量，避免每批重新分配）。
    let mut chunk: Vec<WalkedFile> = Vec::with_capacity(BATCH_SIZE);
    // 分段计时：本函数局部累加器，只为收尾一行 summary + 每批 debug 提供依据（无全局统计框架、
    // 无新增 IPC 字段、不打印任何路径、不逐文件记日志）。
    let mut timings = ScanPhaseTimings::default();

    loop {
        // 拉取至多 BATCH_SIZE 项（流式：一次只持有一批 → 内存 O(batch)）。
        let walk_started = std::time::Instant::now();
        chunk.clear();
        for f in walker.by_ref() {
            chunk.push(f);
            if chunk.len() >= BATCH_SIZE {
                break;
            }
        }
        let batch_walk_us = walk_started.elapsed().as_micros();
        timings.walk_us += batch_walk_us;
        // 取消：丢弃本批、立即返回（与原行为一致——取消即 Err，不进 finalize 差集）。
        if cancel.is_cancelled() {
            warn!("Fast scan cancelled at root_id={root_id} generation={generation}");
            return Err(AppError::Cancelled);
        }
        if chunk.is_empty() {
            break; // 遍历耗尽
        }

        // 分块提尺寸：本批前 `eager` 项并行做真实头读取（含 JPEG orientation / TIFF 超时），
        // 其余廉价占位（Phase-2 常量 / Phase-1 图像 0×0，由 enrichment 回填）。
        let eager = chunk.len().min(eager_remaining);
        let eager_started = std::time::Instant::now();
        let eager_dims: Vec<(i64, i64)> =
            chunk[..eager].par_iter().map(extract_dimensions).collect();
        eager_remaining -= eager;
        let batch_eager_us = eager_started.elapsed().as_micros();
        timings.eager_us += batch_eager_us;

        // 移动消费本批 WalkedFile → FileInfo（**不 clone**；drain 留空复用 chunk 容量）。
        let file_infos: Vec<FileInfo> = chunk
            .drain(..)
            .enumerate()
            .map(|(i, walked)| {
                let (width, height) = if i < eager {
                    eager_dims[i]
                } else {
                    cheap_phase2_dimensions(&walked).unwrap_or((0, 0))
                };
                FileInfo {
                    walked,
                    width,
                    height,
                }
            })
            .collect();

        // 本批一事务入库。
        // 可疑变更(mtime 变 size 同,§3.3.2)攒批:指纹计算是文件 IO,绝不进写事务——
        // 本批提交、写锁释放后,在事务外算指纹、逐项短写定案。
        let mut suspects: Vec<(i64, FastScanItem, std::path::PathBuf)> = Vec::new();
        for fi in &file_infos {
            let rel_path = dir_rel_path(root, &fi.walked.abs_path);
            let rel_path_norm = normalize_db_path(&rel_path);
            prefetch_dir_mtimes(root, &rel_path_norm, &mut dir_mtime_cache);
        }
        // 生产路径把 generation 检查和本批事务放进同一根级闸门；新轮安装后旧轮
        // 只能在闸门外被拒绝，不能再把本批快照写回活行。
        // 写库段包含等 writer 锁与 generation 闸门的时间：锁等待正是本段要区分的成本之一。
        let db_started = std::time::Instant::now();
        let batch_written = with_scan_db_write(writer, write_state, root_id, generation, |conn| {
            let tx = conn.unchecked_transaction()?;
            let root_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("");

            for fi in &file_infos {
                let rel_path = dir_rel_path(root, &fi.walked.abs_path);
                let rel_path_norm = normalize_db_path(&rel_path);

                // T17b quick 剪枝已前置到 walker(阶段3):走到这里的文件都是「目录未剪枝/新目录」,
                // 正常贡献 per-file 工作。剪枝目录的全部未删媒体 id 已由 QuickDirPruner 回填 seen。
                // 递归获取或创建目录记录及其父目录
                let dir_id = ensure_dir_chain(
                    &tx,
                    root_id,
                    &rel_path_norm,
                    &mut dir_cache,
                    root_name,
                    &dir_mtime_cache,
                )?;
                // T17a：每发现一个该目录的直接媒体文件 +1（口径＝walker 已分类媒体，与 T17b 剪枝时
                // 现算 read_dir 分类计数一致）。Inserted/Unchanged/SourceChanged 一律计入——它们都是
                // 「本次存在」的直接子项。
                *dir_media_counts.entry(dir_id).or_insert(0) += 1;

                let cache_key = compute_cache_key_with_mtime_ns(
                    &rel_path_norm,
                    &fi.walked.file_name,
                    fi.walked.file_mtime,
                    fi.walked.file_mtime_ns,
                );

                let fast_item = FastScanItem {
                    directory_id: dir_id,
                    file_name: fi.walked.file_name.clone(),
                    file_size: fi.walked.file_size,
                    file_mtime: fi.walked.file_mtime,
                    file_mtime_ns: fi.walked.file_mtime_ns,
                    file_format: fi.walked.extension.clone(),
                    media_type: fi.walked.media_type.as_str().to_string(),
                    width: fi.width,
                    height: fi.height,
                    sort_datetime: fi.walked.file_mtime, // will be refined in enrichment
                    // 将在丰富信息阶段细化
                    cache_key,
                };

                // 传本根卷 id：新项据此入库、历史 NULL 项顺带治愈 → 新数据可参与缺失检测守门1。
                let outcome = upsert_fast_scan_item(&tx, &fast_item, volume_id)?;
                // 🔴 必须在下方 exotic `continue` 之前收 seen：Unchanged 也要进 seen，
                // 否则未变更文件会被 mark_missing 误判为「本次未出现」而误删（Part2 §3.4/T5）。
                // 插入失败会让本批事务失败、扫描终止——绝不静默丢 seen(那是误标 missing 的方向)。
                seen_writer.insert(&tx, outcome.id())?;
                if matches!(outcome, UpsertOutcome::Inserted(_)) {
                    inserted += 1;
                }
                if let UpsertOutcome::SuspectChanged(id) = outcome {
                    // 可疑变更:留待本批事务外定案(exotic 动作也延后到定案分支)。
                    suspects.push((id, fast_item.clone(), fi.walked.abs_path.clone()));
                    continue;
                }

                // ── exotic 任务播种/失效（R13：扫描事务内完成，不等 enrichment）──────────
                // 只依赖扩展名查 Catalog；命中即为 exotic 格式（如 psd）。
                // builtin offering(D-OCR-5,如 exotic-ocr)非文件格式，扫描器面全跳过——不归类、
                // 不播种任务；哪怕磁盘上真出现杂散同名扩展文件也不会被误收编为该插件的处理任务。
                // 判据区分「合成标记 builtin」(如 exotic-ocr,非真实扩展名)与「真实扩展名叠加
                // builtin」(如 D-427 一期 RAW,cr2/nef 等真实文件格式但插件免安装):
                // !builtin 放行合成标记以外的常规 offering;classify_media_type 命中则说明
                // 该扩展名本身是真实媒体格式,即使 builtin 也要播种(否则 RAW 永不进队列)。
                if let Some(off) = catalog
                    .resolve_format(&fast_item.file_format)
                    .filter(|o| seed_gate_admits(o, &fast_item.file_format))
                {
                    let item_id = outcome.id();
                    match outcome {
                        UpsertOutcome::SourceChanged(_) => {
                            // 源文件变化：先把旧任务退回 pending、清旧产物/指纹/租约。
                            invalidate_exotic_tasks_for_item(&tx, item_id)?;
                        }
                        UpsertOutcome::Inserted(_) => {}
                        // Unchanged：任务已存在且源未变，无需动作。
                        UpsertOutcome::Unchanged(_) => continue,
                        // SuspectChanged 已在上方收集并 continue,不会到达此处。
                        UpsertOutcome::SuspectChanged(_) => unreachable!("suspect 已提前 continue"),
                    }
                    // 按 capabilities 播种（INSERT OR IGNORE，幂等）。SourceChanged 后补齐可能的新能力。
                    let caps: Vec<String> = off
                        .capabilities
                        .iter()
                        .map(|c| c.as_str().to_string())
                        .collect();
                    seed_exotic_tasks_for_item(&tx, item_id, &off.plugin_id, &caps)?;
                }
            }

            Ok(tx.commit()?)
        })?;
        if batch_written.is_none() {
            return Err(AppError::Cancelled);
        }
        let batch_db_us = db_started.elapsed().as_micros();
        timings.record_db(batch_db_us);
        with_scan_generation_action(write_state, root_id, generation, on_batch_committed)?;

        // ── 可疑变更定案(Part2 §3.3.2 三环)──────────────────────────────────────
        // 批事务已提交、写锁已释放:此刻才做指纹 IO(读文件),再逐项短写定案——
        // touch(滤 mtime 抖动,派生全保留)或 SourceChanged(同大小元数据编辑,全失效)。
        let suspect_started = std::time::Instant::now();
        timings.suspects += suspects.len();
        for (sid, s_item, abs_path) in suspects {
            let fp = match crate::utils::hash::content_fingerprint(&abs_path, s_item.file_size) {
                Ok(h) => Some(h),
                Err(e) => {
                    // 读失败(竞态删除/权限):无法证明内容未变 → 交 resolve 保守失效。
                    warn!(
                        "可疑变更指纹计算失败,保守判 SourceChanged | suspect fingerprint failed {:?}: {e}",
                        abs_path
                    );
                    None
                }
            };
            let outcome =
                match with_scan_db_write(writer, write_state, root_id, generation, |conn| {
                    resolve_suspect_change(conn, sid, &s_item, volume_id, fp.as_deref())
                })? {
                    Some(outcome) => outcome,
                    None => return Err(AppError::Cancelled),
                };
            timings.suspect_commits += 1;
            if matches!(outcome, UpsertOutcome::SourceChanged(_)) {
                // 与主循环 SourceChanged 分支同款 exotic 处理(失效旧任务 + 补种能力)。
                // 同样跳过 builtin offering(见上方主循环注释)。
                // 同上方主循环判据(合成标记 vs 真实扩展名叠加 builtin)。
                if let Some(off) = catalog
                    .resolve_format(&s_item.file_format)
                    .filter(|o| seed_gate_admits(o, &s_item.file_format))
                {
                    match with_scan_db_write(writer, write_state, root_id, generation, |conn| {
                        invalidate_exotic_tasks_for_item(conn, sid)?;
                        let caps: Vec<String> = off
                            .capabilities
                            .iter()
                            .map(|c| c.as_str().to_string())
                            .collect();
                        seed_exotic_tasks_for_item(conn, sid, &off.plugin_id, &caps)
                    })? {
                        Some(result) => result,
                        None => return Err(AppError::Cancelled),
                    }
                }
            }
        }
        let batch_suspect_us = suspect_started.elapsed().as_micros();
        timings.suspect_us += batch_suspect_us;

        let batch_bytes = file_infos
            .iter()
            .map(|fi| u64::try_from(fi.walked.file_size).unwrap_or(0))
            .sum::<u64>();
        processed_bytes += batch_bytes;
        let current_dir = file_infos
            .last()
            .and_then(|fi| fi.walked.abs_path.parent())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        batch_count += file_infos.len();
        timings.batches += 1;
        debug!(
            root_id,
            generation,
            run_id,
            batch = timings.batches as u64,
            batch_files = file_infos.len() as u64,
            files_so_far = batch_count as u64,
            walk_us = batch_walk_us as u64,
            eager_us = batch_eager_us as u64,
            db_us = batch_db_us as u64,
            suspect_us = batch_suspect_us as u64,
            "Fast scan batch committed"
        );

        // 进度：流式不预扫总数 → `total=0`（indeterminate），完成事件再携带准确计数。
        with_scan_generation_action(write_state, root_id, generation, || {
            let _ = channel.send(ScanChannelPayload::Progress(ScanProgressPayload {
                root_id,
                run_id: run_id.to_string(),
                scanned: batch_count as u64,
                total: 0,
                processed_bytes,
                total_bytes: None,
                current_dir,
                status: "scanning".to_string(),
            }));
        })?;
        // J2：与本文件其余 writer.lock() 调用处对齐——毒锁不静默跳过，取出内部数据继续
        // 写进度（毒锁通常仅意味着某次 panic 发生在持锁期间，数据本身未必损坏，跳过反而丢进度）。
        match with_scan_db_write(writer, write_state, root_id, generation, |conn| {
            update_scan_root_status(
                conn,
                root_id,
                "scanning",
                batch_count as i64,
                batch_count as i64,
            )
        })? {
            Some(result) => result,
            None => return Err(AppError::Cancelled),
        }
    }

    // 流式遍历收尾门闩（§3.2.2，第二道闸）：唯有 complete==true（零遍历/metadata 错误、未取消）
    // 才允许后续 mark_missing 差集——seen 不完整即绝不删除（不变量「不完整扫描 ≠ 删除」）。
    let outcome = walker.finish();
    let walk_complete = outcome.complete;
    let walk_error_count = outcome.errors.len();
    if cancel.is_cancelled() {
        // 取消可能发生在 walker 耗尽与收尾之间；同样禁止进入缺失差集。
        warn!("Fast scan cancelled before finalize: root_id={root_id} generation={generation}");
        return Err(AppError::Cancelled);
    }
    info!(
        "Walker streamed {} media file(s) generation={} | 扫描器流式发现 {} 个媒体文件",
        batch_count, generation, batch_count
    );

    // ── Step 4: Finalise + 缺失检测 ───────────────────────────────────────
    // ── 第 4 步：收尾 + 缺失检测（四道闸）────────────────────────────────────
    // TOCTOU 复查必须在 writer 锁外完成；结果随后由同一 generation 写闸门保护，
    // 不能让文件系统探测占住 SQLite writer mutex。
    let volume_online = prober.is_online(root);
    let finalize_started = std::time::Instant::now();
    let marked_missing: u64 =
        match with_scan_db_write(writer, write_state, root_id, generation, |conn| {
            let marked = finalize_scan_root(
                conn,
                root_id,
                generation,
                walk_complete,
                walk_error_count,
                volume_online,
                volume_id,
                &dir_media_counts,
                inserted as i64,
            )?;
            Ok(marked as u64)
        })? {
            Some(result) => result,
            None => return Err(AppError::Cancelled),
        };
    timings.finalize_us = finalize_started.elapsed().as_micros();

    let elapsed_ms = started.elapsed().as_millis() as u64;
    info!("Fast scan done: root_id={root_id} inserted={inserted} elapsed={elapsed_ms}ms | 快速扫描完成: root_id={root_id} 插入={inserted} 耗时={elapsed_ms}ms");
    timings.log_summary(root_id, generation, run_id, inserted, elapsed_ms);

    if cancel.is_cancelled() {
        // 停止/重启可能恰好发生在最终写事务之后；不要再给旧轮发布完成消息。
        return Err(AppError::Cancelled);
    }
    with_scan_generation_action(write_state, root_id, generation, || {
        let _ = channel.send(ScanChannelPayload::Completed(ScanCompletedPayload {
            root_id,
            run_id: run_id.to_string(),
            total_items: inserted,
            total_bytes: processed_bytes,
            elapsed_ms,
            marked_missing,
        }));
    })?;

    Ok(inserted)
}

/// exotic 任务化播种守卫(单一真源:主循环 + suspect 定案分支 + 回归测试共用)。
///
/// - 判据一:合成标记 builtin(如 "ocr",`classify_media_type` 恒 None)挡出播种路径;真实
///   扩展名(含叠加 builtin 的 RAW cr2)放行。
/// - 判据二(D-444③ A4 裁决):`media_kind=video` 的 builtin offering(video-extended)**不播**
///   exotic 缩略图任务——rmvb/vob 缩略图走常规 video 派生链(video_cover/video_keyframes →
///   `backend_for` → V5 `WorkerVideoBackend` ffmpeg 桥),单轨;若同时播 exotic Thumbnail 任务,
///   会与派生链双写同一 `media_items.thumb_path`(双轨),故在此掐断 exotic 一侧。
///   coordinator 侧 video-extended descriptor / `VideoThumbnailFactory` 保留作 offering
///   一致性面的可达实现(offering 声明 thumbnail → 路径可达可测),实际缩略图不经它。
fn seed_gate_admits(off: &crate::exotic::catalog::CatalogOffering, fmt: &str) -> bool {
    (!off.builtin || classify_media_type(fmt).is_some())
        && !(off.builtin && off.media_kind == crate::exotic::catalog::MediaKind::Video)
}
