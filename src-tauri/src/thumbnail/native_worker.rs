//! Windows 原生缩略图执行域：fast/tail 有界并行；超时终止并确认子进程退出。

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, FILETIME, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

use crate::db::queries::{ThumbnailCandidate, ThumbnailLane};
use crate::error::{AppError, Result};

use super::coordinator::{LatencyStats, NativeCounters};
use super::generator::EncodedThumbPayload;
use super::native_protocol::{
    read_response, write_request, NativeBackend, NativeExecution, NativeQosAck, NativeRequest,
    NativeResponse, NativeStage, NativeTimings, WORKER_HELLO,
};

use super::qos::{native_timeout, SourceMedia};
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

type ReportedDevice = (NativeBackend, u32, u32, i32, u32);
type NativeReply = std::result::Result<(EncodedThumbPayload, NativeExecution, NativeTimings), u8>;

pub struct NativeWorkers {
    memory: Arc<Mutex<[u64; 2]>>,
    fast: Domain,
    tail: Domain,
    reported_devices: Mutex<HashSet<ReportedDevice>>,
    reported_deadlines: Mutex<HashSet<(bool, SourceMedia, bool)>>,
    backend_completed: [AtomicU64; 7],
    failed_responses: AtomicU64,
    timed_out: AtomicU64,
    worker_lost: AtomicU64,
    qos_apply_accepted: AtomicU64,
    qos_apply_failed: AtomicU64,
    qos_cross_boundary: AtomicU64,
    domain_wait: LatencyStats,
    request_wall: LatencyStats,
    decode_transform: LatencyStats,
    encode_hash: LatencyStats,
    encode_stages: [LatencyStats; super::generator::ENCODE_STAGE_COUNT],
    embedded_jpeg_combined: LatencyStats,
}

#[derive(Clone, Copy)]
struct ProcessMemory {
    working_set_bytes: u64,
    private_bytes: u64,
}

fn process_memory(handle: HANDLE) -> Option<ProcessMemory> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    counters.cb = size;
    // SAFETY: 调用期间宿主伪句柄或持有的 Child 句柄均有效；输出缓冲按 cb 给出完整大小。
    unsafe {
        GetProcessMemoryInfo(
            handle,
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            size,
        )
        .ok()?;
    }
    Some(ProcessMemory {
        working_set_bytes: counters.WorkingSetSize as u64,
        private_bytes: counters.PrivateUsage as u64,
    })
}

fn process_cpu_ms(handle: HANDLE) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
    }
    let ticks =
        |value: FILETIME| (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime);
    Some(ticks(kernel).saturating_add(ticks(user)) / 10_000)
}

// 两个持久域共用一个内核配额；句柄关闭会终止仍关联的子进程。
struct WorkerJob(OwnedHandle);

impl WorkerJob {
    fn cpu_ms(&self) -> Option<u64> {
        // Job 计账包含已经退出的两个原生域进程，重启不会使累计 CPU 用时倒退。
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                HANDLE(self.0.as_raw_handle()),
                JobObjectBasicAccountingInformation,
                std::ptr::from_mut(&mut accounting).cast(),
                std::mem::size_of_val(&accounting) as u32,
                None,
            )
            .ok()?;
        }
        let ticks = accounting
            .TotalUserTime
            .saturating_add(accounting.TotalKernelTime);
        Some(u64::try_from(ticks).ok()? / 10_000)
    }
    fn new(logical: usize) -> io::Result<Self> {
        // SAFETY: 未命名 Job 仅由本进程持有，失败时不继续启动无上限 worker。
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(io::Error::other)?;
        // SAFETY: CreateJobObjectW 返回新句柄，由 OwnedHandle 唯一负责关闭。
        let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT(
            JOB_OBJECT_LIMIT_JOB_MEMORY.0
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY.0
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0,
        );
        limits.JobMemoryLimit = super::limits::get().total_memory_bytes();
        limits.ProcessMemoryLimit = super::limits::get().process_memory_bytes();
        // SAFETY: 传入完整的 ExtendedLimitInformation，调用结束前结构始终有效。
        let set = unsafe {
            SetInformationJobObject(
                HANDLE(handle.as_raw_handle()),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        set.map_err(io::Error::other)?;
        if let Some(rate) = super::qos::native_cpu_rate_for(logical) {
            let cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: rate },
            };
            // SAFETY: 结构类型/长度与信息类一致，两个域随后均加入此 Job。
            unsafe {
                SetInformationJobObject(
                    HANDLE(handle.as_raw_handle()),
                    JobObjectCpuRateControlInformation,
                    (&cpu as *const JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                    std::mem::size_of_val(&cpu) as u32,
                )
            }
            .map_err(io::Error::other)?;
            tracing::info!(target: "scrollery::thumb_perf", logical_cores = logical,
                native_cpu_rate_per_10000 = rate, "native thumbnail shared CPU limit applied");
        }
        Ok(Self(handle))
    }

    fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: 子进程句柄在调用期间有效，Job 在 WorkerProcess 生命周期内持有。
        unsafe {
            AssignProcessToJobObject(
                HANDLE(self.0.as_raw_handle()),
                HANDLE(child.as_raw_handle()),
            )
        }
        .map_err(io::Error::other)
    }
}

struct Domain {
    fast: bool,
    capacity: usize,
    job: Arc<Mutex<Option<Arc<WorkerJob>>>>,
    available: crossbeam_channel::Receiver<()>,
    release: crossbeam_channel::Sender<()>,
    slot: Mutex<WorkerSlot>,
}

pub(super) struct DomainTicket {
    release: crossbeam_channel::Sender<()>,
    fast: bool,
    memory: Option<NativeMemoryPermit>,
}

// 像素预算之外预留进程运行库与原生后端增长空间；Job 硬限仍是最后边界。
const PROCESS_HEADROOM: u64 = 64 * 1024 * 1024;
const GPU_REQUEST_HEADROOM: u64 = 64 * 1024 * 1024;

struct NativeMemoryPermit {
    reserved: Arc<Mutex<[u64; 2]>>,
    domain: usize,
    bytes: u64,
    pixels: u64,
}

impl NativeMemoryPermit {
    fn shrink_to(&mut self, bytes: u64) {
        if bytes < self.bytes {
            self.reserved.lock().unwrap_or_else(|e| e.into_inner())[self.domain] -=
                self.bytes - bytes;
            self.bytes = bytes;
        }
    }
}

impl Drop for NativeMemoryPermit {
    fn drop(&mut self) {
        self.shrink_to(0);
    }
}

pub(super) fn memory_admits(
    private: [u64; 2],
    reserved: [u64; 2],
    domain: usize,
    bytes: u64,
    limits: &super::limits::ThumbnailLimits,
) -> bool {
    // 已用内存与未完成请求的最坏增长同时计入，避免“软预算通过、Job 分配失败”。
    private[domain]
        .saturating_add(reserved[domain])
        .saturating_add(bytes)
        .saturating_add(PROCESS_HEADROOM)
        <= limits.process_memory_bytes() as u64
        && private
            .iter()
            .sum::<u64>()
            .saturating_add(reserved.iter().sum::<u64>())
            .saturating_add(bytes)
            .saturating_add(PROCESS_HEADROOM)
            <= limits.total_memory_bytes() as u64
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum MemoryAdmission {
    Admit,
    Wait,
    RecycleIdle,
    Unavailable,
}

pub(super) fn memory_admission(
    private: [u64; 2],
    reserved: [u64; 2],
    domain: usize,
    bytes: u64,
    limits: &super::limits::ThumbnailLimits,
    recycled: bool,
) -> MemoryAdmission {
    if memory_admits(private, reserved, domain, bytes, limits) {
        MemoryAdmission::Admit
    } else if reserved.iter().any(|bytes| *bytes != 0) {
        MemoryAdmission::Wait
    } else if !recycled {
        MemoryAdmission::RecycleIdle
    } else {
        MemoryAdmission::Unavailable
    }
}

pub(super) struct NativePending {
    _ticket: DomainTicket,
    _gpu: Option<crate::engine::gpu::budget::GpuPermit<'static>>,
    process: Arc<WorkerProcess>,
    reply: PendingReply,
    started: Instant,
    lane: ThumbnailLane,
    item_id: i64,
    media: SourceMedia,
    completed: bool,
}

impl NativePending {
    pub(super) fn source_done(&self) -> bool {
        self.reply.source_done
    }

    pub(super) fn encoding_ready(&self) -> bool {
        self.reply.encoding_ready
    }
}

impl Drop for NativePending {
    fn drop(&mut self) {
        // 宿主退出或放弃在途项时，先终止并确认退出，再归还域额度。
        if !self.completed {
            self.process.terminate();
        }
    }
}

struct PendingReply {
    gpu_done: bool,
    source_done: bool,
    encoding_ready: bool,
    receiver: crossbeam_channel::Receiver<Reply>,
    id: u64,
    qos_revision: u64,
    deadline: Instant,
    background_deadline: Instant,
}

impl PendingReply {
    fn apply_qos_revision(&mut self, revision: u64) {
        // 焦点发生过变化说明可能经历后台节流；只放宽到固定后台截止，不重置起点。
        if revision != self.qos_revision {
            self.deadline = self.background_deadline;
        }
    }

    fn poll(&mut self) -> Option<io::Result<(NativeReply, NativeQosAck)>> {
        loop {
            let response = match self.receiver.try_recv() {
                Ok(response) => response,
                Err(crossbeam_channel::TryRecvError::Empty) if Instant::now() < self.deadline => {
                    return None
                }
                Err(crossbeam_channel::TryRecvError::Empty) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native thumbnail deadline",
                )),
                Err(crossbeam_channel::TryRecvError::Disconnected) => Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "native thumbnail worker exited",
                )),
            };
            if let Ok((arrived, NativeResponse::Stage { id, stage })) = &response {
                let valid = *id == self.id
                    && *arrived <= self.deadline
                    && match stage {
                        NativeStage::GpuDone => !self.gpu_done && !self.source_done,
                        NativeStage::SourceDone => self.gpu_done && !self.source_done,
                        NativeStage::EncodingReady => self.source_done && !self.encoding_ready,
                    };
                if !valid {
                    return Some(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid native stage order",
                    )));
                }
                match stage {
                    NativeStage::GpuDone => self.gpu_done = true,
                    NativeStage::SourceDone => self.source_done = true,
                    NativeStage::EncodingReady => self.encoding_ready = true,
                }
                continue;
            }
            return Some(response.and_then(|(arrived, response)| {
                if arrived > self.deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native thumbnail deadline",
                    ));
                }
                match response {
                    NativeResponse::Ok {
                        id,
                        payload,
                        execution,
                        timings,
                        qos,
                    } if id == self.id && qos.revision == self.qos_revision => {
                        Ok((Ok((payload, execution, timings)), qos))
                    }
                    NativeResponse::Failed { id, code, qos }
                        if id == self.id && qos.revision == self.qos_revision =>
                    {
                        Ok((Err(code), qos))
                    }
                    _ => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "native thumbnail reply identity mismatch",
                    )),
                }
            }));
        }
    }
}

impl Drop for DomainTicket {
    fn drop(&mut self) {
        let _ = self.release.send(());
    }
}

struct WorkerSlot {
    process: Option<Arc<WorkerProcess>>,
    starts: u8,
}

type Reply = io::Result<(Instant, NativeResponse)>;
type PendingWaiter = (crossbeam_channel::Sender<Reply>, Arc<Condvar>);
type Pending = Arc<Mutex<HashMap<u64, PendingWaiter>>>;

struct WorkerProcess {
    qos_receipts: Mutex<Vec<Option<NativeQosAck>>>,
    _job: Arc<WorkerJob>,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
}

impl NativeWorkers {
    pub fn new(fast_capacity: usize) -> Self {
        let job = Arc::new(Mutex::new(None));
        Self {
            memory: Arc::new(Mutex::new([0; 2])),
            fast: Domain::new(true, fast_capacity.max(1), Arc::clone(&job)),
            tail: Domain::new(false, super::limits::get().tail_threads, job),
            reported_devices: Mutex::new(HashSet::new()),
            reported_deadlines: Mutex::new(HashSet::new()),
            backend_completed: std::array::from_fn(|_| AtomicU64::new(0)),
            failed_responses: AtomicU64::new(0),
            timed_out: AtomicU64::new(0),
            worker_lost: AtomicU64::new(0),
            qos_apply_accepted: AtomicU64::new(0),
            qos_apply_failed: AtomicU64::new(0),
            qos_cross_boundary: AtomicU64::new(0),
            domain_wait: LatencyStats::new(),
            request_wall: LatencyStats::new(),
            decode_transform: LatencyStats::new(),
            encode_hash: LatencyStats::new(),
            encode_stages: std::array::from_fn(|_| LatencyStats::new()),
            embedded_jpeg_combined: LatencyStats::new(),
        }
    }

    /// 新的全库/增量轮次重置故障域的自动重启额度；活着的进程仍算首次启动。
    pub(crate) fn begin_run(&self) {
        self.fast.begin_run();
        self.tail.begin_run();
    }

    /// 旧派生入口只驱动原生视频尾批，不改变并行快速域的重启额度。
    pub(crate) fn begin_tail_run(&self) {
        self.tail.begin_run();
    }

    /// 应用生命周期内的成功后端及原生失败累计值，供低频窗口日志读取。
    pub(crate) fn counters(&self, qos_foreground: bool, qos_revision: u64) -> NativeCounters {
        let counts = self
            .backend_completed
            .each_ref()
            .map(|count| count.load(Ordering::Relaxed));
        let host_memory = process_memory(unsafe { GetCurrentProcess() });
        let host_cpu_ms = process_cpu_ms(unsafe { GetCurrentProcess() });
        let worker_job_cpu_ms = self
            .fast
            .job
            .try_lock()
            .ok()
            .and_then(|job| job.as_ref().map_or(Some(0), |job| job.cpu_ms()));
        let mut qos_workers = super::qos::QosCounts::default();
        let (gpu_inflight, gpu_peak, gpu_denied) = crate::engine::gpu::budget::snapshot();
        let processes = [self.fast.current_process(), self.tail.current_process()];
        let mut worker_processes = 0;
        let mut worker_memory_samples = 0;
        let mut worker_working_set_bytes = 0u64;
        let mut worker_private_bytes = 0u64;
        for process in processes.into_iter().flatten() {
            if !process.alive.load(Ordering::Acquire) {
                continue;
            }
            for ack in process
                .qos_receipts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
            {
                qos_workers.record(qos_revision, ack.map(|ack| (ack.revision, ack.applied)));
            }
            worker_processes += 1;
            if let Some(memory) = process.memory_snapshot() {
                worker_memory_samples += 1;
                worker_working_set_bytes =
                    worker_working_set_bytes.saturating_add(memory.working_set_bytes);
                worker_private_bytes = worker_private_bytes.saturating_add(memory.private_bytes);
            }
        }
        let complete_worker_sample = worker_memory_samples == worker_processes;
        NativeCounters {
            gpu_inflight,
            gpu_peak,
            gpu_denied,
            gpu_limit: super::limits::get().gpu_inflight,
            embedded_jpeg: counts[0],
            image_d2d: counts[1],
            image_wic: counts[2],
            image_rs: counts[3],
            video_mf: counts[4],
            video_hardware_mft: counts[5],
            image_vpl: counts[6],
            failed_responses: self.failed_responses.load(Ordering::Relaxed),
            timed_out: self.timed_out.load(Ordering::Relaxed),
            worker_lost: self.worker_lost.load(Ordering::Relaxed),
            qos_apply_accepted: self.qos_apply_accepted.load(Ordering::Relaxed),
            qos_apply_failed: self.qos_apply_failed.load(Ordering::Relaxed),
            qos_cross_boundary: self.qos_cross_boundary.load(Ordering::Relaxed),
            domain_wait: self.domain_wait.snapshot(),
            request_wall: self.request_wall.snapshot(),
            decode_transform: self.decode_transform.snapshot(),
            encode_hash: self.encode_hash.snapshot(),
            encode_stages: self.encode_stages.each_ref().map(LatencyStats::snapshot),
            embedded_jpeg_combined: self.embedded_jpeg_combined.snapshot(),
            host_working_set_bytes: host_memory.map(|memory| memory.working_set_bytes),
            host_private_bytes: host_memory.map(|memory| memory.private_bytes),
            host_cpu_ms,
            worker_job_cpu_ms,
            qos_workers,
            qos_revision,
            qos_foreground,
            worker_processes,
            worker_memory_samples,
            worker_working_set_bytes: complete_worker_sample.then_some(worker_working_set_bytes),
            worker_private_bytes: complete_worker_sample.then_some(worker_private_bytes),
        }
    }

    fn domain(&self, lane: ThumbnailLane) -> &Domain {
        if matches!(
            lane,
            ThumbnailLane::Fast | ThumbnailLane::ViewportFast | ThumbnailLane::ImageRs
        ) {
            &self.fast
        } else {
            &self.tail
        }
    }

    pub(super) fn available(&self) -> (bool, bool) {
        (
            !self.fast.available.is_empty(),
            !self.tail.available.is_empty(),
        )
    }

    pub(super) fn try_reserve(&self, lane: ThumbnailLane) -> Option<DomainTicket> {
        let started = Instant::now();
        let ticket = self.domain(lane).try_acquire()?;
        self.domain_wait.record(started.elapsed());
        Some(ticket)
    }

    /// 领取计算槽前检查整个原生进程及合计内存；繁忙时回到宿主队列等待。
    pub(super) fn try_admit(
        &self,
        lane: ThumbnailLane,
        bytes: u64,
        gpu: bool,
    ) -> Result<Option<DomainTicket>> {
        let Some(mut ticket) = self.try_reserve(lane) else {
            return Ok(None);
        };
        let domain = self.domain(lane);
        let mut reserved = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        let index = usize::from(!ticket.fast);
        let limits = super::limits::get();
        let gpu_bytes = if gpu { GPU_REQUEST_HEADROOM } else { 0 };
        // 单张超大图也不能获得超过进程硬限的像素额度；worker 按真实尺寸拒绝超额。
        let pixels = bytes.min(
            (limits.process_memory_bytes() as u64).saturating_sub(PROCESS_HEADROOM * 2 + gpu_bytes),
        );
        let charge = pixels.saturating_add(gpu_bytes);
        let mut recycled = false;
        loop {
            domain.process().map_err(|error| {
                tracing::error!(target: "scrollery::thumb_perf", %error,
                    "native thumbnail execution domain unavailable");
                AppError::ThumbnailUnavailable("worker_unavailable")
            })?;
            let mut private = [0; 2];
            for (index, domain) in [&self.fast, &self.tail].into_iter().enumerate() {
                if let Some(process) = domain.current_process() {
                    // 日志可跳过采样，准入不能把采样失败解释为零内存。
                    let child = process.child.lock().unwrap_or_else(|e| e.into_inner());
                    private[index] = process_memory(HANDLE(child.as_raw_handle()))
                        .ok_or(AppError::ThumbnailUnavailable("worker_memory_limit"))?
                        .private_bytes;
                }
            }
            match memory_admission(private, *reserved, index, charge, limits, recycled) {
                MemoryAdmission::Admit => break,
                MemoryAdmission::Wait => return Ok(None),
                MemoryAdmission::RecycleIdle => {
                    // 准入锁阻止新预留，两域已无在途项；回收原生缓存后只重查一次。
                    tracing::info!(target: "scrollery::thumb_perf", ?private, charge,
                        "recycling idle native thumbnail workers for memory admission");
                    self.fast.recycle_idle()?;
                    self.tail.recycle_idle()?;
                    recycled = true;
                }
                MemoryAdmission::Unavailable => {
                    tracing::warn!(target: "scrollery::thumb_perf", ?private, charge,
                        process_limit = limits.process_memory_bytes(), total_limit = limits.total_memory_bytes(),
                        "native thumbnail memory limit cannot admit one request after idle recycle");
                    return Err(AppError::ThumbnailUnavailable("worker_memory_limit"));
                }
            }
        }
        reserved[index] += charge;
        ticket.memory = Some(NativeMemoryPermit {
            reserved: Arc::clone(&self.memory),
            domain: index,
            bytes: charge,
            pixels,
        });
        Ok(Some(ticket))
    }

    pub(super) fn start(
        &self,
        ticket: DomainTicket,
        lane: ThumbnailLane,
        candidate: &ThumbnailCandidate,
        mut request: NativeRequest,
        wake: Arc<Condvar>,
        media: SourceMedia,
    ) -> Result<NativePending> {
        let domain = if ticket.fast { &self.fast } else { &self.tail };
        if let Some(memory) = &ticket.memory {
            request.max_pixel_bytes =
                request
                    .max_pixel_bytes
                    .min(super::native_protocol::pixel_limit_for_reservation(
                        memory.pixels,
                    ));
        }
        let process = domain.process().map_err(|error| {
            tracing::debug!(target: "scrollery::thumb_perf", %error, "native worker unavailable");
            AppError::ThumbnailUnavailable("worker_unavailable")
        })?;
        let file = crate::export::open_authorized_source(
            &candidate.root_path,
            &candidate.relative_dir,
            &candidate.item.file_name,
        )?;
        let gpu = request
            .prefer_gpu
            .then(|| crate::engine::gpu::budget::try_acquire(domain.fast))
            .flatten();
        if gpu.is_none() {
            request.prefer_gpu = false;
        }
        (request.qos_foreground, request.qos_revision) = super::qos::native_worker_qos_request();
        // 软件尾批共用普通计算槽，但其中仍可能包含大图，使用既有重型截止。
        let fast_deadline = domain.fast && lane != ThumbnailLane::ImageRs;
        let timeout = native_timeout(fast_deadline, media, request.qos_foreground);
        let background_timeout = native_timeout(fast_deadline, media, false);
        if self
            .reported_deadlines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((domain.fast, media, request.qos_foreground))
        {
            tracing::info!(target: "scrollery::thumb_perf", domain_fast = domain.fast,
                source_media = ?media, foreground = request.qos_foreground,
                native_timeout_ms = timeout.as_millis() as u64,
                "native thumbnail deadline policy");
        }
        let started = Instant::now();
        let reply = process
            .request(&file, request, timeout, background_timeout, wake)
            .map_err(|error| {
                self.worker_lost.fetch_add(1, Ordering::Relaxed);
                process.terminate();
                domain.forget(&process);
                tracing::debug!(target: "scrollery::thumb_perf", %error, "native worker request failed");
                AppError::ThumbnailUnavailable("worker_lost")
            })?;
        Ok(NativePending {
            _ticket: ticket,
            _gpu: gpu,
            process,
            reply,
            started,
            lane,
            item_id: candidate.item.id,
            media,
            completed: false,
        })
    }

    pub(super) fn poll(
        &self,
        pending: &mut NativePending,
    ) -> Option<Result<(EncodedThumbPayload, NativeExecution)>> {
        pending
            .reply
            .apply_qos_revision(super::qos::native_worker_qos_request().1);
        let response = pending.reply.poll();
        if pending.reply.encoding_ready {
            if let Some(memory) = &mut pending._ticket.memory {
                memory.shrink_to(super::native_protocol::ENCODING_RESERVATION_BYTES);
            }
        }
        if pending.reply.gpu_done {
            drop(pending._gpu.take());
        }
        let response = response?;
        pending.completed = true;
        Some(self.finish_response(pending, response))
    }

    fn finish_response(
        &self,
        pending: &NativePending,
        response: io::Result<(NativeReply, NativeQosAck)>,
    ) -> Result<(EncodedThumbPayload, NativeExecution)> {
        let lane = pending.lane;
        let item_id = pending.item_id;
        let process = &pending.process;
        let domain = self.domain(lane);
        let response = response.and_then(|(outcome, qos)| {
            record_qos_receipt(
                &mut process
                    .qos_receipts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
                qos,
            )?;
            if qos.attempted {
                let counter = if qos.applied {
                    &self.qos_apply_accepted
                } else {
                    &self.qos_apply_failed
                };
                counter.fetch_add(1, Ordering::Relaxed);
                tracing::info!(target: "scrollery::thumb_perf", qos_revision = qos.revision,
                    system_request_accepted = qos.applied, "native thumbnail worker QoS applied");
            }
            Ok(outcome)
        });
        self.request_wall.record(pending.started.elapsed());
        if super::qos::native_worker_qos_request().1 != pending.reply.qos_revision {
            self.qos_cross_boundary.fetch_add(1, Ordering::Relaxed);
        }
        match response {
            Ok(Ok((payload, execution, timings))) => {
                if timings.decode_transform_us > 0 {
                    self.decode_transform
                        .record(Duration::from_micros(timings.decode_transform_us));
                }
                if timings.encode_hash_us > 0 {
                    self.encode_hash
                        .record(Duration::from_micros(timings.encode_hash_us));
                    // 零耗时也计入同一批成功请求；AI 未请求或空操作不会抬高分项均值。
                    for (stats, micros) in self.encode_stages.iter().zip(timings.encode_stages_us) {
                        stats.record(Duration::from_micros(micros));
                    }
                }
                if timings.embedded_jpeg_combined_us > 0 {
                    self.embedded_jpeg_combined
                        .record(Duration::from_micros(timings.embedded_jpeg_combined_us));
                }
                self.backend_completed[execution.backend as usize - 1]
                    .fetch_add(1, Ordering::Relaxed);
                if matches!(
                    execution.backend,
                    NativeBackend::ImageD2d
                        | NativeBackend::VideoMfHardwareMft
                        | NativeBackend::ImageVpl
                ) {
                    let device = (
                        execution.backend,
                        execution.vendor_id,
                        execution.device_id,
                        execution.luid_high,
                        execution.luid_low,
                    );
                    let first = self
                        .reported_devices
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(device);
                    if first && execution.backend == NativeBackend::ImageD2d {
                        tracing::info!(
                            target: "scrollery::thumb_perf",
                            vendor_id = execution.vendor_id,
                            device_id = execution.device_id,
                            adapter_luid_high = execution.luid_high,
                            adapter_luid_low = execution.luid_low,
                            decode_backend = "wic",
                            transform_backend = "d2d11",
                            hardware_decode = "unknown",
                            "image GPU transform confirmed by native worker result"
                        );
                    } else if first && execution.backend == NativeBackend::ImageVpl {
                        tracing::info!(target: "scrollery::thumb_perf",
                            vendor_id = execution.vendor_id, device_id = execution.device_id,
                            adapter_luid_high = execution.luid_high, adapter_luid_low = execution.luid_low,
                            decode_backend = "vpl_jpeg", transform_backend = "vpl_vpp",
                            hardware_decode = "confirmed", "JPEG hardware pipeline confirmed by native worker result");
                    } else if first {
                        tracing::info!(
                            target: "scrollery::thumb_perf",
                            vendor_id = execution.vendor_id,
                            device_id = execution.device_id,
                            adapter_luid_high = execution.luid_high,
                            adapter_luid_low = execution.luid_low,
                            decode_backend = "media_foundation",
                            hardware_decode = "confirmed_hardware_mft",
                            "video hardware decoder MFT confirmed by decoded cover result"
                        );
                    }
                }
                tracing::debug!(target: "scrollery::thumb_perf", item_id = item_id, backend = ?execution.backend, "native thumbnail result");
                Ok((payload, execution))
            }
            Ok(Err(code)) => {
                if code == super::native_protocol::IMAGE_RS_DEFERRED {
                    return Err(AppError::ThumbnailUnavailable("image_rs_deferred"));
                }
                self.failed_responses.fetch_add(1, Ordering::Relaxed);
                if code == 6 {
                    return Err(AppError::ThumbnailUnavailable("pixel_limit"));
                }
                Err(AppError::Internal(format!(
                    "native thumbnail decode failed: {code}"
                )))
            }
            Err(error) => {
                if error.kind() == io::ErrorKind::TimedOut {
                    self.timed_out.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.worker_lost.fetch_add(1, Ordering::Relaxed);
                }
                tracing::warn!(
                    target: "scrollery::thumb_perf",
                    item_id = item_id,
                    lane = lane.as_str(),
                    error_kind = ?error.kind(),
                    source_media = ?pending.media,
                    native_deadline_ms = pending.reply.deadline.saturating_duration_since(pending.started).as_millis() as u64,
                    "native thumbnail worker lost; terminating domain"
                );
                let exit_code = process.terminate_with_exit_code();
                tracing::warn!(target: "scrollery::thumb_perf",
                    item_id, lane = lane.as_str(), ?exit_code,
                    abandoned_gpu_work_exit = exit_code == Some(70),
                    "native thumbnail worker exit confirmed");
                domain.forget(process);
                Err(AppError::ThumbnailUnavailable(
                    if error.kind() == io::ErrorKind::TimedOut {
                        "worker_timeout"
                    } else {
                        "worker_lost"
                    },
                ))
            }
        }
    }
}

impl Domain {
    fn new(fast: bool, capacity: usize, job: Arc<Mutex<Option<Arc<WorkerJob>>>>) -> Self {
        let (release, available) = crossbeam_channel::bounded(capacity);
        for _ in 0..capacity {
            release.send(()).expect("bounded domain permits");
        }
        Self {
            fast,
            capacity,
            job,
            available,
            release,
            slot: Mutex::new(WorkerSlot {
                process: None,
                starts: 0,
            }),
        }
    }

    fn begin_run(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.starts = u8::from(
            slot.process
                .as_ref()
                .is_some_and(|process| process.alive.load(Ordering::Acquire)),
        );
    }

    // 调用方必须持有共享准入锁并确认两域预留均为零，不能终止任何在途任务。
    fn recycle_idle(&self) -> Result<()> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(process) = slot.process.as_ref() {
            let was_alive = process.alive.load(Ordering::Acquire);
            // 确认退出后才释放槽位，否则不能假定旧进程占用的内存已归还。
            process
                .terminate_with_exit_code()
                .ok_or(AppError::ThumbnailUnavailable("worker_unavailable"))?;
            slot.process = None;
            if was_alive {
                // 受控回收只替换健康进程，不归零既有故障重启计数。
                slot.starts = slot.starts.saturating_sub(1);
            }
        }
        Ok(())
    }

    fn current_process(&self) -> Option<Arc<WorkerProcess>> {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .process
            .as_ref()
            .filter(|process| process.alive.load(Ordering::Acquire))
            .cloned()
    }

    fn try_acquire(&self) -> Option<DomainTicket> {
        self.available.try_recv().ok()?;
        Some(DomainTicket {
            release: self.release.clone(),
            fast: self.fast,
            memory: None,
        })
    }

    fn process(&self) -> Result<Arc<WorkerProcess>> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(process) = slot
            .process
            .as_ref()
            .filter(|process| process.alive.load(Ordering::Acquire))
        {
            return Ok(Arc::clone(process));
        }
        slot.process = None;
        if slot.starts >= 2 {
            return Err(AppError::Internal(
                "native thumbnail worker restart limit".into(),
            ));
        }
        slot.starts += 1;
        let job = {
            let mut shared = self.job.lock().unwrap_or_else(|e| e.into_inner());
            if shared.is_none() {
                let logical = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(8);
                *shared = Some(Arc::new(WorkerJob::new(logical)?));
            }
            Arc::clone(shared.as_ref().expect("worker job initialized"))
        };
        let process = WorkerProcess::spawn(self.fast, self.capacity, job)?;
        tracing::info!(
            target: "scrollery::thumb_perf",
            domain = if self.fast { "fast" } else { "tail" },
            parallelism = self.capacity,
            resource_settings = ?super::limits::get(),
            job_memory_limit_bytes = super::limits::get().total_memory_bytes(),
            process_memory_limit_bytes = super::limits::get().process_memory_bytes(),
            "native thumbnail worker ready"
        );
        slot.process = Some(Arc::clone(&process));
        Ok(process)
    }

    fn forget(&self, process: &Arc<WorkerProcess>) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot
            .process
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, process))
        {
            slot.process = None;
        }
    }
}

fn record_qos_receipt(receipts: &mut [Option<NativeQosAck>], ack: NativeQosAck) -> io::Result<()> {
    let slot = ack
        .worker_slot
        .checked_sub(1)
        .and_then(|slot| receipts.get_mut(usize::from(slot)))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "native QoS worker slot"))?;
    // 宿主收取顺序不等于执行顺序；仅用线程自己的递增序号接受最新回执。
    if slot.is_none_or(|previous| ack.sequence > previous.sequence) {
        *slot = Some(ack);
    }
    Ok(())
}

impl WorkerProcess {
    fn memory_snapshot(&self) -> Option<ProcessMemory> {
        // 终止路径可能在持锁等待进程退出；低频采样跳过这一拍，不阻塞轮次日志。
        let child = self.child.try_lock().ok()?;
        process_memory(HANDLE(child.as_raw_handle()))
    }

    fn spawn(fast: bool, capacity: usize, job: Arc<WorkerJob>) -> Result<Arc<Self>> {
        let exe = std::env::current_exe()?.with_file_name("native-thumbnail-worker.exe");
        let mut command = Command::new(exe);
        command.arg(format!("--threads={capacity}"));
        command.arg(format!(
            "--thumbnail-limits={}",
            serde_json::to_string(super::limits::get()).expect("thumbnail limits")
        ));
        command.arg(if fast {
            format!("--fast={capacity}")
        } else {
            "--tail".to_owned()
        });
        Self::spawn_command(command, capacity, job)
    }

    fn spawn_command(
        mut command: Command,
        capacity: usize,
        job: Arc<WorkerJob>,
    ) -> Result<Arc<Self>> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Io(error));
        }
        let stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (hello_tx, hello_rx) = crossbeam_channel::bounded(1);
        let reader_pending = Arc::clone(&pending);
        let reader_alive = Arc::clone(&alive);
        std::thread::spawn(move || {
            let mut hello = [0u8; 4];
            let handshake = stdout.read_exact(&mut hello).and_then(|_| {
                if hello == WORKER_HELLO {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "native thumbnail handshake",
                    ))
                }
            });
            if hello_tx.send(handshake).is_err() {
                return;
            }
            loop {
                let response = match read_response(&mut stdout) {
                    Ok(response) => response,
                    Err(error) => {
                        // 协议错误必须保留原始原因，不能只在上层表现为批量 BrokenPipe。
                        if reader_alive.load(Ordering::Acquire) {
                            tracing::warn!(target: "scrollery::thumb_perf", %error, capacity,
                                "native thumbnail response read failed");
                        }
                        break;
                    }
                };
                let id = match &response {
                    NativeResponse::Ok { id, .. }
                    | NativeResponse::Failed { id, .. }
                    | NativeResponse::Stage { id, .. } => *id,
                };
                let waiter = {
                    let mut pending = reader_pending.lock().unwrap_or_else(|e| e.into_inner());
                    if matches!(response, NativeResponse::Stage { .. }) {
                        pending.get(&id).cloned()
                    } else {
                        pending.remove(&id)
                    }
                };
                if let Some((waiter, wake)) = waiter {
                    if waiter.try_send(Ok((Instant::now(), response))).is_err() {
                        break;
                    }
                    wake.notify_all();
                } else {
                    break; // 未请求的响应说明协议状态已失配。
                }
            }
            reader_alive.store(false, Ordering::Release);
            fail_pending(&reader_pending);
        });
        match hello_rx.recv_timeout(HANDSHAKE_TIMEOUT) {
            Ok(Ok(())) => Ok(Arc::new(Self {
                qos_receipts: Mutex::new(vec![None; capacity]),
                _job: job,
                child: Mutex::new(child),
                stdin: Mutex::new(stdin),
                pending,
                next_id: AtomicU64::new(0),
                alive,
            })),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err(AppError::Internal(
                    "native thumbnail worker handshake failed".into(),
                ))
            }
        }
    }

    fn request(
        &self,
        file: &File,
        mut request: NativeRequest,
        timeout: Duration,
        background_timeout: Duration,
        wake: Arc<Condvar>,
    ) -> io::Result<PendingReply> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "worker exited"));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        request.id = id;
        let mut transferred = HANDLE::default();
        {
            let child = self.child.lock().unwrap_or_else(|e| e.into_inner());
            // SAFETY: source 由 File 持有；target 由 Child 持有。仅复制到该进程且禁止继承。
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    HANDLE(file.as_raw_handle()),
                    HANDLE(child.as_raw_handle()),
                    &mut transferred,
                    0,
                    false,
                    DUPLICATE_SAME_ACCESS,
                )
                .map_err(io::Error::other)?;
            }
        }
        request.file_handle = transferred.0 as usize as u64;
        // 三个标量阶段加最终回包，reader 不因宿主尚未轮询而丢失完成消息。
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(4);
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, (reply_tx, wake));
        let written = write_request(
            &mut *self.stdin.lock().unwrap_or_else(|e| e.into_inner()),
            &request,
        );
        if let Err(error) = written {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            // 调用方随即终止子进程，内核回收尚未消费的句柄副本。
            return Err(error);
        }
        let sent = Instant::now();
        Ok(PendingReply {
            gpu_done: false,
            source_done: false,
            encoding_ready: false,
            receiver: reply_rx,
            id,
            qos_revision: request.qos_revision,
            deadline: sent + timeout,
            background_deadline: sent + background_timeout,
        })
    }

    fn terminate(&self) {
        let _ = self.terminate_with_exit_code();
    }

    fn terminate_with_exit_code(&self) -> Option<i32> {
        self.alive.store(false, Ordering::Release);
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        // 已自行退出时保留真实状态；MF/VPL 未完成硬件工作使用70，不能只留下管道失联。
        let status = match child.try_wait() {
            Ok(Some(status)) => Some(status),
            _ => {
                let _ = child.kill();
                child.wait().ok()
            }
        };
        fail_pending(&self.pending);
        status.and_then(|status| status.code())
    }
}

fn fail_pending(pending: &Pending) {
    let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
    for (_, (waiter, wake)) in pending.drain() {
        let _ = waiter.try_send(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "native thumbnail worker stopped",
        )));
        wake.notify_all();
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_fault_process_is_reaped(malformed: bool) {
        use std::io::Write;
        use std::os::windows::process::CommandExt;
        // 故障子进程使用同一握手、Job、reader及终止出口；读取触发字节后挂起或写非法回包。
        let script = format!(
            r#"$out=[Console]::OpenStandardOutput();$out.Write([byte[]](78,84,72,70),0,4);$out.Flush();$null=[Console]::OpenStandardInput().ReadByte();{};Start-Sleep -Seconds 15"#,
            if malformed {
                "$out.Write((New-Object byte[] 27),0,27);$out.Flush()"
            } else {
                "$null=0"
            }
        );
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x08000000);
        let process =
            WorkerProcess::spawn_command(command, 1, Arc::new(WorkerJob::new(8).unwrap())).unwrap();
        let workers = NativeWorkers::new(1);
        workers.fast.slot.lock().unwrap().process = Some(Arc::clone(&process));
        let ticket = workers.try_reserve(ThumbnailLane::Fast).unwrap();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        process
            .pending
            .lock()
            .unwrap()
            .insert(1, (sender, Arc::new(Condvar::new())));
        let started = Instant::now();
        let deadline = started + Duration::from_millis(if malformed { 3000 } else { 100 });
        let mut pending = NativePending {
            _ticket: ticket,
            _gpu: None,
            process: Arc::clone(&process),
            reply: PendingReply {
                gpu_done: false,
                source_done: false,
                encoding_ready: false,
                receiver,
                id: 1,
                qos_revision: super::super::qos::native_worker_qos_request().1,
                deadline,
                background_deadline: deadline,
            },
            started,
            lane: ThumbnailLane::Fast,
            item_id: 1,
            media: SourceMedia::Ssd,
            completed: false,
        };
        {
            let mut input = process.stdin.lock().unwrap();
            input.write_all(&[1]).unwrap();
            input.flush().unwrap();
        }
        let result = loop {
            if let Some(result) = workers.poll(&mut pending) {
                break result;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let expected = if malformed {
            "worker_lost"
        } else {
            "worker_timeout"
        };
        assert!(
            matches!(result, Err(AppError::ThumbnailUnavailable(reason)) if reason == expected)
        );
        assert!(pending.completed);
        assert!(process.child.lock().unwrap().try_wait().unwrap().is_some());
        assert!(process.pending.lock().unwrap().is_empty());
        assert!(workers.fast.slot.lock().unwrap().process.is_none());
        assert!(!workers.available().0, "在途项归还前仍持有域名额");
        drop(pending);
        assert!(workers.available().0);
        assert_eq!(
            workers.timed_out.load(Ordering::Relaxed),
            u64::from(!malformed)
        );
        assert_eq!(
            workers.worker_lost.load(Ordering::Relaxed),
            u64::from(malformed)
        );
        eprintln!(
            "fault {expected} confirmed exit after {:?}",
            started.elapsed()
        );
    }

    #[test]
    #[ignore = "launches a controlled PowerShell fault process"]
    fn real_supervisor_fault_timeout_reaps_before_release() {
        assert_fault_process_is_reaped(false);
    }

    // 依赖当前协议的真实 sidecar；仅在显式暂存到测试程序同目录后运行。

    #[test]
    fn pending_reply_enforces_deadline_and_identity() {
        use super::super::native_protocol::{write_response, NativeTimings};

        // 回包槽号随实际进程容量变化；成功与格式失败都必须穿过真实编解包和 QoS 记账。
        let mut receipts = vec![None; 9];
        for worker_slot in 1..=9 {
            let qos = NativeQosAck {
                worker_slot,
                sequence: 1,
                revision: 3,
                applied: true,
                attempted: true,
            };
            let responses = [
                NativeResponse::Ok {
                    id: 1,
                    payload: EncodedThumbPayload {
                        webp: vec![1],
                        thumbhash: None,
                        ai_cache: None,
                    },
                    execution: NativeExecution::cpu(NativeBackend::ImageRs),
                    timings: NativeTimings {
                        decode_transform_us: 10_000,
                        encode_hash_us: 12_000,
                        embedded_jpeg_combined_us: 0,
                        encode_stages_us: [300, 400, 500, 600, 700],
                    },
                    qos,
                },
                NativeResponse::Failed {
                    id: 1,
                    code: 3,
                    qos,
                },
            ];
            for response in responses {
                let mut bytes = Vec::new();
                write_response(&mut bytes, &response).unwrap();
                let ack = match read_response(&bytes[..]).unwrap() {
                    NativeResponse::Ok { qos, timings, .. } => {
                        assert_eq!(timings.decode_transform_us, 10_000);
                        assert_eq!(timings.encode_hash_us, 12_000);
                        assert_eq!(timings.encode_stages_us, [300, 400, 500, 600, 700]);
                        // 计时字段必须完整且有界，不能把错误的扩展头当成产物长度读取。
                        let stages_offset = 1 + 8 + 18 + 18 + 24;
                        let mut malformed = bytes.clone();
                        malformed[stages_offset..stages_offset + 8]
                            .copy_from_slice(&u64::MAX.to_le_bytes());
                        assert_eq!(
                            read_response(&malformed[..]).err().unwrap().kind(),
                            io::ErrorKind::InvalidData
                        );
                        assert_eq!(
                            read_response(&bytes[..stages_offset + 39])
                                .err()
                                .unwrap()
                                .kind(),
                            io::ErrorKind::UnexpectedEof
                        );
                        qos
                    }
                    NativeResponse::Failed { qos, .. } => qos,
                    _ => panic!("expected final response"),
                };
                assert_eq!(ack.worker_slot, worker_slot);
                record_qos_receipt(&mut receipts, ack).unwrap();
                bytes[18] = 0;
                assert_eq!(
                    read_response(&bytes[..]).err().unwrap().kind(),
                    io::ErrorKind::InvalidData
                );
            }
        }
        assert!(receipts.iter().all(Option::is_some));
        let stats = LatencyStats::new();
        stats.record(Duration::from_micros(700));
        stats.record(Duration::from_micros(700));
        stats.record(Duration::ZERO);
        let summary = stats.snapshot();
        assert_eq!(summary.count, 3);
        assert_eq!(summary.sum_us, 1400);
        assert_eq!(summary.sum_ms, 1);
        let mut ack = receipts[8].unwrap();
        ack.sequence = 2;
        record_qos_receipt(&mut receipts, ack).unwrap();
        ack.sequence = 1;
        record_qos_receipt(&mut receipts, ack).unwrap();
        assert_eq!(receipts[8].unwrap().sequence, 2);
        ack.worker_slot = 10;
        assert_eq!(
            record_qos_receipt(&mut receipts, ack).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let (tx, rx) = crossbeam_channel::bounded(1);
        let mut reply = PendingReply {
            gpu_done: false,
            source_done: false,
            encoding_ready: false,
            receiver: rx,
            id: 1,
            qos_revision: 3,
            deadline: Instant::now(),
            background_deadline: Instant::now(),
        };
        assert_eq!(
            reply.poll().unwrap().err().unwrap().kind(),
            io::ErrorKind::TimedOut
        );
        reply.deadline = Instant::now() + Duration::from_secs(2);
        // 编码就绪不能越过仍在读取/变换的阶段而提前让出工作集。
        tx.send(Ok((
            Instant::now(),
            NativeResponse::Stage {
                id: 1,
                stage: NativeStage::EncodingReady,
            },
        )))
        .unwrap();
        assert_eq!(
            reply.poll().unwrap().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(!reply.encoding_ready);
        let mut bytes = Vec::new();
        super::super::native_protocol::write_response(
            &mut bytes,
            &NativeResponse::Stage {
                id: 1,
                stage: NativeStage::GpuDone,
            },
        )
        .unwrap();
        let stage = read_response(&bytes[..]).unwrap();
        tx.send(Ok((Instant::now(), stage))).unwrap();
        assert!(reply.poll().is_none());
        assert!(reply.gpu_done && !reply.source_done);
        tx.send(Ok((
            Instant::now(),
            NativeResponse::Stage {
                id: 1,
                stage: NativeStage::SourceDone,
            },
        )))
        .unwrap();
        assert!(reply.poll().is_none());
        assert!(reply.source_done);
        assert!(!reply.encoding_ready, "源文件已读完不等于大图缩放已完成");
        bytes.clear();
        super::super::native_protocol::write_response(
            &mut bytes,
            &NativeResponse::Stage {
                id: 1,
                stage: NativeStage::EncodingReady,
            },
        )
        .unwrap();
        tx.send(Ok((Instant::now(), read_response(&bytes[..]).unwrap())))
            .unwrap();
        assert!(reply.poll().is_none());
        assert!(reply.encoding_ready);
        tx.send(Ok((
            Instant::now(),
            NativeResponse::Stage {
                id: 1,
                stage: NativeStage::EncodingReady,
            },
        )))
        .unwrap();
        assert_eq!(
            reply.poll().unwrap().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        // 重复阶段不能伪装成正常完成。
        tx.send(Ok((
            Instant::now(),
            NativeResponse::Stage {
                id: 1,
                stage: NativeStage::SourceDone,
            },
        )))
        .unwrap();
        assert_eq!(
            reply.poll().unwrap().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );

        tx.send(Ok((
            Instant::now(),
            NativeResponse::Failed {
                id: 2,
                code: 7,
                qos: NativeQosAck {
                    worker_slot: 1,
                    sequence: 1,
                    revision: 3,
                    applied: true,
                    attempted: true,
                },
            },
        )))
        .unwrap();
        assert_eq!(
            reply.poll().unwrap().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    #[ignore = "requires SCROLLERY_TEST_NATIVE_WORKER and at least five fast slots"]
    fn real_worker_multislot_replies_keep_reader_alive() {
        use super::super::native_protocol::NativeKind;
        use std::os::windows::process::CommandExt;

        if let Ok(json) = std::env::var("SCROLLERY_TEST_THUMB_LIMITS") {
            super::super::limits::install(serde_json::from_str(&json).unwrap()).unwrap();
        }
        let capacity = super::super::qos::native_fast_parallelism().min(9);
        assert!(
            capacity >= 5,
            "requires a machine supporting slots above four"
        );
        let exe = std::env::var_os("SCROLLERY_TEST_NATIVE_WORKER")
            .expect("set the explicit native worker executable path");
        let mut command = Command::new(exe);
        command
            .arg(format!("--fast={capacity}"))
            .arg(format!(
                "--thumbnail-limits={}",
                serde_json::to_string(super::super::limits::get()).unwrap()
            ))
            .creation_flags(0x08000000);
        let process =
            WorkerProcess::spawn_command(command, capacity, Arc::new(WorkerJob::new(8).unwrap()))
                .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let sample = std::env::var_os("SCROLLERY_TEST_NATIVE_SAMPLE");
        let gpu_stress = sample.is_some();
        let path = sample.map(std::path::PathBuf::from).unwrap_or_else(|| {
            let path = temp.path().join("sample.jpg");
            image::DynamicImage::new_rgb8(128, 256).save(&path).unwrap();
            path
        });
        let output_size = if gpu_stress { 512 } else { 64 };
        let batch_size = if gpu_stress {
            capacity.min(6)
        } else {
            capacity
        };
        let wake = Arc::new(Condvar::new());
        let mut receipts = vec![None; capacity];
        // 同一真实进程分别覆盖每个槽的成功和格式失败回包，经过生产 reader / pending / QoS 出口。
        for format in ["jpg", "protocol_probe"] {
            let mut seen = HashSet::new();
            let mut completed = 0;
            let mut vpl_completed = 0;
            let mut peak_private = 0;
            let started = Instant::now();
            for _ in 0..32 {
                let mut replies: Vec<_> = (0..batch_size)
                    .map(|_| {
                        // DuplicateHandle 共享文件游标；每项独立打开，保持与生产派发一致。
                        let file = File::open(&path).unwrap();
                        let gpu = gpu_stress
                            .then(|| crate::engine::gpu::budget::try_acquire(true))
                            .flatten();
                        let reply = process
                            .request(
                                &file,
                                NativeRequest {
                                    id: 0,
                                    file_handle: 0,
                                    kind: NativeKind::Image,
                                    image_route:
                                        super::super::native_protocol::NativeImageRoute::Automatic,
                                    format: format.into(),
                                    codec_hint: None,
                                    prefer_gpu: gpu.is_some(),
                                    gpu_policy: gpu_stress,
                                    decode_long_edge: output_size,
                                    max_pixel_bytes: 16 * 1024 * 1024,
                                    output_size,
                                    webp_quality: 80,
                                    ai_cache_short_edge: 336,
                                    emit_ai_cache: false,
                                    qos_foreground: true,
                                    qos_revision: 1,
                                },
                                Duration::from_secs(10),
                                Duration::from_secs(10),
                                Arc::clone(&wake),
                            )
                            .unwrap();
                        (reply, gpu)
                    })
                    .collect();
                for (reply, gpu) in &mut replies {
                    let (outcome, qos) = loop {
                        if let Some(memory) = process.memory_snapshot() {
                            peak_private = peak_private.max(memory.private_bytes);
                        }
                        if let Some(result) = reply.poll() {
                            if let Err(error) = &result {
                                panic!("real worker failed: {error}; peak_private={peak_private}; exit={:?}", process.child.lock().unwrap().try_wait().unwrap());
                            }
                            break result.unwrap();
                        }
                        if reply.gpu_done {
                            drop(gpu.take());
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    };
                    drop(gpu.take());
                    record_qos_receipt(&mut receipts, qos).unwrap();
                    seen.insert(qos.worker_slot);
                    if format == "jpg" {
                        let (payload, execution, _) = outcome.unwrap();
                        if gpu_stress {
                            vpl_completed +=
                                usize::from(execution.backend == NativeBackend::ImageVpl);
                        } else {
                            assert_eq!(execution.backend, NativeBackend::ImageRs);
                        }
                        let decoded = image::load_from_memory(&payload.webp).unwrap();
                        if gpu_stress {
                            assert_eq!(decoded.width().max(decoded.height()), output_size);
                        } else {
                            assert_eq!((decoded.width(), decoded.height()), (32, 64));
                        }
                    } else {
                        assert!(matches!(outcome, Err(3)));
                    }
                    completed += 1;
                }
                assert!(process.alive.load(Ordering::Acquire));
                if seen.len() == capacity && !gpu_stress {
                    break;
                }
            }
            assert_eq!(seen.len(), capacity, "all slots must reply for {format}");
            if gpu_stress && format == "jpg" {
                assert!(vpl_completed > 0, "real VPL route required");
            }
            eprintln!("real worker format={format} slots={seen:?} reader_alive=true completed={completed} vpl={vpl_completed} elapsed_ms={} peak_private={peak_private}", started.elapsed().as_millis());
        }
        assert!(process.pending.lock().unwrap().is_empty());
        assert!(receipts.iter().all(Option::is_some));
    }
}
