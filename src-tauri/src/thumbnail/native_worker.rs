//! Windows 图片隔离域：异步有界派发，故障先终止并 wait，再释放在途资源。

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender};
use tokio_util::sync::CancellationToken;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows::Win32::System::ProcessStatus::{
    GetPerformanceInfo, GetProcessMemoryInfo, PERFORMANCE_INFORMATION, PROCESS_MEMORY_COUNTERS,
    PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{GetCurrentProcess, CREATE_NO_WINDOW};

use super::native_protocol::{
    read_reply_with_limit, write_request, NativeEvent, NativeFailure, NativeReply, NativeRequest,
    NativeStats, HELLO, MAX_ARTIFACT_BYTES,
};
use super::wake::Wake;

const MIB: u64 = 1024 * 1024;
// 每个订阅保留有界协议头和通道控制开销，零载荷请求也不能无界积累 ticket。
const REPLY_OVERHEAD: u64 = 4096;
const POLL: Duration = Duration::from_millis(50);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

struct Settings {
    wake: Wake,
    executable: Option<PathBuf>,
    width: usize,
    capacity: usize,
    memory_bytes: u64,
    output_budget: u64,
    handshake_timeout: Duration,
    request_timeout: Duration,
    sample_interval: Duration,
    #[cfg(test)]
    stub_mode: Option<String>,
    #[cfg(test)]
    last_spawned_pid: AtomicUsize,
}

impl Settings {
    fn new(width: usize) -> Self {
        let mut info = PERFORMANCE_INFORMATION::default();
        let size = std::mem::size_of_val(&info) as u32;
        info.cb = size;
        // 一次读取机器物理内存以选择启动默认值；不把系统查询放到逐图路径。
        let physical = unsafe { GetPerformanceInfo(&mut info, size) }
            .map(|_| (info.PhysicalTotal as u64).saturating_mul(info.PageSize as u64))
            .unwrap_or(4 * 1024 * MIB);
        let memory_bytes = (physical / 8).clamp(512 * MIB, 2 * 1024 * MIB);
        // 项数与字节预算独立：小图不按每项协议最大 64MiB 限成贯穿编解码的小槽。
        let output_budget = (physical / 32).clamp(256 * MIB, 512 * MIB);
        let capacity = width.max(1).saturating_mul(4);
        Self {
            wake: Wake::default(),
            executable: std::env::current_exe()
                .ok()
                .map(|path| path.with_file_name("native-thumbnail-worker.exe")),
            width: width.max(1),
            capacity,
            memory_bytes,
            output_budget,
            handshake_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(120),
            sample_interval: Duration::from_millis(250),
            #[cfg(test)]
            stub_mode: None,
            #[cfg(test)]
            last_spawned_pid: AtomicUsize::new(0),
        }
    }
}

struct Admission {
    capacity: usize,
    used: AtomicUsize,
    output_budget: u64,
    bytes: AtomicU64,
    completed: AtomicU64,
    bytes_peak: AtomicU64,
    private_peak: AtomicU64,
    stats: Mutex<NativeStats>,
}

impl Admission {
    fn reserve(
        self: &Arc<Self>,
        bytes: u64,
    ) -> Result<(RunningPermit, Arc<BytePermit>), NativeFailure> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < self.capacity).then_some(used + 1)
            })
            .map_err(|_| NativeFailure::Resource)?;
        let running = RunningPermit(Arc::clone(self));
        let bytes = bytes
            .checked_add(REPLY_OVERHEAD)
            .ok_or(NativeFailure::Resource)?;
        let previous = self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= self.output_budget)
            })
            .map_err(|_| NativeFailure::Resource)?;
        self.bytes_peak
            .fetch_max(previous + bytes, Ordering::Relaxed);
        Ok((
            running,
            Arc::new(BytePermit {
                admission: Arc::clone(self),
                bytes: AtomicU64::new(bytes),
            }),
        ))
    }
}

struct RunningPermit(Arc<Admission>);
impl Drop for RunningPermit {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::AcqRel);
    }
}
struct BytePermit {
    admission: Arc<Admission>,
    bytes: AtomicU64,
}
impl BytePermit {
    fn shrink(&self, payload: u64) {
        let next = payload.saturating_add(REPLY_OVERHEAD);
        // Pending/Ticket 共用同一许可；原子扣除差额，最后一个 Arc 才归还剩余量。
        if let Ok(previous) =
            self.bytes
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bytes| {
                    (next < bytes).then_some(next)
                })
        {
            self.admission
                .bytes
                .fetch_sub(previous - next, Ordering::AcqRel);
        }
    }
}
impl Drop for BytePermit {
    fn drop(&mut self) {
        self.admission
            .bytes
            .fetch_sub(*self.bytes.get_mut(), Ordering::AcqRel);
    }
}

/// 订阅的输出额度随 ticket 保留；移交字节到下游预算后才能 drop。
/// drop 只取消该订阅，已交给 worker 的句柄等真实完成或进程退出后才归还额度。
pub(crate) struct NativeTicket {
    receiver: Receiver<NativeReply>,
    cancel: CancellationToken,
    _bytes: Arc<BytePermit>,
    // 字段顺序保证字节额度先归还，再通知被输出背压阻挡的派发者。
    _wake: WakeOnDrop,
}

struct WakeOnDrop(Wake);
impl Drop for WakeOnDrop {
    fn drop(&mut self) {
        self.0.notify();
    }
}

impl NativeTicket {
    /// 顺序接收 Decoded 与 Finished；Finished 是该订阅唯一终态。
    pub(crate) fn receiver(&self) -> &Receiver<NativeReply> {
        &self.receiver
    }
}
impl Drop for NativeTicket {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

struct Slot {
    process: Option<Arc<Process>>,
    faults: u8,
    last_idle_recycle: Option<u64>,
}

/// 单一原生域，发送与收取在专用线程运行；调用者不占据贯穿编解码的槽。
pub(crate) struct NativeExecutor {
    settings: Arc<Settings>,
    admission: Arc<Admission>,
    slot: Mutex<Slot>,
}

/// 宿主与执行域累计峰值；private 采用已有周期采样，不增加逐图系统查询。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NativeMetrics {
    pub stages: NativeStats,
    pub worker_private_bytes_peak: u64,
    pub host_output_bytes_peak: u64,
}

impl NativeExecutor {
    /// 返回此 executor 跨故障恢复域累计的峰值快照。
    pub(crate) fn metrics(&self) -> NativeMetrics {
        NativeMetrics {
            stages: *lock(&self.admission.stats),
            worker_private_bytes_peak: self.admission.private_peak.load(Ordering::Relaxed),
            host_output_bytes_peak: self.admission.bytes_peak.load(Ordering::Relaxed),
        }
    }
    /// 懒启动位于宿主可执行文件同目录的 worker；启动失败由 submit 返回。
    pub(crate) fn new(width: usize, wake: Wake) -> Arc<Self> {
        let mut settings = Settings::new(width);
        settings.wake = wake;
        Self::with_settings(settings)
    }

    fn with_settings(settings: Settings) -> Arc<Self> {
        Arc::new(Self {
            admission: Arc::new(Admission {
                capacity: settings.capacity,
                used: AtomicUsize::new(0),
                output_budget: settings.output_budget,
                bytes: AtomicU64::new(0),
                completed: AtomicU64::new(0),
                bytes_peak: AtomicU64::new(0),
                private_peak: AtomicU64::new(0),
                stats: Mutex::new(NativeStats::default()),
            }),
            settings: Arc::new(settings),
            slot: Mutex::new(Slot {
                process: None,
                faults: 0,
                last_idle_recycle: None,
            }),
        })
    }

    /// 有界准入后立刻返回订阅，不在调用线程写 stdin 或等待图片处理。
    /// Resource 表示暂时背压；Unavailable 表示执行域故障，均不得登记为坏图。
    pub(crate) fn submit(
        &self,
        request: NativeRequest,
        file: File,
        cancel: &CancellationToken,
    ) -> Result<NativeTicket, NativeFailure> {
        if cancel.is_cancelled() {
            return Err(NativeFailure::Cancelled);
        }
        if request.max_encoded_bytes > MAX_ARTIFACT_BYTES as u64
            || ((request.emit_display || request.emit_ai) && request.max_encoded_bytes == 0)
        {
            return Err(NativeFailure::Resource);
        }
        let (running, bytes) = self.admission.reserve(request.max_encoded_bytes)?;
        let process = self.process(cancel)?;
        if process.paused.load(Ordering::Acquire) {
            return Err(NativeFailure::Resource);
        }
        if cancel.is_cancelled() {
            return Err(NativeFailure::Cancelled);
        }
        let cancel = cancel.child_token();
        let (sender, receiver) = bounded(2);
        let id = request.id;
        let mut pending = lock(&process.pending);
        if !process.running.load(Ordering::Acquire) {
            return Err(NativeFailure::Unavailable);
        }
        if pending.contains_key(&id) {
            return Err(NativeFailure::Resource);
        }
        pending.insert(
            id,
            Pending {
                sender,
                wake: self.settings.wake.clone(),
                cancel: cancel.clone(),
                _bytes: Arc::clone(&bytes),
                _running: running,
                output_limit: request.max_encoded_bytes as usize,
                emit_display: request.emit_display,
                emit_ai: request.emit_ai,
                submitted: Instant::now(),
                decoded: false,
            },
        );
        // 锁内只做非阻塞入队；writer 从不在 pending 锁内执行系统调用或管道写入。
        let queued = lock(&process.writer)
            .as_ref()
            .is_some_and(|writer| writer.try_send(WriteWork { request, file }).is_ok());
        if !queued {
            pending.remove(&id);
            return Err(NativeFailure::Unavailable);
        }
        Ok(NativeTicket {
            receiver,
            cancel,
            _bytes: bytes,
            _wake: WakeOnDrop(self.settings.wake.clone()),
        })
    }

    fn process(&self, cancel: &CancellationToken) -> Result<Arc<Process>, NativeFailure> {
        let mut slot = lock(&self.slot);
        if let Some(process) = slot.process.as_ref() {
            if process.running.load(Ordering::Acquire) && !process.fault.load(Ordering::Acquire) {
                return Ok(Arc::clone(process));
            }
        }
        if let Some(process) = slot.process.take() {
            // stop 会 wait 并 join IO 线程；旧域完全退出前不能派生新域或释放旧配额。
            process.stop();
            if !process.reaped.load(Ordering::Acquire) {
                slot.process = Some(process);
                return Err(NativeFailure::Unavailable);
            }
            if process.recycle.load(Ordering::Acquire) && !process.fault.load(Ordering::Acquire) {
                let progress = self.admission.completed.load(Ordering::Acquire);
                if slot.last_idle_recycle == Some(progress) {
                    slot.faults = 2; // 没有完成任何任务却连续超水位，不得无限重启。
                }
                slot.last_idle_recycle = Some(progress);
            } else {
                slot.faults += 1;
            }
        }
        if slot.faults >= 2 {
            return Err(NativeFailure::Unavailable);
        }
        match Process::spawn(
            Arc::clone(&self.settings),
            Arc::clone(&self.admission),
            cancel,
        ) {
            Ok(process) => {
                slot.process = Some(Arc::clone(&process));
                Ok(process)
            }
            Err(failure) => {
                if failure != NativeFailure::Cancelled {
                    slot.faults += 1;
                }
                Err(failure)
            }
        }
    }
}

impl Drop for NativeExecutor {
    fn drop(&mut self) {
        if let Some(process) = lock(&self.slot).process.take() {
            process.stop();
        }
    }
}

struct Job(OwnedHandle);
impl Job {
    fn new(memory_bytes: u64) -> io::Result<Self> {
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(io::Error::other)?;
        // 新建的匿名 Job 不可继承，仅由宿主持有；关闭时内核终止整个进程树。
        let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.JobMemoryLimit = memory_bytes as usize;
        limits.ProcessMemoryLimit = memory_bytes as usize;
        unsafe {
            SetInformationJobObject(
                HANDLE(owned.as_raw_handle()),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Self(owned))
    }
    fn assign(&self, child: &Child) -> io::Result<()> {
        unsafe {
            AssignProcessToJobObject(
                HANDLE(self.0.as_raw_handle()),
                HANDLE(child.as_raw_handle()),
            )
        }
        .map_err(io::Error::other)
    }
    fn terminate(&self) {
        // 同时关闭后代持有的管道，防止只杀主进程后 reader/writer 永久阻塞。
        let _ = unsafe { TerminateJobObject(HANDLE(self.0.as_raw_handle()), 1) };
    }
}

struct Pending {
    sender: Sender<NativeReply>,
    wake: Wake,
    cancel: CancellationToken,
    _bytes: Arc<BytePermit>,
    _running: RunningPermit,
    output_limit: usize,
    emit_display: bool,
    emit_ai: bool,
    submitted: Instant,
    decoded: bool,
}
impl Pending {
    fn finish(self, reply: NativeReply) -> bool {
        let Self {
            sender,
            wake,
            _bytes,
            _running,
            ..
        } = self;
        _bytes.shrink(reply.display.capacity().saturating_add(reply.ai.capacity()) as u64);
        // 终态一旦可见，消费者就可能立即提交下一项；运行许可必须先归还。
        // 输出额度的另一份 Arc 仍由 ticket 持有，直到落盘消费者结束。
        drop(_running);
        drop(_bytes);
        let delivered = !matches!(
            sender.try_send(reply),
            Err(crossbeam_channel::TrySendError::Full(_))
        );
        wake.notify();
        delivered
    }
}
struct WriteWork {
    request: NativeRequest,
    file: File,
}

struct Process {
    child: Mutex<Child>,
    job: Job,
    writer: Mutex<Option<Sender<WriteWork>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    pending: Mutex<HashMap<u64, Pending>>,
    running: AtomicBool,
    fault: AtomicBool,
    paused: AtomicBool,
    recycle: AtomicBool,
    reaped: AtomicBool,
    admission: Arc<Admission>,
}

impl Process {
    fn spawn(
        settings: Arc<Settings>,
        admission: Arc<Admission>,
        cancel: &CancellationToken,
    ) -> Result<Arc<Self>, NativeFailure> {
        let job = Job::new(settings.memory_bytes).map_err(|_| NativeFailure::Unavailable)?;
        let mut command = Command::new(
            settings
                .executable
                .as_ref()
                .ok_or(NativeFailure::Unavailable)?,
        );
        command
            .arg(format!("--threads={}", settings.width))
            .arg(format!("--memory-bytes={}", settings.memory_bytes))
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(test)]
        if let Some(mode) = &settings.stub_mode {
            command.arg(format!("--stub={mode}"));
        }
        let mut child = command.spawn().map_err(|_| NativeFailure::Unavailable)?;
        #[cfg(test)]
        settings
            .last_spawned_pid
            .store(child.id() as usize, Ordering::Release);
        if job.assign(&child).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NativeFailure::Unavailable);
        }
        // 尚未发送授权句柄；Job 生效且握手完成后才允许图片进入执行域。
        let mut input = child.stdin.take().expect("piped stdin");
        let mut output = child.stdout.take().expect("piped stdout");
        let (write_tx, write_rx) = bounded::<WriteWork>(settings.capacity);
        let process = Arc::new(Self {
            child: Mutex::new(child),
            job,
            writer: Mutex::new(Some(write_tx)),
            threads: Mutex::new(Vec::new()),
            pending: Mutex::new(HashMap::new()),
            running: AtomicBool::new(true),
            fault: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            recycle: AtomicBool::new(false),
            reaped: AtomicBool::new(false),
            admission,
        });
        let (hello_tx, hello_rx) = bounded(1);
        let weak = Arc::downgrade(&process);
        let reader = std::thread::Builder::new()
            .name("native-image-reader".into())
            .spawn(move || {
                let mut hello = [0; HELLO.len()];
                let valid = output.read_exact(&mut hello).is_ok() && hello == HELLO;
                let _ = hello_tx.try_send(valid);
                if !valid {
                    return;
                }
                loop {
                    // header 查到的 pending 已持有输出 permit，才允许协议分配实际载荷。
                    // 不持任何锁等待管道读；旧域的 pending 会在 join reader 后才清空。
                    let reply = read_reply_with_limit(&mut output, |event| {
                        let process = weak.upgrade().ok_or(io::ErrorKind::BrokenPipe)?;
                        process.reply_limit(event)
                    });
                    let Some(process) = weak.upgrade() else {
                        return;
                    };
                    if !process.running.load(Ordering::Acquire) {
                        return;
                    }
                    let delivered = match reply {
                        Ok(reply) => process.deliver(reply),
                        Err(_) => false,
                    };
                    if !delivered {
                        process.fault.store(true, Ordering::Release);
                        return;
                    }
                }
            });
        match reader {
            Ok(reader) => lock(&process.threads).push(reader),
            Err(_) => {
                process.stop();
                return Err(NativeFailure::Unavailable);
            }
        }
        let weak = Arc::downgrade(&process);
        let writer = std::thread::Builder::new()
            .name("native-image-writer".into())
            .spawn(move || {
                while let Ok(mut work) = write_rx.recv() {
                    let Some(process) = weak.upgrade() else {
                        return;
                    };
                    if !process.running.load(Ordering::Acquire) {
                        return;
                    }
                    let cancelled = lock(&process.pending)
                        .get(&work.request.id)
                        .is_none_or(|pending| pending.cancel.is_cancelled());
                    if cancelled {
                        process.finish_unsent(work.request.id, NativeFailure::Cancelled);
                        continue;
                    }
                    let copied = process.duplicate(&work.file);
                    let handle = match copied {
                        Ok(handle) => handle,
                        Err(_) => {
                            process.finish_unsent(work.request.id, NativeFailure::Resource);
                            continue;
                        }
                    };
                    work.request.file_handle = handle;
                    // 管道写入不持 child/pending 锁；监控能随时终止 Job 打断 backpressure。
                    if write_request(&mut input, &work.request).is_err() {
                        // 副本可能已被消费，不在宿主猜测关闭远端句柄；终止域后由内核统一回收。
                        process.fault.store(true, Ordering::Release);
                        return;
                    }
                }
            });
        match writer {
            Ok(writer) => lock(&process.threads).push(writer),
            Err(_) => {
                process.stop();
                return Err(NativeFailure::Unavailable);
            }
        }
        let deadline = Instant::now() + settings.handshake_timeout;
        loop {
            if cancel.is_cancelled() {
                process.stop();
                return Err(NativeFailure::Cancelled);
            }
            if Instant::now() >= deadline {
                process.stop();
                return Err(NativeFailure::Unavailable);
            }
            match hello_rx
                .recv_timeout(POLL.min(deadline.saturating_duration_since(Instant::now())))
            {
                Ok(true) => break,
                Ok(false) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    process.stop();
                    return Err(NativeFailure::Unavailable);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        }
        let weak = Arc::downgrade(&process);
        let monitor = std::thread::Builder::new()
            .name("native-image-monitor".into())
            .spawn(move || {
                let mut sampled = Instant::now();
                loop {
                    std::thread::sleep(POLL);
                    let Some(process) = weak.upgrade() else {
                        return;
                    };
                    if !process.running.load(Ordering::Acquire) {
                        return;
                    }
                    let timed_out = lock(&process.pending)
                        .values()
                        .any(|pending| pending.submitted.elapsed() >= settings.request_timeout);
                    if timed_out {
                        process.fault.store(true, Ordering::Release);
                    }
                    if process.fault.load(Ordering::Acquire) {
                        process.stop();
                        return;
                    }
                    if sampled.elapsed() >= settings.sample_interval {
                        sampled = Instant::now();
                        let (exited, private) = {
                            let mut child = lock(&process.child);
                            (
                                child.try_wait().map_or(true, |status| status.is_some()),
                                private_bytes(&child),
                            )
                        };
                        if exited {
                            process.fault.store(true, Ordering::Release);
                            process.stop();
                            return;
                        }
                        if let Some(private) = private {
                            process
                                .admission
                                .private_peak
                                .fetch_max(private, Ordering::Relaxed);
                            let high = private >= settings.memory_bytes * 4 / 5;
                            if process.paused.swap(high, Ordering::AcqRel) != high {
                                settings.wake.notify();
                            }
                            if high && lock(&process.pending).is_empty() {
                                process.recycle.store(true, Ordering::Release);
                                process.stop();
                                return;
                            }
                        } else {
                            // 监控失效时不能继续接受无资源观测的执行域；先 kill+wait 再归还 pending。
                            process.fault.store(true, Ordering::Release);
                            process.stop();
                            return;
                        }
                    }
                }
            });
        if monitor.is_err() {
            process.stop();
            return Err(NativeFailure::Unavailable);
        }
        Ok(process)
    }

    fn duplicate(&self, file: &File) -> io::Result<u64> {
        let child = lock(&self.child);
        if !self.running.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut target = HANDLE::default();
        // 唯一目标是指定 child；新句柄不可继承，源 File 在复制失败/成功后均正常释放。
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(file.as_raw_handle()),
                HANDLE(child.as_raw_handle()),
                &mut target,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }
        .map_err(io::Error::other)?;
        Ok(target.0 as usize as u64)
    }

    fn finish_unsent(&self, id: u64, failure: NativeFailure) {
        if let Some(pending) = lock(&self.pending).remove(&id) {
            pending.finish(failure_reply(id, failure));
        }
    }

    fn reply_limit(&self, event: &NativeEvent) -> io::Result<usize> {
        let pending = lock(&self.pending);
        if !self.running.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let id = match event {
            NativeEvent::Decoded { id } | NativeEvent::Finished { id, .. } => *id,
        };
        let waiter = pending
            .get(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown native reply id"))?;
        let valid = match event {
            NativeEvent::Decoded { .. } => !waiter.decoded,
            NativeEvent::Finished {
                failure: None,
                display_len,
                ai_len,
                ..
            } => {
                waiter.decoded
                    && (*display_len > 0) == waiter.emit_display
                    && (*ai_len > 0) == waiter.emit_ai
            }
            NativeEvent::Finished { .. } => true,
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native reply contract mismatch",
            ));
        }
        Ok(waiter.output_limit)
    }

    fn deliver(&self, mut reply: NativeReply) -> bool {
        let mut pending = lock(&self.pending);
        if !self.running.load(Ordering::Acquire) {
            return true;
        }
        let id = match reply.event {
            NativeEvent::Decoded { id } | NativeEvent::Finished { id, .. } => id,
        };
        let Some(waiter) = pending.get_mut(&id) else {
            return false;
        };
        let finished = matches!(reply.event, NativeEvent::Finished { .. });
        let progressed = matches!(
            &reply.event,
            NativeEvent::Finished {
                failure: None | Some(NativeFailure::Content | NativeFailure::Unsupported),
                ..
            }
        );
        match &reply.event {
            NativeEvent::Decoded { .. } if waiter.decoded => return false,
            NativeEvent::Decoded { .. } => waiter.decoded = true,
            NativeEvent::Finished { failure: None, .. } if !waiter.decoded => return false,
            _ => {}
        }
        if let NativeEvent::Finished { stats, .. } = &reply.event {
            lock(&self.admission.stats).merge(*stats);
        }
        if finished && waiter.cancel.is_cancelled() {
            reply = failure_reply(id, NativeFailure::Cancelled);
        }
        if finished {
            let waiter = pending.remove(&id).expect("validated pending reply");
            if progressed {
                self.admission.completed.fetch_add(1, Ordering::AcqRel);
            }
            return waiter.finish(reply);
        }
        if !waiter.cancel.is_cancelled() {
            // 两事件槽覆盖完整订阅；断开的消费者等同取消，不能杀掉其它订阅。
            if matches!(
                waiter.sender.try_send(reply),
                Err(crossbeam_channel::TrySendError::Full(_))
            ) {
                return false;
            }
            waiter.wake.notify();
        }
        true
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Release);
        // child 锁串行化并发 stop；不能在确认退出前清空 pending。
        let mut child = lock(&self.child);
        self.job.terminate();
        let _ = child.kill();
        if child.wait().is_err() {
            return;
        }
        self.reaped.store(true, Ordering::Release);
        drop(child);
        lock(&self.writer).take();
        // 杀进程树后管道写和读都会退出；join 后才允许替换域及 reader 预算。
        for thread in lock(&self.threads).drain(..) {
            let _ = thread.join();
        }
        for (id, pending) in lock(&self.pending).drain() {
            let failure = if pending.cancel.is_cancelled() {
                NativeFailure::Cancelled
            } else {
                NativeFailure::Unavailable
            };
            pending.finish(failure_reply(id, failure));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // 正常所有者会先 stop；构造中途的任何返回同样不能遗留子进程。
        if !self.reaped.load(Ordering::Acquire) {
            self.stop();
        }
    }
}

fn private_bytes(child: &Child) -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    counters.cb = std::mem::size_of_val(&counters) as u32;
    unsafe {
        GetProcessMemoryInfo(
            HANDLE(child.as_raw_handle()),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }
    .ok()?;
    Some(counters.PrivateUsage as u64)
}

fn failure_reply(id: u64, failure: NativeFailure) -> NativeReply {
    NativeReply {
        event: NativeEvent::Finished {
            id,
            failure: Some(failure),
            backend: String::new(),
            decode_us: 0,
            encode_us: 0,
            display_len: 0,
            ai_len: 0,
            thumbhash: None,
            stats: Default::default(),
            gpu: None,
        },
        display: Vec::new(),
        ai: Vec::new(),
    }
}
