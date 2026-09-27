// crates/exotic-workers/ai-worker/src/ocr.rs
//! OcrSessionInit 校验 + OcrBatch 处理(D-OCR-1/2/4;T5)。
//!
//! 两段式镜像 session.rs 纪律:[`validate_ocr_init`](纯校验,零 ort,可单测)→
//! `main.rs` 内同步 `OcrEngine::init`(OCR 会话装载是秒级 CPU 小模型,无需 SessionInit
//! 那套流式 Progress 心跳,D-OCR-2)。OCR 会话独立于 CLIP 会话(D-OCR-1):worker 内
//! 双槽并存(`main.rs` 的 `sess`/`ocr_sess` 两个各自 `Option`),互不干扰。
//!
//! 校验清单(全部对**明文**,同 session.rs::validate_and_resolve):
//!   1. `models_root` canonicalize 可达;
//!   2. `ocr_profile_id` 在 `scrollery_ai_core::ocr_profile` 内建注册表可解析;
//!   3. 每个 [`ModelDescriptor`]:handle 为 Path、canonicalize 后以 models_root 为前缀、
//!      按角色对应契约文件名、字节数与 sha256 逐一相符(经 `session::verify_descriptor`
//!      `pub(crate)` 复用,零逻辑复制);
//!   4. 角色集完备:OcrDet/OcrCls/OcrRec/OcrDict 四件**全部必备**,无可选角色
//!      (与 CLIP 的「成对可选」不同,OCR 管线三模型缺一不可)。

use std::collections::HashSet;
use std::path::PathBuf;

use exotic_protocol::{
    Frame, FrameType, ModelDescriptor, ModelRole, OcrBatchSuccess, OcrItem, OcrItemResult, OcrLine,
    SuccessBody, WorkerErrorCode,
};
use scrollery_ai_core::ocr_profile::{find_ocr_profile, OcrProfile};

use crate::session::{verify_descriptor, InitError};

/// OcrBatch 单批项数上限(交互场景一批一图为主,批口面向未来;超限= host bug)。
const MAX_OCR_ITEMS: usize = 8;

/// source_path 回退解码的源文件字节上限。
/// **镜像自 `batch.rs:27`(`MAX_SOURCE_FILE_BYTES`),勿双向漂移**——batch.rs 是他线
/// 在途改动(施工期不触碰),本卡在此本地复制同值常量,而非改 batch.rs 可见性。
const MAX_SOURCE_FILE_BYTES: u64 = 512 << 20;

/// 单项识别文本总字节上限(D-OCR-4 防御 cap):超限该项判 `ResourceLimit`(逐项不连坐)。
const MAX_ITEM_TEXT_BYTES: usize = 512 * 1024;

/// 源像素上限(宽×高)。**镜像自 `crates/exotic-workers/psd-worker/src/decode.rs` 的
/// `MAX_SOURCE_PIXELS` 同值同理由**:100 兆像素 ≈ 10000×10000,OCR 用途足够宽松。
/// 30MB PNG 可解出 400MB+ RGBA——`MAX_SOURCE_FILE_BYTES` 的 stat 只拦压缩字节数,
/// 这里是像素级设防(超限该项判 `ResourceLimit`,不连坐)。
const MAX_SOURCE_PIXELS: u64 = 100_000_000;

/// 终帧 JSON 总长上限(理论不可达——行数 cap × 单项字节 cap 已双重收紧;双保险,
/// 防极端密集文字页把整批 JSON 撑到协议 `MAX_JSON_LEN` 附近)。
const MAX_OCR_RESPONSE_JSON_BYTES: usize = 900 * 1024;

/// 已加载的 OCR 推理会话(worker 端与 CLIP [`crate::session::SessionState`] 并存的
/// 独立槽;严格串行下同一时刻至多一个)。
pub struct OcrSessionState {
    pub session_id: u64,
    pub engine: scrollery_ai_core::ocr::OcrEngine,
}

fn fail(msg: impl Into<String>) -> InitError {
    (WorkerErrorCode::ModelLoadFailed, msg.into())
}

/// 纯校验段:解析 OCR profile + 逐模型完整性。不触 ort。
/// 成功返回 `(profile, models_root 的 canonical 绝对路径)`,供调用方(main.rs)接着跑
/// `OcrEngine::init`(ort 加载段,不在此函数内——保持本函数零 ort、可单测)。
pub fn validate_ocr_init(
    session_id: u64,
    models: &[ModelDescriptor],
    ocr_profile_id: &str,
    models_root: &str,
) -> Result<(OcrProfile, PathBuf), InitError> {
    let _ = session_id; // 会话 id 校验语义同 session.rs:仅记录/日志用,不参与校验逻辑。
    let root = std::fs::canonicalize(models_root)
        .map_err(|e| fail(format!("models_root 不可达:{}", e.kind())))?;

    let profile = find_ocr_profile(ocr_profile_id)
        .ok_or_else(|| fail(format!("未知 ocr_profile_id:{ocr_profile_id}")))?;

    // OCR 四角色全部必备(与 CLIP「成对可选」不同,det/cls/rec/dict 缺一管线不可用)。
    let expected: [(ModelRole, &str); 4] = [
        (ModelRole::OcrDet, profile.det_file.as_str()),
        (ModelRole::OcrCls, profile.cls_file.as_str()),
        (ModelRole::OcrRec, profile.rec_file.as_str()),
        (ModelRole::OcrDict, profile.dict_file.as_str()),
    ];

    let mut seen: HashSet<&str> = HashSet::new();
    for desc in models {
        let expected_file = expected
            .iter()
            .find(|(role, _)| *role == desc.role)
            .map(|(_, f)| *f)
            .ok_or_else(|| {
                fail(format!(
                    "未预期的模型角色:{:?}(与 OCR profile 不符)",
                    desc.role
                ))
            })?;
        // 同角色重复声明 = host bug,拒绝(用契约文件名作去重键,role 与其一一对应)。
        if !seen.insert(expected_file) {
            return Err(fail(format!("模型角色重复声明:{:?}", desc.role)));
        }
        verify_descriptor(desc, expected_file, &root)?;
    }
    if seen.len() != expected.len() {
        return Err(fail(format!(
            "OCR 模型角色不完备:声明 {}/{}(det/cls/rec/dict 四件必备)",
            seen.len(),
            expected.len()
        )));
    }

    Ok((profile, root))
}

fn log_info(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("info", msg, serde_json::Map::new());
}
fn log_warn(msg: impl Into<String>) {
    exotic_protocol::emit_stderr_log("warn", msg, serde_json::Map::new());
}

/// 整批失败帧(逐项 Err 之外的系统性失败:批超限/JSON 超限,姿态同 batch.rs::batch_failure)。
fn batch_failure(request_id: u64, code: WorkerErrorCode, message: String) -> Frame {
    let body = exotic_protocol::FailureBody {
        item_id: None,
        input_fingerprint: None,
        code,
        retryable: code.default_retryable(),
        message,
    };
    Frame::control(FrameType::Failure, request_id, &body).unwrap()
}

/// 单项源解码:仅 `source_path`(信任语义同 `Thumbnail.source_path`,host 提供绝对路径)。
/// **镜像自 `batch.rs:380-403`(`load_face_image`)的 source_path 分支,勿双向漂移**——
/// batch.rs 是他线在途改动,本卡不碰该文件。`cache_key` 分支未接入:`OcrSessionInit`
/// 协议未携带 `ai_cache_dir`(与 CLIP `SessionInit` 不同),且已弃方案裁定 OCR 图片恒走
/// `source_path`(ai_cache webp ≤640 级,文字分辨率不足,见 construction-plan §5)——
/// `OcrItem.cache_key` 字段仅为与 `FaceItem` 同构预留,当前忽略,不视为契约缺陷。
///
/// 像素级设防(深审裁决①):`MAX_SOURCE_FILE_BYTES` 的 stat 只拦压缩字节数——一张
/// 30MB PNG 可解出 400MB+ RGBA。先用 `ImageReader::into_dimensions` 只解头拿声明
/// 尺寸(不解像素数据),超 `MAX_SOURCE_PIXELS` 即该项 `ResourceLimit`,不进入真正
/// 解码分配;通过后才调用 `load_from_memory` 做完整解码。
fn load_ocr_image(item: &OcrItem) -> Result<image::DynamicImage, WorkerErrorCode> {
    let Some(src) = &item.source_path else {
        // cache_key 也缺 = host bug(source_path 是本 op 唯一可用入口)。
        return Err(WorkerErrorCode::MalformedInput);
    };
    let meta = std::fs::metadata(src).map_err(|_| WorkerErrorCode::IoError)?;
    if meta.len() > MAX_SOURCE_FILE_BYTES {
        return Err(WorkerErrorCode::ResourceLimit);
    }
    let bytes = std::fs::read(src).map_err(|_| WorkerErrorCode::IoError)?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| WorkerErrorCode::MalformedInput)?
        .into_dimensions()
        .map_err(|_| WorkerErrorCode::MalformedInput)?;
    if (w as u64) * (h as u64) > MAX_SOURCE_PIXELS {
        return Err(WorkerErrorCode::ResourceLimit);
    }
    image::load_from_memory(&bytes).map_err(|_| WorkerErrorCode::MalformedInput)
}

/// 单项处理:加载源图 → `engine.recognize` → 行数/字节 cap → 装配 `Ok`。
/// 错误映射:读文件失败/缺文件 → IoError;解码失败 → MalformedInput;推理失败 →
/// InternalError(逐项 Err,不连坐整批——batch.rs 模块头同款契约)。
fn ocr_one(sess: &OcrSessionState, item: &OcrItem) -> Result<OcrItemResult, WorkerErrorCode> {
    let img = load_ocr_image(item)?;
    let out = sess.engine.recognize(&img).map_err(|e| {
        log_warn(format!("item {} OCR 推理失败:{e}", item.item_id));
        WorkerErrorCode::InternalError
    })?;

    let profile = &sess.engine.profile;
    // engine.recognize 内部已按 max_lines_per_image 截断(ocr/mod.rs);此处仍显式判断
    // 并 take 封顶——双重收紧,防御 ai-core 一侧行为漂移(契约自检,非冗余噪音)。
    let truncated = out.lines.len() > profile.max_lines_per_image;
    let mut lines: Vec<OcrLine> =
        Vec::with_capacity(out.lines.len().min(profile.max_lines_per_image));
    let mut total_text_bytes = 0usize;
    for l in out.lines.into_iter().take(profile.max_lines_per_image) {
        total_text_bytes += l.text.len();
        lines.push(OcrLine {
            text: l.text,
            quad: l.quad,
            confidence: l.confidence,
        });
    }
    if truncated {
        log_warn(format!(
            "item {} OCR 行数截断至 max_lines_per_image={}",
            item.item_id, profile.max_lines_per_image
        ));
    }
    if total_text_bytes > MAX_ITEM_TEXT_BYTES {
        return Err(WorkerErrorCode::ResourceLimit);
    }

    Ok(OcrItemResult::Ok {
        item_id: item.item_id,
        fingerprint: item.fingerprint.clone(),
        lines,
        width: out.width,
        height: out.height,
    })
}

/// 处理 OcrBatch:逐项串行(交互场景一批一图为主,不组批;`MAX_OCR_ITEMS` 上限防御)。
/// 单项失败不连坐整批(D-OCR-4);终帧 JSON 长度双保险。
pub fn handle_ocr_batch(sess: &OcrSessionState, request_id: u64, items: &[OcrItem]) -> Frame {
    let batch_t0 = std::time::Instant::now();
    if items.len() > MAX_OCR_ITEMS {
        return batch_failure(
            request_id,
            WorkerErrorCode::MalformedInput,
            format!("OCR 批大小 {} 超过上限 {MAX_OCR_ITEMS}", items.len()),
        );
    }

    let mut results = Vec::with_capacity(items.len());
    let mut n_ok = 0usize;
    let mut n_lines = 0usize;
    for item in items {
        match ocr_one(sess, item) {
            Ok(result) => {
                if let OcrItemResult::Ok { ref lines, .. } = result {
                    n_ok += 1;
                    n_lines += lines.len();
                }
                results.push(result);
            }
            Err(code) => results.push(OcrItemResult::Err {
                item_id: item.item_id,
                fingerprint: item.fingerprint.clone(),
                code,
            }),
        }
    }

    let body = SuccessBody {
        ocr: Some(OcrBatchSuccess { results }),
        ..Default::default()
    };
    // 终帧 JSON 长度防御(理论不可达,双保险):行数/单项字节 cap 已在 ocr_one 内拦下。
    // 有意整批降级:单项用法下不可达(行数/单项字节 cap 在前),多项批未来若需严格
    // 不连坐再改逐项预算;勿当 bug 统一。
    if let Ok(bytes) = serde_json::to_vec(&body) {
        if bytes.len() > MAX_OCR_RESPONSE_JSON_BYTES {
            return batch_failure(
                request_id,
                WorkerErrorCode::ResourceLimit,
                format!(
                    "OCR 响应 JSON {} 字节超上限 {MAX_OCR_RESPONSE_JSON_BYTES}",
                    bytes.len()
                ),
            );
        }
    }

    log_info(format!(
        "OcrBatch 批诊断:{}/{} 项 {} 行,墙钟 {}ms",
        n_ok,
        items.len(),
        n_lines,
        batch_t0.elapsed().as_millis(),
    ));

    Frame::with_blob(FrameType::Success, request_id, &body, Vec::new()).unwrap()
}
