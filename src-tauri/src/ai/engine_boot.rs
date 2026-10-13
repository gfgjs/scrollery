// src-tauri/src/ai/engine_boot.rs
//! 启动时解析 ai-worker 所需的 ORT 动态库路径。

use tracing::info;

/// 解析并(必要时)设置 `ORT_DYLIB_PATH`。
///
/// ai-worker 的 ORT preflight 要求显式路径或可执行文件旁的 DLL，以避免误加载系统旧版。
///
/// 优先级：
///   1. ORT_DYLIB_PATH 已设置（.cargo/config.toml 或环境变量）→ 保留，不覆盖
///   2. 可执行文件旁边的 onnxruntime.dll（生产/打包版本）→ 使用
///   3. 都没有 → worker 的 preflight 拒绝加载
pub fn resolve_ort_dylib_path() {
    if std::env::var("ORT_DYLIB_PATH").is_err() {
        // 只有在构建系统未配置时才自行设置
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let ort_dylib = exe_dir.join("onnxruntime.dll");
                if ort_dylib.exists() {
                    std::env::set_var("ORT_DYLIB_PATH", ort_dylib.to_string_lossy().as_ref());
                    info!(
                        "Set ORT_DYLIB_PATH to exe-relative path (production mode): {:?}",
                        ort_dylib
                    );
                } else {
                    info!("onnxruntime.dll not found next to exe, ORT will search system PATH");
                }
            }
        }
    } else {
        info!(
            "ORT_DYLIB_PATH already set (by build system): {}",
            std::env::var("ORT_DYLIB_PATH").unwrap_or_default()
        );
    }
}
