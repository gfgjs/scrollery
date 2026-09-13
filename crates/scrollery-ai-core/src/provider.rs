// crates/scrollery-ai-core/src/provider.rs
//! AI 硬件加速后端探测与选择。
//!
//! 探测顺序：DirectML → CUDA → CoreML → OpenVINO → CPU

use serde::{Deserialize, Serialize};
/// 支持的 AI 执行提供者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    /// Windows DirectML（与厂商无关,兼容 AMD/NVIDIA/Intel）
    DirectML,
    /// NVIDIA CUDA
    CUDA,
    /// Apple CoreML（macOS/iOS）
    CoreML,
    /// Intel OpenVINO
    OpenVINO,
    /// CPU 兜底
    #[default]
    Cpu,
}

impl AiProvider {
    pub fn label(&self) -> &'static str {
        match self {
            AiProvider::DirectML => "DirectML (GPU)",
            AiProvider::CUDA => "CUDA (NVIDIA GPU)",
            AiProvider::CoreML => "CoreML (Apple)",
            AiProvider::OpenVINO => "OpenVINO (Intel)",
            AiProvider::Cpu => "CPU",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AiProvider::DirectML => "directml",
            AiProvider::CUDA => "cuda",
            AiProvider::CoreML => "coreml",
            AiProvider::OpenVINO => "openvino",
            AiProvider::Cpu => "cpu",
        }
    }

    // 固有 from_str：返回 Self（非 std FromStr 的 Result）、且不可失败（未知值回退默认），
    // 与标准 trait 语义不同；改名会波及调用点，保留固有方法。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "directml" => AiProvider::DirectML,
            "cuda" => AiProvider::CUDA,
            "coreml" => AiProvider::CoreML,
            "openvino" => AiProvider::OpenVINO,
            _ => AiProvider::Cpu,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider: AiProvider,
    pub gpu_name: String,
}

/// 检测当前平台上可用的最优 AI 执行提供者。
///
/// 目前使用编译期平台检测。未来版本可能会添加运行时 GPU 探测（如 DirectML 能力检测、CUDA 设备查询）。
pub fn detect_best_provider() -> ProviderInfo {
    #[cfg(target_os = "windows")]
    {
        return ProviderInfo {
            provider: AiProvider::DirectML,
            gpu_name: "DirectML GPU".to_string(),
        };
    }

    #[cfg(target_os = "macos")]
    {
        return ProviderInfo {
            provider: AiProvider::CoreML,
            gpu_name: "Apple Neural Engine".to_string(),
        };
    }

    #[cfg(target_os = "linux")]
    {
        return ProviderInfo {
            provider: AiProvider::Cpu,
            gpu_name: String::new(),
        };
    }

    #[allow(unreachable_code)]
    ProviderInfo {
        provider: AiProvider::Cpu,
        gpu_name: String::new(),
    }
}

/// 探测专用显存大小（字节）。
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
