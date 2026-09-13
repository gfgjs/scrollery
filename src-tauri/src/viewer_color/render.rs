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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use lcms2::{CIExyY, CIExyYTRIPLE, Intent, PixelFormat, Profile, ToneCurve, Transform};

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("scrollery_vc_render_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 极小 TIFF/EXIF blob:仅 Orientation 标签(方案 §0③ orientation 烤入用例的源样本构造)。
    fn exif_orientation(value: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        b.extend_from_slice(&42u16.to_le_bytes());
        b.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
        b.extend_from_slice(&1u16.to_le_bytes()); // entry count
        b.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation tag
        b.extend_from_slice(&3u16.to_le_bytes()); // type SHORT
        b.extend_from_slice(&1u32.to_le_bytes()); // count
        b.extend_from_slice(&u32::from(value).to_le_bytes()); // value (low bytes)
        b.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
        b
    }

    /// ⑧-2:有 alpha 源 → PNG 输出;命中检查复用。
    #[test]
    fn alpha_source_renders_png_and_hits_cache() {
        let dir = tmp_dir("alpha");
        let src = dir.join("src.png");
        // RGBA 源(含半透明像素 → has_alpha=true)。
        let img = DynamicImage::ImageRgba8(ImageBuffer::from_fn(3, 2, |x, _| {
            Rgba([200, 100, 50, if x == 0 { 128 } else { 255 }])
        }));
        img.save(&src).unwrap();

        let out = ensure_derivative(
            &src,
            "png",
            0x1111,
            &dir,
            &ViewerColorTarget::DisplayP3,
            Path::new("C:/unused"),
        )
        .unwrap();
        assert_eq!(out.extension().unwrap(), "png");
        assert!(out.exists());
        assert!(out.to_string_lossy().contains("display-p3"));
        // 再调一次走命中路径,返回同一路径。
        let hit = ensure_derivative(
            &src,
            "png",
            0x1111,
            &dir,
            &ViewerColorTarget::DisplayP3,
            Path::new("C:/unused"),
        )
        .unwrap();
        assert_eq!(hit, out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑧-2:无 alpha 源 → JPEG 输出。
    #[test]
    fn opaque_source_renders_jpeg() {
        let dir = tmp_dir("opaque");
        let src = dir.join("src.png");
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_fn(4, 3, |x, y| {
            image::Rgb([(x * 40) as u8, (y * 50) as u8, 90])
        }));
        img.save(&src).unwrap();

        let out = ensure_derivative(
            &src,
            "png",
            0x2222,
            &dir,
            &ViewerColorTarget::DisplayP3,
            Path::new("C:/unused"),
        )
        .unwrap();
        assert_eq!(out.extension().unwrap(), "jpg");
        assert!(out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑧-2:orientation 烤入——非方形 JPEG 带 EXIF orientation=6(Rotate90),输出复读
    /// 尺寸已交换且 orientation 归一(NoTransforms,镜像 io.rs 复读契约)。
    #[test]
    fn orientation_baked_into_pixels() {
        let dir = tmp_dir("orient");
        let src = dir.join("src.jpg");
        // 6x4 非方形,写入 orientation=6。
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_fn(6, 4, |x, y| {
            image::Rgb([(x * 30) as u8, (y * 40) as u8, 20])
        }));
        let mut bytes = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut bytes, 95);
        enc.set_exif_metadata(exif_orientation(6)).unwrap();
        img.write_with_encoder(enc).unwrap();
        std::fs::write(&src, &bytes).unwrap();

        let out = ensure_derivative(
            &src,
            "jpg",
            0x3333,
            &dir,
            &ViewerColorTarget::DisplayP3,
            Path::new("C:/unused"),
        )
        .unwrap();
        // 输出无 alpha → jpg;复读:尺寸交换为 4x6,orientation 已归一。
        let reader = ImageReader::open(&out)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        let mut decoder = reader.into_decoder().unwrap();
        let meta = metadata::read_source_metadata(&mut decoder);
        let (dw, dh) = decoder.dimensions();
        assert_eq!((dw, dh), (4, 6), "orientation 应已烤入像素并交换宽高");
        assert_eq!(meta.orientation, image::metadata::Orientation::NoTransforms);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn point(x: f64, y: f64) -> CIExyY {
        CIExyY { x, y, Y: 1.0 }
    }

    /// ⑧-3 特征测试(全线最值钱,钉死 D-412 两半):AdobeRGB 源 → Display P3 派生,复读输出断言
    /// (a) 嵌入 ICC == `DisplayP3.resolve_profile().1`(即 `new_display_p3().encode()`);
    /// (b) 像素与「lcms2 AdobeRGB→(moxcms 嵌入的同一 P3 字节构造的目标 profile)」参考 ≤2 code value。
    /// 用 RGBA 源使输出走无损 PNG,排除 JPEG 量化对 ≤2 容差的干扰(apples-to-apples,镜像 color.rs 姿态)。
    #[test]
    fn adobe_rgb_to_display_p3_embeds_target_icc_and_matches_lcms() {
        let dir = tmp_dir("feature");
        let dummy = Path::new("C:/unused");
        let (_p3_obj, p3_bytes) = ViewerColorTarget::DisplayP3.resolve_profile(dummy).unwrap();

        // AdobeRGB 源 profile(lcms 构造,与 color.rs 同参数)。
        let adobe_curve = ToneCurve::new(2.199_218_75);
        let adobe = Profile::new_rgb(
            &point(0.3127, 0.3290),
            &CIExyYTRIPLE {
                Red: point(0.64, 0.33),
                Green: point(0.21, 0.71),
                Blue: point(0.15, 0.06),
            },
            &[&adobe_curve, &adobe_curve, &adobe_curve],
        )
        .unwrap();
        let adobe_icc = adobe.icc().unwrap();

        // RGBA 源(alpha 全 255 → 输出走无损 PNG),嵌入 AdobeRGB ICC。
        let pixels: Vec<u8> = vec![
            0, 0, 0, 255, 16, 64, 220, 255, 32, 128, 240, 255, 128, 64, 255, 255, 240, 180, 20,
            255, 255, 255, 255, 255,
        ];
        let rgba: RgbaImage = ImageBuffer::from_raw(6, 1, pixels.clone()).unwrap();
        let mut src_bytes = Vec::new();
        {
            let mut enc = PngEncoder::new(&mut src_bytes);
            enc.set_icc_profile(adobe_icc.clone()).unwrap();
            enc.write_image(rgba.as_raw(), 6, 1, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        let src = dir.join("adobe.png");
        std::fs::write(&src, &src_bytes).unwrap();

        let out = ensure_derivative(
            &src,
            "png",
            0x4444,
            &dir,
            &ViewerColorTarget::DisplayP3,
            dummy,
        )
        .unwrap();
        assert_eq!(out.extension().unwrap(), "png");

        // (a) 嵌入 ICC == target.encode() 字节。
        let out_bytes = std::fs::read(&out).unwrap();
        let reader = ImageReader::new(std::io::Cursor::new(&out_bytes))
            .with_guessed_format()
            .unwrap();
        let mut decoder = reader.into_decoder().unwrap();
        let embedded = decoder
            .icc_profile()
            .unwrap()
            .expect("输出应嵌入 target ICC");
        // moxcms `encode()` 会把 creation_date_time(ICC header 24..36 字节,秒粒度)写入序列
        // 化结果;本用例分两次构造 P3 profile(此处 setup 用的 `resolve_profile` 一次、
        // `ensure_derivative` 内部派生时又一次),若两次调用横跨系统时钟的秒边界,该字段会
        // 不同,导致逐字节比较偶发假失败(与 profile 内容本身无关)。断言前把两侧该区清零。
        let mut embedded_stable = embedded.clone();
        let mut p3_bytes_stable = p3_bytes.clone();
        for buf in [&mut embedded_stable, &mut p3_bytes_stable] {
            if buf.len() >= 36 {
                buf[24..36].fill(0);
            }
        }
        assert_eq!(
            embedded_stable, p3_bytes_stable,
            "嵌入 ICC 必须是 target.encode() 字节(D-412 上半,忽略秒粒度 creation_date_time)"
        );
        let out_img = DynamicImage::from_decoder(decoder).unwrap().to_rgba8();

        // (b) lcms 参考:AdobeRGB → (由 moxcms 嵌入的同一 P3 字节构造的目标 profile)。
        let dest = Profile::new_icc(&p3_bytes).unwrap();
        let transform = Transform::<[u8; 3], [u8; 3]>::new(
            &adobe,
            PixelFormat::RGB_8,
            &dest,
            PixelFormat::RGB_8,
            Intent::RelativeColorimetric,
        )
        .unwrap();
        let input: Vec<[u8; 3]> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut reference = vec![[0u8; 3]; input.len()];
        transform.transform_pixels(&input, &mut reference);

        for (px, expected) in out_img.pixels().zip(reference) {
            for ch in 0..3 {
                assert!(
                    px[ch].abs_diff(expected[ch]) <= 2,
                    "channel {ch}: mox={} lcms={}",
                    px[ch],
                    expected[ch]
                );
            }
            assert_eq!(px[3], 255, "alpha 应保持");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
