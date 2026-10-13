//! 图像派生：生成 AI 分析和人脸检测使用的小图缓存。
//!
//! # 为什么需要它
//! CLIP 分析按短边裁到模型尺寸。预先生成小图缓存，避免每次分析都解码全分辨率原图。
//!
//! 缓存短边由 `ai_cache_short_edge` 设置决定。
//!
//! `generate_face_cache` 产出短边 640(YuNet detect_size)
//! 的 `face_thumbs/` WebP,face 派发缺缓存时现场预解码(镜像 CLIP 的 T18 现场派生),worker
//! 端不再解全尺寸原图;顺带覆盖 exotic 原图(WIC 可解 heic 等,worker 的 image crate 不可)。

use crate::derive::kind::{DerivationContext, DerivationOutput};
#[cfg(windows)]
use crate::engine::gpu::wic_engine::WicEngine;
use crate::engine::image_rs::ImageRsEngine;
use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{
    ai_cache_db_path, ai_cache_path, ensure_ai_cache_dir, face_source_cache_path,
    FACE_CACHE_SHORT_EDGE,
};

/// 按配置短边解码源图（WIC GPU 路径，CPU `image` crate 回退），按 `cache_key` 写入 AI 缓存目录。
/// 优先编码 WebP，失败时回退 JPEG。
/// 若缓存文件已存在则跳过（免去重复解码）—— 例如缩略图流水线已在自己的解码里顺带产出（见 generator.rs），
/// 本派生只为「已生成缩略图、缺 AI 缓存」的存量图补齐。
pub fn run_ai_thumb(ctx: &DerivationContext) -> Result<DerivationOutput> {
    generate_ai_cache(
        &ctx.cache_dir,
        ctx.cache_key,
        &ctx.file_format,
        &ctx.abs_path,
        ctx.ai_cache_short_edge,
    )?;
    Ok(DerivationOutput {
        payload_path: Some(ai_cache_db_path(ctx.cache_key)),
        thumbhash: None,
        page_count: None,
    })
}

/// 为一张图产出指定短边的 ai_cache；已在磁盘即返回，避免重复解码。
/// 派生管线与 worker 派发时的缺缓存补齐共用此实现。
/// 写盘走 tmp→rename 原子替换(红线):缓存命中判定普遍只查存在性(派生跳过/AI 解码源
/// 发现/worker 派发预检),直写崩溃会把半截文件永久当作有效缓存。
pub(crate) fn generate_ai_cache(
    cache_dir: &std::path::Path,
    cache_key: i64,
    file_format: &str,
    abs_path: &std::path::Path,
    short_edge: u32,
) -> Result<()> {
    if ai_cache_path(cache_dir, cache_key).exists() {
        return Ok(());
    }
    ensure_ai_cache_dir(cache_dir, cache_key).map_err(AppError::Io)?;
    write_short_edge_webp(
        &ai_cache_path(cache_dir, cache_key),
        file_format,
        abs_path,
        short_edge,
    )
}

/// 为一张图产出短边 640 的 face 缓存；已在磁盘即返回。
/// face worker 派发时也调用此函数补齐缺失缓存；解码引擎与
/// ai_cache 同源(WIC 优先/CPU 回退),故 exotic 原图(heic 等)也在覆盖内。
/// 原子写契约同 `generate_ai_cache`(命中判定只查存在性,半截文件=永久坏缓存)。
pub(crate) fn generate_face_cache(
    cache_dir: &std::path::Path,
    cache_key: i64,
    source_revision: i64,
    file_format: &str,
    abs_path: &std::path::Path,
) -> Result<()> {
    let disk = face_source_cache_path(cache_dir, cache_key, source_revision);
    if disk.exists() {
        return Ok(());
    }
    if let Some(parent) = disk.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    write_short_edge_webp(&disk, file_format, abs_path, FACE_CACHE_SHORT_EDGE)
}

/// 共用落盘核心：解码到短边 `short_edge`，优先 WebP、失败回退 JPEG，再原子写到 `disk`。
fn write_short_edge_webp(
    disk: &std::path::Path,
    file_format: &str,
    abs_path: &std::path::Path,
    short_edge: u32,
) -> Result<()> {
    let decoded = decode_short_edge(file_format, abs_path, short_edge)?;

    let (w, h) = (decoded.width, decoded.height);
    let rgba = image::RgbaImage::from_raw(w, h, decoded.pixels).ok_or_else(|| {
        AppError::Internal("AI cache buffer size mismatch | AI 缓存缓冲尺寸不符".into())
    })?;

    // 复用缩略图的 WebP/JPEG 编码器。分析缓存(AI/人脸)恒用默认质量,与显示质量设置解耦。
    let bytes = crate::thumbnail::exif_thumb::encode_as_webp(
        &rgba,
        crate::thumbnail::exif_thumb::DEFAULT_WEBP_QUALITY,
    )
    .or_else(|_| crate::thumbnail::exif_thumb::encode_as_jpeg(&rgba))
    .map_err(|_| {
        AppError::Internal("AI cache WebP encode failed | AI 缓存 WebP 编码失败".into())
    })?;

    crate::thumbnail::generator::write_atomic(disk, &bytes).map_err(AppError::from)
}

/// 把图像解码为短边缩到 `target` 的 `DecodedImage`，优先 GPU（WIC）引擎、回退 CPU `image` crate
/// 引擎 —— 与 AI 流水线自身解码一致，使缓存与"直接分析"产物相同。
fn decode_short_edge(
    file_format: &str,
    path: &std::path::Path,
    target: u32,
) -> Result<DecodedImage> {
    let hint = Some(ResizeHint::ShortEdge(target));

    #[cfg(windows)]
    {
        let gpu = WicEngine;
        if gpu.can_handle(file_format) {
            match gpu.decode(path, hint) {
                Ok(d) => return Ok(d),
                // reviewer 深审修正:这条服务 AI/人脸分析缓存生成(见本函数调用方 generate_ai_cache
                // /generate_face_cache),与视频派生无关,不应挂 video target(按 ai 过滤会漏看)。
                Err(e) => tracing::debug!(
                    target: "scrollery::pipeline::ai",
                    error = %e,
                    "AI cache GPU decode failed, falling back to CPU"
                ),
            }
        }
    }

    if !ImageRsEngine.can_handle(file_format) {
        return Err(AppError::UnsupportedFormat(file_format.to_string()));
    }
    ImageRsEngine.decode(path, hint)
}
