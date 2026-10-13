//! 图片阶段衔接：宿主只授权源、管理订阅和写入编码结果。

use super::benchmark::{MeasuredOutcome, PipelineMetrics};
use super::generator::EncodedImage;
use super::service::{failed_result, finish_with_ai, ImageRequest, Job, Shared, HEAVY_CAPACITY};
use crate::db::models::ThumbResult;
use crate::state::AppState;
use crossbeam_channel::{bounded, Receiver};
use std::sync::{Arc, Weak};
use std::time::Instant;

pub(super) fn direct(request: &ImageRequest) -> bool {
    matches!(
        request.item.file_format.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "svg" | "avif"
    ) && (request.config.strategy == "direct"
        || (request.item.file_size >= 0
            && request.item.file_size as u64 <= request.config.skip_max_bytes))
}

pub(super) fn emit_ai(request: &ImageRequest) -> bool {
    request.config.ai_hq_cache
        && request.cached_ai_path.is_none()
        && request
            .cached_result
            .as_ref()
            .is_none_or(|result| result.thumb_status != 2)
}

pub(super) fn needs_pixels(request: &ImageRequest) -> bool {
    // 直显沿用已有占位数据；只有显式缺失的 AI 产物仍需解码。
    emit_ai(request) || (request.cached_result.is_none() && !direct(request))
}

pub(super) fn heavy_units(request: &ImageRequest) -> usize {
    let item = &request.item;
    if item.width <= 0 || item.height <= 0 {
        return HEAVY_CAPACITY;
    }
    let format = item.file_format.to_ascii_lowercase();
    // 原生 JPEG 显示请求只解码到缩略图档位，原图面积不应把它们全局串行化。
    // AI 产物、软件优先和未知尺寸仍走保守准入；worker 的字节及进程预算继续生效。
    let native_display = cfg!(windows)
        && super::native_protocol::NativeDecodePolicy::from_strategy(&request.config.strategy)
            == super::native_protocol::NativeDecodePolicy::NativeFirst
        && !emit_ai(request);
    if native_display {
        match format.as_str() {
            "jpg" | "jpeg" => return 0,
            // HEIC 解码器仍有显著内部内存成本，只允许四路并发，不像 JPEG 完全豁免。
            "heic" | "heif" => return 1,
            _ => {}
        }
    }
    if (item.width as u64).saturating_mul(item.height as u64) > 40_000_000
        || !matches!(format.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
    {
        HEAVY_CAPACITY
    } else {
        0
    }
}

pub(super) fn valid(shared: &Shared, state: &Weak<AppState>, job: &Job) -> bool {
    shared.interested(&job.key, job.id)
        && state.upgrade().is_some_and(|state| {
            state
                .with_database_lifecycle_read(job.request.epoch, || {
                    job.request
                        .config
                        .same_policy(&state.thumb_config.read().unwrap_or_else(|e| e.into_inner()))
                })
                .unwrap_or(false)
        })
}

pub(super) fn base_result(request: &ImageRequest) -> ThumbResult {
    request
        .cached_result
        .clone()
        .unwrap_or_else(|| ThumbResult {
            item_id: request.item.id,
            source_revision: request.item.source_revision,
            cache_key: request.item.cache_key,
            thumb_status: if direct(request) { 3 } else { 1 },
            thumb_path: Some(if direct(request) {
                request.abs_path.to_string_lossy().replace('\\', "/")
            } else {
                request.config.display_path(
                    request.item.id,
                    request.item.source_revision,
                    request.item.cache_key,
                )
            }),
            thumbhash: if direct(request) {
                request.item.thumbhash.clone()
            } else {
                None
            },
        })
}

pub(super) fn outcome(request: &ImageRequest) -> MeasuredOutcome {
    if request.cached_result.is_some() {
        MeasuredOutcome::CacheHit
    } else if direct(request) {
        MeasuredOutcome::Direct
    } else {
        MeasuredOutcome::Generated
    }
}

pub(super) fn finish_ready(shared: &Shared, mut job: Job) {
    job.metrics.item.outcome = Some(outcome(&job.request));
    let result = base_result(&job.request);
    let ai = job.request.cached_ai_path.clone();
    finish_with_ai(shared, job, Some(result), ai);
}

pub(super) struct WriteWork {
    pub job: Job,
    pub payload: EncodedImage,
    // 额度与输出一起到写入完成；不能在移交 Vec 前归还。
    #[cfg(windows)]
    pub _ticket: super::native_worker::NativeTicket,
    #[cfg(not(windows))]
    pub bytes: super::byte_budget::BytePermit,
}

#[cfg(windows)]
struct WriteBacklog<T> {
    pending: std::collections::VecDeque<T>,
    window: usize,
}

#[cfg(windows)]
impl<T> WriteBacklog<T> {
    fn new(window: usize) -> Self {
        Self {
            pending: std::collections::VecDeque::new(),
            window,
        }
    }

    fn push(&mut self, work: T) {
        debug_assert!(self.pending.len() < self.window);
        self.pending.push_back(work);
    }

    fn remaining(&self, active: usize) -> usize {
        self.window - active - self.pending.len()
    }

    fn flush(&mut self, sender: &crossbeam_channel::Sender<T>) -> Result<usize, ()> {
        let mut sent = 0;
        while let Some(work) = self.pending.pop_front() {
            match sender.try_send(work) {
                Ok(()) => sent += 1,
                Err(crossbeam_channel::TrySendError::Full(work)) => {
                    self.pending.push_front(work);
                    break;
                }
                Err(crossbeam_channel::TrySendError::Disconnected(work)) => {
                    self.pending.push_front(work);
                    return Err(());
                }
            }
        }
        Ok(sent)
    }

    fn take_pending(&mut self) -> std::collections::VecDeque<T> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct Payload {
        id: usize,
        released: Arc<AtomicUsize>,
    }

    impl Drop for Payload {
        fn drop(&mut self) {
            self.released.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn payload(id: usize, released: &Arc<AtomicUsize>) -> Payload {
        Payload {
            id,
            released: Arc::clone(released),
        }
    }

    #[test]
    fn full_write_queue_retains_payload_and_window_without_blocking_dispatch() {
        let released = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = bounded(1);
        sender.send(payload(0, &released)).unwrap();
        let mut backlog = WriteBacklog::new(2);
        backlog.push(payload(1, &released));
        let (done_tx, done_rx) = bounded(1);
        let dispatcher = std::thread::spawn(move || {
            let result = backlog.flush(&sender);
            done_tx.send((backlog, sender, result)).unwrap();
        });
        let progress = done_rx.recv_timeout(Duration::from_millis(150));
        let nonblocking = progress.is_ok();
        let (mut backlog, sender, result) = match progress {
            Ok(progress) => progress,
            Err(_) => {
                drop(receiver.recv().unwrap());
                done_rx.recv_timeout(Duration::from_secs(2)).unwrap()
            }
        };
        dispatcher.join().unwrap();
        assert!(
            nonblocking,
            "a full writer queue must not block event collection"
        );
        assert_eq!(result, Ok(0));
        assert_eq!(released.load(Ordering::SeqCst), 0);
        assert_eq!(backlog.remaining(1), 0);
        drop(receiver.recv().unwrap());
        assert_eq!(backlog.flush(&sender), Ok(1));
        assert_eq!(backlog.remaining(1), 1);
        let output = receiver.recv().unwrap();
        assert_eq!(output.id, 1);
        drop(output);
        assert_eq!(released.load(Ordering::SeqCst), 2);
    }
}

pub(super) fn start(
    state: &Arc<AppState>,
    shared: Arc<Shared>,
    metrics: Arc<PipelineMetrics>,
    width: usize,
) {
    let writers = width.clamp(1, 4);
    let (sender, receiver) = bounded::<WriteWork>(2 * writers);
    for _ in 0..writers {
        let shared = Arc::clone(&shared);
        let state = Arc::downgrade(state);
        let receiver = receiver.clone();
        std::thread::spawn(move || write_loop(shared, state, receiver));
    }
    #[cfg(windows)]
    {
        let state = Arc::downgrade(state);
        std::thread::spawn(move || dispatch(shared, state, sender, metrics, width));
    }
    #[cfg(not(windows))]
    super::portable_execution::start(state, shared, sender, metrics, width);
}

fn write_loop(shared: Arc<Shared>, state: Weak<AppState>, receiver: Receiver<WriteWork>) {
    let mut qos = None;
    while let Ok(mut work) = receiver.recv() {
        // 出队就已腾出写盘入口，及时唤醒派发者，不必等本文件写完。
        #[cfg(windows)]
        shared.wake.notify();
        work.job.metrics.dequeue();
        if !valid(&shared, &state, &work.job) {
            finish_with_ai(&shared, work.job, None, None);
            continue;
        }
        super::qos::refresh_worker_qos(&mut qos);
        let request = &work.job.request;
        let mut result = base_result(request);
        let mut ai_path = request.cached_ai_path.clone();
        let started = request.measured.then(Instant::now);
        let Some(write_state) = state.upgrade() else {
            finish_with_ai(&shared, work.job, None, None);
            continue;
        };
        // 清缓存的生命周期写锁等待正在落盘的短读区结束，旧代次不能随后重建目录。
        let written =
            write_state.with_database_lifecycle_read(request.epoch, || -> std::io::Result<()> {
                if !work.payload.display.is_empty() {
                    let relative = request.config.display_path(
                        request.item.id,
                        request.item.source_revision,
                        request.item.cache_key,
                    );
                    write_state.artifact_writer.write(
                        &request.config.cache_dir,
                        &format!("thumbnails/{relative}"),
                        &work.payload.display,
                    )?;
                    result.thumb_path = Some(relative);
                }
                if let Some(bytes) = &work.payload.ai {
                    let relative = request.config.ai_path(
                        request.item.id,
                        request.item.source_revision,
                        request.item.cache_key,
                    );
                    write_state.artifact_writer.write(
                        &request.config.cache_dir,
                        &relative,
                        bytes,
                    )?;
                    ai_path = Some(relative);
                }
                Ok(())
            });
        if let Some(started) = started {
            work.job.metrics.item.write_us += started.elapsed().as_micros() as u64;
        }
        if !matches!(written, Some(Ok(()))) || !valid(&shared, &state, &work.job) {
            // 磁盘/权限故障可重试，不将图片永久登记为内容损坏。
            finish_with_ai(&shared, work.job, None, None);
            continue;
        }
        if work.payload.thumbhash.is_some() {
            result.thumbhash = work.payload.thumbhash;
        }
        work.job.metrics.item.ai_written += u64::from(work.payload.ai.is_some());
        work.job.metrics.item.ai_cache_hit += u64::from(request.cached_ai_path.is_some());
        work.job.metrics.item.outcome = Some(outcome(request));
        finish_with_ai(&shared, work.job, Some(result), ai_path);
    }
}

#[cfg(windows)]
fn native_request(job: &Job) -> Option<super::native_protocol::NativeRequest> {
    let request = &job.request;
    let ai = emit_ai(request);
    let display = request.cached_result.is_none() && !direct(request);
    let size = request.config.size as u64;
    let (w, h) = (
        request.item.width.max(0) as u64,
        request.item.height.max(0) as u64,
    );
    let known = w != 0 && h != 0;
    let target = if ai && known {
        size.max(
            (request.config.ai_cache_short_edge as u64)
                .checked_mul(w.max(h))?
                .div_ceil(w.min(h)),
        )
    } else {
        size
    };
    let target = if known { target.min(w.max(h)) } else { target };
    // 协议上限按当前目标估算；源尺寸仍由 worker 核验，不信任数据库像素元数据。
    let pixels = if known {
        let short = w.min(h).checked_mul(target)?.div_ceil(w.max(h)).max(1);
        target.checked_mul(short)?.checked_mul(4)?
    } else if ai {
        32 * 1024 * 1024
    } else {
        size * size * 4
    };
    let display_bytes = if display {
        size * size * 4 + 64 * 1024
    } else {
        0
    };
    let ai_bytes = if ai {
        pixels.checked_add(64 * 1024)?
    } else {
        0
    };
    let encoded = display_bytes.checked_add(ai_bytes)?;
    if encoded > super::native_protocol::MAX_ARTIFACT_BYTES as u64 {
        return None;
    }
    Some(super::native_protocol::NativeRequest {
        id: job.id,
        file_handle: 0,
        format: request.item.file_format.clone(),
        decode_policy: super::native_protocol::NativeDecodePolicy::from_strategy(
            &request.config.strategy,
        ),
        output_size: request.config.size,
        decode_long_edge: (target as u32).max(request.config.size),
        webp_quality: request.config.webp_quality,
        ai_cache_short_edge: request.config.ai_cache_short_edge,
        emit_ai: ai,
        emit_display: display,
        max_encoded_bytes: encoded,
        max_pixel_bytes: pixels,
        foreground: super::qos::is_app_foreground(),
    })
}

#[cfg(windows)]
fn dispatch(
    shared: Arc<Shared>,
    state: Weak<AppState>,
    sender: crossbeam_channel::Sender<WriteWork>,
    metrics: Arc<PipelineMetrics>,
    width: usize,
) {
    use super::native_protocol::{NativeEvent, NativeFailure};
    use super::native_worker::{NativeExecutor, NativeTicket};
    use tokio_util::sync::CancellationToken;
    struct Active {
        job: Job,
        ticket: NativeTicket,
        cancel: CancellationToken,
    }
    let executor = NativeExecutor::new(width, shared.wake.clone());
    let mut active: Vec<Active> = Vec::new();
    let mut backlog = WriteBacklog::new(4 * width);
    loop {
        shared.wake.clear();
        let mut advanced = false;
        let mut index = 0;
        while index < active.len() {
            if !valid(&shared, &state, &active[index].job) {
                active[index].cancel.cancel();
            }
            let reply = active[index].ticket.receiver().try_recv();
            match reply {
                Ok(reply) => {
                    advanced = true;
                    match reply.event {
                        NativeEvent::Decoded { .. } => {
                            active[index].job.decoded();
                            index += 1;
                        }
                        NativeEvent::Finished {
                            failure,
                            backend,
                            decode_us,
                            encode_us,
                            thumbhash,
                            gpu,
                            ..
                        } => {
                            let mut work = active.swap_remove(index);
                            // 包括 kill+wait 后的故障终态；只在真实源读取结束后归还。
                            work.job.decoded();
                            work.job.metrics.dequeue();
                            work.job.metrics.queue_us = work
                                .job
                                .metrics
                                .queue_us
                                .saturating_sub(decode_us.saturating_add(encode_us));
                            work.job.metrics.item.backend = Some(backend);
                            work.job.metrics.item.decode_us += decode_us;
                            work.job.metrics.item.encode_us += encode_us;
                            work.job.metrics.item.gpu = gpu;
                            if let Some(failure) = failure {
                                let permanent = matches!(
                                    failure,
                                    NativeFailure::Content | NativeFailure::Unsupported
                                );
                                if permanent
                                    && valid(&shared, &state, &work.job)
                                    && (work.job.request.cached_result.is_some()
                                        || direct(&work.job.request))
                                {
                                    finish_ready(&shared, work.job);
                                    continue;
                                }
                                let result = if permanent && valid(&shared, &state, &work.job) {
                                    work.job.metrics.item.outcome =
                                        Some(if matches!(failure, NativeFailure::Unsupported) {
                                            MeasuredOutcome::Unsupported
                                        } else {
                                            MeasuredOutcome::Failed
                                        });
                                    Some(failed_result(&work.job.request.item))
                                } else {
                                    None
                                };
                                finish_with_ai(&shared, work.job, result, None);
                            } else {
                                work.job.metrics.enqueue();
                                let payload = EncodedImage {
                                    display: reply.display,
                                    ai: (!reply.ai.is_empty()).then_some(reply.ai),
                                    thumbhash,
                                };
                                // Ticket 随输出保留到落盘结束，暂存仍占原在途窗口与字节额度。
                                backlog.push(WriteWork {
                                    job: work.job,
                                    payload,
                                    _ticket: work.ticket,
                                });
                            }
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => index += 1,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    let work = active.swap_remove(index);
                    finish_with_ai(&shared, work.job, None, None);
                }
            }
        }
        if shared.stopped() {
            for work in backlog.take_pending() {
                finish_with_ai(&shared, work.job, None, None);
            }
            if active.is_empty() {
                return;
            }
        }
        match backlog.flush(&sender) {
            Ok(sent) => advanced |= sent > 0,
            Err(()) => {
                for work in active.drain(..) {
                    work.cancel.cancel();
                    finish_with_ai(&shared, work.job, None, None);
                }
                for work in backlog.take_pending() {
                    finish_with_ai(&shared, work.job, None, None);
                }
                shared.stop();
                return;
            }
        }
        // 先收取全部已就绪事件，再补有界窗口，避免回包被准入循环饿死。
        for _ in 0..backlog.remaining(active.len()) {
            let Some(mut job) = shared.try_take() else {
                break;
            };
            if !valid(&shared, &state, &job) {
                finish_with_ai(&shared, job, None, None);
                continue;
            }
            if !needs_pixels(&job.request) {
                finish_ready(&shared, job);
                advanced = true;
                continue;
            }
            let Some(request) = native_request(&job) else {
                finish_with_ai(&shared, job, None, None);
                continue;
            };
            let file = crate::export::open_authorized_source(
                &job.request.root_path,
                &job.request.relative_dir,
                &job.request.item.file_name,
            );
            let Ok(file) = file else {
                finish_with_ai(&shared, job, None, None);
                continue;
            };
            let cancel = CancellationToken::new();
            match executor.submit(request, file, &cancel) {
                Ok(ticket) => {
                    job.metrics.enqueue();
                    active.push(Active {
                        job,
                        ticket,
                        cancel,
                    });
                    advanced = true;
                }
                Err(NativeFailure::Resource) => {
                    shared.requeue(job);
                    break;
                }
                Err(_) => finish_with_ai(&shared, job, None, None),
            }
        }
        let native = executor.metrics();
        use std::sync::atomic::Ordering::Relaxed;
        metrics
            .decode
            .peak
            .fetch_max(native.stages.decode_active_peak, Relaxed);
        metrics
            .encode
            .peak
            .fetch_max(native.stages.encode_active_peak, Relaxed);
        metrics
            .encode_queue_peak
            .fetch_max(native.stages.decoded_queue_peak, Relaxed);
        metrics
            .working_bytes_peak
            .fetch_max(native.stages.working_bytes_peak, Relaxed);
        metrics
            .encoded_bytes_peak
            .fetch_max(native.stages.encoded_bytes_peak, Relaxed);
        metrics
            .worker_private_bytes_peak
            .fetch_max(native.worker_private_bytes_peak, Relaxed);
        metrics
            .host_output_bytes_peak
            .fetch_max(native.host_output_bytes_peak, Relaxed);
        if !advanced {
            shared.wake.wait();
        }
    }
}
