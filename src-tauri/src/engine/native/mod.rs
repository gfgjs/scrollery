//! 系统原生图像编解码入口；WIC 本身不表示 GPU 加速。

#[cfg(windows)]
pub mod wic_engine;

use crate::engine::traits::ImageEngine;

/// 按名称取得系统图像编解码器；返回值不表示硬件加速。
pub fn get_native_image_engine(name: &str) -> Option<Box<dyn ImageEngine>> {
    match name {
        #[cfg(windows)]
        "wic" => Some(Box::new(wic_engine::WicEngine)),
        _ => None,
    }
}
