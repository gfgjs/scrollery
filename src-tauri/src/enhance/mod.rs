// src-tauri/src/enhance/mod.rs
//! 影像增强 host 侧子系统。
//!
//! - [`registry`]:模型下载清单与安装态判定。
//! - [`service`]:`EnhanceService`——内存 job 队列、enhance-worker 惰性持有、GPU 令牌、
//!   claim/EXIF/rename、单文件 ingest。
//!
//! enhance 由 IPC `enhance_start` 驱动，使用独立服务处理请求。

pub mod capability;
pub(crate) mod exif_inject;
pub mod registry;
pub mod service;

pub use service::{EnhanceParams, EnhanceService, JobDto, OutputFormatChoice};
