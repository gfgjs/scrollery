//! 图像解码后端的公共接口。

use crate::error::AppError;
use std::path::Path;

/// `ImageEngine::decode()` 的缩放策略提示。
#[derive(Debug, Clone, Copy)]
pub enum ResizeHint {
    /// 按**长边**适配到 `target`，保持纵横比（缩略图用）。
    /// 例如 6000×4000 + LongEdge(300) → 300×200。
    LongEdge(u32),
    /// 按**短边**适配到 `target`，保持纵横比（CLIP 预处理用）。**只下采样**:短边已 <= `target`
    /// 时按原尺寸解码——AI/face 缓存契约「分析只下采样,绝不上采样」。
    /// 例如 6000×4000 + ShortEdge(224) → 336×224；300×200 + ShortEdge(336) → 300×200。
    ShortEdge(u32),
}

/// 准备处理（缩略图生成、ThumbHash 等）的解码图像。
///
/// 与 ai-worker 共用 `scrollery-ai-core::decoded` 中的类型，并保留本库的引用路径。
pub use scrollery_ai_core::DecodedImage;

/// 所有图像解码后端必须实现的 Trait。
pub trait ImageEngine: Send + Sync {
    fn name(&self) -> &str;

    fn supported_formats(&self) -> &[&str];

    fn can_handle(&self, format: &str) -> bool {
        self.supported_formats().contains(&format)
    }

    /// 将 `file_path` 处的图像完全解码为 RGBA 像素，
    /// 可选地根据给定的 `ResizeHint` 进行缩放。
    fn decode(
        &self,
        file_path: &Path,
        resize: Option<ResizeHint>,
    ) -> Result<DecodedImage, AppError>;
}
