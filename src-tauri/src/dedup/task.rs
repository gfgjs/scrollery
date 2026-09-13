//! 可停止、可续跑的精确去重分析任务。
//!
//! 这个文件只依赖一个很窄的存储适配 trait。真实应用通过
//! [`AppStateDedupStore`] 把 `db::queries` 接入；单元测试仍可注入内存 fake，
//! 从而把任务生命周期和文件摘要工作与数据库实现分开验证。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::db::queries::{
    self, DedupCandidateCursor, DedupIndexUpdate, STATUS_ERROR, STATUS_QUICK, STATUS_READY,
};
use crate::dedup::{
    exact_digest_with_snapshot_cancelled, live_photo_unit_digest_with_components_cancelled,
    physical_key, quick_digest_with_snapshot_cancelled, single_unit_digest, ExactDigest,
    QuickDigest, UnitDigest,
};
use crate::error::AppError;
use crate::state::{AppState, RunTokenSlot};

/// 去重任务的稳定 app 事件名。
pub const DEDUP_PROGRESS_EVENT: &str = "dedup:progress";
/// `start(reset)` 在已有轮次运行时返回的稳定错误码。
pub const DEDUP_BUSY_CODE: &str = "DEDUP_BUSY";
const DEDUP_START_FAILED_CODE: &str = "DEDUP_START_FAILED";
const DEDUP_STORE_CODE: &str = "DEDUP_STORE";
const DEFAULT_BATCH_SIZE: usize = 64;
const PRIORITY_POLL_MS: u64 = 40;

/// 前端可消费的任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DedupStatus {
    Idle,
    Running,
    Stopped,
    Completed,
    Failed,
}

/// 三阶段分析状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DedupPhase {
    Idle,
    Quick,
    Exact,
    Unit,
}

/// 可序列化的错误计数。只保存稳定 code，不保存路径或底层错误串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupProgressError {
    pub code: String,
    pub count: u64,
}

/// 任务最近一次进度快照。
///
/// 快照存在 manager 内存中，事件只是广播通道；webview 重载后应调用
/// `status()` 恢复，而不是依赖事件是否恰好被监听到。`bytes_*` 表示已处理的逻辑
/// 文件字节（quick 阶段按候选文件大小计入，exact 阶段 Live Photo 包含全部组件），
/// 不是实际从磁盘读取的采样字节数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupProgress {
    pub run_id: u64,
    pub status: DedupStatus,
    pub phase: DedupPhase,
    pub items_done: u64,
    pub items_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub groups_found: u64,
    pub potential_logical_bytes: u64,
    pub errors: Vec<DedupProgressError>,
    pub waiting_on: Vec<String>,
}

impl Default for DedupProgress {
    fn default() -> Self {
        Self {
            run_id: 0,
            status: DedupStatus::Idle,
            phase: DedupPhase::Idle,
            items_done: 0,
            items_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            groups_found: 0,
            potential_logical_bytes: 0,
            errors: Vec::new(),
            waiting_on: Vec::new(),
        }
    }
}

/// 一个待分析的扫描项。`source_revision` 必须由查询适配器从当前 media row
/// 带出，写回时由适配器负责用该 revision 做条件保护。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupScanCandidate {
    pub item_id: i64,
    pub path: PathBuf,
    pub source_revision: i64,
    pub size: u64,
    /// 查询时从 media_items 取得的纳秒 mtime；缺失时不能安全发布摘要。
    pub file_mtime_ns: Option<i64>,
    pub companions: Vec<DedupCompanionCandidate>,
}

/// 一个待分析的 Live Photo companion。保留数据库身份和源代次，确保组合摘要与每个
/// companion 的 exact sidecar 都只能写回到仍是同一版本的行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupCompanionCandidate {
    pub item_id: i64,
    pub path: PathBuf,
    pub source_revision: i64,
    pub size: u64,
    /// 查询时从 companion media row 取得的纳秒 mtime。
    pub file_mtime_ns: Option<i64>,
}

/// `write_dedup_quick_for_generation` 的批量行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupQuickRow {
    pub item_id: i64,
    pub source_revision: i64,
    pub size: u64,
    /// hash 完成后核验过、且必须与候选 DB 快照相同的纳秒 mtime。
    pub file_mtime_ns: Option<i64>,
    pub physical_key: Option<Vec<u8>>,
    pub digest: QuickDigest,
}

/// `write_dedup_exact_for_generation` 的批量行。unit digest 在 exact 阶段一并计算并持久化，
/// unit 阶段只做 O(n) 分组统计，避免把文件 IO 重复放进 DB writer 临界区。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupExactRow {
    pub item_id: i64,
    pub source_revision: i64,
    pub size: u64,
    /// hash 完成后核验过、且必须与候选 DB 快照相同的纳秒 mtime。
    pub file_mtime_ns: Option<i64>,
    pub exact_digest: ExactDigest,
    pub unit_digest: Option<UnitDigest>,
    pub unit_size: Option<u64>,
    pub physical_key: Option<Vec<u8>>,
    /// 精确摘要写入前再次核对文件路径；仅用于文件 IO，不进入数据库。
    pub path: PathBuf,
    /// Live Photo 主项及其 companion 共用的逻辑单元标识，任一组件漂移都丢弃整组。
    pub logical_unit_item_id: i64,
}

/// 文件 hash 自己的最终快照还必须与查询候选时的 DB 快照一致。
///
/// `source_revision` 由扫描器异步推进；在扫描器尚未看到替换的窗口内，单独依赖
/// revision 会把新文件摘要写入旧代次。未知 mtime 也不能降级为“只比 size”，否则
/// 同大小改写会绕过这道守门。
fn snapshot_matches_candidate(
    expected_size: u64,
    expected_mtime_ns: Option<i64>,
    actual_size: u64,
    actual_mtime_ns: i128,
) -> bool {
    expected_size == actual_size && expected_mtime_ns == i64::try_from(actual_mtime_ns).ok()
}

fn db_file_size(size: u64) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

/// 去重查询的最小适配面。
///
/// 方法名与未来 `db::queries` 契约一一对应；真实适配器可以在方法内部取得
/// `DbPool`/`DbWriter` 连接并调用查询函数。所有 hash 都在 `write_*` 之前完成，
/// 因而不会在 DB writer 锁内读文件。
pub trait DedupStore: Send + Sync {
    fn list_dedup_scan_candidates(
        &self,
        cursor: Option<&DedupCandidateCursor>,
        limit: usize,
    ) -> Result<Vec<DedupScanCandidate>, AppError>;

    fn count_dedup_scan_candidates(&self) -> Result<u64, AppError>;

    /// 安装本轮数据库代次，并按需清理仍属于该代次的 sidecar。
    fn begin_run(&self, generation: u64, reset: bool) -> Result<(), AppError>;

    /// 让 stop 在数据库侧撤销旧轮写入权。纯内存 fake 不需要额外动作；真实适配器
    /// 必须把 generation 单调推进，且在 stop 返回前完成短事务。
    fn invalidate_run(&self, _generation: u64) -> Result<(), AppError> {
        Ok(())
    }

    /// 原子发布本轮完整结果。仅成功路径调用；停止/失败不发布。
    fn publish_run(&self, _generation: u64) -> Result<(), AppError> {
        Ok(())
    }

    fn write_dedup_quick_for_generation(
        &self,
        generation: u64,
        rows: &[DedupQuickRow],
    ) -> Result<(), AppError>;

    fn count_dedup_quick_collisions(&self) -> Result<u64, AppError> {
        Ok(0)
    }

    /// 按候选 keyset 返回 quick 碰撞项；不会把所有碰撞成员一次装入内存。
    fn list_dedup_quick_collisions(
        &self,
        cursor: Option<&DedupCandidateCursor>,
        limit: usize,
    ) -> Result<Vec<DedupScanCandidate>, AppError>;

    fn write_dedup_exact_for_generation(
        &self,
        generation: u64,
        rows: &[DedupExactRow],
    ) -> Result<(), AppError>;

    fn write_dedup_error_for_generation(
        &self,
        generation: u64,
        item_id: i64,
        source_revision: i64,
        size: u64,
        file_mtime_ns: Option<i64>,
        code: &'static str,
    ) -> Result<(), AppError>;

    fn summarize_groups(&self) -> Result<(u64, u64), AppError> {
        Ok((0, 0))
    }
}

/// 事件输出适配器。默认实现丢弃事件，Tauri 调用方可使用
/// [`TauriDedupEventSink`] 发出 `dedup:progress`。
pub trait DedupEventSink: Send + Sync {
    fn emit(&self, progress: &DedupProgress);
}

impl<F> DedupEventSink for F
where
    F: Fn(&DedupProgress) + Send + Sync,
{
    fn emit(&self, progress: &DedupProgress) {
        self(progress);
    }
}

/// Tauri app 事件适配器。
pub struct TauriDedupEventSink {
    app: AppHandle,
}

impl TauriDedupEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl DedupEventSink for TauriDedupEventSink {
    fn emit(&self, progress: &DedupProgress) {
        // 事件与 IPC 返回值必须共享公开 wire DTO：内部 run_id 是 u64、状态枚举含
        // `stopped`，直接广播会让前端在重启/停止时收到另一套形状。转换在发送边界完成，
        // 任务层仍可保留紧凑的内部快照。
        let payload = crate::ipc::dedup_commands::progress_to_snapshot(progress.clone());
        let _ = self.app.emit(DEDUP_PROGRESS_EVENT, payload);
    }
}

/// 低优先级让步探针。返回非空列表时，任务在下一个 hash/DB 批次前等待。
pub trait DedupPriorityProbe: Send + Sync {
    fn waiting_on(&self) -> Vec<String>;
}

impl<F> DedupPriorityProbe for F
where
    F: Fn() -> Vec<String> + Send + Sync,
{
    fn waiting_on(&self) -> Vec<String> {
        self()
    }
}

/// 可由扫描、缩略图、派生和交互入口设置的最小让步状态。
///
/// 生产接线可把现有流水线状态映射到这里；它独立于任务控制锁，因此不会
/// 跨 await 或在 hash 时持有 DB writer guard。
pub struct DedupPriorityGate {
    scanning: AtomicBool,
    thumbnails: AtomicBool,
    derivation: AtomicBool,
    interactive: AtomicBool,
}

impl DedupPriorityGate {
    pub fn new() -> Self {
        Self {
            scanning: AtomicBool::new(false),
            thumbnails: AtomicBool::new(false),
            derivation: AtomicBool::new(false),
            interactive: AtomicBool::new(false),
        }
    }

    pub fn set_scanning(&self, active: bool) {
        self.scanning.store(active, Ordering::Release);
    }

    pub fn set_thumbnails(&self, active: bool) {
        self.thumbnails.store(active, Ordering::Release);
    }

    pub fn set_derivation(&self, active: bool) {
        self.derivation.store(active, Ordering::Release);
    }

    pub fn set_interactive(&self, active: bool) {
        self.interactive.store(active, Ordering::Release);
    }
}

impl Default for DedupPriorityGate {
    fn default() -> Self {
        Self::new()
    }
}

impl DedupPriorityProbe for DedupPriorityGate {
    fn waiting_on(&self) -> Vec<String> {
        let mut waiting = Vec::with_capacity(4);
        if self.scanning.load(Ordering::Acquire) {
            waiting.push("scanning".to_string());
        }
        if self.thumbnails.load(Ordering::Acquire) {
            waiting.push("thumbnails".to_string());
        }
        if self.derivation.load(Ordering::Acquire) {
            waiting.push("derivation".to_string());
        }
        if self.interactive.load(Ordering::Acquire) {
            waiting.push("interactive".to_string());
        }
        waiting
    }
}

struct EmptyDedupStore;

impl DedupStore for EmptyDedupStore {
    fn list_dedup_scan_candidates(
        &self,
        _cursor: Option<&DedupCandidateCursor>,
        _limit: usize,
    ) -> Result<Vec<DedupScanCandidate>, AppError> {
        Ok(Vec::new())
    }

    fn count_dedup_scan_candidates(&self) -> Result<u64, AppError> {
        Ok(0)
    }

    fn begin_run(&self, _generation: u64, _reset: bool) -> Result<(), AppError> {
        Ok(())
    }

    fn write_dedup_quick_for_generation(
        &self,
        _generation: u64,
        _rows: &[DedupQuickRow],
    ) -> Result<(), AppError> {
        Ok(())
    }

    fn list_dedup_quick_collisions(
        &self,
        _cursor: Option<&DedupCandidateCursor>,
        _limit: usize,
    ) -> Result<Vec<DedupScanCandidate>, AppError> {
        Ok(Vec::new())
    }

    fn write_dedup_exact_for_generation(
        &self,
        _generation: u64,
        _rows: &[DedupExactRow],
    ) -> Result<(), AppError> {
        Ok(())
    }

    fn write_dedup_error_for_generation(
        &self,
        _generation: u64,
        _item_id: i64,
        _source_revision: i64,
        _size: u64,
        _file_mtime_ns: Option<i64>,
        _code: &'static str,
    ) -> Result<(), AppError> {
        Ok(())
    }
}

/// 生产查询适配器。它只持有 `Arc<AppState>`，由任务线程在启动时创建；因此
/// `AppState` 不反向拥有自己，且摘要/文件 IO 永远发生在 writer 锁之外。
pub struct AppStateDedupStore {
    state: Arc<AppState>,
}

impl AppStateDedupStore {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// 去重 worker 的每个短 DB 操作都要进入读区；清库持有写锁时，旧 worker 不能
    /// 在清库事务前后穿透并写入已被删除的 sidecar。调用方不得把文件 IO 放进此闭包。
    fn with_lifecycle<T, F>(&self, action: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.state.with_dedup_lifecycle_read(action)
    }

    fn map_candidate(
        &self,
        conn: &rusqlite::Connection,
        candidate: queries::DedupScanCandidate,
    ) -> Result<DedupScanCandidate, AppError> {
        let companions = queries::list_dedup_companions(conn, candidate.item_id)?;
        let companion_candidates = companions
            .iter()
            .map(|companion| DedupCompanionCandidate {
                item_id: companion.item_id,
                path: PathBuf::from(&companion.path),
                source_revision: companion.source_revision,
                size: u64::try_from(companion.file_size).unwrap_or(0),
                file_mtime_ns: companion.file_mtime_ns,
            })
            .collect::<Vec<_>>();
        Ok(DedupScanCandidate {
            item_id: candidate.item_id,
            path: PathBuf::from(candidate.path),
            source_revision: candidate.source_revision,
            size: u64::try_from(candidate.file_size).unwrap_or(0),
            file_mtime_ns: candidate.file_mtime_ns,
            companions: companion_candidates,
        })
    }
}

impl DedupStore for AppStateDedupStore {
    fn list_dedup_scan_candidates(
        &self,
        cursor: Option<&DedupCandidateCursor>,
        limit: usize,
    ) -> Result<Vec<DedupScanCandidate>, AppError> {
        self.with_lifecycle(|| {
            let conn = self.state.db_read_pool.get().map_err(AppError::from)?;
            queries::list_dedup_scan_candidates(&conn, cursor, limit)?
                .into_iter()
                .map(|candidate| self.map_candidate(&conn, candidate))
                .collect()
        })
    }

    fn count_dedup_scan_candidates(&self) -> Result<u64, AppError> {
        self.with_lifecycle(|| {
            let conn = self.state.db_read_pool.get().map_err(AppError::from)?;
            queries::count_dedup_scan_candidates(&conn)
        })
    }

    fn begin_run(&self, generation: u64, reset: bool) -> Result<(), AppError> {
        self.with_lifecycle(|| {
            let conn = self
                .state
                .db_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !queries::begin_dedup_run(&conn, generation, reset)? {
                return Err(AppError::Dedup {
                    code: "DEDUP_STALE_RUN",
                    message: "去重分析轮次已过期 | dedup analysis generation is stale".to_string(),
                });
            }
            Ok(())
        })
    }

    fn invalidate_run(&self, generation: u64) -> Result<(), AppError> {
        let conn = self
            .state
            .db_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queries::invalidate_dedup_run(&conn, generation)?;
        Ok(())
    }

    fn write_dedup_quick_for_generation(
        &self,
        generation: u64,
        rows: &[DedupQuickRow],
    ) -> Result<(), AppError> {
        self.with_lifecycle(|| {
            let conn = self
                .state
                .db_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let tx = conn.unchecked_transaction()?;
            let checks = rows
                .iter()
                .map(|row| queries::DedupWriteCheck {
                    item_id: row.item_id,
                    source_revision: row.source_revision,
                    file_size: db_file_size(row.size),
                    file_mtime_ns: row.file_mtime_ns,
                    companion_of: None,
                    physical_key: row.physical_key.clone(),
                })
                .collect::<Vec<_>>();
            if !queries::dedup_write_batch_current(&tx, generation, &checks)? {
                tx.commit()?;
                return Ok(());
            }
            for row in rows {
                queries::write_dedup_index_if_generation_current(
                    &tx,
                    &DedupIndexUpdate {
                        item_id: row.item_id,
                        source_revision: row.source_revision,
                        file_size: db_file_size(row.size),
                        file_mtime_ns: row.file_mtime_ns,
                        hash_version: crate::dedup::DEDUP_HASH_VERSION as i64,
                        quick_digest: Some(row.digest.as_ref()),
                        exact_digest: None,
                        unit_digest: None,
                        unit_size: None,
                        physical_key: row.physical_key.as_deref(),
                        status: STATUS_QUICK,
                        error_code: None,
                    },
                    generation,
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    fn count_dedup_quick_collisions(&self) -> Result<u64, AppError> {
        self.with_lifecycle(|| {
            let conn = self.state.db_read_pool.get().map_err(AppError::from)?;
            queries::count_dedup_quick_collisions(&conn)
        })
    }

    fn list_dedup_quick_collisions(
        &self,
        cursor: Option<&DedupCandidateCursor>,
        limit: usize,
    ) -> Result<Vec<DedupScanCandidate>, AppError> {
        self.with_lifecycle(|| {
            let conn = self.state.db_read_pool.get().map_err(AppError::from)?;
            queries::list_dedup_quick_collisions(&conn, cursor, limit)?
                .into_iter()
                .map(|candidate| self.map_candidate(&conn, candidate))
                .collect()
        })
    }

    fn write_dedup_exact_for_generation(
        &self,
        generation: u64,
        rows: &[DedupExactRow],
    ) -> Result<(), AppError> {
        self.with_lifecycle(|| {
            let conn = self
                .state
                .db_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let tx = conn.unchecked_transaction()?;
            let mut main_item_id = None;
            let checks = rows
                .iter()
                .map(|row| {
                    let companion_of = main_item_id;
                    if row.unit_digest.is_some() {
                        main_item_id = Some(row.item_id);
                    }
                    queries::DedupWriteCheck {
                        item_id: row.item_id,
                        source_revision: row.source_revision,
                        file_size: db_file_size(row.size),
                        file_mtime_ns: row.file_mtime_ns,
                        companion_of: if row.unit_digest.is_some() {
                            None
                        } else {
                            companion_of
                        },
                        physical_key: row.physical_key.clone(),
                    }
                })
                .collect::<Vec<_>>();
            if !queries::dedup_write_batch_current(&tx, generation, &checks)? {
                tx.commit()?;
                return Ok(());
            }
            for row in rows {
                queries::write_dedup_index_if_generation_current(
                    &tx,
                    &DedupIndexUpdate {
                        item_id: row.item_id,
                        source_revision: row.source_revision,
                        file_size: db_file_size(row.size),
                        file_mtime_ns: row.file_mtime_ns,
                        hash_version: crate::dedup::DEDUP_HASH_VERSION as i64,
                        quick_digest: None,
                        exact_digest: Some(row.exact_digest.as_ref()),
                        unit_digest: row.unit_digest.as_ref().map(|digest| digest.as_ref()),
                        unit_size: row
                            .unit_size
                            .map(|size| i64::try_from(size).unwrap_or(i64::MAX)),
                        physical_key: row.physical_key.as_deref(),
                        status: STATUS_READY,
                        error_code: None,
                    },
                    generation,
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    fn write_dedup_error_for_generation(
        &self,
        generation: u64,
        item_id: i64,
        source_revision: i64,
        size: u64,
        file_mtime_ns: Option<i64>,
        code: &'static str,
    ) -> Result<(), AppError> {
        self.with_lifecycle(|| {
            let conn = self
                .state
                .db_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queries::write_dedup_index_if_generation_current(
                &conn,
                &DedupIndexUpdate {
                    item_id,
                    source_revision,
                    file_size: db_file_size(size),
                    file_mtime_ns,
                    hash_version: crate::dedup::DEDUP_HASH_VERSION as i64,
                    quick_digest: None,
                    exact_digest: None,
                    unit_digest: None,
                    unit_size: None,
                    physical_key: None,
                    status: STATUS_ERROR,
                    error_code: Some(code),
                },
                generation,
            )?;
            Ok(())
        })
    }

    fn summarize_groups(&self) -> Result<(u64, u64), AppError> {
        self.with_lifecycle(|| {
            let conn = self.state.db_read_pool.get().map_err(AppError::from)?;
            queries::summarize_duplicate_groups(&conn)
        })
    }

    fn publish_run(&self, generation: u64) -> Result<(), AppError> {
        let conn = self
            .state
            .db_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !queries::publish_dedup_run(&conn, generation)? {
            return Err(AppError::Dedup {
                code: "DEDUP_STALE_RUN",
                message: "去重分析轮次已过期 | dedup analysis generation is stale".to_string(),
            });
        }
        self.state.bump_dedup_view_epoch();
        Ok(())
    }
}

struct NoopDedupEventSink;

impl DedupEventSink for NoopDedupEventSink {
    fn emit(&self, _progress: &DedupProgress) {}
}

struct ActiveRun {
    run_id: u64,
    generation: u64,
    token: CancellationToken,
    store: Arc<dyn DedupStore>,
}

struct ManagerState {
    active: Option<ActiveRun>,
    cursor: Option<DedupCandidateCursor>,
    exact_cursor: Option<DedupCandidateCursor>,
    snapshot: DedupProgress,
}

struct ManagerInner {
    control: Mutex<ManagerState>,
    token_slot: RunTokenSlot,
    store: Arc<dyn DedupStore>,
    events: Mutex<Arc<dyn DedupEventSink>>,
    priority: Mutex<Arc<dyn DedupPriorityProbe>>,
    batch_size: usize,
    next_run_id: AtomicU64,
}

/// 精确去重分析管理器。
///
/// `new/start/stop/status` 是稳定的生命周期 API。查询和事件接线通过
/// `with_store`/`with_parts` 注入，便于在查询波次落地后不改任务状态机。
#[derive(Clone)]
pub struct DedupTaskManager {
    inner: Arc<ManagerInner>,
}

impl DedupTaskManager {
    /// 创建可直接使用的 manager。空存储让尚未接入查询的构建仍可验证生命周期。
    pub fn new() -> Self {
        Self::with_parts(
            Arc::new(EmptyDedupStore),
            Arc::new(NoopDedupEventSink),
            Arc::new(DedupPriorityGate::new()),
            DEFAULT_BATCH_SIZE,
        )
    }

    /// 使用未来查询波次提供的存储适配器。
    pub fn with_store(store: Arc<dyn DedupStore>) -> Self {
        Self::with_parts(
            store,
            Arc::new(NoopDedupEventSink),
            Arc::new(DedupPriorityGate::new()),
            DEFAULT_BATCH_SIZE,
        )
    }

    /// 使用 Tauri 事件和查询适配器的构造便捷函数。
    pub fn with_store_and_app(store: Arc<dyn DedupStore>, app: AppHandle) -> Self {
        Self::with_parts(
            store,
            Arc::new(TauriDedupEventSink::new(app)),
            Arc::new(DedupPriorityGate::new()),
            DEFAULT_BATCH_SIZE,
        )
    }

    /// 供查询/调度接线和单测使用的完整注入点；批大小至少为 1。
    pub fn with_parts(
        store: Arc<dyn DedupStore>,
        events: Arc<dyn DedupEventSink>,
        priority: Arc<dyn DedupPriorityProbe>,
        batch_size: usize,
    ) -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                control: Mutex::new(ManagerState {
                    active: None,
                    cursor: None,
                    exact_cursor: None,
                    snapshot: DedupProgress::default(),
                }),
                token_slot: RunTokenSlot::new(),
                store,
                events: Mutex::new(events),
                priority: Mutex::new(priority),
                batch_size: batch_size.max(1),
                next_run_id: AtomicU64::new(1),
            }),
        }
    }

    /// 启动一轮分析。`reset=true` 先清理去重 sidecar；清理失败在快照中进入 failed。
    /// 已有轮次运行时不会取消旧轮，而是稳定返回 `DEDUP_BUSY`。
    pub fn start(&self, reset: bool) -> Result<DedupProgress, AppError> {
        self.start_with_store(reset, self.inner.store.clone())
    }

    /// 使用真实 `AppState` 启动一轮分析。存储适配器只在后台线程中持有状态，
    /// 不会形成 `AppState -> manager -> AppState` 的循环引用。
    pub fn start_with_state(
        &self,
        state: Arc<AppState>,
        reset: bool,
    ) -> Result<DedupProgress, AppError> {
        state.with_dedup_lifecycle_read(|| {
            self.start_with_store(reset, Arc::new(AppStateDedupStore::new(state.clone())))
        })
    }

    /// 生产启动入口：把当前 Tauri app 绑定到本轮事件 sink，再启动真实 DB 适配器。
    pub fn start_with_state_and_app(
        &self,
        state: Arc<AppState>,
        app: AppHandle,
        reset: bool,
    ) -> Result<DedupProgress, AppError> {
        self.set_event_sink(Arc::new(TauriDedupEventSink::new(app)));
        let priority_state = state.clone();
        self.set_priority_probe(Arc::new(move || {
            let mut waiting = Vec::new();
            if priority_state.is_scan_or_thumb_running() {
                waiting.push("scan_or_thumbnail".to_string());
            }
            if priority_state.is_derivation_running() {
                waiting.push("derivation".to_string());
            }
            if priority_state.is_interactive() {
                waiting.push("interaction".to_string());
            }
            waiting
        }));
        self.start_with_state(state, reset)
    }

    fn set_event_sink(&self, events: Arc<dyn DedupEventSink>) {
        *self
            .inner
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = events;
    }

    fn set_priority_probe(&self, priority: Arc<dyn DedupPriorityProbe>) {
        *self
            .inner
            .priority
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = priority;
    }

    fn start_with_store(
        &self,
        reset: bool,
        store: Arc<dyn DedupStore>,
    ) -> Result<DedupProgress, AppError> {
        let (run_id, generation, token, snapshot) = {
            let mut state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.active.is_some() {
                return Err(Self::dedup_error(
                    DEDUP_BUSY_CODE,
                    "精确去重分析正在运行 | Exact dedup analysis is already running",
                ));
            }

            let run_id = self.inner.next_run_id.fetch_add(1, Ordering::Relaxed);
            let (generation, token) = self.inner.token_slot.begin();
            // 游标只服务当前 worker 的 keyset 页。新一轮从头查询 pending sidecar，
            // 由 source_revision/hash_version/status 过滤掉已有有效结果；不复用旧游标，
            // 否则扫描期间发生的旧 item 失效可能落在游标之前而被永久跳过。
            state.cursor = None;
            state.exact_cursor = None;
            state.snapshot = DedupProgress {
                run_id,
                status: DedupStatus::Running,
                phase: DedupPhase::Quick,
                items_done: 0,
                items_total: 0,
                bytes_done: 0,
                bytes_total: 0,
                groups_found: 0,
                potential_logical_bytes: 0,
                errors: Vec::new(),
                waiting_on: Vec::new(),
            };
            state.active = Some(ActiveRun {
                run_id,
                generation,
                token: token.clone(),
                store: store.clone(),
            });
            (run_id, generation, token, state.snapshot.clone())
        };

        self.emit(&snapshot);
        let manager = self.clone();
        let store_for_thread = store;
        let spawn = thread::Builder::new()
            .name("scrollery-dedup".to_string())
            .spawn(move || manager.run(run_id, generation, token, reset, store_for_thread));
        if spawn.is_err() {
            self.fail_current(run_id, generation, DEDUP_START_FAILED_CODE);
            return Err(Self::dedup_error(
                DEDUP_START_FAILED_CODE,
                "无法启动精确去重分析线程 | Failed to start exact dedup analysis",
            ));
        }
        Ok(snapshot)
    }

    /// 请求停止当前轮。停止立即撤销发布权；旧轮迟到的收尾不会覆盖新轮。
    pub fn stop(&self) -> Result<DedupProgress, AppError> {
        let (snapshot, store, generation) = {
            let mut state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(active) = state.active.take() else {
                return Ok(state.snapshot.clone());
            };
            active.token.cancel();
            self.inner.token_slot.cancel();
            state.snapshot.status = DedupStatus::Stopped;
            state.snapshot.waiting_on.clear();
            (state.snapshot.clone(), active.store, active.generation)
        };
        // 先取消内存令牌，再用同一真实 store 推进 DB generation；旧 worker 即使已经
        // 完成一个读块，也只能在这条短事务之前留下可被明确界定的结果。
        store.invalidate_run(generation)?;
        self.emit(&snapshot);
        Ok(snapshot)
    }

    /// 返回内存快照；事件丢失或 webview 重载后仍可恢复显示。
    pub fn status(&self) -> DedupProgress {
        self.inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot
            .clone()
    }

    fn run(
        &self,
        run_id: u64,
        generation: u64,
        token: CancellationToken,
        reset: bool,
        store: Arc<dyn DedupStore>,
    ) {
        if !self.is_current(run_id, &token) {
            return;
        }
        if store.begin_run(generation, reset).is_err() {
            self.fail_current(run_id, generation, DEDUP_STORE_CODE);
            return;
        }

        let total = match store.count_dedup_scan_candidates() {
            Ok(total) => total,
            Err(_) => {
                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                return;
            }
        };
        if !self.update_current(run_id, |progress| {
            progress.items_total = total;
        }) {
            return;
        }

        if !self.run_quick(run_id, generation, &token, store.clone()) {
            return;
        }

        let exact_total = match store.count_dedup_quick_collisions() {
            Ok(count) => count,
            Err(_) => {
                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                return;
            }
        };
        if !self.is_current(run_id, &token) {
            return;
        }
        if !self.update_current(run_id, |progress| {
            progress.phase = DedupPhase::Exact;
            progress.items_done = 0;
            progress.items_total = exact_total;
            progress.bytes_done = 0;
            progress.bytes_total = 0;
            progress.waiting_on.clear();
        }) {
            return;
        }

        let mut pending_rows = Vec::with_capacity(self.inner.batch_size);
        let mut exact_done = 0u64;
        let mut exact_bytes = 0u64;
        let mut exact_cursor = self.exact_cursor();
        loop {
            let candidates = match store
                .list_dedup_quick_collisions(exact_cursor.as_ref(), self.inner.batch_size)
            {
                Ok(candidates) => candidates,
                Err(_) => {
                    self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                    return;
                }
            };
            if candidates.is_empty() {
                break;
            }
            let next_cursor = candidates.last().map(|candidate| DedupCandidateCursor {
                file_size: i64::try_from(candidate.size).unwrap_or(i64::MAX),
                item_id: candidate.item_id,
            });
            for candidate in candidates {
                if !self.wait_for_priority(run_id, &token) {
                    return;
                }
                let (main_exact, unit, component_digests) = if candidate.companions.is_empty() {
                    let cancelled = || !self.is_current(run_id, &token);
                    let exact =
                        match exact_digest_with_snapshot_cancelled(&candidate.path, &cancelled) {
                            Ok(exact) => exact,
                            Err(error) => {
                                if !self.is_current(run_id, &token) {
                                    return;
                                }
                                self.record_error(run_id, error.code());
                                if store
                                    .write_dedup_error_for_generation(
                                        generation,
                                        candidate.item_id,
                                        candidate.source_revision,
                                        candidate.size,
                                        candidate.file_mtime_ns,
                                        error.code(),
                                    )
                                    .is_err()
                                {
                                    self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                                    return;
                                }
                                exact_done = exact_done.saturating_add(1);
                                exact_bytes = exact_bytes.saturating_add(candidate.size);
                                continue;
                            }
                        };
                    if !snapshot_matches_candidate(
                        candidate.size,
                        candidate.file_mtime_ns,
                        exact.snapshot.size,
                        exact.snapshot.mtime_ns,
                    ) {
                        if !self.mark_source_stale(
                            run_id,
                            generation,
                            &token,
                            store.as_ref(),
                            &candidate,
                        ) {
                            return;
                        }
                        exact_done = exact_done.saturating_add(1);
                        exact_bytes = exact_bytes.saturating_add(candidate.size);
                        continue;
                    }
                    let unit = single_unit_digest(exact.snapshot.size, &exact.digest);
                    (exact, unit, Vec::new())
                } else {
                    let companion_paths = candidate
                        .companions
                        .iter()
                        .map(|companion| companion.path.clone())
                        .collect::<Vec<_>>();
                    let cancelled = || !self.is_current(run_id, &token);
                    let components = match live_photo_unit_digest_with_components_cancelled(
                        &candidate.path,
                        companion_paths,
                        &cancelled,
                    ) {
                        Ok(components) => components,
                        Err(error) => {
                            if !self.is_current(run_id, &token) {
                                return;
                            }
                            self.record_error(run_id, error.code());
                            if store
                                .write_dedup_error_for_generation(
                                    generation,
                                    candidate.item_id,
                                    candidate.source_revision,
                                    candidate.size,
                                    candidate.file_mtime_ns,
                                    error.code(),
                                )
                                .is_err()
                            {
                                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                                return;
                            }
                            exact_done = exact_done.saturating_add(1);
                            exact_bytes = exact_bytes.saturating_add(candidate.size);
                            continue;
                        }
                    };
                    let main_matches = snapshot_matches_candidate(
                        candidate.size,
                        candidate.file_mtime_ns,
                        components.main.snapshot.size,
                        components.main.snapshot.mtime_ns,
                    );
                    let companions_match = components.companions.iter().all(|(path, digest)| {
                        let Some(companion) = candidate
                            .companions
                            .iter()
                            .find(|candidate| candidate.path == *path)
                        else {
                            return false;
                        };
                        snapshot_matches_candidate(
                            companion.size,
                            companion.file_mtime_ns,
                            digest.snapshot.size,
                            digest.snapshot.mtime_ns,
                        )
                    });
                    if !main_matches || !companions_match {
                        if !self.mark_source_stale(
                            run_id,
                            generation,
                            &token,
                            store.as_ref(),
                            &candidate,
                        ) {
                            return;
                        }
                        exact_done = exact_done.saturating_add(1);
                        exact_bytes = exact_bytes.saturating_add(candidate.size);
                        continue;
                    }
                    (
                        components.main,
                        components.unit_digest,
                        components.companions,
                    )
                };
                if !self.is_current(run_id, &token) {
                    return;
                }
                let unit_size = main_exact.snapshot.size.saturating_add(
                    component_digests
                        .iter()
                        .map(|(_, digest)| digest.snapshot.size)
                        .sum::<u64>(),
                );
                let main_row = DedupExactRow {
                    item_id: candidate.item_id,
                    source_revision: candidate.source_revision,
                    size: main_exact.snapshot.size,
                    file_mtime_ns: i64::try_from(main_exact.snapshot.mtime_ns).ok(),
                    exact_digest: main_exact.digest,
                    unit_digest: Some(unit),
                    unit_size: Some(unit_size),
                    physical_key: Some(physical_key(main_exact.snapshot.physical_identity)),
                    path: candidate.path.clone(),
                    logical_unit_item_id: candidate.item_id,
                };
                // Companion exact 摘要进入 sidecar，但不带 unit_digest，因此永远不会被
                // 动态重复组当作独立成员；其 source_revision 仍单独守门。
                let component_rows = component_digests
                    .into_iter()
                    .map(|(path, digest)| {
                        candidate
                            .companions
                            .iter()
                            .find(|companion| companion.path == path)
                            .map(|companion| DedupExactRow {
                                item_id: companion.item_id,
                                source_revision: companion.source_revision,
                                size: digest.snapshot.size,
                                file_mtime_ns: i64::try_from(digest.snapshot.mtime_ns).ok(),
                                exact_digest: digest.digest,
                                unit_digest: None,
                                unit_size: None,
                                physical_key: Some(physical_key(digest.snapshot.physical_identity)),
                                path: path.clone(),
                                logical_unit_item_id: candidate.item_id,
                            })
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(component_rows) = component_rows else {
                    if !self.is_current(run_id, &token) {
                        return;
                    }
                    self.record_error(run_id, "SOURCE_STALE");
                    if store
                        .write_dedup_error_for_generation(
                            generation,
                            candidate.item_id,
                            candidate.source_revision,
                            candidate.size,
                            candidate.file_mtime_ns,
                            "SOURCE_STALE",
                        )
                        .is_err()
                    {
                        self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                        return;
                    }
                    continue;
                };
                pending_rows.push(main_row);
                pending_rows.extend(component_rows);
                exact_done = exact_done.saturating_add(1);
                // 进度中的 bytes 是已核验逻辑单元字节，不是 quick 阶段实际读入的采样
                // 字节；Live Photo 因而包含主文件和全部 companion。
                exact_bytes = exact_bytes.saturating_add(unit_size);
                if pending_rows.len() >= self.inner.batch_size {
                    if !self.revalidate_exact_batch(
                        run_id,
                        generation,
                        &token,
                        store.as_ref(),
                        &mut pending_rows,
                    ) {
                        return;
                    }
                    if store
                        .write_dedup_exact_for_generation(generation, &pending_rows)
                        .is_err()
                    {
                        self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                        return;
                    }
                    pending_rows.clear();
                    if !self.update_exact_progress(run_id, exact_done, exact_bytes) {
                        return;
                    }
                }
            }
            exact_cursor = next_cursor;
            if !self.set_exact_cursor(run_id, exact_cursor.clone()) {
                return;
            }
        }
        if !pending_rows.is_empty()
            && !self.revalidate_exact_batch(
                run_id,
                generation,
                &token,
                store.as_ref(),
                &mut pending_rows,
            )
        {
            return;
        }
        if !pending_rows.is_empty() {
            if store
                .write_dedup_exact_for_generation(generation, &pending_rows)
                .is_err()
            {
                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                return;
            }
            if !self.update_exact_progress(run_id, exact_done, exact_bytes) {
                return;
            }
        } else if !self.update_exact_progress(run_id, exact_done, exact_bytes) {
            return;
        }

        if !self.is_current(run_id, &token) {
            return;
        }
        if !self.update_current(run_id, |progress| {
            progress.phase = DedupPhase::Unit;
            progress.items_done = 0;
            progress.items_total = exact_total;
            progress.bytes_done = exact_bytes;
            progress.bytes_total = exact_bytes;
            progress.waiting_on.clear();
        }) {
            return;
        }
        let (groups_found, potential_logical_bytes) = match store.summarize_groups() {
            Ok(summary) => summary,
            Err(_) => {
                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                return;
            }
        };
        if !self.update_current(run_id, |progress| {
            progress.items_done = exact_total;
            progress.bytes_done = progress.bytes_total;
            progress.groups_found = groups_found;
            progress.potential_logical_bytes = potential_logical_bytes;
            progress.waiting_on.clear();
        }) {
            return;
        }
        self.clear_exact_cursor(run_id);
        self.complete_current(run_id, generation);
    }

    /// 在精确摘要进入 DB writer 前重新核对每个逻辑单元的文件身份。摘要线程与写回
    /// 之间仍可能有很短的窗口；再次 no-follow stat 能拒绝同 size/mtime 的替换文件，
    /// 且任一 Live Photo component 失效时整组不写，避免发布混合快照。
    fn revalidate_exact_batch(
        &self,
        run_id: u64,
        generation: u64,
        token: &CancellationToken,
        store: &dyn DedupStore,
        rows: &mut Vec<DedupExactRow>,
    ) -> bool {
        let mut stale_units = BTreeSet::new();
        for row in rows.iter() {
            let current = crate::dedup::file_snapshot(&row.path).ok();
            let matches = current.is_some_and(|snapshot| {
                snapshot.size == row.size
                    && i64::try_from(snapshot.mtime_ns).ok() == row.file_mtime_ns
                    && row.physical_key.as_deref()
                        == Some(physical_key(snapshot.physical_identity).as_slice())
            });
            if !matches {
                stale_units.insert(row.logical_unit_item_id);
            }
        }
        if stale_units.is_empty() {
            return true;
        }
        if !self.is_current(run_id, token) {
            return false;
        }
        let stale_rows = rows
            .iter()
            .filter(|row| stale_units.contains(&row.logical_unit_item_id))
            .collect::<Vec<_>>();
        for row in stale_rows {
            self.record_error(run_id, "SOURCE_STALE");
            if store
                .write_dedup_error_for_generation(
                    generation,
                    row.item_id,
                    row.source_revision,
                    row.size,
                    row.file_mtime_ns,
                    "SOURCE_STALE",
                )
                .is_err()
            {
                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                return false;
            }
        }
        rows.retain(|row| !stale_units.contains(&row.logical_unit_item_id));
        true
    }

    fn run_quick(
        &self,
        run_id: u64,
        generation: u64,
        token: &CancellationToken,
        store: Arc<dyn DedupStore>,
    ) -> bool {
        let mut cursor = self.cursor();
        loop {
            if !self.wait_for_priority(run_id, token) {
                return false;
            }
            let candidates =
                match store.list_dedup_scan_candidates(cursor.as_ref(), self.inner.batch_size) {
                    Ok(candidates) => candidates,
                    Err(_) => {
                        self.fail_current(run_id, self.generation(run_id), DEDUP_STORE_CODE);
                        return false;
                    }
                };
            if candidates.is_empty() {
                return true;
            }

            let mut quick_rows = Vec::with_capacity(candidates.len());
            let mut batch_bytes = 0u64;
            for candidate in &candidates {
                if !self.wait_for_priority(run_id, token) {
                    return false;
                }
                batch_bytes = batch_bytes.saturating_add(candidate.size);
                let cancelled = || !self.is_current(run_id, token);
                match quick_digest_with_snapshot_cancelled(&candidate.path, &cancelled) {
                    Ok(quick) => {
                        if snapshot_matches_candidate(
                            candidate.size,
                            candidate.file_mtime_ns,
                            quick.snapshot.size,
                            quick.snapshot.mtime_ns,
                        ) {
                            quick_rows.push(DedupQuickRow {
                                item_id: candidate.item_id,
                                source_revision: candidate.source_revision,
                                size: quick.snapshot.size,
                                file_mtime_ns: i64::try_from(quick.snapshot.mtime_ns).ok(),
                                physical_key: Some(physical_key(quick.snapshot.physical_identity)),
                                digest: quick.digest,
                            });
                        } else if !self.mark_source_stale(
                            run_id,
                            generation,
                            token,
                            store.as_ref(),
                            candidate,
                        ) {
                            return false;
                        }
                    }
                    Err(error) => {
                        if !self.is_current(run_id, token) {
                            return false;
                        }
                        self.record_error(run_id, error.code());
                        if store
                            .write_dedup_error_for_generation(
                                generation,
                                candidate.item_id,
                                candidate.source_revision,
                                candidate.size,
                                candidate.file_mtime_ns,
                                error.code(),
                            )
                            .is_err()
                        {
                            self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                            return false;
                        }
                    }
                }
            }

            // 文件读取已经完成，写入只接收内存批次，不持有任何文件句柄或 DB
            // writer guard，满足 hash 与 DB writer 解耦。
            if !self.is_current(run_id, token) {
                return false;
            }
            if !quick_rows.is_empty()
                && store
                    .write_dedup_quick_for_generation(generation, &quick_rows)
                    .is_err()
            {
                self.fail_current(run_id, generation, DEDUP_STORE_CODE);
                return false;
            }
            cursor = candidates.last().map(|candidate| DedupCandidateCursor {
                file_size: i64::try_from(candidate.size).unwrap_or(i64::MAX),
                item_id: candidate.item_id,
            });
            if !self.set_cursor(run_id, cursor.clone()) {
                return false;
            }
            if !self.update_current(run_id, |progress| {
                progress.items_done = progress.items_done.saturating_add(candidates.len() as u64);
                progress.bytes_done = progress.bytes_done.saturating_add(batch_bytes);
                progress.bytes_total = progress.bytes_total.saturating_add(batch_bytes);
                progress.waiting_on.clear();
            }) {
                return false;
            }
            thread::yield_now();
        }
    }

    fn update_exact_progress(&self, run_id: u64, done: u64, bytes: u64) -> bool {
        self.update_current(run_id, |progress| {
            progress.items_done = done;
            progress.bytes_done = bytes;
            progress.bytes_total = progress.bytes_total.max(bytes);
            progress.waiting_on.clear();
        })
    }

    fn mark_source_stale(
        &self,
        run_id: u64,
        generation: u64,
        token: &CancellationToken,
        store: &dyn DedupStore,
        candidate: &DedupScanCandidate,
    ) -> bool {
        if !self.is_current(run_id, token) {
            return false;
        }
        self.record_error(run_id, "SOURCE_STALE");
        if store
            .write_dedup_error_for_generation(
                generation,
                candidate.item_id,
                candidate.source_revision,
                candidate.size,
                candidate.file_mtime_ns,
                "SOURCE_STALE",
            )
            .is_err()
        {
            self.fail_current(run_id, generation, DEDUP_STORE_CODE);
            return false;
        }
        true
    }

    fn wait_for_priority(&self, run_id: u64, token: &CancellationToken) -> bool {
        loop {
            if !self.is_current(run_id, token) {
                return false;
            }
            let waiting = self
                .inner
                .priority
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .waiting_on();
            if waiting.is_empty() {
                self.update_waiting_on(run_id, waiting);
                return true;
            }
            self.update_waiting_on(run_id, waiting);
            thread::sleep(Duration::from_millis(PRIORITY_POLL_MS));
        }
    }

    fn update_waiting_on(&self, run_id: u64, waiting: Vec<String>) {
        let _ = self.update_current_if_changed(run_id, |progress| {
            progress.waiting_on = waiting;
        });
    }

    fn update_current<F>(&self, run_id: u64, update: F) -> bool
    where
        F: FnOnce(&mut DedupProgress),
    {
        let snapshot = {
            let mut state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.active.as_ref().map(|active| active.run_id) != Some(run_id) {
                return false;
            }
            update(&mut state.snapshot);
            state.snapshot.clone()
        };
        self.emit(&snapshot);
        true
    }

    fn update_current_if_changed<F>(&self, run_id: u64, update: F) -> bool
    where
        F: FnOnce(&mut DedupProgress),
    {
        let snapshot = {
            let mut state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.active.as_ref().map(|active| active.run_id) != Some(run_id) {
                return false;
            }
            let before = state.snapshot.waiting_on.clone();
            update(&mut state.snapshot);
            if before == state.snapshot.waiting_on {
                return true;
            }
            state.snapshot.clone()
        };
        self.emit(&snapshot);
        true
    }

    fn record_error(&self, run_id: u64, code: &'static str) {
        let _ = self.update_current(run_id, |progress| {
            if let Some(error) = progress.errors.iter_mut().find(|error| error.code == code) {
                error.count = error.count.saturating_add(1);
            } else {
                progress.errors.push(DedupProgressError {
                    code: code.to_string(),
                    count: 1,
                });
            }
        });
    }

    fn complete_current(&self, run_id: u64, generation: u64) {
        let store = {
            let state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(active) = state.active.as_ref() else {
                return;
            };
            if active.run_id != run_id || active.generation != generation {
                return;
            }
            Arc::clone(&active.store)
        };

        // 全表替换可能耗时，不能占着任务控制锁；stop 可在此期间撤销 DB generation。
        let publish_result = store.publish_run(generation);

        let snapshot = {
            let mut state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(active) = state.active.take() else {
                return;
            };
            if active.run_id != run_id || active.generation != generation {
                state.active = Some(active);
                return;
            }
            // compare-and-clear 是旧轮不能清新轮的最后一道闸；stop/restart
            // 若已发生，active 会不匹配而走上面的 no-op 分支。
            if !self.inner.token_slot.finish(generation) {
                state.active = Some(active);
                return;
            }
            state.snapshot.waiting_on.clear();
            if publish_result.is_ok() {
                state.snapshot.status = DedupStatus::Completed;
            } else {
                state.snapshot.status = DedupStatus::Failed;
                state.snapshot.errors.push(DedupProgressError {
                    code: DEDUP_STORE_CODE.to_string(),
                    count: 1,
                });
            }
            state.snapshot.clone()
        };
        self.emit(&snapshot);
    }

    fn fail_current(&self, run_id: u64, generation: u64, code: &'static str) {
        let snapshot = {
            let mut state = self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(active) = state.active.take() else {
                return;
            };
            if active.run_id != run_id || active.generation != generation {
                state.active = Some(active);
                return;
            }
            if !self.inner.token_slot.finish(generation) {
                state.active = Some(active);
                return;
            }
            state.snapshot.status = DedupStatus::Failed;
            state.snapshot.waiting_on.clear();
            if let Some(error) = state
                .snapshot
                .errors
                .iter_mut()
                .find(|error| error.code == code)
            {
                error.count = error.count.saturating_add(1);
            } else {
                state.snapshot.errors.push(DedupProgressError {
                    code: code.to_string(),
                    count: 1,
                });
            }
            state.snapshot.clone()
        };
        self.emit(&snapshot);
    }

    fn is_current(&self, run_id: u64, token: &CancellationToken) -> bool {
        !token.is_cancelled()
            && self
                .inner
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .active
                .as_ref()
                .map(|active| active.run_id)
                == Some(run_id)
    }

    fn set_cursor(&self, run_id: u64, cursor: Option<DedupCandidateCursor>) -> bool {
        let mut state = self
            .inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active.as_ref().map(|active| active.run_id) != Some(run_id) {
            return false;
        }
        state.cursor = cursor;
        true
    }

    fn set_exact_cursor(&self, run_id: u64, cursor: Option<DedupCandidateCursor>) -> bool {
        let mut state = self
            .inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active.as_ref().map(|active| active.run_id) != Some(run_id) {
            return false;
        }
        state.exact_cursor = cursor;
        true
    }

    fn clear_exact_cursor(&self, run_id: u64) {
        let mut state = self
            .inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active.as_ref().map(|active| active.run_id) == Some(run_id) {
            state.exact_cursor = None;
        }
    }

    fn cursor(&self) -> Option<DedupCandidateCursor> {
        self.inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cursor
            .clone()
    }

    fn exact_cursor(&self) -> Option<DedupCandidateCursor> {
        self.inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .exact_cursor
            .clone()
    }

    fn generation(&self, run_id: u64) -> u64 {
        self.inner
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active
            .as_ref()
            .filter(|active| active.run_id == run_id)
            .map(|active| active.generation)
            .unwrap_or(run_id)
    }

    fn emit(&self, snapshot: &DedupProgress) {
        let events = self
            .inner
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        events.emit(snapshot);
    }

    fn dedup_error(code: &'static str, message: &'static str) -> AppError {
        AppError::Dedup {
            code,
            message: message.to_string(),
        }
    }
}

impl Default for DedupTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Condvar;

    struct SlowEmptyStore {
        first_list_started: AtomicBool,
        list_calls: AtomicUsize,
    }

    impl SlowEmptyStore {
        fn new() -> Self {
            Self {
                first_list_started: AtomicBool::new(false),
                list_calls: AtomicUsize::new(0),
            }
        }
    }

    impl DedupStore for SlowEmptyStore {
        fn list_dedup_scan_candidates(
            &self,
            _cursor: Option<&DedupCandidateCursor>,
            _limit: usize,
        ) -> Result<Vec<DedupScanCandidate>, AppError> {
            if self.list_calls.fetch_add(1, Ordering::AcqRel) == 0 {
                self.first_list_started.store(true, Ordering::Release);
                thread::sleep(Duration::from_millis(100));
            }
            Ok(Vec::new())
        }

        fn count_dedup_scan_candidates(&self) -> Result<u64, AppError> {
            Ok(0)
        }

        fn begin_run(&self, _generation: u64, _reset: bool) -> Result<(), AppError> {
            Ok(())
        }

        fn write_dedup_quick_for_generation(
            &self,
            _generation: u64,
            _rows: &[DedupQuickRow],
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn list_dedup_quick_collisions(
            &self,
            _cursor: Option<&DedupCandidateCursor>,
            _limit: usize,
        ) -> Result<Vec<DedupScanCandidate>, AppError> {
            Ok(Vec::new())
        }

        fn write_dedup_exact_for_generation(
            &self,
            _generation: u64,
            _rows: &[DedupExactRow],
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn write_dedup_error_for_generation(
            &self,
            _generation: u64,
            _item_id: i64,
            _source_revision: i64,
            _size: u64,
            _file_mtime_ns: Option<i64>,
            _code: &'static str,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    /// 记录 `publish_run` 钩子调用次数的空存储。trait 默认 no-op，普通 fake
    /// 观测不到发布；专设本存储来验证「完整成功才发布、stop/失败不发布」（方案 §12.2）。
    /// 首个候选查询阻塞片刻，供 stop 测试在运行中途取消；`fail_count_query` 让
    /// `count_dedup_scan_candidates` 报错以驱动失败路径。
    struct HookCountingStore {
        first_list_started: AtomicBool,
        fail_count_query: bool,
        completions: AtomicUsize,
    }

    impl HookCountingStore {
        fn new() -> Self {
            Self {
                first_list_started: AtomicBool::new(false),
                fail_count_query: false,
                completions: AtomicUsize::new(0),
            }
        }

        fn failing() -> Self {
            Self {
                fail_count_query: true,
                ..Self::new()
            }
        }

        fn dedup_store_error() -> AppError {
            AppError::Dedup {
                code: DEDUP_STORE_CODE,
                message: "count failed (test)".to_string(),
            }
        }
    }

    impl DedupStore for HookCountingStore {
        fn list_dedup_scan_candidates(
            &self,
            _cursor: Option<&DedupCandidateCursor>,
            _limit: usize,
        ) -> Result<Vec<DedupScanCandidate>, AppError> {
            if !self.first_list_started.swap(true, Ordering::AcqRel) {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(Vec::new())
        }

        fn count_dedup_scan_candidates(&self) -> Result<u64, AppError> {
            if self.fail_count_query {
                return Err(Self::dedup_store_error());
            }
            Ok(0)
        }

        fn begin_run(&self, _generation: u64, _reset: bool) -> Result<(), AppError> {
            Ok(())
        }

        fn write_dedup_quick_for_generation(
            &self,
            _generation: u64,
            _rows: &[DedupQuickRow],
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn list_dedup_quick_collisions(
            &self,
            _cursor: Option<&DedupCandidateCursor>,
            _limit: usize,
        ) -> Result<Vec<DedupScanCandidate>, AppError> {
            Ok(Vec::new())
        }

        fn write_dedup_exact_for_generation(
            &self,
            _generation: u64,
            _rows: &[DedupExactRow],
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn write_dedup_error_for_generation(
            &self,
            _generation: u64,
            _item_id: i64,
            _source_revision: i64,
            _size: u64,
            _file_mtime_ns: Option<i64>,
            _code: &'static str,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn publish_run(&self, _generation: u64) -> Result<(), AppError> {
            self.completions.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    struct RecordingSink {
        snapshots: Mutex<Vec<DedupProgress>>,
        changed: Condvar,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self {
                snapshots: Mutex::new(Vec::new()),
                changed: Condvar::new(),
            }
        }

        fn all(&self) -> Vec<DedupProgress> {
            self.snapshots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl DedupEventSink for RecordingSink {
        fn emit(&self, progress: &DedupProgress) {
            self.snapshots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(progress.clone());
            self.changed.notify_all();
        }
    }

    fn wait_until<F>(mut predicate: F)
    where
        F: FnMut() -> bool,
    {
        for _ in 0..100 {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(predicate(), "timed out waiting for dedup worker");
    }

    #[test]
    fn start_rejects_a_second_active_run_with_stable_busy_code() {
        let store = Arc::new(SlowEmptyStore::new());
        let manager = DedupTaskManager::with_store(store.clone());
        let first = manager.start(false).expect("first run starts");
        assert_eq!(first.status, DedupStatus::Running);
        wait_until(|| store.first_list_started.load(Ordering::Acquire));

        let error = manager.start(false).expect_err("second run must be busy");
        match error {
            AppError::Dedup { code, .. } => assert_eq!(code, DEDUP_BUSY_CODE),
            other => panic!("unexpected error: {other:?}"),
        }
        let _ = manager.stop().expect("stop active run");
    }

    #[test]
    fn stop_then_restart_does_not_publish_old_terminal_state_or_clear_new_run() {
        let store = Arc::new(SlowEmptyStore::new());
        let sink = Arc::new(RecordingSink::new());
        let manager = DedupTaskManager::with_parts(
            store.clone(),
            sink.clone(),
            Arc::new(DedupPriorityGate::new()),
            4,
        );
        let first = manager.start(false).expect("first run starts");
        wait_until(|| store.first_list_started.load(Ordering::Acquire));
        let stopped = manager.stop().expect("stop first run");
        assert_eq!(stopped.run_id, first.run_id);
        assert_eq!(stopped.status, DedupStatus::Stopped);

        let second = manager.start(false).expect("restart starts");
        assert!(second.run_id > first.run_id);
        wait_until(|| manager.status().status == DedupStatus::Completed);
        thread::sleep(Duration::from_millis(130));

        let snapshots = sink.all();
        assert!(snapshots.iter().any(|snapshot| {
            snapshot.run_id == second.run_id && snapshot.status == DedupStatus::Completed
        }));
        assert!(!snapshots.iter().any(|snapshot| {
            snapshot.run_id == first.run_id && snapshot.status == DedupStatus::Completed
        }));
        assert_eq!(manager.status().run_id, second.run_id);
        assert_eq!(manager.status().status, DedupStatus::Completed);
    }

    #[test]
    fn status_snapshot_contains_resume_fields_and_waiting_on() {
        let store = Arc::new(SlowEmptyStore::new());
        let gate = Arc::new(DedupPriorityGate::new());
        gate.set_scanning(true);
        let manager = DedupTaskManager::with_parts(
            store.clone(),
            Arc::new(NoopDedupEventSink),
            gate.clone(),
            4,
        );
        let started = manager.start(false).expect("run starts");
        assert_eq!(started.run_id, manager.status().run_id);
        assert_eq!(manager.status().status, DedupStatus::Running);
        wait_until(|| manager.status().waiting_on == vec!["scanning".to_string()]);
        for snapshot in [manager.status()] {
            let json = serde_json::to_value(snapshot).expect("progress serializes");
            for field in [
                "runId",
                "status",
                "phase",
                "itemsDone",
                "itemsTotal",
                "bytesDone",
                "bytesTotal",
                "groupsFound",
                "potentialLogicalBytes",
                "errors",
                "waitingOn",
            ] {
                assert!(json.get(field).is_some(), "missing snapshot field {field}");
            }
        }
        gate.set_scanning(false);
        let _ = manager.stop().expect("stop snapshot test");
    }

    #[test]
    fn completed_run_publishes_epoch_hook_exactly_once() {
        let store = Arc::new(HookCountingStore::new());
        let manager = DedupTaskManager::with_store(store.clone());
        manager.start(false).expect("run starts");
        wait_until(|| manager.status().status == DedupStatus::Completed);
        // 完整成功是唯一发布点；第二段 wait 容忍状态翻转与钩子调用间的纳秒级窗口，
        // 若钩子从未触发则超时 panic（方案 §12.2：完整成功后发布一次新 epoch）。
        wait_until(|| store.completions.load(Ordering::Acquire) == 1);
        assert_eq!(manager.status().status, DedupStatus::Completed);
    }

    #[test]
    fn stopped_run_never_publishes_epoch_hook() {
        let store = Arc::new(HookCountingStore::new());
        let manager = DedupTaskManager::with_store(store.clone());
        manager.start(false).expect("run starts");
        wait_until(|| store.first_list_started.load(Ordering::Acquire));
        let stopped = manager.stop().expect("stop run");
        assert_eq!(stopped.status, DedupStatus::Stopped);
        // 等 worker 走完阻塞中的查询并退出：迟到的收尾也不得补发发布钩子。
        thread::sleep(Duration::from_millis(150));
        assert_eq!(store.completions.load(Ordering::Acquire), 0);
        assert_eq!(manager.status().status, DedupStatus::Stopped);
    }

    #[test]
    fn failed_run_never_publishes_epoch_hook() {
        let store = Arc::new(HookCountingStore::failing());
        let manager = DedupTaskManager::with_store(store.clone());
        manager.start(false).expect("run starts");
        wait_until(|| manager.status().status == DedupStatus::Failed);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(store.completions.load(Ordering::Acquire), 0);
    }
}
