// crates/exotic-workers/enhance-worker/src/main.rs
//! 影像增强 Worker 主循环(降噪/超分子系统 design.md §A):独立进程,承接降噪/去伪影/
//! 超分推理。契约照 ai-worker 样板 + enhance 会话族 op:
//!   - **stdout 只走协议帧**;日志只写 stderr(单行 JSON WorkerLogLine,D-313/D-314)。
//!   - 进程握手快而恒定(Hello→Ready):**不承载模型加载**。模型加载 = 显式
//!     EnhanceSessionInit 请求,加载期流式 Progress 心跳(照 CLIP SessionInit 先例)。
//!   - host 主导会话生命周期(EnhanceSessionClose 显式卸载);本端兜底**空闲自杀 300s**
//!     (host 失联不留 VRAM 僵尸)。
//!   - 严格串行:一次一请求,无并发状态。
//!   - 每请求顶层 `catch_unwind`:panic 回 internal_error 并主动退出,不带病服务。

mod run;
mod session;

use std::io::{BufReader, BufWriter, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use exotic_protocol::{
    capability, read_frame, write_frame, FailureBody, Frame, FrameType, ProgressBody,
    ProtocolError, ReadyBody, RequestBody, SuccessBody, WorkerErrorCode, MAX_BLOB_LEN,
    PROTOCOL_VERSION,
};
use scrollery_ai_core::engine::OrtPreflightError;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

use session::{EnhanceSessionState, InitError};

/// Worker 稳定标识(Host 握手校验;installer manifest 的 worker_id 与此一致)。
const WORKER_ID: &str = "enhance-worker";
/// Worker 版本(进指纹/展示/升级失效)。
const WORKER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 空闲自杀阈值:最后一帧后无活动即退出(host 失联不留 VRAM 僵尸)。
/// 计时仅覆盖「等下一帧」——在途加载/推理不在等待态,不会误杀。
const IDLE_SELF_EXIT: Duration = Duration::from_secs(300);
/// EnhanceSessionInit 期间无阶段事件时的心跳节拍:宿主静默限时按其数倍设定,
/// 装载再慢也不会被误杀——只要本进程还活着且装载线程未退。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// ORT 运行时(dylib+环境)装载的短 watchdog:超此预算即断定运行时本体卡死,
/// 立即回 OrtRuntimeInitTimeout(同 ai-worker 姿态)。
const ORT_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(60);

// stderr 结构化日志行(D-313/D-314):统一走 exotic_protocol::WorkerLogLine 单行 JSON。
fn log_info(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("info", msg, serde_json::Map::new());
}
fn log_warn(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("warn", msg, serde_json::Map::new());
}
fn log_error(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("error", msg, serde_json::Map::new());
}
fn log_debug(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("debug", msg, serde_json::Map::new());
}

/// tracing 事件字段收集器(镜像 ai-worker::TracingFieldCollector):`message` 提为
/// [`exotic_protocol::WorkerLogLine::msg`],其余字段进 `fields`。
#[derive(Default)]
struct TracingFieldCollector {
    message: Option<String>,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl TracingFieldCollector {
    fn record_value(&mut self, field: &Field, value: serde_json::Value) {
        if field.name() == "message" {
            self.message = Some(match value {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            });
        } else {
            self.fields.insert(field.name().to_string(), value);
        }
    }
}

impl Visit for TracingFieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record_value(field, serde_json::Value::String(format!("{value:?}")));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, serde_json::Value::String(value.to_string()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_value(field, serde_json::Value::Bool(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_value(field, serde_json::Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_value(field, serde_json::Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_value(field, serde_json::Value::from(value));
    }
}

/// 自定义 FormatEvent(D-314):把 ai-core 内部 tracing 事件(装载/降级日志等)格式化为
/// [`exotic_protocol::WorkerLogLine`] 形状单行 JSON,与手写 `log_*` 系列共用同一 schema。
struct WorkerLogFormat;

impl<S, N> FormatEvent<S, N> for WorkerLogFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let mut visitor = TracingFieldCollector::default();
        event.record(&mut visitor);
        let meta = event.metadata();
        let line = exotic_protocol::WorkerLogLine {
            lvl: meta.level().to_string().to_lowercase(),
            msg: visitor.message.unwrap_or_default(),
            fields: visitor.fields,
        };
        match serde_json::to_string(&line) {
            Ok(json) => writeln!(writer, "{json}"),
            Err(_) => writeln!(writer, "{}", line.msg),
        }
    }
}

type FrameResult = Result<Frame, ProtocolError>;

fn main() {
    // tracing 订阅者接 stderr(stdout 是协议帧通道,日志只允许走 stderr)。
    // event_format 换 WorkerLogFormat:ai-core 内部日志与手写 log_* 系列同一 schema。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .event_format(WorkerLogFormat)
        .with_writer(std::io::stderr)
        .init();
    log_info(format!(
        "启动:exe={:?} ORT_DYLIB_PATH={:?}",
        std::env::current_exe().ok(),
        std::env::var("ORT_DYLIB_PATH").ok()
    ));
    // 启动自检(纯路径判定,不触 ort):死配置在 spawn 时即见于 stderr,真正的快败在
    // EnhanceSessionInit 的 stage-0 preflight(回典型错误码给宿主)。
    match scrollery_ai_core::engine::resolve_ort_dylib() {
        Ok((p, src)) => log_info(format!("ORT 动态库解析通过:{}({src})", p.display())),
        Err(e) => log_warn(format!(
            "⚠️ ORT 动态库预检不过:{e} → EnhanceSessionInit 将快败"
        )),
    }

    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    // stdin → 帧 channel 的读线程:主循环由此获得 recv_timeout(空闲自杀的实现前提)。
    let (tx, rx) = std::sync::mpsc::channel::<FrameResult>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(std::io::stdin().lock());
        loop {
            match read_frame(&mut reader) {
                Ok(f) => {
                    if tx.send(Ok(f)).is_err() {
                        break; // 主线程已退出
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break; // 错误/EOF 即终止读线程
                }
            }
        }
    });

    // ── 握手:等 Hello → 回 Ready ────────────────────────────────────────────────
    match recv_frame(&rx) {
        Some(f) if f.frame_type == FrameType::Hello => {
            if let Ok(hello) = f.parse_json::<exotic_protocol::HelloBody>() {
                if hello.protocol_version != PROTOCOL_VERSION {
                    log_warn(format!(
                        "Hello 协议版本 {} != 本端 {}(仍回 Ready,由 Host 决定)",
                        hello.protocol_version, PROTOCOL_VERSION
                    ));
                }
            }
        }
        Some(f) => {
            log_error(format!("握手期望 Hello,收到 {:?} → 退出", f.frame_type));
            std::process::exit(2);
        }
        None => std::process::exit(0), // EOF/超时/损坏,recv_frame 已写日志
    }

    let ready = ReadyBody {
        worker_id: WORKER_ID.to_string(),
        worker_version: WORKER_VERSION.to_string(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: vec![capability::ENHANCE.to_string()],
        max_blob_len: MAX_BLOB_LEN,
    };
    if let Err(e) = send(
        &mut writer,
        &Frame::control(FrameType::Ready, 0, &ready).unwrap(),
    ) {
        log_error(format!("发送 Ready 失败:{e} → 退出"));
        std::process::exit(2);
    }

    // ── 主循环:严格串行,一帧一请求 ─────────────────────────────────────────────
    let mut sess: Option<EnhanceSessionState> = None;
    loop {
        let Some(frame) = recv_frame(&rx) else {
            std::process::exit(0);
        };
        match frame.frame_type {
            FrameType::Shutdown => {
                log_info("收到 Shutdown → 退出(会话随进程释放)");
                std::process::exit(0);
            }
            FrameType::Request => {
                let req_id = frame.request_id;
                log_debug(format!(
                    "收到 Request req_id={req_id} json={}B blob={}B",
                    frame.json.len(),
                    frame.blob.len()
                ));
                let req: RequestBody = match frame.parse_json() {
                    Ok(r) => r,
                    Err(e) => {
                        log_warn(format!("Request JSON 解析失败:{e}"));
                        let fail = FailureBody {
                            item_id: None,
                            input_fingerprint: None,
                            code: WorkerErrorCode::InternalError,
                            retryable: false,
                            message: "bad request json".to_string(),
                        };
                        if send(
                            &mut writer,
                            &Frame::control(FrameType::Failure, req_id, &fail).unwrap(),
                        )
                        .is_err()
                        {
                            std::process::exit(0);
                        }
                        continue;
                    }
                };
                let is_init = matches!(req, RequestBody::EnhanceSessionInit { .. });
                let is_run = matches!(req, RequestBody::EnhanceRun { .. });
                // 每请求顶层 catch_unwind:panic → 回 internal_error 后主动退出。
                // EnhanceSessionInit 走流式处理(装载线程 + Progress 帧,自行写终态);
                // EnhanceRun 同步跑但内部即时发 Progress 帧,返回终态由主循环发送。
                let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if is_init {
                        handle_session_init_streaming(req, req_id, &mut sess, &mut writer);
                        None
                    } else if is_run {
                        let RequestBody::EnhanceRun {
                            source_path,
                            output_tmp_path,
                            output_format,
                            steps,
                            ..
                        } = req
                        else {
                            unreachable!("is_run 已判过 op")
                        };
                        Some(run::handle_enhance_run(
                            sess.as_ref(),
                            req_id,
                            source_path,
                            output_tmp_path,
                            output_format,
                            steps,
                            &mut writer,
                        ))
                    } else {
                        Some(handle_request(req, req_id, &mut sess))
                    }
                }));
                match handled {
                    Ok(Some(out)) => {
                        if send(&mut writer, &out).is_err() {
                            std::process::exit(0); // Host 消失
                        }
                    }
                    Ok(None) => {} // 流式路径已写完 Progress+终态帧
                    Err(_) => {
                        let fail = FailureBody {
                            item_id: None,
                            input_fingerprint: None,
                            code: WorkerErrorCode::InternalError,
                            retryable: true,
                            message: "worker panic".to_string(),
                        };
                        let _ = send(
                            &mut writer,
                            &Frame::control(FrameType::Failure, req_id, &fail).unwrap(),
                        );
                        log_error("请求 panic → 已回 internal_error,主动退出进程");
                        std::process::exit(4);
                    }
                }
            }
            other => {
                log_warn(format!("意外帧类型 {other:?} → 忽略"));
            }
        }
    }
}

/// 从帧 channel 取下一帧;EOF/损坏/空闲超时/读线程消失均返回 None(日志已写)。
fn recv_frame(rx: &Receiver<FrameResult>) -> Option<Frame> {
    match rx.recv_timeout(IDLE_SELF_EXIT) {
        Ok(Ok(f)) => Some(f),
        Ok(Err(e)) if e.is_clean_eof() => {
            log_info("stdin EOF → 退出");
            None
        }
        Ok(Err(e)) => {
            log_error(format!("读取帧失败(协议损坏):{e} → 退出"));
            std::process::exit(3);
        }
        Err(RecvTimeoutError::Timeout) => {
            log_info(format!(
                "空闲 {}s 无帧 → 自杀兜底(host 失联不留 VRAM 僵尸)",
                IDLE_SELF_EXIT.as_secs()
            ));
            None
        }
        Err(RecvTimeoutError::Disconnected) => {
            log_warn("读线程已终止 → 退出");
            None
        }
    }
}

/// EnhanceSessionInit 流式处理(照 CLIP SessionInit 先例):装载跑独立线程,主线程把
/// 阶段事件转成 Progress 帧、无事件时按 [`HEARTBEAT_INTERVAL`] 发心跳,终态自行写。
fn handle_session_init_streaming<W: Write>(
    req: RequestBody,
    request_id: u64,
    sess: &mut Option<EnhanceSessionState>,
    writer: &mut W,
) {
    let RequestBody::EnhanceSessionInit {
        session_id,
        models,
        models_root,
        work_dir,
    } = req
    else {
        let fail = FailureBody {
            item_id: None,
            input_fingerprint: None,
            code: WorkerErrorCode::InternalError,
            retryable: false,
            message: "内部路由错误:非 EnhanceSessionInit 进入流式处理".to_string(),
        };
        let _ = send(
            writer,
            &Frame::control(FrameType::Failure, request_id, &fail).unwrap(),
        );
        return;
    };

    // host 主导切换语义(先 Close 再 Init);未 Close 即 Init 按切换处理,旧会话先卸。
    if let Some(old) = sess.take() {
        log_info(format!(
            "EnhanceSessionInit 前存在旧会话 {} → 先卸载(切换语义)",
            old.session_id
        ));
        drop(old);
    }

    /// 装载线程 → 主线程的事件。Done 装箱:EnhanceSessionState 含整套 Session 池。
    enum LoadEvent {
        Stage(String),
        Done(Box<Result<EnhanceSessionState, InitError>>),
    }

    let started = Instant::now();
    let (etx, erx) = std::sync::mpsc::channel::<LoadEvent>();

    std::thread::spawn(move || {
        let stage_tx = etx.clone();
        let stage = move |s: &str| {
            let _ = stage_tx.send(LoadEvent::Stage(s.to_string()));
        };
        // stage-0:ORT 运行时 preflight(短 watchdog)。死路径/System32 旧版在此快败。
        stage("ort_runtime_init:begin");
        if let Err(e) = scrollery_ai_core::engine::preflight_ort_runtime(ORT_PREFLIGHT_TIMEOUT) {
            let code = match &e {
                OrtPreflightError::DylibUnavailable(_) => WorkerErrorCode::OrtDylibUnavailable,
                OrtPreflightError::RuntimeInitTimeout(_)
                | OrtPreflightError::RuntimeInitFailed(_) => WorkerErrorCode::OrtRuntimeInitTimeout,
            };
            let _ = etx.send(LoadEvent::Done(Box::new(Err((code, e.to_string())))));
            return;
        }
        stage("ort_runtime_init:ok");

        stage("validate:begin");
        let init = session::validate_enhance_init(session_id, &models, &work_dir, &models_root)
            .and_then(|resolved| {
                let progress_tx = etx.clone();
                let progress_cb = move |ev: &str| {
                    let _ = progress_tx.send(LoadEvent::Stage(ev.to_string()));
                };
                session::build_enhance_sessions(resolved, Some(&progress_cb))
            });
        let _ = etx.send(LoadEvent::Done(Box::new(init)));
    });

    // 主线程:事件 → Progress 帧;静默一拍 → 心跳;Done/线程崩溃 → 终态。
    let mut last_stage = "ort_runtime_init:begin".to_string();
    let mut timeout_stage: Option<String> = None;
    let mut send_progress = |stage: &str, detail: Option<&str>| -> bool {
        let body = ProgressBody {
            stage: stage.to_string(),
            detail: detail.map(str::to_string),
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        send(
            writer,
            &Frame::control(FrameType::Progress, request_id, &body).unwrap(),
        )
        .is_ok()
    };
    let result = loop {
        match erx.recv_timeout(HEARTBEAT_INTERVAL) {
            Ok(LoadEvent::Stage(ev)) => {
                // ai-core 的 `<stage>:timeout(600s)` 事件是 SessionLoadTimeout 分类判据。
                if ev.contains(":timeout(") {
                    timeout_stage = Some(ev.clone());
                }
                if !send_progress(&ev, None) {
                    std::process::exit(0); // Host 消失
                }
                last_stage = ev;
            }
            Err(RecvTimeoutError::Timeout) => {
                if !send_progress(&last_stage, Some("heartbeat")) {
                    std::process::exit(0);
                }
            }
            Ok(LoadEvent::Done(r)) => break Some(*r),
            Err(RecvTimeoutError::Disconnected) => break None,
        }
    };

    let final_frame = match result {
        Some(Ok(state)) => {
            log_info(format!(
                "增强会话 {} 就绪:provider={} 模型数={}(耗时 {:.1}s)",
                session_id,
                state.provider.as_str(),
                state.model_count,
                started.elapsed().as_secs_f32()
            ));
            *sess = Some(state);
            // 增强会话就绪无专属就绪体(design.md §G:就绪即可直接派 EnhanceRun)。
            Frame::control(FrameType::Success, request_id, &SuccessBody::default()).unwrap()
        }
        Some(Err((code, message))) => {
            // 阶段化升级:引擎把超时降级为「池未就绪」的笼统失败,此处按记录的
            // `:timeout` 事件恢复精确语义(哪段卡死一目了然)。
            let (code, message) = if code == WorkerErrorCode::ModelLoadFailed {
                if let Some(ts) = &timeout_stage {
                    (
                        WorkerErrorCode::SessionLoadTimeout,
                        format!("{message};卡死段:{ts}"),
                    )
                } else {
                    (code, message)
                }
            } else {
                (code, message)
            };
            log_warn(format!(
                "EnhanceSessionInit 失败[{}]:{message}",
                code.as_str()
            ));
            let fail = FailureBody {
                item_id: None,
                input_fingerprint: None,
                code,
                retryable: code.default_retryable(),
                message,
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
        None => {
            let fail = FailureBody {
                item_id: None,
                input_fingerprint: None,
                code: WorkerErrorCode::InternalError,
                retryable: true,
                message: "会话装载线程崩溃".to_string(),
            };
            let _ = send(
                writer,
                &Frame::control(FrameType::Failure, request_id, &fail).unwrap(),
            );
            log_error("装载线程 panic → 已回 internal_error,主动退出进程");
            std::process::exit(4);
        }
    };
    if send(writer, &final_frame).is_err() {
        std::process::exit(0);
    }
}

/// 处理一个非 SessionInit/非 Run 的 Request → 返回应发送的 Success/Failure 帧。
fn handle_request(
    req: RequestBody,
    request_id: u64,
    sess: &mut Option<EnhanceSessionState>,
) -> Frame {
    match req {
        RequestBody::EnhanceSessionClose { session_id } => {
            // 幂等:无会话/错 id 也回 Success(host 只关心「之后没有会话」这一后置条件)。
            match sess.take() {
                Some(s) if s.session_id == session_id => {
                    log_info(format!("增强会话 {session_id} 已卸载"));
                }
                Some(s) => {
                    log_warn(format!(
                        "EnhanceSessionClose id 不符:{} != 当前 {} → 仍卸载当前会话",
                        session_id, s.session_id
                    ));
                }
                None => log_info(format!(
                    "EnhanceSessionClose {session_id}:无在载会话(幂等)"
                )),
            }
            Frame::control(FrameType::Success, request_id, &SuccessBody::default()).unwrap()
        }
        // 路由防御:Init/Run 不应进本函数。
        RequestBody::EnhanceSessionInit { .. } | RequestBody::EnhanceRun { .. } => {
            let fail = FailureBody {
                item_id: None,
                input_fingerprint: None,
                code: WorkerErrorCode::InternalError,
                retryable: false,
                message: "内部路由错误:EnhanceSessionInit/EnhanceRun 应走专用路径".to_string(),
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
        // 本 worker 只做影像增强;host 按能力路由不会派发其余 op,防御性兜底稳定错误码。
        other @ (RequestBody::Thumbnail { .. }
        | RequestBody::Metadata { .. }
        | RequestBody::SessionInit { .. }
        | RequestBody::SessionClose { .. }
        | RequestBody::EmbedBatch { .. }
        | RequestBody::FaceDetectEmbed { .. }
        | RequestBody::EncodeText { .. }
        | RequestBody::OcrSessionInit { .. }
        | RequestBody::OcrSessionClose { .. }
        | RequestBody::OcrBatch { .. }
        // 视频格式扩展子系统由独立 video-worker 处理,enhance-worker 不支持。
        | RequestBody::VideoSessionInit { .. }
        | RequestBody::VideoSessionClose { .. }
        | RequestBody::VideoProbe { .. }
        | RequestBody::VideoRemux { .. }
        | RequestBody::VideoTranscode { .. }
        | RequestBody::VideoFrames { .. }) => {
            let fail = FailureBody {
                item_id: other.item_id(),
                input_fingerprint: other.input_fingerprint().map(str::to_string),
                code: WorkerErrorCode::UnsupportedVariant,
                retryable: false,
                message: "该 op 未实现(enhance-worker 仅 enhance)".to_string(),
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
    }
}

/// 写一帧并 flush(保证 Host 立即可读)。run.rs 的 per-tile Progress 亦复用本函数。
pub(crate) fn send<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    write_frame(w, frame)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhance_session_close_no_session_is_idempotent() {
        // 测试:无会话时 EnhanceSessionClose 回 Success(幂等性)
        let mut sess: Option<EnhanceSessionState> = None;

        let req = RequestBody::EnhanceSessionClose { session_id: 1 };
        let frame = handle_request(req, 42, &mut sess);

        assert_eq!(frame.frame_type, FrameType::Success);
        // sess 应仍为 None
        assert!(sess.is_none());

        // 再次 Close 同一 session_id:应仍回 Success(幂等)
        let req2 = RequestBody::EnhanceSessionClose { session_id: 1 };
        let frame2 = handle_request(req2, 43, &mut sess);
        assert_eq!(frame2.frame_type, FrameType::Success);
        assert!(sess.is_none());
    }

    // 以下两测试留待联调批:需真 EnhanceSessionState 对象,涉及 ORT 模型资源
    // #[test]
    // fn enhance_session_close_wrong_id_unloads_current() {
    //     // 测试:Close 错 session_id 时,仍卸载当前会话并回 Success
    //     // 需要构造真实 EnhanceSessionState(含 SessionPool、ORT 模型加载等),
    //     // 当前缺乏足够的测试 helper。建议用集成测试或联调真实会话生命周期。
    // }
    //
    // #[test]
    // fn enhance_session_close_matching_id_unloads() {
    //     // 测试:Close id 匹配当前会话时,卸载该会话并回 Success
    //     // 同样需要真 EnhanceSessionState 对象。
    // }
}
