// src-tauri/src/engine/traits.rs
//! 实施计划第 7.1 节中指定的 `ImageEngine` trait 定义。

use crate::error::AppError;
use std::path::Path;

/// Resize strategy hint for `ImageEngine::decode()`.
/// `ImageEngine::decode()` 的缩放策略提示。
#[derive(Debug, Clone, Copy)]
pub enum ResizeHint {
    /// 按**长边**适配到 `target`，保持纵横比（缩略图用）。
    /// Example: 6000×4000 + LongEdge(300) → 300×200
    LongEdge(u32),
    /// Scale so the **short** edge matches `target`, preserving aspect ratio (CLIP preprocessing).
    /// **Downscale only**: if the short edge is already <= `target`, decode at原尺寸 —
    /// AI/face cache contract "analysis never upsamples" (2026-07-06 审查 R2).
    /// 按**短边**适配到 `target`，保持纵横比（CLIP 预处理用）。**只下采样**:短边已 <= `target`
    /// 时按原尺寸解码——AI/face 缓存契约「分析只下采样,绝不上采样」。
    /// Example: 6000×4000 + ShortEdge(224) → 336×224; 300×200 + ShortEdge(336) → 300×200
    ShortEdge(u32),
}

/// 准备处理（缩略图生成、ThumbHash 等）的解码图像。
///
/// T15 起定义体迁至 `scrollery-ai-core::decoded`(推理核与解码层的唯一数据契约,
/// ai-worker 子进程需同一类型),此处再导出保持全库引用路径不变。
pub use scrollery_ai_core::DecodedImage;

/// 所有图像解码后端必须实现的 Trait。
pub trait ImageEngine: Send + Sync {
    fn name(&self) -> &str;

    fn supported_formats(&self) -> &[&str];

    fn can_handle(&self, format: &str) -> bool {
        self.supported_formats().contains(&format)
    }

    /// Fully decode the image at `file_path` into RGBA pixels,
    /// optionally resizing according to the given `ResizeHint`.
    /// 将 `file_path` 处的图像完全解码为 RGBA 像素，
    /// 可选地根据给定的 `ResizeHint` 进行缩放。
    fn decode(
        &self,
        file_path: &Path,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError>;

    /// 尝试提取嵌入的缩略图（例如 EXIF JPEG 缩略图）。
    /// 如果没有可用的嵌入缩略图，则返回 `Ok(None)`。
    fn extract_embedded_thumb(&self, _file_path: &Path) -> Result<Option<Vec<u8>>, AppError> {
        Ok(None)
    }
}
