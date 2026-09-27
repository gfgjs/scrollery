//! 查看器色域派生的渲染主体(方案 §0②③)。解码源图 → 内存预算准入 → orientation 烤入像素 →
//! CMS 投影到 target 色域(复用 `editing::color::to_target_rgba8`)→ 按 alpha 判定编码格式
//! (有 alpha→PNG 无损;无 alpha→JPEG q92)嵌入 target ICC → 同卷原子落盘。
//!
//! **不经 media_derivations 记行**:命中判定只查 `exists()`,缺失即自愈重渲(与 motion_video
//! 同姿态)。mtime 变→cache_key 变→旧派生成孤儿,归 `thumbnail::cache::reconcile_orphan_gc`。

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader, RgbImage, RgbaImage};

use super::target::ViewerColorTarget;
use super::{
    color_err, CODE_RENDER_DECODE_FAILED, CODE_RENDER_IO, CODE_RENDER_TOO_LARGE,
    CODE_RENDER_UNSUPPORTED,
};
use crate::editing::{color, memory_budget, metadata};
use crate::error::AppError;
use crate::thumbnail::cache::{ensure_viewer_color_dir, viewer_color_path};

/// JPEG 输出质量(无 alpha 分支)。与 `edit_commands` 同档(方案 §0③)。
const JPEG_QUALITY: u8 = 92;

/// 派生输入承诺可稳定解码的静态格式(镜像 `edit_commands::SUPPORTED_INPUT_EXTS`)。gif 不在其列
/// (动画帧契约未定义);heic/avif 依赖 WIC 非跨平台一致。
const SUPPORTED_INPUT_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif"];

/// `source_ext`(不含点,大小写不敏感)是否在派生输入白名单内。
pub fn is_supported_input_ext(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    SUPPORTED_INPUT_EXTS.contains(&lower.as_str())
}

/// 命中检查:同 `(target_id, cache_key)` 的 jpg 优先、png 次之,任一存在即返其路径。
/// (渲染按 alpha 决定 ext,故命中要覆盖两种可能落盘的扩展名。)
pub fn hit_path(cache_dir: &Path, target_id: &str, cache_key: i64) -> Option<PathBuf> {
    for ext in ["jpg", "png"] {
        let p = viewer_color_path(cache_dir, target_id, cache_key, ext);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 确保 `(item 源, target)` 的派生存在,返回绝对路径。命中直接返回;否则渲染 + 原子落盘。
/// 纯文件级函数(不碰 DB),调用方在 `spawn_blocking` 内、持 keyed lock 时调用。
pub fn ensure_derivative(
    source_path: &Path,
    source_ext: &str,
    cache_key: i64,
    cache_dir: &Path,
    target: &ViewerColorTarget,
    app_data_dir: &Path,
) -> Result<PathBuf, AppError> {
    let target_id = target.target_id();

    // ① 命中检查(在解码/装配 profile 之前,省掉热路径开销)。
    if let Some(hit) = hit_path(cache_dir, &target_id, cache_key) {
        return Ok(hit);
    }

    // ② 动画 WebP 前置拒绝(方案 §3 边界 7):image WebP 解码器对动画只出首帧,派生首帧会误导。
    if source_ext.eq_ignore_ascii_case("webp") && webp_is_animated(source_path) {
        return Err(color_err(
            CODE_RENDER_UNSUPPORTED,
            "动画 WebP 不支持色域派生 | animated WebP not supported",
        ));
    }

    // ③ target profile 变换对象 + 嵌入字节(同源,D-412)。
    let (target_profile, icc_bytes) = target.resolve_profile(app_data_dir)?;

    // ④ 解码 + 元数据。
    let reader = ImageReader::open(source_path)
        .map_err(|_| color_err(CODE_RENDER_IO, "源文件读取失败 | failed to open source"))?
        .with_guessed_format()
        .map_err(|_| {
            color_err(
                CODE_RENDER_DECODE_FAILED,
                "无法识别图像格式 | unrecognized format",
            )
        })?;
    let mut decoder = reader.into_decoder().map_err(|_| {
        color_err(
            CODE_RENDER_DECODE_FAILED,
            "解码器构造失败 | decoder init failed",
        )
    })?;

    // 内存预算前置(方案 §0③,100MP 准入):大缓冲分配前拒绝。
    let (w, h) = decoder.dimensions();
    if memory_budget::exceeds_memory_budget(w, h) {
        return Err(color_err(
            CODE_RENDER_TOO_LARGE,
            "图像超出内存预算 | image exceeds memory budget",
        ));
    }
    let src_meta = metadata::read_source_metadata(&mut decoder);
    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|_| color_err(CODE_RENDER_DECODE_FAILED, "解码失败 | decode failed"))?;

    // ⑤ orientation 烤入像素。与查看器显示一致(D-009:仅 jpg 应用 EXIF orientation,防 PNG 双旋)——
    // 派生图替代原图显示,旋转须与用户所见一致。
    let orientation = metadata::effective_source_orientation(source_ext, src_meta.orientation);
    img.apply_orientation(orientation);
    let has_alpha = img.color().has_alpha();

    // ⑥ CMS 投影到 target(无 ICC 源假定 sRGB 仍转换,D-411/边界 1)。变换失败(布局与空间不符等)
    // → viewer_render_decode_failed(不透传 editing 侧的 edit_decode_failed 码)。
    let rgba = color::to_target_rgba8(&img, src_meta.icc_profile.as_deref(), &target_profile)
        .map_err(|_| {
            color_err(
                CODE_RENDER_DECODE_FAILED,
                "色彩变换失败 | color transform failed",
            )
        })?;

    // ⑦ alpha→PNG(无损,RGBA 直编);无 alpha→JPEG(先转 RGB,JpegEncoder 不收 RGBA)。
    let (bytes, ext) = if has_alpha {
        (encode_png(&rgba, &icc_bytes)?, "png")
    } else {
        let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
        (encode_jpeg(&rgb, &icc_bytes)?, "jpg")
    };

    // ⑧ 原子落盘(tmp + 同卷 rename;命中判定只查 exists(),半截文件不得被当作有效缓存)。
    ensure_viewer_color_dir(cache_dir, &target_id, cache_key, ext)
        .map_err(|_| color_err(CODE_RENDER_IO, "创建缓存目录失败 | mkdir cache dir failed"))?;
    let out_path = viewer_color_path(cache_dir, &target_id, cache_key, ext);
    crate::thumbnail::generator::write_atomic(&out_path, &bytes)
        .map_err(|_| color_err(CODE_RENDER_IO, "派生落盘失败 | derivative write failed"))?;
    Ok(out_path)
}

/// 有损 JPEG 编码 + 嵌入 target ICC。
fn encode_jpeg(rgb: &RgbImage, icc: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    let _ = enc.set_icc_profile(icc.to_vec());
    enc.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|_| color_err(CODE_RENDER_IO, "JPEG 编码失败 | jpeg encode failed"))?;
    Ok(out)
}

/// 无损 PNG 编码(RGBA 直编)+ 嵌入 target ICC。
fn encode_png(rgba: &RgbaImage, icc: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    let mut enc = PngEncoder::new(&mut out);
    let _ = enc.set_icc_profile(icc.to_vec());
    enc.write_image(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|_| color_err(CODE_RENDER_IO, "PNG 编码失败 | png encode failed"))?;
    Ok(out)
}

/// 是否为动画 WebP(best-effort:构造失败或非动画一律 false,交后续通用解码处理)。
fn webp_is_animated(source_path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(source_path) else {
        return false;
    };
    image::codecs::webp::WebPDecoder::new(std::io::BufReader::new(file))
        .map(|d| d.has_animation())
        .unwrap_or(false)
}
