//! 全库基准的旁路度量；不参与产物、路由或资源调度决策。

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
#[cfg(any(not(windows), test))]
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::native_protocol::NativeGpuMetrics;

/// 当前 GPU 验证版本直接采集分阶段指标，避免启动环境使回退原因不可见。
pub(crate) const BENCHMARK_ENABLED: bool = true;

/// 生产阶段的实际结果；成功发布前不计入新生成吞吐。
#[derive(Clone, Copy, Debug)]
pub(crate) enum MeasuredOutcome {
    Generated,
    CacheHit,
    Direct,
    Unsupported,
    Failed,
}

/// 单项阶段耗时；原生 decode 包含阶段内背压和 GPU 等待，GPU 子项用于进一步归因。
#[derive(Clone, Debug, Default)]
pub(crate) struct ItemMetrics {
    pub decode_us: u64,
    pub encode_us: u64,
    pub write_us: u64,
    pub embedded_us: u64,
    pub backend: Option<String>,
    pub outcome: Option<MeasuredOutcome>,
    pub ai_written: u64,
    pub ai_cache_hit: u64,
    pub ai_failed: u64,
    pub cpu_fallbacks: u64,
    pub gpu: Option<NativeGpuMetrics>,
}

/// 每段操作只计入一个阶段，AI 与显示产物共用同一口径。
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Encode,
    Write,
}

/// 在正常返回和 panic 展开时都归还已耗费的时间；关闭度量时不读取时钟。
pub(crate) fn stage_started<'a>(
    metrics: &'a mut Option<&mut ItemMetrics>,
    stage: Stage,
) -> Option<StageTimer<'a>> {
    metrics.as_deref_mut().map(|metrics| StageTimer {
        counter: match stage {
            Stage::Encode => &mut metrics.encode_us,
            Stage::Write => &mut metrics.write_us,
        },
        started: Instant::now(),
    })
}

/// 借用当前阶段计数器，作用域结束即完成记账。
pub(crate) struct StageTimer<'a> {
    counter: &'a mut u64,
    started: Instant,
}

impl Drop for StageTimer<'_> {
    fn drop(&mut self) {
        *self.counter += self.started.elapsed().as_micros() as u64;
    }
}

/// 沿原有队列携带时间戳，关闭基准时不读取时钟。
#[derive(Clone, Debug, Default)]
pub(crate) struct WorkMetrics {
    pub item: ItemMetrics,
    pub submitted: Option<Instant>,
    queued: Option<Instant>,
    pub queue_us: u64,
}

impl WorkMetrics {
    pub fn new(enabled: bool) -> Self {
        let now = enabled.then(Instant::now);
        Self {
            submitted: now,
            queued: now,
            ..Self::default()
        }
    }

    pub fn dequeue(&mut self) {
        if let Some(queued) = self.queued.take() {
            self.queue_us += queued.elapsed().as_micros() as u64;
        }
    }

    pub fn enqueue(&mut self) {
        self.queued = self.submitted.map(|_| Instant::now());
    }
}

/// 提交事务的逐项结果；事务回滚时整批必须报告未发布。
#[derive(Clone, Copy, Debug)]
pub(crate) enum Publication {
    Accepted,
    Reused,
    Stale,
}

/// 解码子项仅旁路累计，不再加入 decode_us；峰值跨请求取最大值。
#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct GpuRunMetrics {
    pub observed_samples: u64,
    /// GPU 阶段成功数，最终编码或发布仍可能失败。
    pub success_samples: u64,
    pub hardware_timestamp_samples: u64,
    pub cpu_planar_us: u64,
    pub queue_us: u64,
    pub upload_us: u64,
    #[serde(rename = "hardware_timestamp_gpu_us")]
    pub gpu_us: Option<u64>,
    pub readback_us: u64,
    #[serde(rename = "completion_wait_including_sync_us")]
    pub completion_wait_us: u64,
    pub init_us: u64,
    pub adapter_counts: BTreeMap<String, u64>,
    pub fallback_counts: BTreeMap<String, u64>,
    pub reserved_peak_bytes: u64,
    pub usage_peak_bytes: u64,
    pub budget_bytes: u64,
}

impl GpuRunMetrics {
    fn record(&mut self, gpu: &NativeGpuMetrics) {
        self.observed_samples += 1;
        self.success_samples += u64::from(gpu.fallback.is_none());
        self.cpu_planar_us += gpu.cpu_planar_us;
        self.queue_us += gpu.queue_us;
        self.upload_us += gpu.upload_us;
        if let Some(gpu_us) = gpu.gpu_us {
            self.hardware_timestamp_samples += 1;
            *self.gpu_us.get_or_insert(0) += gpu_us;
        }
        self.readback_us += gpu.readback_us;
        self.completion_wait_us += gpu.completion_wait_us;
        self.init_us += gpu.init_us;
        if let Some(adapter) = &gpu.adapter {
            *self.adapter_counts.entry(adapter.clone()).or_default() += 1;
        }
        if let Some(fallback) = &gpu.fallback {
            *self.fallback_counts.entry(fallback.clone()).or_default() += 1;
        }
        self.reserved_peak_bytes = self.reserved_peak_bytes.max(gpu.reserved_peak_bytes);
        self.usage_peak_bytes = self.usage_peak_bytes.max(gpu.usage_peak_bytes);
        self.budget_bytes = self.budget_bytes.max(gpu.budget_bytes);
    }
}

/// 仅由原有收集线程更新，避免逐图共享计数锁。
#[derive(Debug, Default)]
pub(crate) struct RunMetrics {
    pub generated: u64,
    pub cache_hit: u64,
    pub direct: u64,
    pub unsupported: u64,
    pub failed: u64,
    pub stale: u64,
    pub unpublished: u64,
    pub unclassified: u64,
    pub observed: u64,
    pub stages: ItemMetrics,
    pub queue_us: u64,
    pub end_to_end_us: u64,
    pub db_us: u64,
    pub backends: BTreeMap<String, u64>,
    pub gpu_experiment: Option<GpuRunMetrics>,
    pub first_publish_us: Option<u64>,
    pub last_publish_us: Option<u64>,
    first_batch_generated: u64,
}

impl RunMetrics {
    pub fn record_batch(
        &mut self,
        items: &[WorkMetrics],
        published: Option<&[Publication]>,
        elapsed_us: u64,
    ) {
        let before = self.generated;
        for (index, work) in items.iter().enumerate() {
            let item = &work.item;
            self.observed += 1;
            self.stages.decode_us += item.decode_us;
            self.stages.encode_us += item.encode_us;
            self.stages.write_us += item.write_us;
            self.stages.embedded_us += item.embedded_us;
            self.stages.ai_written += item.ai_written;
            self.stages.ai_cache_hit += item.ai_cache_hit;
            self.stages.ai_failed += item.ai_failed;
            self.stages.cpu_fallbacks += item.cpu_fallbacks;
            if let Some(gpu) = &item.gpu {
                self.gpu_experiment
                    .get_or_insert_with(GpuRunMetrics::default)
                    .record(gpu);
            }
            self.queue_us +=
                work.queue_us + work.queued.map_or(0, |t| t.elapsed().as_micros() as u64);
            if let Some(backend) = &item.backend {
                *self.backends.entry(backend.clone()).or_default() += 1;
            }
            match published.and_then(|outcomes| outcomes.get(index)) {
                Some(Publication::Accepted) => {
                    self.end_to_end_us +=
                        work.submitted.map_or(0, |t| t.elapsed().as_micros() as u64);
                    match item.outcome {
                        Some(MeasuredOutcome::Generated) => self.generated += 1,
                        Some(MeasuredOutcome::CacheHit) => self.cache_hit += 1,
                        Some(MeasuredOutcome::Direct) => self.direct += 1,
                        Some(MeasuredOutcome::Unsupported) => self.unsupported += 1,
                        Some(MeasuredOutcome::Failed) => self.failed += 1,
                        None => self.unclassified += 1,
                    }
                }
                Some(Publication::Stale) => self.stale += 1,
                Some(Publication::Reused) => self.cache_hit += 1,
                None => self.unpublished += 1,
            }
        }
        if self.generated > before {
            if self.first_publish_us.is_none() {
                self.first_publish_us = Some(elapsed_us);
                self.first_batch_generated = self.generated - before;
            }
            self.last_publish_us = Some(elapsed_us);
        }
    }

    /// 从首批新图提交后到末批提交的吞吐，只有一批时无稳态证据。
    pub fn publication_span_rate(&self) -> Option<f64> {
        let duration = self.last_publish_us?.checked_sub(self.first_publish_us?)?;
        (duration > 0).then(|| {
            (self.generated - self.first_batch_generated) as f64 * 1_000_000.0 / duration as f64
        })
    }
}

/// 解码和编码各自的活动峰值；只观测，不占用或限制计算名额。
#[derive(Default)]
pub(crate) struct Activity {
    #[cfg(any(not(windows), test))]
    active: AtomicU64,
    pub peak: AtomicU64,
}

impl Activity {
    #[cfg(any(not(windows), test))]
    pub fn enter(&self, enabled: bool) -> ActivityGuard<'_> {
        if enabled {
            let active = self.active.fetch_add(1, Ordering::Relaxed) + 1;
            self.peak.fetch_max(active, Ordering::Relaxed);
        }
        ActivityGuard {
            activity: self,
            enabled,
        }
    }
}

#[cfg(any(not(windows), test))]
pub(crate) struct ActivityGuard<'a> {
    activity: &'a Activity,
    enabled: bool,
}

#[cfg(any(not(windows), test))]
impl Drop for ActivityGuard<'_> {
    fn drop(&mut self) {
        if self.enabled {
            self.activity.active.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// 跨线程只保留运行级聚合量；队列长度为采样峰值下界。
#[derive(Default)]
pub(crate) struct PipelineMetrics {
    pub discovery_us: AtomicU64,
    pub candidate_read_us: AtomicU64,
    pub candidates: AtomicU64,
    pub dispatch_errors: AtomicU64,
    pub decode_queue_peak: AtomicU64,
    pub encode_queue_peak: AtomicU64,
    pub working_bytes_peak: AtomicU64,
    pub encoded_bytes_peak: AtomicU64,
    pub worker_private_bytes_peak: AtomicU64,
    pub host_output_bytes_peak: AtomicU64,
    pub decode: Activity,
    pub encode: Activity,
}
