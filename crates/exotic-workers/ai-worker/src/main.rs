// crates/exotic-workers/ai-worker/src/main.rs
//! AI 推理 Worker 主循环(Part4-T15;合并单 worker:CLIP 嵌入 + 人脸检测/嵌入,
//! T9.5 VRAM 实测支持合并,正式拍板随 T20)。
//!
//! 契约(承接 psd-worker 样板 + v2 会话族 op):
//!   - **stdout 只走协议帧**;日志只写 stderr。
//!   - 进程握手快而恒定(Hello→Ready,5s 档):**不承载模型加载**(D3 §2)。模型加载 =
//!     显式 SessionInit 请求(host 侧对其配 300s 档),SessionReady = 其 Success 应答。
//!   - host 主导会话生命周期(SessionClose 显式卸载);本端兜底**空闲自杀 timer**:
//!     收到最后一帧后 300s 无活动 `exit(0)`,防 host 失联留 VRAM 僵尸(D3 §4④——
//!     读线程 + channel `recv_timeout` 实现,阻塞式 stdin 读无法带超时)。
//!   - 严格串行:一次一请求(host Supervisor 不变量),无并发状态。
//!   - 每请求顶层 `catch_unwind`:panic 回 internal_error 并主动退出,不带病服务。

mod batch;
mod ocr;
mod session;

use std::io::{BufReader, BufWriter, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use exotic_protocol::{
    capability, read_frame, write_frame, FailureBody, Frame, FrameType, OcrSessionReadyBody,
    ProgressBody, ProtocolError, ReadyBody, RequestBody, SessionReadyBody, SuccessBody,
    WorkerErrorCode, MAX_BLOB_LEN, PROTOCOL_VERSION,
};
use scrollery_ai_core::engine::OrtPreflightError;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

use session::{InitError, SessionState};

/// Worker 稳定标识(Host 握手校验;installer manifest 的 worker_id 与此一致)。
const WORKER_ID: &str = "ai-worker";
/// Worker 版本(进指纹/展示/升级失效,R11)。
const WORKER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 空闲自杀阈值:最后一帧后无活动即退出(D3 §4④ 兜底,host 失联不留 VRAM 僵尸)。
/// 计时仅覆盖「等下一帧」——在途推理(SessionInit 可达分钟级)不在等待态,不会误杀。
const IDLE_SELF_EXIT: Duration = Duration::from_secs(300);
/// SessionInit 期间无阶段事件时的心跳节拍(加固批 A-2):宿主静默限时按其数倍设定,
/// 装载再慢也不会被误杀——只要本进程还活着且装载线程未退。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// ORT 运行时(dylib+环境)装载的短 watchdog:正常 <1s;超此预算即断定运行时本体
/// 卡死(死路径/System32 旧版/损坏 DLL),立即回 OrtRuntimeInitTimeout——2026-07-10/11
/// 夜事故的「无限静默」由此变为秒级可判别错误。
const ORT_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(60);

// stderr 结构化日志行(日志能力重构线 阶段 3 · W3,D-313/D-314):替换原 `[ai-worker] {msg}`
// 纯文本 eprintln,统一走 exotic_protocol::WorkerLogLine 单行 JSON——与下方 [`WorkerLogFormat`]
// 收编的 ai-core 内部 tracing 事件共用同一 schema,host 侧 supervisor 只需一套解析器。
// 定级原则(逐点判断,不一刀切):正常生命周期/握手成功/会话就绪/空闲自杀兜底=info、
// 非致命异常(worker 继续服务)=warn、致命错误(握手失败/协议损坏/panic,均以非零码退出进程)=error、
// 高频请求回执=debug。逐点定级清单见
// docs/worklogs/2026-07-21-span埋点与worker日志汇入/findings.md「worker 线」节。
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

/// tracing 事件字段收集器(镜像 src-tauri/src/logging.rs::FieldCollector 姿态):
/// `message` 提为 [`exotic_protocol::WorkerLogLine::msg`]、其余字段进 `fields`。
/// 本端无需 operation_id/信封字段(那是 host 侧 EnvelopeFormat 的职责),只服务
/// 单行 JSON stderr 协议。
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
/// [`exotic_protocol::WorkerLogLine`] 形状单行 JSON,收编进与手写 `log_*` 系列相同的 schema——
/// 此前两种形状(文本 fmt 行 + `[ai-worker] {msg}`)混流,是本次统一的直接动因。
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
        // 与 WorkerLogLine::emit 同样的失败降级(不 panic):序列化失败时至少留可读消息文本。
        match serde_json::to_string(&line) {
            Ok(json) => writeln!(writer, "{json}"),
            Err(_) => writeln!(writer, "{}", line.msg),
        }
    }
}

type FrameResult = Result<Frame, ProtocolError>;

fn main() {
    // tracing 订阅者接 stderr(2026-07-11 加固:此前 ai-core 的全部装载/降级日志被静默
    // 丢弃——SessionInit 卡死时 host 侧 stderr 环形缓冲一片空白,盲调无据)。stdout 是
    // 协议帧通道,日志只允许走 stderr(与 log_* 同通道,supervisor 环形缓冲统一收集)。
    // event_format 换 WorkerLogFormat(D-314):ai-core 内部日志与手写 log_* 系列同一 schema。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .event_format(WorkerLogFormat)
        .with_writer(std::io::stderr)
        .init();
    // 启动指纹:ORT 动态库解析是历史事故点(clean 后四件套蒸发→落 System32 1.17 无限
    // 阻塞),把关键环境一次性打出,杀实例时可从 stderr 尾部直读。
    log_info(format!(
        "启动:exe={:?} ORT_DYLIB_PATH={:?}",
        std::env::current_exe().ok(),
        std::env::var("ORT_DYLIB_PATH").ok()
    ));
    // A-1 启动自检(纯路径判定,不触 ort):死配置在 spawn 时即见于 stderr,
    // 真正的快败在 SessionInit 的 stage-0 preflight(回典型错误码给宿主)。
    match scrollery_ai_core::engine::resolve_ort_dylib() {
        Ok((p, src)) => log_info(format!("ORT 动态库解析通过:{}({src})", p.display())),
        Err(e) => log_warn(format!("⚠️ ORT 动态库预检不过:{e} → SessionInit 将快败")),
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

    // ── 握手:等 Hello → 回 Ready(空闲上限同样约束握手:host 失联即自杀)──────────────
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
        capabilities: vec![
            capability::EMBEDDING.to_string(),
            capability::FACE_DETECT_EMBED.to_string(),
            capability::OCR_TEXT.to_string(),
        ],
        max_blob_len: MAX_BLOB_LEN,
    };
    if let Err(e) = send(
        &mut writer,
        &Frame::control(FrameType::Ready, 0, &ready).unwrap(),
    ) {
        log_error(format!("发送 Ready 失败:{e} → 退出"));
        std::process::exit(2);
    }

    // ── 主循环:严格串行,一帧一请求 ─────────────────────────────────────────────────
    let mut sess: Option<SessionState> = None;
    // OCR 会话独立槽(D-OCR-1):与 CLIP `sess` 各自生命周期,互不干扰(main.rs 头注)。
    let mut ocr_sess: Option<ocr::OcrSessionState> = None;
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
                // 收帧回执(诊断锚点):帧若在传输层丢失,此行不会出现在 stderr。
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
                let is_init = matches!(req, RequestBody::SessionInit { .. });
                // 每请求顶层 catch_unwind:panic → 回 internal_error 后主动退出(§3.5)。
                // SessionInit 走流式处理(装载线程 + Progress 帧,自行写终态);其余同步。
                let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if is_init {
                        handle_session_init_streaming(req, req_id, &mut sess, &mut writer);
                        None
                    } else {
                        Some(handle_request(req, req_id, &mut sess, &mut ocr_sess))
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

/// 从帧 channel 取下一帧;EOF/损坏/空闲超时/读线程消失均返回 None(日志已写,调用方退出)。
/// 协议损坏用非零码退出使 host 可诊断,其余路径正常退出。
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
                "空闲 {}s 无帧 → 自杀兜底(host 失联不留 VRAM 僵尸,D3 §4)",
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

/// SessionInit 流式处理(2026-07-11 加固批 A-2):装载跑独立线程,主线程把阶段事件
/// 转成 Progress 帧、无事件时按 [`HEARTBEAT_INTERVAL`] 发心跳,终态自行写 Success/Failure。
/// 宿主收到任意帧即重置静默计时——「300s 总限时 vs 单段 600s」的预算倒挂由此消解。
fn handle_session_init_streaming<W: Write>(
    req: RequestBody,
    request_id: u64,
    sess: &mut Option<SessionState>,
    writer: &mut W,
) {
    let RequestBody::SessionInit {
        session_id,
        models,
        model_profile,
        models_root,
        ai_cache_dir,
        image_provider,
    } = req
    else {
        // 调用方已用 matches! 判过 op;此臂只防御签名误用。
        let fail = FailureBody {
            item_id: None,
            input_fingerprint: None,
            code: WorkerErrorCode::InternalError,
            retryable: false,
            message: "内部路由错误:非 SessionInit 进入流式处理".to_string(),
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
            "SessionInit 前存在旧会话 {} → 先卸载(切换语义)",
            old.session_id
        ));
        drop(old);
    }

    /// 装载线程 → 主线程的事件。Done 装箱:SessionState 含整套 Session 池,避免大枚举。
    enum LoadEvent {
        Stage(String),
        Done(Box<Result<SessionState, InitError>>),
    }

    let started = Instant::now();
    let (etx, erx) = std::sync::mpsc::channel::<LoadEvent>();

    std::thread::spawn(move || {
        let stage_tx = etx.clone();
        let stage = move |s: &str| {
            let _ = stage_tx.send(LoadEvent::Stage(s.to_string()));
        };
        // stage-0:ORT 运行时 preflight(短 watchdog)。死路径/System32 旧版在此快败,
        // 不再让后续每段装载各撞一次 600s。
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
        let progress_tx = etx.clone();
        let progress_cb = move |ev: &str| {
            let _ = progress_tx.send(LoadEvent::Stage(ev.to_string()));
        };
        let init = session::validate_and_resolve(
            session_id,
            &models,
            &model_profile,
            &models_root,
            &ai_cache_dir,
            &image_provider,
        )
        .and_then(|resolved| session::load_with_progress(resolved, Some(&progress_cb)));
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
            let body = SuccessBody {
                session: Some(SessionReadyBody {
                    embed_dim: state.profile.embed_dim as u32,
                    face_embed_dim: state.face_profile.as_ref().map(|fp| fp.embed_dim as u32),
                    caps: session_caps(&state),
                    // provider 回声(T16 additive):探测/回退结果只有本端知道,
                    // host 借此写回 ai_provider/ai_gpu_name 配置(状态栏显示)。
                    provider: Some(state.pool.provider.as_str().to_string()),
                    gpu_name: Some(state.pool.gpu_name.clone()),
                }),
                ..Default::default()
            };
            log_info(format!(
                "会话 {} 就绪:arch={} face={:?} provider={}(耗时 {:.1}s)",
                session_id,
                state.profile.id,
                state.face_profile.as_ref().map(|f| f.id.clone()),
                state.pool.provider.as_str(),
                started.elapsed().as_secs_f32()
            ));
            *sess = Some(state);
            Frame::control(FrameType::Success, request_id, &body).unwrap()
        }
        Some(Err((code, message))) => {
            // 阶段化升级:引擎侧把超时降级为「池未就绪」的笼统失败,此处按记录的
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
            log_warn(format!("SessionInit 失败[{}]:{message}", code.as_str()));
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
            // 装载线程 panic(通道断开):按既有 panic 纪律回 internal_error 后退出,不带病服务。
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

/// 处理一个非 SessionInit 的 Request → 返回应发送的 Success/Failure 帧。
/// 会话状态经 `sess` 串行流转;CLIP SessionInit 走 [`handle_session_init_streaming`]。
/// `ocr_sess` 是与 `sess` 并存的独立槽(D-OCR-1):OcrSessionInit/Close/Batch 三臂
/// 只读写 `ocr_sess`,CLIP 三臂只读写 `sess`,双槽互不干扰(边界情况10)。OCR 会话装载
/// 是秒级 CPU 小模型(D-OCR-2),同步处理、不走流式 Progress。
fn handle_request(
    req: RequestBody,
    request_id: u64,
    sess: &mut Option<SessionState>,
    ocr_sess: &mut Option<ocr::OcrSessionState>,
) -> Frame {
    match req {
        RequestBody::SessionInit { .. } => {
            // 路由防御:调用方保证 SessionInit 不进本函数。
            let fail = FailureBody {
                item_id: None,
                input_fingerprint: None,
                code: WorkerErrorCode::InternalError,
                retryable: false,
                message: "内部路由错误:SessionInit 应走流式处理".to_string(),
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
        RequestBody::SessionClose { session_id } => {
            // 幂等:无会话/错 id 也回 Success(host 只关心「之后没有会话」这一后置条件)。
            match sess.take() {
                Some(s) if s.session_id == session_id => {
                    log_info(format!("会话 {session_id} 已卸载"));
                }
                Some(s) => {
                    log_warn(format!(
                        "SessionClose id 不符:{} != 当前 {} → 仍卸载当前会话",
                        session_id, s.session_id
                    ));
                }
                None => log_info(format!("SessionClose {session_id}:无在载会话(幂等)")),
            }
            Frame::control(FrameType::Success, request_id, &SuccessBody::default()).unwrap()
        }
        RequestBody::EmbedBatch { items } => match sess.as_ref() {
            Some(s) => batch::handle_embed(s, request_id, &items),
            None => session_expired(request_id),
        },
        RequestBody::FaceDetectEmbed {
            items,
            det_score_thresh,
        } => match sess.as_ref() {
            Some(s) => batch::handle_face(s, request_id, &items, det_score_thresh),
            None => session_expired(request_id),
        },
        RequestBody::EncodeText { texts } => match sess.as_ref() {
            Some(s) => batch::handle_encode_text(s, request_id, &texts),
            None => session_expired(request_id),
        },
        RequestBody::OcrSessionInit {
            session_id,
            models,
            ocr_profile_id,
            models_root,
        } => {
            // stage-0 preflight(同 CLIP SessionInit 复用同一常量/watchdog):死路径/损坏
            // dylib 在此快败,不再让 OCR 装载撞无限静默。
            if let Err(e) = scrollery_ai_core::engine::preflight_ort_runtime(ORT_PREFLIGHT_TIMEOUT)
            {
                let code = match &e {
                    OrtPreflightError::DylibUnavailable(_) => WorkerErrorCode::OrtDylibUnavailable,
                    OrtPreflightError::RuntimeInitTimeout(_)
                    | OrtPreflightError::RuntimeInitFailed(_) => {
                        WorkerErrorCode::OrtRuntimeInitTimeout
                    }
                };
                let fail = FailureBody {
                    item_id: None,
                    input_fingerprint: None,
                    code,
                    retryable: code.default_retryable(),
                    message: e.to_string(),
                };
                return Frame::control(FrameType::Failure, request_id, &fail).unwrap();
            }
            // host 主导切换语义:已有 OCR 会话时先卸旧(同 CLIP SessionInit 姿态)。
            if let Some(old) = ocr_sess.take() {
                log_info(format!(
                    "OcrSessionInit 前存在旧 OCR 会话 {} → 先卸载(切换语义)",
                    old.session_id
                ));
                drop(old);
            }
            let init_result =
                ocr::validate_ocr_init(session_id, &models, &ocr_profile_id, &models_root)
                    .and_then(|(profile, root)| {
                        scrollery_ai_core::ocr::OcrEngine::init(&root, &profile).map_err(|e| {
                            (
                                WorkerErrorCode::ModelLoadFailed,
                                format!("OCR 引擎初始化失败:{e}"),
                            )
                        })
                    });
            match init_result {
                Ok(engine) => {
                    let body = SuccessBody {
                        ocr_session: Some(OcrSessionReadyBody {
                            caps: vec![capability::OCR_TEXT.to_string()],
                        }),
                        ..Default::default()
                    };
                    log_info(format!("OCR 会话 {session_id} 就绪"));
                    *ocr_sess = Some(ocr::OcrSessionState { session_id, engine });
                    Frame::control(FrameType::Success, request_id, &body).unwrap()
                }
                Err((code, message)) => {
                    log_warn(format!("OcrSessionInit 失败[{}]:{message}", code.as_str()));
                    let fail = FailureBody {
                        item_id: None,
                        input_fingerprint: None,
                        code,
                        retryable: code.default_retryable(),
                        message,
                    };
                    Frame::control(FrameType::Failure, request_id, &fail).unwrap()
                }
            }
        }
        RequestBody::OcrSessionClose { session_id } => {
            // 幂等语义同 CLIP SessionClose:host 只关心「之后没有 OCR 会话」这一后置条件。
            match ocr_sess.take() {
                Some(s) if s.session_id == session_id => {
                    log_info(format!("OCR 会话 {session_id} 已卸载"));
                }
                Some(s) => {
                    log_warn(format!(
                        "OcrSessionClose id 不符:{} != 当前 {} → 仍卸载当前 OCR 会话",
                        session_id, s.session_id
                    ));
                }
                None => log_info(format!(
                    "OcrSessionClose {session_id}:无在载 OCR 会话(幂等)"
                )),
            }
            Frame::control(FrameType::Success, request_id, &SuccessBody::default()).unwrap()
        }
        RequestBody::OcrBatch { items } => match ocr_sess.as_ref() {
            Some(s) => ocr::handle_ocr_batch(s, request_id, &items),
            None => session_expired(request_id),
        },
        // 本 worker 不做缩略图/元数据;host 按能力路由不会派发,防御性兜底稳定错误码。
        // 影像增强三件套由独立 enhance-worker 处理,ai-worker 不支持。
        other @ (RequestBody::Thumbnail { .. }
        | RequestBody::Metadata { .. }
        | RequestBody::EnhanceSessionInit { .. }
        | RequestBody::EnhanceSessionClose { .. }
        | RequestBody::EnhanceRun { .. }
        // 视频格式扩展子系统由独立 video-worker 处理,ai-worker 不支持。
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
                message: "该 op 未实现(ai-worker 仅 embedding/face_detect_embed)".to_string(),
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
    }
}

/// 会话未加载 → SessionExpired(retryable:host 重发 SessionInit 后重派,G6)。
fn session_expired(request_id: u64) -> Frame {
    let fail = FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::SessionExpired,
        retryable: true,
        message: "会话未加载或已卸载".to_string(),
    };
    Frame::control(FrameType::Failure, request_id, &fail).unwrap()
}

/// 本会话实际可服务的能力(host 据此派活;与 Ready.capabilities 的「静态支持范围」区分)。
fn session_caps(state: &SessionState) -> Vec<String> {
    let mut caps = vec![capability::EMBEDDING.to_string()];
    if state.face_profile.is_some() {
        caps.push(capability::FACE_DETECT_EMBED.to_string());
    }
    caps
}

/// 写一帧并 flush(保证 Host 立即可读)。
fn send<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    write_frame(w, frame)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 边界情况10:CLIP SessionClose(或未载)不得误伤 OCR 会话槽,反之亦然——两个
    /// `Option` 各自独立(D-OCR-1)。需真实 OCR 模型(`OCR_MODELS_DIR`),CI 默认不跑
    /// (与 `scrollery-ai-core` 的 `ocr_golden`/`ocr_bench` 同款 characterization 姿态);
    /// 本机手测清单第⑥项(construction-plan §4)同覆盖此路径的真机版本。
    #[test]
    #[ignore = "需 OCR_MODELS_DIR(真实 OCR 模型),本机手跑"]
    fn clip_session_close_does_not_touch_ocr_slot() {
        let Some(dir) = std::env::var_os("OCR_MODELS_DIR").map(std::path::PathBuf::from) else {
            eprintln!("[clip_session_close_does_not_touch_ocr_slot] SKIP: OCR_MODELS_DIR 未设置");
            return;
        };
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let engine = match scrollery_ai_core::ocr::OcrEngine::init(&dir, &profile) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[clip_session_close_does_not_touch_ocr_slot] SKIP: 引擎加载失败:{e}");
                return;
            }
        };

        let mut sess: Option<SessionState> = None; // CLIP 会话本就未载(幂等路径亦须验证)
        let mut ocr_sess = Some(ocr::OcrSessionState {
            session_id: 7,
            engine,
        });

        // CLIP SessionClose:不得触碰 ocr_sess。
        let out = handle_request(
            RequestBody::SessionClose { session_id: 1 },
            1,
            &mut sess,
            &mut ocr_sess,
        );
        assert!(matches!(out.frame_type, FrameType::Success));
        assert!(ocr_sess.is_some(), "CLIP SessionClose 不得清空 OCR 会话槽");

        // OcrBatch 仍可服务(空批走通即证会话独立;真实识别覆盖见本机手测清单)。
        let out2 = handle_request(
            RequestBody::OcrBatch { items: vec![] },
            2,
            &mut sess,
            &mut ocr_sess,
        );
        assert!(
            matches!(out2.frame_type, FrameType::Success),
            "CLIP 会话关闭/未载不应影响 OcrBatch 可服务性"
        );

        // OcrSessionClose 反向验证:不得触碰 sess(仍为 None)。
        let out3 = handle_request(
            RequestBody::OcrSessionClose { session_id: 7 },
            3,
            &mut sess,
            &mut ocr_sess,
        );
        assert!(matches!(out3.frame_type, FrameType::Success));
        assert!(ocr_sess.is_none(), "OcrSessionClose 后 OCR 会话槽应清空");
        assert!(sess.is_none(), "OcrSessionClose 不应影响 CLIP 会话槽");
    }
}
