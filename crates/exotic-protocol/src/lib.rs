// crates/exotic-protocol/src/lib.rs
//! 冷门格式插件 · Host↔Worker IPC 协议（v3 Part2 §3.1-3.3）。
//!
//! 设计天条：
//!   - **唯一帧格式**（[`frame`]）：magic + 版本 + 类型 + request_id + json_len + blob_len + json + blob。
//!     禁止「JSON 行 + 另一套二进制长度前缀」混合协议。
//!   - **先校验后分配**：读头校验 magic/版本/类型/长度上限，再分配 json/blob 缓冲（防内存炸弹）。
//!   - **共享定义**：主程序与 Worker 共用本 crate，两端对消息/错误码理解唯一一致。
//!   - **stdout 只走协议帧、日志只走 stderr**：Host 读取线程视任何非帧前导字节为协议损坏。
//!
//! 兼容性由 [`frame::PROTOCOL_VERSION`] + 握手能力共同决定，不靠「同仓依赖版本」隐式保证（R11）。

mod frame;
mod message;
mod stderr_log;

pub use frame::{
    read_frame, write_frame, Frame, FrameType, ProtocolError, HEADER_LEN, MAGIC, MAX_BLOB_LEN,
    MAX_JSON_LEN, PROTOCOL_VERSION,
};
pub use message::{
    capability, EmbedBatchSuccess, EmbedItem, EmbedResult, EnhanceDone, EnhanceStep, EnhanceTask,
    FaceBatchSuccess, FaceDet, FaceItem, FaceItemResult, FailureBody, HelloBody, ModelDescriptor,
    ModelHandle, ModelProfileSnapshot, ModelRole, OcrBatchSuccess, OcrItem, OcrItemResult, OcrLine,
    OcrSessionReadyBody, ProgressBody, ReadyBody, RequestBody, SessionReadyBody, SuccessBody,
    TextEmbedSuccess, VideoAudioTrack, VideoFramesInfo, VideoFramesMode, VideoOutInfo,
    VideoProbeInfo, VideoSessionInfo, WorkerErrorCode,
};
pub use stderr_log::{emit_stderr_log, WorkerLogLine};
