// crates/exotic-workers/ai-worker/src/session.rs
//! SessionInit 的校验与加载(D1 §3 完整性 + D3 §2「模型加载不进握手」)。
//!
//! 两段式:[`validate_and_resolve`](纯校验,零 ort,可单测)→ [`load`](ort Session 构建)。
//! 任一校验不过即 `ModelLoadFailed`(terminal——重试同一载荷无意义,host 标记待重下载)。
//!
//! 校验清单(全部对**明文**):
//!   1. `models_root` canonicalize 可达;
//!   2. `arch_id`/`face_profile_id` 在共享注册表可解析,snapshot 文件名与契约一致;
//!   3. 每个 [`ModelDescriptor`]:handle 为 Path(Named=AES ④ 通道,T14 后置未启用)、
//!      canonicalize 后以 models_root 为前缀(防宿主被劫持后诱导任意读)、按 role 对应
//!      契约文件名、字节数与 sha256 逐一相符;
//!   4. 角色集完备:ImageEncoder+TextEncoder 必备(CLIP 成对加载,与 AiEnginePool 行为
//!      对齐);FaceDetect+FaceRecog 成对可选,且必须与 `face_profile_id` 的有无一致。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use exotic_protocol::{
    ModelDescriptor, ModelHandle, ModelProfileSnapshot, ModelRole, WorkerErrorCode,
};
use scrollery_ai_core::clip::ClipTokenizer;
use scrollery_ai_core::engine::AiEnginePool;
use scrollery_ai_core::face_profile::{find_face_profile, FaceProfile};
use scrollery_ai_core::profile::{resolve_profile, ModelProfile};

/// 初始化失败 =(稳定错误码, 诊断消息)。消息不含完整绝对路径(协议红线)。
pub type InitError = (WorkerErrorCode, String);

/// 校验通过、待 ort 加载的会话描述。
#[derive(Debug)]
pub struct ResolvedSession {
    pub session_id: u64,
    pub models_root: PathBuf,
    pub profile: ModelProfile,
    pub face_profile: Option<FaceProfile>,
    pub ai_cache_dir: PathBuf,
    pub image_provider: String,
    pub batch_size: u32,
}

/// 已加载的推理会话(worker 端唯一可服务状态;严格串行下同一时刻至多一个)。
pub struct SessionState {
    pub session_id: u64,
    pub pool: AiEnginePool,
    pub profile: ModelProfile,
    pub face_profile: Option<FaceProfile>,
    pub ai_cache_dir: PathBuf,
    /// EmbedBatch 单批 items 上限(host 声明的快照;超限= host bug,回 MalformedInput)。
    pub batch_size: u32,
    /// CLIP 分词器(EncodeText 用;词表从 models_root 按 profile 契约加载,T17)。
    pub tokenizer: ClipTokenizer,
}

fn fail(msg: impl Into<String>) -> InitError {
    (WorkerErrorCode::ModelLoadFailed, msg.into())
}

/// 纯校验段:解析契约 + 逐模型完整性。不触 ort。
pub fn validate_and_resolve(
    session_id: u64,
    models: &[ModelDescriptor],
    snapshot: &ModelProfileSnapshot,
    models_root: &str,
    ai_cache_dir: &str,
    image_provider: &str,
) -> Result<ResolvedSession, InitError> {
    let root = std::fs::canonicalize(models_root)
        .map_err(|e| fail(format!("models_root 不可达:{}", e.kind())))?;

    // 契约解析:arch_id + 选定 batch 变体文件;text_file 必须与架构契约一致。
    let profile = resolve_profile(&snapshot.arch_id, Some(&snapshot.image_file))
        .ok_or_else(|| fail(format!("未知 arch_id:{}", snapshot.arch_id)))?;
    if profile.text_file != snapshot.text_file {
        return Err(fail(format!(
            "text_file 与架构契约不符:{} != {}",
            snapshot.text_file, profile.text_file
        )));
    }

    let face_profile = match &snapshot.face_profile_id {
        Some(id) => {
            Some(find_face_profile(id).ok_or_else(|| fail(format!("未知 face_profile_id:{id}")))?)
        }
        None => None,
    };

    // 角色 → 契约文件名(全部必备:CLIP 成对;声明了 face profile 则检测/嵌入成对)。
    let mut expected: Vec<(ModelRole, &str)> = vec![
        (ModelRole::ImageEncoder, profile.image_file.as_str()),
        (ModelRole::TextEncoder, profile.text_file.as_str()),
    ];
    if let Some(fp) = &face_profile {
        expected.push((ModelRole::FaceDetect, fp.detect_file.as_str()));
        expected.push((ModelRole::FaceRecog, fp.embed_file.as_str()));
    }

    let mut seen: HashSet<&str> = HashSet::new();
    for desc in models {
        let expected_file = expected
            .iter()
            .find(|(role, _)| *role == desc.role)
            .map(|(_, f)| *f)
            .ok_or_else(|| {
                fail(format!(
                    "未预期的模型角色:{:?}(与 profile 快照不符)",
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
            "模型角色不完备:声明 {}/{}(CLIP 成对必备;face 按 profile 成对)",
            seen.len(),
            expected.len()
        )));
    }

    Ok(ResolvedSession {
        session_id,
        models_root: root,
        profile,
        face_profile,
        ai_cache_dir: PathBuf::from(ai_cache_dir),
        image_provider: image_provider.to_string(),
        batch_size: snapshot.batch_size.max(1),
    })
}

/// 单个模型载荷的完整性校验:Path 通道 + 归属 + 文件名 + len + sha256。
/// `pub(crate)`(T5,OCR 会话复用):OCR 四件套(det/cls/rec/dict)与 CLIP 共享同一份
/// Path 通道/归属/文件名/len/sha256 校验纪律,`ocr.rs::validate_ocr_init` 直接调用。
pub(crate) fn verify_descriptor(
    desc: &ModelDescriptor,
    expected_file: &str,
    root: &Path,
) -> Result<(), InitError> {
    let path = match &desc.handle {
        ModelHandle::Path(p) => PathBuf::from(p),
        // AES 共享内存通道随 ④ 变现启用(T14 后置);在此之前收到即协议误用。
        ModelHandle::Named(_) => {
            return Err(fail(format!(
                "named 载荷通道未启用(AES 随 ④):role={:?}",
                desc.role
            )))
        }
    };
    let canon = std::fs::canonicalize(&path)
        .map_err(|e| fail(format!("模型文件不可达:{}({:?})", e.kind(), desc.role)))?;
    if !canon.starts_with(root) {
        return Err(fail(format!(
            "模型路径越界 models_root:role={:?}",
            desc.role
        )));
    }
    if canon.file_name().and_then(|n| n.to_str()) != Some(expected_file) {
        return Err(fail(format!(
            "模型文件名与契约不符:role={:?} 期望 {expected_file}",
            desc.role
        )));
    }
    let meta =
        std::fs::metadata(&canon).map_err(|e| fail(format!("模型文件 stat 失败:{}", e.kind())))?;
    if meta.len() != desc.len {
        return Err(fail(format!(
            "模型字节数不符:role={:?} 实际 {} != 声明 {}",
            desc.role,
            meta.len(),
            desc.len
        )));
    }
    let actual = sha256_file(&canon).map_err(|e| fail(format!("sha256 计算失败:{}", e.kind())))?;
    if actual != desc.sha256.to_lowercase() {
        return Err(fail(format!("模型 sha256 不符:role={:?}", desc.role)));
    }
    Ok(())
}

/// ort 加载段:构建 Session 池并复核「声明的角色都真正就绪」。
/// 冷加载可达分钟级(ViT-L + DirectML 内核编译)。2026-07-11 加固批 A-2 起本函数在
/// 装载线程上执行并经 `progress` 回执阶段事件(main 转译为协议 Progress 帧),宿主
/// watchdog 相应改「静默限时」——300s 总限时的黑盒等待成为历史。
pub fn load_with_progress(
    resolved: ResolvedSession,
    progress: scrollery_ai_core::engine::LoadProgress<'_>,
) -> Result<SessionState, InitError> {
    let pool = AiEnginePool::init_with_progress(
        &resolved.models_root,
        &resolved.profile,
        resolved.face_profile.as_ref(),
        &resolved.image_provider,
        progress,
    )
    .map_err(|e| fail(format!("引擎初始化失败:{e}")))?;

    // AiEnginePool 对缺失文件是「优雅降级」语义;会话语义下声明即必须就绪,降级=加载失败。
    if !pool.clip_ready() {
        return Err(fail("CLIP 编码器未能加载(见 stderr 日志)".to_string()));
    }
    if resolved.face_profile.is_some() && !pool.face_ready() {
        return Err(fail("人脸模型未能完整加载(见 stderr 日志)".to_string()));
    }

    // 分词器与文本塔同属会话必备(TextEncoder 是必备角色,EncodeText 是搜索必经载体,
    // T17):host 侧 variant_installed 已把 vocab 列为安装前提,此处缺失即载荷不完整。
    let tokenizer = ClipTokenizer::from_profile(&resolved.models_root, &resolved.profile)
        .map_err(|e| fail(format!("分词器加载失败:{e}")))?;

    Ok(SessionState {
        session_id: resolved.session_id,
        pool,
        profile: resolved.profile,
        face_profile: resolved.face_profile,
        ai_cache_dir: resolved.ai_cache_dir,
        batch_size: resolved.batch_size,
        tokenizer,
    })
}

/// 流式 sha256(模型可达 GB 级,不整读进内存),输出 64 位小写 hex。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrollery_ai_core::profile::DEFAULT_PROFILE_ID;

    /// 每测试独立临时目录(进程 id + 名字),避免并行测试互踩。
    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ai-worker-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 在 root 下落一个契约文件并返回其合法 descriptor。
    fn lay_model(root: &Path, role: ModelRole, file: &str, content: &[u8]) -> ModelDescriptor {
        let p = root.join(file);
        std::fs::write(&p, content).unwrap();
        ModelDescriptor {
            role,
            handle: ModelHandle::Path(p.to_string_lossy().into_owned()),
            len: content.len() as u64,
            sha256: sha256_file(&p).unwrap(),
            model_id: None,
        }
    }

    fn default_snapshot() -> ModelProfileSnapshot {
        // 用共享注册表的默认架构,文件名从契约取(而非硬编码),注册表变更时测试自适应。
        let prof = resolve_profile(DEFAULT_PROFILE_ID, None).unwrap();
        ModelProfileSnapshot {
            arch_id: DEFAULT_PROFILE_ID.into(),
            image_file: prof.image_file,
            text_file: prof.text_file,
            batch_size: 16,
            face_profile_id: None,
        }
    }

    fn clip_pair(root: &Path, snap: &ModelProfileSnapshot) -> Vec<ModelDescriptor> {
        vec![
            lay_model(root, ModelRole::ImageEncoder, &snap.image_file, b"img"),
            lay_model(root, ModelRole::TextEncoder, &snap.text_file, b"txt"),
        ]
    }

    #[test]
    fn happy_path_clip_only_resolves() {
        let root = temp_dir("happy");
        let snap = default_snapshot();
        let models = clip_pair(&root, &snap);
        let r = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c:/ai", "cpu")
            .expect("合法载荷应通过校验");
        assert_eq!(r.session_id, 1);
        assert_eq!(r.batch_size, 16);
        assert!(r.face_profile.is_none());
        assert_eq!(r.profile.id, DEFAULT_PROFILE_ID);
    }

    #[test]
    fn named_handle_rejected_until_aes() {
        let root = temp_dir("named");
        let snap = default_snapshot();
        let mut models = clip_pair(&root, &snap);
        models[0].handle = ModelHandle::Named("pn-00-ff".into());
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("named"), "diagnostic: {}", e.1);
    }

    #[test]
    fn path_outside_models_root_rejected() {
        let root = temp_dir("outside-root");
        let elsewhere = temp_dir("outside-elsewhere");
        let snap = default_snapshot();
        let mut models = clip_pair(&root, &snap);
        // 把 image 塔换成 root 之外的同名合法文件 → 归属校验必须拦下。
        models[0] = lay_model(
            &elsewhere,
            ModelRole::ImageEncoder,
            &snap.image_file,
            b"img",
        );
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("越界"), "diagnostic: {}", e.1);
    }

    #[test]
    fn sha256_mismatch_rejected() {
        let root = temp_dir("sha");
        let snap = default_snapshot();
        let mut models = clip_pair(&root, &snap);
        models[1].sha256 = "0".repeat(64);
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("sha256"), "diagnostic: {}", e.1);
    }

    #[test]
    fn len_mismatch_rejected() {
        let root = temp_dir("len");
        let snap = default_snapshot();
        let mut models = clip_pair(&root, &snap);
        models[0].len += 1;
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("字节数"), "diagnostic: {}", e.1);
    }

    #[test]
    fn unknown_arch_rejected() {
        let root = temp_dir("arch");
        let mut snap = default_snapshot();
        snap.arch_id = "no-such-arch".into();
        let models = clip_pair(&root, &default_snapshot());
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("arch_id"), "diagnostic: {}", e.1);
    }

    #[test]
    fn missing_text_role_rejected() {
        let root = temp_dir("missing-text");
        let snap = default_snapshot();
        let models = vec![lay_model(
            &root,
            ModelRole::ImageEncoder,
            &snap.image_file,
            b"img",
        )];
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("不完备"), "diagnostic: {}", e.1);
    }

    #[test]
    fn face_role_without_face_profile_rejected() {
        let root = temp_dir("face-undeclared");
        let snap = default_snapshot(); // face_profile_id = None
        let mut models = clip_pair(&root, &snap);
        models.push(lay_model(
            &root,
            ModelRole::FaceDetect,
            "face_detection_yunet_2023mar.onnx",
            b"det",
        ));
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("未预期"), "diagnostic: {}", e.1);
    }

    #[test]
    fn face_pair_with_declared_profile_resolves() {
        let root = temp_dir("face-pair");
        let mut snap = default_snapshot();
        snap.face_profile_id = Some("yunet-sface".into());
        let fp = find_face_profile("yunet-sface").unwrap();
        let mut models = clip_pair(&root, &snap);
        models.push(lay_model(
            &root,
            ModelRole::FaceDetect,
            &fp.detect_file,
            b"det",
        ));
        models.push(lay_model(
            &root,
            ModelRole::FaceRecog,
            &fp.embed_file,
            b"emb",
        ));
        let r = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .expect("成对人脸角色应通过");
        assert!(r.face_profile.is_some());
    }

    #[test]
    fn duplicate_role_rejected() {
        let root = temp_dir("dup");
        let snap = default_snapshot();
        let mut models = clip_pair(&root, &snap);
        models.push(models[0].clone());
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu")
            .unwrap_err();
        assert!(e.1.contains("重复"), "diagnostic: {}", e.1);
    }
}
