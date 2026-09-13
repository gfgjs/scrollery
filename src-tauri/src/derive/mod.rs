// src-tauri/src/derive/mod.rs
//! 统一派生框架（§2.2）：一套可续传 / 可让步 / 可取消的调度器，供所有「派生产物」任务共享 ——
//! 视频封面与关键帧、文档缩略图、音频封面与元数据。每种 kind 只实现纯函数
//! `run(ctx) -> Result<Output>`；流水线（`pipeline.rs`）提供生产者/消费者/写入器、续传、
//! 孤儿恢复，以及分级让步（扫描 > 缩略图 > 派生 > AI）。

pub mod audio;
pub mod doc;
pub mod image;
pub mod kind;
pub mod pipeline;
pub mod video;

pub use kind::{DerivationContext, DerivationKind, DerivationOutput};
pub use pipeline::{derivation_counts, start_derivation_pipeline};
