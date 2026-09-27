//! Windows 原生缩略图执行域：fast 有限并行、tail 串行；超时终止并确认子进程退出。

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
    NativeResponse, NativeTimings, WORKER_HELLO,
};

use super::qos::{native_timeout, SourceMedia};
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WORKER_JOB_MEMORY_BYTES: usize = 512 * 1024 * 1024;
const WORKER_PROCESS_MEMORY_BYTES: usize = 384 * 1024 * 1024;

type ReportedDevice = (NativeBackend, u32, u32, i32, u32);
type NativeReply = std::result::Result<(EncodedThumbPayload, NativeExecution, NativeTimings), u8>;

pub struct NativeWorkers {
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
        limits.JobMemoryLimit = WORKER_JOB_MEMORY_BYTES;
        limits.ProcessMemoryLimit = WORKER_PROCESS_MEMORY_BYTES;
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

impl Drop for NativePending {
    fn drop(&mut self) {
        // 宿主退出或放弃在途项时，先终止并确认退出，再归还域额度。
        if !self.completed {
            self.process.terminate();
        }
    }
}

struct PendingReply {
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

    fn poll(&self) -> Option<io::Result<(NativeReply, NativeQosAck)>> {
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
        Some(response.and_then(|(arrived, response)| {
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
        }))
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
            fast: Domain::new(true, fast_capacity.clamp(1, 4), Arc::clone(&job)),
            tail: Domain::new(false, 1, job),
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
            gpu_limit: crate::engine::gpu::budget::GPU_INFLIGHT_LIMIT,
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
        if matches!(lane, ThumbnailLane::Fast | ThumbnailLane::ViewportFast) {
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
        let timeout = native_timeout(domain.fast, media, request.qos_foreground);
        let background_timeout = native_timeout(domain.fast, media, false);
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
        let response = pending.reply.poll()?;
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
                self.failed_responses.fetch_add(1, Ordering::Relaxed);
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
                    mf_reader_timeout_exit = exit_code == Some(70),
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
            job_memory_limit_bytes = WORKER_JOB_MEMORY_BYTES,
            process_memory_limit_bytes = WORKER_PROCESS_MEMORY_BYTES,
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
            while let Ok(response) = read_response(&mut stdout) {
                let id = match &response {
                    NativeResponse::Ok { id, .. } | NativeResponse::Failed { id, .. } => *id,
                };
                let waiter = reader_pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                if let Some((waiter, wake)) = waiter {
                    let _ = waiter.send(Ok((Instant::now(), response)));
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
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
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
        // 已自行退出时保留真实状态；MF弃用超时reader使用70，不能只留下管道失联。
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
        let _ = waiter.send(Err(io::Error::new(
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
            r#"$out=[Console]::OpenStandardOutput();$out.Write([byte[]](78,84,72,68),0,4);$out.Flush();$null=[Console]::OpenStandardInput().ReadByte();{};Start-Sleep -Seconds 15"#,
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
        let (tx, rx) = crossbeam_channel::bounded(1);
        let mut reply = PendingReply {
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
    fn domain_reservations_are_independent_and_release() {
        let workers = NativeWorkers::new(1);
        let tail = workers.try_reserve(ThumbnailLane::Heavy).unwrap();
        assert!(workers.try_reserve(ThumbnailLane::ViewportHeavy).is_none());
        let fast = workers.try_reserve(ThumbnailLane::ViewportFast).unwrap();
        assert_eq!(workers.available(), (false, false));
        drop(fast);
        assert_eq!(workers.available(), (true, false));
        drop(tail);
        assert_eq!(workers.available(), (true, true));
    }
}
