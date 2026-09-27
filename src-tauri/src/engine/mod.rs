// src-tauri/src/engine/mod.rs
//! 媒体解码引擎注册表（EngineArena）。

pub mod gpu;
pub mod image_rs;
pub mod native;
pub mod traits;
// pub mod heic; // Phase 2
// pub mod heic; // 阶段 2
// pub mod raw;  // Phase 2
// pub mod raw;  // 阶段 2

pub use traits::{DecodedImage, ImageEngine};

use crate::engine::image_rs::ImageRsEngine;
use std::sync::Arc;

/// 引擎竞技场根据文件格式将解码分派给适当的引擎。
pub struct EngineArena {
    engines: Vec<Arc<dyn ImageEngine>>,
}

impl EngineArena {
    /// 构建阶段 1 的竞技场。
    ///
    /// 注册顺序即分发优先级(`engine_for` 取首个 can_handle 命中):
    /// ImageRsEngine 在前——Phase 1 格式(jpg/png/webp/bmp/gif/tiff)保持既有引擎与行为不变
    /// (Part3 §3.2 裁决:WIC 排 ImageRsEngine 之后);WicEngine 在后兜接 image-rs 不认的
    /// heic/heif/avif/ico(审查 R0-2:此前 arena 无引擎认 HEIC → UnsupportedFormat →
    /// iPhone 照片永远占位图)。WIC 对 HEIC/AVIF 依赖系统已装 HEIF/AV1 扩展,缺失时解码
    /// 失败按既有链路记 thumb_status=2,属 Part0 §5.2 预期的「运行时检测降级」。
    pub fn phase1() -> Self {
        #[allow(unused_mut)] // 非 Windows 无后续 push,mut 仅 Windows 用到
        let mut engines: Vec<Arc<dyn ImageEngine>> = vec![Arc::new(ImageRsEngine)];
        #[cfg(windows)]
        engines.push(Arc::new(crate::engine::native::wic_engine::WicEngine));
        Self { engines }
    }

    /// 为给定格式查找引擎。如果不被支持，则返回 `None`。
    pub fn engine_for(&self, format: &str) -> Option<Arc<dyn ImageEngine>> {
        self.engines.iter().find(|e| e.can_handle(format)).cloned()
    }
}
