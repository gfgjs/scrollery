//! ONNX Runtime 会话池与模型装载。
//!
//! 动态库必须显式解析并通过 preflight，避免隐式搜索命中过旧运行时。
//! FP16 外部权重使用 Basic 图优化；禁用优化会产生类型错误，高级优化可能长时间阻塞装载。
//! 图像塔可使用 GPU；BERT 文本塔固定 CPU，避开 DirectML 的 int64 Gather 静默错误。
//! 张量 I/O、归一化与词表校验由模型契约和 clip 模块负责。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use tracing::{info, warn};

use crate::clip::ClipTokenizer;
use crate::error::Result;
use crate::face_profile::FaceProfile;
use crate::profile::{ClipRoles, ModelProfile};
use crate::provider::AiProvider;

/// 线程安全的 ONNX Runtime Session 池。
/// Session::run 需要可变借用，因此通过 channel 借出独立 Session，避免共享锁串行化。
#[derive(Clone)]
pub struct SessionPool {
    rx: Receiver<Session>,
    tx: Sender<Session>,
}

impl SessionPool {
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = bounded(capacity);
        Self { rx, tx }
    }

    pub fn push(&self, session: Session) {
        let _ = self.tx.send(session);
    }

    /// 未借出的 Session 数；启动时用于确定人脸推理并发数，可能小于请求容量。
    pub fn available(&self) -> usize {
        self.rx.len()
    }

    pub fn get(&self) -> Option<SessionGuard> {
        // 阻塞直到有可用 session
        match self.rx.recv() {
            Ok(session) => Some(SessionGuard {
                session: Some(session),
                tx: self.tx.clone(),
            }),
            Err(e) => {
                tracing::error!("Session pool channel disconnected: {}", e);
                None
            }
        }
    }
}

/// RAII 守卫:drop 时自动把 Session 归还池中。
pub struct SessionGuard {
    session: Option<Session>,
    tx: Sender<Session>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            let _ = self.tx.send(session);
        }
    }
}

impl std::ops::Deref for SessionGuard {
    type Target = Session;
    fn deref(&self) -> &Self::Target {
        self.session
            .as_ref()
            .expect("SessionGuard accessed after drop")
    }
}

impl std::ops::DerefMut for SessionGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.session
            .as_mut()
            .expect("SessionGuard accessed after drop")
    }
}

impl std::fmt::Debug for SessionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionPool")
            .field("available", &self.rx.len())
            .finish()
    }
}

/// 单段装载的后备超时，覆盖慢速磁盘与 GPU 内核编译。
/// 宿主按进度与心跳判断静默超时；动态库初始化另有 preflight 短 watchdog。
const SESSION_LOAD_TIMEOUT: Duration = Duration::from_secs(600);

// ORT 运行时预检

/// ORT 运行时 preflight 失败(三态映射协议错误码:unavailable→OrtDylibUnavailable,
/// timeout→OrtRuntimeInitTimeout,failed→同 timeout 归运行时本体病灶)。
#[derive(Debug, Clone, thiserror::Error)]
pub enum OrtPreflightError {
    /// 路径层不可用:env 指向不存在文件,或未设 env 且 exe 旁无库文件。
    #[error("ORT 动态库不可用:{0}")]
    DylibUnavailable(String),
    /// 装载卡死超出 watchdog 预算(如损坏的 DLL/System32 旧版无限阻塞)。
    #[error("ORT 运行时初始化超时:{0}")]
    RuntimeInitTimeout(String),
    /// 装载明确失败(libloading 报错:符号缺失/依赖 DLL 缺失等)。
    #[error("ORT 运行时初始化失败:{0}")]
    RuntimeInitFailed(String),
}

/// 平台动态库文件名(与 ort setup_api 的裸名默认一致)。
#[cfg(target_os = "windows")]
const ORT_DYLIB_NAME: &str = "onnxruntime.dll";
#[cfg(target_os = "linux")]
const ORT_DYLIB_NAME: &str = "libonnxruntime.so";
#[cfg(target_os = "macos")]
const ORT_DYLIB_NAME: &str = "libonnxruntime.dylib";

/// 解析动态库路径及其来源：非空 ORT_DYLIB_PATH 必须指向现有文件，
/// 未设置时仅检查可执行文件同目录，避免系统搜索链命中过旧运行时。
pub fn resolve_ort_dylib() -> std::result::Result<(PathBuf, &'static str), OrtPreflightError> {
    match std::env::var("ORT_DYLIB_PATH") {
        Ok(v) if !v.is_empty() => {
            let p = PathBuf::from(&v);
            if p.is_file() {
                Ok((p, "env ORT_DYLIB_PATH"))
            } else {
                Err(OrtPreflightError::DylibUnavailable(format!(
                    "env ORT_DYLIB_PATH 指向不存在的文件:{v}(常见成因:仓库配置里的失效路径/DLL 未随构建复制)"
                )))
            }
        }
        _ => {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .ok_or_else(|| {
                    OrtPreflightError::DylibUnavailable("无法定位当前 exe 目录".to_string())
                })?;
            let p = exe_dir.join(ORT_DYLIB_NAME);
            if p.is_file() {
                Ok((p, "exe 同目录"))
            } else {
                Err(OrtPreflightError::DylibUnavailable(format!(
                    "未设 ORT_DYLIB_PATH 且 exe 旁无 {ORT_DYLIB_NAME}(目录:{});拒绝回退系统 DLL 搜索——System32 旧版 1.17 会无限阻塞",
                    exe_dir.display()
                )))
            }
        }
    }
}

/// 急切装载 ORT，进程内缓存首次结果；失败时由调用方映射协议错误。
/// 装载不可中断，watchdog 超时后线程仍可能存活，交由 worker 退出回收。
pub fn preflight_ort_runtime(timeout: Duration) -> std::result::Result<String, OrtPreflightError> {
    static RESULT: std::sync::OnceLock<std::result::Result<String, OrtPreflightError>> =
        std::sync::OnceLock::new();
    RESULT
        .get_or_init(|| {
            let (path, source) = resolve_ort_dylib()?;
            let desc = format!("{}({source})", path.display());
            info!("ORT preflight:急切装载 {desc}");
            let (tx, rx) = std::sync::mpsc::channel();
            let path_clone = path.clone();
            std::thread::spawn(move || {
                // init_from 急切 libloading;commit 仅存环境配置(重复 commit 返回 false 无害)。
                let r = ort::init_from(&path_clone).map(|builder| {
                    builder.commit();
                });
                let _ = tx.send(r);
            });
            match rx.recv_timeout(timeout) {
                Ok(Ok(())) => {
                    info!("ORT preflight 通过:{desc}");
                    Ok(desc)
                }
                Ok(Err(e)) => Err(OrtPreflightError::RuntimeInitFailed(format!("{desc}:{e}"))),
                Err(_) => Err(OrtPreflightError::RuntimeInitTimeout(format!(
                    "{desc}:{}s 内未完成(疑似损坏/不兼容的运行时,装载线程已泄漏)",
                    timeout.as_secs()
                ))),
            }
        })
        .clone()
}

/// AI 推理引擎池。
pub struct AiEnginePool {
    /// 已探测到的最优执行提供者。
    pub provider: AiProvider,

    /// GPU 显示名称（CPU 时为空字符串）。
    pub gpu_name: String,

    /// 本池所加载模型的契约（驱动预处理 / I/O / 维度 / 分词器）。
    pub profile: ModelProfile,

    /// Chinese-CLIP 图像编码器 Session 池。
    pub clip_image_session: Option<SessionPool>,

    /// Chinese-CLIP 文本编码器 Session 池。
    pub clip_text_session: Option<SessionPool>,

    /// 缓存的 BERT 分词器（从 vocab.txt 加载）。
    /// 避免每次语义搜索都重新读取 vocab.txt。
    pub clip_tokenizer: Option<ClipTokenizer>,

    /// 人脸检测 Session 池（YuNet/SCRFD）；模型文件缺失则为 None（功能优雅降级）。
    pub face_detect_session: Option<SessionPool>,

    /// 人脸嵌入 Session 池（SFace/ArcFace）。
    pub face_embed_session: Option<SessionPool>,

    /// 已加载的人脸模型契约（驱动 F2 检测/嵌入的后处理分派）；None = 未加载人脸模型。
    pub face_profile: Option<FaceProfile>,
}

/// 装载阶段进度回调:参数为事件字符串 `<stage>:<event>`(如 `clip_image_load:begin(DirectML)`、
/// `face_embed_load:timeout(600s)`)。worker 侧转译成协议 Progress 帧/据 `:timeout` 分类错误码;
/// `None` = 静默(进程内调用/测试)。回调在装载调用线程同步执行,须轻量不阻塞。
pub type LoadProgress<'a> = Option<&'a dyn Fn(&str)>;

/// 向进度回调发射一个阶段事件(回调缺省时零开销)。
fn emit(progress: LoadProgress<'_>, event: &str) {
    if let Some(f) = progress {
        f(event);
    }
}

impl AiEnginePool {
    /// 按角色装载模型，并保留调用方已核实身份兼容的现有 Session。
    /// 阶段回调供 worker 发进度和心跳；未请求的角色不会访问模型文件。
    pub fn load_with_progress(
        existing: Option<Self>,
        models_dir: &Path,
        profile: &ModelProfile,
        roles: ClipRoles,
        face_profile: Option<&FaceProfile>,
        provider_override: &str,
        progress: LoadProgress<'_>,
    ) -> Result<Self> {
        let image_path = models_dir.join(&profile.image_file);
        let text_path = models_dir.join(&profile.text_file);

        // ── 步骤 1：提供者探测 ──────────────────────────────────────
        info!("Starting AI provider detection | 开始 AI 提供者探测...");
        emit(progress, "provider_detect:begin");
        let mut provider_info = crate::provider::detect_best_provider();

        if provider_override == "cpu" {
            info!("User override: Forcing CPU | 用户强制指定：使用 CPU");
            provider_info.provider = AiProvider::Cpu;
            provider_info.gpu_name = String::new();
        }

        let mut pool = existing.unwrap_or_else(|| Self {
            provider: provider_info.provider,
            gpu_name: provider_info.gpu_name,
            profile: profile.clone(),
            clip_image_session: None,
            clip_text_session: None,
            clip_tokenizer: None,
            face_detect_session: None,
            face_embed_session: None,
            face_profile: None,
        });

        let pool_size = match pool.provider {
            // CPU 单次推理使用全部核心，限制池容量以控制资源占用。
            AiProvider::Cpu => 2,
            _ => 1, // GPU 使用单 Session，避免多实例争用驱动资源。
        };

        // 人脸独立使用两个 Session，允许检测与嵌入并发；GPU 下仍需关注资源争用。
        let face_pool_size = match pool.provider {
            AiProvider::Cpu => 2,
            _ => 2,
        };

        // ── 步骤 2：加载 CLIP 模型 ──────────────────────────────────────
        // 图像编码器使用探测到的提供者，GPU 加载失败时再回退 CPU。
        if roles.image && pool.clip_image_session.is_none() {
            pool.clip_image_session = load_session_pool(
                &image_path,
                &pool.provider,
                "CLIP image encoder | CLIP 图像编码器",
                pool_size,
                "clip_image_load",
                progress,
            );

            // 已有 GPU 人脸池时保留提供者回声；仅新图像塔回退 CPU。
            if pool.clip_image_session.is_none() && pool.provider != AiProvider::Cpu {
                tracing::warn!("图像编码器 GPU 加载失败，回退 CPU");
                if !pool.face_ready() {
                    pool.provider = AiProvider::Cpu;
                    pool.gpu_name.clear();
                }
                pool.clip_image_session = load_session_pool(
                    &image_path,
                    &AiProvider::Cpu,
                    "CLIP image encoder (CPU) | CLIP 图像编码器 (CPU)",
                    2,
                    "clip_image_cpu_fallback",
                    progress,
                );
            }
            emit(progress, "contract_check:begin");
            if let Some(session) = &pool.clip_image_session {
                if let Some(guard) = session.get() {
                    crate::clip::verify_image_tower_contract(&guard, profile)?;
                }
            }
        }
        // BERT 的 int64 Gather 在 DirectML 上曾静默算错，文本塔固定 CPU 保证查询向量正确。
        if roles.text && pool.clip_text_session.is_none() {
            pool.clip_text_session = load_session_pool(
                &text_path,
                &AiProvider::Cpu,
                "CLIP text encoder (CPU forced) | CLIP 文本编码器 (强制 CPU)",
                2,
                "clip_text_load",
                progress,
            );
        }

        info!(
            "AI provider ready: {} ({}) | AI 提供者就绪: {} ({})",
            pool.provider.label(),
            pool.gpu_name,
            pool.provider.label(),
            pool.gpu_name
        );

        // ── 步骤 3：加载人脸模型（可选；文件缺失则优雅降级，不阻断 CLIP）──
        // 人脸 CNN 跟随图像侧提供者；检测与嵌入须同时就绪，否则整组不启用。
        if let Some(fp) = face_profile.filter(|_| !pool.face_ready()) {
            let det = load_session_pool(
                &models_dir.join(&fp.detect_file),
                &pool.provider,
                "Face detector | 人脸检测器",
                face_pool_size,
                "face_detect_load",
                progress,
            );
            let emb = load_session_pool(
                &models_dir.join(&fp.embed_file),
                &pool.provider,
                "Face embedder | 人脸嵌入器",
                face_pool_size,
                "face_embed_load",
                progress,
            );
            if det.is_some() && emb.is_some() {
                info!("Face models ready: {} | 人脸模型就绪: {}", fp.id, fp.id);
                pool.face_detect_session = det;
                pool.face_embed_session = emb;
                pool.face_profile = Some(fp.clone());
            } else {
                warn!(
                        "Face models not fully loaded (detector={}, embedder={}), face feature disabled | 人脸模型未完整加载，人脸功能禁用",
                        det.is_some(),
                        emb.is_some()
                    );
            }
        }

        Ok(pool)
    }

    /// 返回 `true` 如果 CLIP 图像编码器已加载。
    pub fn clip_image_ready(&self) -> bool {
        self.clip_image_session.is_some()
    }

    /// 返回 `true` 如果两个 CLIP 编码器都已加载。
    pub fn clip_ready(&self) -> bool {
        self.clip_image_session.is_some() && self.clip_text_session.is_some()
    }

    /// 返回 `true` 如果人脸检测器与嵌入器均已加载。
    pub fn face_ready(&self) -> bool {
        self.face_detect_session.is_some() && self.face_embed_session.is_some()
    }
}

/// 装载影像增强的单 Session 池；增强请求串行，提供者由调用方探测。
/// 缺文件、超时或装载失败返回 None，调用方据此报告 ModelLoadFailed。
pub fn load_enhance_session_pool(
    model_path: &Path,
    provider: &AiProvider,
    stage: &str,
    progress: LoadProgress<'_>,
) -> Option<SessionPool> {
    load_session_pool(
        &model_path.to_path_buf(),
        provider,
        "enhance model | 影像增强模型",
        1,
        stage,
        progress,
    )
}

// 辅助函数

// OCR 复用池装载、超时与进度处理，以 CPU 和单 Session 分别加载 det/cls/rec。
pub(crate) fn load_session_pool(
    model_path: &PathBuf,
    provider: &AiProvider,
    label: &str,
    pool_size: usize,
    stage: &str,
    progress: LoadProgress<'_>,
) -> Option<SessionPool> {
    if !model_path.exists() {
        warn!(
            "Model file not found, skipping {} | 模型文件未找到，跳过 {}: {:?}",
            label, label, model_path
        );
        emit(progress, &format!("{stage}:missing_file"));
        return None;
    }
    emit(
        progress,
        &format!("{stage}:begin({},pool={pool_size})", provider.label()),
    );

    info!(
        "Loading {} (pool size: {}) with provider {} | 正在用 {} 加载 {} (容量: {}): {:?}",
        label,
        pool_size,
        provider.label(),
        provider.label(),
        label,
        pool_size,
        model_path
    );

    let pool = SessionPool::new(pool_size);

    for i in 0..pool_size {
        let path_clone = model_path.clone();
        let provider_clone = provider.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = build_session(&path_clone, &provider_clone);
            let _ = tx.send(result);
        });

        match rx.recv_timeout(SESSION_LOAD_TIMEOUT) {
            Ok(Ok(session)) => {
                if i == 0 {
                    let input_names: Vec<&str> =
                        session.inputs().iter().map(|i| i.name()).collect();
                    let output_names: Vec<&str> =
                        session.outputs().iter().map(|o| o.name()).collect();
                    info!(
                        "Loaded {} [1/{}] — inputs: {:?}, outputs: {:?} | {} 加载成功 [1/{}] — 输入: {:?}, 输出: {:?}",
                        label, pool_size, input_names, output_names,
                        label, pool_size, input_names, output_names
                    );
                } else {
                    info!(
                        "Loaded {} [{}/{}] | {} 加载成功 [{}/{}]",
                        label,
                        i + 1,
                        pool_size,
                        label,
                        i + 1,
                        pool_size
                    );
                }
                pool.push(session);
                emit(progress, &format!("{stage}:slot_ok({}/{pool_size})", i + 1));
            }
            Ok(Err(e)) => {
                warn!("Failed to load {} [{}/{}], AI feature degraded | {} 加载失败 [{}/{}], AI 功能降级: {}", label, i + 1, pool_size, label, i + 1, pool_size, e);
                emit(progress, &format!("{stage}:fail({e})"));
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                warn!(
                    "Timeout loading {} [{}/{}] after {:?} | {} 加载超时 [{}/{}] ({:?})",
                    label,
                    i + 1,
                    pool_size,
                    SESSION_LOAD_TIMEOUT,
                    label,
                    i + 1,
                    pool_size,
                    SESSION_LOAD_TIMEOUT
                );
                // 事件尾缀 `:timeout` 是 worker 分类 SessionLoadTimeout 错误码的判据,勿改拼写。
                emit(
                    progress,
                    &format!("{stage}:timeout({}s)", SESSION_LOAD_TIMEOUT.as_secs()),
                );
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                warn!("Session loader thread panicked while loading {} [{}/{}] | 加载 {} 时 Session 加载线程崩溃 [{}/{}]", label, i + 1, pool_size, label, i + 1, pool_size);
                emit(progress, &format!("{stage}:loader_panicked"));
                break;
            }
        }
    }

    if pool.rx.is_empty() {
        None
    } else {
        if pool.rx.len() < pool_size {
            warn!(
                "{} pool loaded with degraded capacity: {}/{}",
                label,
                pool.rx.len(),
                pool_size
            );
        }
        Some(pool)
    }
}

/// 使用选定提供者对应的 EP 构建 Session。
/// DirectML 必须满足的约束：
///   - 单线程（intra_threads=1）
///   - 禁用内存模式优化
///   - 只使用 Basic 图优化（避免 ViT 模型 shader 预编译导致的无限期卡死）
fn build_session(model_path: &PathBuf, provider: &AiProvider) -> ort::Result<Session> {
    match provider {
        #[cfg(target_os = "windows")]
        AiProvider::DirectML => {
            // DirectML 需要顺序执行且不能使用内存模式优化。
            // 图优化级别必须限制为 Basic（Level1）——Level3 会触发完整的 DML shader 预编译，
            // 在含有复杂 Attention 或 Int64 算子的 ViT/Transformer 模型上会无限期卡死。
            let mut b = Session::builder()?
                .with_intra_threads(1)?
                .with_inter_threads(1)?
                .with_parallel_execution(false)?
                .with_optimization_level(GraphOptimizationLevel::Level1)?
                .with_memory_pattern(false)?
                .with_execution_providers([ort::ep::DirectML::default().build()])?;
            b.commit_from_file(model_path)
        }
        AiProvider::CUDA => {
            let mut b = Session::builder()?
                .with_intra_threads(1)?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_execution_providers([ort::ep::CUDA::default().build()])?;
            b.commit_from_file(model_path)
        }
        #[cfg(target_os = "macos")]
        AiProvider::CoreML => {
            let mut b = Session::builder()?
                .with_intra_threads(1)?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_execution_providers([ort::ep::CoreML::default().build()])?;
            b.commit_from_file(model_path)
        }
        AiProvider::OpenVINO => {
            let mut b = Session::builder()?
                .with_intra_threads(1)?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_execution_providers([ort::ep::OpenVINO::default().build()])?;
            b.commit_from_file(model_path)
        }
        _ => {
            // 外层仅一个推理线程，Session 使用全部核心，避免全量分析退化为单核。
            let cores = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            let mut b = Session::builder()?
                .with_intra_threads(cores as _)?
                .with_optimization_level(GraphOptimizationLevel::Level1)?;
            b.commit_from_file(model_path)
        }
    }
}

impl std::fmt::Debug for AiEnginePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiEnginePool")
            .field("provider", &self.provider)
            .field("gpu_name", &self.gpu_name)
            .field("clip_image_ready", &self.clip_image_session.is_some())
            .field("clip_text_ready", &self.clip_text_session.is_some())
            .field("face_ready", &self.face_ready())
            .finish()
    }
}
