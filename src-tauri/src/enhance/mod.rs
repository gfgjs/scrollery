// src-tauri/src/enhance/mod.rs
//! 影像增强 host 侧子系统（降噪/超分子系统 design.md §A/§B/§E）。
//!
//! - [`registry`]:模型下载清单 + 安装态判定（照 `ai::ocr_registry`）。
//! - [`service`]:`EnhanceService`——内存 job 队列、enhance-worker 惰性持有、GPU 令牌、
//!   claim/EXIF/rename、单文件 ingest。
//!
//! enhance **不进** exotic 任务化调度（D-OCR-7 同型豁免）；由 IPC `enhance_start` 显式驱动。

pub(crate) mod exif_inject;
pub mod registry;
pub mod service;

pub use service::{EnhanceParams, EnhanceService, JobDto, OutputFormatChoice};
