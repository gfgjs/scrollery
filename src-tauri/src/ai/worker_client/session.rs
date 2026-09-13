// src-tauri/src/ai/worker_client/session.rs
//! B 组:会话协议组装(CLIP 会话 + OCR 会话,D-OCR-1 双槽独立)。

use super::*;

impl AiWorkerClient {
    /// 确保 worker 会话与 `spec` 一致,返回会话快照(embed_dim 等校验参数来源)。
    /// 快照匹配时零帧直接返回;不符先 close 再 init(切换语义)。
    ///
    /// 失败按 [`EnsureError`] 二分:进程级(死管道/超时/协议违例)由 `run_validated`
    /// 的 attempt 圈重建重试;数据级(模型加载失败等 worker 明确拒绝)重试同死,直接冒泡。
    pub(super) fn ensure_session(
        &mut self,
        spec: &SessionSpec,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<SessionDescriptor, EnsureError> {
        // spawn 失败(exe 缺失等)是确定性环境问题,重试无意义 → 数据级。
        self.ensure_worker().map_err(EnsureError::Terminal)?;
        // 借用分离:先只读比对,需要切换时再取 &mut。
        let need_switch = {
            let w = self.worker.as_ref().expect("ensure_worker 后必有实例");
            match w.session() {
                Some(desc) if spec.matches(desc) => return Ok(desc.clone()),
                Some(_) => true,
                None => false,
            }
        };
        if need_switch {
            let w = self.worker.as_mut().expect("上方已确保实例");
            let _ = w.close_session(op_timeouts::SESSION_CLOSE, cancelled);
            // close 的进程级失败已由 Supervisor kill;重建后继续走 init。
            self.ensure_worker().map_err(EnsureError::Terminal)?;
        }

        let session_id = self.next_session_id;
        self.next_session_id += 1;
        let req = build_session_init(session_id, spec, &mut self.sha_cache)
            .map_err(EnsureError::Terminal)?;

        let w = self.worker.as_mut().expect("上方已确保实例");
        match w.init_session(&req, op_timeouts::SESSION_INIT, cancelled) {
            RawOutcome::Success { .. } => w.session().cloned().ok_or_else(|| {
                EnsureError::Terminal(AppError::Internal("init_session 成功但无会话快照".into()))
            }),
            // worker 明确拒绝(ModelLoadFailed 等):数据级,重建重发同死。
            RawOutcome::Failure(fb) => Err(EnsureError::Terminal(AppError::System(format!(
                "AI worker 会话初始化失败[{}]:{}",
                fb.code.as_str(),
                fb.message
            )))),
            // 进程级三态(2026-07-10 审查 W1):典型来源是 worker 空闲 300s 自杀后 host
            // `alive` 旗标陈旧——init 写死管道得 Disconnected。必须进重试圈重建重发,
            // 否则「上轮结束超 5 分钟后首次操作必失败一次」(间歇性,极难排查)。
            RawOutcome::TimedOut => Err(EnsureError::Process(AppError::System(
                "AI worker 会话初始化超时(实例已回收)".into(),
            ))),
            RawOutcome::Disconnected => Err(EnsureError::Process(AppError::System(
                "AI worker 会话初始化中断开".into(),
            ))),
            RawOutcome::Protocol(msg) => Err(EnsureError::Process(AppError::System(format!(
                "AI worker 会话初始化协议违例:{msg}"
            )))),
        }
    }

    /// 确保 worker OCR 会话与 `spec` 一致(与 [`ensure_session`](Self::ensure_session)
    /// **同构**,但目标是独立的 OCR 会话槽——CLIP 双塔与 OCR 双槽在 worker 内并存,D-OCR-1)。
    /// `ocr_loaded == Some(profile.id)` 即零帧返回;否则发 `OcrSessionInit`(经 `run_batch`
    /// 底层 run_request 通吃任意 RequestBody)。失败按 [`EnsureError`] 二分,消费同
    /// [`ocr_batch`](Self::ocr_batch) 的 attempt 圈。
    pub(super) fn ensure_ocr_session(
        &mut self,
        spec: &OcrSessionSpec,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<(), EnsureError> {
        self.ensure_worker().map_err(EnsureError::Terminal)?;
        // 换代/自杀后 ocr_loaded 已在 drop_worker/ensure_worker 复位;此处只比对在载 id。
        if self.ocr_loaded.as_deref() == Some(spec.profile.id.as_str()) {
            return Ok(());
        }
        let session_id = self.next_session_id;
        self.next_session_id += 1;
        let req = build_ocr_session_init(session_id, spec, &mut self.sha_cache)
            .map_err(EnsureError::Terminal)?;

        let w = self.worker.as_mut().expect("ensure_worker 后必有实例");
        match w.run_batch(&req, op_timeouts::OCR_SESSION_INIT, cancelled) {
            RawOutcome::Success { body, .. } if body.ocr_session.is_some() => {
                self.ocr_loaded = Some(spec.profile.id.clone());
                Ok(())
            }
            // Success 但缺应答体 = worker 协议实现错(不可重试同死):数据级。
            RawOutcome::Success { .. } => Err(EnsureError::Terminal(AppError::Internal(
                "OcrSessionInit 成功但缺 ocr_session 应答体".into(),
            ))),
            // 模型加载失败(sha 不符/文件缺失/ort 构建失败):类型化判别(非字符串匹配,
            // `fb.code` 本就是 WorkerErrorCode 枚举),直出用户可操作的稳定码——引导前往
            // 设置页重下载,而非笼统 System(2026-07-23 深审#1)。
            RawOutcome::Failure(fb)
                if fb.code == exotic_protocol::WorkerErrorCode::ModelLoadFailed =>
            {
                Err(EnsureError::Terminal(AppError::Ocr {
                    code: "ocr_model_missing",
                    message: "OCR 模型加载失败，请重新下载对应档位".into(),
                }))
            }
            // worker 明确拒绝(其余角色/校验类错误):数据级,重建重发同死。
            RawOutcome::Failure(fb) => Err(EnsureError::Terminal(AppError::System(format!(
                "OCR 会话初始化失败[{}]:{}",
                fb.code.as_str(),
                fb.message
            )))),
            // 进程级三态(同 ensure_session W1):空闲自杀后 alive 旗标陈旧 → init 写死管道。
            RawOutcome::TimedOut => Err(EnsureError::Process(AppError::System(
                "OCR 会话初始化超时(实例已回收)".into(),
            ))),
            RawOutcome::Disconnected => Err(EnsureError::Process(AppError::System(
                "OCR 会话初始化中断开".into(),
            ))),
            RawOutcome::Protocol(msg) => Err(EnsureError::Process(AppError::System(format!(
                "OCR 会话初始化协议违例:{msg}"
            )))),
        }
    }
}

/// 组装 SessionInit 请求:按 profile 契约列模型角色(CLIP 成对必备;face 按 spec 成对),
/// 逐模型带 len+sha256(D1 §3;sha 经 mtime+len 备忘,GB 级文件不重算)。
pub(super) fn build_session_init(
    session_id: u64,
    spec: &SessionSpec,
    sha_cache: &mut HashMap<PathBuf, ShaEntry>,
) -> Result<RequestBody> {
    let mut models = vec![
        model_descriptor(
            ModelRole::ImageEncoder,
            &spec.models_dir.join(&spec.profile.image_file),
            sha_cache,
        )?,
        model_descriptor(
            ModelRole::TextEncoder,
            &spec.models_dir.join(&spec.profile.text_file),
            sha_cache,
        )?,
    ];
    if let Some(fp) = &spec.face_profile {
        models.push(model_descriptor(
            ModelRole::FaceDetect,
            &spec.models_dir.join(&fp.detect_file),
            sha_cache,
        )?);
        models.push(model_descriptor(
            ModelRole::FaceRecog,
            &spec.models_dir.join(&fp.embed_file),
            sha_cache,
        )?);
    }
    Ok(RequestBody::SessionInit {
        session_id,
        models,
        model_profile: ModelProfileSnapshot {
            arch_id: spec.profile.id.clone(),
            image_file: spec.profile.image_file.clone(),
            text_file: spec.profile.text_file.clone(),
            batch_size: spec.batch_size,
            face_profile_id: spec.face_profile.as_ref().map(|f| f.id.clone()),
        },
        models_root: spec.models_dir.to_string_lossy().into_owned(),
        ai_cache_dir: spec.ai_cache_dir.to_string_lossy().into_owned(),
        image_provider: spec.image_provider.clone(),
    })
}

/// 组装 OcrSessionInit 请求:四角色四文件(OcrDet/OcrCls/OcrRec/OcrDict,文件名单源取自
/// [`OcrProfile`](scrollery_ai_core::ocr_profile::OcrProfile)),逐件带 len+sha256
/// (复用同一 sha 备忘)。与 [`build_session_init`] 同构,角色集换 OCR 四件。
fn build_ocr_session_init(
    session_id: u64,
    spec: &OcrSessionSpec,
    sha_cache: &mut HashMap<PathBuf, ShaEntry>,
) -> Result<RequestBody> {
    let p = &spec.profile;
    let models = vec![
        model_descriptor(
            ModelRole::OcrDet,
            &spec.models_dir.join(&p.det_file),
            sha_cache,
        )?,
        model_descriptor(
            ModelRole::OcrCls,
            &spec.models_dir.join(&p.cls_file),
            sha_cache,
        )?,
        model_descriptor(
            ModelRole::OcrRec,
            &spec.models_dir.join(&p.rec_file),
            sha_cache,
        )?,
        model_descriptor(
            ModelRole::OcrDict,
            &spec.models_dir.join(&p.dict_file),
            sha_cache,
        )?,
    ];
    Ok(RequestBody::OcrSessionInit {
        session_id,
        models,
        ocr_profile_id: p.id.clone(),
        models_root: spec.models_dir.to_string_lossy().into_owned(),
    })
}

/// 单模型描述:stat 取 len/mtime → sha 备忘命中即复用,否则流式重算。
/// 文件缺失 → AiModelNotLoaded(可操作:请先下载),与 set_active_model 语义一致。
fn model_descriptor(
    role: ModelRole,
    path: &Path,
    sha_cache: &mut HashMap<PathBuf, ShaEntry>,
) -> Result<ModelDescriptor> {
    let meta = std::fs::metadata(path).map_err(|_| {
        AppError::AiModelNotLoaded(format!(
            "模型文件缺失,请先下载:{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
        ))
    })?;
    let len = meta.len();
    let mtime = meta.modified().map_err(AppError::Io)?;

    let hit = sha_cache
        .get(path)
        .is_some_and(|e| e.len == len && e.mtime == mtime);
    if !hit {
        let hex = crate::utils::hash::sha256_hex_of_file(path).map_err(AppError::Io)?;
        sha_cache.insert(path.to_path_buf(), ShaEntry { len, mtime, hex });
    }
    let sha256 = sha_cache
        .get(path)
        .map(|e| e.hex.clone())
        .expect("上方必已插入");

    Ok(ModelDescriptor {
        role,
        handle: ModelHandle::Path(path.to_string_lossy().into_owned()),
        len,
        sha256,
        model_id: None,
    })
}

/// 从应用状态组装会话规格(worker 派发与搜索共用):models_dir 沿用 runtime_config 推导,
/// ai_cache_dir = `{cache_dir}/ai_thumbs`(与 `thumbnail::cache::ai_cache_path` 同构),
/// provider 取 `ai_provider_override` 配置(缺省 auto),batch 取 pipeline 的统一解析。
///
/// A2:`ai_provider_override` 是 schema 设置类键,唯一真源已切到 `ConfigManager`——不再需要
/// 读池连接。
pub fn build_session_spec(
    state: &crate::state::AppState,
    profile: ModelProfile,
    face_profile: Option<FaceProfile>,
) -> SessionSpec {
    let models_dir = crate::ai::runtime_config::models_dir(state);
    let ai_cache_dir = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .cache_dir
        .join("ai_thumbs");
    let image_provider = state
        .config
        .get("ai_provider_override")
        .unwrap_or_else(|| "auto".to_string());
    let batch_size = crate::ai::pipeline::resolve_batch_size(state, &profile) as u32;
    SessionSpec {
        profile,
        face_profile,
        models_dir,
        ai_cache_dir,
        image_provider,
        batch_size,
    }
}

/// 从应用状态组装 OCR 会话规格:活跃档位读 config 键 `ocr_active_tier`(缺省
/// [`DEFAULT_OCR_PROFILE_ID`](scrollery_ai_core::ocr_profile::DEFAULT_OCR_PROFILE_ID)),
/// models_dir 取法同 [`build_session_spec`]。
///
/// 注:`ocr_active_tier` 此处仅**读**——`ConfigManager::get` 对未登记键回退 None,
/// `unwrap_or` 兜底默认,读侧无需 schema 白名单;设置页切档的**写**侧才需登记(归 T10)。
pub fn build_ocr_session_spec(state: &crate::state::AppState) -> Result<OcrSessionSpec> {
    use scrollery_ai_core::ocr_profile::{find_ocr_profile, DEFAULT_OCR_PROFILE_ID};
    let tier = state
        .config
        .get("ocr_active_tier")
        .unwrap_or_else(|| DEFAULT_OCR_PROFILE_ID.to_string());
    // 版本演进韧性:已下线/改名的档位值不硬错——warn 后回退默认(缺键与非法值同归默认)。
    let profile = find_ocr_profile(&tier).unwrap_or_else(|| {
        warn!("未知 OCR 档位 `{tier}`(已下线/改名?)→ 回退默认 {DEFAULT_OCR_PROFILE_ID}");
        scrollery_ai_core::ocr_profile::default_ocr_profile()
    });
    let models_dir = crate::ai::runtime_config::models_dir(state);
    Ok(OcrSessionSpec {
        profile,
        models_dir,
    })
}
