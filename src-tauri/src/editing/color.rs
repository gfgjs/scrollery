//! 图片编辑色彩管理公共入口。
//!
//! 预览与后续调色保存都必须先把带 ICC 的源像素转换到 sRGB；无 ICC 时按业界惯例假定
//! 已是 sRGB。生产实现只用纯 Rust `moxcms`，`lcms2` 仅在测试中作参考对拍。

use image::{DynamicImage, RgbaImage};
use moxcms::{ColorProfile, DataColorSpace, Layout, RenderingIntent, TransformOptions};

use crate::error::AppError;

const CODE_DECODE_FAILED: &str = "edit_decode_failed";

/// 与 `editing::adjust` 共用:CMS 失败统一落 `edit_decode_failed` 稳定码。
pub(super) fn cms_error() -> AppError {
    AppError::Edit {
        code: CODE_DECODE_FAILED,
        message: "源图 ICC profile 无法安全转换 | source ICC profile conversion failed".into(),
    }
}

/// 与 `editing::adjust` 共用:预览与保存链必须同 intent(relative colorimetric,P0-CM 裁决)。
/// `pub(crate)`(而非 `pub(super)`):B 线(2026-07-23)查看器色域渲染的 ICC 导入变换探针
/// (`ipc::viewer_color_commands`)复用同一 intent,不重复定义一份可能漂移的常量。
pub(crate) fn options() -> TransformOptions {
    TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        ..TransformOptions::default()
    }
}

/// 把任意 `DynamicImage` 投影到 `target` 色域编码的 RGBA8(B 线,2026-07-23 泛化自原
/// `to_srgb_rgba8`)。**无 ICC 源时的分支差异**(计划 §0③/边界情况1):`target` 恰为 sRGB
/// (编辑链 `to_srgb_rgba8` 的唯一调用姿态)时原样返回、不经 CMS 数值管线——既有契约
/// (D-413)锁的是「不经变换的原始字节相等」,sRGB→sRGB 恒等变换在离散 8-bit LUT 下不保证
/// 逐字节相等,故该分支由 `to_srgb_rgba8` 自身短路处理,不进入本函数;本函数被直接以
/// 非 sRGB target 调用时(查看器渲染,D-411),无 ICC 源假定源已是 sRGB 仍须转换。
///
/// 16-bit / f32 源先在原位深完成 CMS，再量化为预览所需的 8-bit；全分辨率保存链会复用
/// 同一 profile 解析和 intent，但保持源位深。
pub fn to_target_rgba8(
    image: &DynamicImage,
    icc_profile: Option<&[u8]>,
    target: &ColorProfile,
) -> Result<RgbaImage, AppError> {
    let Some(icc) = icc_profile else {
        // 查看器侧专属分支(编辑链的 sRGB 短路已在 `to_srgb_rgba8` 里处理,不会走到这里):
        // 假定源为 sRGB,仍须投影到 target(边界情况1)。任意色型(Luma/LumaA/Rgb/Rgba/…)
        // 统一先 `to_rgba8()` 展平成 RGBA 缓冲,再按 (Rgb,Rgba) 走 CMS——灰度→RGB 是色度学
        // 无损扩展,不引入误差;而若照搬 `transform_dynamic_image` 原有的按 `DynamicImage`
        // 变体分派逻辑(要求 source.color_space 与缓冲变体匹配),灰度缓冲配 Rgb 假定源
        // 永远落 `_ => Err`——非 sRGB target 下灰度图完全不可渲染,是本次 B 线复核修复的破口。
        let assumed_source = ColorProfile::new_srgb();
        let rgba = image.to_rgba8();
        return transform_u8(
            &assumed_source,
            target,
            Layout::Rgba,
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
        );
    };
    let source = ColorProfile::new_from_slice(icc).map_err(|_| cms_error())?;

    // `image` 的 JPEG 解码器已经把 CMYK 样本转换为 RGB(plan-B §3 边界2);此时再把解码后
    // 的 RGB 缓冲按 CMYK layout 喂给 ICC 会二次误解通道,moxcms 不参与该边界。此前直接
    // `Ok(image.to_rgba8())` 短路却仍在外层嵌入 target ICC 元数据的做法是 D-412 破口——
    // 像素其实从未投影,只是被谎称已按 target 编码。这里与 None 分支保持一致:假定解码后
    // 的 RGB 结果为 sRGB 编码,显式投影到 target。
    if source.color_space == DataColorSpace::Cmyk {
        let assumed_source = ColorProfile::new_srgb();
        let rgba = image.to_rgba8();
        return transform_u8(
            &assumed_source,
            target,
            Layout::Rgba,
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
        );
    }
    transform_dynamic_image(&source, target, image)
}

/// 把任意 `DynamicImage` 投影为 sRGB 编码域 RGBA8，供编辑预览使用。薄封装：无 ICC 源时
/// 原样返回（D-413 既有契约，见 [`to_target_rgba8`] 文档的分支差异说明），否则委托给
/// 泛化后的 [`to_target_rgba8`]（`target = ColorProfile::new_srgb()`）。
pub fn to_srgb_rgba8(
    image: &DynamicImage,
    icc_profile: Option<&[u8]>,
) -> Result<RgbaImage, AppError> {
    let Some(icc) = icc_profile else {
        return Ok(image.to_rgba8());
    };
    // CMYK 源短路(D-413 既有契约维持):`to_target_rgba8` 泛化后对 CMYK 会做真实的
    // sRGB→target 投影,但编辑链这里 target 恒为 sRGB,该投影退化为 sRGB→sRGB 却仍要
    // 经过离散 8-bit LUT,不保证逐字节相等——与既有「无 ICC」短路同理,编辑链在此维持
    // 「解码器已产出 RGB,直接返回」的原始行为,不进入泛化后的投影路径(该路径只供查看器
    // 等非 sRGB target 调用方使用)。解析失败时不在此处处理,交给下方 `to_target_rgba8`
    // 统一走稳定的 `cms_error()` 错误码。
    if matches!(
        ColorProfile::new_from_slice(icc).map(|profile| profile.color_space),
        Ok(DataColorSpace::Cmyk)
    ) {
        return Ok(image.to_rgba8());
    }
    to_target_rgba8(image, icc_profile, &ColorProfile::new_srgb())
}

/// `to_srgb_rgba8`/`to_target_rgba8` 共用的变换主体(原 `to_srgb_rgba8` 内联逻辑,B 线抽出以
/// 支持任意 `target`,而非硬编码 `ColorProfile::new_srgb()`)。
fn transform_dynamic_image(
    source: &ColorProfile,
    target: &ColorProfile,
    image: &DynamicImage,
) -> Result<RgbaImage, AppError> {
    match (source.color_space, image) {
        (DataColorSpace::Gray, DynamicImage::ImageLuma8(src)) => transform_u8(
            source,
            target,
            Layout::Gray,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Gray, DynamicImage::ImageLumaA8(src)) => transform_u8(
            source,
            target,
            Layout::GrayAlpha,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Gray, DynamicImage::ImageLuma16(src)) => transform_u16(
            source,
            target,
            Layout::Gray,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Gray, DynamicImage::ImageLumaA16(src)) => transform_u16(
            source,
            target,
            Layout::GrayAlpha,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Rgb, DynamicImage::ImageRgb8(src)) => transform_u8(
            source,
            target,
            Layout::Rgb,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Rgb, DynamicImage::ImageRgba8(src)) => transform_u8(
            source,
            target,
            Layout::Rgba,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Rgb, DynamicImage::ImageRgb16(src)) => transform_u16(
            source,
            target,
            Layout::Rgb,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Rgb, DynamicImage::ImageRgba16(src)) => transform_u16(
            source,
            target,
            Layout::Rgba,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Rgb, DynamicImage::ImageRgb32F(src)) => transform_f32(
            source,
            target,
            Layout::Rgb,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        (DataColorSpace::Rgb, DynamicImage::ImageRgba32F(src)) => transform_f32(
            source,
            target,
            Layout::Rgba,
            src.as_raw(),
            image.width(),
            image.height(),
        ),
        // 罕见的「profile 空间与解码缓冲布局不符」不能猜通道，按稳定解码失败拒绝。
        _ => Err(cms_error()),
    }
}

/// 缩略图链路薄封装:把已解码的 RGBA8 缓冲投影到 sRGB,**绝不失败**——缩略图可用性优先于色准
/// (D-410)。与 `to_srgb_rgba8`(编辑预览/保存链,失败会返回 `Err`)不同,这里任何异常路径
/// (ICC 缺失、解析失败、非 RGB 色彩空间)一律原样放行原始像素,只记一行 warn。
///
/// 现有 `to_srgb_rgba8` 对 `(Gray profile, Rgba 缓冲)` 组合会落 `_ => Err`——因为它按 `DynamicImage`
/// 变体分派,Gray profile 只匹配 `ImageLuma8`/`ImageLumaA8` 等灰度缓冲变体。而本函数的调用方(缩略图
/// 解码引擎)恒输出 RGBA 缓冲,不带 `DynamicImage` 变体信息,必须在此前置显式判 `color_space`,
/// 不能像 `to_srgb_rgba8` 那样靠类型匹配兜底,否则 Gray/CMYK profile 会被误判为"不支持"而报错。
pub fn project_rgba8_to_srgb(image: RgbaImage, icc: Option<&[u8]>) -> RgbaImage {
    let Some(icc) = icc else {
        return image;
    };
    let source = match ColorProfile::new_from_slice(icc) {
        Ok(profile) => profile,
        Err(_) => {
            tracing::warn!(
                target: "scrollery::pipeline::thumb",
                "缩略图 ICC profile 解析失败,按 sRGB 假定原样使用"
            );
            return image;
        }
    };
    if source.color_space != DataColorSpace::Rgb {
        tracing::warn!(
            target: "scrollery::pipeline::thumb",
            "缩略图源 ICC 色彩空间非 RGB(Gray/CMYK 等),跳过投影原样使用"
        );
        return image;
    }
    let target = ColorProfile::new_srgb();
    let (width, height) = (image.width(), image.height());
    match transform_u8(
        &source,
        &target,
        Layout::Rgba,
        image.as_raw(),
        width,
        height,
    ) {
        Ok(projected) => projected,
        Err(_) => {
            // 防御性分支,无可构造触发用例
            tracing::warn!(
                target: "scrollery::pipeline::thumb",
                "缩略图 ICC 转换失败,按 sRGB 假定原样使用"
            );
            image
        }
    }
}

fn transform_u8(
    source: &ColorProfile,
    target: &ColorProfile,
    layout: Layout,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<RgbaImage, AppError> {
    let transform = source
        .create_transform_8bit(layout, target, Layout::Rgba, options())
        .map_err(|_| cms_error())?;
    let mut output = vec![0u8; pixel_count(width, height)? * 4];
    transform
        .transform(pixels, &mut output)
        .map_err(|_| cms_error())?;
    RgbaImage::from_raw(width, height, output).ok_or_else(cms_error)
}

fn transform_u16(
    source: &ColorProfile,
    target: &ColorProfile,
    layout: Layout,
    pixels: &[u16],
    width: u32,
    height: u32,
) -> Result<RgbaImage, AppError> {
    let transform = source
        .create_transform_16bit(layout, target, Layout::Rgba, options())
        .map_err(|_| cms_error())?;
    let mut output = vec![0u16; pixel_count(width, height)? * 4];
    transform
        .transform(pixels, &mut output)
        .map_err(|_| cms_error())?;
    let output = output
        .into_iter()
        .map(|value| ((u32::from(value) + 128) / 257) as u8)
        .collect();
    RgbaImage::from_raw(width, height, output).ok_or_else(cms_error)
}

fn transform_f32(
    source: &ColorProfile,
    target: &ColorProfile,
    layout: Layout,
    pixels: &[f32],
    width: u32,
    height: u32,
) -> Result<RgbaImage, AppError> {
    let transform = source
        .create_transform_f32(layout, target, Layout::Rgba, options())
        .map_err(|_| cms_error())?;
    let mut output = vec![0.0f32; pixel_count(width, height)? * 4];
    transform
        .transform(pixels, &mut output)
        .map_err(|_| cms_error())?;
    let output = output
        .into_iter()
        .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    RgbaImage::from_raw(width, height, output).ok_or_else(cms_error)
}

fn pixel_count(width: u32, height: u32) -> Result<usize, AppError> {
    usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| cms_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, RgbImage};
    use lcms2::{CIExyY, CIExyYTRIPLE, Flags, Intent, PixelFormat, Profile, ToneCurve, Transform};

    fn point(x: f64, y: f64) -> CIExyY {
        CIExyY { x, y, Y: 1.0 }
    }

    fn rgb_profile(primaries: CIExyYTRIPLE, curve: &ToneCurve) -> Profile {
        Profile::new_rgb(&point(0.3127, 0.3290), &primaries, &[curve, curve, curve])
            .expect("build RGB profile")
    }

    fn compare_profile_with_lcms(profile: &Profile, tolerance: u8) {
        let icc = profile.icc().expect("serialize profile");
        let source = DynamicImage::ImageRgb8(
            ImageBuffer::from_raw(
                6,
                1,
                vec![
                    0, 0, 0, 16, 64, 220, 32, 128, 240, 128, 64, 255, 240, 180, 20, 255, 255, 255,
                ],
            )
            .expect("source buffer"),
        );
        let mox = to_srgb_rgba8(&source, Some(&icc)).expect("moxcms transform");
        let destination = Profile::new_srgb();
        let transform = Transform::<[u8; 3], [u8; 3]>::new_flags(
            profile,
            PixelFormat::RGB_8,
            &destination,
            PixelFormat::RGB_8,
            Intent::RelativeColorimetric,
            Flags::BLACKPOINT_COMPENSATION,
        )
        .expect("lcms transform");
        let input = source.to_rgb8();
        let input: Vec<[u8; 3]> = input.pixels().map(|pixel| pixel.0).collect();
        let mut reference = vec![[0u8; 3]; input.len()];
        transform.transform_pixels(&input, &mut reference);
        for (actual, expected) in mox.pixels().zip(reference) {
            for channel in 0..3 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= tolerance,
                    "channel {channel}: mox={} lcms={} tolerance={tolerance}",
                    actual[channel],
                    expected[channel]
                );
            }
        }
    }

    #[test]
    fn no_profile_is_assumed_srgb_without_pixel_changes() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 1, Rgb([12, 34, 56])));
        let output = to_srgb_rgba8(&image, None).expect("convert no-profile image");
        assert_eq!(output.as_raw(), &[12, 34, 56, 255, 12, 34, 56, 255]);
    }

    #[test]
    fn malformed_profile_is_stable_decode_error_not_panic() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(1, 1));
        let error = to_srgb_rgba8(&image, Some(b"broken-profile")).expect_err("must reject");
        assert!(matches!(
            error,
            AppError::Edit {
                code: CODE_DECODE_FAILED,
                ..
            }
        ));
    }

    #[test]
    fn srgb_profile_matches_lcms2_reference_within_one_code_value() {
        let profile = Profile::new_srgb();
        let icc = profile.icc().expect("serialize sRGB profile");
        let source = DynamicImage::ImageRgb8(
            ImageBuffer::from_raw(
                4,
                1,
                vec![0, 0, 0, 32, 128, 240, 128, 64, 255, 255, 255, 255],
            )
            .expect("source buffer"),
        );
        let mox = to_srgb_rgba8(&source, Some(&icc)).expect("moxcms transform");

        let transform = Transform::<[u8; 3], [u8; 3]>::new(
            &profile,
            PixelFormat::RGB_8,
            &Profile::new_srgb(),
            PixelFormat::RGB_8,
            Intent::RelativeColorimetric,
        )
        .expect("lcms transform");
        let input = source.to_rgb8();
        let input: Vec<[u8; 3]> = input.pixels().map(|p| p.0).collect();
        let mut reference = vec![[0u8; 3]; input.len()];
        transform.transform_pixels(&input, &mut reference);

        for (actual, expected) in mox.pixels().zip(reference) {
            for channel in 0..3 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 1,
                    "channel {channel}: mox={} lcms={}",
                    actual[channel],
                    expected[channel]
                );
            }
            assert_eq!(actual[3], 255);
        }
    }

    #[test]
    fn adobe_rgb_and_display_p3_matrix_profiles_match_lcms_reference() {
        let adobe_curve = ToneCurve::new(2.199_218_75);
        let adobe = rgb_profile(
            CIExyYTRIPLE {
                Red: point(0.64, 0.33),
                Green: point(0.21, 0.71),
                Blue: point(0.15, 0.06),
            },
            &adobe_curve,
        );
        compare_profile_with_lcms(&adobe, 2);

        let srgb_table: Vec<u16> = (0..=4096)
            .map(|index| {
                let encoded = f64::from(index) / 4096.0;
                let linear = if encoded <= 0.04045 {
                    encoded / 12.92
                } else {
                    ((encoded + 0.055) / 1.055).powf(2.4)
                };
                (linear * 65535.0).round() as u16
            })
            .collect();
        let p3_curve = ToneCurve::new_tabulated(&srgb_table);
        let display_p3 = rgb_profile(
            CIExyYTRIPLE {
                Red: point(0.68, 0.32),
                Green: point(0.265, 0.69),
                Blue: point(0.15, 0.06),
            },
            &p3_curve,
        );
        compare_profile_with_lcms(&display_p3, 2);
    }

    #[test]
    fn gray_16_profile_parses_and_preserves_alpha() {
        let curve = ToneCurve::new(2.2);
        let profile = Profile::new_gray(&point(0.3127, 0.3290), &curve).expect("gray profile");
        let icc = profile.icc().expect("serialize gray profile");
        let source = DynamicImage::ImageLumaA16(
            ImageBuffer::from_raw(2, 1, vec![0u16, 12345, 65535, 54321])
                .expect("gray alpha source"),
        );
        let output = to_srgb_rgba8(&source, Some(&icc)).expect("gray transform");
        assert_eq!(output.get_pixel(0, 0)[3], ((12345u32 + 128) / 257) as u8);
        assert_eq!(output.get_pixel(1, 0)[3], ((54321u32 + 128) / 257) as u8);
        assert!(output.get_pixel(1, 0)[0] > output.get_pixel(0, 0)[0]);
    }

    // ── project_rgba8_to_srgb(缩略图链路薄封装)专项 ──────────────────────────────

    #[test]
    fn project_none_icc_returns_original_pixels() {
        let image = RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]));
        let output = project_rgba8_to_srgb(image.clone(), None);
        assert_eq!(output.as_raw(), image.as_raw());
    }

    #[test]
    fn project_malformed_icc_returns_original_pixels_not_panic() {
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        let output = project_rgba8_to_srgb(image.clone(), Some(b"broken-profile"));
        assert_eq!(output.as_raw(), image.as_raw());
    }

    #[test]
    fn project_gray_profile_with_rgba_buffer_returns_original_not_err() {
        // to_srgb_rgba8 对 (Gray profile, Rgba buffer) 组合会落 `_ => Err`(D-410);
        // project_rgba8_to_srgb 必须前置判 color_space,原样放行而非失败。
        let curve = ToneCurve::new(2.2);
        let profile = Profile::new_gray(&point(0.3127, 0.3290), &curve).expect("gray profile");
        let icc = profile.icc().expect("serialize gray profile");
        let image = RgbaImage::from_pixel(2, 1, image::Rgba([100, 150, 200, 255]));
        let output = project_rgba8_to_srgb(image.clone(), Some(&icc));
        assert_eq!(output.as_raw(), image.as_raw());
    }

    #[test]
    fn project_adobe_rgb_matches_to_srgb_rgba8_within_tolerance() {
        let adobe_curve = ToneCurve::new(2.199_218_75);
        let adobe = rgb_profile(
            CIExyYTRIPLE {
                Red: point(0.64, 0.33),
                Green: point(0.21, 0.71),
                Blue: point(0.15, 0.06),
            },
            &adobe_curve,
        );
        let icc = adobe.icc().expect("serialize adobe rgb profile");

        let rgba = RgbaImage::from_raw(
            6,
            1,
            vec![
                0, 0, 0, 255, 16, 64, 220, 255, 32, 128, 240, 255, 128, 64, 255, 255, 240, 180, 20,
                255, 255, 255, 255, 255,
            ],
        )
        .expect("rgba source buffer");
        let projected = project_rgba8_to_srgb(rgba.clone(), Some(&icc));

        // 与既有 to_srgb_rgba8(DynamicImage::ImageRgba8 分支)结果比对,容差 ≤1。
        let dyn_image = DynamicImage::ImageRgba8(rgba.clone());
        let reference = to_srgb_rgba8(&dyn_image, Some(&icc)).expect("to_srgb_rgba8 baseline");
        for (actual, expected) in projected.pixels().zip(reference.pixels()) {
            for channel in 0..4 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 1,
                    "channel {channel}: project={} to_srgb={}",
                    actual[channel],
                    expected[channel]
                );
            }
        }
        // 投影确实发生:至少一像素的某通道与源 rgba 差 >1
        let has_projection = projected
            .pixels()
            .zip(rgba.pixels())
            .any(|(proj, orig)| (0..3).any(|ch| proj[ch].abs_diff(orig[ch]) > 1));
        assert!(has_projection, "投影应改变至少一像素的某通道>1");
    }

    // ── to_target_rgba8 源假定重构(B 线,2026-07-23)专项 ──────────────────────────

    /// 手工构造最小合法 CMYK ICC(仅 132 字节头,tag_count=0,无任何 tag)。
    /// lcms2 的 `Profile` 构造器族没有暴露通用 CMYK 工厂,而 moxcms 的 header 解析
    /// (`ColorProfile::new_from_slice`)只依赖头部字段——tags_count=0 时完全不读 tag 区,
    /// 故一段按 ICC 头部字节布局手填的最小 buffer 即可让 `color_space` 解析为
    /// `DataColorSpace::Cmyk`,足够驱动本文件 CMYK 分支的判定逻辑(该分支不使用
    /// source profile 的 LUT,只据 color_space 分派)。
    fn minimal_cmyk_icc() -> Vec<u8> {
        let mut buf = vec![0u8; 132];
        buf[8..12].copy_from_slice(&0x0430_0000u32.to_be_bytes()); // version 4.3
        buf[12..16].copy_from_slice(b"prtr"); // profile_class = OutputDevice
        buf[16..20].copy_from_slice(b"CMYK"); // data_color_space
        buf[20..24].copy_from_slice(b"Lab "); // pcs(任意合法值,未被本用例依赖)
        buf[36..40].copy_from_slice(b"acsp"); // signature
                                              // rendering_intent(64..68)/illuminant(68..80)/tag_count(128..132) 保持全零即合法。
        buf
    }

    #[test]
    fn minimal_cmyk_icc_parses_with_cmyk_color_space() {
        let icc = minimal_cmyk_icc();
        let profile = ColorProfile::new_from_slice(&icc).expect("minimal CMYK ICC must parse");
        assert_eq!(profile.color_space, DataColorSpace::Cmyk);
    }

    #[test]
    fn luma8_no_icc_projects_to_non_srgb_target_and_is_not_identity() {
        // 修复前:None 分支以 `transform_dynamic_image` 按 (source.color_space, image 变体)
        // 分派,Luma8 缓冲配 Rgb 假定源永远匹配不到分支,落 `_ => Err`。这里用一组跨值域的
        // 灰阶样本断言:1) 非 sRGB target 下不再报错;2) 输出并非把灰度值原样复制到三通道
        // ——Adobe RGB 的 TRC(纯 2.19921875 次幂)与 sRGB 的分段参数曲线不同,即便灰色在
        // 色度上恒等,编码值也会随之改变,足以证明确实发生了投影而非误判恒等短路。
        let raw: Vec<u8> = vec![0, 16, 48, 96, 128, 160, 200, 224, 255];
        let width = raw.len() as u32;
        let image = DynamicImage::ImageLuma8(
            ImageBuffer::from_raw(width, 1, raw.clone()).expect("luma8 buffer"),
        );
        let target = ColorProfile::new_adobe_rgb();
        let output =
            to_target_rgba8(&image, None, &target).expect("luma8 无 ICC 应能投影到非 sRGB target");
        assert_eq!(output.width(), width);
        let changed = (0..width as usize).any(|index| {
            let pixel = output.get_pixel(index as u32, 0);
            pixel[0] != raw[index] || pixel[1] != raw[index] || pixel[2] != raw[index]
        });
        assert!(
            changed,
            "Adobe RGB target 投影应至少改变一个灰阶样本的编码值"
        );
    }

    #[test]
    fn cmyk_profile_with_rgb_buffer_is_projected_to_non_srgb_target() {
        // D-412 破口:此前 CMYK 分支对 to_target_rgba8 直接 `Ok(image.to_rgba8())` 返回未投影
        // 像素,却由调用方把它当作已按 target ICC 编码的结果使用。这里断言修复后确实发生了
        // 投影(与不经变换的原始像素比较,至少一像素某通道差值 >1),而不仅仅是"不再是 Err"。
        let icc = minimal_cmyk_icc();
        let rgb = RgbImage::from_raw(3, 1, vec![10, 20, 30, 100, 150, 200, 240, 245, 250])
            .expect("rgb buffer");
        let image = DynamicImage::ImageRgb8(rgb.clone());
        let target = ColorProfile::new_display_p3();
        let output = to_target_rgba8(&image, Some(&icc), &target)
            .expect("CMYK profile + Rgb8 buffer 应能投影到 target,不落 Err");
        assert_eq!(output.width(), 3);
        let has_projection = output
            .pixels()
            .zip(rgb.pixels())
            .any(|(proj, orig)| (0..3).any(|channel| proj[channel].abs_diff(orig[channel]) > 1));
        assert!(
            has_projection,
            "CMYK 分支应把像素投影到 Display P3 target,而非原样返回"
        );
    }
}
