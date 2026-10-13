// crates/exotic-workers/ai-worker/src/session.rs
//! SessionInit 完整性校验与模型装载；模型加载在握手之后进行。
//!
//! 先由 validate_and_resolve 校验，再由 load_with_progress 构建 ORT Session。
//! 任一校验不过即 `ModelLoadFailed`(terminal——重试同一载荷无意义,host 标记待重下载)。
//!
//! 校验清单(全部对**明文**):
//!   1. `models_root` canonicalize 可达;
//!   2. `arch_id`/`face_profile_id` 在共享注册表可解析,snapshot 文件名与契约一致;
//!   3. 每个 [`ModelDescriptor`] 仅接受 Path 通道；
//!      canonicalize 后以 models_root 为前缀(防宿主被劫持后诱导任意读)、按 role 对应
//!      契约文件名、字节数逐一相符；实际新装载的模型还须核对 sha256；
//!   4. 非空角色集：图像/文本编码器可独立声明；人脸检测/识别成对，
//!      且必须与 `face_profile_id` 的有无一致。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use exotic_protocol::{
    ModelDescriptor, ModelHandle, ModelProfileSnapshot, ModelRole, WorkerErrorCode,
};
use scrollery_ai_core::clip::ClipTokenizer;
use scrollery_ai_core::engine::AiEnginePool;
use scrollery_ai_core::face_profile::{find_face_profile, FaceProfile};
use scrollery_ai_core::profile::{resolve_profile, ClipRoles, ModelProfile};

/// 初始化失败 =(稳定错误码, 诊断消息)。消息不含完整绝对路径(协议红线)。
pub type InitError = (WorkerErrorCode, String);

/// 校验通过、待 ort 加载的会话描述。
#[derive(Debug, Clone)]
pub struct ResolvedSession {
    pub session_id: u64,
    pub models_root: PathBuf,
    pub profile: ModelProfile,
    pub face_profile: Option<FaceProfile>,
    pub ai_cache_dir: PathBuf,
    pub image_provider: String,
    pub batch_size: u32,
    pub roles: ClipRoles,
    pub models: Vec<ModelDescriptor>,
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
    /// EncodeText 分词器，词表按 profile 从 models_root 加载。
    pub tokenizer: Option<ClipTokenizer>,
    /// 图像模型的真实固定 batch；动态模型为 None，用于避免子批重复补齐。
    pub image_fixed_batch: Option<usize>,
    /// 装载身份用于补载时核对路径、提供者、契约及已加载文件的哈希。
    pub identity: ResolvedSession,
}

fn fail(msg: impl Into<String>) -> InitError {
    (WorkerErrorCode::ModelLoadFailed, msg.into())
}

/// 纯校验段：解析契约与逐模型完整性，不触 ort。
/// 补载时只有整个会话可复用且描述符未变的已加载角色跳过内容重读。
pub fn validate_and_resolve(
    session_id: u64,
    models: &[ModelDescriptor],
    snapshot: &ModelProfileSnapshot,
    models_root: &str,
    ai_cache_dir: &str,
    image_provider: &str,
    previous: Option<&ResolvedSession>,
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

    // 角色 → 契约文件名；CLIP 双塔独立，人脸仍按 profile 成对。
    let mut expected: Vec<(ModelRole, &str)> = vec![
        (ModelRole::ImageEncoder, profile.image_file.as_str()),
        (ModelRole::TextEncoder, profile.text_file.as_str()),
    ];
    if let Some(fp) = &face_profile {
        expected.push((ModelRole::FaceDetect, fp.detect_file.as_str()));
        expected.push((ModelRole::FaceRecog, fp.embed_file.as_str()));
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut paths = Vec::with_capacity(models.len());
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
        paths.push(resolve_descriptor_path(desc, expected_file, &root)?);
    }
    if models.is_empty()
        || face_profile.as_ref().is_some_and(|fp| {
            !seen.contains(fp.detect_file.as_str()) || !seen.contains(fp.embed_file.as_str())
        })
    {
        return Err(fail(
            "模型角色不完备:至少声明一个角色；face 按 profile 成对",
        ));
    }
    let roles = ClipRoles {
        image: models.iter().any(|d| d.role == ModelRole::ImageEncoder),
        text: models.iter().any(|d| d.role == ModelRole::TextEncoder),
    };

    let resolved = ResolvedSession {
        session_id,
        models_root: root,
        profile,
        face_profile,
        ai_cache_dir: PathBuf::from(ai_cache_dir),
        image_provider: image_provider.to_string(),
        batch_size: snapshot.batch_size.max(1),
        roles,
        models: models.to_vec(),
    };
    // 引擎只补载缺少的角色；已在内存中的 Session 不会再次打开模型文件。
    // 必须先确认整池可复用，否则提供者或契约变化会重载全部角色，仍需逐件验 hash。
    let reusable = previous.filter(|old| old.can_reuse(&resolved));
    for (desc, path) in models.iter().zip(paths) {
        if !reusable.is_some_and(|old| old.models.contains(desc)) {
            verify_descriptor_hash(desc, &path)?;
        }
    }
    Ok(resolved)
}

/// 单个模型载荷的完整性校验:Path 通道 + 归属 + 文件名 + len + sha256。
/// OCR 的 det/cls/rec/dict 复用同一校验，避免与 CLIP 的安全边界分叉。
pub(crate) fn verify_descriptor(
    desc: &ModelDescriptor,
    expected_file: &str,
    root: &Path,
) -> Result<(), InitError> {
    let canon = resolve_descriptor_path(desc, expected_file, root)?;
    verify_descriptor_hash(desc, &canon)
}

fn resolve_descriptor_path(
    desc: &ModelDescriptor,
    expected_file: &str,
    root: &Path,
) -> Result<PathBuf, InitError> {
    let path = match &desc.handle {
        ModelHandle::Path(p) => PathBuf::from(p),
        // Named 通道未启用，收到时按协议误用拒绝。
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
    Ok(canon)
}

fn verify_descriptor_hash(desc: &ModelDescriptor, path: &Path) -> Result<(), InitError> {
    let actual = sha256_file(path).map_err(|e| fail(format!("sha256 计算失败:{}", e.kind())))?;
    if actual != desc.sha256.to_lowercase() {
        return Err(fail(format!("模型 sha256 不符:role={:?}", desc.role)));
    }
    Ok(())
}

/// 构建 Session 池并复核所有声明角色已就绪。
/// 冷加载可能耗时较长，由装载线程报告进度，宿主按静默时长判断超时。
pub fn load_with_progress(
    resolved: ResolvedSession,
    previous: Option<SessionState>,
    progress: scrollery_ai_core::engine::LoadProgress<'_>,
) -> Result<SessionState, InitError> {
    // 身份不兼容时先卸载旧池；补载只保留相同文件的 Session，避免并存两套大模型。
    let previous = previous.filter(|old| old.identity.can_reuse(&resolved));
    let (existing, mut tokenizer, previous_models) = match previous {
        Some(old) => (Some(old.pool), old.tokenizer, old.identity.models),
        None => (None, None, Vec::new()),
    };
    let pool = AiEnginePool::load_with_progress(
        existing,
        &resolved.models_root,
        &resolved.profile,
        resolved.roles,
        resolved.face_profile.as_ref(),
        &resolved.image_provider,
        progress,
    )
    .map_err(|e| fail(format!("引擎初始化失败:{e}")))?;

    // AiEnginePool 对缺失文件是「优雅降级」语义;会话语义下声明即必须就绪,降级=加载失败。
    if (resolved.roles.image && !pool.clip_image_ready())
        || (resolved.roles.text && pool.clip_text_session.is_none())
    {
        return Err(fail("CLIP 编码器未能加载(见 stderr 日志)".to_string()));
    }
    if resolved.face_profile.is_some() && !pool.face_ready() {
        return Err(fail("人脸模型未能完整加载(见 stderr 日志)".to_string()));
    }

    if resolved.roles.text && tokenizer.is_none() {
        tokenizer = Some(
            ClipTokenizer::from_profile(&resolved.models_root, &resolved.profile)
                .map_err(|e| fail(format!("分词器加载失败:{e}")))?,
        );
    }

    let image_fixed_batch = pool
        .clip_image_session
        .as_ref()
        .map(|p| scrollery_ai_core::clip::fixed_image_batch(p, &resolved.profile))
        .transpose()
        .map_err(|e| fail(format!("图像 batch 契约读取失败:{e}")))?
        .flatten();

    let mut identity = resolved.clone();
    for model in previous_models {
        if !identity.models.iter().any(|d| d.role == model.role) {
            identity.models.push(model);
        }
    }
    let face_profile = pool.face_profile.clone();
    Ok(SessionState {
        session_id: resolved.session_id,
        pool,
        profile: resolved.profile,
        face_profile,
        ai_cache_dir: resolved.ai_cache_dir,
        batch_size: resolved.batch_size,
        tokenizer,
        image_fixed_batch,
        identity,
    })
}

impl ResolvedSession {
    /// 已有角色只有在契约、根目录、提供者及重叠文件身份相同时才能复用。
    fn can_reuse(&self, next: &Self) -> bool {
        self.models_root == next.models_root
            && self.image_provider == next.image_provider
            && self.profile.id == next.profile.id
            && self.profile.image_file == next.profile.image_file
            && self.profile.text_file == next.profile.text_file
            && match (&self.face_profile, &next.face_profile) {
                (Some(a), Some(b)) => a.id == b.id,
                _ => true,
            }
            && next.models.iter().all(|model| {
                self.models
                    .iter()
                    .find(|old| old.role == model.role)
                    .is_none_or(|old| old == model)
            })
    }
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
        let e = validate_and_resolve(1, &models, &snap, root.to_str().unwrap(), "c", "cpu", None)
            .unwrap_err();
        assert!(e.1.contains("越界"), "diagnostic: {}", e.1);
    }

    fn resolved_image(
        root: &Path,
        snap: &ModelProfileSnapshot,
        models: &[ModelDescriptor],
    ) -> ResolvedSession {
        validate_and_resolve(
            1,
            &models[..1],
            snap,
            root.to_str().unwrap(),
            "c",
            "cpu",
            None,
        )
        .unwrap()
    }

    #[test]
    fn extending_session_checks_new_model_hash() {
        let root = temp_dir("reuse-new-hash");
        let snap = default_snapshot();
        let mut models = clip_pair(&root, &snap);
        let previous = resolved_image(&root, &snap, &models);
        models[1].sha256 = "0".repeat(64);
        let error = validate_and_resolve(
            2,
            &models,
            &snap,
            root.to_str().unwrap(),
            "c",
            "cpu",
            Some(&previous),
        )
        .unwrap_err();
        assert!(error.1.contains("sha256"));
    }

    #[test]
    fn incompatible_session_rechecks_previously_loaded_model() {
        let root = temp_dir("reload-hash");
        let snap = default_snapshot();
        let models = clip_pair(&root, &snap);
        let previous = resolved_image(&root, &snap, &models);
        std::fs::write(root.join(&snap.image_file), b"new").unwrap();
        // 提供者变化会卸载旧池，不能沿用只针对内存中 Session 的免重读规则。
        let error = validate_and_resolve(
            2,
            &models,
            &snap,
            root.to_str().unwrap(),
            "c",
            "auto",
            Some(&previous),
        )
        .unwrap_err();
        assert!(error.1.contains("sha256"));
    }
}
