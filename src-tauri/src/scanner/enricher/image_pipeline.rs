// src-tauri/src/scanner/enricher/image_pipeline.rs
//! 图片富化批内的有界「头读 → 解析」重叠流水线。
//!
//! 旧实现先把整批（最多 1000 项）的头读与文件句柄全部收齐，再进入整批解析：批内首个结果要
//! 等最慢的那个头读，单批在途句柄峰值也等于批大小（最坏 1000 个句柄、约 63~250 MiB 头字节，
//! 即 64 KiB×1000 到 256 KiB×1000）。
//! 这里把同一批切成 64 项小块：一个生产者线程在既有 `header_pool` 上顺次读块，读一块交一块
//! （`sync_channel` 容量 1）；消费侧在 `std::thread::scope` 主线程上用既有 `img_pool` 解析该块，
//! 结果逐块顺序追加，每项解析完的载荷（头缓冲 + 文件句柄）当场释放。批的选批、写回、事件发布
//! 仍在批边界上，一次批不会提前写回部分结果。
//!
//! 并发上界：生产中 1 块 + channel 排队 1 块 + 消费中 1 块 = [`MAX_INFLIGHT_HEADER_CHUNKS`] 块，
//! 因此单批在途头读不超过 192 项（最坏 192×256 KiB = 48 MiB，通常更低）。两个 rayon 池复用富化
//! 阶段既有线程预算；本模块每批只 spawn 一个短生命周期生产者线程（批末 join），不为每个文件或
//! 每块建线程，也不跨块重建池。
//!
//! 退出契约：取消、消费侧提前收工、任一侧 panic 都不得留下阻塞线程或漏归还的在途块计数
//! （[`InflightChunk`] 随块生命周期 RAII 记账）。取消只在块边界生效：已开始的同步文件 I/O 必须
//! 自己返回，生产者线程才能被 join 收工——本模块不提供中止阻塞 I/O 的能力。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, Result};

/// 头读块大小（项）：生产者按块读取并在块边界移交消费侧，也是取消/收工的检查粒度。
pub(crate) const HEADER_CHUNK_ITEMS: usize = 64;

/// 在途头读块上界：生产中 1 块 + channel 排队 1 块 + 消费中 1 块。
pub(crate) const MAX_INFLIGHT_HEADER_CHUNKS: usize = 3;

/// 图片批流水线的累计分段计时与在途块峰值。
///
/// 各 `*_ns` 都是**累计服务时间**：头读与解析在块级真重叠，禁止相加当 wall time；
/// `producer_backpressure_ns`（生产侧等消费腾位置）与 `consumer_wait_ns`（消费侧等头读）
/// 才是空转等待。计数用原子以便跨批累加，峰值取全段最大值。
#[derive(Debug, Default)]
pub(crate) struct PipelineTiming {
    header_read_ns: AtomicU64,
    parse_ns: AtomicU64,
    producer_backpressure_ns: AtomicU64,
    consumer_wait_ns: AtomicU64,
    chunks_produced: AtomicU64,
    chunks_consumed: AtomicU64,
    in_flight_chunks: AtomicUsize,
    peak_inflight_chunks: AtomicUsize,
}

/// [`PipelineTiming`] 的只读快照：结构化日志字段与断言用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PipelineTimingSnapshot {
    pub header_read_ns: u64,
    pub parse_ns: u64,
    pub producer_backpressure_ns: u64,
    pub consumer_wait_ns: u64,
    pub chunks_produced: u64,
    pub chunks_consumed: u64,
    /// 当前在途块数：正常收工时必须为 0。
    pub in_flight_chunks: usize,
    pub peak_inflight_chunks: usize,
}

impl PipelineTiming {
    pub(crate) fn snapshot(&self) -> PipelineTimingSnapshot {
        PipelineTimingSnapshot {
            header_read_ns: self.header_read_ns.load(Ordering::Relaxed),
            parse_ns: self.parse_ns.load(Ordering::Relaxed),
            producer_backpressure_ns: self.producer_backpressure_ns.load(Ordering::Relaxed),
            consumer_wait_ns: self.consumer_wait_ns.load(Ordering::Relaxed),
            chunks_produced: self.chunks_produced.load(Ordering::Relaxed),
            chunks_consumed: self.chunks_consumed.load(Ordering::Relaxed),
            in_flight_chunks: self.in_flight_chunks.load(Ordering::Relaxed),
            peak_inflight_chunks: self.peak_inflight_chunks.load(Ordering::Relaxed),
        }
    }

    fn add_header_read(&self, d: Duration) {
        self.header_read_ns
            .fetch_add(duration_ns(d), Ordering::Relaxed);
    }

    fn add_parse(&self, d: Duration) {
        self.parse_ns.fetch_add(duration_ns(d), Ordering::Relaxed);
    }

    fn add_producer_backpressure(&self, d: Duration) {
        self.producer_backpressure_ns
            .fetch_add(duration_ns(d), Ordering::Relaxed);
    }

    fn add_consumer_wait(&self, d: Duration) {
        self.consumer_wait_ns
            .fetch_add(duration_ns(d), Ordering::Relaxed);
    }
}

impl PipelineTimingSnapshot {
    /// 与更早快照的差值（按批记日志用）；峰值不可相减，保留全段峰值。
    pub(crate) fn delta_since(&self, before: Self) -> Self {
        Self {
            header_read_ns: self.header_read_ns.saturating_sub(before.header_read_ns),
            parse_ns: self.parse_ns.saturating_sub(before.parse_ns),
            producer_backpressure_ns: self
                .producer_backpressure_ns
                .saturating_sub(before.producer_backpressure_ns),
            consumer_wait_ns: self
                .consumer_wait_ns
                .saturating_sub(before.consumer_wait_ns),
            chunks_produced: self.chunks_produced.saturating_sub(before.chunks_produced),
            chunks_consumed: self.chunks_consumed.saturating_sub(before.chunks_consumed),
            in_flight_chunks: self.in_flight_chunks,
            peak_inflight_chunks: self.peak_inflight_chunks,
        }
    }
}

/// `Duration` → 纳秒（饱和到 u64）。
pub(super) fn duration_ns(d: Duration) -> u64 {
    d.as_nanos().min(u64::MAX as u128) as u64
}

/// 单块的在途记账：块从「开始头读」到「解析完成」恰好持有 1 个守卫。
///
/// 守卫随块经 channel 交给消费侧（每块只在生产开始时创建一次），因此正常收工、取消、通道断开、
/// 任一侧 panic 展开都靠 `Drop` 自动归还，不依赖哪条路径记得手动减一。
struct InflightChunk<'a> {
    timing: &'a PipelineTiming,
}

impl<'a> InflightChunk<'a> {
    fn begin(timing: &'a PipelineTiming) -> Self {
        let in_flight = timing.in_flight_chunks.fetch_add(1, Ordering::Relaxed) + 1;
        timing
            .peak_inflight_chunks
            .fetch_max(in_flight, Ordering::Relaxed);
        Self { timing }
    }
}

impl Drop for InflightChunk<'_> {
    fn drop(&mut self) {
        self.timing.in_flight_chunks.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 有界流水线：`read` 在 `header_pool` 上按 [`HEADER_CHUNK_ITEMS`] 项一块生产，`parse` 在
/// `parse_pool` 上消费同一块；返回按输入下标排列的结果。
///
/// - 两个池都为 `None` 时回退全局 rayon 池（与旧并行路径同源），此时两阶段共享全局预算，
///   重叠度取决于池余量。
/// - `cancel` 触发后按块边界收工并返回 [`AppError::Cancelled`]（整批结果丢弃，调用方据此不写库，
///   避免半批回写）；已在进行的同步文件 I/O 必须返回后生产者线程才 join 收工。
pub(crate) fn run_bounded_header_parse<'a, T, R, H, P>(
    len: usize,
    cancel: &CancellationToken,
    header_pool: Option<&rayon::ThreadPool>,
    parse_pool: Option<&rayon::ThreadPool>,
    timing: &'a PipelineTiming,
    read: H,
    parse: P,
) -> Result<Vec<R>>
where
    T: Send,
    R: Send,
    H: Fn(usize) -> T + Send + Sync,
    P: Fn(usize, T) -> R + Send + Sync,
{
    if len == 0 {
        return Ok(Vec::new());
    }

    let mut out: Vec<R> = Vec::with_capacity(len);
    let (tx, rx) = sync_channel::<(Vec<(usize, T)>, InflightChunk<'a>)>(1);

    std::thread::scope(|scope| {
        let producer = scope.spawn(move || {
            let mut start = 0usize;
            while start < len {
                if cancel.is_cancelled() {
                    return;
                }
                let end = (start + HEADER_CHUNK_ITEMS).min(len);
                // 守卫随本块进入流水线；持有者只有消费侧（经 channel 移交）或本栈（被丢弃时）。
                let inflight = InflightChunk::begin(timing);
                let read_started = Instant::now();
                let reads: Vec<(usize, T)> = match header_pool {
                    Some(pool) => pool.install(|| {
                        (start..end)
                            .into_par_iter()
                            .map(|index| (index, read(index)))
                            .collect()
                    }),
                    None => (start..end)
                        .into_par_iter()
                        .map(|index| (index, read(index)))
                        .collect(),
                };
                timing.add_header_read(read_started.elapsed());
                timing.chunks_produced.fetch_add(1, Ordering::Relaxed);
                let send_started = Instant::now();
                // 投递失败=消费侧已收工（取消/提前返回/panic）：载荷含守卫在此就地丢弃归还。
                let sent = tx.send((reads, inflight)).is_ok();
                timing.add_producer_backpressure(send_started.elapsed());
                if !sent {
                    return;
                }
                start = end;
            }
        });

        // 消费循环跑在 scope 主线程：`rx` 随本栈帧，取消/panic 展开时先 drop 它（Receiver 的
        // Drop 会排空缓冲消息并唤醒阻塞中的生产者），随后 scope 才 join 生产线程。
        while !cancel.is_cancelled() {
            let wait_started = Instant::now();
            // `_inflight` 命名绑定而非 `_`：守卫要活到本次迭代末尾（解析完成/展开时）才归还。
            let (chunk, _inflight) = match rx.recv() {
                Ok(chunk) => chunk,
                Err(_) => break, // 生产者收工（正常结束/取消/panic）
            };
            timing.add_consumer_wait(wait_started.elapsed());
            let parse_started = Instant::now();
            let parsed: Vec<(usize, R)> = match parse_pool {
                Some(pool) => pool.install(|| {
                    chunk
                        .into_par_iter()
                        .map(|(index, item)| (index, parse(index, item)))
                        .collect()
                }),
                None => chunk
                    .into_par_iter()
                    .map(|(index, item)| (index, parse(index, item)))
                    .collect(),
            };
            timing.add_parse(parse_started.elapsed());
            timing.chunks_consumed.fetch_add(1, Ordering::Relaxed);
            // 块按输入下标升序产出（生产者顺序发块 + rayon 索引 collect 保序），逐块追加即为输入序。
            out.extend(parsed.into_iter().map(|(_, item)| item));
        }
        drop(rx);
        if let Err(payload) = producer.join() {
            // 生产者 panic：此刻消费侧已拆栈（rx 已 drop）、生产线程已 join，再把 panic 交回调用方。
            std::panic::resume_unwind(payload);
        }
    });

    // 生产/消费两侧都已 join：短结果只可能来自取消路径（生产侧在块边界收工）。
    if cancel.is_cancelled() || out.len() != len {
        return Err(AppError::Cancelled);
    }
    Ok(out)
}
