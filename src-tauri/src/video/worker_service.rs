// src-tauri/src/video/worker_service.rs
//! 视频格式扩展 · host 侧 `VideoWorkerService`(视频格式扩展子系统 design.md §2.1/§2.4/§4.1/§6)。
//!
//! **Service 型 worker**(OCR/Enhance 先例):常见容器的 remux/转码/缩略图增强**不进** exotic
//! 任务化调度,由 host 侧本 Service 直持一个 [`WorkerSupervisor`] + video-worker client。
//! 总闸是 `availability_of(VIDEO_PLUGIN_ID)`(builtin+free 直放行既有语义,§2.1);FFmpeg 组件
//! 未就绪即回 [`VideoServiceError::NeedsComponent`]。
//!
//! ## 并发形态(硬约束:不跨 `.await` 持 std Mutex)
//! supervisor 句柄由**专用阻塞 OS 线程**(driver)独占同步驱动;async 侧只经 std `Mutex`+`Condvar`
//! 的任务队列 + tokio `oneshot` 交互——**driver 线程内无任何 `.await`**,async 侧持 std 锁的窗口
//! 只在同步入队块内、绝不跨 await(Enhance 先例同型)。
//!
//! ## 双优先队列 + 抢占(§2.4 / §9.12)
//! 交互(播放:probe/remux/transcode)> 背景(封面/雪碧图:frames)。交互任务到达且在途为背景任务时,
//! 背景 op 的取消轮询命中「有交互在队」→ 返回 Disconnected → supervisor kill → 背景任务**重回队列头**
//! → driver 先跑交互 → 再重建 worker 续跑背景。同 `(item, kind)` 在途/在队去重挂载同一任务。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use exotic_protocol::{
    ProgressBody, VideoFramesInfo, VideoFramesMode, VideoOutInfo, VideoProbeInfo,
};
use tokio::sync::{oneshot, watch};

use crate::exotic::worker::RawOutcome;
use crate::state::AppState;

/// 握手超时(模型/ffmpeg 装载不在握手期,恒 5s 快失败;同 exotic coordinator 惯例)。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// 单 op 重试预算(genuine Disconnected/worker 通路失败:respawn 后重派一次;§2.3
/// `InternalError` retryable 预算 1 次)。
const MAX_ATTEMPTS: u32 = 2;

mod runner;
/// 视频格式扩展 host 侧 Service 启动期装配(design.md §2.1/§8 V4/V5)。
///
/// 惰性起 worker(首次实际派活才 spawn),此处只装配 Service 本体 + 绑定
/// `video::backend_for` 桥用的 AppState 句柄(晚绑定,同 exotic coordinator 姿态)。
///
/// 自 `lib.rs::run()` 的 setup 段 q 迁出(D-450 纯结构移动,行为不变)。
/// 顺序不变量:必须在 `app.manage(app_state)` 之后调用。
pub fn bootstrap(state: Arc<AppState>) {
    let video_svc = Arc::new(VideoWorkerService::new(state.clone()));
    state.set_video_worker_service(video_svc);
    crate::video::bind_app_state(state);
    tracing::info!("视频格式扩展 Service 已装配 | video format extension service assembled");
}

#[cfg(test)]
mod tests;
mod types;

use runner::WorkerVideoRunner;
use types::map_outcome;
pub use types::{VideoKind, VideoOp, VideoOutput, VideoPriority, VideoServiceError};

/// per-job 进度快照(design.md §5.3:preparing 态要 ffmpeg %)。`percent` 仅在能从
/// Progress 帧解出时更新;`stage="finalize"` 心跳到达时(video-worker 约定其 `detail`
/// 是产物字节数、非百分比)percent 保持上次已知值、`stage` 照常透传。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoProgressSnapshot {
    pub stage: String,
    pub percent: Option<u32>,
}

/// 从 Progress 帧 `detail` 解百分比(video-worker 约定:普通阶段 `detail = "{p}%"`,
/// finalize 心跳 `detail = "{bytes}B"`,§5.3;协议本身无 percent 字段,以现状约定为准)。
/// 非本约定格式(包括 `None`)一律返回 `None`——调用方据此保留上次已知值。
fn parse_percent_detail(detail: Option<&str>) -> Option<u32> {
    detail?.strip_suffix('%')?.parse::<u32>().ok()
}

/// driver 线程对 worker 的最小驱动接口。真实现 [`WorkerVideoRunner`] 直持 supervisor;
/// 测试注入 mock 以验证队列/抢占/去重纯逻辑(worker.rs mock 样式复用)。
pub trait VideoJobRunner: Send {
    /// 保证一个已授权 + FFmpeg 就绪 + 已 `VideoSessionInit` 的活 worker;不可用即回具体错误。
    fn ensure_session(&mut self) -> Result<(), VideoServiceError>;
    /// 跑一个 op。`cancelled` 每 100ms 轮询(supervisor 语义);命中即 kill 在途 worker 返回
    /// `Disconnected`。`total_cap` 为不可重置总上界(transcode 放宽,§2.4)。`on_progress`
    /// 每收到一帧本请求的 Progress(非陈旧)即回调一次(§5.3,供上层转发 percent)。
    fn run_op(
        &mut self,
        op: &VideoOp,
        timeout: Duration,
        total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
        on_progress: &mut dyn FnMut(&ProgressBody),
    ) -> RawOutcome;
    /// 显式销毁当前 worker/session(抢占后强制重建的一步)。
    fn kill(&mut self);
}

type DedupKey = (i64, VideoKind);
type JobResult = Arc<Result<VideoOutput, VideoServiceError>>;

struct Job {
    op: VideoOp,
    priority: VideoPriority,
    /// 去重:同 `(item, kind)` 的全部请求共挂本 job,完成时逐一 fan-out。
    waiters: Vec<oneshot::Sender<JobResult>>,
    /// per-job 硬取消标志(§5.4):`cancel()` 置位;在途 job 由 `run_job` 的 cancelled 闭包
    /// 下一轮询命中,在队未起跑 job 由 `cancel()` 直接出队,二者互斥(标志位对后者是死代码
    /// 但保留以防未来 cancel 时序变化)。
    cancelled: Arc<AtomicBool>,
    /// per-job 进度广播(§5.3):remux/transcode 调用方可选注册,driver 收到 Progress 帧即更新;
    /// probe/frames 恒 `None`(不需要细粒度进度)。
    progress_tx: Option<watch::Sender<VideoProgressSnapshot>>,
}

#[derive(Default)]
struct Inner {
    interactive: VecDeque<DedupKey>,
    background: VecDeque<DedupKey>,
    jobs: HashMap<DedupKey, Job>,
    shutdown: bool,
}

struct SharedState {
    inner: Mutex<Inner>,
    cvar: Condvar,
}

/// 视频格式扩展 host 服务(§2.1)。driver 线程独占 supervisor;公开异步方法入队 + `oneshot` 收结果。
pub struct VideoWorkerService {
    shared: Arc<SharedState>,
    driver: Option<JoinHandle<()>>,
}

impl VideoWorkerService {
    /// 生产装配:直持 supervisor 的真实 runner(惰性起 worker,§2.4)。
    pub fn new(state: Arc<AppState>) -> Self {
        Self::with_runner(Box::new(WorkerVideoRunner {
            state,
            supervisor: None,
            session_id: 0,
        }))
    }

    /// 以任意 runner 装配(测试注入 mock)。启动 driver 阻塞线程。
    pub fn with_runner(runner: Box<dyn VideoJobRunner>) -> Self {
        let shared = Arc::new(SharedState {
            inner: Mutex::new(Inner::default()),
            cvar: Condvar::new(),
        });
        let driver = {
            let shared_for_thread = Arc::clone(&shared);
            match std::thread::Builder::new()
                .name("video-worker-service".into())
                .spawn(move || driver_loop(&shared_for_thread, runner))
            {
                Ok(h) => Some(h),
                Err(e) => {
                    // spawn 失败(§2.4 深审 V4-2):service 永久不可用,置 shutdown 让 enqueue
                    // 侧立即回错误,不留无人处理的挂起 waiter。
                    tracing::error!("video-worker-service driver 线程 spawn 失败:{e}");
                    let mut g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                    g.shutdown = true;
                    None
                }
            }
        };
        VideoWorkerService { shared, driver }
    }

    /// 入队(同步块内持 std 锁,绝不跨 await)。同 `(item, kind)` 命中在途/在队任务即去重挂载。
    /// driver 已不可用(§2.4 深审 V4-2:线程 spawn 失败/service 已 shutdown)时直接回
    /// [`VideoServiceError::WorkerUnavailable`],waiter 不入 map——杜绝无人处理的永久挂起。
    fn enqueue(
        &self,
        op: VideoOp,
        item_id: i64,
        priority: VideoPriority,
    ) -> Result<oneshot::Receiver<JobResult>, VideoServiceError> {
        self.enqueue_with_progress(op, item_id, priority, None)
    }

    /// [`Self::enqueue`] 的进度可注册版(§5.3):`progress_tx` 供 remux/transcode 挂载,
    /// 仅在**新建** job 时生效——去重命中(同 `(item, kind)` 已在途/在队)时沿用既有 job
    /// 自身的 progress_tx(若有),新传入的静默丢弃(同一 job 只应有一个广播源)。去重键取
    /// `op.kind()`——绝大多数调用方(kind 即语义)。
    fn enqueue_with_progress(
        &self,
        op: VideoOp,
        item_id: i64,
        priority: VideoPriority,
        progress_tx: Option<watch::Sender<VideoProgressSnapshot>>,
    ) -> Result<oneshot::Receiver<JobResult>, VideoServiceError> {
        let dedup_kind = op.kind();
        self.enqueue_keyed(op, item_id, priority, progress_tx, dedup_kind)
    }

    /// 内部通用入队:去重键 `(item_id, dedup_kind)` 与 `op.kind()`(供 [`map_outcome`] 解包应答体)
    /// 分离可控——[`Self::probe_verify`](产物级复探,§V6-9)借此以独立维度
    /// ([`VideoKind::ProbeVerify`])避开源 probe 的 `(item_id, VideoKind::Probe)` 键,其余调用方
    /// 经 [`Self::enqueue`]/[`Self::enqueue_with_progress`] 恒传 `op.kind()`,行为不变。
    fn enqueue_keyed(
        &self,
        op: VideoOp,
        item_id: i64,
        priority: VideoPriority,
        progress_tx: Option<watch::Sender<VideoProgressSnapshot>>,
        dedup_kind: VideoKind,
    ) -> Result<oneshot::Receiver<JobResult>, VideoServiceError> {
        let (tx, rx) = oneshot::channel();
        let key: DedupKey = (item_id, dedup_kind);
        {
            let mut g = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.shutdown {
                return Err(VideoServiceError::WorkerUnavailable);
            }
            match g.jobs.get_mut(&key) {
                Some(job) => job.waiters.push(tx), // 去重(§9.12)
                None => {
                    g.jobs.insert(
                        key,
                        Job {
                            op,
                            priority,
                            waiters: vec![tx],
                            cancelled: Arc::new(AtomicBool::new(false)),
                            progress_tx,
                        },
                    );
                    match priority {
                        VideoPriority::Interactive => g.interactive.push_back(key),
                        VideoPriority::Background => g.background.push_back(key),
                    }
                }
            }
        }
        self.shared.cvar.notify_one();
        Ok(rx)
    }

    async fn await_output(
        rx: oneshot::Receiver<JobResult>,
    ) -> Result<VideoOutput, VideoServiceError> {
        match rx.await {
            Ok(arc) => match Arc::try_unwrap(arc) {
                Ok(res) => res,
                // 多 waiter 去重共享同一 Arc:取不到独占所有权时按引用重建(错误 Clone、产物少见并发)。
                Err(arc) => match &*arc {
                    Ok(VideoOutput::Probe(p)) => Ok(VideoOutput::Probe(p.clone())),
                    Ok(VideoOutput::Out(o)) => Ok(VideoOutput::Out(*o)),
                    Ok(VideoOutput::Frames { blob, info }) => Ok(VideoOutput::Frames {
                        blob: Arc::clone(blob),
                        info: *info,
                    }),
                    Err(e) => Err(e.clone()),
                },
            },
            Err(_) => Err(VideoServiceError::Cancelled), // driver 线程已退出
        }
    }

    /// 流事实探测(交互优先级,播放侧默认调用姿态)。
    pub async fn probe(
        &self,
        item_id: i64,
        source_path: String,
        input_fingerprint: String,
    ) -> Result<VideoProbeInfo, VideoServiceError> {
        self.probe_with_priority(
            item_id,
            source_path,
            input_fingerprint,
            VideoPriority::Interactive,
        )
        .await
    }

    /// [`Self::probe`] 的可选优先级版(V5 深审批4):缩略图后端桥
    /// (`WorkerVideoBackend::probe`,批量导入场景)走 Background —— 不抢占播放交互队列在途
    /// kill、也不堵播放侧 probe;播放侧仍走 [`Self::probe`] 恒 Interactive。
    pub async fn probe_with_priority(
        &self,
        item_id: i64,
        source_path: String,
        input_fingerprint: String,
        priority: VideoPriority,
    ) -> Result<VideoProbeInfo, VideoServiceError> {
        let rx = self.enqueue(
            VideoOp::Probe {
                source_path,
                input_fingerprint,
            },
            item_id,
            priority,
        )?;
        match Self::await_output(rx).await? {
            VideoOutput::Probe(p) => Ok(p),
            _ => Err(VideoServiceError::WorkerFailed),
        }
    }

    /// 产物级复探(§V6-9 深检,playback_orchestrator.rs `spawn_playable_job` rename 前调用):与
    /// [`Self::probe`] 同为 VideoProbe 请求,但去重键取独立维度 [`VideoKind::ProbeVerify`]、
    /// **不与**源 probe 共 `(item_id, VideoKind::Probe)` 键——防并发窗口内产物复探误挂靠/
    /// 取消同一 item 的源 probe(反之亦然)。调用方**不得**把此结果回填 `video_meta`
    /// (只用于验收比对,写回仍走源 `probe()` 的 `write_back_probe`)。
    pub async fn probe_verify(
        &self,
        item_id: i64,
        source_path: String,
        input_fingerprint: String,
    ) -> Result<VideoProbeInfo, VideoServiceError> {
        let rx = self.enqueue_keyed(
            VideoOp::Probe {
                source_path,
                input_fingerprint,
            },
            item_id,
            VideoPriority::Interactive,
            None,
            VideoKind::ProbeVerify,
        )?;
        match Self::await_output(rx).await? {
            VideoOutput::Probe(p) => Ok(p),
            _ => Err(VideoServiceError::WorkerFailed),
        }
    }

    /// 容器改封(交互优先级)。`probe_duration_ms` 供算 remux 总上界(§2.4 深审 V4-1);
    /// `progress` 可选注册进度广播(§5.3,video_commands.rs 转发用)。
    #[allow(clippy::too_many_arguments)]
    pub async fn remux(
        &self,
        item_id: i64,
        source_path: String,
        output_tmp_path: String,
        audio_transcode: bool,
        audio_track_index: Option<u32>,
        probe_duration_ms: Option<u64>,
        progress: Option<watch::Sender<VideoProgressSnapshot>>,
    ) -> Result<VideoOutInfo, VideoServiceError> {
        let rx = self.enqueue_with_progress(
            VideoOp::Remux {
                source_path,
                output_tmp_path,
                audio_transcode,
                audio_track_index,
                probe_duration_ms,
            },
            item_id,
            VideoPriority::Interactive,
            progress,
        )?;
        match Self::await_output(rx).await? {
            VideoOutput::Out(o) => Ok(o),
            _ => Err(VideoServiceError::WorkerFailed),
        }
    }

    /// 一次性全转码(交互优先级)。`probe_duration_ms` 供算 transcode 总上界(§2.4);
    /// `progress` 可选注册进度广播(§5.3)。
    #[allow(clippy::too_many_arguments)]
    pub async fn transcode(
        &self,
        item_id: i64,
        source_path: String,
        output_tmp_path: String,
        encoder_ladder: Vec<String>,
        crf: Option<u8>,
        bitrate_kbps: Option<u32>,
        max_long_edge: Option<u32>,
        audio_track_index: Option<u32>,
        hw_decode: bool,
        probe_duration_ms: Option<u64>,
        progress: Option<watch::Sender<VideoProgressSnapshot>>,
    ) -> Result<VideoOutInfo, VideoServiceError> {
        let rx = self.enqueue_with_progress(
            VideoOp::Transcode {
                source_path,
                output_tmp_path,
                encoder_ladder,
                crf,
                bitrate_kbps,
                max_long_edge,
                audio_track_index,
                hw_decode,
                probe_duration_ms,
            },
            item_id,
            VideoPriority::Interactive,
            progress,
        )?;
        match Self::await_output(rx).await? {
            VideoOutput::Out(o) => Ok(o),
            _ => Err(VideoServiceError::WorkerFailed),
        }
    }

    /// 封面/雪碧图取帧(背景优先级——交互播放到达即被抢占,§2.4)。返回 (WebP blob, 切格元数据)。
    pub async fn frames(
        &self,
        item_id: i64,
        source_path: String,
        input_fingerprint: String,
        mode: VideoFramesMode,
    ) -> Result<(Arc<Vec<u8>>, Option<VideoFramesInfo>), VideoServiceError> {
        let rx = self.enqueue(
            VideoOp::Frames {
                source_path,
                input_fingerprint,
                mode,
            },
            item_id,
            VideoPriority::Background,
        )?;
        match Self::await_output(rx).await? {
            VideoOutput::Frames { blob, info } => Ok((blob, info)),
            _ => Err(VideoServiceError::WorkerFailed),
        }
    }

    /// per-job 硬取消(§5.4,video_commands.rs `cancel_video_playback` 收口):
    /// - 在队未起跑 → 直接出队 + 立即以 `Cancelled` fan-out 全部 waiter(不必等 driver 拿到)。
    /// - 已在途(driver 已 pop)→ 仅置 job 的 cancelled 标志,`run_job` 的 cancelled 闭包下一轮询
    ///   (≤100ms,supervisor 既有轮询节奏)命中即 kill 在途 worker 并回 `Cancelled`
    ///   (kill/respawn 语义与抢占复用,不 respawn 重试)。
    ///
    /// 未命中任何在途/在队 job(已完成或本就不存在)时静默 no-op。
    pub fn cancel(&self, item_id: i64, kind: VideoKind) {
        let key: DedupKey = (item_id, kind);
        let mut g = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(job) = g.jobs.get(&key) else {
            return;
        };
        job.cancelled.store(true, Ordering::SeqCst);

        let queued_pos = g
            .interactive
            .iter()
            .position(|k| *k == key)
            .map(|i| (true, i))
            .or_else(|| {
                g.background
                    .iter()
                    .position(|k| *k == key)
                    .map(|i| (false, i))
            });
        if let Some((is_interactive, idx)) = queued_pos {
            if is_interactive {
                g.interactive.remove(idx);
            } else {
                g.background.remove(idx);
            }
            if let Some(job) = g.jobs.remove(&key) {
                let res: JobResult = Arc::new(Err(VideoServiceError::Cancelled));
                for w in job.waiters {
                    let _ = w.send(Arc::clone(&res));
                }
            }
        }
        // else:已在途 —— cancelled 标志已置,交给 run_job 的 cancelled 闭包下一轮询处理。
    }
}

impl Drop for VideoWorkerService {
    fn drop(&mut self) {
        {
            let mut g = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.shutdown = true;
        }
        self.shared.cvar.notify_all();
        if let Some(h) = self.driver.take() {
            let _ = h.join();
        }
    }
}

/// driver 阻塞循环:取下一任务(交互优先)→ 运行 → fan-out。**无 `.await`,std 锁在同步块内用**。
fn driver_loop(shared: &Arc<SharedState>, mut runner: Box<dyn VideoJobRunner>) {
    loop {
        let key = {
            let mut g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if g.shutdown {
                    // 收尾:未决任务全部以 Cancelled 回收 waiter,driver 退出,runner drop → worker kill。
                    let keys: Vec<DedupKey> = g.jobs.keys().copied().collect();
                    for k in keys {
                        if let Some(job) = g.jobs.remove(&k) {
                            let res: JobResult = Arc::new(Err(VideoServiceError::Cancelled));
                            for w in job.waiters {
                                let _ = w.send(Arc::clone(&res));
                            }
                        }
                    }
                    return;
                }
                if let Some(k) = g
                    .interactive
                    .pop_front()
                    .or_else(|| g.background.pop_front())
                {
                    break k;
                }
                g = shared.cvar.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        };
        run_job(shared, &mut *runner, key);
    }
}

fn run_job(shared: &Arc<SharedState>, runner: &mut dyn VideoJobRunner, key: DedupKey) {
    // 取出 op + 优先级(锁外运行;op Clone 廉价——paths/指纹字符串)。
    let (op, is_background) = {
        let g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        match g.jobs.get(&key) {
            Some(job) => (job.op.clone(), job.priority == VideoPriority::Background),
            None => return, // 兜底:不应发生(pop 出的 key 恒在 map)
        }
    };

    let mut attempts = 0u32;
    loop {
        attempts += 1;
        if let Err(e) = runner.ensure_session() {
            deliver(shared, key, Arc::new(Err(e)));
            return;
        }

        // 抢占探测器:背景任务运行期,一旦有交互任务在队 → 置 preempted 并令 run_op 取消(kill worker)。
        // `shut` 独立于 preempted:cancelled() 命中 shutdown 分支时置位,供 run_op 返回后与
        // preempted 同位检查——命中即直接 Cancelled 回 waiter,不再走 respawn+ensure_session
        // 徒劳一轮(§2.4 深审 V4-3)。`job_cancel` 为 per-job 硬取消(§5.4):`VideoWorkerService::cancel`
        // 置位 job 的 cancelled 标志后,本闭包下一轮询命中即令 run_op 返回(worker 侧同抢占一样
        // 被 kill 回收),run_job 直接回 Cancelled、不 respawn 重试——判定优先于抢占(显式取消恒赢)。
        let preempted = Arc::new(AtomicBool::new(false));
        let shut = Arc::new(AtomicBool::new(false));
        let job_cancel = Arc::new(AtomicBool::new(false));
        let cancelled = {
            let shared = Arc::clone(shared);
            let preempted = Arc::clone(&preempted);
            let shut = Arc::clone(&shut);
            let job_cancel = Arc::clone(&job_cancel);
            move || -> bool {
                let g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                if g.shutdown {
                    shut.store(true, Ordering::SeqCst);
                    return true;
                }
                if g.jobs
                    .get(&key)
                    .map(|j| j.cancelled.load(Ordering::SeqCst))
                    .unwrap_or(false)
                {
                    job_cancel.store(true, Ordering::SeqCst);
                    return true;
                }
                if is_background && !g.interactive.is_empty() {
                    preempted.store(true, Ordering::SeqCst);
                    return true;
                }
                false
            }
        };

        // 进度观察者(§5.3):收到 Progress 帧即据 job 当前 progress_tx(若已注册)广播快照。
        // finalize 心跳(detail 是字节数非百分比)解不出 percent → `.or(last_percent)` 保留上次值。
        let mut on_progress = {
            let shared = Arc::clone(shared);
            move |p: &ProgressBody| {
                let g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = g.jobs.get(&key) {
                    if let Some(tx) = &job.progress_tx {
                        let last_percent = tx.borrow().percent;
                        let percent = parse_percent_detail(p.detail.as_deref()).or(last_percent);
                        let _ = tx.send(VideoProgressSnapshot {
                            stage: p.stage.clone(),
                            percent,
                        });
                    }
                }
            }
        };
        let outcome = runner.run_op(
            &op,
            op.silence_timeout(),
            op.total_cap(),
            &cancelled,
            &mut on_progress,
        );
        drop(cancelled);
        drop(on_progress);

        if shut.load(Ordering::SeqCst) {
            // shutdown 命中:worker 已被 kill(Disconnected/取消态),直接回 Cancelled,不 respawn 重派。
            deliver(shared, key, Arc::new(Err(VideoServiceError::Cancelled)));
            return;
        }

        if job_cancel.load(Ordering::SeqCst) {
            // per-job 硬取消命中(§5.4):worker 已被 supervisor 因 Disconnected 回收(kill_and_reap),
            // 这里再显式 kill 一次兜底(幂等,Drop 语义);直接回 Cancelled,不 respawn 重试。
            runner.kill();
            deliver(shared, key, Arc::new(Err(VideoServiceError::Cancelled)));
            return;
        }

        if preempted.load(Ordering::SeqCst) {
            // 抢占:背景任务重回队列头,强制销毁 worker(下轮 ensure_session 重建),交回 driver 先跑交互。
            runner.kill();
            let mut g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.background.push_front(key);
            return;
        }

        match map_outcome(&op, outcome) {
            Ok(out) => {
                deliver(shared, key, Arc::new(Ok(out)));
                return;
            }
            Err(e) if e.retryable() && attempts < MAX_ATTEMPTS => {
                tracing::warn!(
                    "video-worker op 失败({}),respawn 重试 {}/{}",
                    e.code(),
                    attempts,
                    MAX_ATTEMPTS
                );
                // worker 已被 supervisor kill;ensure_session 下轮重建后重派。
                continue;
            }
            Err(e) => {
                deliver(shared, key, Arc::new(Err(e)));
                return;
            }
        }
    }
}

/// 完成:从 map 摘除 job(结束在途/去重态)并向全部 waiter fan-out。
fn deliver(shared: &Arc<SharedState>, key: DedupKey, result: JobResult) {
    let job = {
        let mut g = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.jobs.remove(&key)
    };
    if let Some(job) = job {
        for w in job.waiters {
            let _ = w.send(Arc::clone(&result));
        }
    }
}
