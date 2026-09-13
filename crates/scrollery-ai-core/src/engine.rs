// crates/scrollery-ai-core/src/engine.rs
//! AI 推理引擎池 — 封装用于 CLIP 模型的 ort Session。
//!
//! # 踩坑记录（2026-06-03）
//!
//! ## 坑1：ort crate 的 `load-dynamic` 与 `download-binaries` 互斥
//! `load-dynamic` feature 会激活 `ort-sys/disable-linking`，
//! 导致 build.rs 提前返回，`download-binaries` **完全不运行**。
//! **后果**：即使在 Cargo.toml 同时写了两个 feature，DLL 也不会自动下载。
//! **应对**：必须手动管理 DLL，并通过 ORT_DYLIB_PATH 指定路径。
//!
//! ## 坑2：ORT DLL 版本必须 ≥ 1.19（ONNX IR v10 要求）
//! Chinese-CLIP ViT-B/16 模型用 PyTorch 2.11 导出，ONNX IR version = 10。
//! ORT 1.17（旧版/WebView2 System32 自带版本）不支持 IR v10，
//! 会在 CreateSession 时报 "model IR version is higher than supported" 错误，
//! 或在某些路径下**直接无限卡死不报错**。
//!
//! ## 坑3：ORT 版本与 FP16 外部数据格式兼容性
//! - ORT 1.21：无法加载 eisneim/cn-clip_vit-b-16 的 FP16 外部数据格式
//!   （`.onnx` + `.extra_file`），在 `disabled` 和 `Level1` 图优化下均**无限卡死**。
//!   Node.js ORT 1.26 在相同模型 421ms 内快速失败并报 `GetIndexFromName` 错误。
//! - ORT 1.26：正常加载，Level1 优化下 ~200ms。
//!
//! **应对**：从 `onnxruntime-node@1.26.0` 的 `bin/napi-v6/win32/x64/` 中复制 DLL。
//!
//! ## 坑4：FP16 模型在 disabled 图优化下的类型错误
//! `GraphOptimizationLevel::Disable` 下加载 FP16 模型会报：
//! "Type (tensor(float)) of output arg (InsertedPrecisionFreeCast_...) does not match
//! expected type (tensor(float16))"
//! **原因**：FP16 模型内部有 ORT 插入的 PrecisionFreeCast 节点，这些节点依赖
//! Level1+ 的 SimplifiedLayerNormFusion 优化才能正确处理类型。
//! **应对**：必须使用 `Level1`（Basic）或更高级别，**不能用 Disable**。
//!
//! ## 坑5：单体 fp32 ONNX 格式（330MB）在 CPU 上极慢
//! ORT 加载单体格式时必须一次性反序列化整个 Protobuf，
//! 即使 `GraphOptimizationLevel::Disable`，330MB 文件也需要 >5 分钟。
//! **应对**：使用外部数据格式（`.onnx` header + `.extra_file` 权重），
//! ORT 通过内存映射按需读取权重，Session 创建只需解析小 header 文件。
//!
//! ## 坑6：CPU 路径不能用 Level3 图优化
//! Level3（ORT_ENABLE_ALL）对 330MB ViT-B/16 图执行完整图融合和布局变换，
//! 首次加载可能需要 5–10 分钟（无缓存）。
//! **应对**：CPU 路径使用 Level1（Basic）= 常量折叠 + 死节点消除，
//! Session 创建时间为秒级，推理性能影响极小。
//!
//! ## 坑7：新旧模型的张量 I/O 接口完全不同
//! - 旧模型（cn-clip-vit-b16-*.onnx）:
//!   图像: `pixel_values: f32[1,3,224,224]` → `image_features: f32[1,512]`（已L2归一化）
//!   文本: `input_ids + attention_mask + token_type_ids: i64[1,52]` → `text_features`
//! - 新模型（eisneim/cn-clip_vit-b-16）:
//!   图像: `image: f32[1,3,224,224]` → `unnorm_image_features: f32[1,512]`（未归一化！）
//!   文本: `text: i64[1,52]`（仅 token IDs）→ `unnorm_text_features: f32[1,512]`
//!
//! **应对**：推理后必须手动 L2 归一化；文本编码器不再需要 attention_mask/token_type_ids。
//!
//! ## 坑8（致命）：vocab.txt 与模型不匹配 → 准确率降为随机
//! `eisneim/cn-clip_vit-b-16` 仓库附带的 vocab.txt 是**英文 CLIP 的 BPE 词表**
//! （~5594 tokens），而非 Chinese-CLIP 需要的 **bert-base-chinese 词表**
//! （21128 tokens）。误用导致所有中文字符被编码为 [UNK]，
//! 任何查询产生完全相同的嵌入向量（cosine=1.0），搜索准确率降至随机水平。
//! **应对**：从模型原始作者 `OFA-Sys/chinese-clip-vit-base-patch16` 获取 vocab.txt；
//! 加载后校验 vocab_size ≥ 10000，否则立即报错。
//! **详细记录**：见 `clip.rs` 模块头部的踩坑记录。
//!
//! ## 坑9（致命）：DirectML 静默算错文本编码器 → 语义搜索结果错乱
//! `eisneim/cn-clip_vit-b-16` 的 **文本** 编码器是 BERT，输入为 int64 token ids，含
//! embedding `Gather` 等算子。在 **DirectML** 上这些算子会被**静默算错**（不报错、不回退），
//! 产出被污染的查询向量；图像（ViT）侧不受影响。症状：同一组（DirectML 生成且正确的）图像
//! 向量下，DirectML 文本排序与 CPU 排序毫无重合——“白色的猫”返回纸箱/快递、“黄色的猫”返回
//! 大量纯黑图，而 CPU 下均正确。重构前的旧三输入文本模型不触发此问题。
//! **应对**：**文本编码器固定走 CPU**（见 `init()`）。它很小、每次查询只跑一次，CPU 延迟
//! （~200ms）可忽略；图像编码器仍走 GPU 以保证批量分析速度。
//! **排查关键**：若“图像向量正确（CPU 复算结果正确）但 app 搜索错乱”，优先怀疑查询向量（文本侧 EP）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use tracing::{info, warn};

use crate::clip::ClipTokenizer;
use crate::error::Result;
use crate::face_profile::FaceProfile;
use crate::profile::ModelProfile;
use crate::provider::AiProvider;

/// 线程安全的 ONNX Runtime Session 池。
/// 用于解决 ort rc.12 中 Session::run 需要 &mut self 导致的串行瓶颈:session 经 channel 借出/归还,而非共享。
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

    /// 当前池中可用（未借出）的 session 数。流水线启动瞬间无人借出 → 等于池实际容量
    /// （可能因部分加载失败而小于请求值）。人脸据此决定 detect/embed 并发 worker 数（问题6c）。
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

/// 会话加载超时时间。
///
/// FP16 外部数据格式（eisneim/cn-clip_vit-b-16）在 ORT 1.26 + CPU + Level1 下
/// 加载仅需 ~200ms；设 600s 超时是为了应对极端情况（NAS/慢速 HDD）或
/// DirectML shader 编译（DirectML 卡死是无限期的，600s 内会被捕获）。
/// 2026-07-11 加固批 A 起,这只是**单段后备上界**:宿主侧改静默限时(心跳在途即不杀),
/// dylib 级卡死已被 [`preflight_ort_runtime`] 的短 watchdog 前置拦截。
const SESSION_LOAD_TIMEOUT: Duration = Duration::from_secs(600);

// ── ORT 运行时 preflight(2026-07-11 加固批 A-1:死路径快败)──────────────────
//
// 病灶背景:ort `load-dynamic` 的懒加载在首个 Session::builder 时才解析动态库——
// env `ORT_DYLIB_PATH` 指向不存在文件、或裸名回退命中 System32 的 1.17 旧版时,
// 装载线程**无限阻塞而非快败**(0 CPU 零日志,2026-07-10/11 夜事故的直接杀伤机制)。
// 对策:worker 侧**自解析**路径(拒绝隐式 DLL 搜索回退)+ `ort::init_from` 急切装载
// + 短 watchdog,把「无限静默」变成秒级可判别错误。

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
#[cfg(any(target_os = "linux", target_os = "android"))]
const ORT_DYLIB_NAME: &str = "libonnxruntime.so";
#[cfg(any(target_os = "macos", target_os = "ios"))]
const ORT_DYLIB_NAME: &str = "libonnxruntime.dylib";

/// 解析 ort 将使用的动态库路径。规则与 ort 懒加载一致但**收紧**:
/// ① env `ORT_DYLIB_PATH` 非空 → 该路径必须真实存在(死路径即错,不静默回退);
/// ② 未设 env → exe 同目录的 [`ORT_DYLIB_NAME`] 必须存在——**刻意拒绝**裸名交给
///    系统 DLL 搜索链的回退(会命中 System32 的 WebView2 遗留 1.17,无限阻塞,坑2)。
/// 返回 (绝对路径, 来源描述)。
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

/// 一次性急切装载 ORT 运行时(进程级缓存,重复调用直接返回首次结果)。
/// 成功后 ort 全局库句柄已就位,后续 `Session::builder` 不再走 env 懒解析;
/// 失败则调用方应把错误映射为协议级错误码立即回报,**不要**再让加载路径去撞无限阻塞。
/// watchdog 超时的装载线程会泄漏(ort 装载不可中断)——进程级一次、可接受,与
/// `load_session_pool` 超时泄漏同性质。
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
    /// 从给定的模型目录初始化引擎池。
    pub fn init(
        models_dir: &Path,
        profile: &ModelProfile,
        face_profile: Option<&FaceProfile>,
        provider_override: &str,
    ) -> Result<Self> {
        Self::init_with_progress(models_dir, profile, face_profile, provider_override, None)
    }

    /// 同 [`Self::init`],但带装载阶段进度回调(2026-07-11 加固批 A-2:
    /// worker 借此把「卡在哪段」实时回执给宿主,替代黑盒 300s 总限时)。
    pub fn init_with_progress(
        models_dir: &Path,
        profile: &ModelProfile,
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

        let pool_size = match provider_info.provider {
            // CPU 路径：流水线是单推理线程的，所以给单个 Session 分配全核心，将池子大小限制为 2
            AiProvider::Cpu => 2,
            _ => 1, // GPU:DirectML/CUDA 驱动自行处理内部并发,多 session 会导致严重的 DX12 锁争用
        };

        // 人脸专用 pool 尺寸（问题6c 提速）：独立于 CLIP 的 `pool_size`，不影响 CLIP。
        // ⚠️ GPU 也给 2，试图让人脸 detect/embed 多 session 并发吃满 GPU——这与上面「GPU 多 session
        // 致严重 DX12 锁争用」的结论相悖，是计划明知风险、要求用户实测的尝试。人脸是轻量纯 CNN
        // （112/640 输入），争用代价可能低于 CLIP 大 ViT；若实测变慢/不稳，把此处改回 1 即可。
        let face_pool_size = match provider_info.provider {
            AiProvider::Cpu => 2,
            _ => 2,
        };

        // ── 步骤 2：加载 CLIP 模型 ──────────────────────────────────────
        // 图像编码器：用探测到的最优 provider。ViT 在 DirectML/CUDA 上结果正确且快得多
        // —— 已用库内向量产出准确搜索结果验证。
        let mut clip_image_session = load_session_pool(
            &image_path,
            &provider_info.provider,
            "CLIP image encoder | CLIP 图像编码器",
            pool_size,
            "clip_image_load",
            progress,
        );

        // 【坑9·致命·2026-06-17】eisneim cn-clip 的 BERT 文本编码器含 int64 的 embedding
        // Gather 等算子，**DirectML 会静默算错**（不报错、不回退），产出被污染的查询向量，使
        // 语义搜索结果完全错乱。实测：同一组（DirectML 生成且正确的）图像向量下，app 的
        // DirectML 文本排序与 CPU 排序毫无重合——“白色的猫”在 DirectML 下返回纸箱/快递，
        // 在 CPU 下正确返回白猫；“黄色的猫”在 DirectML 下返回大量纯黑图。重构前的旧文本模型
        // （input_ids/attention_mask/token_type_ids 三输入）不触发此问题。
        // 文本模型很小、每次查询只跑一次，CPU 延迟（~200ms）可忽略，故固定 CPU 保证正确性。
        // 图像编码器不受影响（ViT 在 DirectML 上结果正确）。
        let clip_text_session = load_session_pool(
            &text_path,
            &AiProvider::Cpu,
            "CLIP text encoder (CPU forced) | CLIP 文本编码器 (强制 CPU)",
            2,
            "clip_text_load",
            progress,
        );

        // 图像编码器 GPU 加载失败则回退 CPU（文本已是 CPU）。
        if clip_image_session.is_none() && provider_info.provider != AiProvider::Cpu {
            tracing::warn!("GPU image encoder failed to load, falling back to CPU | 图像编码器 GPU 加载失败，回退 CPU...");
            provider_info.provider = AiProvider::Cpu;
            provider_info.gpu_name = String::new();
            clip_image_session = load_session_pool(
                &image_path,
                &AiProvider::Cpu,
                "CLIP image encoder (CPU) | CLIP 图像编码器 (CPU)",
                2,
                "clip_image_cpu_fallback",
                progress,
            );
        }

        // 装载期契约自检(2026-07-10 审查 K1):错配模型文件(手动导入/改名错放)在激活时
        // 即拒,给出指向明确的错误;否则要等运行期形状断言,且旧版本会静默产出错切向量。
        emit(progress, "contract_check:begin");
        if let Some(pool) = &clip_image_session {
            if let Some(guard) = pool.get() {
                crate::clip::verify_image_tower_contract(&guard, profile)?;
            }
        }

        info!(
            "AI provider ready: {} ({}) | AI 提供者就绪: {} ({})",
            provider_info.provider.label(),
            provider_info.gpu_name,
            provider_info.provider.label(),
            provider_info.gpu_name
        );

        // ── 步骤 3：加载人脸模型（可选；文件缺失则优雅降级，不阻断 CLIP）──
        // 人脸为纯 CNN（YuNet/SCRFD/SFace/ArcFace），跟随图像侧探测到的 provider。不同于 CLIP 文本塔
        // （BERT int64 在 DirectML 静默算错，坑9），CNN 在 GPU 上一般正确，留待 F8 对拍参考实现验正。
        // 检测+嵌入须同时就绪方为人脸可用（仅有检测而无嵌入无意义），任一缺失则整体降级为 None。
        let (face_detect_session, face_embed_session, loaded_face_profile) = match face_profile {
            Some(fp) => {
                let det = load_session_pool(
                    &models_dir.join(&fp.detect_file),
                    &provider_info.provider,
                    "Face detector | 人脸检测器",
                    face_pool_size,
                    "face_detect_load",
                    progress,
                );
                let emb = load_session_pool(
                    &models_dir.join(&fp.embed_file),
                    &provider_info.provider,
                    "Face embedder | 人脸嵌入器",
                    face_pool_size,
                    "face_embed_load",
                    progress,
                );
                if det.is_some() && emb.is_some() {
                    info!("Face models ready: {} | 人脸模型就绪: {}", fp.id, fp.id);
                    (det, emb, Some(fp.clone()))
                } else {
                    warn!(
                        "Face models not fully loaded (detector={}, embedder={}), face feature disabled | 人脸模型未完整加载，人脸功能禁用",
                        det.is_some(),
                        emb.is_some()
                    );
                    (None, None, None)
                }
            }
            None => (None, None, None),
        };

        Ok(Self {
            provider: provider_info.provider,
            gpu_name: provider_info.gpu_name,
            profile: profile.clone(),
            clip_image_session,
            clip_text_session,
            clip_tokenizer: None, // loaded lazily from models_dir in ai_commands
            face_detect_session,
            face_embed_session,
            face_profile: loaded_face_profile,
        })
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

/// 影像增强会话池装载(P0 批 3:enhance-worker 复用既有 `build_session`/EP 骨架,
/// design.md §A/§E「复用既有 ort engine/provider/session 骨架」)。单模型单 session
/// (增强严格串行、一次一请求,无并发压力);`provider` 由调用方 [`detect_best_provider`]
/// 探测后传入。缺文件/超时/装载失败均返回 `None`(语义同 [`load_session_pool`]),
/// 调用方据此回 `ModelLoadFailed`。
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

// ── Helpers ──────────────────────────────────────────────────────────────────
// ── 辅助函数 ──────────────────────────────────────────────────────────────────

// `pub(crate)`:OCR 模块(ocr::mod::OcrEngine::init)复用它的池装载+超时+进度姿态,
// 以 CPU EP、pool_size=1 加载 det/cls/rec 三 session(D-OCR-2)。
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
        #[cfg(any(target_os = "macos", target_os = "ios"))]
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
            // CPU 路径 — 因为扫描流水线的外层 `run_inference_tasks` 只有 1 个线程在请求 Session，
            // 所以我们必须给该 Session 赋予全部的核心资源（with_intra_threads(cores)），
            // 否则会造成全量扫描时 CPU 使用率极低（退化成单核执行）的问题。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// env 是进程级全局:三分支收进单个测试串行覆盖,避免并行测试互踩;
    /// 结尾还原现场(cargo 经根 config [env] 给测试进程注入了真实值)。
    #[test]
    fn resolve_ort_dylib_branches() {
        let saved = std::env::var("ORT_DYLIB_PATH").ok();

        // ① env 指向死路径 → DylibUnavailable,且消息指名 env 源(事故签名)。
        // 死路径必须落在**存在的盘符**下:不存在盘符(如 Z:)会触发 Windows 网络驱动器
        // 解析,is_file 可拖 ~60s——测试首跑实测 63s,生产自检同理不该撞这条慢路。
        let dead = std::env::temp_dir().join("no-such-dir-x7f3q\\onnxruntime.dll");
        std::env::set_var("ORT_DYLIB_PATH", &dead);
        let err = resolve_ort_dylib().unwrap_err();
        assert!(matches!(err, OrtPreflightError::DylibUnavailable(_)));
        assert!(err.to_string().contains("ORT_DYLIB_PATH"), "err: {err}");

        // ② env 指向真实存在的文件 → Ok(用测试可执行文件自身充当存在文件,零依赖)。
        let self_exe = std::env::current_exe().unwrap();
        std::env::set_var("ORT_DYLIB_PATH", &self_exe);
        let (p, src) = resolve_ort_dylib().expect("存在的 env 路径应通过");
        assert_eq!(p, self_exe);
        assert_eq!(src, "env ORT_DYLIB_PATH");

        // ③ 未设 env → 只认 exe 旁,拒绝系统搜索回退:结果取决于 exe 旁是否真有库文件,
        //    两种情形都断言到对应分支(deps 测试目录通常无库 → DylibUnavailable)。
        std::env::remove_var("ORT_DYLIB_PATH");
        let exe_adjacent = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join(ORT_DYLIB_NAME);
        match resolve_ort_dylib() {
            Ok((p, src)) => {
                assert!(exe_adjacent.is_file());
                assert_eq!(p, exe_adjacent);
                assert_eq!(src, "exe 同目录");
            }
            Err(e) => {
                assert!(!exe_adjacent.is_file());
                assert!(matches!(e, OrtPreflightError::DylibUnavailable(_)));
                assert!(e.to_string().contains("拒绝回退"), "err: {e}");
            }
        }

        match saved {
            Some(v) => std::env::set_var("ORT_DYLIB_PATH", v),
            None => std::env::remove_var("ORT_DYLIB_PATH"),
        }
    }
}
