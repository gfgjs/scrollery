//! 图片执行服务：同源同配置合并计算，订阅独立取消，解码与编码分别常驻。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use tokio_util::sync::CancellationToken;

use crate::db::models::{MediaItem, ThumbResult};

use crate::state::AppState;

use super::benchmark::{PipelineMetrics, WorkMetrics};
use super::generator::{self, ThumbConfig};
use super::qos::thumb_cpu_budget;
use super::source_budget::{SourceBudget, SourceDevice, SourcePermit};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Priority {
    Viewport,
    Background,
}

/// 请求持有源和配置快照，不从后续可变设置中重新拼装生产身份。
pub(crate) struct ImageRequest {
    pub item: MediaItem,
    pub abs_path: PathBuf,
    pub root_path: String,
    pub relative_dir: String,
    pub cached_result: Option<ThumbResult>,
    pub cached_ai_path: Option<String>,
    pub config: ThumbConfig,
    pub epoch: u64,
    pub priority: Priority,
    pub cancellation: CancellationToken,
    pub measured: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct TaskKey {
    epoch: u64,
    item_id: i64,
    source_revision: i64,
    cache_key: i64,
    config: ThumbConfig,
}

impl ImageRequest {
    fn key(&self) -> TaskKey {
        TaskKey {
            epoch: self.epoch,
            item_id: self.item.id,
            source_revision: self.item.source_revision,
            cache_key: self.item.cache_key,
            config: self.config.clone(),
        }
    }
}

struct PreparedRequest {
    request: ImageRequest,
    key: TaskKey,
    device: SourceDevice,
    heavy: bool,
}

/// None 表示请求已取消或库已换代，不能作为内容失败写入数据库。
#[derive(Clone)]
pub(crate) struct ImageCompletion {
    pub result: Option<ThumbResult>,
    pub ai_path: Option<String>,
    pub metrics: WorkMetrics,
}

struct Subscriber {
    sender: Sender<ImageCompletion>,
    cancellation: CancellationToken,
}

struct Task {
    id: u64,
    request: Option<ImageRequest>,
    subscribers: HashMap<u64, Subscriber>,
    completed: Option<ImageCompletion>,
    metrics: WorkMetrics,
    device: SourceDevice,
}

#[derive(Default)]
struct Registry {
    tasks: HashMap<TaskKey, Task>,
    viewport: VecDeque<TaskKey>,
    background: VecDeque<TaskKey>,
    next_id: u64,
    viewport_streak: usize,
    heavy_waiter: Option<TaskKey>,
    shutdown: bool,
}

impl Registry {
    fn queue(&mut self, priority: Priority) -> &mut VecDeque<TaskKey> {
        match priority {
            Priority::Viewport => &mut self.viewport,
            Priority::Background => &mut self.background,
        }
    }

    fn next_key(&mut self) -> Option<TaskKey> {
        // 视口连续最多四项；后台为空时不浪费可用吞吐。
        if !self.background.is_empty() && (self.viewport_streak >= 4 || self.viewport.is_empty()) {
            self.viewport_streak = 0;
            self.background.pop_front()
        } else {
            let key = self.viewport.pop_front();
            if key.is_some() {
                self.viewport_streak = (self.viewport_streak + 1).min(4);
            }
            key
        }
    }

    fn remove(&mut self, key: &TaskKey) {
        self.tasks.remove(key);
        self.viewport.retain(|queued| queued != key);
        self.background.retain(|queued| queued != key);
    }

    fn all_cancelled(&self, key: &TaskKey) -> bool {
        self.tasks.get(key).is_some_and(|task| {
            task.subscribers
                .values()
                .all(|subscriber| subscriber.cancellation.is_cancelled())
        })
    }

    fn background_heavy(&self) -> usize {
        self.background
            .iter()
            .filter(|key| {
                self.tasks
                    .get(*key)
                    .and_then(|task| task.request.as_ref())
                    .is_some_and(|request| {
                        super::execution::needs_pixels(request)
                            && super::execution::heavy_units(request) > 0
                    })
            })
            .count()
    }
}

// HEIC 原生显示解码占一份；其它重任务占满四份，避免独立限流后内存峰值叠加。
pub(super) const HEAVY_CAPACITY: usize = 4;

pub(super) struct Shared {
    registry: Mutex<Registry>,
    // 发布只阻挡执行身份退役；取任务、补任务和完成事件不经过此闸门。
    publications: RwLock<()>,
    changed: Condvar,
    lane_capacity: usize,
    sources: Arc<SourceBudget>,
    heavy: Arc<AtomicUsize>,
    #[cfg(windows)]
    pub(super) wake: super::wake::Wake,
}

impl Shared {
    pub(super) fn stop(&self) {
        let _publication = self.publications.write().unwrap_or_else(|e| e.into_inner());
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        registry.shutdown = true;
        registry.tasks.clear();
        self.changed.notify_all();
        #[cfg(windows)]
        self.wake.notify();
    }

    fn retire_cancelled<'a, 'key>(
        &'a self,
        mut registry: MutexGuard<'a, Registry>,
        keys: impl Iterator<Item = &'key TaskKey> + Clone,
    ) -> MutexGuard<'a, Registry> {
        if keys.clone().any(|key| registry.all_cancelled(key)) {
            // 等发布完成前先归还注册表，避免身份退役反过来堵住正常派发。
            drop(registry);
            let _publication = self.publications.write().unwrap_or_else(|e| e.into_inner());
            registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            for key in keys {
                if registry.all_cancelled(key) {
                    registry.remove(key);
                }
            }
        }
        registry
    }

    pub(super) fn interested(&self, key: &TaskKey, id: u64) -> bool {
        let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        !registry.shutdown
            && registry.tasks.get(key).is_some_and(|task| {
                task.id == id
                    && task
                        .subscribers
                        .values()
                        .any(|subscriber| !subscriber.cancellation.is_cancelled())
            })
    }

    fn complete(&self, key: &TaskKey, id: u64, completion: ImageCompletion) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = registry.tasks.get_mut(key).filter(|task| task.id == id) {
            for subscriber in task.subscribers.values() {
                let mut result = completion.clone();
                if subscriber.cancellation.is_cancelled() {
                    result.result = None;
                }
                // 每个订阅只有一个终态，消费端中断不能堵住整个编码池。
                let _ = subscriber.sender.try_send(result);
            }
            task.completed = Some(completion);
        }
        self.changed.notify_all();
        #[cfg(windows)]
        self.wake.notify();
    }

    pub(super) fn try_take(&self) -> Option<Job> {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry.shutdown {
            return None;
        }
        if registry.heavy_waiter.as_ref().is_some_and(|key| {
            !registry.tasks.get(key).is_some_and(|task| {
                task.request.is_some()
                    && task
                        .subscribers
                        .values()
                        .any(|s| !s.cancellation.is_cancelled())
            })
        }) {
            registry.heavy_waiter = None;
        }
        // 每次只扫描当前有界队列一轮，受限源盘/大图不堵住可执行任务。
        let count = registry.viewport.len() + registry.background.len();
        for _ in 0..count {
            let key = registry.next_key()?;
            let other_heavy_waiting = registry
                .heavy_waiter
                .as_ref()
                .is_some_and(|waiter| waiter != &key);
            let Some(task) = registry.tasks.get_mut(&key) else {
                continue;
            };
            let Some(request) = task.request.as_ref() else {
                continue;
            };
            let interested = task
                .subscribers
                .values()
                .any(|s| !s.cancellation.is_cancelled());
            let pixels = interested && super::execution::needs_pixels(request);
            let mut heavy = None;
            let mut source = None;
            if pixels {
                let units = super::execution::heavy_units(request);
                if units > 0 {
                    // 独占任务等齐全部额度时暂停 HEIC 补位；普通小图仍可绕过继续。
                    if other_heavy_waiting {
                        let priority = request.priority;
                        registry.queue(priority).push_back(key);
                        continue;
                    }
                    if self
                        .heavy
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                            (used + units <= HEAVY_CAPACITY).then_some(used + units)
                        })
                        .is_err()
                    {
                        let priority = request.priority;
                        if units == HEAVY_CAPACITY {
                            registry.heavy_waiter = Some(key.clone());
                        }
                        registry.queue(priority).push_back(key);
                        continue;
                    }
                    heavy = Some(HeavyPermit {
                        used: Arc::clone(&self.heavy),
                        units,
                    });
                }
                source = self.sources.try_acquire(task.device);
                if source.is_none() {
                    drop(heavy);
                    let priority = request.priority;
                    if units == HEAVY_CAPACITY {
                        registry.heavy_waiter = None;
                    }
                    registry.queue(priority).push_back(key);
                    continue;
                }
            }
            let request = task.request.take().expect("queued request");
            let mut metrics = std::mem::take(&mut task.metrics);
            metrics.dequeue();
            self.changed.notify_all();
            let job = Job {
                key,
                id: task.id,
                request,
                metrics,
                source,
                heavy,
            };
            if registry.heavy_waiter.as_ref() == Some(&job.key) {
                registry.heavy_waiter = None;
            }
            return Some(job);
        }
        None
    }

    pub(super) fn stopped(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown
    }

    pub(super) fn requeue(&self, mut job: Job) {
        job.decoded();
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = registry
            .tasks
            .get_mut(&job.key)
            .filter(|task| task.id == job.id)
        {
            let priority = job.request.priority;
            task.request = Some(job.request);
            task.metrics = job.metrics;
            registry.queue(priority).push_back(job.key);
        }
        self.changed.notify_all();
    }

    #[cfg(not(windows))]
    pub(super) fn wait(&self) {
        let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = self
            .changed
            .wait_timeout(registry, Duration::from_millis(10))
            .unwrap_or_else(|e| e.into_inner());
    }

    #[cfg(test)]
    fn take(&self) -> Option<Job> {
        self.try_take()
    }
}

pub(super) struct HeavyPermit {
    used: Arc<AtomicUsize>,
    units: usize,
}
impl Drop for HeavyPermit {
    fn drop(&mut self) {
        self.used.fetch_sub(self.units, Ordering::AcqRel);
    }
}

pub(super) struct Job {
    pub key: TaskKey,
    pub id: u64,
    pub request: ImageRequest,
    pub metrics: WorkMetrics,
    pub source: Option<SourcePermit>,
    pub heavy: Option<HeavyPermit>,
}

impl Job {
    pub fn decoded(&mut self) {
        self.source.take();
        self.heavy.take();
    }
}
/// 持有订阅直至数据库发布结束，保留完成到提交窗口内的同图去重。
pub(crate) struct ImageSubscription {
    shared: Arc<Shared>,
    key: TaskKey,
    subscriber_id: u64,
    receiver: Receiver<ImageCompletion>,
}

impl ImageSubscription {
    pub fn item_id(&self) -> i64 {
        self.key.item_id
    }

    /// 捕获当前完成结果，发布闸门保留执行身份；SQL 和通知不占调度注册表锁。
    pub(crate) fn with_completed_batch<T>(
        subscriptions: &[Self],
        action: impl FnOnce(&[Option<(&ThumbConfig, &ImageCompletion)>]) -> T,
    ) -> T {
        let Some(first) = subscriptions.first() else {
            return action(&[]);
        };
        let _publication = first
            .shared
            .publications
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let registry = first
            .shared
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let completed: Vec<_> = subscriptions
            .iter()
            .map(|subscription| {
                if !Arc::ptr_eq(&first.shared, &subscription.shared) {
                    return None;
                }
                let task = registry.tasks.get(&subscription.key)?;
                let subscriber = task.subscribers.get(&subscription.subscriber_id)?;
                if subscriber.cancellation.is_cancelled() {
                    return None;
                }
                Some((&subscription.key.config, task.completed.clone()?))
            })
            .collect();
        drop(registry);
        let completed_refs: Vec<_> = completed
            .iter()
            .map(|work| {
                work.as_ref()
                    .map(|(config, completion)| (*config, completion))
            })
            .collect();
        action(&completed_refs)
    }

    /// 批量收集器按完成顺序选择，避免被慢任务挡住后续结果。
    pub fn receiver(&self) -> &Receiver<ImageCompletion> {
        &self.receiver
    }

    #[cfg(test)]
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<ImageCompletion, crossbeam_channel::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for ImageSubscription {
    fn drop(&mut self) {
        let _publication = self
            .shared
            .publications
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let mut registry = self
            .shared
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(task) = registry.tasks.get_mut(&self.key) {
            task.subscribers.remove(&self.subscriber_id);
            if task.subscribers.is_empty() {
                registry.remove(&self.key);
            }
        }
        self.shared.changed.notify_all();
        #[cfg(windows)]
        self.shared.wake.notify();
    }
}

pub(crate) struct ImageService {
    shared: Arc<Shared>,
    /// 服务生命周期聚合；不是单次全库运行的峰值。
    pub metrics: Arc<PipelineMetrics>,
    pub width: usize,
    measured: bool,
}

impl ImageService {
    pub fn for_state(state: &Arc<AppState>) -> Arc<Self> {
        Arc::clone(state.image_service.get_or_init(|| Self::start(state)))
    }

    fn start(state: &Arc<AppState>) -> Arc<Self> {
        let width = thumb_cpu_budget();
        let service = Arc::new(Self {
            shared: Arc::new(Shared {
                registry: Mutex::new(Registry::default()),
                publications: RwLock::new(()),
                changed: Condvar::new(),
                lane_capacity: 2 * width,
                sources: SourceBudget::new(),
                heavy: Arc::new(AtomicUsize::new(0)),
                #[cfg(windows)]
                wake: super::wake::Wake::default(),
            }),
            metrics: Arc::new(PipelineMetrics::default()),
            width,
            measured: super::benchmark::BENCHMARK_ENABLED,
        });
        super::execution::start(
            state,
            Arc::clone(&service.shared),
            Arc::clone(&service.metrics),
            width,
        );
        service
    }

    fn prepare(&self, mut request: ImageRequest) -> PreparedRequest {
        request.config.size = generator::snap_to_tier(request.config.size);
        request.measured |= self.measured;
        let key = request.key();
        let device = self
            .shared
            .sources
            .classify(request.epoch, &request.root_path);
        let heavy =
            super::execution::needs_pixels(&request) && super::execution::heavy_units(&request) > 0;
        PreparedRequest {
            request,
            key,
            device,
            heavy,
        }
    }

    /// 只在阻塞编排线程调用。两个优先级分别限长，后台排队不占满视口入口。
    pub fn submit(&self, request: ImageRequest) -> ImageSubscription {
        let prepared = self.prepare(request);
        let mut registry = self
            .shared
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry = self
            .shared
            .retire_cancelled(registry, std::iter::once(&prepared.key));
        while !registry.tasks.contains_key(&prepared.key)
            && registry.queue(prepared.request.priority).len() >= self.shared.lane_capacity
            && !registry.shutdown
            && !prepared.request.cancellation.is_cancelled()
        {
            // 外部取消令牌没有 Condvar 通知；有界等待使阻塞准入也能响应取消。
            registry = self
                .shared
                .changed
                .wait_timeout(registry, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        self.register(&mut registry, prepared)
    }

    /// 提交全库读取者已准备的至多 256 项后台候选，逐项交出订阅。
    ///
    /// 本页使用同一取消令牌；取消或回调返回 false 时停止准入并返回 false。
    /// 回调不持有注册表锁，普通图只可绕过因重任务排队限额暂缓的 SSD 候选。
    pub fn submit_background_page(
        &self,
        requests: Vec<ImageRequest>,
        accept: impl FnMut(ImageSubscription) -> bool,
    ) -> bool {
        let prepared = requests
            .into_iter()
            .map(|request| self.prepare(request))
            .collect();
        self.submit_prepared_page(prepared, accept)
    }

    fn submit_prepared_page(
        &self,
        mut pending: Vec<PreparedRequest>,
        mut accept: impl FnMut(ImageSubscription) -> bool,
    ) -> bool {
        let heavy_capacity = (self.shared.lane_capacity / 2).max(1);
        while !pending.is_empty() {
            let subscription = {
                let mut registry = self
                    .shared
                    .registry
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                loop {
                    if registry.shutdown
                        || pending
                            .iter()
                            .any(|prepared| prepared.request.cancellation.is_cancelled())
                    {
                        return false;
                    }
                    registry = self
                        .shared
                        .retire_cancelled(registry, pending.iter().map(|prepared| &prepared.key));
                    // 清理取消身份后只统计一次，整轮扫描共用同一注册表快照。
                    let background_heavy = registry.background_heavy();
                    let mut selected = None;
                    for (index, prepared) in pending.iter().enumerate() {
                        debug_assert!(prepared.request.priority == Priority::Background);
                        if registry.tasks.contains_key(&prepared.key) {
                            selected = Some(index);
                            break;
                        }
                        if registry.background.len() >= self.shared.lane_capacity {
                            break;
                        }
                        if prepared.heavy && background_heavy >= heavy_capacity {
                            // 只有确认 SSD 的重任务可被绕过；机械盘及未知盘保持源顺序。
                            if matches!(prepared.device, SourceDevice::Ssd(_)) {
                                continue;
                            }
                            break;
                        }
                        selected = Some(index);
                        break;
                    }
                    if let Some(index) = selected {
                        break self.register(&mut registry, pending.remove(index));
                    }
                    // 没有普通候选时，已排队的重任务仍会解码并释放排队名额。
                    // 外部取消没有 Condvar 通知，沿用单项准入的有界可取消等待。
                    registry = self
                        .shared
                        .changed
                        .wait_timeout(registry, Duration::from_millis(100))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            };
            if !accept(subscription) {
                return false;
            }
        }
        true
    }

    fn register(&self, registry: &mut Registry, prepared: PreparedRequest) -> ImageSubscription {
        let PreparedRequest {
            request,
            key,
            device,
            ..
        } = prepared;
        let measured = request.measured;
        let (sender, receiver) = bounded(1);
        registry.next_id += 1;
        let subscriber_id = registry.next_id;
        if request.cancellation.is_cancelled() || registry.shutdown {
            let _ = sender.try_send(ImageCompletion {
                ai_path: None,
                result: None,
                metrics: WorkMetrics::default(),
            });
        } else {
            let priority = request.priority;
            let cancellation = request.cancellation.clone();
            if !registry.tasks.contains_key(&key) {
                let metrics = WorkMetrics::new(request.measured);
                registry.tasks.insert(
                    key.clone(),
                    Task {
                        id: subscriber_id,
                        request: Some(request),
                        subscribers: HashMap::new(),
                        completed: None,
                        metrics,
                        device,
                    },
                );
                registry.queue(priority).push_back(key.clone());
            } else if priority == Priority::Viewport
                && registry.viewport.len() < self.shared.lane_capacity
                && registry.background.iter().any(|queued| queued == &key)
            {
                registry.background.retain(|queued| queued != &key);
                registry.viewport.push_back(key.clone());
            }
            let task = registry.tasks.get_mut(&key).expect("registered image task");
            if let Some(completed) = &task.completed {
                let _ = sender.try_send(completed.clone());
            }
            task.subscribers.insert(
                subscriber_id,
                Subscriber {
                    sender,
                    cancellation,
                },
            );
        }
        if measured {
            self.metrics.decode_queue_peak.fetch_max(
                (registry.viewport.len() + registry.background.len()) as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        self.shared.changed.notify_all();
        #[cfg(windows)]
        self.shared.wake.notify();
        ImageSubscription {
            shared: Arc::clone(&self.shared),
            key,
            subscriber_id,
            receiver,
        }
    }
}

impl Drop for ImageService {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

pub(super) fn failed_result(item: &MediaItem) -> ThumbResult {
    ThumbResult {
        item_id: item.id,
        thumb_status: 2,
        thumb_path: None,
        thumbhash: None,
        source_revision: item.source_revision,
        cache_key: item.cache_key,
    }
}

pub(super) fn finish_with_ai(
    shared: &Shared,
    job: Job,
    result: Option<ThumbResult>,
    ai_path: Option<String>,
) {
    let mut metrics = job.metrics;
    metrics.enqueue();
    shared.complete(
        &job.key,
        job.id,
        ImageCompletion {
            result,
            ai_path,
            metrics,
        },
    );
}

#[cfg(test)]
fn finish(shared: &Shared, job: Job, result: Option<ThumbResult>) {
    finish_with_ai(shared, job, result, None);
}
#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> ImageService {
        ImageService {
            shared: Arc::new(Shared {
                registry: Mutex::new(Registry::default()),
                publications: RwLock::new(()),
                changed: Condvar::new(),
                lane_capacity: 8,
                sources: SourceBudget::new(),
                heavy: Arc::new(AtomicUsize::new(0)),
                #[cfg(windows)]
                wake: super::super::wake::Wake::default(),
            }),
            metrics: Arc::new(PipelineMetrics::default()),
            width: 1,
            measured: false,
        }
    }

    fn request(id: i64, priority: Priority) -> ImageRequest {
        ImageRequest {
            item: MediaItem {
                id,
                directory_id: 1,
                file_name: format!("{id}.png"),
                file_size: 10,
                file_mtime: 0,
                file_format: "png".into(),
                media_type: "image".into(),
                width: 1,
                height: 1,
                duration_ms: None,
                sort_datetime: 0,
                cache_key: id,
                thumb_status: 0,
                thumb_path: None,
                thumbhash: None,
                is_favorited: false,
                is_deleted: false,
                deleted_at: None,
                rating: 0,
                color_label: 0,
                view_rotation: 0,
                playback_position_ms: 0,
                is_live_photo: false,
                has_embedded_video: false,
                companion_of: None,
                content_hash: None,
                created_at: 0,
                updated_at: 0,
                source_revision: 1,
            },
            abs_path: PathBuf::from(format!("{id}.png")),
            root_path: String::new(),
            relative_dir: String::new(),
            cached_result: None,
            cached_ai_path: None,
            config: ThumbConfig {
                cache_dir: PathBuf::from("cache"),
                size: 64,
                skip_max_bytes: 0,
                strategy: "cpu".into(),
                gpu_engine: "wic".into(),
                ai_hq_cache: false,
                webp_quality: 80,
                ai_cache_short_edge: 336,
            },
            epoch: 1,
            priority,
            cancellation: CancellationToken::new(),
            measured: false,
        }
    }

    #[cfg(windows)]
    fn native_heic_request(id: i64) -> ImageRequest {
        let mut image = request(id, Priority::Background);
        image.config.strategy = "gpu".into();
        image.item.file_format = "heic".into();
        image.item.width = 400;
        image.item.height = 400;
        image
    }

    #[cfg(windows)]
    fn submit_on(
        service: &ImageService,
        image: ImageRequest,
        device: SourceDevice,
    ) -> ImageSubscription {
        let subscription = service.submit(image);
        service
            .shared
            .registry
            .lock()
            .unwrap()
            .tasks
            .get_mut(&subscription.key)
            .unwrap()
            .device = device;
        subscription
    }

    #[cfg(windows)]
    #[test]
    fn native_heic_decode_is_limited_to_four_and_excludes_other_heavy_work() {
        let service = service();
        let first = submit_on(&service, native_heic_request(1), SourceDevice::Ssd(0));
        let mut cached_ai = native_heic_request(2);
        cached_ai.item.file_format = "HEIF".into();
        cached_ai.config.ai_hq_cache = true;
        cached_ai.cached_ai_path = Some("ai/ready.webp".into());
        let second = submit_on(&service, cached_ai, SourceDevice::Ssd(0));
        let third = submit_on(&service, native_heic_request(3), SourceDevice::Ssd(0));
        let fourth = submit_on(&service, native_heic_request(4), SourceDevice::Ssd(0));
        let fifth = submit_on(&service, native_heic_request(5), SourceDevice::Ssd(0));
        let mut first_job = service.shared.take().unwrap();
        let mut second_job = service.shared.take().unwrap();
        let mut third_job = service
            .shared
            .take()
            .expect("third native HEIC decode can overlap");
        let fourth_job = service
            .shared
            .take()
            .expect("fourth native HEIC decode can overlap");
        assert!(service.shared.take().is_none());
        // Decoded 归还许可后可继续编码；随后 Finished 再归还不能多释放名额。
        first_job.decoded();
        first_job.decoded();
        let fifth_job = service.shared.take().unwrap();
        assert_eq!(fifth_job.request.item.id, 5);
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 4);
        let mut large_png = request(6, Priority::Background);
        large_png.item.width = 10_000;
        large_png.item.height = 10_000;
        let exclusive = submit_on(&service, large_png, SourceDevice::Ssd(0));
        assert!(service.shared.take().is_none());
        second_job.decoded();
        assert!(service.shared.take().is_none());
        third_job.decoded();
        assert!(service.shared.take().is_none());
        drop(fourth_job);
        assert!(service.shared.take().is_none());
        drop(fifth_job);
        let exclusive_job = service.shared.take().unwrap();
        assert_eq!(exclusive_job.request.item.id, 6);
        let seventh = submit_on(&service, native_heic_request(7), SourceDevice::Ssd(0));
        assert!(service.shared.take().is_none());
        drop(exclusive_job);
        assert_eq!(service.shared.take().unwrap().request.item.id, 7);
        drop((first, second, third, fourth, fifth, exclusive, seventh));
    }

    #[cfg(windows)]
    #[test]
    fn heic_permits_survive_source_contention_requeue_and_cancellation() {
        let service = service();
        let first = submit_on(&service, native_heic_request(1), SourceDevice::Hdd(0));
        let second = submit_on(&service, native_heic_request(2), SourceDevice::Hdd(0));
        let third = submit_on(&service, native_heic_request(3), SourceDevice::Ssd(1));
        let first_job = service.shared.take().unwrap();
        // 同盘请求借到 heavy 许可后会被源盘闸门拒绝，须归还才能让另一盘继续。
        let third_job = service
            .shared
            .take()
            .expect("blocked source must return its heavy permit");
        assert_eq!(third_job.request.item.id, 3);
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 2);
        service.shared.requeue(first_job);
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 1);
        let mut second_job = service.shared.take().unwrap();
        assert_eq!(second_job.request.item.id, 2);
        // 退订不能在仍有解码进行时提早释放资源；任务 Drop 才归还其许可。
        drop(third);
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 2);
        assert!(service.shared.take().is_none());
        drop(third_job);
        assert!(service.shared.take().is_none());
        second_job.decoded();
        let first_job = service.shared.take().unwrap();
        assert_eq!(first_job.request.item.id, 1);
        service.shared.requeue(first_job);
        drop(first);
        assert!(service.shared.take().is_none());
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 0);
        let fourth = submit_on(&service, native_heic_request(4), SourceDevice::Ssd(1));
        let fifth = submit_on(&service, native_heic_request(5), SourceDevice::Ssd(1));
        let fourth_job = service.shared.take().unwrap();
        let fifth_job = service.shared.take().unwrap();
        assert!(service.shared.take().is_none());
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 2);
        drop((fourth_job, fifth_job, second, fourth, fifth));
        assert_eq!(service.shared.heavy.load(Ordering::Acquire), 0);
    }

    #[cfg(windows)]
    #[test]
    fn exclusive_waiter_blocks_heic_refills_until_admission_or_cancellation() {
        let service = service();
        let first = submit_on(&service, native_heic_request(1), SourceDevice::Ssd(0));
        let second = submit_on(&service, native_heic_request(2), SourceDevice::Ssd(0));
        let mut first_job = service.shared.take().unwrap();
        let mut second_job = service.shared.take().unwrap();
        let mut exclusive_request = request(3, Priority::Background);
        exclusive_request.item.width = 10_000;
        exclusive_request.item.height = 10_000;
        let exclusive = submit_on(&service, exclusive_request, SourceDevice::Ssd(0));
        let ordinary = submit_on(
            &service,
            request(4, Priority::Background),
            SourceDevice::Ssd(0),
        );
        let later = submit_on(&service, native_heic_request(5), SourceDevice::Ssd(0));
        assert_eq!(service.shared.take().unwrap().request.item.id, 4);
        first_job.decoded();
        assert!(service.shared.take().is_none());
        drop(exclusive);
        let later_job = service.shared.take().unwrap();
        assert_eq!(later_job.request.item.id, 5);

        let mut replacement = request(6, Priority::Background);
        replacement.item.width = 10_000;
        replacement.item.height = 10_000;
        let replacement = submit_on(&service, replacement, SourceDevice::Ssd(0));
        let last = submit_on(&service, native_heic_request(7), SourceDevice::Ssd(0));
        assert!(service.shared.take().is_none());
        second_job.decoded();
        assert!(service.shared.take().is_none());
        drop(later_job);
        let exclusive_job = service.shared.take().unwrap();
        assert_eq!(exclusive_job.request.item.id, 6);
        assert!(service.shared.take().is_none());
        drop(exclusive_job);
        assert_eq!(service.shared.take().unwrap().request.item.id, 7);
        drop((first, second, ordinary, later, replacement, last));
    }

    #[test]
    fn shared_job_runs_once_and_cancelling_one_subscriber_keeps_the_other() {
        let service = service();
        let background_request = request(1, Priority::Background);
        let cancel_background = background_request.cancellation.clone();
        let mut duplicate = request(1, Priority::Background);
        duplicate.config.size = 61;
        let mut backgrounds = Vec::new();
        assert!(service.submit_background_page(
            vec![background_request, duplicate],
            |subscription| {
                assert_eq!(service.shared.registry.lock().unwrap().tasks.len(), 1);
                backgrounds.push(subscription);
                true
            }
        ));
        let background = backgrounds.remove(0);
        drop(backgrounds);
        let viewport = service.submit(request(1, Priority::Viewport));
        {
            let registry = service.shared.registry.lock().unwrap();
            assert_eq!(registry.tasks.len(), 1);
            assert!(registry.background.is_empty());
            assert_eq!(registry.viewport.len(), 1);
        }
        let job = service.shared.take().unwrap();
        {
            let mut registry = service.shared.registry.lock().unwrap();
            assert_eq!(registry.tasks.len(), 1);
            assert!(registry.next_key().is_none());
        }
        cancel_background.cancel();
        assert!(service.shared.interested(&job.key, job.id));
        let result = failed_result(&job.request.item);
        finish(&service.shared, job, Some(result));
        assert!(background
            .recv_timeout(Duration::ZERO)
            .unwrap()
            .result
            .is_none());
        assert_eq!(
            viewport
                .recv_timeout(Duration::ZERO)
                .unwrap()
                .result
                .unwrap()
                .item_id,
            1
        );
        // 提交前的新订阅复用已完成结果，不重新进入计算队列。
        let late = service.submit(request(1, Priority::Viewport));
        assert_eq!(
            late.recv_timeout(Duration::ZERO)
                .unwrap()
                .result
                .unwrap()
                .item_id,
            1
        );
        drop((background, viewport, late));
        assert!(service.shared.registry.lock().unwrap().tasks.is_empty());
    }

    #[test]
    fn publication_allows_unrelated_work_but_pins_cancelled_task_identity() {
        let service = Arc::new(service());
        let original = request(1, Priority::Background);
        let cancel = original.cancellation.clone();
        let original = service.submit(original);
        let job = service.shared.take().unwrap();
        let result = failed_result(&job.request.item);
        finish(&service.shared, job, Some(result));
        let (started_tx, started_rx) = bounded(1);
        let (resume_tx, resume_rx) = bounded(1);
        let (normal_tx, normal_rx) = bounded(1);
        let (replacement_tx, replacement_rx) = bounded(1);
        std::thread::scope(|scope| {
            let publisher = scope.spawn(move || {
                ImageSubscription::with_completed_batch(&[original], |completed| {
                    assert!(completed[0].is_some());
                    started_tx.send(()).unwrap();
                    resume_rx.recv().unwrap();
                });
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            cancel.cancel();
            let replacement_service = Arc::clone(&service);
            let replacement = scope.spawn(move || {
                replacement_tx
                    .send(replacement_service.submit(request(1, Priority::Background)))
                    .unwrap();
            });
            let normal_service = Arc::clone(&service);
            let normal = scope.spawn(move || {
                let subscription = normal_service.submit(request(2, Priority::Background));
                let job = normal_service.shared.take().unwrap();
                let id = job.request.item.id;
                let result = failed_result(&job.request.item);
                finish(&normal_service.shared, job, Some(result));
                normal_tx.send((id, subscription)).unwrap();
            });
            let normal_done = normal_rx.recv_timeout(Duration::from_secs(2));
            let replaced_early = replacement_rx.recv_timeout(Duration::from_millis(100));
            // 先放行发布再断言，失败时也不能让作用域线程等待未释放的测试闸门。
            resume_tx.send(()).unwrap();
            publisher.join().unwrap();
            replacement.join().unwrap();
            normal.join().unwrap();
            assert!(
                replaced_early.is_err(),
                "replacement must wait for publication"
            );
            let (id, subscription) =
                normal_done.expect("publication must not block unrelated work");
            assert_eq!(id, 2);
            assert_eq!(
                subscription
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .result
                    .unwrap()
                    .item_id,
                2
            );
            let replacement = replacement_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let job = service.shared.take().unwrap();
            assert_eq!(job.request.item.id, 1);
            drop(replacement);
        });
    }

    #[test]
    fn source_config_identity_and_late_cancelled_completion_cannot_cross_tasks() {
        let service = service();
        let old = service.submit(request(1, Priority::Background));
        let old_job = service.shared.take().unwrap();
        drop(old);
        let current = service.submit(request(1, Priority::Viewport));
        let result = failed_result(&old_job.request.item);
        finish(&service.shared, old_job, Some(result));
        assert!(matches!(
            current.recv_timeout(Duration::ZERO),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        ));
        let mut new_config = request(1, Priority::Viewport);
        new_config.config.webp_quality = 100;
        let changed = service.submit(new_config);
        let mut new_source = request(1, Priority::Viewport);
        new_source.item.source_revision = 2;
        let replaced = service.submit(new_source);
        assert_eq!(service.shared.registry.lock().unwrap().tasks.len(), 3);
        drop((current, changed, replaced));
    }
}
