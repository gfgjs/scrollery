// src-tauri/src/exotic/outcome.rs
//! Worker 输出的共享词汇(U-P3,2026-07-16 从 worker.rs 拆出)。
//!
//! 这些 outcome 类型是 [`super::worker::WorkerConn`] 状态机与 [`super::validate`]
//! 纯校验器的**共享词汇**,消费方横跨 exotic 流水线与 AI 层(`ai::worker_client`/
//! `ai::face_pipeline`)。单独成模块使两边引用同一处定义;旧路径
//! `exotic::worker::{RawOutcome, ...}` 经 worker.rs 的 `pub use` 继续可用。

use exotic_protocol::{FaceDet, FailureBody, SuccessBody, WorkerErrorCode};

/// op 无关的原始请求结果(T15,D3 §4①)。`Success` **未经任何 op 特定校验**——
/// thumbnail 的 WebP 复核在 [`super::worker::WorkerConn::run_thumbnail`],embed/face 批的
/// 维度×数量一致性在 [`super::validate::validate_embed_batch_output`]/
/// [`super::validate::validate_face_batch_output`],由调用方按 op 分派。
/// 进程级三态(TimedOut/Disconnected/Protocol)语义与 [`TaskOutcome`] 一致
/// (Supervisor 据此 kill 回收)。
// SessionReady 回声字段(T16)使 Success 变体略超 clippy 阈值;本枚举按值单次传递、
// 不进集合,变体大小差异无实际内存代价,故豁免而非 Box(免全链解引用噪音)。
#[allow(clippy::large_enum_variant)]
pub enum RawOutcome {
    /// 收到 Success 帧(body+blob 原样交出,尚未按 op 校验)。
    Success { body: SuccessBody, blob: Vec<u8> },
    /// Worker 显式失败(item/fingerprint 核对属 op 语义,由调用方做)。
    Failure(FailureBody),
    /// 超时(Supervisor 应 kill)。
    TimedOut,
    /// 连接断开 / Worker 退出。
    Disconnected,
    /// 协议违例(错序 / 损坏帧 / 意外帧类型)。
    Protocol(String),
}

/// 一次任务的结果。`Success` 已通过 Host 全部验证。
pub enum TaskOutcome {
    /// 验证通过的缩略图。
    Success {
        width: u32,
        height: u32,
        mime: String,
        blob: Vec<u8>,
        /// 宿主对已验证像素计算，绝不采信 worker 声明。
        thumbhash: Vec<u8>,
    },
    /// Worker 显式失败（已核对 item/fingerprint）。
    Failure(FailureBody),
    /// 超时（Supervisor 应 kill）。
    TimedOut,
    /// 连接断开 / Worker 退出（Supervisor 应 wait 回收）。
    Disconnected,
    /// 协议违例或输出非法（错序 / 错 id / 非法 WebP / 尺寸不符）→ terminal invalid_worker_output。
    Protocol(String),
}

/// EmbedBatch 单项的校验后结果(与请求 items 同序对齐)。
#[derive(Debug)]
pub enum EmbedItemOutcome {
    /// 该项嵌入(已按 embed_dim 从 blob 切出,f32 LE)。
    Ok(Vec<f32>),
    /// 该项失败(worker 逐项报错,不连坐)。
    Err(WorkerErrorCode),
}

/// FaceDetectEmbed 单项的校验后结果(与请求 items 同序对齐)。
#[derive(Debug)]
pub enum FaceItemOutcome {
    /// 几何 + 逐脸嵌入(faces 与 embeddings 同序同长;0 脸也是 Ok)。
    Ok {
        faces: Vec<FaceDet>,
        embeddings: Vec<Vec<f32>>,
        /// worker 实际解码尺寸(几何为该图像素坐标;归一化/quality 派生用)。
        width: u32,
        height: u32,
    },
    /// 该项失败(不连坐)。
    Err(WorkerErrorCode),
}
