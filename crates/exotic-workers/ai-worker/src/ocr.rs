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

#[cfg(test)]
mod tests {
    use super::*;
    use exotic_protocol::ModelHandle;

    /// 每测试独立临时目录(进程 id + 名字),避免并行测试互踩(镜像 session.rs::temp_dir)。
    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ai-worker-ocr-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 在 root 下落一个契约文件并返回其合法 descriptor(镜像 session.rs::lay_model)。
    fn lay_model(
        root: &std::path::Path,
        role: ModelRole,
        file: &str,
        content: &[u8],
    ) -> ModelDescriptor {
        let p = root.join(file);
        std::fs::write(&p, content).unwrap();
        ModelDescriptor {
            role,
            handle: ModelHandle::Path(p.to_string_lossy().into_owned()),
            len: content.len() as u64,
            sha256: crate::session::sha256_file(&p).unwrap(),
            model_id: None,
        }
    }

    fn full_ocr_set(root: &std::path::Path, profile: &OcrProfile) -> Vec<ModelDescriptor> {
        vec![
            lay_model(root, ModelRole::OcrDet, &profile.det_file, b"det"),
            lay_model(root, ModelRole::OcrCls, &profile.cls_file, b"cls"),
            lay_model(root, ModelRole::OcrRec, &profile.rec_file, b"rec"),
            lay_model(root, ModelRole::OcrDict, &profile.dict_file, b"dict"),
        ]
    }

    #[test]
    fn happy_path_full_set_resolves() {
        let root = temp_dir("happy");
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let models = full_ocr_set(&root, &profile);
        let (resolved, resolved_root) =
            validate_ocr_init(1, &models, &profile.id, root.to_str().unwrap())
                .expect("合法四件套应通过校验");
        assert_eq!(resolved.id, profile.id);
        assert_eq!(resolved_root, std::fs::canonicalize(&root).unwrap());
    }

    #[test]
    fn unknown_profile_id_rejected() {
        let root = temp_dir("unknown-profile");
        let e = validate_ocr_init(1, &[], "no-such-profile", root.to_str().unwrap()).unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("ocr_profile_id"), "diagnostic: {}", e.1);
    }

    #[test]
    fn models_root_unreachable_rejected() {
        // 用本机临时目录下一个刻意不存在的子路径(镜像 session.rs 同名测试思路避开
        // 不存在的盘符——`Z:/...` 在部分机器上触发慢速网络驱动器探测,>60s)。
        let root = temp_dir("unreachable-parent");
        let missing = root.join("does-not-exist");
        let e =
            validate_ocr_init(1, &[], "pp-ocrv5-mobile", missing.to_str().unwrap()).unwrap_err();
        assert!(e.1.contains("models_root"), "diagnostic: {}", e.1);
    }

    #[test]
    fn sha256_mismatch_rejected() {
        let root = temp_dir("sha");
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let mut models = full_ocr_set(&root, &profile);
        models[0].sha256 = "0".repeat(64);
        let e = validate_ocr_init(1, &models, &profile.id, root.to_str().unwrap()).unwrap_err();
        assert!(e.1.contains("sha256"), "diagnostic: {}", e.1);
    }

    #[test]
    fn missing_role_rejected() {
        let root = temp_dir("missing-role");
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let mut models = full_ocr_set(&root, &profile);
        models.pop(); // 丢 OcrDict
        let e = validate_ocr_init(1, &models, &profile.id, root.to_str().unwrap()).unwrap_err();
        assert!(e.1.contains("不完备"), "diagnostic: {}", e.1);
    }

    #[test]
    fn duplicate_role_rejected() {
        let root = temp_dir("dup");
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let mut models = full_ocr_set(&root, &profile);
        models.push(models[0].clone());
        let e = validate_ocr_init(1, &models, &profile.id, root.to_str().unwrap()).unwrap_err();
        assert!(e.1.contains("重复"), "diagnostic: {}", e.1);
    }

    #[test]
    fn unexpected_role_rejected() {
        let root = temp_dir("unexpected-role");
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let mut models = full_ocr_set(&root, &profile);
        // CLIP 角色混入 OCR 会话 = host bug,必须拒。
        models.push(lay_model(
            &root,
            ModelRole::ImageEncoder,
            "stray.onnx",
            b"x",
        ));
        let e = validate_ocr_init(1, &models, &profile.id, root.to_str().unwrap()).unwrap_err();
        assert!(e.1.contains("未预期"), "diagnostic: {}", e.1);
    }

    #[test]
    fn path_outside_models_root_rejected() {
        let root = temp_dir("outside-root");
        let elsewhere = temp_dir("outside-elsewhere");
        let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
        let mut models = full_ocr_set(&root, &profile);
        // 把 det 塔换成 root 之外的同名合法文件(镜像 session.rs::path_outside_models_root_rejected)
        // → 归属校验(verify_descriptor 的 canon.starts_with(root))必须拦下。
        models[0] = lay_model(&elsewhere, ModelRole::OcrDet, &profile.det_file, b"det");
        let e = validate_ocr_init(1, &models, &profile.id, root.to_str().unwrap()).unwrap_err();
        assert!(e.1.contains("越界"), "diagnostic: {}", e.1);
    }

    /// 标准 CRC-32(zlib/PNG 用,polynomial 0xEDB88320,reflected,init/final 0xFFFFFFFF)。
    /// 构造畸形 IHDR fixture 需要在改字段后自行重算 chunk CRC。
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in bytes {
            crc ^= b as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn oversized_pixel_header_rejected_before_full_decode() {
        // 编码一张真实可解的 1×1 PNG,再原地改写 IHDR 的 width/height 声明为超上限值
        // 并重算 chunk CRC——into_dimensions 只解头不解像素,超限判定必须发生在真正
        // 解码分配之前(深审裁决①:stat 只拦压缩字节,像素级设防补在此)。
        let mut bytes: Vec<u8> = Vec::new();
        {
            let img = image::RgbImage::from_pixel(1, 1, image::Rgb([0u8, 0, 0]));
            let mut cursor = std::io::Cursor::new(&mut bytes);
            image::DynamicImage::ImageRgb8(img)
                .write_to(&mut cursor, image::ImageFormat::Png)
                .unwrap();
        }
        // IHDR 是签名(8 字节)后第一个 chunk(PNG 规范强制):
        // [8..12]=length(13) [12..16]="IHDR" [16..20]=width [20..24]=height [29..33]=CRC。
        assert_eq!(
            &bytes[12..16],
            b"IHDR",
            "PNG 编码器必须把 IHDR 作为首个 chunk"
        );
        let (huge_w, huge_h): (u32, u32) = (20_000, 20_000); // 4e8 像素 > MAX_SOURCE_PIXELS(1e8)
        bytes[16..20].copy_from_slice(&huge_w.to_be_bytes());
        bytes[20..24].copy_from_slice(&huge_h.to_be_bytes());
        let crc = crc32(&bytes[12..29]); // chunk type(4) + data(13)
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());

        let dir = temp_dir("oversized-pixels");
        let path = dir.join("huge_header.png");
        std::fs::write(&path, &bytes).unwrap();

        let item = OcrItem {
            item_id: 1,
            cache_key: None,
            source_path: Some(path.to_string_lossy().into_owned()),
            fingerprint: "fp".into(),
        };
        let err = load_ocr_image(&item).unwrap_err();
        assert_eq!(
            err,
            WorkerErrorCode::ResourceLimit,
            "超像素上限应在解码前判 ResourceLimit(不连坐)"
        );
    }
}
