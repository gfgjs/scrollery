//! 非 Windows 图片阶段：ImageRs 解码与既有编码器分别常驻，额度随缓冲移交。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, SendTimeoutError, Sender};

use crate::engine::image_rs::{resize_dimensions, ImageRsEngine};
use crate::engine::traits::{DecodedImage, ResizeHint};
use crate::error::AppError;
use crate::state::AppState;

use super::benchmark::{MeasuredOutcome, PipelineMetrics};
use super::byte_budget::{ByteBudget, BytePermit};
use super::execution::{direct, emit_ai, finish_ready, needs_pixels, valid, WriteWork};
use super::generator::{encode_image_payload, ThumbConfig};
use super::service::{failed_result, finish_with_ai, Job, Shared, TaskKey};

const MIB: u64 = 1024 * 1024;
const WORKING_BYTES: u64 = 512 * MIB;
const PIXEL_BYTES: u64 = 256 * MIB;
const ENCODE_BYTES: u64 = 256 * MIB;
const METADATA_BYTES: u64 = 16 * MIB;
const POLL: Duration = Duration::from_millis(50);

struct Budgets {
    working: Arc<ByteBudget>,
    pixels: Arc<ByteBudget>,
    encoded: Arc<ByteBudget>,
}

// 只保存身份快照；监视线程不持有 Job、源许可或数据库生命周期锁。
struct Interest {
    key: TaskKey,
    id: u64,
    epoch: u64,
    config: ThumbConfig,
    stopped: AtomicBool,
}

impl Interest {
    fn refresh(&self, shared: &Shared, state: &Weak<AppState>) {
        let active = shared.interested(&self.key, self.id)
            && state.upgrade().is_some_and(|state| {
                state
                    .with_database_lifecycle_read(self.epoch, || {
                        self.config.same_policy(
                            &state.thumb_config.read().unwrap_or_else(|e| e.into_inner()),
                        )
                    })
                    .unwrap_or(false)
            });
        if !active {
            self.stopped.store(true, Ordering::Release);
        }
    }
}

type Interests = Arc<Mutex<Vec<Weak<Interest>>>>;

struct Pixels {
    image: DecodedImage,
    // Rust 按字段顺序析构，先释放真实缓冲再归还额度。
    bytes: BytePermit,
}

struct EncodeWork {
    job: Job,
    pixels: Pixels,
    interest: Arc<Interest>,
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Transient,
    Content,
    Unsupported,
}

fn classify(error: AppError) -> Failure {
    match error {
        AppError::UnsupportedFormat(_) | AppError::Engine(image::ImageError::Unsupported(_)) => {
            Failure::Unsupported
        }
        AppError::Engine(image::ImageError::Decoding(_)) => Failure::Content,
        AppError::Engine(image::ImageError::IoError(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::InvalidData
            ) =>
        {
            Failure::Content
        }
        // 资源限制、源 IO 和内部/panic 故障都允许以后重试。
        _ => Failure::Transient,
    }
}

fn guarded<T>(action: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    catch_unwind(AssertUnwindSafe(action)).unwrap_or(Err(Failure::Transient))
}

/// 启动 D=E=width 的常驻池，小图队列最多 2E 项。
pub(super) fn start(
    state: &Arc<AppState>,
    shared: Arc<Shared>,
    sender: Sender<WriteWork>,
    metrics: Arc<PipelineMetrics>,
    width: usize,
) {
    let budgets = Arc::new(Budgets {
        working: ByteBudget::new(WORKING_BYTES),
        pixels: ByteBudget::new(PIXEL_BYTES),
        encoded: ByteBudget::new(ENCODE_BYTES),
    });
    let interests: Interests = Arc::default();
    let (decoded_sender, decoded_receiver) = bounded(2 * width);
    for _ in 0..width {
        let shared = Arc::clone(&shared);
        let state = Arc::downgrade(state);
        let budgets = Arc::clone(&budgets);
        let metrics = Arc::clone(&metrics);
        let receiver = decoded_receiver.clone();
        let sender = sender.clone();
        std::thread::spawn(move || encode_loop(shared, state, budgets, metrics, receiver, sender));
    }
    for _ in 0..width {
        let shared = Arc::clone(&shared);
        let state = Arc::downgrade(state);
        let budgets = Arc::clone(&budgets);
        let metrics = Arc::clone(&metrics);
        let interests = Arc::clone(&interests);
        let sender = decoded_sender.clone();
        std::thread::spawn(move || decode_loop(shared, state, budgets, metrics, interests, sender));
    }
    let state = Arc::downgrade(state);
    std::thread::spawn(move || loop {
        // ByteBudget 的等待只接收 AtomicBool；统一监视器让退订和换库也能打断等待。
        let active: Vec<_> = {
            let mut interests = interests.lock().unwrap_or_else(|e| e.into_inner());
            interests.retain(|interest| interest.strong_count() != 0);
            interests.iter().filter_map(Weak::upgrade).collect()
        };
        for interest in &active {
            interest.refresh(&shared, &state);
        }
        // 所有解码线程退出且没有在途缓冲后才结束；不能漏掉关停竞态中新注册的等待者。
        if Arc::strong_count(&interests) == 1 && active.is_empty() {
            break;
        }
        std::thread::sleep(POLL);
    });
}

fn decode_loop(
    shared: Arc<Shared>,
    state: Weak<AppState>,
    budgets: Arc<Budgets>,
    metrics: Arc<PipelineMetrics>,
    interests: Interests,
    sender: Sender<EncodeWork>,
) {
    let mut qos = None;
    while !shared.stopped() && state.upgrade().is_some() {
        let Some(mut job) = shared.try_take() else {
            shared.wait();
            continue;
        };
        if !valid(&shared, &state, &job) {
            finish_with_ai(&shared, job, None, None);
            continue;
        }
        if !needs_pixels(&job.request) {
            finish_ready(&shared, job);
            continue;
        }
        let interest = Arc::new(Interest {
            key: job.key.clone(),
            id: job.id,
            epoch: job.request.epoch,
            config: job.request.config.clone(),
            stopped: AtomicBool::new(false),
        });
        interests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::downgrade(&interest));
        super::qos::refresh_worker_qos(&mut qos);
        job.metrics.item.backend = Some("image-rs".into());
        let decoded = guarded(|| decode(&mut job, &shared, &state, &budgets, &interest, &metrics));
        // 包括 codec panic：栈已展开、文件已关闭，立即释放源盘和 heavy 许可。
        job.decoded();
        match decoded {
            Ok(pixels) if valid(&shared, &state, &job) => {
                if job.request.measured {
                    metrics.encode_queue_peak.fetch_max(
                        (sender.len() + 1).min(sender.capacity().unwrap()) as u64,
                        Ordering::Relaxed,
                    );
                }
                job.metrics.enqueue();
                let work = EncodeWork {
                    job,
                    pixels,
                    interest,
                };
                if let Err(work) =
                    send_while(&sender, work, |work| valid(&shared, &state, &work.job))
                {
                    finish_with_ai(&shared, work.job, None, None);
                }
            }
            Ok(_) => finish_with_ai(&shared, job, None, None),
            Err(failure) => finish_failure(&shared, &state, job, failure),
        }
    }
}

fn check(shared: &Shared, state: &Weak<AppState>, job: &Job) -> Result<(), Failure> {
    valid(shared, state, job)
        .then_some(())
        .ok_or(Failure::Transient)
}

fn reserve(
    budget: &Arc<ByteBudget>,
    bytes: u64,
    interest: &Interest,
) -> Result<BytePermit, Failure> {
    budget
        .acquire(bytes, &interest.stopped)
        .ok_or(Failure::Transient)
}

fn decode(
    job: &mut Job,
    shared: &Shared,
    state: &Weak<AppState>,
    budgets: &Budgets,
    interest: &Interest,
    metrics: &PipelineMetrics,
) -> Result<Pixels, Failure> {
    check(shared, state, job)?;
    let request = &job.request;
    let mut file = crate::export::open_authorized_source(
        &request.root_path,
        &request.relative_dir,
        &request.item.file_name,
    )
    .map_err(|_| Failure::Transient)?;
    // JPEG 探测会读入压缩流；为容量增长及元数据预留额度，再构造 decoder。
    let input_bytes = file
        .metadata()
        .map_err(|_| Failure::Transient)?
        .len()
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(METADATA_BYTES))
        .ok_or(Failure::Transient)?;
    let probe = reserve(&budgets.working, input_bytes, interest)?;
    check(shared, state, job)?;
    let info = ImageRsEngine::working_set_bytes(&mut file, input_bytes).map_err(classify)?;
    drop(probe);
    let hint = decode_hint(info.width, info.height, &request.config, emit_ai(request))?;
    let working_bytes = info.bytes_for_resize(Some(hint)).map_err(classify)?;
    let (width, height) =
        resize_dimensions(info.width, info.height, Some(hint)).map_err(classify)?;
    let pixel_bytes = u64::from(width) * u64::from(height) * 4 + METADATA_BYTES;
    let mut bytes = reserve(&budgets.pixels, pixel_bytes, interest)?;
    // 探测结果仅含整数；先释放探测额度再申请完整工作集，避免持小额度等增量死锁。
    let working = reserve(
        &budgets.working,
        working_bytes.saturating_add(input_bytes),
        interest,
    )?;
    check(shared, state, job)?;
    let measured = job.request.measured;
    let started = measured.then(Instant::now);
    let result = {
        let _active = metrics.decode.enter(measured);
        guarded(|| {
            ImageRsEngine::decode_open_file_bounded(
                &mut file,
                &job.request.item.file_format,
                Some(hint),
                working_bytes,
            )
            .map_err(classify)
        })
    };
    if let Some(started) = started {
        job.metrics.item.decode_us += started.elapsed().as_micros() as u64;
    }
    // 实际源读取在这里结束；后面的像素交接不再占用源许可。
    drop(file);
    job.decoded();
    drop(working);
    let image = result?;
    let actual =
        image.pixels.capacity() as u64 + image.icc.as_ref().map_or(0, |icc| icc.capacity() as u64);
    if actual > pixel_bytes {
        return Err(Failure::Transient);
    }
    bytes.shrink(actual);
    Ok(Pixels { image, bytes })
}

fn decode_hint(
    width: u32,
    height: u32,
    config: &ThumbConfig,
    ai: bool,
) -> Result<ResizeHint, Failure> {
    let long = width.max(height);
    let short = width.min(height);
    if short == 0 {
        return Err(Failure::Transient);
    }
    let ai_long = if ai {
        (u64::from(config.ai_cache_short_edge) * u64::from(long)).div_ceil(u64::from(short))
    } else {
        0
    };
    Ok(ResizeHint::LongEdge(
        u64::from(config.size).max(ai_long).min(u64::from(long)) as u32,
    ))
}

fn encode_loop(
    shared: Arc<Shared>,
    state: Weak<AppState>,
    budgets: Arc<Budgets>,
    metrics: Arc<PipelineMetrics>,
    receiver: Receiver<EncodeWork>,
    sender: Sender<WriteWork>,
) {
    let mut qos = None;
    while let Ok(mut work) = receiver.recv() {
        work.job.metrics.dequeue();
        if !valid(&shared, &state, &work.job) {
            finish_with_ai(&shared, work.job, None, None);
            continue;
        }
        super::qos::refresh_worker_qos(&mut qos);
        let EncodeWork {
            mut job,
            pixels,
            interest,
        } = work;
        let Pixels {
            image,
            bytes: pixel_permit,
        } = pixels;
        let encoded = guarded(|| {
            // 既有编码器含 FIR/CMS 临时图、WebP 原生输出与 Vec 副本、JPEG 回退。
            // 两种输出均不放大，以源 RGBA 的 16 倍覆盖这些同时存活的已知缓冲；
            // codec 私有分配不是此账本的进程硬上限。
            let reservation = (image.pixels.len() as u64)
                .checked_mul(16)
                .and_then(|bytes| bytes.checked_add(MIB))
                .ok_or(Failure::Transient)?;
            let mut bytes = reserve(&budgets.encoded, reservation, &interest)?;
            check(&shared, &state, &job)?;
            let display = job.request.cached_result.is_none() && !direct(&job.request);
            let ai = emit_ai(&job.request);
            let measured = job.request.measured;
            let _active = metrics.encode.enter(measured);
            let payload = encode_image_payload(
                image,
                None,
                &job.request.config,
                display,
                ai,
                measured.then_some(&mut job.metrics.item),
            )
            .map_err(|_| Failure::Transient)?;
            let actual = payload.display.capacity() as u64
                + payload
                    .ai
                    .as_ref()
                    .map_or(0, |bytes| bytes.capacity() as u64)
                + payload
                    .thumbhash
                    .as_ref()
                    .map_or(0, |bytes| bytes.capacity() as u64);
            if actual > reservation {
                return Err(Failure::Transient);
            }
            bytes.shrink(actual);
            Ok((payload, bytes))
        });
        // encode 已消耗或释放 image，即使 panic 也先展开 codec 再归还小图额度。
        drop(pixel_permit);
        match encoded {
            Ok((payload, bytes)) => {
                job.metrics.enqueue();
                let work = WriteWork {
                    job,
                    payload,
                    bytes,
                };
                if let Err(work) =
                    send_while(&sender, work, |work| valid(&shared, &state, &work.job))
                {
                    finish_with_ai(&shared, work.job, None, None);
                }
            }
            Err(failure) => finish_failure(&shared, &state, job, failure),
        }
    }
}

fn send_while<T>(sender: &Sender<T>, mut work: T, keep: impl Fn(&T) -> bool) -> Result<(), T> {
    loop {
        if !keep(&work) {
            return Err(work);
        }
        match sender.send_timeout(work, POLL) {
            Ok(()) => return Ok(()),
            Err(SendTimeoutError::Timeout(value)) => work = value,
            Err(SendTimeoutError::Disconnected(value)) => return Err(value),
        }
    }
}

fn finish_failure(shared: &Shared, state: &Weak<AppState>, mut job: Job, failure: Failure) {
    let permanent = matches!(failure, Failure::Content | Failure::Unsupported);
    if permanent
        && valid(shared, state, &job)
        && (job.request.cached_result.is_some() || direct(&job.request))
    {
        finish_ready(shared, job);
        return;
    }
    let result = if permanent && valid(shared, state, &job) {
        job.metrics.item.outcome = Some(if failure == Failure::Unsupported {
            MeasuredOutcome::Unsupported
        } else {
            MeasuredOutcome::Failed
        });
        Some(failed_result(&job.request.item))
    } else {
        None
    };
    finish_with_ai(shared, job, result, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_panic_releases_budget_and_next_item_runs() {
        let budget = ByteBudget::new(10);
        let stopped = AtomicBool::new(false);
        assert_eq!(
            guarded::<()>(|| {
                let _permit = budget.acquire(10, &stopped).unwrap();
                panic!("codec panic");
            }),
            Err(Failure::Transient)
        );
        assert!(guarded(|| budget.acquire(10, &stopped).ok_or(Failure::Transient)).is_ok());
    }
}
