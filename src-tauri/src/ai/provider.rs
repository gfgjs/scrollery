// src-tauri/src/ai/provider.rs
//! host 侧 VRAM 探测(T16 收束:EP 探测/选择已随进程内引擎迁往 worker 侧,
//! ai-core 的 provider 模块在 `inference` feature 门内;host 仅保留 DXGI 显存
//! 探测——batch 自动档(resolve_batch_size)与状态栏 vram_gb 展示是 host 关切,
//! 与推理面无关,故在此自持实现,调用路径 `crate::ai::provider::*` 不变)。

/// 探测专用显存大小(字节)。
pub fn detect_vram_bytes() -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        unsafe {
            if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
                if let Ok(adapter) = factory.EnumAdapters1(0) {
                    if let Ok(desc) = adapter.GetDesc1() {
                        return Some(desc.DedicatedVideoMemory as u64);
                    }
                }
            }
        }
    }
    None
}
