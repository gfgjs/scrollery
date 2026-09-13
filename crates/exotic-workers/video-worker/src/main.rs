// crates/exotic-workers/video-worker/src/main.rs
//! 视频格式扩展 Worker 主循环(视频格式扩展子系统 design.md §2):独立进程,承接
//! probe/remux/transcode/frames 四能力,底层 spawn 外部 ffmpeg/ffprobe(LGPL-shared)。
//! 契约照 raw/enhance-worker 样板:
//!   - **stdout 只走协议帧**;日志只写 stderr(单行 JSON WorkerLogLine)。
//!   - stdin 收 Hello → 回 Ready(四 video 能力)→ 循环收 Request/Shutdown。
//!   - stdin EOF / Shutdown / Host 消失 → 立即退出。
//!   - 严格串行:一次一请求(视频派生本就串行,无并发放大)。
//!   - remux/transcode 期间即时发 Progress 帧(host 静默限时机制据此重置计时,§2.4)。
//!   - 每请求顶层 `catch_unwind`:panic 回 internal_error 并主动退出,不带病服务。
//!   - 每个 ffmpeg/ffprobe 子进程纳入进程级 Job Object(kill-on-close,§9.10):worker
//!     被 supervisor kill 时 OS 自动收割 ffmpeg,杜绝孤儿。
//!
//! BELOW_NORMAL 低优先级在 host spawn 侧统一设定(worker.rs 先例),本端不重复。

mod error;
mod ffrun;
mod frames;
mod job;
mod probe;
mod progress;
mod remux;
mod session;
mod transcode;

use std::io::{BufReader, BufWriter, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use exotic_protocol::{
    capability, read_frame, write_frame, FailureBody, Frame, FrameType, ProgressBody,
    ProtocolError, ReadyBody, RequestBody, SuccessBody, VideoSessionInfo, WorkerErrorCode,
    MAX_BLOB_LEN, PROTOCOL_VERSION,
};

use error::VideoError;
use ffrun::{CancelFlag, StreamEvent};
use session::VideoSessionState;

/// Worker 稳定标识(Host 握手校验;installer manifest 的 worker_id 与此一致)。
const WORKER_ID: &str = "video-worker";
/// Worker 版本(进指纹/展示/升级失效)。
const WORKER_VERSION: &str = env!("CARGO_PKG_VERSION");

// stderr 结构化日志行:统一走 exotic_protocol::WorkerLogLine 单行 JSON(host 侧逐行解析
// 转发进主 tracing/JSONL)。三档:正常生命周期=info、非致命异常=warn、致命=error。
pub(crate) fn log_info(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("info", msg, serde_json::Map::new());
}
pub(crate) fn log_warn(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("warn", msg, serde_json::Map::new());
}
fn log_error(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("error", msg, serde_json::Map::new());
}

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    // ── 握手:等 Hello → 回 Ready ────────────────────────────────────────────────
    match read_frame(&mut reader) {
        Ok(f) if f.frame_type == FrameType::Hello => {
            if let Ok(hello) = f.parse_json::<exotic_protocol::HelloBody>() {
                if hello.protocol_version != PROTOCOL_VERSION {
                    log_warn(format!(
                        "Hello 协议版本 {} != 本端 {}(仍回 Ready,由 Host 决定)",
                        hello.protocol_version, PROTOCOL_VERSION
                    ));
                }
            }
        }
        Ok(f) => {
            log_error(format!("握手期望 Hello,收到 {:?} → 退出", f.frame_type));
            std::process::exit(2);
        }
        Err(e) if e.is_clean_eof() => std::process::exit(0),
        Err(e) => {
            log_error(format!("握手读取失败:{e} → 退出"));
            std::process::exit(2);
        }
    }

    let ready = ReadyBody {
        worker_id: WORKER_ID.to_string(),
        worker_version: WORKER_VERSION.to_string(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: vec![
            capability::VIDEO_PROBE.to_string(),
            capability::VIDEO_REMUX.to_string(),
            capability::VIDEO_TRANSCODE.to_string(),
            capability::VIDEO_FRAMES.to_string(),
            // THUMBNAIL:rmvb/vob 收编走 exotic 任务化(D-444③)。任务化 Thumbnail 无
            // session_id,由 host 侧 VideoThumbnailWorker 先发一次 VideoSessionInit 建 ffmpeg
            // 会话(同 Service 门样式),再派 Thumbnail;能力声明使 host 握手能力校验放行。
            capability::THUMBNAIL.to_string(),
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

    // ── 主循环:严格串行,一帧一请求 ─────────────────────────────────────────────
    let mut sess: Option<VideoSessionState> = None;
    loop {
        let frame = match read_frame(&mut reader) {
            Ok(f) => f,
            Err(e) if e.is_clean_eof() => {
                log_info("stdin EOF → 退出");
                std::process::exit(0);
            }
            Err(e) => {
                log_error(format!("读取帧失败(协议损坏):{e} → 退出"));
                std::process::exit(3);
            }
        };

        match frame.frame_type {
            FrameType::Shutdown => {
                log_info("收到 Shutdown → 退出(会话随进程释放)");
                std::process::exit(0);
            }
            FrameType::Request => {
                let req_id = frame.request_id;
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
                // 每请求顶层 catch_unwind:panic → 回 internal_error 后主动退出。
                // dispatch 内部即时发 Progress 帧(remux/transcode),返回终态由主循环发送。
                let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    dispatch(req, req_id, &mut sess, &mut writer)
                }));
                match handled {
                    Ok(out) => {
                        if send(&mut writer, &out).is_err() {
                            std::process::exit(0); // Host 消失
                        }
                    }
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

/// 分派一个 Request → 返回应发送的终态帧(remux/transcode/frames 期间经 `writer` 即时发
/// Progress,终态经返回值)。
fn dispatch<W: Write>(
    req: RequestBody,
    request_id: u64,
    sess: &mut Option<VideoSessionState>,
    writer: &mut W,
) -> Frame {
    match req {
        RequestBody::VideoSessionInit {
            session_id,
            ffmpeg_exe_path,
            ffmpeg_sha256,
            work_dir,
        } => {
            // host 切换语义:未 Close 即 Init 按切换处理,旧会话先弃。
            *sess = None;
            match session::validate_video_init(
                session_id,
                &ffmpeg_exe_path,
                &ffmpeg_sha256,
                &work_dir,
            ) {
                Ok(state) => {
                    log_info(format!(
                        "视频会话 {} 就绪:ffmpeg={}",
                        session_id, state.ffmpeg_version
                    ));
                    let body = SuccessBody {
                        video_session: Some(VideoSessionInfo {
                            caps: VideoSessionState::caps(),
                            ffmpeg_version: state.ffmpeg_version.clone(),
                        }),
                        ..Default::default()
                    };
                    *sess = Some(state);
                    Frame::control(FrameType::Success, request_id, &body).unwrap()
                }
                Err(e) => {
                    log_warn(format!("VideoSessionInit 失败[{}]:{e}", e.code().as_str()));
                    e.to_failure_frame(request_id)
                }
            }
        }
        RequestBody::VideoSessionClose { session_id } => {
            // 幂等:无会话/错 id 也回 Success(host 只关心「之后没有会话」)。
            match sess.take() {
                Some(s) if s.session_id == session_id => {
                    log_info(format!("视频会话 {session_id} 已卸载"))
                }
                Some(s) => log_warn(format!(
                    "VideoSessionClose id 不符:{session_id} != 当前 {} → 仍卸载当前",
                    s.session_id
                )),
                None => log_info(format!("VideoSessionClose {session_id}:无在载会话(幂等)")),
            }
            Frame::control(FrameType::Success, request_id, &SuccessBody::default()).unwrap()
        }
        RequestBody::VideoProbe {
            session_id,
            source_path,
            ..
        } => match require_session(sess, session_id) {
            Ok(state) => match probe::run_probe(&state.ffprobe_path, &source_path) {
                Ok(info) => {
                    let body = SuccessBody {
                        video_probe: Some(info),
                        ..Default::default()
                    };
                    Frame::control(FrameType::Success, request_id, &body).unwrap()
                }
                Err(e) => e.to_failure_frame(request_id),
            },
            Err(e) => e.to_failure_frame(request_id),
        },
        RequestBody::VideoRemux {
            session_id,
            source_path,
            output_tmp_path,
            audio_transcode,
            audio_track_index,
        } => match require_session(sess, session_id) {
            Ok(state) => remux::handle_remux(
                state,
                request_id,
                &source_path,
                &output_tmp_path,
                audio_transcode,
                audio_track_index,
                &new_cancel(),
                writer,
            ),
            Err(e) => e.to_failure_frame(request_id),
        },
        RequestBody::VideoTranscode {
            session_id,
            source_path,
            output_tmp_path,
            encoder_ladder,
            crf,
            bitrate_kbps,
            max_long_edge,
            audio_track_index,
            hw_decode,
        } => match require_session(sess, session_id) {
            Ok(state) => transcode::handle_transcode(
                state,
                request_id,
                &source_path,
                &output_tmp_path,
                &encoder_ladder,
                crf,
                bitrate_kbps,
                max_long_edge,
                audio_track_index,
                hw_decode,
                &new_cancel(),
                writer,
            ),
            Err(e) => e.to_failure_frame(request_id),
        },
        RequestBody::VideoFrames {
            session_id,
            source_path,
            mode,
            ..
        } => match require_session(sess, session_id) {
            Ok(state) => {
                frames::handle_frames(state, request_id, &source_path, mode, &new_cancel())
            }
            Err(e) => e.to_failure_frame(request_id),
        },
        // 任务化缩略图 op(rmvb/vob 收编,D-444③):任务化 Thumbnail 无 session_id,用当前
        // 在载会话(host 侧 VideoThumbnailWorker 已先发 VideoSessionInit)。语义等同 Cover,
        // 响应回填 item_id/input_fingerprint(host `run_thumbnail` 按二者核对)。
        RequestBody::Thumbnail {
            item_id,
            source_path,
            target_long_edge,
            input_fingerprint,
        } => match sess.as_ref() {
            Some(state) => frames::handle_thumbnail(
                state,
                request_id,
                item_id,
                &source_path,
                target_long_edge,
                input_fingerprint,
                &new_cancel(),
            ),
            None => {
                // 无在载会话 → SessionExpired(retryable:host 重建实例 + 重 Init 后重派)。
                let fail = FailureBody {
                    item_id: Some(item_id),
                    input_fingerprint: Some(input_fingerprint),
                    code: WorkerErrorCode::SessionExpired,
                    retryable: true,
                    message: "无在载视频会话(需先 VideoSessionInit)".to_string(),
                };
                Frame::control(FrameType::Failure, request_id, &fail).unwrap()
            }
        },
        // 其余 op 归其它 worker;host 按能力路由不会派发,防御性兜底稳定错误码。
        other @ (RequestBody::Metadata { .. }
        | RequestBody::SessionInit { .. }
        | RequestBody::SessionClose { .. }
        | RequestBody::EmbedBatch { .. }
        | RequestBody::FaceDetectEmbed { .. }
        | RequestBody::EncodeText { .. }
        | RequestBody::OcrSessionInit { .. }
        | RequestBody::OcrSessionClose { .. }
        | RequestBody::OcrBatch { .. }
        | RequestBody::EnhanceSessionInit { .. }
        | RequestBody::EnhanceSessionClose { .. }
        | RequestBody::EnhanceRun { .. }) => {
            let fail = FailureBody {
                item_id: other.item_id(),
                input_fingerprint: other.input_fingerprint().map(str::to_string),
                code: WorkerErrorCode::UnsupportedVariant,
                retryable: false,
                message: "该 op 未实现(video-worker 仅 video_*)".to_string(),
            };
            Frame::control(FrameType::Failure, request_id, &fail).unwrap()
        }
    }
}

/// 取在载会话:无会话/未知 session_id → SessionExpired(retryable:host 重发 Init 后重派)。
fn require_session(
    sess: &Option<VideoSessionState>,
    session_id: u64,
) -> Result<&VideoSessionState, VideoError> {
    match sess {
        Some(s) if s.session_id == session_id => Ok(s),
        _ => Err(VideoError::SessionExpired),
    }
}

/// 每请求一枚取消旗标。当前串行模型无带内取消源(host kill + Job Object kill-on-close 是
/// 操作路径,§2.4/§9.10);旗标已全程贯通 kill+tmp 清理逻辑,待带内取消控制通道落地即接源。
fn new_cancel() -> CancelFlag {
    Arc::new(AtomicBool::new(false))
}

/// 删除失败/取消残留的 `.tmp` 产物(best-effort)。
pub(crate) fn cleanup_tmp(path: &str) {
    let _ = std::fs::remove_file(path);
}

/// remux/transcode 完工后对**产物本身**跑一次 ffprobe,回填实测 `out_duration_ms`
/// (不再抄源时长——容器改封/转码理论上时长不变,但产物本身才是唯一真相源)。产物打不开
/// /无视频流(损坏)一律归 Malformed(完工却产出坏文件属源侧问题,非 worker 内部错误)。
pub(crate) fn probe_output_duration_ms(
    ffprobe: &std::path::Path,
    output: &std::path::Path,
) -> Result<u64, VideoError> {
    let out_str = output.to_string_lossy();
    probe::run_probe(ffprobe, &out_str)
        .map(|info| info.duration_ms.unwrap_or(0))
        .map_err(|e| match e {
            VideoError::Malformed(m) => VideoError::Malformed(format!("产物校验失败:{m}")),
            other => other,
        })
}

/// 流式 op(remux/transcode)统一执行:跑 ffmpeg,`-progress` 位置推进 → 节流(≤2s)后
/// 发 Progress 帧(host 静默限时据此重置)。writer 发送失败(host 消失)→ 主动退出。
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_streaming_op<W: Write>(
    ffmpeg: &std::path::Path,
    args: &[String],
    duration_ms: Option<u64>,
    stage: &str,
    output: &std::path::Path,
    request_id: u64,
    cancel: &CancelFlag,
    writer: &mut W,
) -> Result<(), VideoError> {
    let started = Instant::now();
    let mut throttle = progress::ProgressThrottle::default();
    ffrun::run_ffmpeg_streaming(ffmpeg, args, Some(output), cancel, |ev| {
        // Progress:节流 ≤2s + 百分比 detail。Finalize:trailer 静默窗心跳(stage="finalize",
        // detail 携产物当前字节数;节流由 ffrun 内 FinalizeMonitor 保证,此处直接发)。
        let (st, detail) = match ev {
            StreamEvent::Progress(pos_ms) => {
                if !throttle.should_emit(Instant::now(), false) {
                    return;
                }
                (
                    stage,
                    progress::percent(pos_ms, duration_ms).map(|p| format!("{p}%")),
                )
            }
            StreamEvent::Finalize { out_bytes } => ("finalize", Some(format!("{out_bytes}B"))),
        };
        let body = ProgressBody {
            stage: st.to_string(),
            detail,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        if send(
            writer,
            &Frame::control(FrameType::Progress, request_id, &body).unwrap(),
        )
        .is_err()
        {
            std::process::exit(0); // Host 消失
        }
    })
}

/// 写一帧并 flush(保证 Host 立即可读)。
pub(crate) fn send<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    write_frame(w, frame)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_session_gates_on_id() {
        let none: Option<VideoSessionState> = None;
        assert!(matches!(
            require_session(&none, 1),
            Err(VideoError::SessionExpired)
        ));
    }

    #[test]
    fn video_session_close_no_session_is_idempotent() {
        let mut sess: Option<VideoSessionState> = None;
        let mut buf: Vec<u8> = Vec::new();
        let frame = dispatch(
            RequestBody::VideoSessionClose { session_id: 1 },
            42,
            &mut sess,
            &mut buf,
        );
        assert_eq!(frame.frame_type, FrameType::Success);
        assert!(sess.is_none());
    }

    #[test]
    fn probe_without_session_is_session_expired() {
        let mut sess: Option<VideoSessionState> = None;
        let mut buf: Vec<u8> = Vec::new();
        let frame = dispatch(
            RequestBody::VideoProbe {
                session_id: 1,
                source_path: "x.mkv".into(),
                input_fingerprint: "fp".into(),
            },
            7,
            &mut sess,
            &mut buf,
        );
        assert_eq!(frame.frame_type, FrameType::Failure);
        let fail: FailureBody = frame.parse_json().unwrap();
        assert_eq!(fail.code, WorkerErrorCode::SessionExpired);
    }

    #[test]
    fn thumbnail_without_session_is_session_expired_with_item_echo() {
        // 任务化 Thumbnail 无 session_id;无在载会话 → SessionExpired(retryable),且回填
        // item_id/input_fingerprint(host `run_thumbnail` 对 Failure 亦按二者核对)。
        let mut sess: Option<VideoSessionState> = None;
        let mut buf: Vec<u8> = Vec::new();
        let frame = dispatch(
            RequestBody::Thumbnail {
                item_id: 77,
                source_path: "a.rmvb".into(),
                target_long_edge: 480,
                input_fingerprint: "fp-x".into(),
            },
            11,
            &mut sess,
            &mut buf,
        );
        assert_eq!(frame.frame_type, FrameType::Failure);
        let fail: FailureBody = frame.parse_json().unwrap();
        assert_eq!(fail.code, WorkerErrorCode::SessionExpired);
        assert!(fail.retryable);
        assert_eq!(fail.item_id, Some(77));
        assert_eq!(fail.input_fingerprint.as_deref(), Some("fp-x"));
    }

    #[test]
    fn foreign_op_rejected_unsupported() {
        let mut sess: Option<VideoSessionState> = None;
        let mut buf: Vec<u8> = Vec::new();
        let frame = dispatch(
            RequestBody::Metadata {
                item_id: 3,
                source_path: "a.mkv".into(),
                input_fingerprint: "fp".into(),
            },
            9,
            &mut sess,
            &mut buf,
        );
        assert_eq!(frame.frame_type, FrameType::Failure);
        let fail: FailureBody = frame.parse_json().unwrap();
        assert_eq!(fail.code, WorkerErrorCode::UnsupportedVariant);
    }
}
