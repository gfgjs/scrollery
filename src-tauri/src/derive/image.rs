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
use crate::engine::gpu::get_gpu_engine;
use crate::engine::image_rs::ImageRsEngine;
use crate::engine::traits::{DecodedImage, ImageEngine, ResizeHint};
use crate::error::{AppError, Result};
use crate::thumbnail::cache::{
    ai_cache_db_path, ai_cache_path, ensure_ai_cache_dir, face_cache_path, FACE_CACHE_SHORT_EDGE,
};

/// Decode the source at short-edge 336 (WIC GPU path, CPU `image` crate fallback), encode a
/// WebP, and write it to the AI cache dir keyed by `cache_key`. Skips the work (and re-decode)
/// if the cache file already exists — e.g. the thumbnail pipeline produced it in its own decode
/// pass (`generator.rs`), so this derivation only fills the gaps for already-thumbnailed images.
/// 按短边 336 解码源图（WIC GPU 路径，CPU `image` crate 回退），编码 WebP，按 `cache_key` 写入 AI 缓存目录。
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
    write_short_edge_webp(&disk, file_format, abs_path, FACE_CACHE_SHORT_EDGE)
}

/// 共用落盘核心:解码(短边 `short_edge`)→ WebP 编码 → tmp→rename 原子写到 `disk`。
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
/// GPU (WIC) engine and falling back to the CPU `image` crate engine — mirrors the AI
/// pipeline's own decode so the cache matches what analysis would otherwise produce.
/// 把图像解码为短边缩到 `target` 的 `DecodedImage`，优先 GPU（WIC）引擎、回退 CPU `image` crate
/// 引擎 —— 与 AI 流水线自身解码一致，使缓存与"直接分析"产物相同。
fn decode_short_edge(
    file_format: &str,
    path: &std::path::Path,
    target: u32,
) -> Result<DecodedImage> {
    let hint = Some(ResizeHint::ShortEdge(target));

    if let Some(gpu) = get_gpu_engine("wic") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail::cache::AI_CACHE_SHORT_EDGE;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("scrollery_aicache_{}_{}", name, std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 现场派生落盘 + 无 tmp 残留(原子写)+ 幂等(已存在不重解码、内容不变)。
    #[test]
    fn generate_ai_cache_atomic_and_idempotent() {
        let dir = temp_dir("gen");
        let src = dir.join("src.png");
        image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 120, 40, 255]))
            .save(&src)
            .unwrap();

        let key: i64 = 0x1234_5678_9abc_def0u64 as i64;
        generate_ai_cache(&dir, key, "png", &src, AI_CACHE_SHORT_EDGE).unwrap();

        let out = ai_cache_path(&dir, key);
        assert!(out.exists(), "缓存文件应已落盘");
        // 原子写契约:正式目录不得残留 *.tmp(预检只查存在性,半截文件=永久坏缓存)。
        let leftover_tmp = std::fs::read_dir(out.parent().unwrap())
            .unwrap()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "tmp"));
        assert!(!leftover_tmp, "不得残留 tmp 文件");
        // 产物必须可按 WebP 解码(worker 端 load_cache_image 按 WebP 显式解码)。
        let bytes = std::fs::read(&out).unwrap();
        image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .expect("产物应为可解码 WebP");

        // 幂等:再次调用不重写(内容逐字节不变)。
        generate_ai_cache(&dir, key, "png", &src, AI_CACHE_SHORT_EDGE).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 源文件缺失/不可解码 → Err(worker 派发路径据此标 Error,同进程内解码失败语义)。
    #[test]
    fn generate_ai_cache_missing_source_errors() {
        let dir = temp_dir("miss");
        let e = generate_ai_cache(
            &dir,
            42,
            "png",
            &dir.join("nonexistent.png"),
            AI_CACHE_SHORT_EDGE,
        );
        assert!(e.is_err());
        assert!(!ai_cache_path(&dir, 42).exists(), "失败不得留下缓存文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// face 缓存(方案 A):落盘 + 无 tmp 残留(原子写)+ 幂等,产物为可解码 WebP。
    #[test]
    fn generate_face_cache_atomic_and_idempotent() {
        let dir = temp_dir("face");
        let src = dir.join("src.png");
        image::RgbaImage::from_pixel(64, 48, image::Rgba([10, 200, 90, 255]))
            .save(&src)
            .unwrap();

        let key: i64 = 0x0fed_cba9_8765_4321u64 as i64;
        generate_face_cache(&dir, key, "png", &src).unwrap();

        let out = face_cache_path(&dir, key);
        assert!(out.exists(), "face 缓存应已落盘");
        let leftover_tmp = std::fs::read_dir(out.parent().unwrap())
            .unwrap()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "tmp"));
        assert!(!leftover_tmp, "不得残留 tmp 文件");
        let bytes = std::fs::read(&out).unwrap();
        image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .expect("产物应为可解码 WebP");

        generate_face_cache(&dir, key, "png", &src).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
