// crates/scrollery-ai-core/src/enhance/mod.rs
//! 影像增强推理面(解码 → 任务链 → tiling → ort → 融合 → 编码)。
//!
//! 分层:
//!   - [`tiling`]:纯几何(零 ort),永远可用;
//!   - [`chain`]:执行链(ort/image 依赖),门控 `inference` feature——同 clip/ocr 门法。
//!
//! 模型契约(几何/文件名/scale/task)全部取自 [`crate::enhance_profile`](批 1 产物)。
//! `EnhanceChainRequest.steps` **严格按序执行,chain 不重排**——执行序(降噪→去伪影→超分)
//! 是 host 责任,core 只忠实执行给定顺序。

use std::path::PathBuf;

use thiserror::Error;

pub use crate::enhance_profile::EnhanceTaskKind;

pub mod tiling;
pub use tiling::{plan_tiles, Rect, TileSpec};

#[cfg(feature = "inference")]
pub mod chain;
#[cfg(feature = "inference")]
pub use chain::{run_enhance_chain, EnhanceSessions};

/// 增强链中的单步:任务 + 模型档 id + 可选强度(σ/QF,任务相关)。
#[derive(Debug, Clone)]
pub struct EnhanceStepSpec {
    pub task: EnhanceTaskKind,
    /// 对应 [`crate::enhance_profile::EnhanceProfile::id`]。
    pub profile_id: String,
    /// 强度参数:DRUNet 为 σ(原生量纲 0–50),FBCNN 为 QF(0–100);其余任务忽略。
    pub strength: Option<f32>,
}

/// 输出编码格式(无元数据;EXIF 由 host 容器级注入,core 不管)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhanceOutputFormat {
    Jpeg,
    Png,
}

/// 一次增强链请求。
#[derive(Debug, Clone)]
pub struct EnhanceChainRequest {
    /// 源位图路径(JPEG/PNG/TIFF)。
    pub source_path: PathBuf,
    /// 产物临时路径(host 指定的 `{work_dir}/{job}.tmp`;写 `*.tmp` 后由 host 认领 rename)。
    pub output_tmp_path: PathBuf,
    pub output_format: EnhanceOutputFormat,
    /// 严格按序执行的步骤链(host 已排序,core 不重排)。
    pub steps: Vec<EnhanceStepSpec>,
}

/// 增强链产物统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnhanceReport {
    pub out_width: u32,
    pub out_height: u32,
    /// 全链所有 step 的瓦片数总和(= progress 回调的 `total`)。
    pub tiles_total: u32,
}

/// 增强链错误。变体携字符串(不外泄内部类型、不耦合 `inference` feature 的 ort 错误)。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EnhanceError {
    #[error("解码失败: {0}")]
    Decode(String),
    #[error("编码失败: {0}")]
    Encode(String),
    #[error("IO 错误: {0}")]
    Io(String),
    #[error("推理失败: {0}")]
    Inference(String),
    #[error("模型档缺失: {0}")]
    ProfileMissing(String),
    #[error("不支持的输入: {0}")]
    UnsupportedInput(String),
}
