// src-tauri/src/exotic/supervisor.rs
//! 冷门格式插件 · WorkerSupervisor（v3 Part2 §3.6）。
//!
//! 持有真实子进程，并把进程生命周期与 [`super::worker::WorkerConn`] 的协议状态机绑在一起：
//!
//! ```text
//! Supervisor（control）── 持 Child + stdin（经 WorkerConn 写）
//!        ├── stdout reader 线程 ── FrameResult channel ──> WorkerConn.rx
//!        └── stderr drain 线程  ── 有界环形缓冲（最近 64 KiB）
//! ```
//!
//! 关键边界：
//!   - 每 Supervisor **同一时刻只一个请求**；并发由进程池实现（Pipeline 控制）。
//!   - 超时 / 断开 / 协议违例 → `kill → wait`，标实例死亡；池补新实例前由 Pipeline 做崩溃退避。
//!   - stderr 持续排空（独立线程），防管道写满死锁；只保留最近 64 KiB 诊断。
//!   - Drop 兜底 kill + wait + join，绝不留孤儿进程或泄漏线程。

use std::collections::VecDeque;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tracing::{debug, warn};

use super::worker::{
    spawn_frame_reader, spawn_worker_process, RawOutcome, TaskOutcome, WorkerConfig, WorkerConn,
    WorkerLimits, WorkerSpec,
};
use super::worker_log::{spawn_stderr_drain, worker_kind_label};
use exotic_protocol::{ProgressBody, RequestBody, SuccessBody};

/// 子进程句柄抽象(R2-5 测试缝):生产实现为 [`Child`] 的 1:1 机械委托。
/// ExitStatus 被整体擦除——本模块所有调用点本就丢弃它(`let _ = wait()`、try_wait 只
/// match `Ok(Some(_))`),擦除不损失任何决策信息;kill/wait/try_wait 的调用语义与顺序不变。
/// 真实 Child 路径由显式启用的冒烟测试(real_worker_thumbnail_and_shutdown)端到端覆盖。
trait ChildHandle: Send {
    fn kill(&mut self) -> std::io::Result<()>;
    fn wait(&mut self) -> std::io::Result<()>;
    fn try_wait(&mut self) -> std::io::Result<Option<()>>;
}

impl ChildHandle for Child {
    fn kill(&mut self) -> std::io::Result<()> {
        Child::kill(self)
    }
    fn wait(&mut self) -> std::io::Result<()> {
        Child::wait(self).map(|_| ())
    }
    fn try_wait(&mut self) -> std::io::Result<Option<()>> {
        Child::try_wait(self).map(|o| o.map(|_| ()))
    }
}

/// stderr 环形缓冲上限：只保留最近 64 KiB 诊断，不无限累积内存（§3.4）。
/// worker stderr 行日志转发(日志能力重构线 阶段 3 · W4,D-310/D-313)已迁至
/// `worker_log` 模块:字节级环形缓冲(本常量)与行级 `worker_log::LineScanner`
/// 是两套并行机制(D-313 已裁决双写可接受),迁移/改动时**不得合并**。
const STDERR_RING_CAP: usize = 64 * 1024;

/// 已加载会话的 host 侧快照(T15,D3 §4②):记录 SessionInit 时的 model_profile 关键字段
/// 与 SessionReady 应答。派批前 host 据此比对目标模型——不符则先 SessionClose 再
/// SessionInit(切换语义);实例被 kill 时随之清零(重建后首个动作必然是重 Init,§4⑤)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDescriptor {
    pub session_id: u64,
    /// CLIP 架构族 id(= `ai_embeddings.model_name`,向量空间身份)。
    pub arch_id: String,
    /// 选定的图像塔 batch 变体文件(同架构不同变体切换也须重 Init)。
    pub image_file: String,
    pub face_profile_id: Option<String>,
    /// SessionInit 快照里的单批上限(2026-07-10 审查 W3):worker 端按此硬拒超限批
    /// (terminal),host 侧 matches 必须比对——否则「先小批建会话(如搜索)→ 调大配置
    /// → 复用旧会话派大批」会被 worker 拒到整轮打死。
    pub batch_size: u32,
    /// SessionReady 回报的嵌入维度(EmbedBatch blob 校验用)。
    pub embed_dim: u32,
    pub face_embed_dim: Option<u32>,
    /// 本会话实际可服务的能力(worker 声明,host 派活依据)。
    pub caps: Vec<String>,
    /// worker 回声的执行提供器/GPU 名(T16;None=旧 worker 帧,host 保留既有配置)。
    pub provider: Option<String>,
    pub gpu_name: Option<String>,
}

/// 长驻 Worker 的监督者。
pub struct WorkerSupervisor {
    child: Box<dyn ChildHandle>,
    conn: WorkerConn,
    stderr_ring: Arc<Mutex<VecDeque<u8>>>,
    reader_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
    /// 实例死亡（超时/断开/协议违例后置位）；池据此回收并补新实例。
    alive: bool,
    worker_version: String,
    /// 已加载会话快照(T15);None = 未加载/已卸载/实例已死。
    session: Option<SessionDescriptor>,
}

impl WorkerSupervisor {
    /// 启动并完成握手。失败则 kill 子进程后返回 Err（不留孤儿）。
    ///
    /// 注：§3.6 启动顺序第一步「验证安装记录与当前文件 hash」在 Part3 接入；Part2 仅校验路径存在。
    pub fn spawn(spec: &WorkerSpec, cfg: &WorkerConfig) -> Result<Self, String> {
        let mut child =
            spawn_worker_process(spec).map_err(|e| format!("创建 Worker 进程失败：{e}"))?;

        let stdin = child.stdin.take().ok_or("无法获取 Worker stdin")?;
        let stdout = child.stdout.take().ok_or("无法获取 Worker stdout")?;
        let stderr = child.stderr.take().ok_or("无法获取 Worker stderr")?;

        // stdout → 协议帧 channel。
        // 每实例只有一个请求；帧队列背压限制异常 worker 的宿主内存占用。
        let (tx, rx) = crossbeam_channel::bounded(2);
        let reader_handle = spawn_frame_reader(stdout, tx);

        // stderr → 有界环形缓冲（持续排空，防管道写满死锁）+ 行级转发进主 tracing（W4）。
        let stderr_ring = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING_CAP)));
        let worker_kind = worker_kind_label(spec);
        let stderr_handle = spawn_stderr_drain(
            stderr,
            Arc::clone(&stderr_ring),
            STDERR_RING_CAP,
            worker_kind,
        );

        // 握手（写 Hello → 等 Ready → 校验）。失败要 kill 兜底。
        let conn = match WorkerConn::handshake(Box::new(stdin), rx, spec, cfg) {
            Ok(c) => c,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader_handle.join();
                let _ = stderr_handle.join();
                return Err(format!("握手失败：{e}"));
            }
        };

        let worker_version = conn.worker_version().to_string();
        Ok(WorkerSupervisor {
            child: Box::new(child),
            conn,
            stderr_ring,
            reader_handle: Some(reader_handle),
            stderr_handle: Some(stderr_handle),
            alive: true,
            worker_version,
            session: None,
        })
    }

    pub fn worker_version(&self) -> &str {
        &self.worker_version
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// 当前已加载会话的快照;None = 未加载(派批前 host 据此判定是否需先 Init/切换)。
    pub fn session(&self) -> Option<&SessionDescriptor> {
        self.session.as_ref()
    }

    /// 跑任意 op 的请求(T15 泛化,D3 §4①)。进程级异常(超时/断开/协议违例)时
    /// `kill → wait` 并标死亡,与 [`Self::run_thumbnail`] 同一回收语义;返回的
    /// `Success` **未经 op 校验**,由调用方按 op 分派验证。
    pub fn run_request(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        self.run_request_capped(
            req,
            timeout,
            crate::exotic::worker::PROGRESS_TOTAL_CAP,
            cancelled,
        )
    }

    /// [`Self::run_request`] 的 per-op 总上界参数化版(视频格式扩展子系统 design.md §2.4):
    /// `timeout` 为静默限时、`total_cap` 为不可重置总上界。进程级异常同一 kill 回收语义。
    /// video transcode 传 `max(2h, 探测时长×6)`;其余 op 由 `run_request` 传缺省 3600s。
    pub fn run_request_capped(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        self.run_request_observed(req, timeout, total_cap, cancelled, None)
    }

    /// 同 [`Self::run_request`],额外透传 per-tile 进度回调(深审 b)。`None` 时行为
    /// 与 `run_request` 完全一致;委托到 [`Self::run_request_observed`] 并取缺省 total_cap。
    pub fn run_request_with_progress(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
        on_progress: Option<&mut dyn FnMut(&ProgressBody)>,
    ) -> RawOutcome {
        self.run_request_observed(
            req,
            timeout,
            crate::exotic::worker::PROGRESS_TOTAL_CAP,
            cancelled,
            on_progress,
        )
    }

    /// [`Self::run_request_capped`] 的进度可观察版(视频格式扩展子系统 design.md §5.3):
    /// 镜像 `total_cap` 参数化时的转发手法——不动私有字段结构,只把观察者原样转发给
    /// [`WorkerConn::run_request_observed`];kill 回收 / 死亡判定逻辑与
    /// [`Self::run_request_capped`] 完全一致。
    pub fn run_request_observed(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
        progress_observer: Option<&mut dyn FnMut(&ProgressBody)>,
    ) -> RawOutcome {
        if !self.alive {
            return RawOutcome::Disconnected;
        }
        let outcome =
            self.conn
                .run_request_observed(req, timeout, total_cap, cancelled, progress_observer);
        match &outcome {
            RawOutcome::Success { .. } | RawOutcome::Failure(_) => {}
            RawOutcome::TimedOut | RawOutcome::Disconnected | RawOutcome::Protocol(_) => {
                warn!(
                    "Worker 实例异常（{}）→ kill 回收；stderr 尾部：{}",
                    raw_outcome_label(&outcome),
                    self.stderr_tail_lossy()
                );
                self.kill_and_reap();
            }
        }
        outcome
    }

    /// 发送 SessionInit 并在成功时记录会话快照(D3 §4②)。`req` 必须是
    /// `RequestBody::SessionInit`;超时用 host 侧 per-op 表的 SESSION_INIT 档
    /// (300s,冷加载 ViT-L + DirectML 编译内核上界,D3 §2)。
    /// 超时/断开走 [`Self::run_request`] 的 kill 回收,session 随实例清零(§4⑤)。
    pub fn init_session(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        let RequestBody::SessionInit {
            session_id,
            model_profile,
            ..
        } = req
        else {
            // host 调用错误(非 worker 过错):不发帧、不 kill,仅回诊断。
            return RawOutcome::Protocol("init_session 需要 SessionInit 请求体".into());
        };
        let (session_id, model_profile) = (*session_id, model_profile.clone());

        let outcome = self.run_request(req, timeout, cancelled);
        if let RawOutcome::Success { body, .. } = &outcome {
            match &body.session {
                Some(ready) => {
                    self.session = Some(SessionDescriptor {
                        session_id,
                        arch_id: model_profile.arch_id,
                        image_file: model_profile.image_file,
                        face_profile_id: model_profile.face_profile_id,
                        batch_size: model_profile.batch_size,
                        embed_dim: ready.embed_dim,
                        face_embed_dim: ready.face_embed_dim,
                        caps: ready.caps.clone(),
                        provider: ready.provider.clone(),
                        gpu_name: ready.gpu_name.clone(),
                    });
                }
                None => {
                    // SessionReady 就是 SessionInit 的 Success(D3 §2);缺应答体即协议违例。
                    warn!("SessionInit Success 缺 session 应答体 → kill 回收");
                    self.kill_and_reap();
                    return RawOutcome::Protocol("SessionInit Success 缺 session 应答体".into());
                }
            }
        }
        outcome
    }

    /// 发送 SessionClose 显式卸载(host 主导生命周期:空闲计时到期/切换模型前,D3 §4④)。
    /// 幂等:无在载会话时不发帧、直接回成功语义;无论结果如何 host 侧快照即刻清空
    /// (失败路径 run_request 已 kill,worker 端会话随进程消亡)。
    pub fn close_session(&mut self, timeout: Duration, cancelled: &dyn Fn() -> bool) -> RawOutcome {
        let Some(desc) = self.session.take() else {
            return RawOutcome::Success {
                body: SuccessBody::default(),
                blob: Vec::new(),
            };
        };
        self.run_request(
            &RequestBody::SessionClose {
                session_id: desc.session_id,
            },
            timeout,
            cancelled,
        )
    }

    /// 跑一个缩略图任务。超时/断开/协议违例时 `kill → wait` 并标死亡（不再复用本实例）。
    /// `cancelled` 由上层注入（stop/退出）；命中时等待中止并 `kill` 在途 Worker（v3.1 §4.1）。
    pub fn run_thumbnail(
        &mut self,
        req: &RequestBody,
        limits: &WorkerLimits,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> TaskOutcome {
        if !self.alive {
            return TaskOutcome::Disconnected;
        }
        let outcome = self.conn.run_thumbnail(req, limits, timeout, cancelled);
        match &outcome {
            TaskOutcome::Success { .. } | TaskOutcome::Failure(_) => {}
            TaskOutcome::TimedOut | TaskOutcome::Disconnected | TaskOutcome::Protocol(_) => {
                warn!(
                    "Worker 实例异常（{}）→ kill 回收；stderr 尾部：{}",
                    outcome_label(&outcome),
                    self.stderr_tail_lossy()
                );
                self.kill_and_reap();
            }
        }
        outcome
    }

    /// 取 stderr 环形缓冲的有损字符串（诊断用）。
    pub fn stderr_tail_lossy(&self) -> String {
        let ring = self.stderr_ring.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&ring.iter().copied().collect::<Vec<u8>>()).into_owned()
    }

    /// kill → wait → join 读取线程；标实例死亡。幂等。
    /// 会话快照随实例清零(D3 §4⑤:重建后首个动作必然是重 Init,天然正确)。
    fn kill_and_reap(&mut self) {
        if !self.alive {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.alive = false;
        self.session = None;
        self.join_threads();
    }

    fn join_threads(&mut self) {
        // reader 可能阻塞于满队列；先断开接收端再 join，取消/Drop 也可正常回收。
        self.conn.close_reader();
        if let Some(h) = self.reader_handle.take() {
            let _ = h.join();
        }
        if let Some(h) = self.stderr_handle.take() {
            let _ = h.join();
        }
    }

    /// 优雅关闭：发 Shutdown → 等宽限期 → 超时 kill → wait → join。消费 self。
    pub fn shutdown(mut self, grace: Duration) {
        if !self.alive {
            self.join_threads();
            return;
        }
        self.conn.send_shutdown();
        let deadline = Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.alive = false;
                    break;
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        debug!("Worker 未在宽限期内退出 → kill");
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                        self.alive = false;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    self.alive = false;
                    break;
                }
            }
        }
        self.join_threads();
    }
}

impl Drop for WorkerSupervisor {
    fn drop(&mut self) {
        // 兜底：绝不留孤儿进程。已死则只 join。
        if self.alive {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.alive = false;
        }
        self.join_threads();
    }
}

fn outcome_label(o: &TaskOutcome) -> &'static str {
    match o {
        TaskOutcome::Success { .. } => "success",
        TaskOutcome::Failure(_) => "failure",
        TaskOutcome::TimedOut => "timeout",
        TaskOutcome::Disconnected => "disconnected",
        TaskOutcome::Protocol(_) => "protocol_violation",
    }
}

fn raw_outcome_label(o: &RawOutcome) -> &'static str {
    match o {
        RawOutcome::Success { .. } => "success",
        RawOutcome::Failure(_) => "failure",
        RawOutcome::TimedOut => "timeout",
        RawOutcome::Disconnected => "disconnected",
        RawOutcome::Protocol(_) => "protocol_violation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::worker::{default_thumbnail_limits, resolve_psd_worker_path};

    /// 合成最小合法 RGB 8-bit raw PSD（与 worker 解码单测同结构）。
    fn make_rgb_psd(w: u32, h: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"8BPS");
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&8u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u16.to_be_bytes());
        for ch in 0..3u32 {
            for y in 0..h {
                for x in 0..w {
                    b.push(match ch {
                        0 => {
                            if w > 1 {
                                (x * 255 / (w - 1)) as u8
                            } else {
                                200
                            }
                        }
                        1 => {
                            if h > 1 {
                                (y * 255 / (h - 1)) as u8
                            } else {
                                120
                            }
                        }
                        _ => 128,
                    });
                }
            }
        }
        b
    }

    fn test_spec(path: std::path::PathBuf) -> WorkerSpec {
        WorkerSpec {
            exe_path: path,
            expected_worker_id: "psd-worker".into(),
            required_capabilities: vec!["thumbnail".into()],
        }
    }

    fn test_cfg() -> WorkerConfig {
        WorkerConfig {
            handshake_timeout: Duration::from_secs(5),
            host_version: "0.1.0".into(),
            max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        }
    }

    // ── R2-5 确定性单测:kill/reap/Drop/shutdown 簿记(经 ChildHandle 假件,零真进程) ──

    use exotic_protocol::{
        FailureBody, Frame, FrameType, ProtocolError, ReadyBody, WorkerErrorCode,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 记录 kill/wait 次数的假子进程;try_wait 行为由闭包脚本化。
    struct FakeChild {
        kills: Arc<AtomicUsize>,
        waits: Arc<AtomicUsize>,
        try_wait_fn: Box<dyn FnMut() -> std::io::Result<Option<()>> + Send>,
    }
    impl ChildHandle for FakeChild {
        fn kill(&mut self) -> std::io::Result<()> {
            self.kills.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn wait(&mut self) -> std::io::Result<()> {
            self.waits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn try_wait(&mut self) -> std::io::Result<Option<()>> {
            (self.try_wait_fn)()
        }
    }

    /// 共享捕获写端:conn 写出的帧字节落入 Vec,供断言「是否发过 Shutdown/Request」。
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct SupParts {
        kills: Arc<AtomicUsize>,
        waits: Arc<AtomicUsize>,
        written: Arc<Mutex<Vec<u8>>>,
        /// 持有发送端防 channel 变 clean-EOF;测试可预灌响应帧。
        tx: crossbeam_channel::Sender<Result<Frame, ProtocolError>>,
    }

    /// 以字面量组装含私有字段的 supervisor(tests 是子模块,可直构)。
    /// 线程句柄给立即退出的空线程,join_threads 不会挂起。
    fn make_sup(
        alive: bool,
        try_wait_fn: Box<dyn FnMut() -> std::io::Result<Option<()>> + Send>,
    ) -> (WorkerSupervisor, SupParts) {
        let kills = Arc::new(AtomicUsize::new(0));
        let waits = Arc::new(AtomicUsize::new(0));
        let written = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = crossbeam_channel::unbounded();
        let ready = ReadyBody {
            worker_id: "psd-worker".into(),
            worker_version: "0.0-test".into(),
            protocol_version: 0, // from_parts 跳过握手,不校验
            capabilities: vec!["thumbnail".into()],
            max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        };
        let sup = WorkerSupervisor {
            child: Box::new(FakeChild {
                kills: Arc::clone(&kills),
                waits: Arc::clone(&waits),
                try_wait_fn,
            }),
            conn: WorkerConn::from_parts(Box::new(SharedWriter(Arc::clone(&written))), rx, ready),
            stderr_ring: Arc::new(Mutex::new(VecDeque::new())),
            reader_handle: Some(std::thread::spawn(|| {})),
            stderr_handle: Some(std::thread::spawn(|| {})),
            alive,
            worker_version: "0.0-test".into(),
            session: None,
        };
        (
            sup,
            SupParts {
                kills,
                waits,
                written,
                tx,
            },
        )
    }

    fn thumb_req() -> RequestBody {
        RequestBody::Thumbnail {
            item_id: 7,
            source_path: "x.psd".into(),
            target_long_edge: 480,
            input_fingerprint: "fp".into(),
        }
    }

    fn never_exits() -> Box<dyn FnMut() -> std::io::Result<Option<()>> + Send> {
        Box::new(|| Ok(None))
    }

    #[test]
    fn drop_releases_reader_blocked_by_full_frame_queue() {
        let (mut sup, _parts) = make_sup(true, never_exits());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let ready = ReadyBody {
            worker_id: "psd-worker".into(),
            worker_version: "test".into(),
            protocol_version: 0,
            capabilities: vec!["thumbnail".into()],
            max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        };
        sup.conn = WorkerConn::from_parts(Box::new(std::io::sink()), rx, ready);
        let frame = || Ok(Frame::control(FrameType::Shutdown, 0, &serde_json::json!({})).unwrap());
        tx.send(frame()).unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        sup.reader_handle.take().unwrap().join().unwrap();
        sup.reader_handle = Some(std::thread::spawn(move || {
            assert!(tx.send(frame()).is_err());
        }));
        std::thread::spawn(move || {
            drop(sup);
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("满队列不能阻止 Drop 回收");
    }

    #[test]
    fn dead_instance_guard_skips_conn_and_child() {
        let (mut sup, parts) = make_sup(false, never_exits());
        let out = sup.run_thumbnail(
            &thumb_req(),
            &default_thumbnail_limits(),
            Duration::from_secs(1),
            &|| false,
        );
        assert!(matches!(out, TaskOutcome::Disconnected));
        assert!(
            parts.written.lock().unwrap().is_empty(),
            "不得向坏管道写请求"
        );
        assert_eq!(parts.kills.load(Ordering::SeqCst), 0);
        assert_eq!(parts.waits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn cancelled_disconnect_kills_once_and_stays_dead() {
        let (mut sup, parts) = make_sup(true, never_exits());
        // cancelled=||true 在任何等待前命中(worker.rs 取消轮询),零 sleep 确定性触发 kill 路径。
        let out = sup.run_thumbnail(
            &thumb_req(),
            &default_thumbnail_limits(),
            Duration::from_secs(30),
            &|| true,
        );
        assert!(matches!(out, TaskOutcome::Disconnected));
        assert_eq!(parts.kills.load(Ordering::SeqCst), 1);
        assert_eq!(parts.waits.load(Ordering::SeqCst), 1);
        assert!(!sup.is_alive());
        assert!(
            sup.reader_handle.is_none(),
            "kill_and_reap 应已 join 读取线程"
        );
        assert!(sup.stderr_handle.is_none());

        // 幂等:再跑直接 Disconnected、不二次 kill;Drop 因 alive=false 也只 join 不复杀。
        let out2 = sup.run_thumbnail(
            &thumb_req(),
            &default_thumbnail_limits(),
            Duration::from_secs(1),
            &|| false,
        );
        assert!(matches!(out2, TaskOutcome::Disconnected));
        drop(sup);
        assert_eq!(
            parts.kills.load(Ordering::SeqCst),
            1,
            "全生命周期恰好 1 次 kill"
        );
        assert_eq!(parts.waits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn timeout_kills_and_marks_dead() {
        let (mut sup, parts) = make_sup(true, never_exits());
        let out = sup.run_thumbnail(
            &thumb_req(),
            &default_thumbnail_limits(),
            Duration::from_millis(50),
            &|| false,
        );
        assert!(matches!(out, TaskOutcome::TimedOut));
        assert_eq!(parts.kills.load(Ordering::SeqCst), 1);
        assert!(!sup.is_alive());
        drop(parts.tx); // 显式:发送端存活至此,超时非 EOF 所致
    }

    #[test]
    fn failure_outcome_keeps_instance_alive() {
        // Failure 与 Success 走同一「数据类结果不杀进程」匹配臂;Success 需真 WebP 过
        // Host 校验(worker.rs 单测已覆盖协议侧),此处以 Failure 锁 supervisor 的保活语义。
        let (mut sup, parts) = make_sup(true, never_exits());
        let body = FailureBody {
            item_id: Some(7),
            input_fingerprint: Some("fp".into()),
            code: WorkerErrorCode::MalformedInput,
            retryable: false,
            message: "synthetic".into(),
        };
        // 预灌响应帧(request_id=1:from_parts 后首个分配值),recv 立即命中。
        parts
            .tx
            .send(Ok(Frame::control(FrameType::Failure, 1, &body).unwrap()))
            .unwrap();
        let out = sup.run_thumbnail(
            &thumb_req(),
            &default_thumbnail_limits(),
            Duration::from_secs(1),
            &|| false,
        );
        assert!(matches!(out, TaskOutcome::Failure(_)));
        assert_eq!(
            parts.kills.load(Ordering::SeqCst),
            0,
            "数据类失败不得杀进程"
        );
        assert!(sup.is_alive(), "实例应可复用");
    }

    #[test]
    fn shutdown_graceful_exit_sends_frame_and_never_kills() {
        let (sup, parts) = make_sup(true, Box::new(|| Ok(Some(()))));
        sup.shutdown(Duration::from_secs(5));
        // shutdown 消费 self,返回时 Drop 已跑完——断言全生命周期总数。
        assert!(
            !parts.written.lock().unwrap().is_empty(),
            "应已发送 Shutdown 帧"
        );
        assert_eq!(
            parts.kills.load(Ordering::SeqCst),
            0,
            "优雅退出不得 kill(含 Drop)"
        );
        assert_eq!(parts.waits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn shutdown_grace_expired_kills_once() {
        // grace=0:首轮 try_wait Ok(None) 即命中 deadline 分支,零 sleep 确定性。
        let (sup, parts) = make_sup(true, never_exits());
        sup.shutdown(Duration::ZERO);
        assert_eq!(parts.kills.load(Ordering::SeqCst), 1);
        assert_eq!(parts.waits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shutdown_try_wait_error_kills_immediately() {
        let (sup, parts) = make_sup(
            true,
            Box::new(|| Err(std::io::Error::other("try_wait failed"))),
        );
        sup.shutdown(Duration::from_secs(5));
        assert_eq!(
            parts.kills.load(Ordering::SeqCst),
            1,
            "try_wait Err 不等宽限期"
        );
        assert_eq!(parts.waits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shutdown_on_dead_instance_only_joins() {
        let (mut sup, parts) = make_sup(true, never_exits());
        let _ = sup.run_thumbnail(
            &thumb_req(),
            &default_thumbnail_limits(),
            Duration::from_secs(30),
            &|| true,
        ); // 先经 kill 路径致死
        parts.written.lock().unwrap().clear();
        sup.shutdown(Duration::from_secs(3));
        assert!(
            parts.written.lock().unwrap().is_empty(),
            "已死实例不得再发 Shutdown 帧"
        );
        assert_eq!(parts.kills.load(Ordering::SeqCst), 1, "kill 计数不变");
    }

    #[test]
    fn drop_alive_kills_once_dead_only_joins() {
        let (sup, parts) = make_sup(true, never_exits());
        drop(sup);
        assert_eq!(
            parts.kills.load(Ordering::SeqCst),
            1,
            "活实例 Drop 兜底 kill"
        );
        assert_eq!(parts.waits.load(Ordering::SeqCst), 1);

        let (sup2, parts2) = make_sup(false, never_exits());
        drop(sup2);
        assert_eq!(
            parts2.kills.load(Ordering::SeqCst),
            0,
            "死实例 Drop 只 join"
        );
    }

    #[test]
    fn stderr_ring_keeps_last_64k() {
        let data: Vec<u8> = (0..100_000usize).map(|i| (i % 251) as u8).collect();
        let ring = Arc::new(Mutex::new(VecDeque::new()));
        spawn_stderr_drain(
            std::io::Cursor::new(data.clone()),
            Arc::clone(&ring),
            STDERR_RING_CAP,
            "test".to_string(),
        )
        .join()
        .unwrap();
        let g = ring.lock().unwrap();
        assert_eq!(g.len(), STDERR_RING_CAP);
        let kept: Vec<u8> = g.iter().copied().collect();
        assert_eq!(
            &kept[..],
            &data[100_000 - STDERR_RING_CAP..],
            "保留的是最后 64 KiB"
        );
        drop(g);

        let small = b"short stderr".to_vec();
        let ring2 = Arc::new(Mutex::new(VecDeque::new()));
        spawn_stderr_drain(
            std::io::Cursor::new(small.clone()),
            Arc::clone(&ring2),
            STDERR_RING_CAP,
            "test".to_string(),
        )
        .join()
        .unwrap();
        assert_eq!(
            ring2.lock().unwrap().iter().copied().collect::<Vec<u8>>(),
            small
        );
    }

    #[test]
    fn outcome_label_maps_all_variants() {
        assert_eq!(
            outcome_label(&TaskOutcome::Success {
                width: 1,
                height: 1,
                mime: "image/webp".into(),
                blob: Vec::new(),
                thumbhash: Vec::new(),
            }),
            "success"
        );
        assert_eq!(
            outcome_label(&TaskOutcome::Failure(FailureBody {
                item_id: Some(1),
                input_fingerprint: Some("f".into()),
                code: WorkerErrorCode::IoError,
                retryable: true,
                message: String::new(),
            })),
            "failure"
        );
        assert_eq!(outcome_label(&TaskOutcome::TimedOut), "timeout");
        assert_eq!(outcome_label(&TaskOutcome::Disconnected), "disconnected");
        assert_eq!(
            outcome_label(&TaskOutcome::Protocol(String::new())),
            "protocol_violation"
        );
    }

    // ── T15 会话生命周期(D3 §4②⑤ + §5 T_t:SessionInit 超时→kill→session 清零)────────

    use exotic_protocol::{
        ModelDescriptor, ModelHandle, ModelProfileSnapshot, ModelRole, SessionReadyBody,
        SuccessBody,
    };

    fn session_init_req(session_id: u64) -> RequestBody {
        RequestBody::SessionInit {
            session_id,
            models: vec![ModelDescriptor {
                role: ModelRole::ImageEncoder,
                handle: ModelHandle::Path("C:/models/img.onnx".into()),
                len: 3,
                sha256: "ab".repeat(32),
                model_id: None,
            }],
            model_profile: ModelProfileSnapshot {
                arch_id: "cn-clip-vit-b16".into(),
                image_file: "img.onnx".into(),
                text_file: "txt.onnx".into(),
                batch_size: 16,
                face_profile_id: Some("yunet-sface".into()),
            },
            models_root: "C:/models".into(),
            ai_cache_dir: "C:/cache/ai".into(),
            image_provider: "cpu".into(),
        }
    }

    fn session_ready_success(request_id: u64) -> Frame {
        let body = SuccessBody {
            session: Some(SessionReadyBody {
                embed_dim: 512,
                face_embed_dim: Some(128),
                caps: vec!["embedding".into(), "face_detect_embed".into()],
                provider: Some("directml".into()),
                gpu_name: Some("Mock GPU".into()),
            }),
            ..Default::default()
        };
        Frame::control(FrameType::Success, request_id, &body).unwrap()
    }

    #[test]
    fn init_session_success_stores_descriptor() {
        let (mut sup, parts) = make_sup(true, never_exits());
        parts.tx.send(Ok(session_ready_success(1))).unwrap();
        let out = sup.init_session(&session_init_req(7), Duration::from_secs(1), &|| false);
        assert!(matches!(out, RawOutcome::Success { .. }));
        let desc = sup.session().expect("成功后应记录会话快照");
        assert_eq!(desc.session_id, 7);
        assert_eq!(desc.arch_id, "cn-clip-vit-b16");
        assert_eq!(desc.batch_size, 16, "快照须记 SessionInit 的批上限(W3)");
        assert_eq!(desc.embed_dim, 512);
        assert_eq!(desc.face_embed_dim, Some(128));
        assert_eq!(desc.face_profile_id.as_deref(), Some("yunet-sface"));
        assert!(sup.is_alive());
        assert_eq!(parts.kills.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn init_session_failure_keeps_alive_without_session() {
        let (mut sup, parts) = make_sup(true, never_exits());
        let fail = FailureBody {
            item_id: None,
            input_fingerprint: None,
            code: WorkerErrorCode::ModelLoadFailed,
            retryable: false,
            message: "sha 不符".into(),
        };
        parts
            .tx
            .send(Ok(Frame::control(FrameType::Failure, 1, &fail).unwrap()))
            .unwrap();
        let out = sup.init_session(&session_init_req(7), Duration::from_secs(1), &|| false);
        assert!(matches!(out, RawOutcome::Failure(_)));
        assert!(sup.session().is_none(), "失败不得记录会话");
        assert!(sup.is_alive(), "数据类失败不杀进程");
        assert_eq!(parts.kills.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn init_session_timeout_kills_and_clears_session() {
        // D3 §5 T_t 链路的 supervisor 半边:SessionInit 超时 → kill_and_reap → session 清零。
        // (池层重建 + 重 Init 由 T17 派发器驱动,重建后 session=None 天然成立。)
        let (mut sup, parts) = make_sup(true, never_exits());
        let out = sup.init_session(&session_init_req(7), Duration::from_millis(50), &|| false);
        assert!(matches!(out, RawOutcome::TimedOut));
        assert!(!sup.is_alive());
        assert!(sup.session().is_none());
        assert_eq!(parts.kills.load(Ordering::SeqCst), 1);
        drop(parts.tx); // 显式:发送端存活至此,超时非 EOF 所致
    }

    #[test]
    fn init_session_success_without_ready_body_is_protocol_kill() {
        let (mut sup, parts) = make_sup(true, never_exits());
        // Success 但缺 session 应答体 → 协议违例,kill 回收。
        parts
            .tx
            .send(Ok(Frame::control(
                FrameType::Success,
                1,
                &SuccessBody::default(),
            )
            .unwrap()))
            .unwrap();
        let out = sup.init_session(&session_init_req(7), Duration::from_secs(1), &|| false);
        assert!(matches!(out, RawOutcome::Protocol(_)));
        assert!(!sup.is_alive());
        assert!(sup.session().is_none());
        assert_eq!(parts.kills.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn init_session_rejects_non_session_request_without_side_effects() {
        let (mut sup, parts) = make_sup(true, never_exits());
        let out = sup.init_session(&thumb_req(), Duration::from_secs(1), &|| false);
        assert!(matches!(out, RawOutcome::Protocol(_)));
        assert!(sup.is_alive(), "host 调用错误不得殃及 worker");
        assert!(parts.written.lock().unwrap().is_empty(), "不得发出任何帧");
    }

    #[test]
    fn close_session_idempotent_and_clears_descriptor() {
        // 无会话:幂等成功、零帧。
        let (mut sup, parts) = make_sup(true, never_exits());
        let out = sup.close_session(Duration::from_secs(1), &|| false);
        assert!(matches!(out, RawOutcome::Success { .. }));
        assert!(parts.written.lock().unwrap().is_empty());

        // 有会话:发 SessionClose(request_id=1),Success 后快照清空、实例保活。
        parts
            .tx
            .send(Ok(Frame::control(
                FrameType::Success,
                1,
                &SuccessBody::default(),
            )
            .unwrap()))
            .unwrap();
        sup.session = Some(SessionDescriptor {
            session_id: 9,
            arch_id: "cn-clip-vit-b16".into(),
            image_file: "img.onnx".into(),
            face_profile_id: None,
            batch_size: 16,
            embed_dim: 512,
            face_embed_dim: None,
            caps: vec!["embedding".into()],
            provider: None,
            gpu_name: None,
        });
        let out = sup.close_session(Duration::from_secs(1), &|| false);
        assert!(matches!(out, RawOutcome::Success { .. }));
        assert!(sup.session().is_none());
        assert!(sup.is_alive());
        assert!(
            !parts.written.lock().unwrap().is_empty(),
            "应已发送 SessionClose 帧"
        );
    }

    /// 真实子进程冒烟测试：显式启用时必须提供 `EXOTIC_PSD_WORKER_PATH`，缺条件即失败。
    ///
    /// 构建并运行：
    ///   cargo build --release -p psd-worker
    ///   EXOTIC_PSD_WORKER_PATH=target/release/psd-worker.exe \
    ///     cargo test -p scrollery --lib exotic::supervisor::tests::real_worker -- --ignored --nocapture
    #[test]
    #[ignore = "需要 EXOTIC_PSD_WORKER_PATH 指向真实 PSD worker"]
    fn real_worker_thumbnail_and_shutdown() {
        let path = resolve_psd_worker_path().expect("必须设置 EXOTIC_PSD_WORKER_PATH");
        // 写一张合成 PSD 到临时文件。
        let dir = tempfile::tempdir().unwrap();
        let psd_path = dir.path().join("synthetic.psd");
        std::fs::write(&psd_path, make_rgb_psd(300, 200)).unwrap();

        let mut sup = WorkerSupervisor::spawn(&test_spec(path), &test_cfg()).expect("spawn+握手");
        assert!(sup.is_alive());
        assert!(
            !sup.worker_version().is_empty(),
            "握手应拿到 worker_version"
        );

        let req = RequestBody::Thumbnail {
            item_id: 42,
            source_path: psd_path.to_string_lossy().into_owned(),
            target_long_edge: 480,
            input_fingerprint: "fp-real".into(),
        };
        let out = sup.run_thumbnail(
            &req,
            &default_thumbnail_limits(),
            Duration::from_secs(15),
            &|| false,
        );
        match out {
            TaskOutcome::Success { width, height, .. } => {
                assert_eq!((width, height), (300, 200));
            }
            other => panic!("期望 Success，得到 {}", outcome_label(&other)),
        }

        sup.shutdown(Duration::from_secs(3));
    }

    /// 同一真实 RAW 进程先处理畸形输入、再处理缺文件；数据失败不杀进程，最后正常回收。
    #[test]
    #[ignore = "需要 EXOTIC_RAW_WORKER_PATH 指向真实 RAW worker"]
    fn real_raw_worker_failures_and_shutdown() {
        let exe =
            std::env::var_os("EXOTIC_RAW_WORKER_PATH").expect("必须设置 EXOTIC_RAW_WORKER_PATH");
        let spec = WorkerSpec {
            exe_path: exe.into(),
            expected_worker_id: "raw-worker".into(),
            required_capabilities: vec!["thumbnail".into()],
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("malformed.dng"), [0xabu8; 64]).unwrap();
        let start = Instant::now();
        let mut sup = WorkerSupervisor::spawn(&spec, &test_cfg()).expect("RAW spawn+握手");
        let handshake = start.elapsed();
        assert!(!sup.worker_version().is_empty());
        for (file, expected, retryable) in [
            ("malformed.dng", WorkerErrorCode::MalformedInput, false),
            ("missing.dng", WorkerErrorCode::IoError, true),
        ] {
            let req = RequestBody::Thumbnail {
                item_id: 42,
                source_path: dir.path().join(file).to_string_lossy().into_owned(),
                target_long_edge: 480,
                input_fingerprint: file.into(),
            };
            let out = sup.run_thumbnail(
                &req,
                &default_thumbnail_limits(),
                Duration::from_secs(5),
                &|| false,
            );
            match out {
                TaskOutcome::Failure(body) => {
                    assert_eq!(body.code, expected);
                    assert_eq!(body.retryable, retryable);
                    assert_eq!(body.item_id, Some(42));
                    assert_eq!(body.input_fingerprint.as_deref(), Some(file));
                }
                other => panic!("期望 RAW Failure，得到 {}", outcome_label(&other)),
            }
            assert!(sup.is_alive(), "数据失败后应继续复用进程");
        }
        let version = sup.worker_version().to_string();
        sup.shutdown(Duration::from_secs(3));
        eprintln!(
            "RAW version={} handshake_ms={:.2} failure_smoke_total_ms={:.2}",
            version,
            handshake.as_secs_f64() * 1000.0,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
