// src-tauri/src/exotic/worker_traits.rs
//! Worker 行为 trait 分类学(U-P3,2026-07-16 从 pipeline.rs 拆出)。
//!
//! [`ThumbnailWorker`] 服务 exotic 缩略图流水线;[`EmbedWorker`] 只服务 AI 层
//! (`ai::worker_client`),流水线自身用不到——两族 trait 与流水线状态机没有共享
//! 状态,分离后 pipeline.rs 回归单一状态机。旧路径 `exotic::pipeline::{...}`
//! 经 pipeline.rs 的 `pub use` 继续可用。

use std::time::Duration;

use exotic_protocol::RequestBody;

use super::outcome::{RawOutcome, TaskOutcome};
use super::worker::{WorkerConfig, WorkerLimits, WorkerSpec};

/// Worker 生命周期公共面(T15 拆分,Part6 §3.4.2 C5):thumbnail 与 embed 两类 worker
/// 共享的簿记能力;各自的任务方法在子 trait。
pub trait WorkerTask: Send {
    fn worker_version(&self) -> String;
    fn is_alive(&self) -> bool;
    fn shutdown(self: Box<Self>, grace: Duration);
}

/// 缩略图 Worker 行为（[`crate::exotic::supervisor::WorkerSupervisor`] 实现；测试用 mock）。
pub trait ThumbnailWorker: WorkerTask {
    fn run_thumbnail(
        &mut self,
        req: &RequestBody,
        limits: &WorkerLimits,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> TaskOutcome;
}

/// 嵌入/人脸 Worker 行为(C5;会话生命周期 + 批量 op)。消费方 = T17 的
/// [`crate::ai::worker_client::AiWorkerClient`];批结果校验用 [`super::validate`] 的 validate_*。
pub trait EmbedWorker: WorkerTask {
    /// 当前已加载会话的快照;None = 未加载。派批前 host 据此比对目标模型,
    /// 不符则先 close 再 init(切换语义,D3 §4②)。
    fn session(&self) -> Option<&crate::exotic::supervisor::SessionDescriptor>;
    /// SessionInit(记录会话快照);超时用 op_timeouts::SESSION_INIT 档。
    fn init_session(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome;
    /// SessionClose(幂等;host 主导卸载)。
    fn close_session(&mut self, timeout: Duration, cancelled: &dyn Fn() -> bool) -> RawOutcome;
    /// EmbedBatch / FaceDetectEmbed / EncodeText 批请求(Success 未经 op 校验,调用方分派验证)。
    fn run_batch(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome;
}

impl WorkerTask for crate::exotic::supervisor::WorkerSupervisor {
    fn worker_version(&self) -> String {
        crate::exotic::supervisor::WorkerSupervisor::worker_version(self).to_string()
    }
    fn is_alive(&self) -> bool {
        crate::exotic::supervisor::WorkerSupervisor::is_alive(self)
    }
    fn shutdown(self: Box<Self>, grace: Duration) {
        crate::exotic::supervisor::WorkerSupervisor::shutdown(*self, grace)
    }
}

impl ThumbnailWorker for crate::exotic::supervisor::WorkerSupervisor {
    fn run_thumbnail(
        &mut self,
        req: &RequestBody,
        limits: &WorkerLimits,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> TaskOutcome {
        crate::exotic::supervisor::WorkerSupervisor::run_thumbnail(
            self, req, limits, timeout, cancelled,
        )
    }
}

impl EmbedWorker for crate::exotic::supervisor::WorkerSupervisor {
    fn session(&self) -> Option<&crate::exotic::supervisor::SessionDescriptor> {
        crate::exotic::supervisor::WorkerSupervisor::session(self)
    }
    fn init_session(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        crate::exotic::supervisor::WorkerSupervisor::init_session(self, req, timeout, cancelled)
    }
    fn close_session(&mut self, timeout: Duration, cancelled: &dyn Fn() -> bool) -> RawOutcome {
        crate::exotic::supervisor::WorkerSupervisor::close_session(self, timeout, cancelled)
    }
    fn run_batch(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        crate::exotic::supervisor::WorkerSupervisor::run_request(self, req, timeout, cancelled)
    }
}

/// Worker 工厂：按需创建新 Worker 实例（崩溃后补充池）。
pub trait WorkerFactory: Send + Sync {
    fn spawn(&self) -> Result<Box<dyn ThumbnailWorker>, String>;
}

/// 真实工厂：从 [`WorkerSpec`] + [`WorkerConfig`] 创建
/// [`crate::exotic::supervisor::WorkerSupervisor`]。
pub struct SupervisorFactory {
    pub spec: WorkerSpec,
    pub cfg: WorkerConfig,
}

impl WorkerFactory for SupervisorFactory {
    fn spawn(&self) -> Result<Box<dyn ThumbnailWorker>, String> {
        let sup = crate::exotic::supervisor::WorkerSupervisor::spawn(&self.spec, &self.cfg)?;
        Ok(Box::new(sup))
    }
}

/// 视频缩略图 Worker 包装(rmvb/vob 收编,D-444③):video-worker 是 Service 型进程——
/// 任务化 Thumbnail 请求本身不携带 ffmpeg 路径(协议 `Thumbnail` 无此字段,亦无 session_id)。
/// 故本包装在首个 thumbnail 派发前,先经既有公共面 [`WorkerSupervisor::run_request`] 发一次
/// `VideoSessionInit`(ffmpeg 路径 + sha256 防换包 + work_dir 白名单,与 Service 门同样式),
/// 会话就绪后再 [`WorkerSupervisor::run_thumbnail`] 派发缩略图——不改 supervisor/协议,复用
/// 现有两个公共面。实例被 kill/重建后 `session_ready` 随新实例归 false,自动重 Init(§4⑤)。
pub struct VideoThumbnailWorker {
    inner: crate::exotic::supervisor::WorkerSupervisor,
    /// `VideoSessionInit` 请求模板(session_id 固定 1:每个 worker 进程单会话)。
    init: RequestBody,
    /// 本实例是否已成功建立视频会话。
    session_ready: bool,
}

impl WorkerTask for VideoThumbnailWorker {
    fn worker_version(&self) -> String {
        self.inner.worker_version().to_string()
    }
    fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }
    fn shutdown(self: Box<Self>, grace: Duration) {
        self.inner.shutdown(grace)
    }
}

impl ThumbnailWorker for VideoThumbnailWorker {
    fn run_thumbnail(
        &mut self,
        req: &RequestBody,
        limits: &WorkerLimits,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> TaskOutcome {
        if !self.session_ready {
            // VideoSessionInit 用 host 侧 per-op 表的 SESSION_INIT 静默限时档(冷起 ffmpeg
            // -version 秒级,余量充足)。进程级异常(超时/断开/协议违例)时 run_request 已
            // kill 回收,本实例随即 is_alive=false,pipeline 会重建。
            match self.inner.run_request(
                &self.init,
                crate::exotic::coordinator::op_timeouts::SESSION_INIT,
                cancelled,
            ) {
                RawOutcome::Success { body, .. } if body.video_session.is_some() => {
                    self.session_ready = true;
                }
                RawOutcome::Success { .. } => {
                    return TaskOutcome::Protocol(
                        "VideoSessionInit Success 缺 video_session 应答体".into(),
                    );
                }
                RawOutcome::Failure(fb) => return TaskOutcome::Failure(fb),
                RawOutcome::TimedOut => return TaskOutcome::TimedOut,
                RawOutcome::Disconnected => return TaskOutcome::Disconnected,
                RawOutcome::Protocol(r) => return TaskOutcome::Protocol(r),
            }
        }
        self.inner.run_thumbnail(req, limits, timeout, cancelled)
    }
}

/// 视频缩略图工厂:spawn video-worker 后包装为 [`VideoThumbnailWorker`],携 `VideoSessionInit`
/// 模板(ffmpeg 路径/sha/work_dir 由 coordinator 从 tools 就绪态解析后填入)。
pub struct VideoThumbnailFactory {
    pub spec: WorkerSpec,
    pub cfg: WorkerConfig,
    /// `RequestBody::VideoSessionInit` 模板(coordinator 构造)。
    pub init: RequestBody,
}

impl WorkerFactory for VideoThumbnailFactory {
    fn spawn(&self) -> Result<Box<dyn ThumbnailWorker>, String> {
        let sup = crate::exotic::supervisor::WorkerSupervisor::spawn(&self.spec, &self.cfg)?;
        Ok(Box::new(VideoThumbnailWorker {
            inner: sup,
            init: self.init.clone(),
            session_ready: false,
        }))
    }
}
