//! 只装配隔离进程需要的图像后端，GPU 执行实现不再编入宿主库。

#[path = "engine_gpu.rs"]
pub(crate) mod gpu;
#[path = "../../../../src-tauri/src/engine/image_rs.rs"]
pub(crate) mod image_rs;
#[path = "../../../../src-tauri/src/engine/orientation.rs"]
pub(crate) mod orientation;
#[path = "../../../../src-tauri/src/engine/traits.rs"]
pub(crate) mod traits;

pub(crate) use traits::{DecodedImage, ImageEngine};
