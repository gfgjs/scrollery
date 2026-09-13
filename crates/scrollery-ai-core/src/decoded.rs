// crates/scrollery-ai-core/src/decoded.rs
//! 解码后图像的最小载体(T15 自 src-tauri engine/traits.rs 迁入)。
//!
//! 这是推理核与图像解码层之间的唯一数据契约:RGBA8 平面 + 宽高。
//! src-tauri 的 `engine::traits` 再导出本类型,主进程各解码后端签名不变;
//! ai-worker 子进程自行解码(ai_cache WebP / 源文件)后构造同一类型喂推理。

/// 解码后的图像:原始 RGBA 像素 + 尺寸。
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// 原始 RGBA 像素数据。
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 源图 ICC profile 原始字节;`None` = 未知/无,按 sRGB 假定。
    /// 仅 `image_rs` 引擎(真实文件解码)才可能填充 `Some`;其余构造点(视频关键帧、
    /// 文档/音频封面、exotic sink、WIC 等)均为 `None`(见各自构造点注释)。
    pub icc: Option<Vec<u8>>,
}
