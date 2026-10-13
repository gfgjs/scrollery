//! 实验性的有界 GPU 阶段；只接收已计费的 JPEG 平面，源句柄保留到成功或 CPU 回退。

use std::collections::VecDeque;
use std::sync::Mutex;

use crossbeam_channel::TryRecvError;
use windows::Win32::Graphics::Imaging::IWICBitmapFrameDecode;

use super::*;
use crate::engine::gpu::wic_gpu::{GpuProcessor, PlanarImage, PlanarLayout};
use crate::thumbnail::thumbhash::thumbhash_resize_target;

const QUEUE_CAPACITY: usize = 2;
// GPU 实测出现整机卡死，先停用硬件路径；根因确认前保持 CPU 路线。
const D3D11_ENABLED: bool = false;

pub(super) struct Prepared {
    image: PlanarImage,
    working: BytePermit,
    reserved: u64,
}

pub(super) struct Job {
    source: DecodeJob,
    prepared: Prepared,
    started: Instant,
    queued_at: Instant,
    metrics: NativeGpuMetrics,
    _queued: Queued,
}

impl Job {
    pub(super) fn new(
        source: DecodeJob,
        prepared: Prepared,
        started: Instant,
        metrics: NativeGpuMetrics,
        budgets: &Budgets,
    ) -> Self {
        Self {
            source,
            prepared,
            started,
            queued_at: Instant::now(),
            metrics,
            _queued: Queued::new(&budgets.ready),
        }
    }
}

#[derive(Clone)]
pub(super) struct Bridge {
    sender: Sender<Job>,
    failure: Arc<Mutex<Option<String>>>,
    init_us: Arc<AtomicU64>,
}

impl Bridge {
    pub(super) fn start(
        threads: &mut Threads,
        output: &Sender<EncodeJob>,
        replies: &Sender<Outgoing>,
        budgets: &Arc<Budgets>,
        memory: u64,
    ) -> io::Result<Option<Self>> {
        if !D3D11_ENABLED {
            return Ok(None);
        }
        let (sender, input) = bounded(QUEUE_CAPACITY);
        let bridge = Self {
            sender,
            failure: Arc::new(Mutex::new(None)),
            init_us: Arc::new(AtomicU64::new(0)),
        };
        // GPU 线程不能保留发送端，否则解码池正常退出后接收端永远不会关闭。
        let (failure, init_us) = (Arc::clone(&bridge.failure), Arc::clone(&bridge.init_us));
        let (output, replies, budgets) = (output.clone(), replies.clone(), Arc::clone(budgets));
        threads.spawn("native-gpu".into(), move || {
            with_image_worker_session(|| {
                gpu_loop(
                    input,
                    output,
                    replies,
                    &budgets,
                    memory / 4,
                    failure,
                    init_us,
                )
            });
        })?;
        Ok(Some(bridge))
    }

    pub(super) fn send(&self, job: Job, stopped: &AtomicBool) -> bool {
        send(&self.sender, job, stopped)
    }
}

#[cfg(test)]
pub(super) fn test_bridge() -> (Bridge, Receiver<Job>) {
    let (sender, input) = bounded(QUEUE_CAPACITY);
    (
        Bridge {
            sender,
            failure: Arc::new(Mutex::new(None)),
            init_us: Arc::new(AtomicU64::new(0)),
        },
        input,
    )
}

#[cfg(test)]
pub(super) fn test_fallback(
    job: Job,
    output: &Sender<EncodeJob>,
    replies: &Sender<Outgoing>,
    budgets: &Budgets,
) -> bool {
    fallback(job, "gpu_device", output, replies, budgets)
}

pub(super) fn prepare(
    bridge: &Bridge,
    frame: &IWICBitmapFrameDecode,
    target: (u32, u32),
    orientation: u32,
    request: &NativeRequest,
    budgets: &Budgets,
    metrics: &mut Option<NativeGpuMetrics>,
) -> Option<Prepared> {
    let metrics = metrics.get_or_insert_with(NativeGpuMetrics::default);
    let failure = bridge
        .failure
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(reason) = failure {
        metrics.fallback = Some(reason);
        metrics.init_us = bridge.init_us.swap(0, Ordering::AcqRel);
        return None;
    }
    let started = Instant::now();
    let layout = PlanarLayout::query(frame, target, orientation);
    metrics.cpu_planar_us = micros(started);
    let layout = match layout {
        Ok(Some(layout)) => layout,
        Ok(None) => {
            metrics.fallback = Some("planar_unsupported".into());
            return None;
        }
        Err(error) => {
            metrics.fallback = Some(error.diagnostic_code());
            return None;
        }
    };
    // 一次预留平面、Hash 小图和编码工作集；GPU 失败时仍足够原位执行 WIC CPU 回退。
    let reserved = layout
        .cpu_bytes()
        .saturating_add(encoding_peak(target, 0, request))
        .saturating_add(thumbhash_input_bytes(target));
    let working = match budgets.reserve(reserved) {
        Ok(working) => working,
        Err(_) => {
            metrics.fallback = Some("plane_budget".into());
            return None;
        }
    };
    let started = Instant::now();
    let image = layout.decode(frame);
    metrics.cpu_planar_us += micros(started);
    match image {
        Ok(image) => Some(Prepared {
            image,
            working,
            reserved,
        }),
        Err(_) => {
            metrics.fallback = Some("planar_decode".into());
            None
        }
    }
}

fn gpu_loop(
    input: Receiver<Job>,
    output: Sender<EncodeJob>,
    replies: Sender<Outgoing>,
    budgets: &Budgets,
    max_gpu_bytes: u64,
    failure: Arc<Mutex<Option<String>>>,
    init_us: Arc<AtomicU64>,
) {
    let started = Instant::now();
    let initialized = GpuProcessor::new(max_gpu_bytes);
    init_us.store(micros(started), Ordering::Release);
    let mut processor = match initialized {
        Ok(processor) => Some(processor),
        Err(error) => {
            *failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.diagnostic_code());
            None
        }
    };
    let mut pending = VecDeque::<Job>::new();
    let mut disconnected = false;
    let mut qos = None;
    while !budgets.stopped.load(Ordering::Acquire) {
        refresh_worker_qos(&mut qos);
        if let Some(gpu) = &mut processor {
            match gpu.poll() {
                Ok(Some(done)) => {
                    let Some(mut job) = pending.pop_front() else {
                        budgets.stopped.store(true, Ordering::Release);
                        return;
                    };
                    if done.id != job.source.request.id {
                        budgets.stopped.store(true, Ordering::Release);
                        return;
                    }
                    job.metrics.upload_us = done.upload_us;
                    job.metrics.gpu_us = done.gpu_us;
                    job.metrics.readback_us = done.readback_us;
                    job.metrics.completion_wait_us = done.completion_wait_us;
                    let stats = gpu.stats();
                    job.metrics.reserved_peak_bytes = stats.reserved_peak_bytes;
                    job.metrics.usage_peak_bytes = stats.usage_peak_bytes;
                    job.metrics.budget_bytes = stats.budget_bytes;
                    let expected = job.prepared.image.target_size();
                    let valid = (done.decoded.width, done.decoded.height) == expected
                        && done.decoded.pixels.len() as u64 == rgba_bytes(expected)
                        && rgba_bytes(expected) <= job.source.request.max_pixel_bytes
                        && valid_thumbhash_input(expected, done.thumbhash_input.as_ref());
                    if !valid {
                        if !fallback(job, "gpu_output", &output, &replies, budgets) {
                            return;
                        }
                    } else {
                        job.prepared.working.shrink(
                            encoding_peak(expected, 0, &job.source.request)
                                .saturating_add(thumbhash_input_bytes(expected)),
                        );
                        let result = Ok(EncodePixels {
                            decoded: done.decoded,
                            thumbhash_input: done.thumbhash_input,
                            backend: "wic-d3d11-planar",
                            working: job.prepared.working,
                        });
                        drop((job.source.file, job.prepared.image));
                        if !complete_decode(
                            job.source.request,
                            result,
                            micros(job.started),
                            Some(job.metrics),
                            &output,
                            &replies,
                            budgets,
                        ) {
                            return;
                        }
                    }
                    continue;
                }
                Ok(None) => (),
                Err(error) => {
                    let code = error.diagnostic_code();
                    if error.disables_device() {
                        *failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.clone());
                        // D3D 保留已提交命令的资源引用；设备失效后不再次提交或重建设备。
                        processor = None;
                        for job in pending.drain(..) {
                            if !fallback(job, &code, &output, &replies, budgets) {
                                return;
                            }
                        }
                    } else {
                        // poll 已退役发生错误的队首槽，其余已提交图片仍可继续。
                        let Some(job) = pending.pop_front() else {
                            budgets.stopped.store(true, Ordering::Release);
                            return;
                        };
                        if !fallback(job, &code, &output, &replies, budgets) {
                            return;
                        }
                    }
                    continue;
                }
            }
        }
        if disconnected && pending.is_empty() {
            break;
        }
        let has_slot = processor
            .as_ref()
            .is_none_or(|gpu| gpu.available_slots() > 0);
        if has_slot && !disconnected {
            let next = if pending.is_empty() {
                match input.recv_timeout(TICK) {
                    Ok(job) => Some(job),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => {
                        disconnected = true;
                        None
                    }
                }
            } else {
                match input.try_recv() {
                    Ok(job) => Some(job),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        None
                    }
                }
            };
            if let Some(mut job) = next {
                job.metrics.queue_us = micros(job.queued_at);
                job.metrics.init_us = init_us.swap(0, Ordering::AcqRel);
                if let Some(gpu) = &mut processor {
                    job.metrics.adapter = Some(gpu.adapter_name().into());
                    match gpu.submit(job.source.request.id, &job.prepared.image) {
                        Ok(()) => pending.push_back(job),
                        Err(error) => {
                            let code = error.diagnostic_code();
                            if error.disables_device() {
                                *failure.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(code.clone());
                                processor = None;
                                for pending_job in pending.drain(..) {
                                    if !fallback(pending_job, &code, &output, &replies, budgets) {
                                        return;
                                    }
                                }
                            }
                            if !fallback(job, &code, &output, &replies, budgets) {
                                return;
                            }
                        }
                    }
                } else {
                    let code = failure
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone()
                        .unwrap_or_else(|| "gpu_unavailable".into());
                    if !fallback(job, &code, &output, &replies, budgets) {
                        return;
                    }
                }
                continue;
            }
        }
        if !pending.is_empty() {
            // 仅 GPU 专用线程轮询完成；CPU 解码和编码池继续推进。
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn fallback(
    mut job: Job,
    reason: &str,
    output: &Sender<EncodeJob>,
    replies: &Sender<Outgoing>,
    budgets: &Budgets,
) -> bool {
    job.metrics.fallback = Some(reason.into());
    let Prepared {
        image,
        working,
        reserved,
    } = job.prepared;
    drop(image);
    // 必须带原许可回退；先释放再申请可能与占满队列的解码线程形成等待环。
    let result = decode_wic(
        &mut job.source.file,
        &job.source.request,
        budgets,
        None,
        &mut None,
        Some((working, reserved)),
    )
    .map_err(|error| classify(&error))
    .and_then(|output| match output {
        DecodeOutput::Cpu(decoded) => Ok(decoded),
        DecodeOutput::Gpu(_) => Err(NativeFailure::Unavailable),
    });
    drop(job.source.file);
    complete_decode(
        job.source.request,
        result.map(EncodePixels::from),
        micros(job.started),
        Some(job.metrics),
        output,
        replies,
        budgets,
    )
}

fn thumbhash_input_bytes(size: (u32, u32)) -> u64 {
    thumbhash_resize_target(size.0, size.1).map_or(0, rgba_bytes)
}

fn valid_thumbhash_input(display: (u32, u32), input: Option<&DecodedImage>) -> bool {
    match (thumbhash_resize_target(display.0, display.1), input) {
        (Some(expected), Some(input)) => {
            (input.width, input.height) == expected
                && input.pixels.len() as u64 == rgba_bytes(expected)
                && input.icc.is_none()
        }
        (None, None) => true,
        _ => false,
    }
}
