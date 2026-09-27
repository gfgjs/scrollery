//! 图片与原生视频封面的共享领取与执行出口。入口只提供源快照和需求，任务状态由数据库 lease 决定。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ring::rand::SecureRandom;
use tokio_util::sync::CancellationToken;

use crate::db::models::ThumbResult;
use crate::db::queries::{
    self, ThumbnailCandidate, ThumbnailLane, ThumbnailTaskKey, ThumbnailTaskKind,
};
use crate::error::{AppError, Result};
use crate::scanner::hdd_io::StorageDevice;
use crate::state::AppState;

#[cfg(not(windows))]
use super::generator::{decode_deferred_cpu, decode_media_step};
use super::generator::{
    encode_media_payload, snap_to_tier, write_encoded_media_payload, DecodeResult,
    EncodedThumbPayload, ThumbConfig,
};
use super::scheduler::OutputFingerprint;

/// 缩略图任务的执行结果；跳过表示旧源、重复领取、取消或数据库已换代。
#[derive(Clone)]
pub enum ImageExecution {
    Published(ThumbResult, ExecutionFacts),
    Deferred(i64),
    /// 本轮已条件收口，媒体展示保留，后续请求可重试。
    Unavailable(i64),
    Skipped(i64),
}

/// 与发布结果一起分发的执行事实；复用产物没有本次解码后端。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExecutionFacts {
    pub newly_generated: bool,
    /// None 表示宿主尚未提供阶段事实，不能据设置推断硬件。
    pub native: Option<super::native_protocol::NativeExecution>,
}

/// 原生子进程的应用生命周期累计值；成功表示产出编码结果，不表示数据库已发布。
#[derive(Default)]
pub(crate) struct NativeCounters {
    pub(crate) gpu_inflight: usize,
    pub(crate) gpu_peak: usize,
    pub(crate) gpu_denied: u64,
    pub(crate) gpu_limit: usize,
    pub(crate) embedded_jpeg: u64,
    pub(crate) image_d2d: u64,
    pub(crate) image_vpl: u64,
    pub(crate) image_wic: u64,
    pub(crate) image_rs: u64,
    pub(crate) video_mf: u64,
    pub(crate) video_hardware_mft: u64,
    pub(crate) failed_responses: u64,
    pub(crate) timed_out: u64,
    pub(crate) worker_lost: u64,
    pub(crate) qos_apply_accepted: u64,
    pub(crate) qos_apply_failed: u64,
    pub(crate) qos_cross_boundary: u64,
    pub(crate) domain_wait: LatencySummary,
    pub(crate) request_wall: LatencySummary,
    pub(crate) decode_transform: LatencySummary,
    pub(crate) encode_hash: LatencySummary,
    pub(crate) embedded_jpeg_combined: LatencySummary,
    pub(crate) host_working_set_bytes: Option<u64>,
    pub(crate) host_private_bytes: Option<u64>,
    pub(crate) host_cpu_ms: Option<u64>,
    pub(crate) worker_job_cpu_ms: Option<u64>,
    pub(crate) qos_workers: super::qos::QosCounts,
    pub(crate) qos_revision: u64,
    pub(crate) qos_foreground: bool,
    pub(crate) worker_processes: usize,
    pub(crate) worker_memory_samples: usize,
    pub(crate) worker_working_set_bytes: Option<u64>,
    pub(crate) worker_private_bytes: Option<u64>,
}

/// 缩略图阶段的应用生命周期耗时；分位值为二次幂桶的近似上界。
#[derive(Debug, Default)]
pub(crate) struct LatencySummary {
    pub(crate) count: u64,
    pub(crate) sum_ms: u64,
    pub(crate) max_ms: u64,
    pub(crate) p50_ms_upper: u64,
    pub(crate) p95_ms_upper: u64,
}

const LATENCY_BUCKETS: usize = 17;

pub(crate) struct LatencyStats {
    sum_ms: AtomicU64,
    max_ms: AtomicU64,
    buckets: [AtomicU64; LATENCY_BUCKETS],
}

impl LatencyStats {
    pub(crate) fn new() -> Self {
        Self {
            sum_ms: AtomicU64::new(0),
            max_ms: AtomicU64::new(0),
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(crate) fn record(&self, elapsed: Duration) {
        let millis = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let index = if millis <= 1 {
            0
        } else {
            (u64::BITS - (millis - 1).leading_zeros()) as usize
        }
        .min(LATENCY_BUCKETS - 1);
        self.sum_ms.fetch_add(millis, Ordering::Relaxed);
        self.max_ms.fetch_max(millis, Ordering::Relaxed);
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> LatencySummary {
        let buckets = self
            .buckets
            .each_ref()
            .map(|count| count.load(Ordering::Relaxed));
        let count: u64 = buckets.iter().sum();
        let max_ms = self.max_ms.load(Ordering::Relaxed);
        LatencySummary {
            count,
            sum_ms: self.sum_ms.load(Ordering::Relaxed),
            max_ms,
            p50_ms_upper: latency_percentile_upper(&buckets, count, 50, max_ms),
            p95_ms_upper: latency_percentile_upper(&buckets, count, 95, max_ms),
        }
    }
}

struct LatencyTimer<'a> {
    stats: &'a LatencyStats,
    started: Instant,
}

impl<'a> LatencyTimer<'a> {
    fn new(stats: &'a LatencyStats) -> Self {
        Self {
            stats,
            started: Instant::now(),
        }
    }
}

impl Drop for LatencyTimer<'_> {
    fn drop(&mut self) {
        self.stats.record(self.started.elapsed());
    }
}

fn latency_percentile_upper(
    buckets: &[u64; LATENCY_BUCKETS],
    count: u64,
    percentile: u64,
    max_ms: u64,
) -> u64 {
    if count == 0 {
        return 0;
    }
    let target = count.saturating_mul(percentile).div_ceil(100);
    let mut seen = 0;
    for (index, entries) in buckets.iter().enumerate() {
        seen += *entries;
        if seen >= target {
            return if index < LATENCY_BUCKETS - 1 {
                1u64 << index
            } else {
                max_ms
            };
        }
    }
    max_ms
}

pub(crate) struct HostStageTimings {
    pub(crate) queue_wall: LatencySummary,
    pub(crate) e2e_wall: LatencySummary,
    pub(crate) encode_wall: LatencySummary,
    pub(crate) write_wall: LatencySummary,
    pub(crate) db_transaction_wall: LatencySummary,
}

type RunResultHandler = Arc<dyn Fn(&ImageExecution) + Send + Sync>;
type RunResults = Arc<Mutex<HashMap<String, RunResultHandler>>>;

/// 活跃全库轮次的提交观察者；生命周期结束后不接收迟到结果。
pub(crate) struct RunResultObserver {
    run_id: String,
    results: RunResults,
}

impl RunResultObserver {
    /// 等待已可见的DB提交完成计数，防止零待办快照先于提交回调结束。
    pub(crate) fn synchronize(&self) {
        drop(self.results.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

impl Drop for RunResultObserver {
    fn drop(&mut self) {
        self.results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.run_id);
    }
}

/// 所有缩略图入口共享的执行额度。DB lease 负责同一任务键去重，此额度限制不同键的并行工作集。
pub struct ThumbnailCoordinator {
    run_results: RunResults,
    capacity: usize,
    queue: Arc<WorkQueue>,
    memory_budget: Arc<MemoryBudget>,
    volume_io_budget: Arc<VolumeIoBudget>,
    encode_wall: LatencyStats,
    write_wall: LatencyStats,
    db_transaction_wall: LatencyStats,
    started: OnceLock<()>,
    viewport_requests: Arc<Mutex<HashMap<String, Weak<ViewportRequest>>>>,
    #[cfg(windows)]
    native: super::native_worker::NativeWorkers,
}

/// 单次视口 IPC 的逐项订阅；取消只影响该请求方的回传和等待。
pub struct ViewportRequest {
    id: String,
    active: Mutex<HashSet<i64>>,
    registry: Weak<Mutex<HashMap<String, Weak<ViewportRequest>>>>,
}

impl ViewportRequest {
    pub fn is_active(&self, item_id: i64) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&item_id)
    }

    pub fn cancel(&self, item_ids: &[i64]) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        for item_id in item_ids {
            active.remove(item_id);
        }
    }

    /// 与取消串行化，防止取消已受理后仍向旧 Channel 发送结果。
    pub fn deliver<R>(&self, item_id: i64, send: impl FnOnce() -> R) -> Option<R> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.remove(&item_id) {
            Some(send())
        } else {
            None
        }
    }
}

impl Drop for ViewportRequest {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            let mut entries = registry.lock().unwrap_or_else(|e| e.into_inner());
            if entries
                .get(&self.id)
                .is_some_and(|entry| std::ptr::eq(entry.as_ptr(), self))
            {
                entries.remove(&self.id);
            }
        }
    }
}

struct ImageJob {
    // 共享执行从首次入队计时；订阅方合并和资源重排不重置起点。
    queued_at: Instant,
    kind: ThumbnailTaskKind,
    candidate: ThumbnailCandidate,
    config: ThumbConfig,
    epoch: u64,
    lane: ThumbnailLane,
    exception_budget: bool,
    cancel: CancellationToken,
    viewport_request: Option<Weak<ViewportRequest>>,
    response: crossbeam_channel::Sender<Result<ImageExecution>>,
    shared: Option<Arc<SharedWork>>,
}

struct PreparedResult {
    key: ThumbnailTaskKey,
    lease_id: String,
    result: Option<ThumbResult>,
    payload: Option<EncodedThumbPayload>,
    execution: ExecutionFacts,
    unavailable_reason: Option<&'static str>,
    write_size: u32,
    exception_lane: bool,
}

enum TaskPreparation {
    #[cfg(windows)]
    Pending(Box<super::native_worker::NativePending>, PreparedResult),
    Immediate(ImageExecution),
    Ready(PreparedResult),
    ToEncode(EncodePreparation),
}

#[cfg(windows)]
struct NativeWork {
    request: Box<super::native_worker::NativePending>,
    job: ImageJob,
    prepared: PreparedResult,
    _heavy_permit: Option<crate::exotic::limiter::HeavyPermit>,
    memory_permit: MemoryPermit,
    _volume_permit: Option<VolumeIoPermit>,
}

struct EncodePreparation {
    key: ThumbnailTaskKey,
    lease_id: String,
    exception_lane: bool,
    decoded: crate::engine::traits::DecodedImage,
    encode_size: u32,
}

struct EncodeWork {
    job: ImageJob,
    prepared: EncodePreparation,
    heavy_permit: Option<crate::exotic::limiter::HeavyPermit>,
    _memory_permit: MemoryPermit,
}

struct CommitWork {
    job: ImageJob,
    prepared: PreparedResult,
    // 包括子进程回包和落盘等待，结果字节不得绕过在途额度。
    _memory_permit: MemoryPermit,
}

const COMMIT_BATCH_SIZE: usize = 50;
const COMMIT_BATCH_WAIT: Duration = Duration::from_millis(100);
const MAX_INFLIGHT_DECODE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SINGLE_RESERVATION_BYTES: u64 = 256 * 1024 * 1024;
const UNCERTAIN_RESERVATION_BYTES: u64 = 432 * 1024 * 1024;
const MIN_ENCODED_RESERVATION_BYTES: u64 = 80 * 1024 * 1024;
const SSD_VOLUME_IO_LIMIT: usize = 4;
const HDD_VOLUME_IO_LIMIT: usize = 1;
const UNKNOWN_VOLUME_IO_LIMIT: usize = 1;

/// 源卷准入按系统探测的物理设备聚合；未知设备共用保守额度。
pub(crate) struct VolumeIoBudget {
    state: Mutex<VolumeIoState>,
}

type SourceVolume = (u64, Option<i64>);

struct ActiveVolume {
    count: usize,
    device: Option<StorageDevice>,
}

#[derive(Default)]
struct VolumeIoState {
    active: HashMap<SourceVolume, ActiveVolume>,
    media_epoch: Option<u64>,
    devices: HashMap<i64, StorageDevice>,
    current: usize,
    peak: usize,
    denied_attempts: u64,
}

impl VolumeIoState {
    fn device_for(&self, epoch: u64, volume_id: Option<i64>) -> Option<StorageDevice> {
        // 旧库在途资源保留其设备身份，新库同号 volume_id 使用独立键。
        self.active
            .get(&(epoch, volume_id))
            .and_then(|active| active.device)
            .or_else(|| {
                (self.media_epoch == Some(epoch))
                    .then(|| volume_id.and_then(|id| self.devices.get(&id)).copied())
                    .flatten()
            })
    }
}

pub(crate) struct VolumeIoSnapshot {
    pub active_by_volume: Vec<(u64, Option<i64>, usize)>,
    pub media_by_volume: Vec<(i64, u32, bool)>,
    pub media_epoch: Option<u64>,
    pub current: usize,
    pub peak: usize,
    pub denied_attempts: u64,
    pub ssd_limit: usize,
    pub hdd_limit: usize,
    pub unknown_limit: usize,
}

impl VolumeIoBudget {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(VolumeIoState::default()),
        })
    }

    /// 探测在锁外完成；调用方在数据库代次校验后一次发布。
    pub(crate) fn publish_devices(&self, epoch: u64, devices: HashMap<i64, StorageDevice>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for ((active_epoch, id), active) in &mut state.active {
            if *active_epoch == epoch {
                active.device = id.and_then(|id| devices.get(&id)).copied();
            }
        }
        state.media_epoch = Some(epoch);
        state.devices = devices;
    }

    fn device_for(&self, epoch: u64, volume_id: Option<i64>) -> Option<StorageDevice> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .device_for(epoch, volume_id)
    }

    fn device_is_idle(&self, number: u32) -> bool {
        !self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active
            .values()
            .any(|active| active.device.is_some_and(|device| device.number == number))
    }

    #[cfg(windows)]
    fn media_for(&self, epoch: u64, volume_id: Option<i64>) -> super::qos::SourceMedia {
        match self.device_for(epoch, volume_id) {
            Some(device) if device.incurs_seek_penalty => super::qos::SourceMedia::Hdd,
            Some(_) => super::qos::SourceMedia::Ssd,
            None => super::qos::SourceMedia::Unknown,
        }
    }

    /// 共享准入水位；拒绝数含同一任务重试，不代表实际磁盘 IO 次数。
    pub(crate) fn snapshot(&self) -> VolumeIoSnapshot {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut active_by_volume: Vec<_> = state
            .active
            .iter()
            .map(|((epoch, id), active)| (*epoch, *id, active.count))
            .collect();
        active_by_volume.sort_unstable();
        let mut media_by_volume: Vec<_> = state
            .devices
            .iter()
            .map(|(id, device)| (*id, device.number, device.incurs_seek_penalty))
            .collect();
        media_by_volume.sort_unstable();
        VolumeIoSnapshot {
            active_by_volume,
            media_by_volume,
            media_epoch: state.media_epoch,
            current: state.current,
            peak: state.peak,
            denied_attempts: state.denied_attempts,
            ssd_limit: SSD_VOLUME_IO_LIMIT,
            hdd_limit: HDD_VOLUME_IO_LIMIT,
            unknown_limit: UNKNOWN_VOLUME_IO_LIMIT,
        }
    }

    pub(crate) fn try_acquire(
        self: &Arc<Self>,
        epoch: u64,
        volume_id: Option<i64>,
    ) -> Option<VolumeIoPermit> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let source = (epoch, volume_id);
        let device = state.device_for(epoch, volume_id);
        let limit = match device {
            Some(device) if device.incurs_seek_penalty => HDD_VOLUME_IO_LIMIT,
            Some(_) => SSD_VOLUME_IO_LIMIT,
            None => UNKNOWN_VOLUME_IO_LIMIT,
        };
        let count: usize = state
            .active
            .values()
            .filter(|active| active.device.map(|d| d.number) == device.map(|d| d.number))
            .map(|active| active.count)
            .sum();
        if count >= limit {
            state.denied_attempts = state.denied_attempts.saturating_add(1);
            return None;
        }
        state
            .active
            .entry(source)
            .or_insert(ActiveVolume { count: 0, device })
            .count += 1;
        state.current += 1;
        state.peak = state.peak.max(state.current);
        Some(VolumeIoPermit {
            budget: Arc::clone(self),
            source,
        })
    }
}

pub(crate) struct VolumeIoPermit {
    budget: Arc<VolumeIoBudget>,
    source: SourceVolume,
}

impl Drop for VolumeIoPermit {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap_or_else(|e| e.into_inner());
        let active = state
            .active
            .get_mut(&self.source)
            .expect("held volume IO permit");
        active.count -= 1;
        if active.count == 0 {
            state.active.remove(&self.source);
        }
        state.current -= 1;
    }
}

pub(crate) struct MemoryBudget {
    reserved: Mutex<u64>,
    changed: Condvar,
    peak: std::sync::atomic::AtomicU64,
}

impl MemoryBudget {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            reserved: Mutex::new(0),
            changed: Condvar::new(),
            peak: AtomicU64::new(0),
        })
    }

    fn try_reserve(self: &Arc<Self>, bytes: u64) -> Option<MemoryPermit> {
        let mut reserved = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        if reserved.saturating_add(bytes) > MAX_INFLIGHT_DECODE_BYTES {
            return None;
        }
        *reserved += bytes;
        self.peak
            .fetch_max(*reserved, std::sync::atomic::Ordering::Relaxed);
        Some(MemoryPermit {
            budget: Arc::clone(self),
            bytes,
        })
    }

    /// 同时取重活和工作集额度；任一额度不足即释放另一项，不持旧额度阻塞等待。
    pub(crate) fn acquire_with_heavy(
        self: &Arc<Self>,
        limiter: &Arc<crate::exotic::limiter::BackgroundHeavyLimiter>,
        bytes: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<(crate::exotic::limiter::HeavyPermit, MemoryPermit)> {
        if bytes > MAX_INFLIGHT_DECODE_BYTES {
            return None;
        }
        loop {
            let heavy = limiter.acquire_cancellable(cancelled)?;
            if let Some(memory) = self.try_reserve(bytes) {
                return Some((heavy, memory));
            }
            drop(heavy);
            if !self.wait_for_room(bytes, cancelled) {
                return None;
            }
        }
    }

    fn wait_for_room(&self, bytes: u64, cancelled: &dyn Fn() -> bool) -> bool {
        loop {
            if cancelled() {
                return false;
            }
            let reserved = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
            if reserved.saturating_add(bytes) <= MAX_INFLIGHT_DECODE_BYTES {
                return true;
            }
            let (reserved, _) = self
                .changed
                .wait_timeout(reserved, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner());
            drop(reserved);
        }
    }
}

pub(crate) struct MemoryPermit {
    budget: Arc<MemoryBudget>,
    bytes: u64,
}

impl MemoryPermit {
    /// 当前实际持有工作集可支持的单像素缓冲上限。
    pub(crate) fn pixel_limit(&self) -> u64 {
        super::native_protocol::pixel_limit_for_reservation(self.bytes)
    }
    /// 解码结束后只保留结果缓冲的容量和少量任务记录空间，及时让出下一项的工作集。
    fn retain_result(&mut self, prepared: &PreparedResult) {
        let payload_bytes = prepared.payload.as_ref().map_or(0, |payload| {
            payload.webp.capacity() as u64
                + payload
                    .thumbhash
                    .as_ref()
                    .map_or(0, |bytes| bytes.capacity() as u64)
                + payload
                    .ai_cache
                    .as_ref()
                    .map_or(0, |bytes| bytes.capacity() as u64)
        });
        self.shrink_to(payload_bytes.saturating_add(1024 * 1024));
    }

    fn shrink_to(&mut self, bytes: u64) {
        // 只归还已预留的解码空间；容量未下降时不额外借额或等待。
        if bytes >= self.bytes {
            return;
        }
        let mut reserved = self
            .budget
            .reserved
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *reserved -= self.bytes - bytes;
        self.bytes = bytes;
        drop(reserved);
        self.budget.changed.notify_all();
    }
}

impl Drop for MemoryPermit {
    fn drop(&mut self) {
        let mut reserved = self
            .budget
            .reserved
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *reserved -= self.bytes;
        drop(reserved);
        self.budget.changed.notify_all();
    }
}

fn estimated_decode_bytes(job: &ImageJob) -> u64 {
    if job.kind == ThumbnailTaskKind::VideoCover {
        return 64 * 1024 * 1024;
    }
    let item = &job.candidate.item;
    if super::generator::direct_display_reason(item, &job.config).is_some() {
        return 1024 * 1024;
    }
    if job.exception_budget
        || job.lane == ThumbnailLane::Exception
        || job.shared.as_ref().is_some_and(|shared| {
            shared
                .subscribers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|subscriber| subscriber.lane == ThumbnailLane::Exception)
        })
    {
        // 元数据低估导致首次超额时，有限异常补救使用最大单项额度。
        return UNCERTAIN_RESERVATION_BYTES;
    }
    estimated_image_decode_bytes(item.width, item.height)
}

fn estimated_image_decode_bytes(width: i64, height: i64) -> u64 {
    if width <= 0 || height <= 0 {
        return UNCERTAIN_RESERVATION_BYTES;
    }
    let estimated = (width as u64)
        .saturating_mul(height as u64)
        .saturating_mul(16)
        .saturating_add(super::native_protocol::RESULT_RESERVATION_BYTES);
    if estimated > MAX_SINGLE_RESERVATION_BYTES {
        // 元数据过大时不强压回普通额度；保留 80 MiB 给一个普通视口结果。
        UNCERTAIN_RESERVATION_BYTES
    } else {
        estimated.max(MIN_ENCODED_RESERVATION_BYTES)
    }
}

fn collect_commit_batch<T>(first: T, rx: &crossbeam_channel::Receiver<T>) -> Vec<T> {
    let deadline = Instant::now() + COMMIT_BATCH_WAIT;
    let mut batch = vec![first];
    while batch.len() < COMMIT_BATCH_SIZE {
        match rx.recv_deadline(deadline) {
            Ok(work) => batch.push(work),
            Err(_) => break,
        }
    }
    batch
}

impl ImageJob {
    fn cancelled_before_claim(&self) -> bool {
        if let Some(shared) = &self.shared {
            return !shared.has_active_subscriber();
        }
        self.cancel.is_cancelled()
            || self.viewport_request.as_ref().is_some_and(|request| {
                request
                    .upgrade()
                    .is_none_or(|request| !request.is_active(self.candidate.item.id))
            })
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct WorkKey {
    epoch: u64,
    item_id: i64,
    source_revision: i64,
    kind: &'static str,
    output_fingerprint: String,
}

struct WorkSubscriber {
    item_id: i64,
    lane: ThumbnailLane,
    cancel: CancellationToken,
    viewport_request: Option<Weak<ViewportRequest>>,
    response: crossbeam_channel::Sender<Result<ImageExecution>>,
}

impl WorkSubscriber {
    fn is_active(&self, item_id: i64) -> bool {
        !self.cancel.is_cancelled()
            && self.viewport_request.as_ref().is_none_or(|request| {
                request
                    .upgrade()
                    .is_some_and(|request| request.is_active(item_id))
            })
    }

    fn skip(self) {
        let _ = self
            .response
            .send(Ok(ImageExecution::Skipped(self.item_id)));
    }
}

struct SharedWork {
    key: WorkKey,
    subscribers: Mutex<Vec<WorkSubscriber>>,
}

impl SharedWork {
    fn preferred_lane(&self, current: ThumbnailLane) -> ThumbnailLane {
        let subscribers = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        let active: Vec<_> = subscribers
            .iter()
            .filter(|subscriber| subscriber.is_active(self.key.item_id))
            .collect();
        if active
            .iter()
            .any(|subscriber| subscriber.viewport_request.is_some())
        {
            if matches!(
                current,
                ThumbnailLane::Heavy | ThumbnailLane::Exception | ThumbnailLane::ViewportHeavy
            ) || active
                .iter()
                .any(|subscriber| subscriber.lane == ThumbnailLane::ViewportHeavy)
            {
                ThumbnailLane::ViewportHeavy
            } else {
                ThumbnailLane::ViewportFast
            }
        } else {
            active
                .iter()
                .find(|subscriber| subscriber.viewport_request.is_none())
                .map_or(current, |subscriber| subscriber.lane)
        }
    }

    fn has_active_subscriber(&self) -> bool {
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|subscriber| subscriber.is_active(self.key.item_id))
    }

    fn detach_inactive(&self) -> Vec<WorkSubscriber> {
        let mut subscribers = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        let mut detached = Vec::new();
        let mut index = 0;
        while index < subscribers.len() {
            if subscribers[index].is_active(self.key.item_id) {
                index += 1;
            } else {
                detached.push(subscribers.remove(index));
            }
        }
        detached
    }

    fn finish(&self, result: Result<ImageExecution>) {
        let subscribers =
            std::mem::take(&mut *self.subscribers.lock().unwrap_or_else(|e| e.into_inner()));
        match result {
            Ok(outcome) => {
                for subscriber in subscribers {
                    if subscriber.is_active(self.key.item_id) {
                        let _ = subscriber.response.send(Ok(outcome.clone()));
                    } else {
                        subscriber.skip();
                    }
                }
            }
            Err(error) => {
                let mut error = Some(error);
                for subscriber in subscribers {
                    if subscriber.is_active(self.key.item_id) {
                        let error = error.take().unwrap_or_else(|| {
                            AppError::Internal("shared thumbnail task failed".into())
                        });
                        let _ = subscriber.response.send(Err(error));
                    } else {
                        subscriber.skip();
                    }
                }
            }
        }
    }
}

struct RunOptions<'a> {
    config: &'a ThumbConfig,
    kind: ThumbnailTaskKind,
    cancel: &'a CancellationToken,
}

/// 固定轮次中视频封面当前尾批的查询条件。
pub struct VideoCoverPhase<'a> {
    pub run_id: &'a str,
    pub lane: ThumbnailLane,
    pub index: usize,
}

#[derive(Default)]
struct JobQueues {
    active: HashMap<WorkKey, Arc<SharedWork>>,
    viewport_fast: VecDeque<ImageJob>,
    viewport_heavy: VecDeque<ImageJob>,
    background_fast: VecDeque<ImageJob>,
    background_heavy: VecDeque<ImageJob>,
    background_exception: VecDeque<ImageJob>,
    viewport_streak: u8,
    locality_epoch: Option<u64>,
    hdd_locality: HashMap<u32, DirectoryLocality>,
}

struct DirectoryLocality {
    directory_id: i64,
    dispatched: u8,
}

const QUEUE_CAPACITY: usize = 256;
const VIEWPORT_RESERVED_CAPACITY: usize = 32;

impl JobQueues {
    fn prefer_local_job(&mut self, job: ImageJob, budget: &VolumeIoBudget) -> ImageJob {
        if self.locality_epoch != Some(job.epoch) {
            return job;
        }
        let Some(device) = budget
            .device_for(job.epoch, job.candidate.volume_id)
            .filter(|d| d.incurs_seek_penalty)
        else {
            return job;
        };
        let Some(hint) = self.hdd_locality.get(&device.number) else {
            return job;
        };
        // 已有读取占盘时沿用 FIFO 让步，避免不断挑出同盘项挡住其它空闲设备。
        if !budget.device_is_idle(device.number) {
            return job;
        }
        // 连续四项后让 FIFO 队首前进，目录局部性不能长期饿死其它路径。
        if hint.dispatched >= 4 || hint.directory_id == job.candidate.item.directory_id {
            return job;
        }
        let lane = match job.lane {
            ThumbnailLane::ViewportFast => &mut self.viewport_fast,
            ThumbnailLane::ViewportHeavy => &mut self.viewport_heavy,
            ThumbnailLane::Fast => &mut self.background_fast,
            ThumbnailLane::Heavy => &mut self.background_heavy,
            ThumbnailLane::Exception => &mut self.background_exception,
        };
        let index = lane.iter().position(|other| {
            other.epoch == job.epoch
                && other.kind == job.kind
                && other.candidate.volume_id == job.candidate.volume_id
                && other.candidate.item.directory_id == hint.directory_id
        });
        if let Some(index) = index {
            let local = lane.remove(index).expect("queued local job");
            lane.push_front(job);
            local
        } else {
            job
        }
    }

    fn note_source_dispatch(&mut self, job: &ImageJob, budget: &VolumeIoBudget) {
        let Some(device) = budget
            .device_for(job.epoch, job.candidate.volume_id)
            .filter(|d| d.incurs_seek_penalty)
        else {
            return;
        };
        if self.locality_epoch != Some(job.epoch) {
            self.locality_epoch = Some(job.epoch);
            self.hdd_locality.clear();
        }
        let hint = self
            .hdd_locality
            .entry(device.number)
            .or_insert(DirectoryLocality {
                directory_id: job.candidate.item.directory_id,
                dispatched: 0,
            });
        if hint.directory_id != job.candidate.item.directory_id || hint.dispatched >= 4 {
            hint.directory_id = job.candidate.item.directory_id;
            hint.dispatched = 0;
        }
        hint.dispatched += 1;
    }

    fn reprioritize_queued(&mut self, shared: &Arc<SharedWork>) {
        let take = |queue: &mut VecDeque<ImageJob>| {
            queue
                .iter()
                .position(|job| {
                    job.shared
                        .as_ref()
                        .is_some_and(|other| Arc::ptr_eq(other, shared))
                        && shared.preferred_lane(job.lane) != job.lane
                })
                .and_then(|index| queue.remove(index))
        };
        let queued = take(&mut self.background_fast)
            .or_else(|| take(&mut self.background_heavy))
            .or_else(|| take(&mut self.background_exception))
            .or_else(|| take(&mut self.viewport_fast))
            .or_else(|| take(&mut self.viewport_heavy));
        if let Some(mut job) = queued {
            job.exception_budget |= job.lane == ThumbnailLane::Exception;
            job.lane = shared.preferred_lane(job.lane);
            self.push(job);
        }
    }

    fn remove_cancelled_viewport_jobs(&mut self) -> Vec<ImageJob> {
        fn remove(queue: &mut VecDeque<ImageJob>, skipped: &mut Vec<ImageJob>) {
            let mut remaining = VecDeque::with_capacity(queue.len());
            while let Some(job) = queue.pop_front() {
                if job.cancelled_before_claim() {
                    skipped.push(job);
                } else {
                    remaining.push_back(job);
                }
            }
            *queue = remaining;
        }
        let mut skipped = Vec::new();
        remove(&mut self.viewport_fast, &mut skipped);
        remove(&mut self.viewport_heavy, &mut skipped);
        skipped
    }

    fn len(&self) -> usize {
        self.viewport_fast.len()
            + self.viewport_heavy.len()
            + self.background_fast.len()
            + self.background_heavy.len()
            + self.background_exception.len()
    }

    fn push(&mut self, job: ImageJob) {
        match job.lane {
            ThumbnailLane::ViewportFast => self.viewport_fast.push_back(job),
            ThumbnailLane::ViewportHeavy => self.viewport_heavy.push_back(job),
            ThumbnailLane::Fast => self.background_fast.push_back(job),
            ThumbnailLane::Heavy => self.background_heavy.push_back(job),
            ThumbnailLane::Exception => self.background_exception.push_back(job),
        }
    }

    fn requeue_front(&mut self, job: ImageJob) {
        match job.lane {
            ThumbnailLane::ViewportFast => self.viewport_fast.push_front(job),
            ThumbnailLane::ViewportHeavy => self.viewport_heavy.push_front(job),
            ThumbnailLane::Fast => self.background_fast.push_front(job),
            ThumbnailLane::Heavy => self.background_heavy.push_front(job),
            ThumbnailLane::Exception => self.background_exception.push_front(job),
        }
    }

    fn has_fast(&self) -> bool {
        !self.viewport_fast.is_empty() || !self.background_fast.is_empty()
    }

    fn can_accept(&self, lane: ThumbnailLane) -> bool {
        if self.len() >= QUEUE_CAPACITY {
            return false;
        }
        match lane {
            ThumbnailLane::ViewportFast | ThumbnailLane::ViewportHeavy => true,
            ThumbnailLane::Fast | ThumbnailLane::Heavy | ThumbnailLane::Exception => {
                self.background_fast.len()
                    + self.background_heavy.len()
                    + self.background_exception.len()
                    < QUEUE_CAPACITY - VIEWPORT_RESERVED_CAPACITY
            }
        }
    }

    fn pop_viewport(&mut self, fast: bool, tail: bool) -> Option<ImageJob> {
        fast.then(|| self.viewport_fast.pop_front())
            .flatten()
            .or_else(|| tail.then(|| self.viewport_heavy.pop_front()).flatten())
    }

    fn pop_background(&mut self, fast: bool, tail: bool) -> Option<ImageJob> {
        fast.then(|| self.background_fast.pop_front())
            .flatten()
            .or_else(|| tail.then(|| self.background_heavy.pop_front()).flatten())
            .or_else(|| {
                tail.then(|| self.background_exception.pop_front())
                    .flatten()
            })
    }

    #[cfg(test)]
    fn pop_next(&mut self, reserved_viewport: bool) -> Option<ImageJob> {
        self.pop_ready(reserved_viewport, true, true)
    }

    fn pop_direct(&mut self, reserved_viewport: bool) -> Option<ImageJob> {
        let direct = |job: &ImageJob| {
            job.kind == ThumbnailTaskKind::Image
                && super::generator::direct_display_reason(&job.candidate.item, &job.config)
                    .is_some()
        };
        let viewport = self.viewport_fast.iter().position(direct);
        let background = (!reserved_viewport)
            .then(|| self.background_fast.iter().position(direct))
            .flatten();
        if super::scheduler::choose_viewport_dispatch(
            viewport.is_some(),
            background.is_some(),
            self.viewport_streak,
        ) {
            self.viewport_streak = self.viewport_streak.saturating_add(1).min(4);
            self.viewport_fast
                .remove(viewport.expect("viewport direct ready"))
        } else if let Some(index) = background {
            self.viewport_streak = 0;
            self.background_fast.remove(index)
        } else {
            None
        }
    }

    fn pop_ready(&mut self, reserved_viewport: bool, fast: bool, tail: bool) -> Option<ImageJob> {
        if reserved_viewport {
            return self.pop_viewport(fast, tail);
        }
        // 全库 Q1 已就绪时，视口重型项不能占走其普通执行席位。
        if fast && self.viewport_fast.is_empty() && !self.background_fast.is_empty() {
            self.viewport_streak = 0;
            return self.background_fast.pop_front();
        }
        let viewport_ready =
            (fast && !self.viewport_fast.is_empty()) || (tail && !self.viewport_heavy.is_empty());
        let background_ready = (fast && !self.background_fast.is_empty())
            || (tail
                && (!self.background_heavy.is_empty() || !self.background_exception.is_empty()));
        if super::scheduler::choose_viewport_dispatch(
            viewport_ready,
            background_ready,
            self.viewport_streak,
        ) {
            self.viewport_streak = self.viewport_streak.saturating_add(1).min(4);
            self.pop_viewport(fast, tail)
        } else if background_ready {
            self.viewport_streak = 0;
            self.pop_background(fast, tail)
        } else {
            None
        }
    }
}

struct WorkQueue {
    queue_wall: LatencyStats,
    e2e_wall: LatencyStats,
    jobs: Mutex<JobQueues>,
    changed: Arc<Condvar>,
}

impl WorkQueue {
    fn finish_job(&self, job: ImageJob, result: Result<ImageExecution>) {
        self.e2e_wall.record(job.queued_at.elapsed());
        if let Some(shared) = job.shared {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            if jobs
                .active
                .get(&shared.key)
                .is_some_and(|current| Arc::ptr_eq(current, &shared))
            {
                jobs.active.remove(&shared.key);
            }
            drop(jobs);
            shared.finish(result);
        } else {
            let _ = job.response.send(result);
        }
    }
}

/// 单次生产者往应用级队列送任务；通道只回流结果，不创建执行线程。
pub struct ImageSubmitter {
    kind: ThumbnailTaskKind,
    queue: Arc<WorkQueue>,
    config: ThumbConfig,
    epoch: u64,
    cancel: CancellationToken,
    response: crossbeam_channel::Sender<Result<ImageExecution>>,
}

impl ImageSubmitter {
    /// 有界提交一个源快照；运行令牌取消时立即停止等待队列空位。
    pub fn send(&self, (candidate, lane): (ThumbnailCandidate, ThumbnailLane)) -> Result<()> {
        self.send_inner((candidate, lane), None)
    }

    /// 视口项在排队期间随订阅撤销；已领取的共享工作继续按租约收口。
    pub fn send_viewport(
        &self,
        (candidate, lane): (ThumbnailCandidate, ThumbnailLane),
        request: &Arc<ViewportRequest>,
    ) -> Result<()> {
        self.send_inner((candidate, lane), Some(Arc::downgrade(request)))
    }

    fn send_inner(
        &self,
        (candidate, lane): (ThumbnailCandidate, ThumbnailLane),
        viewport_request: Option<Weak<ViewportRequest>>,
    ) -> Result<()> {
        let is_cancelled = || {
            self.cancel.is_cancelled()
                || viewport_request.as_ref().is_some_and(|request| {
                    request
                        .upgrade()
                        .is_none_or(|request| !request.is_active(candidate.item.id))
                })
        };
        let key = self.config.output_fingerprint.map(|fp| WorkKey {
            epoch: self.epoch,
            item_id: candidate.item.id,
            source_revision: candidate.item.source_revision,
            kind: self.kind.as_str(),
            output_fingerprint: fp.hex(),
        });
        let subscriber = || WorkSubscriber {
            item_id: candidate.item.id,
            lane,
            cancel: self.cancel.clone(),
            viewport_request: viewport_request.clone(),
            response: self.response.clone(),
        };
        let mut jobs = self.queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if is_cancelled() {
                return Err(AppError::Internal("thumbnail submission cancelled".into()));
            }
            if let Some(shared) = key.as_ref().and_then(|key| jobs.active.get(key)).cloned() {
                shared
                    .subscribers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(subscriber());
                jobs.reprioritize_queued(&shared);
                self.queue.changed.notify_all();
                return Ok(());
            }
            if jobs.can_accept(lane) {
                break;
            }
            jobs = self
                .queue
                .changed
                .wait_timeout(jobs, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        if is_cancelled() {
            return Err(AppError::Internal("thumbnail submission cancelled".into()));
        }
        let shared = key.map(|key| {
            let shared = Arc::new(SharedWork {
                key: key.clone(),
                subscribers: Mutex::new(vec![subscriber()]),
            });
            jobs.active.insert(key, Arc::clone(&shared));
            shared
        });
        jobs.push(ImageJob {
            queued_at: Instant::now(),
            kind: self.kind,
            candidate,
            config: self.config.clone(),
            epoch: self.epoch,
            lane: if viewport_request.is_some() && lane == ThumbnailLane::Exception {
                ThumbnailLane::ViewportHeavy
            } else {
                lane
            },
            exception_budget: lane == ThumbnailLane::Exception,
            cancel: self.cancel.clone(),
            viewport_request,
            response: self.response.clone(),
            shared,
        });
        self.queue.changed.notify_all();
        Ok(())
    }
}

fn release_owned_lease(
    state: &AppState,
    epoch: u64,
    key: &ThumbnailTaskKey,
    lease_id: &str,
) -> Result<()> {
    state
        .with_database_lifecycle_read(epoch, || {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            queries::release_thumbnail_lease(&conn, key, lease_id)
        })
        .transpose()?;
    Ok(())
}

impl ThumbnailCoordinator {
    /// 创建跨入口共享的工作集上限。
    pub fn new(capacity: usize) -> Self {
        #[cfg(windows)]
        let (capacity, native_fast_capacity) = {
            // host 解码/编码/提交与两个原生域合计使用同一处理线程额度。
            let total = capacity.max(4);
            let native_fast = ((total - 2) / 3).clamp(1, 4);
            (total - native_fast - 1, native_fast)
        };
        #[cfg(not(windows))]
        let capacity = capacity.max(2);
        Self {
            capacity,
            run_results: Arc::default(),
            queue: Arc::new(WorkQueue {
                queue_wall: LatencyStats::new(),
                e2e_wall: LatencyStats::new(),
                jobs: Mutex::new(JobQueues::default()),
                changed: Arc::new(Condvar::new()),
            }),
            memory_budget: MemoryBudget::new(),
            volume_io_budget: VolumeIoBudget::new(),
            encode_wall: LatencyStats::new(),
            write_wall: LatencyStats::new(),
            db_transaction_wall: LatencyStats::new(),
            started: OnceLock::new(),
            viewport_requests: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(windows)]
            native: super::native_worker::NativeWorkers::new(native_fast_capacity),
        }
    }

    /// 开始新的全库/增量轮次；每个原生执行域最多允许一次自动重启。
    pub(crate) fn begin_native_run(&self) {
        #[cfg(windows)]
        self.native.begin_run();
    }

    /// 旧派生入口的新原生视频轮次只重置 tail 执行域。
    pub(crate) fn begin_native_video_tail_run(&self) {
        #[cfg(windows)]
        self.native.begin_tail_run();
    }

    /// 当前/峰值共享工作集预留字节及缩略图排队项数；预留是估算准入量。
    pub fn resource_snapshot(&self) -> (u64, u64, usize) {
        let current = *self
            .memory_budget
            .reserved
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let peak = self
            .memory_budget
            .peak
            .load(std::sync::atomic::Ordering::Relaxed);
        let queued = self
            .queue
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len();
        (current, peak, queued)
    }

    pub(crate) fn workset_budget(&self) -> Arc<MemoryBudget> {
        Arc::clone(&self.memory_budget)
    }

    pub(crate) fn volume_io_budget(&self) -> Arc<VolumeIoBudget> {
        Arc::clone(&self.volume_io_budget)
    }

    /// 在固定成员入库前登记，仅接收属于该轮且实际提交成功的结果。
    pub(crate) fn observe_run(&self, run_id: &str, handler: RunResultHandler) -> RunResultObserver {
        self.run_results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(run_id.to_owned(), handler);
        RunResultObserver {
            run_id: run_id.to_owned(),
            results: Arc::clone(&self.run_results),
        }
    }

    pub(crate) fn host_stage_timings(&self) -> HostStageTimings {
        HostStageTimings {
            queue_wall: self.queue.queue_wall.snapshot(),
            e2e_wall: self.queue.e2e_wall.snapshot(),
            encode_wall: self.encode_wall.snapshot(),
            write_wall: self.write_wall.snapshot(),
            db_transaction_wall: self.db_transaction_wall.snapshot(),
        }
    }

    /// 原生子进程的应用生命周期累计结果；非 Windows 平台没有此执行域。
    pub(crate) fn native_counters(&self) -> NativeCounters {
        let (foreground, revision) = super::qos::native_worker_qos_request();
        #[cfg(windows)]
        {
            self.native.counters(foreground, revision)
        }
        #[cfg(not(windows))]
        {
            NativeCounters {
                qos_revision: revision,
                qos_foreground: foreground,
                ..Default::default()
            }
        }
    }

    /// 为一次 IPC 注册逐项回传订阅；登记本身不占有或取消共享任务 lease。
    pub fn register_viewport_request(
        &self,
        request_id: String,
        item_ids: &[i64],
    ) -> Result<Arc<ViewportRequest>> {
        let mut entries = self
            .viewport_requests
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if entries.get(&request_id).and_then(Weak::upgrade).is_some() {
            return Err(AppError::Internal("duplicate viewport request id".into()));
        }
        let request = Arc::new(ViewportRequest {
            id: request_id.clone(),
            active: Mutex::new(item_ids.iter().copied().collect()),
            registry: Arc::downgrade(&self.viewport_requests),
        });
        entries.insert(request_id, Arc::downgrade(&request));
        Ok(request)
    }

    /// 撤销某个前端格子的后端订阅；其他请求方与全库生产者继续使用自己的租约。
    pub fn cancel_viewport_request(&self, request_id: &str, item_ids: &[i64]) {
        let request = self
            .viewport_requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(request_id)
            .and_then(Weak::upgrade);
        if let Some(request) = request {
            request.cancel(item_ids);
            let (detached, skipped) = {
                let mut jobs = self.queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                let shared: Vec<_> = jobs.active.values().cloned().collect();
                let detached: Vec<_> = shared
                    .iter()
                    .flat_map(|shared| shared.detach_inactive())
                    .collect();
                for work in &shared {
                    jobs.reprioritize_queued(work);
                }
                let skipped = jobs.remove_cancelled_viewport_jobs();
                for job in &skipped {
                    if let Some(shared) = &job.shared {
                        jobs.active.remove(&shared.key);
                    }
                }
                if !skipped.is_empty() {
                    self.queue.changed.notify_all();
                }
                (detached, skipped)
            };
            for subscriber in detached {
                subscriber.skip();
            }
            for job in skipped {
                let item_id = job.candidate.item.id;
                self.queue
                    .finish_job(job, Ok(ImageExecution::Skipped(item_id)));
            }
        }
    }

    fn start_workers(&self, state: &Arc<AppState>) {
        self.started.get_or_init(|| {
            let (commit_tx, commit_rx) = crossbeam_channel::bounded(COMMIT_BATCH_SIZE);
            // 两个小后段允许编码/写盘与解码重叠，队列只容纳两张已解码图。
            // 提交器与编码线程均从原处理额度中划出，不额外增加常驻处理线程。
            let encode_count = if self.capacity >= 5 {
                2
            } else if self.capacity >= 3 {
                1
            } else {
                0
            };
            let decode_count = self.capacity - encode_count - 1;
            let (encode_tx, encode_rx) = crossbeam_channel::bounded(encode_count);
            let memory_budget = Arc::clone(&self.memory_budget);
            let queue = Arc::clone(&self.queue);
            let state_ref = Arc::downgrade(state);
            std::thread::spawn(move || Self::commit_loop(queue, state_ref, commit_rx));
            for _ in 0..encode_count {
                let queue = Arc::clone(&self.queue);
                let state = Arc::downgrade(state);
                let commit_tx = commit_tx.clone();
                let encode_rx = encode_rx.clone();
                std::thread::spawn(move || Self::encode_loop(queue, state, encode_rx, commit_tx));
            }
            for index in 0..decode_count {
                let queue = Arc::clone(&self.queue);
                let state = Arc::downgrade(state);
                let commit_tx = commit_tx.clone();
                let encode_tx = (encode_count > 0).then(|| encode_tx.clone());
                let memory_budget = Arc::clone(&memory_budget);
                std::thread::spawn(move || {
                    Self::worker_loop(
                        queue,
                        state,
                        memory_budget,
                        encode_tx,
                        commit_tx,
                        // 单解码线程须兼顾全库；至少两条才可划一条视口专用。
                        index == 0 && decode_count > 1,
                    )
                });
            }
        });
    }

    fn worker_loop(
        queue: Arc<WorkQueue>,
        state: Weak<AppState>,
        memory_budget: Arc<MemoryBudget>,
        encode_tx: Option<crossbeam_channel::Sender<EncodeWork>>,
        commit_tx: crossbeam_channel::Sender<CommitWork>,
        reserved_viewport: bool,
    ) {
        let mut qos_state = super::qos::WorkerQosState::new();
        #[cfg(windows)]
        let mut pending: Vec<NativeWork> = Vec::new();
        loop {
            let Some(active_state) = state.upgrade() else {
                break;
            };
            #[cfg(windows)]
            Self::collect_native(&queue, &active_state, &mut pending, &commit_tx);
            #[cfg(windows)]
            let (fast, tail) = active_state.thumb_coordinator.native.available();
            #[cfg(not(windows))]
            let (fast, tail) = (true, true);
            #[cfg(windows)]
            let wait = if pending.is_empty() {
                Duration::from_millis(250)
            } else {
                Duration::from_millis(25)
            };
            #[cfg(not(windows))]
            let wait = Duration::from_millis(250);
            let job = {
                let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = jobs.pop_direct(reserved_viewport).or_else(|| {
                    jobs.pop_ready(reserved_viewport, fast, tail).map(|job| {
                        jobs.prefer_local_job(job, &active_state.thumb_coordinator.volume_io_budget)
                    })
                }) {
                    queue.changed.notify_all();
                    job
                } else {
                    drop(active_state);
                    // 在途回包和截止由原派发线程收取；等待期间不新建处理线程。
                    let _ = queue
                        .changed
                        .wait_timeout(jobs, wait)
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
            };
            let state = active_state;
            let direct = job.kind == ThumbnailTaskKind::Image
                && super::generator::direct_display_reason(&job.candidate.item, &job.config)
                    .is_some();
            #[cfg(windows)]
            let native_ticket = if direct {
                None
            } else {
                match state.thumb_coordinator.native.try_reserve(job.lane) {
                    Some(ticket) => Some(ticket),
                    None => {
                        queue
                            .jobs
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .requeue_front(job);
                        continue;
                    }
                }
            };
            if job.cancelled_before_claim() {
                let item_id = job.candidate.item.id;
                queue.finish_job(job, Ok(ImageExecution::Skipped(item_id)));
                continue;
            }
            // 像素计算共享 CPU 准入，快速项保留名额；先取额度再领取 DB lease。
            // 视口保留线程不阻塞等额度：让它继续服务后续快速项。
            let heavy_permit = match job.lane {
                _ if direct => None,
                lane if cfg!(windows)
                    || reserved_viewport
                    || matches!(lane, ThumbnailLane::Fast | ThumbnailLane::ViewportFast) =>
                {
                    let permit =
                        if matches!(lane, ThumbnailLane::Fast | ThumbnailLane::ViewportFast) {
                            state.background_heavy_limiter.try_acquire_fast()
                        } else {
                            state.background_heavy_limiter.try_acquire()
                        };
                    match permit {
                        Some(permit) => Some(permit),
                        None => {
                            #[cfg(windows)]
                            drop(native_ticket);
                            let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                            jobs.requeue_front(job);
                            let _ = queue
                                .changed
                                .wait_timeout(jobs, Duration::from_millis(50))
                                .unwrap_or_else(|e| e.into_inner());
                            continue;
                        }
                    }
                }
                _ => {
                    let permit = state.background_heavy_limiter.acquire_cancellable(&|| {
                        job.cancelled_before_claim()
                            || queue
                                .jobs
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .has_fast()
                    });
                    match permit {
                        Some(permit) => Some(permit),
                        None => {
                            if job.cancelled_before_claim() {
                                let item_id = job.candidate.item.id;
                                queue.finish_job(job, Ok(ImageExecution::Skipped(item_id)));
                            } else {
                                let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                                jobs.requeue_front(job);
                                queue.changed.notify_all();
                            }
                            continue;
                        }
                    }
                }
            };
            // 先取得共享工作集额度，再领取 DB lease；不足时退回队列，让快速项有机会前进。
            let Some(memory_permit) = memory_budget.try_reserve(estimated_decode_bytes(&job))
            else {
                #[cfg(windows)]
                drop(native_ticket);
                drop(heavy_permit);
                let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                jobs.requeue_front(job);
                let _ = queue
                    .changed
                    .wait_timeout(jobs, Duration::from_millis(50))
                    .unwrap_or_else(|e| e.into_inner());
                continue;
            };
            // 仅准入源卷读取/解码阶段；提交后段写缓存不占源卷名额。
            let volume_admission = if direct {
                Some(None)
            } else {
                state
                    .thumb_coordinator
                    .volume_io_budget
                    .try_acquire(job.epoch, job.candidate.volume_id)
                    .map(Some)
            };
            let Some(volume_permit) = volume_admission else {
                #[cfg(windows)]
                drop(native_ticket);
                drop(memory_permit);
                drop(heavy_permit);
                let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                // 同卷已满时排到本 lane 后面，让其它卷的任务有机会前进。
                jobs.push(job);
                let _ = queue
                    .changed
                    .wait_timeout(jobs, Duration::from_millis(50))
                    .unwrap_or_else(|e| e.into_inner());
                continue;
            };
            if !direct {
                queue
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .note_source_dispatch(&job, &state.thumb_coordinator.volume_io_budget);
            }
            crate::thumbnail::qos::refresh_worker_qos(&mut qos_state);
            // 所有宿主准入完成才记队列等待，退队重试不会重复累计。
            queue.queue_wall.record(job.queued_at.elapsed());
            let prepared = state.thumb_coordinator.execute_task(
                &state,
                &job,
                &memory_permit,
                #[cfg(windows)]
                native_ticket,
            );
            #[cfg(windows)]
            let prepared = match prepared {
                Ok(TaskPreparation::Pending(request, prepared)) => {
                    pending.push(NativeWork {
                        request,
                        job,
                        prepared,
                        _heavy_permit: heavy_permit,
                        memory_permit,
                        _volume_permit: volume_permit,
                    });
                    continue;
                }
                other => other,
            };
            drop(volume_permit);
            match prepared {
                #[cfg(windows)]
                Ok(TaskPreparation::Pending(..)) => unreachable!("native request transferred"),
                Ok(TaskPreparation::Immediate(result)) => queue.finish_job(job, Ok(result)),
                Ok(TaskPreparation::ToEncode(prepared)) => {
                    let work = EncodeWork {
                        job,
                        prepared,
                        heavy_permit,
                        _memory_permit: memory_permit,
                    };
                    if let Some(tx) = &encode_tx {
                        if let Err(error) = tx.send(work) {
                            let EncodeWork { job, prepared, .. } = error.0;
                            let _ = release_owned_lease(
                                &state,
                                job.epoch,
                                &prepared.key,
                                &prepared.lease_id,
                            );
                            queue.finish_job(
                                job,
                                Err(AppError::Internal("thumbnail encoder unavailable".into())),
                            );
                        }
                    } else {
                        if let Err(error) = commit_tx.send(Self::encode_work(
                            work,
                            &state.thumb_coordinator.encode_wall,
                        )) {
                            let CommitWork { job, prepared, .. } = error.0;
                            let _ = release_owned_lease(
                                &state,
                                job.epoch,
                                &prepared.key,
                                &prepared.lease_id,
                            );
                            queue.finish_job(
                                job,
                                Err(AppError::Internal("thumbnail committer unavailable".into())),
                            );
                        }
                    }
                }
                Ok(TaskPreparation::Ready(prepared)) => {
                    drop(heavy_permit);
                    let mut memory_permit = memory_permit;
                    memory_permit.retain_result(&prepared);
                    if let Err(error) = commit_tx.send(CommitWork {
                        job,
                        prepared,
                        _memory_permit: memory_permit,
                    }) {
                        let CommitWork { job, prepared, .. } = error.0;
                        let _ = release_owned_lease(
                            &state,
                            job.epoch,
                            &prepared.key,
                            &prepared.lease_id,
                        );
                        queue.finish_job(
                            job,
                            Err(AppError::Internal("thumbnail committer unavailable".into())),
                        );
                    }
                }
                Err(error) => queue.finish_job(job, Err(error)),
            }
        }
    }

    #[cfg(windows)]
    fn collect_native(
        queue: &WorkQueue,
        state: &AppState,
        pending: &mut Vec<NativeWork>,
        commit_tx: &crossbeam_channel::Sender<CommitWork>,
    ) {
        let mut index = 0;
        while index < pending.len() {
            let result = state
                .thumb_coordinator
                .native
                .poll(&mut pending[index].request);
            if pending[index].request.source_done() {
                drop(pending[index]._volume_permit.take());
            }
            let Some(result) = result else {
                index += 1;
                continue;
            };
            let NativeWork {
                request,
                job,
                mut prepared,
                mut memory_permit,
                _heavy_permit,
                _volume_permit,
            } = pending.swap_remove(index);
            // 原生源读取/编码已经结束；发布等待只保留结果内存额度。
            drop(request);
            drop(_heavy_permit);
            drop(_volume_permit);
            if !state.is_database_epoch_current(job.epoch) || job.cancelled_before_claim() {
                let _ = release_owned_lease(state, job.epoch, &prepared.key, &prepared.lease_id);
                let item_id = job.candidate.item.id;
                queue.finish_job(job, Ok(ImageExecution::Skipped(item_id)));
                continue;
            }
            match result {
                Ok((payload, execution)) => {
                    prepared.payload = Some(payload);
                    prepared.execution = ExecutionFacts {
                        newly_generated: true,
                        native: Some(execution),
                    };
                }
                Err(error) => {
                    prepared.unavailable_reason = unavailable_reason(&error);
                    prepared.result = Some(failed_result(&prepared.key, &job, &error));
                }
            }
            memory_permit.retain_result(&prepared);
            if let Err(error) = commit_tx.send(CommitWork {
                job,
                prepared,
                _memory_permit: memory_permit,
            }) {
                let CommitWork { job, prepared, .. } = error.0;
                let _ = release_owned_lease(state, job.epoch, &prepared.key, &prepared.lease_id);
                queue.finish_job(
                    job,
                    Err(AppError::Internal("thumbnail committer unavailable".into())),
                );
            }
        }
    }

    fn encode_loop(
        queue: Arc<WorkQueue>,
        state: Weak<AppState>,
        rx: crossbeam_channel::Receiver<EncodeWork>,
        commit_tx: crossbeam_channel::Sender<CommitWork>,
    ) {
        let mut qos_state = super::qos::WorkerQosState::new();
        while let Ok(work) = rx.recv() {
            let Some(state) = state.upgrade() else {
                break;
            };
            crate::thumbnail::qos::refresh_worker_qos(&mut qos_state);
            let work = Self::encode_work(work, &state.thumb_coordinator.encode_wall);
            if let Err(error) = commit_tx.send(work) {
                let CommitWork { job, prepared, .. } = error.0;
                let _ = release_owned_lease(&state, job.epoch, &prepared.key, &prepared.lease_id);
                queue.finish_job(
                    job,
                    Err(AppError::Internal("thumbnail committer unavailable".into())),
                );
            }
        }
    }

    fn encode_work(work: EncodeWork, timing: &LatencyStats) -> CommitWork {
        let EncodeWork {
            job,
            prepared,
            heavy_permit: _heavy_permit,
            _memory_permit: mut memory_permit,
        } = work;
        let EncodePreparation {
            key,
            lease_id,
            exception_lane,
            decoded,
            encode_size,
        } = prepared;
        // 编码只处理像素，不持切库读锁；提交器按 epoch 丢弃迟到的字节结果。
        let mut config = job.config.clone();
        config.size = encode_size;
        let emit_ai_cache = config.ai_hq_cache
            && !super::cache::ai_cache_path(&config.cache_dir, job.candidate.item.cache_key)
                .exists();
        let encode_started = Instant::now();
        let result = encode_media_payload(key.item_id, decoded, &config, emit_ai_cache);
        timing.record(encode_started.elapsed());
        let (result, payload) = match result {
            Ok(payload) => (None, Some(payload)),
            Err(error) => (Some(failed_result(&key, &job, &error)), None),
        };
        let prepared = PreparedResult {
            result,
            payload,
            unavailable_reason: None,
            execution: ExecutionFacts {
                newly_generated: true,
                native: None,
            },
            write_size: encode_size,
            key,
            lease_id,
            exception_lane,
        };
        memory_permit.retain_result(&prepared);
        CommitWork {
            job,
            prepared,
            _memory_permit: memory_permit,
        }
    }

    fn commit_loop(
        queue: Arc<WorkQueue>,
        state: Weak<AppState>,
        rx: crossbeam_channel::Receiver<CommitWork>,
    ) {
        while let Ok(first) = rx.recv() {
            let mut by_epoch: BTreeMap<u64, Vec<CommitWork>> = BTreeMap::new();
            for work in collect_commit_batch(first, &rx) {
                by_epoch.entry(work.job.epoch).or_default().push(work);
            }
            for (epoch, group) in by_epoch {
                let Some(state) = state.upgrade() else {
                    for work in group {
                        let item_id = work.job.candidate.item.id;
                        queue.finish_job(work.job, Ok(ImageExecution::Skipped(item_id)));
                    }
                    continue;
                };
                Self::commit_group(&queue, &state, epoch, group);
            }
        }
    }

    fn commit_group(queue: &WorkQueue, state: &AppState, epoch: u64, mut group: Vec<CommitWork>) {
        let committed =
            state.with_database_lifecycle_read(epoch, || -> Result<Vec<ImageExecution>> {
                // 文件 IO 在 DB 写事务之前完成，避免慢盘延长写锁占用。
                for work in &mut group {
                    if work.job.cancelled_before_claim() {
                        continue;
                    }
                    if let Some(payload) = work.prepared.payload.take() {
                        let mut config = work.job.config.clone();
                        config.size = work.prepared.write_size;
                        let write_started = Instant::now();
                        let result = write_encoded_media_payload(
                            work.prepared.key.item_id,
                            work.prepared.key.source_revision,
                            work.job.candidate.item.cache_key,
                            payload,
                            &config,
                        );
                        state
                            .thumb_coordinator
                            .write_wall
                            .record(write_started.elapsed());
                        work.prepared.result = Some(result.unwrap_or_else(|error| {
                            failed_result(&work.prepared.key, &work.job, &error)
                        }));
                    }
                }
                // 包含等待写锁与 SQLite 事务，截止于提交并释放连接；失败路径由 RAII 记录。
                let db_timer = LatencyTimer::new(&state.thumb_coordinator.db_transaction_wall);
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                let tx = conn.unchecked_transaction()?;
                let mut outcomes = Vec::with_capacity(group.len());
                let mut run_ids = Vec::with_capacity(group.len());
                for work in &group {
                    let CommitWork { job, prepared, .. } = work;
                    let key = &prepared.key;
                    let lease_id = &prepared.lease_id;
                    let exception_lane = prepared.exception_lane;
                    let item_id = key.item_id;
                    run_ids.push(queries::thumbnail_lease_run_id(&tx, key, lease_id)?);
                    if job.cancelled_before_claim() {
                        queries::release_thumbnail_lease(&tx, key, lease_id)?;
                        outcomes.push(ImageExecution::Skipped(item_id));
                        continue;
                    }
                    let result = prepared
                        .result
                        .as_ref()
                        .expect("active thumbnail result must be finalized before DB commit");
                    if result.thumb_status == 2
                        && job.lane != ThumbnailLane::Exception
                        && !exception_lane
                    {
                        let deferred = queries::defer_thumbnail_failure(
                            &tx,
                            key,
                            lease_id,
                            prepared.unavailable_reason.unwrap_or("decode_or_encode"),
                        )?;
                        outcomes.push(if deferred {
                            ImageExecution::Deferred(item_id)
                        } else {
                            ImageExecution::Skipped(item_id)
                        });
                        continue;
                    }
                    if let Some(reason) = prepared.unavailable_reason {
                        outcomes.push(
                            if queries::finish_thumbnail_unavailable(&tx, key, lease_id, reason)? {
                                ImageExecution::Unavailable(item_id)
                            } else {
                                ImageExecution::Skipped(item_id)
                            },
                        );
                        continue;
                    }
                    let current_fp = {
                        let current = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
                        let mut current = current.clone();
                        current.size = job.config.size;
                        match job.kind {
                            ThumbnailTaskKind::Image => OutputFingerprint::for_image(&current),
                            ThumbnailTaskKind::VideoCover => {
                                OutputFingerprint::for_native_video_cover(&current)
                            }
                        }
                    };
                    let publish = job
                        .config
                        .output_fingerprint
                        .is_some_and(|fp| fp == current_fp);
                    let finished = queries::finish_thumbnail_task_in_transaction(
                        &tx, key, lease_id, result, publish,
                    )?;
                    outcomes.push(if finished.display_updated {
                        ImageExecution::Published(result.clone(), prepared.execution)
                    } else {
                        ImageExecution::Skipped(item_id)
                    });
                }
                // 提交可见前持有观察者锁；阶段结束的同步点因此一定看到已提交的计数。
                // 回调只更新内存统计，不执行DB或IPC；事务失败不会分发任何完成事实。
                let observers = state
                    .thumb_coordinator
                    .run_results
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                tx.commit()?;
                for (run_id, outcome) in run_ids.iter().zip(&outcomes) {
                    if matches!(
                        outcome,
                        ImageExecution::Published(..) | ImageExecution::Unavailable(_)
                    ) {
                        if let Some(handler) = run_id.as_ref().and_then(|id| observers.get(id)) {
                            handler(outcome);
                        }
                    }
                }
                drop(observers);
                drop(conn);
                drop(db_timer);
                let published: Vec<_> = outcomes
                    .iter()
                    .filter_map(|outcome| match outcome {
                        ImageExecution::Published(result, _) => Some(result.clone()),
                        _ => None,
                    })
                    .collect();
                state.apply_thumb_results(&published);
                Ok(outcomes)
            });
        match committed.transpose() {
            Ok(Some(outcomes)) => {
                for (work, outcome) in group.into_iter().zip(outcomes) {
                    queue.finish_job(work.job, Ok(outcome));
                }
            }
            Ok(None) => {
                for work in group {
                    let item_id = work.job.candidate.item.id;
                    queue.finish_job(work.job, Ok(ImageExecution::Skipped(item_id)));
                }
            }
            Err(error) => {
                let mut original = Some(error);
                for work in group {
                    let _ = release_owned_lease(
                        state,
                        epoch,
                        &work.prepared.key,
                        &work.prepared.lease_id,
                    );
                    queue.finish_job(
                        work.job,
                        Err(original.take().unwrap_or_else(|| {
                            AppError::Internal("thumbnail commit batch failed".into())
                        })),
                    );
                }
            }
        }
    }

    /// 有界流式派发。生产者逐页送入候选，worker 与分页同时推进，所有入口共用执行额度。
    pub fn run_images<P, F>(
        &self,
        state: &Arc<AppState>,
        epoch: u64,
        config: &ThumbConfig,
        cancel: &CancellationToken,
        producer: P,
        on_result: F,
    ) -> Result<()>
    where
        P: FnOnce(&ImageSubmitter) -> Result<()>,
        F: Fn(ImageExecution) + Sync,
    {
        self.run_tasks(
            state,
            epoch,
            RunOptions {
                config,
                kind: ThumbnailTaskKind::Image,
                cancel,
            },
            producer,
            on_result,
        )
    }

    /// 原生视频第一帧与图片共用同一有界队列及 lease 执行额度。
    pub fn run_native_video_covers<P, F>(
        &self,
        state: &Arc<AppState>,
        epoch: u64,
        config: &ThumbConfig,
        cancel: &CancellationToken,
        producer: P,
        on_result: F,
    ) -> Result<()>
    where
        P: FnOnce(&ImageSubmitter) -> Result<()>,
        F: Fn(ImageExecution) + Sync,
    {
        self.run_tasks(
            state,
            epoch,
            RunOptions {
                config,
                kind: ThumbnailTaskKind::VideoCover,
                cancel,
            },
            producer,
            on_result,
        )
    }

    /// 按固定轮次分页执行一段原生视频封面尾批，供全库和派生入口共用。
    pub fn run_background_video_cover_phase<F>(
        &self,
        state: &Arc<AppState>,
        epoch: u64,
        config: &ThumbConfig,
        phase: VideoCoverPhase<'_>,
        cancel: &CancellationToken,
        on_result: &F,
    ) -> Result<()>
    where
        F: Fn(ImageExecution) + Sync,
    {
        let VideoCoverPhase {
            run_id,
            lane,
            index: phase_index,
        } = phase;
        loop {
            if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
                return Ok(());
            }
            let counts = {
                let conn = state.db_read_pool.get().map_err(AppError::from)?;
                queries::native_video_cover_run_open_counts(&conn, run_id)?
            };
            if counts[phase_index] == 0 {
                return Ok(());
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| AppError::Internal("system clock before Unix epoch".into()))?
                .as_secs() as i64;
            let ready = {
                let conn = state.db_read_pool.get().map_err(AppError::from)?;
                !queries::native_video_cover_lane_page(&conn, run_id, lane, 0, 1, now)?.is_empty()
            };
            if !ready {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            self.run_native_video_covers(
                state,
                epoch,
                config,
                cancel,
                |tx| {
                    let mut cursor = 0;
                    loop {
                        if cancel.is_cancelled() || !state.is_database_epoch_current(epoch) {
                            break;
                        }
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_err(|_| {
                                AppError::Internal("system clock before Unix epoch".into())
                            })?
                            .as_secs() as i64;
                        let page = {
                            let conn = state.db_read_pool.get().map_err(AppError::from)?;
                            queries::native_video_cover_lane_page(
                                &conn, run_id, lane, cursor, 256, now,
                            )?
                        };
                        if page.is_empty() {
                            break;
                        }
                        for candidate in page {
                            cursor = candidate.item.id;
                            if tx.send((candidate, lane)).is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Ok(())
                },
                on_result,
            )?;
        }
    }

    fn run_tasks<P, F>(
        &self,
        state: &Arc<AppState>,
        epoch: u64,
        options: RunOptions<'_>,
        producer: P,
        on_result: F,
    ) -> Result<()>
    where
        P: FnOnce(&ImageSubmitter) -> Result<()>,
        F: Fn(ImageExecution) + Sync,
    {
        let RunOptions {
            config,
            kind,
            cancel,
        } = options;
        self.start_workers(state);
        let (tx, rx) = crossbeam_channel::unbounded::<Result<ImageExecution>>();
        let submitter = ImageSubmitter {
            kind,
            queue: Arc::clone(&self.queue),
            config: config.clone(),
            epoch,
            cancel: cancel.clone(),
            response: tx,
        };
        let first_error = Mutex::new(None);
        let produced = std::thread::scope(|scope| {
            scope.spawn(|| {
                while let Ok(result) = rx.recv() {
                    match result {
                        Ok(outcome) => on_result(outcome),
                        Err(error) => {
                            let mut first = first_error.lock().unwrap_or_else(|e| e.into_inner());
                            if first.is_none() {
                                *first = Some(error);
                            }
                            cancel.cancel();
                        }
                    }
                }
            });
            let result = producer(&submitter);
            drop(submitter);
            result
        });
        produced?;
        if let Some(error) = first_error.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(error);
        }
        Ok(())
    }

    /// 对单个源快照领取并生成结果；条件提交交给独立短事务提交器。
    fn execute_task(
        &self,
        state: &AppState,
        job: &ImageJob,
        _memory_permit: &MemoryPermit,
        #[cfg(windows)] native_ticket: Option<super::native_worker::DomainTicket>,
    ) -> Result<TaskPreparation> {
        let ImageJob {
            candidate,
            config,
            epoch,
            kind,
            ..
        } = job;
        let epoch = *epoch;
        let kind = *kind;
        if job.cancelled_before_claim() {
            return Ok(TaskPreparation::Immediate(ImageExecution::Skipped(
                candidate.item.id,
            )));
        }
        let fp = config
            .output_fingerprint
            .ok_or_else(|| AppError::Internal("coordinator output fingerprint missing".into()))?;
        let key = ThumbnailTaskKey {
            item_id: candidate.item.id,
            source_revision: candidate.item.source_revision,
            kind,
            output_fingerprint: fp.hex(),
        };
        let lease_id = unique_id()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AppError::Internal("system clock before Unix epoch".into()))?
            .as_secs() as i64;
        // 租期覆盖在途处理和提交背压；Windows 原生执行另有可终止的域截止。
        let claimed = state.with_database_lifecycle_read(epoch, || -> Result<Option<bool>> {
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            if queries::claim_thumbnail_task(&conn, &key, &lease_id, now, now + 3600)? {
                Ok(Some(queries::is_thumbnail_exception(&conn, &key)?))
            } else {
                Ok(None)
            }
        });
        let Some(exception_lane) = claimed.transpose()?.flatten() else {
            return Ok(TaskPreparation::Immediate(ImageExecution::Skipped(
                candidate.item.id,
            )));
        };
        if job.cancelled_before_claim() {
            release_owned_lease(state, epoch, &key, &lease_id)?;
            return Ok(TaskPreparation::Immediate(ImageExecution::Skipped(
                candidate.item.id,
            )));
        }

        // 原生同步操作由子进程截止终止；切库锁只用于租约和结果身份检查。
        #[cfg(windows)]
        let (qos_foreground, qos_revision) = super::qos::native_worker_qos_request();
        let result = match kind {
            ThumbnailTaskKind::Image => {
                #[cfg(windows)]
                {
                    if let Some(ready) = super::generator::preflight_media_step(
                        &candidate.item,
                        &candidate.abs_path,
                        config,
                    ) {
                        Ok((DecodeResult::Ready(ready), false))
                    } else {
                        let need_ai_cache =
                            super::generator::needs_ai_cache(config, candidate.item.cache_key);
                        let request = super::native_protocol::NativeRequest {
                            id: 0,
                            file_handle: 0,
                            kind: super::native_protocol::NativeKind::Image,
                            format: candidate.item.file_format.clone(),
                            codec_hint: None,
                            prefer_gpu: config.strategy == "gpu",
                            gpu_policy: config.strategy == "gpu",
                            decode_long_edge: super::generator::decode_long_edge(
                                config,
                                &candidate.item,
                                need_ai_cache,
                            )
                            .min(8192),
                            max_pixel_bytes: _memory_permit.pixel_limit(),
                            output_size: config.size,
                            webp_quality: config.webp_quality,
                            ai_cache_short_edge: config.ai_cache_short_edge,
                            emit_ai_cache: need_ai_cache,
                            qos_foreground,
                            qos_revision,
                        };
                        match self.native.start(
                            native_ticket.expect("native domain reserved"),
                            job.lane,
                            candidate,
                            request,
                            Arc::clone(&self.queue.changed),
                            self.volume_io_budget
                                .media_for(job.epoch, candidate.volume_id),
                        ) {
                            Ok(request) => {
                                return Ok(TaskPreparation::Pending(
                                    Box::new(request),
                                    PreparedResult {
                                        key,
                                        lease_id,
                                        result: None,
                                        payload: None,
                                        execution: ExecutionFacts::default(),
                                        unavailable_reason: None,
                                        write_size: config.size,
                                        exception_lane,
                                    },
                                ))
                            }
                            Err(error) => Err(error),
                        }
                    }
                }
                #[cfg(not(windows))]
                match decode_media_step(
                    &candidate.item,
                    &candidate.abs_path,
                    &state.engine_arena,
                    config,
                    _memory_permit.pixel_limit(),
                ) {
                    Ok(ready @ DecodeResult::Ready(_)) => Ok((ready, false)),
                    Ok(encoded @ DecodeResult::Encoded(_)) => Ok((encoded, false)),
                    Ok(to_encode @ DecodeResult::ToEncode { .. }) => Ok((to_encode, false)),
                    Ok(DecodeResult::DeferredToCpu { item, abs_path }) => decode_deferred_cpu(
                        &item,
                        &abs_path,
                        &state.engine_arena,
                        config,
                        _memory_permit.pixel_limit(),
                    )
                    .map(|decoded| (decoded, true)),
                    Err(error) => Err(error),
                }
            }
            ThumbnailTaskKind::VideoCover => {
                #[cfg(windows)]
                {
                    let request = super::native_protocol::NativeRequest {
                        id: 0,
                        file_handle: 0,
                        kind: super::native_protocol::NativeKind::VideoCover,
                        format: candidate.item.file_format.clone(),
                        codec_hint: candidate
                            .video_codec
                            .clone()
                            .filter(|hint| hint.len() <= 64),
                        prefer_gpu: config.strategy != "cpu",
                        gpu_policy: false,
                        decode_long_edge: config.size,
                        max_pixel_bytes: _memory_permit.pixel_limit(),
                        output_size: config.size,
                        webp_quality: config.webp_quality,
                        ai_cache_short_edge: config.ai_cache_short_edge,
                        emit_ai_cache: false,
                        qos_foreground,
                        qos_revision,
                    };
                    match self.native.start(
                        native_ticket.expect("native domain reserved"),
                        job.lane,
                        candidate,
                        request,
                        Arc::clone(&self.queue.changed),
                        self.volume_io_budget
                            .media_for(job.epoch, candidate.volume_id),
                    ) {
                        Ok(request) => {
                            return Ok(TaskPreparation::Pending(
                                Box::new(request),
                                PreparedResult {
                                    key,
                                    lease_id,
                                    result: None,
                                    payload: None,
                                    execution: ExecutionFacts::default(),
                                    unavailable_reason: None,
                                    write_size: config.size,
                                    exception_lane,
                                },
                            ))
                        }
                        Err(error) => Err(error),
                    }
                }
                #[cfg(not(windows))]
                super::generator::panic_guard("native_video_cover", || {
                    let backend =
                        crate::video::native_cover_backend_for(&candidate.item.file_format)
                            .ok_or_else(|| {
                                AppError::UnsupportedFormat(candidate.item.file_format.clone())
                            })?;
                    let decoded = backend.cover_with_codec_hint(
                        &candidate.abs_path,
                        config.size,
                        candidate.video_codec.as_deref(),
                    )?;
                    Ok((
                        DecodeResult::ToEncode {
                            item_id: candidate.item.id,
                            source_revision: candidate.item.source_revision,
                            cache_key: candidate.item.cache_key,
                            decoded,
                        },
                        false,
                    ))
                })
            }
        };
        if state.with_database_lifecycle_read(epoch, || ()).is_none() {
            return Ok(TaskPreparation::Immediate(ImageExecution::Skipped(
                candidate.item.id,
            )));
        }
        if job.cancelled_before_claim() {
            release_owned_lease(state, epoch, &key, &lease_id)?;
            return Ok(TaskPreparation::Immediate(ImageExecution::Skipped(
                candidate.item.id,
            )));
        }
        match result {
            Ok((DecodeResult::ToEncode { decoded, .. }, snapped)) => {
                Ok(TaskPreparation::ToEncode(EncodePreparation {
                    key,
                    lease_id,
                    exception_lane,
                    decoded,
                    encode_size: if snapped {
                        snap_to_tier(config.size)
                    } else {
                        config.size
                    },
                }))
            }
            Ok((DecodeResult::Ready(result), _)) => Ok(TaskPreparation::Ready(PreparedResult {
                key,
                lease_id,
                result: Some(result),
                payload: None,
                execution: ExecutionFacts::default(),
                unavailable_reason: None,
                write_size: config.size,
                exception_lane,
            })),
            Ok((DecodeResult::Encoded(payload), snapped)) => {
                Ok(TaskPreparation::Ready(PreparedResult {
                    key,
                    lease_id,
                    result: None,
                    payload: Some(payload),
                    unavailable_reason: None,
                    execution: ExecutionFacts {
                        newly_generated: true,
                        native: None,
                    },
                    write_size: if snapped {
                        snap_to_tier(config.size)
                    } else {
                        config.size
                    },
                    exception_lane,
                }))
            }
            Ok((DecodeResult::DeferredToCpu { .. }, _)) => unreachable!("CPU fallback must decode"),
            Err(error) => Ok(TaskPreparation::Ready(PreparedResult {
                result: Some(failed_result(&key, job, &error)),
                unavailable_reason: unavailable_reason(&error),
                payload: None,
                execution: ExecutionFacts::default(),
                write_size: config.size,
                key,
                lease_id,
                exception_lane,
            })),
        }
    }
}

fn unavailable_reason(error: &AppError) -> Option<&'static str> {
    match error {
        AppError::ThumbnailUnavailable(reason) => Some(reason),
        _ => None,
    }
}

fn failed_result(key: &ThumbnailTaskKey, job: &ImageJob, error: &AppError) -> ThumbResult {
    if let Some(reason) = unavailable_reason(error) {
        // 故障域关闭可能影响整批，逐项仅 DEBUG；INFO 由本轮计数汇总。
        tracing::debug!(target: "scrollery::thumb_perf", item_id = key.item_id, reason,
            "thumbnail temporarily unavailable");
    } else {
        tracing::warn!(
        target: "scrollery::pipeline::thumb",
        item_id = key.item_id,
        error = %error,
        "thumbnail task failed"
        );
    }
    ThumbResult {
        item_id: key.item_id,
        thumb_status: 2,
        thumb_path: None,
        thumbhash: None,
        source_revision: key.source_revision,
        cache_key: job.candidate.item.cache_key,
    }
}

/// 运行/租约 ID 跨重启唯一，避免旧 worker 的迟到写误认新轮。
pub fn unique_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| AppError::Internal("thumbnail random ID unavailable".into()))?;
    let mut id = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut id, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_io_budget_shares_hdd_partitions_and_unknown_devices() {
        let budget = VolumeIoBudget::new();
        let hdd = StorageDevice {
            number: 2,
            incurs_seek_penalty: true,
        };
        let ssd = StorageDevice {
            number: 3,
            incurs_seek_penalty: false,
        };
        budget.publish_devices(1, HashMap::from([(7, hdd), (8, hdd), (9, ssd)]));
        let held = budget.try_acquire(1, Some(7)).unwrap();
        assert!(budget.try_acquire(1, Some(8)).is_none());
        assert!(budget.try_acquire(1, Some(9)).is_some());
        let unknown = budget.try_acquire(1, Some(10)).unwrap();
        assert!(budget.try_acquire(1, Some(11)).is_none());
        assert!(budget.try_acquire(1, None).is_none());
        drop(held);
        assert!(budget.try_acquire(1, Some(8)).is_some());
        drop(unknown);
        assert_eq!(budget.snapshot().current, 0);
    }

    #[test]
    fn decoded_workset_budget_recovers_after_encoder_releases_result() {
        let budget = MemoryBudget::new();
        let mut first = budget.try_reserve(MAX_SINGLE_RESERVATION_BYTES).unwrap();
        let second = budget.try_reserve(MAX_SINGLE_RESERVATION_BYTES).unwrap();
        assert!(budget.try_reserve(4 * 1024 * 1024).is_none());
        assert_eq!(
            budget.peak.load(std::sync::atomic::Ordering::Relaxed),
            MAX_INFLIGHT_DECODE_BYTES
        );
        first.shrink_to(1024 * 1024);
        assert!(budget
            .try_reserve(MAX_SINGLE_RESERVATION_BYTES - 1024 * 1024)
            .is_some());
        assert_eq!(
            *budget.reserved.lock().unwrap(),
            MAX_SINGLE_RESERVATION_BYTES + 1024 * 1024
        );
        // 收缩后 Drop 只归还保留部分，不能重复扣掉已让出的解码额度。
        drop(first);
        assert_eq!(
            *budget.reserved.lock().unwrap(),
            MAX_SINGLE_RESERVATION_BYTES
        );
        drop(second);
        assert_eq!(*budget.reserved.lock().unwrap(), 0);
    }

    #[test]
    fn shared_workset_wait_does_not_hold_heavy_permit() {
        let budget = MemoryBudget::new();
        let held = budget.try_reserve(MAX_INFLIGHT_DECODE_BYTES).unwrap();
        let heavy = crate::exotic::limiter::BackgroundHeavyLimiter::new(1);
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        let waiting_budget = Arc::clone(&budget);
        let waiting_heavy = Arc::clone(&heavy);
        let waiter = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            waiting_budget.acquire_with_heavy(&waiting_heavy, 128 * 1024 * 1024, &|| false)
        });
        ready_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(heavy.available(), 1);
        drop(held);
        let pair = waiter.join().unwrap().unwrap();
        assert_eq!(heavy.available(), 0);
        drop(pair);
        assert_eq!(heavy.available(), 1);
        assert_eq!(*budget.reserved.lock().unwrap(), 0);
    }

    fn image_candidate_and_config() -> (ThumbnailCandidate, ThumbConfig) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, rel_path, name) VALUES (1, 1, '', 'R');
             INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                 file_format, media_type, sort_datetime, cache_key)
             VALUES (1, 1, 'a.jpg', 100, 1, 'jpg', 'image', 1, 91);",
        )
        .unwrap();
        let candidate = queries::image_thumbnail_candidates_for_ids(&conn, &[1])
            .unwrap()
            .remove(0);
        let config = ThumbConfig {
            cache_dir: std::path::PathBuf::new(),
            size: 256,
            skip_max_bytes: 0,
            strategy: "cpu".into(),
            ai_hq_cache: false,
            webp_quality: 80,
            ai_cache_short_edge: 336,
            output_fingerprint: None,
        };
        (candidate, config)
    }

    #[test]
    fn viewport_exception_keeps_recovery_budget() {
        let (mut candidate, config) = image_candidate_and_config();
        candidate.item.width = 64;
        candidate.item.height = 64;
        let (response, _) = crossbeam_channel::unbounded();
        let mut job = ImageJob {
            queued_at: Instant::now(),
            kind: ThumbnailTaskKind::Image,
            candidate,
            config,
            epoch: 1,
            lane: ThumbnailLane::Exception,
            exception_budget: true,
            cancel: CancellationToken::new(),
            viewport_request: None,
            response,
            shared: None,
        };
        assert_eq!(estimated_decode_bytes(&job), UNCERTAIN_RESERVATION_BYTES);
        job.lane = ThumbnailLane::ViewportHeavy;
        assert_eq!(estimated_decode_bytes(&job), UNCERTAIN_RESERVATION_BYTES);
        job.exception_budget = false;
        assert!(estimated_decode_bytes(&job) < UNCERTAIN_RESERVATION_BYTES);
    }

    #[test]
    fn existing_ai_cache_keeps_display_decode_size() {
        let (mut candidate, mut config) = image_candidate_and_config();
        let cache = tempfile::tempdir().unwrap();
        config.cache_dir = cache.path().to_path_buf();
        config.size = 512;
        config.ai_hq_cache = true;
        candidate.item.width = 6000;
        candidate.item.height = 500;
        let need = super::super::generator::needs_ai_cache(&config, candidate.item.cache_key);
        assert!(need);
        assert_eq!(
            super::super::generator::decode_long_edge(&config, &candidate.item, need),
            4032
        );
        let path = super::super::cache::ai_cache_path(&config.cache_dir, candidate.item.cache_key);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"existing cache").unwrap();
        let need = super::super::generator::needs_ai_cache(&config, candidate.item.cache_key);
        assert!(!need);
        assert_eq!(
            super::super::generator::decode_long_edge(&config, &candidate.item, need),
            512
        );
    }

    #[test]
    fn domain_backpressure_preserves_other_domain_and_direct_progress() {
        let (candidate, config) = image_candidate_and_config();
        let (response, _replies) = crossbeam_channel::unbounded();
        let job = |lane| ImageJob {
            queued_at: Instant::now(),
            kind: ThumbnailTaskKind::Image,
            candidate: candidate.clone(),
            config: config.clone(),
            epoch: 1,
            lane,
            exception_budget: lane == ThumbnailLane::Exception,
            cancel: CancellationToken::new(),
            viewport_request: None,
            response: response.clone(),
            shared: None,
        };
        let mut queues = JobQueues::default();
        queues.push(job(ThumbnailLane::ViewportHeavy));
        queues.push(job(ThumbnailLane::Fast));
        assert_eq!(
            queues.pop_ready(false, true, false).unwrap().lane,
            ThumbnailLane::Fast
        );
        queues.push(job(ThumbnailLane::ViewportFast));
        assert_eq!(
            queues.pop_ready(false, false, true).unwrap().lane,
            ThumbnailLane::ViewportHeavy
        );
        assert!(queues.pop_ready(false, false, false).is_none());
        let mut direct = job(ThumbnailLane::Fast);
        direct.config.strategy = "direct".into();
        queues.push(direct);
        assert!(queues.pop_direct(true).is_none());
        assert_eq!(queues.pop_direct(false).unwrap().lane, ThumbnailLane::Fast);
        assert_eq!(
            queues.pop_ready(true, true, false).unwrap().lane,
            ThumbnailLane::ViewportFast
        );
    }

    #[test]
    fn shared_key_keeps_one_job_and_survives_single_request_cancellation() {
        let (candidate, mut config) = image_candidate_and_config();
        config.output_fingerprint = Some(OutputFingerprint::for_image(&config));
        let coordinator = ThumbnailCoordinator::new(2);
        let first = coordinator
            .register_viewport_request("shared-first".into(), &[1])
            .unwrap();
        let second = coordinator
            .register_viewport_request("shared-second".into(), &[1])
            .unwrap();
        let (first_tx, first_rx) = crossbeam_channel::unbounded();
        let (second_tx, second_rx) = crossbeam_channel::unbounded();
        let submitter = |response| ImageSubmitter {
            kind: ThumbnailTaskKind::Image,
            queue: Arc::clone(&coordinator.queue),
            config: config.clone(),
            epoch: 1,
            cancel: CancellationToken::new(),
            response,
        };
        submitter(first_tx)
            .send_viewport((candidate.clone(), ThumbnailLane::ViewportFast), &first)
            .unwrap();
        submitter(second_tx)
            .send_viewport((candidate.clone(), ThumbnailLane::ViewportFast), &second)
            .unwrap();
        {
            let jobs = coordinator.queue.jobs.lock().unwrap();
            assert_eq!(jobs.len(), 1, "同键请求只占一个队列位置");
            assert_eq!(jobs.viewport_fast.len(), 1);
        }
        coordinator.cancel_viewport_request("shared-first", &[1]);
        assert!(matches!(
            first_rx.try_recv().unwrap(),
            Ok(ImageExecution::Skipped(1))
        ));
        let job = coordinator
            .queue
            .jobs
            .lock()
            .unwrap()
            .pop_next(true)
            .unwrap();
        assert!(!job.cancelled_before_claim());
        assert_eq!(coordinator.host_stage_timings().e2e_wall.count, 0);
        let result = ThumbResult {
            item_id: 1,
            thumb_status: 1,
            thumb_path: Some("ready.webp".into()),
            thumbhash: None,
            source_revision: candidate.item.source_revision,
            cache_key: candidate.item.cache_key,
        };
        let execution = ExecutionFacts {
            newly_generated: true,
            native: Some(super::super::native_protocol::NativeExecution::cpu(
                super::super::native_protocol::NativeBackend::ImageWic,
            )),
        };
        coordinator.queue.finish_job(
            job,
            Ok(ImageExecution::Published(result.clone(), execution)),
        );
        assert!(
            matches!(second_rx.try_recv().unwrap(), Ok(ImageExecution::Published(value, facts)) if value.thumb_path == result.thumb_path && facts == execution)
        );
        assert!(coordinator.queue.jobs.lock().unwrap().active.is_empty());
        assert_eq!(coordinator.host_stage_timings().e2e_wall.count, 1);

        let third = coordinator
            .register_viewport_request("shared-third".into(), &[1])
            .unwrap();
        let background_cancel = CancellationToken::new();
        let (background_tx, background_rx) = crossbeam_channel::unbounded();
        let (third_tx, third_rx) = crossbeam_channel::unbounded();
        let background = ImageSubmitter {
            kind: ThumbnailTaskKind::Image,
            queue: Arc::clone(&coordinator.queue),
            config: config.clone(),
            epoch: 1,
            cancel: background_cancel.clone(),
            response: background_tx,
        };
        background
            .send((candidate.clone(), ThumbnailLane::Fast))
            .unwrap();
        submitter(third_tx)
            .send_viewport((candidate.clone(), ThumbnailLane::ViewportFast), &third)
            .unwrap();
        background_cancel.cancel();
        let mut jobs = coordinator.queue.jobs.lock().unwrap();
        assert_eq!(jobs.viewport_fast.len(), 1, "视口命中应提升后台排队项");
        let job = jobs.pop_next(true).unwrap();
        drop(jobs);
        assert!(!job.cancelled_before_claim(), "后台取消不能丢弃视口需求");
        coordinator
            .queue
            .finish_job(job, Ok(ImageExecution::Published(result, execution)));
        assert!(matches!(
            background_rx.try_recv().unwrap(),
            Ok(ImageExecution::Skipped(1))
        ));
        assert!(matches!(
            third_rx.try_recv().unwrap(),
            Ok(ImageExecution::Published(_, facts)) if facts == execution
        ));

        let fourth = coordinator
            .register_viewport_request("shared-fourth".into(), &[1])
            .unwrap();
        let (fourth_tx, fourth_rx) = crossbeam_channel::unbounded();
        submitter(fourth_tx)
            .send_viewport((candidate, ThumbnailLane::ViewportFast), &fourth)
            .unwrap();
        coordinator.cancel_viewport_request("shared-fourth", &[1]);
        let jobs = coordinator.queue.jobs.lock().unwrap();
        assert_eq!(jobs.len(), 0, "最后一个请求撤销后不保留排队工作");
        assert!(jobs.active.is_empty());
        assert!(matches!(
            fourth_rx.try_recv().unwrap(),
            Ok(ImageExecution::Skipped(1))
        ));
    }
}
