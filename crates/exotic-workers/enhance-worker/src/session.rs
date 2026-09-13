// crates/exotic-workers/enhance-worker/src/session.rs
//! EnhanceSessionInit 的校验与装载(对齐 ai-worker session.rs::validate_and_resolve
//! 的单件完整性纪律:Path 通道 + 文件名契约 + 字节数 + sha256)。
//!
//! 两段式:[`validate_enhance_init`](纯校验,零 ort,可单测)→ [`build_enhance_sessions`]
//! (ort Session 池构建,复用 ai-core engine 骨架)。
//!
//! # 与 CLIP SessionInit 的差异
//! EnhanceSessionInit 载荷是**单角色多档位**(全 `ModelRole::Enhance`,靠 `model_id`
//! 寻址具体档),而非 CLIP 的「按角色取各自 handle」。故校验清单:
//!   1. 每 descriptor `role == Enhance`(否则 MalformedInput);
//!   2. `model_id` 为 `Some` 且能在 `enhance_profiles` 注册表解析(否则 MalformedInput);
//!   3. 单件完整性:Path 通道(Named=AES ④ 未启用)、canonicalize 可达、以 `models_root`
//!      canonicalize 结果为前缀(防宿主被劫持后诱导任意读,对齐 ai-worker
//!      `session.rs::validate_and_resolve` 先例)、文件名 ∈ profile 的
//!      `{id}-fp32.onnx` / `{id}-fp16.onnx`、字节数、sha256(否则 ModelLoadFailed)。
//!
//! # 输出白名单前缀
//! `work_dir` canonicalize 后存会话态,`EnhanceRun.output_tmp_path` 据此做越界拒绝
//! (见 run.rs::resolve_output_path)。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use exotic_protocol::{ModelDescriptor, ModelHandle, ModelRole, WorkerErrorCode};
use scrollery_ai_core::engine::{load_enhance_session_pool, LoadProgress};
use scrollery_ai_core::enhance::EnhanceSessions;
use scrollery_ai_core::enhance_profile::{find_enhance_profile, EnhanceProfile};
use scrollery_ai_core::provider::{detect_best_provider, AiProvider};

/// 初始化失败 =(稳定错误码, 诊断消息)。消息不含完整绝对路径(协议红线)。
pub type InitError = (WorkerErrorCode, String);

/// 角色/寻址类校验失败 → MalformedInput(host bug)。
fn malformed(msg: impl Into<String>) -> InitError {
    (WorkerErrorCode::MalformedInput, msg.into())
}

/// 单件完整性校验失败 → ModelLoadFailed(terminal;重试同一载荷无意义,host 标记待重下载)。
fn load_failed(msg: impl Into<String>) -> InitError {
    (WorkerErrorCode::ModelLoadFailed, msg.into())
}

/// 校验通过、待 ort 加载的增强会话描述。
#[derive(Debug)]
pub struct ResolvedEnhance {
    pub session_id: u64,
    /// canonicalize 后的输出白名单前缀。
    pub work_dir: PathBuf,
    /// 逐模型:(契约, canonicalize 后的权重路径)。
    pub models: Vec<(EnhanceProfile, PathBuf)>,
}

/// 已装载的增强会话(worker 端唯一可服务状态;严格串行下同一时刻至多一个)。
pub struct EnhanceSessionState {
    pub session_id: u64,
    pub sessions: EnhanceSessions,
    /// canonicalize 后的输出白名单前缀。
    pub work_dir: PathBuf,
    /// 实际探测/选用的执行提供器(日志回声用;增强会话无专属就绪体,gpu_name 暂不回报)。
    pub provider: AiProvider,
    /// 已装载模型档数(日志/诊断用;EnhanceSessions 不外露 len)。
    pub model_count: usize,
}

/// 纯校验段:角色/寻址 + 逐模型完整性(含 models_root 归属)+ work_dir 归属根。不触 ort。
pub fn validate_enhance_init(
    session_id: u64,
    models: &[ModelDescriptor],
    work_dir: &str,
    models_root: &str,
) -> Result<ResolvedEnhance, InitError> {
    if models.is_empty() {
        return Err(malformed("EnhanceSessionInit.models 为空"));
    }
    let work_dir = std::fs::canonicalize(work_dir)
        .map_err(|e| malformed(format!("work_dir 不可达:{}", e.kind())))?;
    let models_root = std::fs::canonicalize(models_root)
        .map_err(|e| malformed(format!("models_root 不可达:{}", e.kind())))?;

    let mut resolved = Vec::with_capacity(models.len());
    let mut seen: HashSet<&str> = HashSet::new();
    for desc in models {
        if desc.role != ModelRole::Enhance {
            return Err(malformed(format!(
                "EnhanceSessionInit 只接受 Enhance 角色,实得 {:?}",
                desc.role
            )));
        }
        let model_id = desc
            .model_id
            .as_deref()
            .ok_or_else(|| malformed("Enhance descriptor 缺 model_id(无法寻址档位)"))?;
        // 同 model_id 重复声明 = host bug。
        if !seen.insert(model_id) {
            return Err(malformed(format!("model_id 重复声明:{model_id}")));
        }
        let profile = find_enhance_profile(model_id)
            .ok_or_else(|| malformed(format!("未知 model_id(注册表无此档):{model_id}")))?;
        let path = verify_enhance_descriptor(desc, &profile, &models_root)?;
        resolved.push((profile, path));
    }

    Ok(ResolvedEnhance {
        session_id,
        work_dir,
        models: resolved,
    })
}

/// 单个增强模型载荷的完整性校验:Path 通道 + models_root 归属 + 文件名契约 + len + sha256。
/// 返回 canonicalize 后的权重路径。对齐 ai-worker session.rs::verify_descriptor,
/// 差异:文件名校验对 profile 的 fp32/fp16 双候选(而非单一契约文件名)。
fn verify_enhance_descriptor(
    desc: &ModelDescriptor,
    profile: &EnhanceProfile,
    models_root: &Path,
) -> Result<PathBuf, InitError> {
    let path = match &desc.handle {
        ModelHandle::Path(p) => PathBuf::from(p),
        // AES 共享内存通道随 ④ 变现启用(未启用);收到即协议误用。
        ModelHandle::Named(_) => {
            return Err(load_failed(format!(
                "named 载荷通道未启用(AES 随 ④):model_id={}",
                profile.id
            )))
        }
    };
    let canon = std::fs::canonicalize(&path).map_err(|e| {
        load_failed(format!(
            "模型文件不可达:{}(model_id={})",
            e.kind(),
            profile.id
        ))
    })?;
    // 归属校验:防宿主被劫持后诱导 worker 读 models_root 之外的任意文件(D1 §3,
    // 对齐 CLIP/OCR validate_and_resolve 先例)。
    if !canon.starts_with(models_root) {
        return Err(load_failed(format!(
            "模型路径越界 models_root:model_id={}",
            profile.id
        )));
    }
    // 文件名须为该档的 fp32 或 fp16 权重(GPU/CPU 双份,design.md §E)。
    let name = canon.file_name().and_then(|n| n.to_str());
    if name != Some(profile.file_fp32.as_str()) && name != Some(profile.file_fp16.as_str()) {
        return Err(load_failed(format!(
            "模型文件名与契约不符:model_id={} 期望 {} 或 {}",
            profile.id, profile.file_fp32, profile.file_fp16
        )));
    }
    let meta = std::fs::metadata(&canon)
        .map_err(|e| load_failed(format!("模型文件 stat 失败:{}", e.kind())))?;
    if meta.len() != desc.len {
        return Err(load_failed(format!(
            "模型字节数不符:model_id={} 实际 {} != 声明 {}",
            profile.id,
            meta.len(),
            desc.len
        )));
    }
    let actual =
        sha256_file(&canon).map_err(|e| load_failed(format!("sha256 计算失败:{}", e.kind())))?;
    if actual != desc.sha256.to_lowercase() {
        return Err(load_failed(format!(
            "模型 sha256 不符:model_id={}",
            profile.id
        )));
    }
    Ok(canon)
}

/// ort 加载段:探测 provider,逐模型建 SessionPool 填入 [`EnhanceSessions`]。
/// `progress` 回执阶段事件(main 转译为协议 Progress 帧);任一模型池装载失败即
/// ModelLoadFailed(声明即必须就绪,不做优雅降级——增强链缺一档即断)。
pub fn build_enhance_sessions(
    resolved: ResolvedEnhance,
    progress: LoadProgress<'_>,
) -> Result<EnhanceSessionState, InitError> {
    // provider 探测(design.md §E:DirectML→CUDA→CoreML→OpenVINO→CPU 既有链)。
    let info = detect_best_provider();
    let provider = info.provider;

    let mut sessions = EnhanceSessions::new();
    let model_count = resolved.models.len();
    for (profile, path) in &resolved.models {
        let stage = format!("enhance_load:{}", profile.id);
        let pool =
            load_enhance_session_pool(path, &provider, &stage, progress).ok_or_else(|| {
                load_failed(format!("模型 {} 会话装载失败(见 stderr 日志)", profile.id))
            })?;
        sessions.insert(profile.id.clone(), pool);
    }

    Ok(EnhanceSessionState {
        session_id: resolved.session_id,
        sessions,
        work_dir: resolved.work_dir,
        provider,
        model_count,
    })
}

/// 流式 sha256(模型可达数十 MB,不整读进内存),输出 64 位小写 hex。
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
    use exotic_protocol::ModelHandle;

    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("enhance-worker-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 在 root 下落一个契约文件并返回其合法 descriptor(用 profile 的 fp16 文件名)。
    fn lay_model(root: &Path, model_id: &str, file: &str, content: &[u8]) -> ModelDescriptor {
        let p = root.join(file);
        std::fs::write(&p, content).unwrap();
        ModelDescriptor {
            role: ModelRole::Enhance,
            handle: ModelHandle::Path(p.to_string_lossy().into_owned()),
            len: content.len() as u64,
            sha256: sha256_file(&p).unwrap(),
            model_id: Some(model_id.to_string()),
        }
    }

    #[test]
    fn happy_path_resolves() {
        let root = temp_dir("happy");
        let prof = find_enhance_profile("scunet").unwrap();
        let m = lay_model(&root, "scunet", &prof.file_fp16, b"weights");
        let r = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .expect("合法载荷应通过校验");
        assert_eq!(r.session_id, 1);
        assert_eq!(r.models.len(), 1);
        assert_eq!(r.models[0].0.id, "scunet");
    }

    #[test]
    fn non_enhance_role_rejected_malformed() {
        let root = temp_dir("role");
        let prof = find_enhance_profile("scunet").unwrap();
        let mut m = lay_model(&root, "scunet", &prof.file_fp16, b"weights");
        m.role = ModelRole::ImageEncoder;
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn missing_model_id_rejected_malformed() {
        let root = temp_dir("noid");
        let prof = find_enhance_profile("scunet").unwrap();
        let mut m = lay_model(&root, "scunet", &prof.file_fp16, b"weights");
        m.model_id = None;
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn unknown_model_id_rejected_malformed() {
        let root = temp_dir("unknown");
        // 文件名随便,先撞 model_id 未知(在文件名校验之前)。
        let m = lay_model(
            &root,
            "no-such-model",
            "no-such-model-fp16.onnx",
            b"weights",
        );
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
        assert!(e.1.contains("未知 model_id"), "diagnostic: {}", e.1);
    }

    #[test]
    fn named_handle_rejected_load_failed() {
        let root = temp_dir("named");
        let prof = find_enhance_profile("scunet").unwrap();
        let mut m = lay_model(&root, "scunet", &prof.file_fp16, b"weights");
        m.handle = ModelHandle::Named("pn-00-ff".into());
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("named"), "diagnostic: {}", e.1);
    }

    #[test]
    fn wrong_filename_rejected_load_failed() {
        let root = temp_dir("fname");
        // 文件名与 scunet 契约(scunet-fp32/fp16.onnx)不符。
        let m = lay_model(&root, "scunet", "scunet-wrong.onnx", b"weights");
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("文件名"), "diagnostic: {}", e.1);
    }

    #[test]
    fn sha256_mismatch_rejected_load_failed() {
        let root = temp_dir("sha");
        let prof = find_enhance_profile("drunet").unwrap();
        let mut m = lay_model(&root, "drunet", &prof.file_fp32, b"weights");
        m.sha256 = "0".repeat(64);
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("sha256"), "diagnostic: {}", e.1);
    }

    #[test]
    fn len_mismatch_rejected_load_failed() {
        let root = temp_dir("len");
        let prof = find_enhance_profile("fbcnn").unwrap();
        let mut m = lay_model(&root, "fbcnn", &prof.file_fp16, b"weights");
        m.len += 1;
        let e = validate_enhance_init(1, &[m], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("字节数"), "diagnostic: {}", e.1);
    }

    #[test]
    fn duplicate_model_id_rejected_malformed() {
        let root = temp_dir("dup");
        let prof = find_enhance_profile("scunet").unwrap();
        let m1 = lay_model(&root, "scunet", &prof.file_fp16, b"weights");
        let m2 = m1.clone();
        let e = validate_enhance_init(1, &[m1, m2], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
        assert!(e.1.contains("重复"), "diagnostic: {}", e.1);
    }

    #[test]
    fn empty_models_rejected_malformed() {
        let root = temp_dir("empty");
        let e = validate_enhance_init(1, &[], root.to_str().unwrap(), root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::MalformedInput);
    }

    #[test]
    fn path_outside_models_root_rejected_load_failed() {
        // 模型文件真实存在、契约齐全,但落在 models_root 之外 → 归属校验必须拦下
        // (防宿主被劫持后诱导 worker 读任意路径,对齐 CLIP/OCR validate_and_resolve 先例)。
        let work_dir = temp_dir("outside-work");
        let elsewhere = temp_dir("outside-elsewhere");
        let prof = find_enhance_profile("scunet").unwrap();
        let m = lay_model(&elsewhere, "scunet", &prof.file_fp16, b"weights");
        let models_root = temp_dir("outside-root");
        let e = validate_enhance_init(
            1,
            &[m],
            work_dir.to_str().unwrap(),
            models_root.to_str().unwrap(),
        )
        .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("越界"), "diagnostic: {}", e.1);
    }

    #[test]
    fn dotdot_traversal_outside_models_root_rejected_load_failed() {
        // handle 路径用 `..` 穿越出 models_root 子目录、指向根外的同名合法文件——
        // canonicalize 后仍须被前缀校验拦下(不能靠字符串层面误判为"在 root 下")。
        let models_root = temp_dir("dotdot-root");
        let sibling = temp_dir("dotdot-sibling");
        let prof = find_enhance_profile("scunet").unwrap();
        let real = lay_model(&sibling, "scunet", &prof.file_fp16, b"weights");
        let sibling_name = sibling.file_name().unwrap().to_str().unwrap();
        let traversal_path = models_root
            .join("..")
            .join(sibling_name)
            .join(&prof.file_fp16);
        assert!(traversal_path.to_str().unwrap().contains(".."));
        let mut m = real;
        m.handle = ModelHandle::Path(traversal_path.to_string_lossy().into_owned());
        let e = validate_enhance_init(
            1,
            &[m],
            models_root.to_str().unwrap(),
            models_root.to_str().unwrap(),
        )
        .unwrap_err();
        assert_eq!(e.0, WorkerErrorCode::ModelLoadFailed);
        assert!(e.1.contains("越界"), "diagnostic: {}", e.1);
    }
}
