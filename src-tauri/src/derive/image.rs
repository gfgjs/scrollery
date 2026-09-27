//! Image derivation: the AI-analysis cache (§ AI pipeline). A pure `run(ctx) -> Result<Output>`;
//! the generic pipeline handles scheduling / resume / yield / orphan recovery.
//!
//! 图像派生：AI 分析缓存。纯函数 `run(ctx) -> Result<Output>`；通用流水线负责调度/续传/让步/孤儿恢复。
//!
//! # 为什么需要它
//! CLIP 分析按短边裁到 `image_size`（224/336）。若每次都解码全分辨率原图，24MP JPEG 的熵解码
//! 会把 CPU 全核占满、把 GPU 饿死（实测 CPU 99% / GPU 45%）。本派生预先把每张图缩成一份
//! **短边≥336** 的小 WebP，分析阶段只解这份小缓存 —— CPU 解码量降两个数量级，GPU 得以吃满。
//!
//! 短边取 336 而非 224：分析只会把短边**下采样**到 image_size、绝不上采样，故 336 同时覆盖
//! B/16·L/14（224）与 L/14@336（336）；做 224 则无法服务 336 模型且白占空间（用户要求不做 224）。
//!
//! T16-R2 方案 A:人脸管线同法炮制——`generate_face_cache` 产出短边 640(YuNet detect_size)
//! 的 `face_thumbs/` WebP,face 派发缺缓存时现场预解码(镜像 CLIP 的 T18 现场派生),worker
//! 端不再解全尺寸原图;顺带覆盖 exotic 原图(WIC 可解 heic 等,worker 的 image crate 不可)。

use crate::derive::kind::{DerivationContext, DerivationOutput};
use crate::engine::image_rs::ImageRsEngine;
use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{
    ai_cache_db_path, ai_cache_path, ensure_ai_cache_dir, face_cache_path, FACE_CACHE_SHORT_EDGE,
};

/// Decode the source at short-edge 336 (native WIC CPU path, `image` crate fallback), encode a
/// WebP, and write it to the AI cache dir keyed by `cache_key`. Skips the work (and re-decode)
/// if the cache file already exists — e.g. the thumbnail pipeline produced it in its own decode
/// pass (`generator.rs`), so this derivation only fills the gaps for already-thumbnailed images.
/// 按短边 336 解码源图（原生 WIC CPU 路径，`image` crate 回退），编码 WebP，按 `cache_key` 写入 AI 缓存目录。
/// 若缓存文件已存在则跳过（免去重复解码）—— 例如缩略图流水线已在自己的解码里顺带产出（见 generator.rs），
/// 本派生只为「已生成缩略图、缺 AI 缓存」的存量图补齐。
pub fn run_ai_thumb(ctx: &DerivationContext) -> Result<DerivationOutput> {
    generate_ai_cache_bounded(
        &ctx.cache_dir,
        ctx.cache_key,
        &ctx.file_format,
        &ctx.abs_path,
        ctx.ai_cache_short_edge,
        ctx.max_pixel_bytes,
    )?;
    Ok(DerivationOutput {
        payload_path: Some(ai_cache_db_path(ctx.cache_key)),
        thumbhash: None,
        page_count: None,
    })
}

/// 为一张图产出 ai_cache(短边 336 WebP);已在磁盘即幂等返回(缩略图流水线顺带产出或
/// 上次运行所产,免重复解码)。派生管线(`run_ai_thumb`)与 **worker 派发路径的 T18 降级**
/// (缺缓存回退解原图现场生成,Part4 §3.8)共用同一实现。
/// 写盘走 tmp→rename 原子替换(红线):缓存命中判定普遍只查存在性(派生跳过/AI 解码源
/// 发现/worker 派发预检),直写崩溃会把半截文件永久当作有效缓存。
pub(crate) fn generate_ai_cache(
    cache_dir: &std::path::Path,
    cache_key: i64,
    file_format: &str,
    abs_path: &std::path::Path,
    short_edge: u32,
) -> Result<()> {
    generate_ai_cache_bounded(
        cache_dir,
        cache_key,
        file_format,
        abs_path,
        short_edge,
        512 * 1024 * 1024,
    )
}

fn generate_ai_cache_bounded(
    cache_dir: &std::path::Path,
    cache_key: i64,
    file_format: &str,
    abs_path: &std::path::Path,
    short_edge: u32,
    max_pixel_bytes: u64,
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
        max_pixel_bytes,
    )
}

/// 为一张图产出 face 缓存(短边 640 WebP,T16-R2 方案 A);已在磁盘即幂等返回。
/// face worker 派发的「缺缓存现场预解码」调用(镜像 CLIP 的 T18 降级);解码引擎与
/// ai_cache 同源(WIC 优先/CPU 回退),故 exotic 原图(heic 等)也在覆盖内。
/// 原子写契约同 `generate_ai_cache`(命中判定只查存在性,半截文件=永久坏缓存)。
pub(crate) fn generate_face_cache(
    cache_dir: &std::path::Path,
    cache_key: i64,
    file_format: &str,
    abs_path: &std::path::Path,
) -> Result<()> {
    let disk = face_cache_path(cache_dir, cache_key);
    if disk.exists() {
        return Ok(());
    }
    if let Some(parent) = disk.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    write_short_edge_webp(
        &disk,
        file_format,
        abs_path,
        FACE_CACHE_SHORT_EDGE,
        512 * 1024 * 1024,
    )
}

/// 共用落盘核心:解码(短边 `short_edge`)→ WebP 编码 → tmp→rename 原子写到 `disk`。
fn write_short_edge_webp(
    disk: &std::path::Path,
    file_format: &str,
    abs_path: &std::path::Path,
    short_edge: u32,
    max_pixel_bytes: u64,
) -> Result<()> {
    let decoded = decode_short_edge(file_format, abs_path, short_edge, max_pixel_bytes)?;

    let (w, h) = (decoded.width, decoded.height);
    let rgba = image::RgbaImage::from_raw(w, h, decoded.pixels).ok_or_else(|| {
        AppError::Internal("AI cache buffer size mismatch | AI 缓存缓冲尺寸不符".into())
    })?;

    // Reuse the thumbnail WebP/JPEG encoders. Analysis caches (AI/face) keep the fixed
    // default quality — decoupled from the user-facing display-quality setting.
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

/// Decode an image to a `DecodedImage` with short edge resized to `target`, preferring the
/// native WIC CPU engine and falling back to the `image` crate engine — mirrors the AI
/// pipeline's own decode so the cache matches what analysis would otherwise produce.
/// 把图像解码为短边缩到 `target` 的 `DecodedImage`，优先原生 WIC CPU 引擎、回退 `image` crate
/// 引擎 —— 与 AI 流水线自身解码一致，使缓存与"直接分析"产物相同。
fn decode_short_edge(
    file_format: &str,
    path: &std::path::Path,
    target: u32,
    max_pixel_bytes: u64,
) -> Result<DecodedImage> {
    let hint = Some(ResizeHint::ShortEdge(target));

    #[cfg(windows)]
    {
        use crate::engine::native::wic_engine::WicEngine;
        if WicEngine.can_handle(file_format) {
            match WicEngine::decode_open_file_bounded(
                std::fs::File::open(path)?,
                file_format,
                hint,
                max_pixel_bytes,
            ) {
                Ok(d) => return Ok(d),
                // reviewer 深审修正:这条服务 AI/人脸分析缓存生成(见本函数调用方 generate_ai_cache
                // /generate_face_cache),与视频派生无关,不应挂 video target(按 ai 过滤会漏看)。
                Err(e) => tracing::debug!(
                    target: "scrollery::pipeline::ai",
                    error = %e,
                    "AI cache WIC decode failed, falling back to image-rs"
                ),
            }
        }
    }

    if !ImageRsEngine.can_handle(file_format) {
        return Err(AppError::UnsupportedFormat(file_format.to_string()));
    }
    ImageRsEngine::decode_open_file_bounded(
        std::fs::File::open(path)?,
        file_format,
        hint,
        max_pixel_bytes,
    )
}
