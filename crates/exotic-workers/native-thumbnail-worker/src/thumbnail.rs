//! 协议、预算、QoS 与图像处理均直接引用生产源码，避免两份实现发生漂移。

#[path = "../../../../src-tauri/src/thumbnail/byte_budget.rs"]
pub(crate) mod byte_budget;
#[path = "../../../../src-tauri/src/thumbnail/exif_thumb.rs"]
pub(crate) mod exif_thumb;
#[path = "../../../../src-tauri/src/thumbnail/image_payload.rs"]
pub(crate) mod image_payload;
#[path = "../../../../src-tauri/src/thumbnail/native_protocol.rs"]
pub(crate) mod native_protocol;
#[path = "../../../../src-tauri/src/thumbnail/native_runtime.rs"]
pub(crate) mod native_runtime;
#[path = "../../../../src-tauri/src/thumbnail/qos.rs"]
pub(crate) mod qos;
#[path = "../../../../src-tauri/src/thumbnail/thumbhash.rs"]
pub(crate) mod thumbhash;
