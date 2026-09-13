// crates/exotic-workers/raw-worker/src/main.rs
//! RAW 缩略图 Worker 主循环（RAW 支持线 阶段 C；骨架照抄 psd-worker）。
//!
//! 契约：
//!   - **stdout 只走协议帧**；任何日志只写 stderr（Host 视 stdout 前导非帧字节为协议损坏）。
//!   - stdin 收 Hello → 回 Ready（声明 probe 通过范围）→ 循环收 Request/Shutdown。
//!   - stdin EOF / Shutdown / Host 消失 → 立即退出。
//!   - 每任务顶层 `catch_unwind`：panic 后回 `internal_error` 并**主动退出进程**，不带病继续服务。

mod decode;

use std::io::{BufReader, BufWriter, Write};

use exotic_protocol::{
    capability, read_frame, write_frame, FailureBody, Frame, FrameType, ProtocolError, ReadyBody,
    RequestBody, SuccessBody, WorkerErrorCode, MAX_BLOB_LEN, PROTOCOL_VERSION,
};

/// Worker 稳定标识（Host 握手校验）。
const WORKER_ID: &str = "raw-worker";
/// Worker 版本（进入指纹/展示/兼容）。
const WORKER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 源文件字节上限：读盘前用 metadata 拦截，避免巨文件吃满内存（→ resource_limit）。
const MAX_SOURCE_FILE_BYTES: u64 = 512 << 20;

// stderr 结构化日志行：统一走 exotic_protocol::WorkerLogLine 单行 JSON——host 侧 supervisor 逐行解析
// 转发进主 tracing/JSONL 体系。三档按调用点语义分:正常生命周期(EOF/Shutdown)=info、
// 非致命异常(单请求解析失败/意外帧,worker 继续服务)=warn、致命错误(握手失败/协议损坏/panic,
// 均以非零码退出进程)=error。与 psd-worker 同法。
fn log_info(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("info", msg, serde_json::Map::new());
}
fn log_warn(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("warn", msg, serde_json::Map::new());
}
fn log_error(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("error", msg, serde_json::Map::new());
}

fn main() {
    // 锁定 stdin/stdout 原始字节流。BufWriter 后每帧 flush，保证 Host 及时收到。
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    // ── 握手：等 Hello → 回 Ready ────────────────────────────────────────────────
    match read_frame(&mut reader) {
        Ok(f) if f.frame_type == FrameType::Hello => {
            // 解析 Hello 仅为记录；协议版本不一致由 Host 在收到 Ready 后判定（本端如实声明自己的版本）。
            if let Ok(hello) = f.parse_json::<exotic_protocol::HelloBody>() {
                if hello.protocol_version != PROTOCOL_VERSION {
                    log_warn(format!(
                        "Hello 协议版本 {} != 本端 {}（仍回 Ready，由 Host 决定）",
                        hello.protocol_version, PROTOCOL_VERSION
                    ));
                }
            }
        }
        Ok(f) => {
            log_error(format!("握手期望 Hello，收到 {:?} → 退出", f.frame_type));
            std::process::exit(2);
        }
        Err(e) if e.is_clean_eof() => std::process::exit(0),
        Err(e) => {
            log_error(format!("握手读取失败：{e} → 退出"));
            std::process::exit(2);
        }
    }

    let ready = ReadyBody {
        worker_id: WORKER_ID.to_string(),
        worker_version: WORKER_VERSION.to_string(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: vec![capability::THUMBNAIL.to_string()],
        max_blob_len: MAX_BLOB_LEN,
    };
    if let Err(e) = send(
        &mut writer,
        &Frame::control(FrameType::Ready, 0, &ready).unwrap(),
    ) {
        log_error(format!("发送 Ready 失败：{e} → 退出"));
        std::process::exit(2);
    }

    // ── 主循环：每帧一个请求 ─────────────────────────────────────────────────────
    loop {
        let frame = match read_frame(&mut reader) {
            Ok(f) => f,
            // stdin EOF（Host 关闭管道/消失）→ 正常退出。
            Err(e) if e.is_clean_eof() => {
                log_info("stdin EOF → 退出");
                std::process::exit(0);
            }
            Err(e) => {
                log_error(format!("读取帧失败（协议损坏）：{e} → 退出"));
                std::process::exit(3);
            }
        };

        match frame.frame_type {
            FrameType::Shutdown => {
                log_info("收到 Shutdown → 退出");
                std::process::exit(0);
            }
            FrameType::Request => {
                // 每任务顶层 catch_unwind：panic → 回 internal_error 后主动退出进程。
                let req_id = frame.request_id;
                let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle_request(&frame)
                }));
                match handled {
                    Ok(out) => {
                        if send(&mut writer, &out).is_err() {
                            std::process::exit(0); // Host 消失
                        }
                    }
                    Err(_) => {
                        // 已知 item_id/fingerprint 才能回 Failure；panic 时尽力解析请求体。
                        let (item_id, fp) = frame
                            .parse_json::<RequestBody>()
                            .map(|r| (r.item_id(), r.input_fingerprint().map(str::to_string)))
                            .unwrap_or((None, None));
                        let fail = FailureBody {
                            item_id,
                            input_fingerprint: fp,
                            code: WorkerErrorCode::InternalError,
                            retryable: true,
                            message: "worker panic".to_string(),
                        };
                        let _ = send(
                            &mut writer,
                            &Frame::control(FrameType::Failure, req_id, &fail).unwrap(),
                        );
                        log_error("任务 panic → 已回 internal_error，主动退出进程");
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

/// 处理一个 Request 帧 → 返回应发送的 Success/Failure 帧。
fn handle_request(frame: &Frame) -> Frame {
    let req: RequestBody = match frame.parse_json() {
        Ok(r) => r,
        Err(e) => {
            // 请求体都解析不了：无法可靠取 item_id；回 internal_error（request_id 仍匹配）。
            log_warn(format!("Request JSON 解析失败：{e}"));
            let fail = FailureBody {
                item_id: None,
                input_fingerprint: None,
                code: WorkerErrorCode::InternalError,
                retryable: false,
                message: "bad request json".to_string(),
            };
            return Frame::control(FrameType::Failure, frame.request_id, &fail).unwrap();
        }
    };

    match req {
        RequestBody::Thumbnail {
            item_id,
            source_path,
            target_long_edge,
            input_fingerprint,
        } => handle_thumbnail(
            frame.request_id,
            item_id,
            &source_path,
            target_long_edge,
            input_fingerprint,
        ),
        RequestBody::Metadata {
            item_id,
            input_fingerprint,
            ..
        } => {
            // 首发不实现 metadata 能力 → 稳定 unsupported_variant。
            let fail = FailureBody {
                item_id: Some(item_id),
                input_fingerprint: Some(input_fingerprint),
                code: WorkerErrorCode::UnsupportedVariant,
                retryable: false,
                message: "metadata 能力未实现".to_string(),
            };
            Frame::control(FrameType::Failure, frame.request_id, &fail).unwrap()
        }
        // v2 会话/嵌入族 op:raw-worker 不声明 embedding/face 能力,host 按能力路由不会派发;
        // 防御性兜底回稳定 unsupported_variant(而非 panic/协议损坏)。
        other @ (RequestBody::SessionInit { .. }
        | RequestBody::SessionClose { .. }
        | RequestBody::EmbedBatch { .. }
        | RequestBody::FaceDetectEmbed { .. }
        | RequestBody::EncodeText { .. }
        // OCR op 归 ai-worker,raw-worker 不支持。
        | RequestBody::OcrSessionInit { .. }
        | RequestBody::OcrSessionClose { .. }
        | RequestBody::OcrBatch { .. }
        // 以下 9 臂系 E0004 穷举兜底补录:Enhance 三臂系 2a36286 波(降噪/超分子系统独立
        // enhance-worker 落地)遗留漏补,Video 六臂系视频格式扩展子系统(独立
        // video-worker 处理)新增,raw-worker 均不支持,防御性兜底稳定错误码。
        | RequestBody::EnhanceSessionInit { .. }
        | RequestBody::EnhanceSessionClose { .. }
        | RequestBody::EnhanceRun { .. }
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
                message: "该 op 未实现(raw-worker 仅 thumbnail)".to_string(),
            };
            Frame::control(FrameType::Failure, frame.request_id, &fail).unwrap()
        }
    }
}

fn handle_thumbnail(
    request_id: u64,
    item_id: i64,
    source_path: &str,
    target_long_edge: u32,
    input_fingerprint: String,
) -> Frame {
    let fail = |code: WorkerErrorCode, retryable: bool, message: String| {
        Frame::control(
            FrameType::Failure,
            request_id,
            &FailureBody {
                item_id: Some(item_id),
                input_fingerprint: Some(input_fingerprint.clone()),
                code,
                retryable,
                message,
            },
        )
        .unwrap()
    };

    // 读盘前先看大小，拦截巨文件（→ resource_limit）。
    match std::fs::metadata(source_path) {
        Ok(m) if m.len() > MAX_SOURCE_FILE_BYTES => {
            return fail(
                WorkerErrorCode::ResourceLimit,
                false,
                format!("源文件过大：{} 字节", m.len()),
            );
        }
        Ok(_) => {}
        Err(e) => {
            // 文件不存在/占用：IO 错误 → retryable。
            return fail(
                WorkerErrorCode::IoError,
                true,
                format!("stat 失败：{}", e.kind()),
            );
        }
    }

    let bytes = match std::fs::read(source_path) {
        Ok(b) => b,
        Err(e) => {
            return fail(
                WorkerErrorCode::IoError,
                true,
                format!("读取失败：{}", e.kind()),
            )
        }
    };

    match decode::decode_raw_to_webp(&bytes, target_long_edge) {
        Ok(out) => {
            let body = SuccessBody {
                item_id: Some(item_id),
                input_fingerprint: Some(input_fingerprint.clone()),
                mime: Some("image/webp".to_string()),
                width: Some(out.width),
                height: Some(out.height),
                ..Default::default()
            };
            Frame::with_blob(FrameType::Success, request_id, &body, out.webp).unwrap()
        }
        Err(e) => fail(e.code, e.code.default_retryable(), e.message),
    }
}

/// 写一帧并 flush（保证 Host 立即可读）。
fn send<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    write_frame(w, frame)?;
    w.flush()?;
    Ok(())
}
