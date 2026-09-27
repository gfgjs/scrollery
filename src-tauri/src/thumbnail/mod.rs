// src-tauri/src/thumbnail/mod.rs
pub mod cache;
pub mod coordinator;
pub mod exif_thumb;
pub mod generator;
#[cfg(windows)]
mod native_adapter;
pub mod native_protocol;
#[cfg(windows)]
pub mod native_worker;
pub mod qos;
pub mod router;
pub mod scheduler;
pub mod serve;
pub mod thumbhash;

pub use generator::{
    decode_media_step, encode_media_step, encode_media_step_with_snapshot, process_deferred_cpu,
    DecodeResult, ThumbConfig,
};
pub use router::{route_thumbnail, ThumbnailRoute, ThumbnailRouteInput};
