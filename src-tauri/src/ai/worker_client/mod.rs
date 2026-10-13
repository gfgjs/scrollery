// src-tauri/src/ai/worker_client/mod.rs
//! AI worker 生命周期、会话管理与请求派发。
//!
//! 主进程控制面持有 ai-worker 子进程的生命周期:spawn(死亡重建)→ ensure_session
//! (相同模型补载角色，切模型先 close 再 init)→ 批请求(EmbedBatch /
//! EncodeText)→ exotic::worker::validate_* 输出校验。
//!
//! AI 推理由 worker 执行，此句柄负责宿主侧控制。
//!

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use exotic_protocol::{
    capability, EmbedItem, FaceItem, ModelDescriptor, ModelHandle, ModelProfileSnapshot, ModelRole,
    OcrItem, RequestBody, MAX_BLOB_LEN,
};
use scrollery_ai_core::profile::ClipRoles;
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

/// 握手不加载模型，使用短超时快速识别启动失败。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// 弃用实例时给 worker 的体面退出宽限(Shutdown 帧后等待;超时 kill)。
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);
/// 批请求硬止损:同一请求至多尝试次数(首发 + 重建后重发一次)。
const MAX_ATTEMPTS: usize = 2;
/// 瞬态推理失败重发前短暂退避，给 GPU 内核或显存压力留出恢复窗口。
const RETRY_BACKOFF: Duration = Duration::from_millis(500);

/// 期望会话规格:host 侧真相(活跃 profile + 运行参数),ensure_session 据此与
/// worker 快照比对。由 [`build_session_spec`] 从 AppState/配置组装。
#[derive(Clone)]
pub struct SessionSpec {
    pub profile: ModelProfile,
    pub roles: ClipRoles,
    /// None 表示本请求不需要人脸角色。
    pub face_profile: Option<FaceProfile>,
    /// 模型目录(= SessionInit.models_root,worker 侧归属校验根)。
    pub models_dir: PathBuf,
    /// AI 缓存根(= `{cache_dir}/ai_thumbs`;worker 只在其下按白名单 key 读图)。
    pub ai_cache_dir: PathBuf,
    /// 图像塔提供者（auto/directml/cpu）；文本塔固定 CPU。
    pub image_provider: String,
    /// EmbedBatch 单批上限(进 SessionInit 快照;worker 超限即拒)。
    pub batch_size: u32,
}

impl SessionSpec {
    /// 已加载能力超集可直接复用；缺少角色时补载，模型契约变化时重建。
    fn matches(&self, desc: &SessionDescriptor) -> bool {
        let clip_ok = desc.arch_id == self.profile.id && desc.image_file == self.profile.image_file;
        let face_ok = match &self.face_profile {
            Some(fp) => desc.face_profile_id.as_deref() == Some(fp.id.as_str()),
            None => true,
        };
        // 会话上限不足时更新快照；更大的既有上限可直接复用，避免不必要重建。
        let batch_ok = desc.batch_size >= self.batch_size;
        let has = |cap| desc.caps.iter().any(|c| c == cap);
        let roles_ok = (!self.roles.image || has(capability::EMBEDDING))
            && (!self.roles.text || has(capability::TEXT_ENCODING))
            && (self.face_profile.is_none() || has(capability::FACE_DETECT_EMBED));
        clip_ok && face_ok && batch_ok && roles_ok
    }

    /// 相同契约可补载角色或增大批上限；已有不同的人脸模型需要卸载。
    fn can_extend(&self, desc: &SessionDescriptor) -> bool {
        desc.arch_id == self.profile.id
            && desc.image_file == self.profile.image_file
            && self
                .face_profile
                .as_ref()
                .is_none_or(|fp| desc.face_profile_id.as_deref().is_none_or(|id| id == fp.id))
    }
}

/// 调用方所需的推理能力，避免图片、人脸和查询互相强制装载模型。
pub enum SessionPurpose<'a> {
    Image,
    Text,
    Face(&'a FaceProfile),
}

/// OCR 独立于 CLIP 会话槽，按 profile id 精确匹配，由 build_ocr_session_spec 组装。
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

/// 会话准备失败分为可重建的进程异常与不可重试的终止错误。
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
    /// 独立 OCR 槽的 profile id；worker 弃用或重建时清空。
    ocr_loaded: Option<String>,
}

impl Default for AiWorkerClient {
    fn default() -> Self {
        Self::new()
    }
}
