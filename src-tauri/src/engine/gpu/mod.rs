//! Windows 图像 GPU 变换后端；编解码与 WebP/ThumbHash 仍在 CPU。

#[cfg(windows)]
pub mod d2d_resize;

#[cfg(windows)]
pub(crate) mod budget;

#[cfg(all(windows, feature = "native-vpl"))]
pub mod vpl_jpeg;
