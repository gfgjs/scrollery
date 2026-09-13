// src-tauri/src/engine/gpu/mod.rs
// wic_engine 全文是 Win32 WIC 调用,模块级 cfg(windows) 门控(审查 R0-2 / todo C 节):
// `windows` dep 已 target 限定,非 Windows 目标此模块不参与编译——公开树 Linux check 的唯一硬错误面即此。
#[cfg(windows)]
pub mod wic_engine;

use crate::engine::traits::ImageEngine;

/// Factory to get a GPU engine by name
pub fn get_gpu_engine(name: &str) -> Option<Box<dyn ImageEngine>> {
    match name {
        // 非 Windows 无 WIC → 落到 `_ => None`,调用方(generator/ai pipeline/derive)均已有
        // None → CPU 解码回退,行为不变。
        #[cfg(windows)]
        "wic" => Some(Box::new(wic_engine::WicEngine)),
        // 后续可在此接入更多 GPU 引擎（如 nvjpeg、dxva）
        _ => None,
    }
}
