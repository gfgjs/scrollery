// src-tauri/src/ai/worker_client/mod.rs
//! AI worker 句柄(Part4-T17「AiEnginePool→worker 句柄」)。
//!
//! 主进程控制面持有 ai-worker 子进程的生命周期:spawn(死亡重建)→ ensure_session
//! (按活跃 profile 比对快照,不符先 close 再 init,D3 §4②)→ 批请求(EmbedBatch /
//! EncodeText)→ `exotic::worker::validate_*` 输出校验(「不信任 worker」纪律延续)。
//!
//! 路径地位(T16 收束):host 已恒 worker-only——进程内 ort 推理整段删除,本句柄是唯一推理通路;
//! 遗留 `ai_backend` 配置键已退役(读到非 worker 值仅 warn 忽略,见 `runtime_config::warn_legacy_ai_backend`)。
//!
//! 错误恢复契约(硬止损=重试一次):
//!   - 进程级异常(超时/断开/协议违例/输出校验失败)→ Supervisor 已 kill(或本端弃用
//!     实例)→ 重建 worker + 重建会话 + 重发一次;再败即向上返错。
//!   - `SessionExpired`(worker 端会话丢失,如其自杀重启)→ close(清 host 快照)后
//!     重 init 重发一次。
//!   - terminal Failure(EmbedDimMismatch/ModelLoadFailed 等)→ 不重试,直接返错。
//!
//! 人脸批(FaceDetectEmbed)已随 face_pipeline 接线波补齐(`face_detect_embed`);
//! 会话匹配采用**超集放宽**:不需要人脸的请求可复用带人脸的合并会话(见 `matches`)。
//!
//! 模块内部按职责拆分四组(process/session/dispatch/tests,超长文件拆分方案
//! 2026-07-25,`docs/planning/2026-07-25-超长文件拆分方案/analysis/worker_client-rs.md`):
//! 本文件只留模块文档、常量、核心类型与 `AiWorkerClient` 结构体定义 + `Default`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use exotic_protocol::{
    capability, EmbedItem, FaceItem, ModelDescriptor, ModelHandle, ModelProfileSnapshot, ModelRole,
    OcrItem, RequestBody, MAX_BLOB_LEN,
};
use tracing::{info, warn};

use crate::ai::face_profile::FaceProfile;
use crate::ai::profile::ModelProfile;
use crate::error::{AppError, Result};
use crate::exotic::coordinator::op_timeouts;
use crate::exotic::pipeline::EmbedWorker;
use crate::exotic::supervisor::{SessionDescriptor, WorkerSupervisor};
use crate::exotic::worker::{
    validate_embed_batch_output, validate_encode_text_output, validate_face_batch_output,
    validate_ocr_batch_output, EmbedItemOutcome, FaceItemOutcome, OcrItemOutcome, RawOutcome,
    WorkerConfig, WorkerSpec,
};

mod dispatch;
mod process;
mod session;
#[cfg(test)]
mod tests;

pub use process::ai_worker_exe;
pub use session::{build_ocr_session_spec, build_session_spec};

/// worker 进程握手超时(与 exotic 各插件一致:模型加载不在握手,恒快,D3 §2)。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// 弃用实例时给 worker 的体面退出宽限(Shutdown 帧后等待;超时 kill)。
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);
/// 批请求硬止损:同一请求至多尝试次数(首发 + 重建后重发一次)。
const MAX_ATTEMPTS: usize = 2;
/// retryable 失败(瞬态推理错误)重发前的退避(2026-07-10 审查 W2):给 DirectML
/// 内核错误/显存瞬时紧张一个恢复窗口,立刻重发大概率撞同一状态。
const RETRY_BACKOFF: Duration = Duration::from_millis(500);

/// 期望会话规格:host 侧真相(活跃 profile + 运行参数),ensure_session 据此与
/// worker 快照比对。由 [`build_session_spec`] 从 AppState/配置组装。
#[derive(Clone)]
pub struct SessionSpec {
    pub profile: ModelProfile,
    /// None = 本会话不载人脸角色(CLIP 管线/搜索只需图文双塔;face 接线波传 Some)。
    pub face_profile: Option<FaceProfile>,
    /// 模型目录(= SessionInit.models_root,worker 侧归属校验根)。
    pub models_dir: PathBuf,
    /// AI 缓存根(= `{cache_dir}/ai_thumbs`;worker 只在其下按白名单 key 读图)。
    pub ai_cache_dir: PathBuf,
    /// 图像塔 EP("auto"/"directml"/"cpu";文本塔 worker 内恒 CPU,§8.6)。
    pub image_provider: String,
    /// EmbedBatch 单批上限(进 SessionInit 快照;worker 超限即拒)。
    pub batch_size: u32,
}

impl SessionSpec {
    /// 会话快照是否与本规格匹配(不符 = 需切换:先 close 再 init)。
    ///
    /// **超集放宽**(face 波裁决):spec 不需要人脸(`face_profile=None`)时,带人脸的
    /// 合并会话照样可服务——CLIP 双塔独立于 face 角色,face 边际 VRAM 仅 ~96MB(T9.5
    /// 实测)。否则 face 运行期间的语义搜索会把会话抖成 close→init 循环(每次 ~1.4s)。
    /// 需要人脸(Some)时仍须精确匹配。
    fn matches(&self, desc: &SessionDescriptor) -> bool {
        let clip_ok = desc.arch_id == self.profile.id && desc.image_file == self.profile.image_file;
        let face_ok = match &self.face_profile {
            Some(fp) => desc.face_profile_id.as_deref() == Some(fp.id.as_str()),
            None => true,
        };
        // 批上限须比对(2026-07-10 审查 W3):worker 按会话快照硬拒超限批(terminal)。
        // 旧会话上限更小(如搜索先建会话后用户调大 ai_batch_size)必须切换重建;
        // 更大则可服务(派发批 ≤ spec 上限 ≤ 会话上限),不抖会话。
        let batch_ok = desc.batch_size >= self.batch_size;
        clip_ok && face_ok && batch_ok
    }
}

/// 期望 OCR 会话规格:活跃 profile + 模型目录。**独立于 [`SessionSpec`]**(CLIP 会话)
/// ——worker 内 OCR 与 CLIP 双槽并存互不干扰(D-OCR-1)。由 [`build_ocr_session_spec`]
/// 从 AppState/配置组装。OCR 会话无「超集放宽」语义:profile.id 精确比对即切换判据。
#[derive(Clone)]
pub struct OcrSessionSpec {
    pub profile: scrollery_ai_core::ocr_profile::OcrProfile,
    /// 模型目录(= OcrSessionInit.models_root,worker 侧归属校验根)。
    pub models_dir: PathBuf,
}

/// sha256 备忘条目:模型文件 GB 级,SessionInit 重建时不重算(len+mtime 未变即命中)。
struct ShaEntry {
    len: u64,
    mtime: SystemTime,
    hex: String,
}

/// 生成 EmbedWorker 实例的工厂闭包(测试注入 mock;运行时为 [`spawn_ai_worker`])。
type Spawner = Box<dyn Fn() -> std::result::Result<Box<dyn EmbedWorker>, String> + Send>;

/// [`AiWorkerClient::ensure_session`] 的失败二分(2026-07-10 审查 W1)。
enum EnsureError {
    /// 进程级(死管道/超时/协议违例):弃实例重建可能自愈——空闲自杀后的陈旧
    /// `alive` 旗标是主要来源,必须落进 `run_validated` 的 attempt 圈。
    Process(AppError),
    /// 数据级/环境级(worker 明确拒绝、spawn 失败):重建重发同死,直接冒泡。
    Terminal(AppError),
}

/// AI worker 句柄:worker 实例 + 会话簿记 + 模型 sha 备忘。放 `AppState.ai_worker`
/// (std Mutex,按调用粒度持锁——批与批之间可插入搜索请求,worker 本身严格串行)。
pub struct AiWorkerClient {
    worker: Option<Box<dyn EmbedWorker>>,
    spawner: Spawner,
    sha_cache: HashMap<PathBuf, ShaEntry>,
    next_session_id: u64,
    /// 在载 OCR 会话的 profile id(D-OCR-1:worker 内 OCR 与 CLIP 双槽并存,故独立于
    /// CLIP 的 `session()` 快照)。worker 换代/自杀 = OCR 会话蒸发,随 `drop_worker`/
    /// `ensure_worker` 重建复位为 None。
    ocr_loaded: Option<String>,
}

impl Default for AiWorkerClient {
    fn default() -> Self {
        Self::new()
    }
}
