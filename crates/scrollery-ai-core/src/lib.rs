// crates/scrollery-ai-core/src/lib.rs
//! Scrollery · AI 推理核心(Part4-T15,自 src-tauri/src/ai/ 迁出)。
//!
//! 定位:**纯推理层**——ort Session 池、CLIP 图像/文本编解码、人脸检测/对齐/嵌入、
//! 模型契约注册表(`ModelProfile`/`FaceProfile`)。**不含**任何控制面(调度/DB/缓存/
//! 让步/下载),那些留在主进程(Part4 §3.1.3 / T17)。
//!
//! 消费方(T16「过渡双活」期,Part4 §3.2):
//!   - `ai-worker` 子进程(crates/exotic-workers/ai-worker):经 exotic-protocol v2 的
//!     SessionInit/EmbedBatch/FaceDetectEmbed 驱动本核;
//!   - `src-tauri` 主进程:进程内路径继续直调(`src-tauri/src/ai/*` 已退化为再导出薄壳),
//!     直至 T16 worker e2e 验收通过后删 ort 依赖。
//!
//! 错误契约:本 crate 自持 [`error::AiError`](thiserror);src-tauri 侧经
//! `From<AiError> for AppError` 无损映射,`?` 传播不变。

// ── feature 分层(T16 准备)─────────────────────────────────────────────────────
// `inference`(缺省开):ort Session/CLIP 编解码/人脸检测嵌入/EP 探测——ai-worker 全开;
// src-tauri 在 T16 删 ort 时改 `default-features = false`,只留下方永远可用的纯契约面
// (模型注册表/几何类型/字节序工具/解码图类型),彻底摆脱 ort+tokenizers 链接。
// 门在模块级:纯件已外移(embedding/face_types),clip/face 以 `pub use` 原位再导出,
// feature 全开时引用路径零变化。

#[cfg(feature = "inference")]
pub mod clip;
pub mod decoded;
pub mod embedding;
#[cfg(feature = "inference")]
pub mod engine;
// 影像增强(降噪/去伪影/超分)模型契约:纯数据零 ort,同 ocr_profile/face_profile 模式,
// 不门控 inference feature。推理面留 P0 批 2(crates/scrollery-ai-core/src/enhance/)。
pub mod enhance_profile;
// 影像增强推理面(P0 批 2):enhance/tiling 纯几何无门,enhance/chain 推理面在 inference 门内
// (模块内自门,照 clip/ocr 门法)。mod 本身无门:纯类型/错误/tiling 恒可用。
pub mod enhance;
pub mod error;
#[cfg(feature = "inference")]
pub mod face;
pub mod face_profile;
pub mod face_types;
// OCR 管线(自研 det+cls+rec+CTC):`ocr_profile` 纯契约不门控(src-tauri 关默认特性消费,
// 同 face_profile 模式);`ocr` 推理面在 inference 门内(ort/imageproc)。
#[cfg(feature = "inference")]
pub mod ocr;
pub mod ocr_profile;
pub mod profile;
#[cfg(feature = "inference")]
pub mod provider;

pub use decoded::DecodedImage;
pub use error::{AiError, Result};
