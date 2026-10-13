// src-tauri/src/thumbnail/mod.rs
pub(crate) mod artifact_io;
pub(crate) mod benchmark;
#[cfg(not(windows))]
pub(crate) mod byte_budget;
pub mod cache;
pub mod exif_thumb;
pub mod generator;
pub(crate) mod identity;
pub(crate) mod image_payload;
pub(crate) mod intake;
// 宿主与独立 worker 共用协议源码，宿主不调用 worker 的读请求、写回复入口。
#[allow(dead_code)]
pub(crate) mod native_protocol;
pub(crate) mod progress;
pub(crate) mod publication;
pub mod qos;
pub mod router;
pub mod serve;
pub(crate) mod service;
pub mod thumbhash;
pub(crate) mod viewport;
pub(crate) mod viewport_images;

pub use generator::{encode_media_step_with_snapshot, ThumbConfig};
pub use router::{route_thumbnail, ThumbnailRoute, ThumbnailRouteInput};

#[cfg(windows)]
pub(crate) mod native_worker;
pub(crate) mod source_budget;
#[cfg(windows)]
pub(crate) mod wake;

pub(crate) mod execution;
#[cfg(not(windows))]
pub(crate) mod portable_execution;
