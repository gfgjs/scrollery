//! 查看器渲染色域。ContentViewer 大图按用户选定的目标色域派生一份嵌入
//! target ICC 的图片,替代直显原图,让 WebView2 对宽色域屏做正确的 target→display 映射。
//!
//! 渲染契约：
//! - 派生渲染把源像素投影到 target 色域并嵌入 target ICC（输出 JPEG/PNG）。
//! - CMS 变换与嵌入使用同一 profile 对象和字节，保持像素域与嵌入 profile 一致
//!   (内置=`ColorProfile::encode()` 的字节;自定义=导入 `.icc` 原字节),构造性成立。
//! - 编辑链走 sRGB；本模块复用 `editing::color::to_target_rgba8`，编辑侧保留无 ICC 短路。
//! - sRGB target = 直显原图、零派生(`target::ViewerColorTarget::from_config` 对 srgb 返 `None`)。
//! - 范围为 ContentViewer 大图；缩略图色管见 `editing::color::project_rgba8_to_srgb`。
//!
//! 缓存布局与记账并入缩略图治理：`cache/viewer_color/{target_id}/{prefix}/
//! {hex}.{ext}`,与 10GB 单源 LRU 预算共用(见 `thumbnail::cache`)。

pub mod render;
pub mod target;

use crate::error::AppError;

// 稳定错误码原样透到 IPC `code` 字段。
// ICC 导入校验链:
pub const CODE_ICC_PARSE_FAILED: &str = "icc_parse_failed";
pub const CODE_ICC_NOT_RGB: &str = "icc_not_rgb";
pub const CODE_ICC_NOT_DISPLAY_CLASS: &str = "icc_not_display_class";
pub const CODE_ICC_TRANSFORM_UNSUPPORTED: &str = "icc_transform_unsupported";
pub const CODE_ICC_TOO_LARGE: &str = "icc_too_large";
pub const CODE_ICC_IO: &str = "icc_io";
pub const CODE_ICC_NOT_FOUND: &str = "icc_not_found";
// 渲染链:
pub const CODE_RENDER_UNSUPPORTED: &str = "viewer_render_unsupported";
pub const CODE_RENDER_DECODE_FAILED: &str = "viewer_render_decode_failed";
pub const CODE_RENDER_TOO_LARGE: &str = "viewer_render_too_large";
pub const CODE_RENDER_IO: &str = "viewer_render_io";

/// 导入 ICC 的大小上限：16 MB。超限即 `icc_too_large`，不读入内存解析。
pub const ICC_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// 构造 `AppError::Color`。message 只写中文+英文提示语,**不携带路径 / 底层错误串**(泄漏面,
/// 硬约束;同 `Reveal`/`Player` 姿态)。
pub(crate) fn color_err(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::Color {
        code,
        message: message.into(),
    }
}
