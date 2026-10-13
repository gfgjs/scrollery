//! 独立图片执行域；复用宿主的图像源码，不依赖应用库或 Tauri 构建配置。

#[cfg(windows)]
mod error;

// 共享模块还提供宿主专用入口，worker 只使用其中的解码、编码及 stdio 路径。
#[cfg(windows)]
#[allow(dead_code)]
mod editing;
#[cfg(windows)]
#[allow(dead_code)]
mod engine;
#[cfg(windows)]
#[allow(dead_code)]
mod thumbnail;

fn main() {
    #[cfg(windows)]
    let result = thumbnail::native_runtime::run_stdio();
    #[cfg(not(windows))]
    let result: std::io::Result<()> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "native thumbnail worker requires Windows",
    ));
    if let Err(error) = result {
        eprintln!("native thumbnail worker: {error}");
        std::process::exit(1);
    }
}
