// src-tauri/src/exotic/worker.rs
//! 冷门格式插件 · Worker 进程规格、子进程创建与「传输无关」的连接抽象（v3 Part2 §3.4-3.7）。
//!
//! 分层（为可测性）：
//!   - [`WorkerSpec`] / [`spawn_worker_process`]：定位 + 以**低优先级、隐藏窗口、管道 stdio** 创建子进程。
//!     低优先级是 exotic 让步阶梯的 OS 软让步底层手段（R1：主进程线程 sleep 无法令子进程让出 CPU）。
//!   - [`WorkerConn`]：**只依赖 `Write` + frame `Receiver`** 的协议状态机（握手 / run_task / 输出验证）。
//!     不持 `Child`，因此可用内存管道 + mock worker 线程做确定性单测（协议/恶意 Worker/超时/错序）。
//!   - 真正的进程生命周期（kill/wait/join 线程）在 [`super::supervisor`]。
//!
//! Host **不信任** Worker 返回值（§3.7）：用独立解码器验证 WebP 实际尺寸、声明与实际一致、像素上限，
//! 并核对 request_id / item_id / fingerprint。任一不符 → terminal `invalid_worker_output`，丢弃 blob。

use std::io::{BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use exotic_protocol::{
    read_frame, write_frame, FailureBody, Frame, FrameType, HelloBody, ProgressBody, ReadyBody,
    RequestBody, SuccessBody,
};

// U-P3(2026-07-16):outcome 类型与纯校验器拆至同级模块,此处 re-export 保住既有
// `exotic::worker::{RawOutcome, TaskOutcome, validate_*, ...}` 引用路径(消费方零迁移)。
pub use super::outcome::*;
pub use super::validate::*;

/// 在途任务取消轮询周期：`run_thumbnail` 等待响应期间每隔此间隔检查取消标志，
/// 使 stop/App 退出能及时让 Supervisor kill 在途 Worker（v3.1 §4.1）。
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// 收到 Progress 帧(v3)即重置静默限时后,单请求仍受此**总上界**约束(防御:worker 心跳
/// 线程活着但装载线程死锁的病态组合不至于让宿主永久等待)。取值 ≥ worker 最坏串行装载
/// (5 池 × 单段 600s 后备上界远超实际;正常装载秒级、心跳只是在途证明)。
/// 不发 Progress 的 op(thumbnail/embed 等)静默限时=总限时,行为与 v2 完全一致。
pub(crate) const PROGRESS_TOTAL_CAP: Duration = Duration::from_secs(3600);

/// 定位 PSD Worker 可执行文件（**仅测试**）。
///
/// Part2（dev/test）旧入口：经环境变量 `EXOTIC_PSD_WORKER_PATH` 注入已构建的 worker 二进制路径、**不验签**。
/// 生产路径已由 [`crate::exotic::installer::resolve_worker_path`]（验签 + hash 复核，§3.6）替代；coordinator 只调后者。
/// 🔒 本函数整体 `#[cfg(test)]`：Release/普通 debug app 构建中**不存在**，杜绝经环境变量加载未验签 worker
/// 的信任链击穿（SEC-02，对齐 D8「Release 不得有验签旁路」红线）。仅 cargo test 的真实 worker 冒烟用例编入。
#[cfg(test)]
pub fn resolve_psd_worker_path() -> Option<PathBuf> {
    std::env::var_os("EXOTIC_PSD_WORKER_PATH").map(PathBuf::from)
}

/// Worker 进程规格 + Host 对其的期望（握手校验）。
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// 可执行文件路径。
    pub exe_path: PathBuf,
    /// 期望的 worker_id（握手时 ReadyBody.worker_id 必须匹配）。
    pub expected_worker_id: String,
    /// Host 需要的能力（握手时 ReadyBody.capabilities 必须包含全部）。
    pub required_capabilities: Vec<String>,
}

/// Supervisor/Conn 配置常量。
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// 握手超时。
    pub handshake_timeout: Duration,
    /// Host 语义版本（写入 Hello）。
    pub host_version: String,
    /// Host 能接收的最大 blob（写入 Hello；与协议 MAX_BLOB_LEN 取小）。
    pub max_blob_len: u32,
}

/// Host 对缩略图输出的硬上限（§3.7）。
#[derive(Debug, Clone)]
pub struct WorkerLimits {
    /// blob 字节上限。
    pub max_blob_len: u32,
    /// 输出总像素上限。
    pub max_output_pixels: u64,
    /// 请求档位之上允许的长边误差（缩放取整/比例换算的容差）。
    pub long_edge_tolerance: u32,
}

/// 以低优先级、隐藏窗口、管道 stdio 创建 Worker 子进程（§3.6 / R1）。
pub fn spawn_worker_process(spec: &WorkerSpec) -> std::io::Result<Child> {
    let mut cmd = Command::new(&spec.exe_path);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_low_priority(&mut cmd);
    cmd.spawn()
}

/// 平台相关的低优先级 + 隐藏窗口设置。**始终生效**的 OS 软让步（R1 第 1 层）。
#[cfg(windows)]
fn apply_low_priority(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // BELOW_NORMAL_PRIORITY_CLASS(0x4000)：低于普通优先级；CREATE_NO_WINDOW(0x0800_0000)：无控制台窗口。
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);
}

/// macOS/Linux：Part2 在 Windows 落地与验证；此处记录降级（未降优先级），
/// 由 Part4 跨平台发布按实测接入 `nice`/QoS utility（R1 要求记录创建失败与降级行为）。
#[cfg(not(windows))]
fn apply_low_priority(_cmd: &mut Command) {
    tracing::debug!(
        "非 Windows：Worker 低优先级未接入（Part4 实测 nice/QoS）；本次以普通优先级创建"
    );
}

/// 启动一个把 `r` 的协议帧持续读入 `tx` 的线程（Supervisor 与测试共用）。
/// 读到错误（含干净 EOF）后发送该错误并结束——下游据此判定断开/违例。
pub fn spawn_frame_reader<R: Read + Send + 'static>(
    r: R,
    tx: Sender<Result<Frame, exotic_protocol::ProtocolError>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(r);
        loop {
            match read_frame(&mut reader) {
                Ok(f) => {
                    if tx.send(Ok(f)).is_err() {
                        break; // 下游已走
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break; // 读到错误/EOF 即终止本线程
                }
            }
        }
    })
}

/// 「传输无关」的 Worker 连接：握手后用 `run_thumbnail` 跑任务。不持 `Child`，便于单测。
pub struct WorkerConn {
    writer: Box<dyn Write + Send>,
    rx: Receiver<Result<Frame, exotic_protocol::ProtocolError>>,
    ready: ReadyBody,
    next_request_id: u64,
}

impl WorkerConn {
    /// Supervisor join 前断开接收端，解除有界 reader 的发送等待。
    pub(super) fn close_reader(&mut self) {
        self.rx = crossbeam_channel::bounded(0).1;
    }

    /// 握手：写 Hello → 等 Ready（带超时）→ 校验 worker_id/protocol/capabilities。
    pub fn handshake(
        mut writer: Box<dyn Write + Send>,
        rx: Receiver<Result<Frame, exotic_protocol::ProtocolError>>,
        spec: &WorkerSpec,
        cfg: &WorkerConfig,
    ) -> Result<WorkerConn, String> {
        let hello = HelloBody {
            host_version: cfg.host_version.clone(),
            protocol_version: exotic_protocol::PROTOCOL_VERSION,
            max_blob_len: cfg.max_blob_len.min(exotic_protocol::MAX_BLOB_LEN),
        };
        let frame = Frame::control(FrameType::Hello, 0, &hello).map_err(|e| e.to_string())?;
        write_frame(&mut writer, &frame).map_err(|e| format!("写 Hello 失败：{e}"))?;
        writer
            .flush()
            .map_err(|e| format!("flush Hello 失败：{e}"))?;

        let ready_frame = match rx.recv_timeout(cfg.handshake_timeout) {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => return Err(format!("握手读取失败：{e}")),
            Err(RecvTimeoutError::Timeout) => return Err("握手超时（未收到 Ready）".into()),
            Err(RecvTimeoutError::Disconnected) => return Err("握手时连接断开".into()),
        };
        if ready_frame.frame_type != FrameType::Ready {
            return Err(format!("握手期望 Ready，收到 {:?}", ready_frame.frame_type));
        }
        let ready: ReadyBody = ready_frame
            .parse_json()
            .map_err(|e| format!("Ready 解析失败：{e}"))?;

        if ready.protocol_version != exotic_protocol::PROTOCOL_VERSION {
            return Err(format!(
                "协议版本不兼容：worker {} != host {}",
                ready.protocol_version,
                exotic_protocol::PROTOCOL_VERSION
            ));
        }
        if ready.worker_id != spec.expected_worker_id {
            return Err(format!(
                "worker_id 不符：{} != {}",
                ready.worker_id, spec.expected_worker_id
            ));
        }
        for need in &spec.required_capabilities {
            if !ready.capabilities.iter().any(|c| c == need) {
                return Err(format!("缺少能力：{need}"));
            }
        }

        Ok(WorkerConn {
            writer,
            rx,
            ready,
            next_request_id: 1,
        })
    }

    /// 测试用：以现成部件构造连接（跳过握手）。
    #[cfg(test)]
    pub fn from_parts(
        writer: Box<dyn Write + Send>,
        rx: Receiver<Result<Frame, exotic_protocol::ProtocolError>>,
        ready: ReadyBody,
    ) -> WorkerConn {
        WorkerConn {
            writer,
            rx,
            ready,
            next_request_id: 1,
        }
    }

    pub fn worker_version(&self) -> &str {
        &self.ready.worker_version
    }

    /// 分配单调递增 request_id。
    fn alloc_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    /// 发送任意 op 的请求并等待响应(T15 泛化,D3 §4①)。返回**未经 op 校验**的
    /// [`RawOutcome`];`cancelled` 在等待期间被周期轮询,返回 true 即放弃等待返回
    /// `Disconnected`(上层 kill 在途 Worker,v3.1 §4.1)。
    pub fn run_request(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        // 缺省沿用 PROGRESS_TOTAL_CAP(3600s):既有 op(thumbnail/embed/face/session 等)行为
        // 逐字节不变——回归锚 `progress_resets_silence_deadline_and_stale_ignored` 直调本方法保绿。
        self.run_request_capped(req, timeout, PROGRESS_TOTAL_CAP, cancelled)
    }

    /// [`Self::run_request`] 的 per-op 总上界参数化版(视频格式扩展子系统 design.md §2.4)。
    /// `timeout` 仍是**静默限时**(收 Progress 即重置);`total_cap` 为不可重置的**总上界**
    /// (心跳线程活着但装载线程死锁的病态组合的最后防线)。video transcode 传
    /// `max(2h, 探测时长 × 6)`(上限 12h);其余 op 由 [`Self::run_request`] 传缺省 3600s。
    ///
    /// 无观察者的薄委托(既有 AI/enhance/OCR 全部调用点走这条路,逻辑一行不动)——
    /// 真正实现见 [`Self::run_request_observed`]。
    pub fn run_request_capped(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        self.run_request_observed(req, timeout, total_cap, cancelled, None)
    }

    /// 同 [`Self::run_request`],但在收到本请求的 Progress 帧时额外回调 `on_progress`
    /// (per-tile 进度接线,深审 b:增强 EnhanceRun 借此把 tile 心跳上报队列状态)。
    /// 视频线合并后统一委托到 [`Self::run_request_observed`] 并取缺省 total_cap——
    /// 既有语义逐字节不变,仅换实现载体(观察者形参名不同,行为一致)。
    pub fn run_request_with_progress(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
        on_progress: Option<&mut dyn FnMut(&ProgressBody)>,
    ) -> RawOutcome {
        self.run_request_observed(req, timeout, PROGRESS_TOTAL_CAP, cancelled, on_progress)
    }

    /// [`Self::run_request_capped`] 的进度可观察版(视频格式扩展子系统 design.md §5.3):
    /// 每收到一帧本请求的 Progress(非陈旧)即回调 `progress_observer`(若提供),
    /// 静默重置 + 日志逻辑与 [`Self::run_request_capped`] 完全一致、逐字节不变——
    /// 仅追加这一次回调,不改变任何既有分支/返回值。
    pub fn run_request_observed(
        &mut self,
        req: &RequestBody,
        timeout: Duration,
        total_cap: Duration,
        cancelled: &dyn Fn() -> bool,
        mut progress_observer: Option<&mut dyn FnMut(&ProgressBody)>,
    ) -> RawOutcome {
        let request_id = self.alloc_request_id();
        let frame = match Frame::control(FrameType::Request, request_id, req) {
            Ok(f) => f,
            Err(e) => return RawOutcome::Protocol(format!("构造 Request 失败：{e}")),
        };
        if write_frame(&mut self.writer, &frame).is_err() || self.writer.flush().is_err() {
            return RawOutcome::Disconnected;
        }

        // 可取消等待：每 CANCEL_POLL 检查一次取消标志，使 stop/App 退出能及时让 Supervisor kill
        // 在途 Worker（v3.1 §4.1：停止按取消协议终止在途，不等其自然完成；返回 Disconnected → kill）。
        // v3(加固批 A-2):`timeout` 语义 = **静默限时**——收到本请求的 Progress 帧即重置;
        // 不发 Progress 的 op 永不重置,行为与旧「总限时」逐字节相同。总上界见 `total_cap`。
        let started = Instant::now();
        let hard_deadline = started + total_cap;
        let mut deadline = started + timeout;
        let resp = loop {
            if cancelled() {
                return RawOutcome::Disconnected;
            }
            let now = Instant::now();
            if now >= deadline || now >= hard_deadline {
                return RawOutcome::TimedOut;
            }
            let remaining = deadline
                .saturating_duration_since(now)
                .min(hard_deadline.saturating_duration_since(now));
            match self.rx.recv_timeout(remaining.min(CANCEL_POLL)) {
                Ok(Ok(f)) if f.frame_type == FrameType::Progress => {
                    if f.request_id == request_id {
                        match f.parse_json::<ProgressBody>() {
                            Ok(p) => {
                                tracing::info!(
                                    "worker 进度[req={request_id}]:{}{} 已 {:.1}s",
                                    p.stage,
                                    p.detail
                                        .as_deref()
                                        .map(|d| format!("({d})"))
                                        .unwrap_or_default(),
                                    p.elapsed_ms as f64 / 1000.0
                                );
                                // per-tile 进度回调(深审 b + 视频 §5.3):除重置静默计时外通知上层。
                                if let Some(obs) = progress_observer.as_deref_mut() {
                                    obs(&p);
                                }
                            }
                            Err(e) => tracing::warn!("Progress 帧 JSON 无效(忽略):{e}"),
                        }
                        deadline = Instant::now() + timeout; // 静默计时重置
                    } else {
                        // 迟到的陈旧进度(前一请求残留)不判违例——终态帧错配才是硬错误。
                        tracing::warn!(
                            "忽略陈旧 Progress:req {} != 在途 {request_id}",
                            f.request_id
                        );
                    }
                    continue;
                }
                Ok(Ok(f)) => break f,
                Ok(Err(e)) => {
                    return if e.is_clean_eof() {
                        RawOutcome::Disconnected
                    } else {
                        // Worker 发出损坏帧 → 协议违例（Supervisor 会 kill）。
                        RawOutcome::Protocol(format!("响应帧损坏：{e}"))
                    };
                }
                Err(RecvTimeoutError::Timeout) => continue, // 轮询：再查取消 / 静默或总超时
                Err(RecvTimeoutError::Disconnected) => return RawOutcome::Disconnected,
            }
        };

        // request_id 必须匹配当前在途请求（每 Supervisor 同时只一个请求）。
        if resp.request_id != request_id {
            return RawOutcome::Protocol(format!(
                "request_id 错配：{} != {}",
                resp.request_id, request_id
            ));
        }

        match resp.frame_type {
            FrameType::Success => match resp.parse_json::<SuccessBody>() {
                Ok(body) => RawOutcome::Success {
                    body,
                    blob: resp.blob,
                },
                Err(e) => RawOutcome::Protocol(format!("Success 解析失败：{e}")),
            },
            FrameType::Failure => match resp.parse_json::<FailureBody>() {
                Ok(body) => RawOutcome::Failure(body),
                Err(e) => RawOutcome::Protocol(format!("Failure 解析失败：{e}")),
            },
            other => RawOutcome::Protocol(format!("意外帧类型：{other:?}")),
        }
    }

    /// 发送一个缩略图请求并等待响应，验证后返回 [`TaskOutcome`]。
    /// `req` 必须是 `RequestBody::Thumbnail`；`target_long_edge` 须为吸附后档位（R5）。
    /// T15 起 = [`Self::run_request`] + thumbnail 专属输出校验(WebP 复核/尺寸/上限)。
    pub fn run_thumbnail(
        &mut self,
        req: &RequestBody,
        limits: &WorkerLimits,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> TaskOutcome {
        match self.run_request(req, timeout, cancelled) {
            RawOutcome::Success { body, blob } => {
                match validate_thumbnail_output(req, &body, &blob, limits) {
                    Ok(out) => TaskOutcome::Success {
                        width: out.width,
                        height: out.height,
                        mime: out.mime,
                        blob,
                        thumbhash: out.thumbhash,
                    },
                    Err(reason) => TaskOutcome::Protocol(reason),
                }
            }
            RawOutcome::Failure(body) => {
                // 即便是失败响应也要核对 id/fingerprint，防错序串扰。
                if body.item_id != req.item_id()
                    || body.input_fingerprint.as_deref() != req.input_fingerprint()
                {
                    return TaskOutcome::Protocol("Failure 的 item/fingerprint 错配".into());
                }
                TaskOutcome::Failure(body)
            }
            RawOutcome::TimedOut => TaskOutcome::TimedOut,
            RawOutcome::Disconnected => TaskOutcome::Disconnected,
            RawOutcome::Protocol(p) => TaskOutcome::Protocol(p),
        }
    }

    /// 尽力发送 Shutdown（关闭流程用；失败忽略）。
    pub fn send_shutdown(&mut self) {
        if let Ok(f) = Frame::control(FrameType::Shutdown, 0, &serde_json::json!({})) {
            let _ = write_frame(&mut self.writer, &f);
            let _ = self.writer.flush();
        }
    }
}

// validate_thumbnail_output/default_thumbnail_limits/validate_embed_batch_output/
// validate_face_batch_output/validate_encode_text_output 及 EmbedItemOutcome/
// FaceItemOutcome/RawOutcome/TaskOutcome 已拆至 `super::validate`/`super::outcome`
// (U-P3),经文件头 `pub use` 保旧路径;其测试随被测符号迁走。

#[cfg(test)]
mod tests {
    use super::*;
    use exotic_protocol::{WorkerErrorCode, PROTOCOL_VERSION};
    use std::io::Cursor;

    fn thumb_req(item_id: i64, fp: &str, tier: u32) -> RequestBody {
        RequestBody::Thumbnail {
            item_id,
            source_path: "x.psd".into(),
            target_long_edge: tier,
            input_fingerprint: fp.into(),
        }
    }

    /// 生成一张真实 WebP（用 image crate 编码一张纯色图）。
    fn make_webp(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut buf = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut buf))
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    fn limits() -> WorkerLimits {
        default_thumbnail_limits()
    }

    // validate_* 的纯函数测试已随被测符号迁 `super::super::validate`(U-P3)。

    // ── WorkerConn 端到端（内存管道 + mock worker 线程，无真实子进程）──────────────────

    /// 一个内存单向管道：写端 + 读端共享有界缓冲（用 crossbeam 字节通道模拟）。
    /// 这里直接用 Vec→Cursor 不便于流式；改用 os 无关的简单实现：std::sync::mpsc 传字节块 + Read 适配。
    struct PipeWriter(Sender<Vec<u8>>);
    impl Write for PipeWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .send(buf.to_vec())
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    struct PipeReader {
        rx: Receiver<Vec<u8>>,
        buf: std::collections::VecDeque<u8>,
    }
    impl Read for PipeReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            while self.buf.is_empty() {
                match self.rx.recv() {
                    Ok(chunk) => self.buf.extend(chunk),
                    Err(_) => return Ok(0), // 写端关闭 → EOF
                }
            }
            let n = out.len().min(self.buf.len());
            for slot in out.iter_mut().take(n) {
                *slot = self.buf.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    fn unidir() -> (PipeWriter, PipeReader) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            PipeWriter(tx),
            PipeReader {
                rx,
                buf: std::collections::VecDeque::new(),
            },
        )
    }

    /// 搭一对连接：返回（host 侧 conn，worker 侧 reader/writer）。
    /// host 写 → worker 读；worker 写 → 经 frame_reader → host rx。
    fn wired_conn() -> (WorkerConn, PipeReader, PipeWriter) {
        let (host_w, worker_r) = unidir(); // host→worker
        let (worker_w, host_r) = unidir(); // worker→host
        let (tx, rx) = crossbeam_channel::unbounded();
        spawn_frame_reader(host_r, tx);
        let ready = ReadyBody {
            worker_id: "psd-worker".into(),
            worker_version: "1.0.0".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: vec!["thumbnail".into()],
            max_blob_len: exotic_protocol::MAX_BLOB_LEN,
        };
        let conn = WorkerConn::from_parts(Box::new(host_w), rx, ready);
        (conn, worker_r, worker_w)
    }

    /// RawOutcome 未派生 Debug(大变体);测试断言用变体名。
    fn outcome_name(o: &RawOutcome) -> &'static str {
        match o {
            RawOutcome::Success { .. } => "Success",
            RawOutcome::Failure(_) => "Failure",
            RawOutcome::TimedOut => "TimedOut",
            RawOutcome::Disconnected => "Disconnected",
            RawOutcome::Protocol(_) => "Protocol",
        }
    }

    /// v3 静默限时:Progress 帧重置计时——总时长远超 `timeout` 但拍间静默不超,必须成功;
    /// 顺带覆盖「陈旧 Progress(错 req_id)只忽略不判违例」。
    #[test]
    fn progress_resets_silence_deadline_and_stale_ignored() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req_id = frame.request_id;
            let stale = Frame::control(
                FrameType::Progress,
                req_id + 999,
                &ProgressBody {
                    stage: "stale".into(),
                    detail: None,
                    elapsed_ms: 0,
                },
            )
            .unwrap();
            write_frame(&mut worker_w, &stale).unwrap();
            worker_w.flush().unwrap();
            // 4 拍进度 × 100ms 间隔 = 总时长 ~500ms,远超 400ms 静默限时;
            // 每拍间静默 100ms ≪ 400ms → 重置生效才能活到终态。
            for i in 0..4u64 {
                std::thread::sleep(Duration::from_millis(100));
                let p = Frame::control(
                    FrameType::Progress,
                    req_id,
                    &ProgressBody {
                        stage: format!("stage{i}"),
                        detail: None,
                        elapsed_ms: i * 100,
                    },
                )
                .unwrap();
                write_frame(&mut worker_w, &p).unwrap();
                worker_w.flush().unwrap();
            }
            std::thread::sleep(Duration::from_millis(100));
            let ok = Frame::control(FrameType::Success, req_id, &SuccessBody::default()).unwrap();
            write_frame(&mut worker_w, &ok).unwrap();
            worker_w.flush().unwrap();
        });
        let out = conn.run_request(
            &thumb_req(1, "fp", 480),
            Duration::from_millis(400),
            &|| false,
        );
        handle.join().unwrap();
        assert_eq!(
            outcome_name(&out),
            "Success",
            "进度帧应重置静默计时并被消费(非终态)"
        );
    }

    /// 进度观察者(视频格式扩展子系统 design.md §5.3):`run_request_observed` 对每帧本请求的
    /// Progress(非陈旧)回调一次;陈旧 Progress(错 req_id)不触发观察者——镜像上面回归锚的
    /// 陈旧过滤断言,验证观察者挂载不改变既有陈旧帧处理逻辑。
    #[test]
    fn run_request_observed_calls_observer_for_each_progress_frame() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req_id = frame.request_id;
            // 陈旧 Progress(错 req_id):不应触发观察者。
            let stale = Frame::control(
                FrameType::Progress,
                req_id + 999,
                &ProgressBody {
                    stage: "stale".into(),
                    detail: None,
                    elapsed_ms: 0,
                },
            )
            .unwrap();
            write_frame(&mut worker_w, &stale).unwrap();
            worker_w.flush().unwrap();
            for i in 0..3u64 {
                let p = Frame::control(
                    FrameType::Progress,
                    req_id,
                    &ProgressBody {
                        stage: format!("s{i}"),
                        detail: Some(format!("{}%", i * 30)),
                        elapsed_ms: i * 10,
                    },
                )
                .unwrap();
                write_frame(&mut worker_w, &p).unwrap();
                worker_w.flush().unwrap();
            }
            let ok = Frame::control(FrameType::Success, req_id, &SuccessBody::default()).unwrap();
            write_frame(&mut worker_w, &ok).unwrap();
            worker_w.flush().unwrap();
        });
        let mut seen: Vec<(String, Option<String>)> = Vec::new();
        let out = conn.run_request_observed(
            &thumb_req(1, "fp", 480),
            Duration::from_secs(2),
            Duration::from_secs(2),
            &|| false,
            Some(&mut |p: &ProgressBody| seen.push((p.stage.clone(), p.detail.clone()))),
        );
        handle.join().unwrap();
        assert_eq!(outcome_name(&out), "Success");
        assert_eq!(
            seen,
            vec![
                ("s0".to_string(), Some("0%".to_string())),
                ("s1".to_string(), Some("30%".to_string())),
                ("s2".to_string(), Some("60%".to_string())),
            ],
            "观察者应逐帧收到非陈旧 Progress,陈旧帧不计入"
        );
    }

    /// 不发 Progress 的 op:`timeout` 仍是事实上的总限时,行为与 v2 一致(回归锚)。
    #[test]
    fn silence_timeout_without_progress_unchanged() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            std::thread::sleep(Duration::from_millis(900));
            let ok = Frame::control(
                FrameType::Success,
                frame.request_id,
                &SuccessBody::default(),
            )
            .unwrap();
            let _ = write_frame(&mut worker_w, &ok);
        });
        let out = conn.run_request(
            &thumb_req(1, "fp", 480),
            Duration::from_millis(200),
            &|| false,
        );
        assert_eq!(outcome_name(&out), "TimedOut");
        handle.join().unwrap();
    }

    /// 视频格式扩展 §2.4:显式 `total_cap` 生效——Progress 不断重置静默计时,但总上界到点
    /// 仍强制 TimedOut(心跳活着装载死锁的病态组合的最后防线)。静默限时给足(2s,永不触发),
    /// 唯一能终止等待的是 200ms 的总上界。
    #[test]
    fn total_cap_enforced_despite_continuous_progress() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req_id = frame.request_id;
            // 每 30ms 一帧 Progress、永不发终态:静默计时被反复重置,只有 total_cap 能终止。
            for i in 0..40u64 {
                std::thread::sleep(Duration::from_millis(30));
                let p = Frame::control(
                    FrameType::Progress,
                    req_id,
                    &ProgressBody {
                        stage: format!("s{i}"),
                        detail: None,
                        elapsed_ms: i * 30,
                    },
                )
                .unwrap();
                if write_frame(&mut worker_w, &p).is_err() {
                    break; // host 已放弃等待(命中 total_cap)
                }
                let _ = worker_w.flush();
            }
        });
        let started = Instant::now();
        let out = conn.run_request_capped(
            &thumb_req(1, "fp", 480),
            Duration::from_secs(2),     // 静默限时给足,永不触发
            Duration::from_millis(200), // 总上界:唯一能终止等待者
            &|| false,
        );
        let elapsed = started.elapsed();
        assert_eq!(outcome_name(&out), "TimedOut", "总上界到点须强制 TimedOut");
        assert!(
            elapsed < Duration::from_secs(1),
            "应在 ~200ms total_cap 处超时,而非等满 2s 静默限时:{elapsed:?}"
        );
        let _ = handle.join();
    }

    #[test]
    fn run_thumbnail_success_roundtrip() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        // mock worker：读一个 Request，回 Success + 真 WebP。
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req: RequestBody = frame.parse_json().unwrap();
            let webp = make_webp(480, 240);
            let body = SuccessBody {
                item_id: req.item_id(),
                input_fingerprint: req.input_fingerprint().map(String::from),
                mime: Some("image/webp".into()),
                width: Some(480),
                height: Some(240),
                ..Default::default()
            };
            let resp = Frame::with_blob(FrameType::Success, frame.request_id, &body, webp).unwrap();
            write_frame(&mut worker_w, &resp).unwrap();
            worker_w.flush().unwrap();
        });
        let req = thumb_req(7, "fp", 480);
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_secs(5), &|| false);
        handle.join().unwrap();
        match out {
            TaskOutcome::Success { width, height, .. } => assert_eq!((width, height), (480, 240)),
            _ => panic!("期望 Success"),
        }
    }

    #[test]
    fn run_thumbnail_timeout_when_worker_silent() {
        let (mut conn, _worker_r, _worker_w) = wired_conn();
        // worker 不回复（持有读端但不读不写）。
        let req = thumb_req(7, "fp", 480);
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_millis(150), &|| false);
        assert!(matches!(out, TaskOutcome::TimedOut));
    }

    #[test]
    fn run_thumbnail_cancelled_returns_disconnected_fast() {
        let (mut conn, _worker_r, _worker_w) = wired_conn();
        // worker 静默；cancelled 立即 true → 不等满 timeout，快速返回 Disconnected（stop 终止在途）。
        let req = thumb_req(7, "fp", 480);
        let start = std::time::Instant::now();
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_secs(30), &|| true);
        assert!(matches!(out, TaskOutcome::Disconnected));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "取消应快速返回，不等满 30s timeout"
        );
    }

    #[test]
    fn run_thumbnail_disconnect_when_worker_exits() {
        let (mut conn, worker_r, worker_w) = wired_conn();
        // worker 立即关闭两端 → host 读到 EOF。
        drop(worker_r);
        drop(worker_w);
        let req = thumb_req(7, "fp", 480);
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_secs(2), &|| false);
        assert!(matches!(out, TaskOutcome::Disconnected));
    }

    #[test]
    fn run_thumbnail_request_id_mismatch_is_protocol() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req: RequestBody = frame.parse_json().unwrap();
            let body = SuccessBody {
                item_id: req.item_id(),
                input_fingerprint: req.input_fingerprint().map(String::from),
                mime: Some("image/webp".into()),
                width: Some(10),
                height: Some(10),
                ..Default::default()
            };
            // 故意用错的 request_id。
            let resp = Frame::with_blob(
                FrameType::Success,
                frame.request_id + 99,
                &body,
                make_webp(10, 10),
            )
            .unwrap();
            write_frame(&mut worker_w, &resp).unwrap();
            worker_w.flush().unwrap();
        });
        let req = thumb_req(7, "fp", 480);
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_secs(5), &|| false);
        handle.join().unwrap();
        assert!(matches!(out, TaskOutcome::Protocol(_)));
    }

    #[test]
    fn run_thumbnail_failure_passthrough() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req: RequestBody = frame.parse_json().unwrap();
            let body = FailureBody {
                item_id: req.item_id(),
                input_fingerprint: req.input_fingerprint().map(String::from),
                code: WorkerErrorCode::UnsupportedVariant,
                retryable: false,
                message: "cmyk".into(),
            };
            let resp = Frame::control(FrameType::Failure, frame.request_id, &body).unwrap();
            write_frame(&mut worker_w, &resp).unwrap();
            worker_w.flush().unwrap();
        });
        let req = thumb_req(7, "fp", 480);
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_secs(5), &|| false);
        handle.join().unwrap();
        match out {
            TaskOutcome::Failure(b) => assert_eq!(b.code, WorkerErrorCode::UnsupportedVariant),
            _ => panic!("期望 Failure"),
        }
    }

    #[test]
    fn run_thumbnail_invalid_webp_output_is_protocol() {
        let (mut conn, mut worker_r, mut worker_w) = wired_conn();
        let handle = std::thread::spawn(move || {
            let frame = read_frame(&mut worker_r).unwrap();
            let req: RequestBody = frame.parse_json().unwrap();
            let body = SuccessBody {
                item_id: req.item_id(),
                input_fingerprint: req.input_fingerprint().map(String::from),
                mime: Some("image/webp".into()),
                width: Some(10),
                height: Some(10),
                ..Default::default()
            };
            // blob 不是合法 WebP。
            let resp = Frame::with_blob(FrameType::Success, frame.request_id, &body, vec![1, 2, 3])
                .unwrap();
            write_frame(&mut worker_w, &resp).unwrap();
            worker_w.flush().unwrap();
        });
        let req = thumb_req(7, "fp", 480);
        let out = conn.run_thumbnail(&req, &limits(), Duration::from_secs(5), &|| false);
        handle.join().unwrap();
        assert!(matches!(out, TaskOutcome::Protocol(_)));
    }
}
