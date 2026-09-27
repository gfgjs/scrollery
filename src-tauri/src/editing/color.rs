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
