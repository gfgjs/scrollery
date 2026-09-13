//! 查看器渲染色域(B 线,2026-07-23)。ContentViewer 大图按用户选定的目标色域派生一份嵌入
//! target ICC 的图片,替代直显原图,让 WebView2 对宽色域屏做正确的 target→display 映射。
//!
//! 契约(architect 计划 §0,主线已批,前提已钉死):
//! - **D-411**:派生渲染把源像素投影到 target 色域并**嵌入 target ICC**(输出 JPEG/PNG)。
//! - **D-412**:像素域 == 嵌入 profile——CMS 变换与嵌入用**同一 profile 对象/同一字节**
//!   (内置=`ColorProfile::encode()` 的字节;自定义=导入 `.icc` 原字节),构造性成立。
//! - **D-413**:编辑链走 sRGB 不受影响——本模块复用 `editing::color::to_target_rgba8`(泛化),
//!   但编辑侧 `to_srgb_rgba8` 的 None 短路语义原样保留,既有对拍测试即守卫。
//! - **D-414**:桌面先行,移动端(android/ios)锁 sRGB——命令层 cfg 门控(见 `ipc::viewer_color_commands`)。
//! - sRGB target = 直显原图、零派生(`target::ViewerColorTarget::from_config` 对 srgb 返 `None`)。
//! - 范围 = ContentViewer 大图;缩略图色管归 A 线(`editing::color::project_rgba8_to_srgb`)。
//!
//! 缓存布局与记账并入既有缩略图治理(方案 §0②):`cache/viewer_color/{target_id}/{prefix}/
//! {hex}.{ext}`,与 10GB 单源 LRU 预算共用(见 `thumbnail::cache`)。

pub mod render;
pub mod target;

use crate::error::AppError;

// ── 稳定错误码集(方案 §0①,原样透到 IPC `code` 字段;error.rs `AppError::Color` 文档同源)──
// ICC 导入校验链:
pub const CODE_ICC_PARSE_FAILED: &str = "icc_parse_failed";
pub const CODE_ICC_NOT_RGB: &str = "icc_not_rgb";
pub const CODE_ICC_NOT_DISPLAY_CLASS: &str = "icc_not_display_class";
pub const CODE_ICC_TRANSFORM_UNSUPPORTED: &str = "icc_transform_unsupported";
pub const CODE_ICC_TOO_LARGE: &str = "icc_too_large";
pub const CODE_ICC_IO: &str = "icc_io";
pub const CODE_ICC_NOT_FOUND: &str = "icc_not_found";
// 平台门控:
pub const CODE_UNSUPPORTED_PLATFORM: &str = "unsupported_platform";
// 渲染链:
pub const CODE_RENDER_UNSUPPORTED: &str = "viewer_render_unsupported";
pub const CODE_RENDER_DECODE_FAILED: &str = "viewer_render_decode_failed";
pub const CODE_RENDER_TOO_LARGE: &str = "viewer_render_too_large";
pub const CODE_RENDER_IO: &str = "viewer_render_io";

/// 导入 ICC 的大小上限(方案 §0④):16 MB。超限即 `icc_too_large`,不读入内存解析。
pub const ICC_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// 构造 `AppError::Color`。message 只写中文+英文提示语,**不携带路径 / 底层错误串**(泄漏面,
/// 硬约束;同 `Reveal`/`Player` 姿态)。
pub(crate) fn color_err(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::Color {
        code,
        message: message.into(),
    }
}
