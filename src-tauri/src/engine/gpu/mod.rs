// WIC 仅在 Windows 编译，非 Windows 目标不引入平台依赖。
#[cfg(windows)]
pub mod wic_engine;
