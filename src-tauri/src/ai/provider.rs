// src-tauri/src/ai/provider.rs
//! host 侧通过 DXGI 探测显存，供自动 batch 大小与状态展示使用。
//! 推理执行提供器由 ai-worker 选择。

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
