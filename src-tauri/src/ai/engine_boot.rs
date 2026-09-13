// src-tauri/src/ai/engine_boot.rs
//! 推理引擎启动期装配:目前仅 ORT 动态库路径解析(自 `lib.rs::run()` 的 setup 段 d 迁出,
//! 纯结构移动,行为不变)。

use tracing::info;

/// 解析并(必要时)设置 `ORT_DYLIB_PATH`。
///
/// 【踩坑1】WebView2 在 Windows 上可能在我们加载之前就把 System32 里的
///   onnxruntime.dll（通常是 ORT 1.17）加载进进程空间。
///   设置 ORT_DYLIB_PATH 强制 ort crate 从指定路径加载，绕过系统版本。
///
/// 【踩坑2】`load-dynamic` 与 `download-binaries` 互斥：
///   load-dynamic 激活 ort-sys/disable-linking，build.rs 提前退出，
///   download-binaries 完全不运行。必须手动管理 DLL。
///
/// 【踩坑3】ORT 版本要求：
///   - ONNX IR v10（PyTorch 2.11 导出）要求 ORT >= 1.19
///   - eisneim FP16 外部数据格式模型要求 ORT >= 1.26
///   - 使用 onnxruntime-node@1.26.0 自带的 DLL（bin/napi-v6/win32/x64/）
///
/// 优先级：
///   1. ORT_DYLIB_PATH 已设置（.cargo/config.toml 或环境变量）→ 保留，不覆盖
///   2. 可执行文件旁边的 onnxruntime.dll（生产/打包版本）→ 使用
///   3. 都没有 → ORT 自行搜索（可能加载到错误版本）
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
