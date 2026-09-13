// crates/exotic-workers/enhance-worker/src/run.rs
//! EnhanceRun 执行:输出白名单校验 → task 映射 → 构造链请求 → 驱动 run_enhance_chain
//! → per-tile Progress 帧 → 终态映射(design.md §E)。

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use exotic_protocol::{
    EnhanceDone, EnhanceStep, EnhanceTask, FailureBody, Frame, FrameType, ProgressBody,
    SuccessBody, WorkerErrorCode,
};
use scrollery_ai_core::enhance::{
    run_enhance_chain, EnhanceChainRequest, EnhanceError, EnhanceOutputFormat, EnhanceStepSpec,
    EnhanceTaskKind,
};

use crate::session::EnhanceSessionState;

/// wire `EnhanceTask`(snake_case)→ core `EnhanceTaskKind`(camelCase)的**显式** match 映射。
/// 两枚举串形刻意不同(协议 snake vs profile camel),**禁串比对**:靠类型系统的穷尽性
/// 保证任一端新增变体时本函数编译不过、必须显式补齐。
pub fn map_task(task: EnhanceTask) -> EnhanceTaskKind {
    match task {
        EnhanceTask::Denoise => EnhanceTaskKind::Denoise,
        EnhanceTask::DejpegArtifact => EnhanceTaskKind::DejpegArtifact,
        EnhanceTask::Upscale => EnhanceTaskKind::Upscale,
    }
}

/// 解析输出格式;非 `"jpeg"`/`"png"` → MalformedInput。
fn parse_output_format(fmt: &str) -> Result<EnhanceOutputFormat, InitFail> {
    match fmt {
        "jpeg" => Ok(EnhanceOutputFormat::Jpeg),
        "png" => Ok(EnhanceOutputFormat::Png),
        other => Err(InitFail(
            WorkerErrorCode::MalformedInput,
            format!("不支持的输出格式:{other}(仅 jpeg/png)"),
        )),
    }
}

/// 前置校验失败(未进推理):(码, 消息)。
#[derive(Debug)]
struct InitFail(WorkerErrorCode, String);

/// 输出路径白名单校验:`output_tmp_path` 的父目录 canonicalize 后必须位于 `work_dir`
/// (会话态已 canonicalize)之下,否则 MalformedInput。canonicalize 会解析 `..`,故
/// `..` 穿越会在 `starts_with` 判定处被拦(OCR cache_key 越界拒绝同型)。返回安全落盘路径。
fn resolve_output_path(output_tmp_path: &str, work_dir: &Path) -> Result<PathBuf, InitFail> {
    let p = Path::new(output_tmp_path);
    let parent = p
        .parent()
        .filter(|s| !s.as_os_str().is_empty())
        .ok_or_else(|| {
            InitFail(
                WorkerErrorCode::MalformedInput,
                "output_tmp_path 无父目录".into(),
            )
        })?;
    let canon_parent = std::fs::canonicalize(parent).map_err(|e| {
        InitFail(
            WorkerErrorCode::MalformedInput,
            format!("output 父目录不可达:{}", e.kind()),
        )
    })?;
    if !canon_parent.starts_with(work_dir) {
        return Err(InitFail(
            WorkerErrorCode::MalformedInput,
            "output_tmp_path 越界 work_dir 白名单".into(),
        ));
    }
    let name = p.file_name().ok_or_else(|| {
        InitFail(
            WorkerErrorCode::MalformedInput,
            "output_tmp_path 无文件名".into(),
        )
    })?;
    Ok(canon_parent.join(name))
}

/// `EnhanceError` → `WorkerErrorCode` 的**逐变体**显式映射(禁 catch-all 泛化;
/// 结尾 `_` 仅为 `#[non_exhaustive]` 的未来变体兜底,非已知变体归并)。
fn map_enhance_error(err: &EnhanceError) -> WorkerErrorCode {
    match err {
        // 解码失败/不支持的输入 = 输入侧畸形。
        EnhanceError::Decode(_) => WorkerErrorCode::MalformedInput,
        EnhanceError::UnsupportedInput(_) => WorkerErrorCode::MalformedInput,
        // 推理失败 = worker 内部(池断/张量构造/输出形状不符)。
        EnhanceError::Inference(_) => WorkerErrorCode::InternalError,
        // 读写盘 IO(暂时性)。
        EnhanceError::Io(_) => WorkerErrorCode::IoError,
        // 模型档缺失 = 会话装载契约未满足,terminal。
        EnhanceError::ProfileMissing(_) => WorkerErrorCode::ModelLoadFailed,
        // 编码失败:task 明列外(编解码器内部),归 InternalError。
        EnhanceError::Encode(_) => WorkerErrorCode::InternalError,
        // #[non_exhaustive] 未来变体兜底(含未来内存/尺寸类若新增)。
        _ => WorkerErrorCode::InternalError,
    }
}

/// 会话未加载 → SessionExpired(retryable:host 重发 EnhanceSessionInit 后重派)。
pub fn session_expired(request_id: u64) -> Frame {
    let fail = FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::SessionExpired,
        retryable: true,
        message: "增强会话未加载或已卸载".to_string(),
    };
    Frame::control(FrameType::Failure, request_id, &fail).unwrap()
}

fn failure_frame(request_id: u64, code: WorkerErrorCode, message: String) -> Frame {
    let fail = FailureBody {
        item_id: None,
        input_fingerprint: None,
        code,
        retryable: code.default_retryable(),
        message,
    };
    Frame::control(FrameType::Failure, request_id, &fail).unwrap()
}

/// 处理一次 EnhanceRun:无会话 → SessionExpired;否则校验输出白名单 + 映射 steps,
/// 同步驱动 [`run_enhance_chain`],per-tile 回调 → Progress 帧,终态回 Success/Failure。
/// `writer` 用于即时发送 Progress 帧(host 收帧即重置静默计时,design.md §E)。
pub fn handle_enhance_run<W: Write>(
    state: Option<&EnhanceSessionState>,
    request_id: u64,
    source_path: String,
    output_tmp_path: String,
    output_format: String,
    steps: Vec<EnhanceStep>,
    writer: &mut W,
) -> Frame {
    let Some(state) = state else {
        return session_expired(request_id);
    };

    // ── 前置校验(未进推理):格式 + 输出白名单 ────────────────────────────────
    let fmt = match parse_output_format(&output_format) {
        Ok(f) => f,
        Err(InitFail(code, msg)) => return failure_frame(request_id, code, msg),
    };
    let safe_output = match resolve_output_path(&output_tmp_path, &state.work_dir) {
        Ok(p) => p,
        Err(InitFail(code, msg)) => return failure_frame(request_id, code, msg),
    };

    let step_specs: Vec<EnhanceStepSpec> = steps
        .iter()
        .map(|s| EnhanceStepSpec {
            task: map_task(s.task),
            profile_id: s.model_id.clone(),
            strength: s.strength,
        })
        .collect();

    let req = EnhanceChainRequest {
        source_path: PathBuf::from(source_path),
        output_tmp_path: safe_output,
        output_format: fmt,
        steps: step_specs,
    };

    // ── 驱动增强链;per-tile 回调即时发 Progress 帧(host gone → exit(0))──────────
    let started = Instant::now();
    let writer_cell = RefCell::new(writer);
    let result = {
        let progress = |done: u32, total: u32| {
            let body = ProgressBody {
                stage: "enhance_tile".to_string(),
                detail: Some(format!("{done}/{total}")),
                elapsed_ms: started.elapsed().as_millis() as u64,
            };
            let mut w = writer_cell.borrow_mut();
            if crate::send(
                &mut **w,
                &Frame::control(FrameType::Progress, request_id, &body).unwrap(),
            )
            .is_err()
            {
                std::process::exit(0); // Host 消失
            }
        };
        run_enhance_chain(&state.sessions, &req, Some(&progress))
    };

    match result {
        Ok(report) => {
            let body = SuccessBody {
                enhance: Some(EnhanceDone {
                    out_width: report.out_width,
                    out_height: report.out_height,
                    tiles_total: report.tiles_total,
                }),
                ..Default::default()
            };
            Frame::control(FrameType::Success, request_id, &body).unwrap()
        }
        Err(e) => {
            let code = map_enhance_error(&e);
            failure_frame(request_id, code, e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_mapping_is_explicit_and_total() {
        assert_eq!(map_task(EnhanceTask::Denoise), EnhanceTaskKind::Denoise);
        assert_eq!(
            map_task(EnhanceTask::DejpegArtifact),
            EnhanceTaskKind::DejpegArtifact
        );
        assert_eq!(map_task(EnhanceTask::Upscale), EnhanceTaskKind::Upscale);
    }

    #[test]
    fn output_format_parse() {
        assert!(matches!(
            parse_output_format("jpeg"),
            Ok(EnhanceOutputFormat::Jpeg)
        ));
        assert!(matches!(
            parse_output_format("png"),
            Ok(EnhanceOutputFormat::Png)
        ));
        assert!(matches!(
            parse_output_format("webp"),
            Err(InitFail(WorkerErrorCode::MalformedInput, _))
        ));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "enhance-worker-run-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn output_whitelist_accepts_inside() {
        let work = temp_dir("inside");
        let out = work.join("job1.tmp");
        let resolved = resolve_output_path(out.to_str().unwrap(), &work).expect("同目录合法");
        assert!(resolved.starts_with(&work));
        assert_eq!(resolved.file_name().unwrap(), "job1.tmp");
    }

    #[test]
    fn output_whitelist_accepts_subdir() {
        let work = temp_dir("subdir");
        let sub = work.join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        let out = sub.join("job.tmp");
        let resolved = resolve_output_path(out.to_str().unwrap(), &work).expect("子目录合法");
        assert!(resolved.starts_with(&work));
    }

    #[test]
    fn output_whitelist_rejects_sibling() {
        let work = temp_dir("wd");
        // 兄弟目录(与 work 同父但不在其下)。
        let sibling = work.parent().unwrap().join(format!(
            "enhance-worker-run-test-{}-sibling",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&sibling);
        std::fs::create_dir_all(&sibling).unwrap();
        let out = sibling.join("evil.tmp");
        let e = resolve_output_path(out.to_str().unwrap(), &work).expect_err("越界应拒");
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn output_whitelist_rejects_dotdot_traversal() {
        let work = temp_dir("traverse");
        let outside = work.parent().unwrap().join(format!(
            "enhance-worker-run-test-{}-outside",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        // work/../<outside>/evil.tmp —— canonicalize 解析 `..` 后落在 work 之外。
        let traversal = work
            .join("..")
            .join(outside.file_name().unwrap())
            .join("evil.tmp");
        let e = resolve_output_path(traversal.to_str().unwrap(), &work).expect_err("`..` 穿越应拒");
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn output_whitelist_rejects_missing_parent() {
        let work = temp_dir("missing");
        let out = work.join("no-such-subdir").join("job.tmp");
        // 父目录不存在 → canonicalize 失败 → MalformedInput。
        let e = resolve_output_path(out.to_str().unwrap(), &work).expect_err("父不可达应拒");
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn error_mapping_is_per_variant() {
        assert_eq!(
            map_enhance_error(&EnhanceError::Decode("x".into())),
            WorkerErrorCode::MalformedInput
        );
        assert_eq!(
            map_enhance_error(&EnhanceError::UnsupportedInput("x".into())),
            WorkerErrorCode::MalformedInput
        );
        assert_eq!(
            map_enhance_error(&EnhanceError::Inference("x".into())),
            WorkerErrorCode::InternalError
        );
        assert_eq!(
            map_enhance_error(&EnhanceError::Io("x".into())),
            WorkerErrorCode::IoError
        );
        assert_eq!(
            map_enhance_error(&EnhanceError::ProfileMissing("x".into())),
            WorkerErrorCode::ModelLoadFailed
        );
        assert_eq!(
            map_enhance_error(&EnhanceError::Encode("x".into())),
            WorkerErrorCode::InternalError
        );
    }

    #[test]
    fn run_without_session_is_session_expired() {
        let mut buf: Vec<u8> = Vec::new();
        let frame = handle_enhance_run(
            None,
            5,
            "a.jpg".into(),
            "x.tmp".into(),
            "jpeg".into(),
            vec![],
            &mut buf,
        );
        assert_eq!(frame.frame_type, FrameType::Failure);
        let fail: FailureBody = frame.parse_json().unwrap();
        assert_eq!(fail.code, WorkerErrorCode::SessionExpired);
    }
}
