//! Windows 隔离进程的 D/E 双池；源句柄、像素和编码字节分别交接所有权。
#![cfg(windows)]

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, SendTimeoutError, Sender};
use windows::Win32::Graphics::Imaging::WICDecodeMetadataCacheOnDemand;
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::GetCurrentProcess;

use super::byte_budget::{ByteBudget, BytePermit};
use super::image_payload::{encode_image_payload, ThumbConfig};
use super::native_protocol::{
    read_request, write_reply, NativeDecodePolicy, NativeEvent, NativeFailure, NativeGpuMetrics,
    NativeReply, NativeRequest, NativeStats, HELLO,
};
use super::qos::{refresh_worker_qos, set_app_foreground};
use crate::engine::gpu::wic_engine::{
    decode_frame, frame_icc_context, wic_error, with_image_worker_session, with_wic_factory,
};
use crate::engine::image_rs::{pixel_limit, resize_dimensions, ImageRsEngine};
use crate::engine::traits::ResizeHint;
use crate::engine::{DecodedImage, ImageEngine};
use crate::error::AppError;

const MIB: u64 = 1024 * 1024;
const ICC_LIMIT: u64 = 16 * MIB;
const HASH_BYTES: u64 = 128;
const TICK: Duration = Duration::from_millis(50);

#[path = "embedded_jpeg.rs"]
mod embedded_jpeg;

#[path = "native_gpu.rs"]
mod native_gpu;

type CpuDecoded = (DecodedImage, &'static str, BytePermit);

struct EncodePixels {
    decoded: DecodedImage,
    thumbhash_input: Option<DecodedImage>,
    backend: &'static str,
    working: BytePermit,
}

impl From<CpuDecoded> for EncodePixels {
    fn from((decoded, backend, working): CpuDecoded) -> Self {
        Self {
            decoded,
            thumbhash_input: None,
            backend,
            working,
        }
    }
}

enum DecodeOutput {
    Cpu(CpuDecoded),
    Gpu(native_gpu::Prepared),
}

struct DecodeJob {
    request: NativeRequest,
    file: File,
}
struct EncodeJob {
    request: NativeRequest,
    decoded: DecodedImage,
    thumbhash_input: Option<DecodedImage>,
    backend: &'static str,
    decode_us: u64,
    gpu: Option<NativeGpuMetrics>,
    _working: BytePermit,
    queued: Queued,
}
struct Queued(Arc<AtomicU64>);
impl Queued {
    fn new(count: &Arc<AtomicU64>) -> Self {
        count.fetch_add(1, Ordering::Release);
        Self(Arc::clone(count))
    }
}
impl Drop for Queued {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}
struct Outgoing {
    reply: NativeReply,
    _bytes: Option<BytePermit>,
}

struct Budgets {
    working: Arc<ByteBudget>,
    encoded: Arc<ByteBudget>,
    stopped: Arc<AtomicBool>,
    decode_active: Active,
    encode_active: Active,
    queue_peak: AtomicU64,
    ready: Arc<AtomicU64>,
    paused: AtomicBool,
}

#[derive(Default)]
struct Active {
    now: AtomicU64,
    peak: AtomicU64,
}
impl Active {
    fn enter(&self) -> ActiveGuard<'_> {
        let current = self.now.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak.fetch_max(current, Ordering::Relaxed);
        ActiveGuard(self)
    }
}
struct ActiveGuard<'a>(&'a Active);
impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.now.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Budgets {
    fn new(memory: u64, stopped: Arc<AtomicBool>) -> Self {
        // 初值保留四分之一给 codec 私有堆、线程栈和运行时；Job 才是最终硬边界。
        Self {
            working: ByteBudget::new(memory / 2),
            encoded: ByteBudget::new(memory / 4),
            stopped,
            decode_active: Active::default(),
            encode_active: Active::default(),
            queue_peak: AtomicU64::new(0),
            ready: Arc::new(AtomicU64::new(0)),
            paused: AtomicBool::new(false),
        }
    }
    fn reserve(&self, bytes: u64) -> Result<BytePermit, NativeFailure> {
        self.working
            .acquire(bytes, &self.stopped)
            .ok_or_else(|| self.failure())
    }
    fn failure(&self) -> NativeFailure {
        if self.stopped.load(Ordering::Acquire) {
            NativeFailure::Unavailable
        } else {
            NativeFailure::Resource
        }
    }
    fn stamp(&self, mut outgoing: Outgoing) -> Outgoing {
        if let NativeEvent::Finished { stats, .. } = &mut outgoing.reply.event {
            *stats = NativeStats {
                decode_active_peak: self.decode_active.peak.load(Ordering::Relaxed),
                encode_active_peak: self.encode_active.peak.load(Ordering::Relaxed),
                decoded_queue_peak: self.queue_peak.load(Ordering::Relaxed),
                working_bytes_peak: self.working.peak(),
                encoded_bytes_peak: self.encoded.peak(),
            };
        }
        outgoing
    }
    fn begin_decode(&self) -> Result<ActiveGuard<'_>, NativeFailure> {
        loop {
            if self.stopped.load(Ordering::Acquire) {
                return Err(NativeFailure::Unavailable);
            }
            if !self.paused.load(Ordering::Acquire) {
                return Ok(self.decode_active.enter());
            }
            // E 和 writer 始终能排空；空闲后仍高水位则拒绝排队源，宿主才有机会 idle recycle。
            if self.decode_active.now.load(Ordering::Acquire) == 0
                && self.encode_active.now.load(Ordering::Acquire) == 0
                && self.ready.load(Ordering::Acquire) == 0
            {
                return Err(NativeFailure::Resource);
            }
            thread::sleep(TICK);
        }
    }
}

/// 运行宿主约定的 `--threads=N --memory-bytes=N` stdio 协议，stdout 仅写协议帧。
/// 调用方负责将本模块公开给独立 binary；不初始化 Tauri 或日志 subscriber。
pub fn run_stdio() -> io::Result<()> {
    let (width, memory) = parse_options(std::env::args().skip(1))?;
    run(width, memory)
}

fn parse_options(args: impl Iterator<Item = String>) -> io::Result<(usize, u64)> {
    let mut width = None;
    let mut memory = None;
    for arg in args {
        if let Some(value) = arg.strip_prefix("--threads=") {
            if width.is_some() {
                return Err(invalid("duplicate threads"));
            }
            width = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| invalid("invalid threads"))?,
            );
        } else if let Some(value) = arg.strip_prefix("--memory-bytes=") {
            if memory.is_some() {
                return Err(invalid("duplicate memory limit"));
            }
            memory = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| invalid("invalid memory limit"))?,
            );
        } else {
            return Err(invalid("unknown worker option"));
        }
    }
    match (width, memory) {
        (Some(width), Some(memory))
            if width > 0 && width.checked_mul(4).is_some() && memory >= 4 * ICC_LIMIT =>
        {
            Ok((width, memory))
        }
        _ => Err(invalid("missing or invalid worker budget")),
    }
}

fn run(width: usize, memory: u64) -> io::Result<()> {
    let stopped = Arc::new(AtomicBool::new(false));
    let budgets = Arc::new(Budgets::new(memory, Arc::clone(&stopped)));
    let mut threads = Threads {
        stopped: Arc::clone(&stopped),
        input: None,
        stages: Vec::new(),
    };
    let (requests_tx, requests_rx) = bounded::<DecodeJob>(2 * width);
    let (decoded_tx, decoded_rx) = bounded::<EncodeJob>(2 * width);
    let (reply_tx, reply_rx) = bounded::<Outgoing>(2 * width);
    let gpu = native_gpu::Bridge::start(&mut threads, &decoded_tx, &reply_tx, &budgets, memory)?;
    let monitor_budget = Arc::clone(&budgets);
    threads.spawn("native-memory".into(), move || {
        let mut sampled = Instant::now() - Duration::from_millis(250);
        while !monitor_budget.stopped.load(Ordering::Acquire) {
            if sampled.elapsed() >= Duration::from_millis(250) {
                sampled = Instant::now();
                let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
                counters.cb = std::mem::size_of_val(&counters) as u32;
                let sampled = unsafe {
                    GetProcessMemoryInfo(
                        GetCurrentProcess(),
                        (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX)
                            .cast::<PROCESS_MEMORY_COUNTERS>(),
                        counters.cb,
                    )
                };
                if sampled.is_err() {
                    monitor_budget.stopped.store(true, Ordering::Release);
                    break;
                }
                monitor_budget.paused.store(
                    counters.PrivateUsage as u64 >= memory.saturating_mul(4) / 5,
                    Ordering::Release,
                );
            }
            thread::sleep(TICK);
        }
    })?;
    for index in 0..width {
        let (input, output, replies, decode_budgets) = (
            requests_rx.clone(),
            decoded_tx.clone(),
            reply_tx.clone(),
            Arc::clone(&budgets),
        );
        let gpu = gpu.clone();
        threads.spawn(format!("native-decode-{index}"), move || {
            with_image_worker_session(|| decode_loop(input, output, replies, &decode_budgets, gpu));
        })?;
        let (input, replies, budgets) =
            (decoded_rx.clone(), reply_tx.clone(), Arc::clone(&budgets));
        threads.spawn(format!("native-encode-{index}"), move || {
            encode_loop(input, replies, &budgets)
        })?;
    }
    drop((requests_rx, decoded_tx, decoded_rx, reply_tx, gpu));
    let input_stopped = Arc::clone(&stopped);
    threads.input = Some(
        thread::Builder::new()
            .name("native-input".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut input = io::stdin().lock();
                    while !input_stopped.load(Ordering::Acquire) {
                        let request = match read_request(&mut input) {
                            Ok(request) => request,
                            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
                            Err(_) => {
                                input_stopped.store(true, Ordering::Release);
                                break;
                            }
                        };
                        // 协议只接受宿主 DuplicateHandle 到本进程的独占副本；不按路径重新打开。
                        let file = unsafe {
                            File::from_raw_handle(request.file_handle as usize as *mut _)
                        };
                        set_app_foreground(request.foreground);
                        if !send(&requests_tx, DecodeJob { request, file }, &input_stopped) {
                            break;
                        }
                    }
                }));
                if result.is_err() {
                    input_stopped.store(true, Ordering::Release);
                }
            })?,
    );
    let mut output = io::stdout().lock();
    output.write_all(&HELLO)?;
    output.flush()?;
    loop {
        if stopped.load(Ordering::Acquire) {
            return Err(io::Error::other("native worker stage failed"));
        }
        match reply_rx.recv_timeout(TICK) {
            Ok(outgoing) => {
                write_reply(&mut output, &outgoing.reply)?;
                // 只有管道写完才归还编码字节，不能在发送到 writer 队列时提前释放。
                drop(outgoing);
            }
            Err(RecvTimeoutError::Timeout) => (),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if stopped.load(Ordering::Acquire) {
        Err(io::Error::other("native worker stage failed"))
    } else {
        Ok(())
    }
}

// 显式 join 保证正常退出和可协作故障都不遗留线程；原生 codec 卡死由宿主 kill+wait。
struct Threads {
    stopped: Arc<AtomicBool>,
    input: Option<JoinHandle<()>>,
    stages: Vec<JoinHandle<()>>,
}
impl Threads {
    fn spawn(&mut self, name: String, operation: impl FnOnce() + Send + 'static) -> io::Result<()> {
        let stopped = Arc::clone(&self.stopped);
        self.stages
            .push(thread::Builder::new().name(name).spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).is_err() {
                    stopped.store(true, Ordering::Release);
                }
            })?);
        Ok(())
    }
}
impl Drop for Threads {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(input) = self.input.take() {
            // 取消同步管道读取，使 writer/协议故障不会永远等待仍打开的 stdin。
            // 重复取消覆盖「检查停止标志之后、进入 ReadFile 之前」的竞争窗口。
            while !input.is_finished() {
                unsafe {
                    cancel_synchronous_io(input.as_raw_handle());
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = input.join();
        }
        for stage in self.stages.drain(..) {
            let _ = stage.join();
        }
    }
}

// 此单一 Win32 ABI 不要求扩大 windows crate feature；缓存 rlib 也能验证真实取消行为。
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "CancelSynchronousIo"]
    fn cancel_synchronous_io(thread: *mut std::ffi::c_void) -> i32;
}

fn send<T>(output: &Sender<T>, mut value: T, stopped: &AtomicBool) -> bool {
    loop {
        if stopped.load(Ordering::Acquire) {
            return false;
        }
        match output.send_timeout(value, TICK) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(returned)) => value = returned,
            Err(SendTimeoutError::Disconnected(_)) => return false,
        }
    }
}
fn receive<T>(input: &Receiver<T>, stopped: &AtomicBool) -> Option<T> {
    while !stopped.load(Ordering::Acquire) {
        match input.recv_timeout(TICK) {
            Ok(value) => return Some(value),
            Err(RecvTimeoutError::Timeout) => (),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    None
}

fn decode_loop(
    input: Receiver<DecodeJob>,
    output: Sender<EncodeJob>,
    replies: Sender<Outgoing>,
    budgets: &Budgets,
    gpu: Option<native_gpu::Bridge>,
) {
    let mut qos = None;
    while let Some(DecodeJob { request, mut file }) = receive(&input, &budgets.stopped) {
        refresh_worker_qos(&mut qos);
        let started = Instant::now();
        let active = budgets.begin_decode();
        let mut gpu_metrics = None;
        let result = match &active {
            Ok(_) => decode(&mut file, &request, budgets, gpu.as_ref(), &mut gpu_metrics),
            Err(failure) => Err(*failure),
        };
        drop(active);
        let result = match result {
            Ok(DecodeOutput::Gpu(prepared)) => {
                let Some(gpu) = &gpu else {
                    budgets.stopped.store(true, Ordering::Release);
                    break;
                };
                if !gpu.send(
                    native_gpu::Job::new(
                        DecodeJob { request, file },
                        prepared,
                        started,
                        gpu_metrics.unwrap_or_default(),
                        budgets,
                    ),
                    &budgets.stopped,
                ) {
                    break;
                }
                continue;
            }
            Ok(DecodeOutput::Cpu(decoded)) => Ok(decoded),
            Err(failure) => Err(failure),
        };
        drop(file);
        if !complete_decode(
            request,
            result.map(EncodePixels::from),
            micros(started),
            gpu_metrics,
            &output,
            &replies,
            budgets,
        ) {
            break;
        }
    }
}

fn complete_decode(
    request: NativeRequest,
    result: Result<EncodePixels, NativeFailure>,
    decode_us: u64,
    gpu: Option<NativeGpuMetrics>,
    output: &Sender<EncodeJob>,
    replies: &Sender<Outgoing>,
    budgets: &Budgets,
) -> bool {
    // 即使资源拒绝也已结束源读取；编码成功与否不改变源许可的归还时机。
    if !send(
        replies,
        Outgoing {
            reply: NativeReply {
                event: NativeEvent::Decoded { id: request.id },
                display: vec![],
                ai: vec![],
            },
            _bytes: None,
        },
        &budgets.stopped,
    ) {
        return false;
    }
    match result {
        Ok(EncodePixels {
            decoded,
            thumbhash_input,
            backend,
            working,
        }) => {
            if !send(
                output,
                EncodeJob {
                    request,
                    decoded,
                    thumbhash_input,
                    backend,
                    decode_us,
                    gpu,
                    _working: working,
                    queued: Queued::new(&budgets.ready),
                },
                &budgets.stopped,
            ) {
                return false;
            }
            budgets
                .queue_peak
                .fetch_max(output.len() as u64, Ordering::Relaxed);
        }
        Err(failure) => {
            return send(
                replies,
                budgets.stamp(with_gpu(failed(request.id, failure, "", decode_us, 0), gpu)),
                &budgets.stopped,
            );
        }
    }
    true
}

fn decode(
    file: &mut File,
    request: &NativeRequest,
    budgets: &Budgets,
    gpu: Option<&native_gpu::Bridge>,
    gpu_metrics: &mut Option<NativeGpuMetrics>,
) -> Result<DecodeOutput, NativeFailure> {
    if request.decode_policy == NativeDecodePolicy::ImageRsFirst
        && ImageRsEngine.can_handle(&request.format)
    {
        return decode_fallback(
            decode_image_rs(file, request, budgets).map(DecodeOutput::Cpu),
            || {
                decode_wic(file, request, budgets, None, gpu_metrics, None)
                    .map_err(|error| classify(&error))
            },
        );
    }
    decode_fallback(
        decode_wic(file, request, budgets, gpu, gpu_metrics, None)
            .map_err(|error| classify(&error)),
        || decode_image_rs(file, request, budgets).map(DecodeOutput::Cpu),
    )
}

fn decode_fallback<T>(
    first: Result<T, NativeFailure>,
    fallback: impl FnOnce() -> Result<T, NativeFailure>,
) -> Result<T, NativeFailure> {
    match first {
        Err(NativeFailure::Content | NativeFailure::Unsupported) => fallback(),
        result => result,
    }
}

fn decode_image_rs(
    file: &mut File,
    request: &NativeRequest,
    budgets: &Budgets,
) -> Result<(DecodedImage, &'static str, BytePermit), NativeFailure> {
    if !ImageRsEngine.can_handle(&request.format) {
        return Err(NativeFailure::Unsupported);
    }
    // 通用解码承担 JPEG 构造器/EXIF 库可能读入压缩流的工作集，普通 WIC 不缴纳全源额度。
    let metadata = file
        .metadata()
        .map_err(|_| NativeFailure::Unavailable)?
        .len()
        .saturating_mul(2)
        .saturating_add(ICC_LIMIT);
    let probe = budgets.reserve(metadata)?;
    let info = ImageRsEngine::working_set_bytes(file, metadata).map_err(|e| classify(&e))?;
    drop(probe);
    let (hint, target) = target(request, info.width, info.height)?;
    let pixel_peak = info
        .bytes_for_resize(Some(hint))
        .map_err(|e| classify(&e))?;
    let reserve = pixel_peak
        .saturating_add(metadata)
        .max(encoding_peak(target, ICC_LIMIT, request));
    let mut working = budgets.reserve(reserve)?;
    // 此上限来自实际 decoder 位深/全图尺寸，不把目标 RGBA 上限传作源解码额度。
    let decoded =
        ImageRsEngine::decode_open_file_bounded(file, &request.format, Some(hint), reserve)
            .map_err(|e| classify(&e))?;
    validate_decoded(&decoded, request, info.width, info.height)?;
    let retained = encoding_peak(
        (decoded.width, decoded.height),
        decoded.icc.as_ref().map_or(0, |v| v.len() as u64),
        request,
    );
    if retained > reserve {
        return Err(NativeFailure::Resource);
    }
    working.shrink(retained);
    Ok((decoded, "image-rs", working))
}

fn decode_wic(
    file: &mut File,
    request: &NativeRequest,
    budgets: &Budgets,
    gpu: Option<&native_gpu::Bridge>,
    gpu_metrics: &mut Option<NativeGpuMetrics>,
    spare: Option<(BytePermit, u64)>,
) -> Result<DecodeOutput, AppError> {
    // 只读有界 EXIF；AI 和兼容解码策略继续使用主图，避免改变其输入语义。
    let jpeg = if request.emit_display
        && !request.emit_ai
        && request.decode_policy == NativeDecodePolicy::NativeFirst
        && matches!(request.format.as_str(), "jpg" | "jpeg")
    {
        Some(embedded_jpeg::inspect(file).map_err(AppError::Io)?)
    } else {
        None
    };
    let orientation = if let Some(metadata) = &jpeg {
        metadata.orientation
    } else {
        bounded_orientation(file).map_err(AppError::Io)?
    };
    file.rewind().map_err(AppError::Io)?;
    with_wic_factory(|factory| unsafe {
        let decoder = factory
            .CreateDecoderFromFileHandle(
                file.as_raw_handle() as usize,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|e| wic_error("WIC metadata", e))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| wic_error("WIC metadata frame", e))?;
        let (mut width, mut height) = (0, 0);
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|e| wic_error("WIC metadata size", e))?;
        let orientation = orientation.unwrap_or_else(|| {
            use windows::core::{w, PROPVARIANT};
            let Ok(reader) = frame.GetMetadataQueryReader() else {
                return 1;
            };
            for key in [
                w!("/app1/ifd/{ushort=274}"),
                w!("/ifd/{ushort=274}"),
                w!("/eXIf/ifd/{ushort=274}"),
            ] {
                let mut value = PROPVARIANT::new();
                if reader.GetMetadataByName(key, &mut value).is_ok() {
                    if let Ok(value @ 1..=8) = u32::try_from(&value) {
                        return value;
                    }
                }
            }
            1
        });
        let (hint, target) = target(request, width, height).map_err(|_| pixel_limit())?;
        let context = frame_icc_context(factory, &frame)?;
        // 独立 JPEG 的色域未必沿用主图 ICC；首版只走无 ICC、sRGB/未标记的明确路径。
        let simple_color = context.is_none()
            && jpeg
                .as_ref()
                .is_some_and(|metadata| matches!(metadata.color_space, None | Some(1)));
        if let Some(metadata) = jpeg.as_ref().filter(|_| simple_color && spare.is_none()) {
            if let Some(embedded) = &metadata.thumbnail {
                if let Some(result) = decode_embedded_jpeg(
                    file,
                    factory,
                    embedded,
                    (width, height, orientation),
                    request,
                    budgets,
                )? {
                    return Ok(DecodeOutput::Cpu(result));
                }
            }
        }
        if let Some(gpu) = gpu.filter(|_| simple_color) {
            if let Some(prepared) = native_gpu::prepare(
                gpu,
                &frame,
                target,
                orientation,
                request,
                budgets,
                gpu_metrics,
            ) {
                return Ok(DecodeOutput::Gpu(prepared));
            }
        }
        let icc_bytes = context.as_ref().map_or(0, |(_, size)| u64::from(*size));
        // metadata context 只是至多 16 个 COM 指针；profile 字节按实际长度申请后才读取。
        let reserve = encoding_peak(target, icc_bytes, request).max(
            rgba_bytes(target)
                .saturating_mul(2)
                .saturating_add(icc_bytes),
        );
        // GPU 回退复用已持有的完整工作集，不能在有界阶段之间释放后重新阻塞申请。
        let mut working = if let Some((working, reserved)) = spare {
            if reserve > reserved {
                return Err(pixel_limit());
            }
            working
        } else {
            budgets.reserve(reserve).map_err(|_| pixel_limit())?
        };
        let icc = if let Some((context, mut size)) = context {
            let mut profile = vec![0; size as usize];
            context
                .GetProfileBytes(&mut profile, &mut size)
                .map_err(|e| wic_error("WIC ICC read", e))?;
            if size as usize > profile.len() {
                return Err(pixel_limit());
            }
            profile.truncate(size as usize);
            Some(profile)
        } else {
            None
        };
        let decoded = decode_frame(
            factory,
            &frame,
            icc,
            orientation,
            Some(hint),
            request.max_pixel_bytes,
        )?;
        validate_decoded(&decoded, request, width, height).map_err(|_| pixel_limit())?;
        let retained = encoding_peak((decoded.width, decoded.height), icc_bytes, request);
        if retained > reserve {
            return Err(pixel_limit());
        }
        working.shrink(retained);
        Ok(DecodeOutput::Cpu((decoded, "wic", working)))
    })
}

fn oriented_size(size: (u32, u32), orientation: u32) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (size.1, size.0)
    } else {
        size
    }
}

fn decode_embedded_jpeg(
    file: &mut File,
    factory: &windows::Win32::Graphics::Imaging::IWICImagingFactory,
    embedded: &embedded_jpeg::EmbeddedJpeg,
    main: (u32, u32, u32),
    request: &NativeRequest,
    budgets: &Budgets,
) -> Result<Option<(DecodedImage, &'static str, BytePermit)>, AppError> {
    let orientation = match embedded.orientation {
        Some(value) => value,
        None if main.2 == 1 => 1,
        // 非正向主图不能证明缺失方向的内嵌像素是否已转正，继续解码主图。
        None => return Ok(None),
    };
    let (hint, target) = target(request, main.0, main.1).map_err(|_| pixel_limit())?;
    // APP1 已限制压缩长度；申请预算后才读入，COM 流释放前保持缓冲存活。
    let reserve = encoding_peak(target, 0, request)
        .max(rgba_bytes(target).saturating_mul(2))
        .saturating_add(u64::from(embedded.length));
    // 压缩流和像素一次取足，避免持有部分许可再等待同一字节池。
    let mut working = budgets.reserve(reserve).map_err(|_| pixel_limit())?;
    let mut bytes = vec![0; embedded.length as usize];
    file.seek(SeekFrom::Start(embedded.offset))
        .map_err(AppError::Io)?;
    if let Err(error) = file.read_exact(&mut bytes) {
        return if error.kind() == io::ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(AppError::Io(error))
        };
    }
    let attempt = (|| unsafe {
        use windows::core::{w, PROPVARIANT};
        let stream = factory
            .CreateStream()
            .map_err(|e| wic_error("embedded stream", e))?;
        stream
            .InitializeFromMemory(&bytes)
            .map_err(|e| wic_error("embedded bytes", e))?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(|e| wic_error("embedded decoder", e))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| wic_error("embedded frame", e))?;
        let (mut width, mut height) = (0, 0);
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|e| wic_error("embedded size", e))?;
        let expected = oriented_size(target, main.2);
        let available = oriented_size((width, height), orientation);
        if available.0 < expected.0
            || available.1 < expected.1
            || oriented_size(resize_dimensions(width, height, Some(hint))?, orientation) != expected
            || frame_icc_context(factory, &frame)?.is_some()
        {
            return Ok(None);
        }
        // 内嵌 JPEG 若又声明一套方向/色域，仅接受与外层 IFD1 一致的解释。
        if let Ok(reader) = frame.GetMetadataQueryReader() {
            for (key, expected) in [
                (w!("/app1/ifd/{ushort=274}"), orientation),
                (w!("/app1/ifd/exif/{ushort=40961}"), 1),
            ] {
                let mut value = PROPVARIANT::new();
                if reader.GetMetadataByName(key, &mut value).is_ok()
                    && u32::try_from(&value).ok() != Some(expected)
                {
                    return Ok(None);
                }
            }
        }
        let decoded = decode_frame(
            factory,
            &frame,
            None,
            orientation,
            Some(hint),
            request.max_pixel_bytes,
        )?;
        if (decoded.width, decoded.height) != expected {
            return Ok(None);
        }
        Ok(Some(decoded))
    })();
    drop(bytes);
    // 可选图损坏不能拖累完整主图；资源耗尽/停止仍遵守既有执行域失败策略。
    match attempt {
        Err(error) if classify(&error) == NativeFailure::Resource => Err(error),
        Err(_) => Ok(None),
        Ok(None) => Ok(None),
        Ok(Some(decoded)) => {
            working.shrink(encoding_peak((decoded.width, decoded.height), 0, request));
            Ok(Some((decoded, "wic-embedded", working)))
        }
    }
}

// 只寻找 IFD0 的 Orientation；不加载缩略图、MakerNote、ICC 或压缩像素。
// 有界逐块扫描覆盖 WIC 尚不公开 PNG eXIf 的系统版本，堆分配为零。
fn bounded_orientation(file: &mut File) -> io::Result<Option<u32>> {
    file.rewind()?;
    let mut magic = [0; 12];
    if file.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    let length = file.metadata()?.len();
    let mut position = if magic.starts_with(b"\x89PNG\r\n\x1a\n") {
        8
    } else if magic.starts_with(b"\xff\xd8") {
        2
    } else if &magic[..4] == b"RIFF" && &magic[8..] == b"WEBP" {
        12
    } else {
        0
    };
    if position == 0 {
        return tiff_orientation(file, 0, length);
    }
    for _ in 0..4096 {
        if position >= length {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(position))?;
        if magic.starts_with(b"\xff\xd8") {
            let mut header = [0; 4];
            if file.read_exact(&mut header).is_err()
                || header[0] != 255
                || matches!(header[1], 0xda | 0xd9)
            {
                return Ok(None);
            }
            let size = u16::from_be_bytes([header[2], header[3]]) as u64;
            if size < 2 {
                return Ok(None);
            }
            if header[1] == 0xe1 && size >= 8 {
                let mut exif = [0; 6];
                file.read_exact(&mut exif)?;
                if &exif == b"Exif\0\0" {
                    return tiff_orientation(
                        file,
                        position + 10,
                        (position + 2 + size).min(length),
                    );
                }
            }
            position = position.saturating_add(2 + size);
        } else {
            let mut header = [0; 8];
            if file.read_exact(&mut header).is_err() {
                return Ok(None);
            }
            let png = magic[0] == 0x89;
            let (size, exif) = if png {
                (
                    u32::from_be_bytes(header[..4].try_into().unwrap()) as u64,
                    &header[4..] == b"eXIf",
                )
            } else {
                (
                    u32::from_le_bytes(header[4..].try_into().unwrap()) as u64,
                    &header[..4] == b"EXIF",
                )
            };
            if exif {
                let mut base = position + 8;
                // WebP 允许携带 JPEG 风格的 Exif 前缀；PNG eXIf 直接从 TIFF header 开始。
                if !png && size >= 6 {
                    let mut prefix = [0; 6];
                    file.read_exact(&mut prefix)?;
                    if &prefix == b"Exif\0\0" {
                        base += 6;
                    }
                }
                return tiff_orientation(file, base, (position + 8 + size).min(length));
            }
            position = position.saturating_add(8 + size + if png { 4 } else { size & 1 });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::OutOfMemory,
        "metadata scan limit exceeded",
    ))
}

fn tiff_orientation(file: &mut File, base: u64, end: u64) -> io::Result<Option<u32>> {
    if end.saturating_sub(base) < 8 {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(base))?;
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    let little = match &header[..4] {
        b"II\x2a\0" => true,
        b"MM\0\x2a" => false,
        _ => return Ok(None),
    };
    let u16_at = |bytes: &[u8]| {
        if little {
            u16::from_le_bytes(bytes.try_into().unwrap())
        } else {
            u16::from_be_bytes(bytes.try_into().unwrap())
        }
    };
    let offset = if little {
        u32::from_le_bytes(header[4..].try_into().unwrap())
    } else {
        u32::from_be_bytes(header[4..].try_into().unwrap())
    };
    let position = base.saturating_add(u64::from(offset));
    if position.saturating_add(2) > end {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(position))?;
    let mut count = [0; 2];
    file.read_exact(&mut count)?;
    let count = u16_at(&count) as u64;
    if count > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            "IFD entry limit exceeded",
        ));
    }
    if position.saturating_add(2 + count * 12) > end {
        return Ok(None);
    }
    for _ in 0..count {
        let mut entry = [0; 12];
        file.read_exact(&mut entry)?;
        let one = if little {
            entry[4..8] == [1, 0, 0, 0]
        } else {
            entry[4..8] == [0, 0, 0, 1]
        };
        if u16_at(&entry[..2]) == 0x112 && u16_at(&entry[2..4]) == 3 && one {
            return Ok(Some(u32::from(u16_at(&entry[8..10]))).filter(|v| (1..=8).contains(v)));
        }
    }
    Ok(None)
}
fn target(
    request: &NativeRequest,
    width: u32,
    height: u32,
) -> Result<(ResizeHint, (u32, u32)), NativeFailure> {
    if width == 0 || height == 0 {
        return Err(NativeFailure::Resource);
    }
    let long = u64::from(width.max(height));
    let ai_long = if request.emit_ai {
        (long * u64::from(request.ai_cache_short_edge)).div_ceil(u64::from(width.min(height)))
    } else {
        0
    };
    // decode_long_edge 是宿主 DB 预估；真实 AI 短边需求必须在执行域重算。
    let hint = ResizeHint::LongEdge(u64::from(request.output_size).max(ai_long).min(long) as u32);
    let target = resize_dimensions(width, height, Some(hint)).map_err(|e| classify(&e))?;
    if rgba_bytes(target) > request.max_pixel_bytes {
        return Err(NativeFailure::Resource);
    }
    Ok((hint, target))
}

fn validate_decoded(
    decoded: &DecodedImage,
    request: &NativeRequest,
    source_w: u32,
    source_h: u32,
) -> Result<(), NativeFailure> {
    if rgba_bytes((decoded.width, decoded.height)) > request.max_pixel_bytes
        || (request.emit_ai
            && decoded.width.min(decoded.height)
                < request.ai_cache_short_edge.min(source_w.min(source_h)))
    {
        return Err(NativeFailure::Resource);
    }
    Ok(())
}

fn rgba_bytes(size: (u32, u32)) -> u64 {
    u64::from(size.0)
        .saturating_mul(u64::from(size.1))
        .saturating_mul(4)
}

fn encoding_peak(size: (u32, u32), icc: u64, request: &NativeRequest) -> u64 {
    // 源 RGBA + resize/CMS/JPEG 回退的同时存活副本和 FIR 中间图；都不会大于源小图。
    // 权重按长边计，ICC 解析留一份副本，编码器 native buffer 到 Vec 的临时复制单独计入。
    rgba_bytes(size)
        .saturating_mul(6)
        .saturating_add(u64::from(size.0.max(size.1)).saturating_mul(128))
        .saturating_add(icc.saturating_mul(2))
        .saturating_add(request.max_encoded_bytes)
        .saturating_add(256 * 1024)
}

fn encode_loop(input: Receiver<EncodeJob>, replies: Sender<Outgoing>, budgets: &Budgets) {
    let mut qos = None;
    while let Some(job) = receive(&input, &budgets.stopped) {
        // 即时消费也曾有一项排队；消费侧与生产侧共同观察，避免峰值恒为零。
        budgets.queue_peak.fetch_max(
            (input.len() + 1).min(input.capacity().unwrap_or(1)) as u64,
            Ordering::Relaxed,
        );
        refresh_worker_qos(&mut qos);
        let EncodeJob {
            request,
            decoded,
            thumbhash_input,
            backend,
            decode_us,
            gpu,
            _working,
            queued,
        } = job;
        let started = Instant::now();
        let Some(mut bytes) = budgets.encoded.acquire(
            request.max_encoded_bytes.saturating_add(HASH_BYTES),
            &budgets.stopped,
        ) else {
            drop((decoded, thumbhash_input, _working));
            if !send(
                &replies,
                budgets.stamp(with_gpu(
                    failed(
                        request.id,
                        budgets.failure(),
                        backend,
                        decode_us,
                        micros(started),
                    ),
                    gpu,
                )),
                &budgets.stopped,
            ) {
                break;
            }
            continue;
        };
        let config = ThumbConfig {
            cache_dir: Default::default(),
            size: request.output_size,
            skip_max_bytes: 0,
            strategy: String::new(),
            gpu_engine: String::new(),
            ai_hq_cache: request.emit_ai,
            webp_quality: request.webp_quality,
            ai_cache_short_edge: request.ai_cache_short_edge,
        };
        let active = budgets.encode_active.enter();
        drop(queued);
        let encoded = encode_image_payload(
            decoded,
            thumbhash_input,
            &config,
            request.emit_display,
            request.emit_ai,
        );
        drop(active);
        let encode_us = micros(started);
        let outgoing = match encoded {
            Ok(encoded) => {
                let ai = encoded.ai.unwrap_or_default();
                // JPEG Vec 可能预留多余容量；账本跟随真实分配，wire 仍报告有效长度。
                let capacity = encoded.display.capacity().saturating_add(ai.capacity()) as u64;
                let hash_capacity = encoded
                    .thumbhash
                    .as_ref()
                    .map_or(0, |hash| hash.capacity() as u64);
                if capacity > request.max_encoded_bytes
                    || hash_capacity > HASH_BYTES
                    || (request.emit_display && encoded.display.is_empty())
                    || (request.emit_ai && ai.is_empty())
                {
                    drop((encoded.display, ai, encoded.thumbhash));
                    drop(bytes);
                    failed(
                        request.id,
                        NativeFailure::Resource,
                        backend,
                        decode_us,
                        encode_us,
                    )
                } else {
                    bytes.shrink(capacity + hash_capacity);
                    Outgoing {
                        reply: NativeReply {
                            event: NativeEvent::Finished {
                                id: request.id,
                                failure: None,
                                backend: backend.into(),
                                decode_us,
                                encode_us,
                                display_len: encoded.display.len(),
                                ai_len: ai.len(),
                                thumbhash: encoded.thumbhash,
                                stats: Default::default(),
                                gpu: None,
                            },
                            display: encoded.display,
                            ai,
                        },
                        _bytes: Some(bytes),
                    }
                }
            }
            // 原图已解码成功，编码器失败不能转成原图坏图。
            Err(error) => {
                drop(bytes);
                failed(
                    request.id,
                    if classify(&error) == NativeFailure::Resource {
                        NativeFailure::Resource
                    } else {
                        NativeFailure::Unavailable
                    },
                    backend,
                    decode_us,
                    encode_us,
                )
            }
        };
        drop(_working);
        if !send(
            &replies,
            budgets.stamp(with_gpu(outgoing, gpu)),
            &budgets.stopped,
        ) {
            break;
        }
    }
}

fn failed(
    id: u64,
    failure: NativeFailure,
    backend: &str,
    decode_us: u64,
    encode_us: u64,
) -> Outgoing {
    Outgoing {
        reply: NativeReply {
            event: NativeEvent::Finished {
                id,
                failure: Some(failure),
                backend: backend.into(),
                decode_us,
                encode_us,
                display_len: 0,
                ai_len: 0,
                thumbhash: None,
                stats: Default::default(),
                gpu: None,
            },
            display: vec![],
            ai: vec![],
        },
        _bytes: None,
    }
}
fn with_gpu(mut outgoing: Outgoing, gpu: Option<NativeGpuMetrics>) -> Outgoing {
    if let NativeEvent::Finished { gpu: metrics, .. } = &mut outgoing.reply.event {
        *metrics = gpu;
    }
    outgoing
}
fn classify(error: &AppError) -> NativeFailure {
    match error {
        AppError::Engine(image::ImageError::Limits(_)) => NativeFailure::Resource,
        AppError::Engine(image::ImageError::Decoding(_)) => NativeFailure::Content,
        AppError::UnsupportedFormat(_) | AppError::Engine(image::ImageError::Unsupported(_)) => {
            NativeFailure::Unsupported
        }
        AppError::Engine(image::ImageError::IoError(error))
            if error.kind() == io::ErrorKind::UnexpectedEof =>
        {
            NativeFailure::Content
        }
        AppError::Io(error) | AppError::Engine(image::ImageError::IoError(error))
            if error.kind() == io::ErrorKind::OutOfMemory =>
        {
            NativeFailure::Resource
        }
        _ => NativeFailure::Unavailable,
    }
}
fn micros(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> NativeRequest {
        NativeRequest {
            id: 1,
            file_handle: 1,
            format: "png".into(),
            decode_policy: NativeDecodePolicy::NativeFirst,
            output_size: 64,
            decode_long_edge: 64,
            webp_quality: 80,
            ai_cache_short_edge: 32,
            emit_ai: true,
            emit_display: true,
            max_encoded_bytes: MIB,
            max_pixel_bytes: 64 * MIB,
            foreground: true,
        }
    }

    struct EmbeddedFixture {
        main_orientation: u16,
        thumb_orientation: Option<u16>,
        thumb_size: (u32, u32),
        color_space: u16,
        corrupt: bool,
        present: bool,
        main_icc: bool,
        thumb_icc: bool,
    }

    impl Default for EmbeddedFixture {
        fn default() -> Self {
            Self {
                main_orientation: 1,
                thumb_orientation: Some(1),
                thumb_size: (128, 96),
                color_space: 1,
                corrupt: false,
                present: true,
                main_icc: false,
                thumb_icc: false,
            }
        }
    }

    fn embedded_fixture(options: EmbeddedFixture) -> File {
        fn entry(out: &mut Vec<u8>, tag: u16, kind: u16, value: u32) {
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(1u32.to_le_bytes());
            out.extend(value.to_le_bytes());
        }
        fn jpeg(image: image::RgbImage, icc: bool) -> Vec<u8> {
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
                .encode_image(&image)
                .unwrap();
            if icc {
                let profile = moxcms::ColorProfile::new_srgb().encode().unwrap();
                let mut segment = vec![255, 226];
                segment.extend(u16::try_from(profile.len() + 16).unwrap().to_be_bytes());
                segment.extend(b"ICC_PROFILE\0\x01\x01");
                segment.extend(profile);
                bytes.splice(2..2, segment);
            }
            bytes
        }
        let main = jpeg(
            image::RgbImage::from_pixel(320, 240, image::Rgb([0, 0, 240])),
            options.main_icc,
        );
        let mut thumb = jpeg(
            image::RgbImage::from_fn(options.thumb_size.0, options.thumb_size.1, |x, _| {
                if x < options.thumb_size.0 / 2 {
                    image::Rgb([240, 0, 0])
                } else {
                    image::Rgb([0, 240, 0])
                }
            }),
            options.thumb_icc,
        );
        if options.corrupt {
            thumb = vec![255, 216, 255, 217];
        }
        let entries = 3 + u16::from(options.thumb_orientation.is_some());
        let thumb_offset = 56 + 2 + u32::from(entries) * 12 + 4;
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        exif.extend(2u16.to_le_bytes());
        entry(&mut exif, 0x112, 3, u32::from(options.main_orientation));
        entry(&mut exif, 0x8769, 4, 38);
        exif.extend(56u32.to_le_bytes());
        exif.extend(1u16.to_le_bytes());
        entry(&mut exif, 0xa001, 3, u32::from(options.color_space));
        exif.extend(0u32.to_le_bytes());
        exif.extend(entries.to_le_bytes());
        entry(&mut exif, 0x103, 3, 6);
        if let Some(orientation) = options.thumb_orientation {
            entry(&mut exif, 0x112, 3, u32::from(orientation));
        }
        entry(&mut exif, 0x201, 4, thumb_offset);
        entry(&mut exif, 0x202, 4, thumb.len() as u32);
        exif.extend(0u32.to_le_bytes());
        exif.extend(thumb);
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&main[..2]).unwrap();
        if options.present {
            file.write_all(&[255, 225]).unwrap();
            file.write_all(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes())
                .unwrap();
            file.write_all(&exif).unwrap();
        }
        file.write_all(&main[2..]).unwrap();
        file.rewind().unwrap();
        file
    }

    fn display_request() -> NativeRequest {
        NativeRequest {
            format: "jpeg".into(),
            emit_ai: false,
            ..request()
        }
    }

    fn fixture_decode(
        options: EmbeddedFixture,
        request: NativeRequest,
    ) -> (DecodedImage, &'static str) {
        let mut file = embedded_fixture(options);
        let budgets = Budgets::new(64 * MIB, Arc::new(AtomicBool::new(false)));
        let DecodeOutput::Cpu((decoded, backend, permit)) =
            decode_wic(&mut file, &request, &budgets, None, &mut None, None).unwrap()
        else {
            panic!("disabled experiment must keep CPU decoding");
        };
        assert!(budgets.working.peak() <= 2 * MIB);
        drop(permit);
        (decoded, backend)
    }

    fn assert_main(decoded: (DecodedImage, &'static str)) {
        assert_eq!(decoded.1, "wic");
        assert!(decoded.0.pixels[2] > 220);
        assert!(decoded.0.pixels[0] < 15);
    }

    #[test]
    fn gpu_handoff_defers_decoded_and_fallback_reuses_held_budget() {
        let (gpu, input) = native_gpu::test_bridge();
        let stopped = Arc::new(AtomicBool::new(false));
        let budgets = Arc::new(Budgets::new(64 * MIB, Arc::clone(&stopped)));
        let file = embedded_fixture(EmbeddedFixture {
            present: false,
            ..Default::default()
        });
        let (requests, received) = bounded(1);
        let (output, encoded) = bounded(1);
        let (replies, events) = bounded(2);
        requests
            .send(DecodeJob {
                request: display_request(),
                file,
            })
            .unwrap_or_else(|_| panic!("request queue closed"));
        drop(requests);
        let thread_budgets = Arc::clone(&budgets);
        let thread_output = output.clone();
        let thread_replies = replies.clone();
        let decoder = thread::spawn(move || {
            with_image_worker_session(|| {
                decode_loop(
                    received,
                    thread_output,
                    thread_replies,
                    &thread_budgets,
                    Some(gpu),
                )
            });
        });
        let pending = input
            .recv_timeout(Duration::from_secs(2))
            .expect("main JPEG must reach planar queue");
        decoder.join().unwrap();
        assert!(
            events.is_empty(),
            "Decoded must wait for GPU or CPU fallback"
        );
        assert!(encoded.is_empty());
        // 此时只有一项持有许可；占满其余额，验证回退无需追加额度且不重复计费。
        let held = budgets.working.peak();
        let remaining = budgets.reserve(32 * MIB - held).unwrap();
        let fallback_budgets = Arc::clone(&budgets);
        let (finished, completed) = bounded(1);
        let worker = thread::spawn(move || {
            let ok = with_image_worker_session(|| {
                native_gpu::test_fallback(pending, &output, &replies, &fallback_budgets)
            });
            finished.send(ok).unwrap();
        });
        let result = completed.recv_timeout(Duration::from_secs(2));
        if result.is_err() {
            stopped.store(true, Ordering::Release);
        }
        worker.join().unwrap();
        assert!(result.unwrap());
        assert!(matches!(
            events.recv().unwrap().reply.event,
            NativeEvent::Decoded { .. }
        ));
        let encoded = encoded.recv().unwrap();
        assert_eq!(encoded.backend, "wic");
        assert!(encoded.thumbhash_input.is_none());
        assert_eq!(
            encoded.gpu.as_ref().unwrap().fallback.as_deref(),
            Some("gpu_device")
        );
        assert_eq!((encoded.decoded.width, encoded.decoded.height), (64, 48));
        assert_eq!(budgets.working.peak(), 32 * MIB);
        drop((encoded, remaining));
        assert!(
            budgets.reserve(32 * MIB).is_ok(),
            "all GPU/CPU working permits must be released"
        );
    }

    #[test]
    fn ai_output_keeps_main_pixels_even_with_usable_embedded_image() {
        assert_main(fixture_decode(
            EmbeddedFixture::default(),
            NativeRequest {
                emit_ai: true,
                ..display_request()
            },
        ));
    }

    #[test]
    fn high_water_idle_rejection_closes_queued_source_before_decoded() {
        let budgets = Arc::new(Budgets::new(512 * MIB, Arc::new(AtomicBool::new(false))));
        budgets.paused.store(true, Ordering::Release);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queued.png");
        std::fs::write(&path, b"not read while paused").unwrap();
        let (tx, rx) = bounded(1);
        let (out, encoded) = bounded(1);
        let (replies, received) = bounded(2);
        tx.send(DecodeJob {
            request: request(),
            file: File::open(&path).unwrap(),
        })
        .unwrap_or_else(|_| panic!("decode queue disconnected"));
        drop(tx);
        let worker_budget = Arc::clone(&budgets);
        let worker = thread::spawn(move || decode_loop(rx, out, replies, &worker_budget, None));
        let first = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(first.reply.event, NativeEvent::Decoded { .. }));
        use std::os::windows::fs::OpenOptionsExt;
        let _exclusive = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let final_reply = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            final_reply.reply.event,
            NativeEvent::Finished {
                failure: Some(NativeFailure::Resource),
                ..
            }
        ));
        worker.join().unwrap();
        assert!(encoded.try_recv().is_err());
        assert_eq!(budgets.working.peak(), 0);
    }

    #[test]
    fn decoder_fallback_only_retries_content_or_unsupported() {
        for failure in [
            NativeFailure::Resource,
            NativeFailure::Unavailable,
            NativeFailure::Cancelled,
        ] {
            assert_eq!(
                decode_fallback::<()>(Err(failure), || panic!("must not retry")),
                Err(failure)
            );
        }
        for failure in [NativeFailure::Content, NativeFailure::Unsupported] {
            assert_eq!(
                decode_fallback(Err(failure), || Ok("fallback")),
                Ok("fallback")
            );
        }
        assert_eq!(
            decode_fallback(Ok("first"), || panic!("must not retry")),
            Ok("first")
        );
    }
}
