//! E3 调色(设计 §4.3/§4.4;D-104/D-106/D-107/D-108):亮度/对比度/饱和度三参数。
//!
//! 公式钉死,与前端 `src/composables/adjustFormula.ts` 双端同源,共享
//! `src/fixtures/adjustGolden.json` 黄金向量。**有意**作用在 sRGB gamma 编码域而非线性光,
//! 与 CSS/主流简易编辑器语义对齐——不是疏漏,防未来误「修」:
//! - 亮度 b∈[-100,100]:`k_b = 2^(b/100)`,`v' = v·k_b`(乘法;±100 恰为 ±1EV);
//! - 对比度 c∈[-100,100]:`k_c = (100+c)/100`,`v' = (v−0.5)·k_c + 0.5`(线性系数;
//!   **不采用** image crate `imageops::contrast` 的平方映射,两者数学不等价,设计 §1.3 实证);
//! - 饱和度 s∈[-100,100]:`k_s = (100+s)/100`,W3C Filter Effects saturate 矩阵,
//!   luma 权重 [0.213, 0.715, 0.072](仅在 sRGB 原色下成立,D-108 保证进公式前已在 sRGB);
//! - 应用序:亮度 → 对比度 → 饱和度(钉死);每步 clamp [0,1];alpha 通道不动。
//!
//! 管线(设计 §4.4「有 adjust」):源 ICC 经 moxcms(relative colorimetric,与 E0 预览同
//! intent)转 sRGB → f32 公式 → 按**源位深**量化回写(8→8、16→16、f32→f32;带 ICC 的灰度
//! 经 CMS 后升 RGB 是色彩转换的固有结果)。无 ICC 按 sRGB 直入且完全保持原变体。CMS 走
//! 分条(strip)转换:瞬态 f32 缓冲 O(条),整体峰值≈输入+输出两个像素缓冲,与 rotate90
//! 已实测计量的双缓冲峰形同级,不需要独立 memory_budget 门。
//! 全零参数在命令层被 [`effective_adjust`] 过滤为 `None`,完全不进入本模块(D-106:
//! 位深与 ICC 直通行为与 v1 字节级不变)。

use image::DynamicImage;
use moxcms::{ColorProfile, DataColorSpace, Layout};
use serde::Deserialize;

use crate::error::AppError;

use super::color;
use super::geometry::CODE_INVALID_OPS;

/// 三参数共用闭区间端点(前端 `adjustFormula.ts` 同值)。
pub const ADJUST_MIN: i8 = -100;
pub const ADJUST_MAX: i8 = 100;

/// CMS 分条行数:限制瞬态 f32 缓冲量级(10000px 宽 RGBA 一条约 10 MB×2),与全图面积无关。
const STRIP_ROWS: usize = 64;

/// `EditOps.adjust` 载荷(设计 §4.3 IPC 契约)。序列化域为 i8,超出 [-100,100] 拒
/// [`CODE_INVALID_OPS`];全零由命令层视同未提供(D-106)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustOps {
    pub brightness: i8,
    pub contrast: i8,
    pub saturation: i8,
}

impl AdjustOps {
    pub fn is_neutral(&self) -> bool {
        self.brightness == 0 && self.contrast == 0 && self.saturation == 0
    }
}

/// 域校验:任一参数越界拒稳定码 `edit_invalid_ops`(与 rotate/拉直同码族)。
pub fn validate_adjust(ops: &AdjustOps) -> Result<(), AppError> {
    let in_domain = |v: i8| (ADJUST_MIN..=ADJUST_MAX).contains(&v);
    if in_domain(ops.brightness) && in_domain(ops.contrast) && in_domain(ops.saturation) {
        Ok(())
    } else {
        Err(AppError::Edit {
            code: CODE_INVALID_OPS,
            message: "调色参数超出 [-100,100] | adjust parameter out of range".into(),
        })
    }
}

/// D-106:全零载荷视同未提供——调用方据此完全跳过调色管线,保住 v1 字节级直通契约。
pub fn effective_adjust(adjust: Option<AdjustOps>) -> Option<AdjustOps> {
    adjust.filter(|ops| !ops.is_neutral())
}

/// 由三参数预计算的逐像素系数(饱和度矩阵每参数组只算一次,不进像素循环)。
#[derive(Debug, Clone, Copy)]
pub struct AdjustCoefficients {
    brightness: f32,
    contrast: f32,
    saturate: [[f32; 3]; 3],
}

pub fn coefficients(ops: &AdjustOps) -> AdjustCoefficients {
    let k_b = 2.0f32.powf(f32::from(ops.brightness) / 100.0);
    let k_c = (100.0 + f32::from(ops.contrast)) / 100.0;
    let k_s = (100.0 + f32::from(ops.saturation)) / 100.0;
    AdjustCoefficients {
        brightness: k_b,
        contrast: k_c,
        saturate: [
            [
                0.213 + 0.787 * k_s,
                0.715 - 0.715 * k_s,
                0.072 - 0.072 * k_s,
            ],
            [
                0.213 - 0.213 * k_s,
                0.715 + 0.285 * k_s,
                0.072 - 0.072 * k_s,
            ],
            [
                0.213 - 0.213 * k_s,
                0.715 - 0.715 * k_s,
                0.072 + 0.928 * k_s,
            ],
        ],
    }
}

/// 单像素同源公式(模块文档里的钉死链序);输入输出均为 sRGB 编码域 [0,1]。
#[inline]
pub fn adjust_rgb(rgb: [f32; 3], k: &AdjustCoefficients) -> [f32; 3] {
    let [r, g, b] = rgb.map(|v| adjust_luma(v, k));
    [0, 1, 2].map(|row| {
        let m = k.saturate[row];
        (m[0] * r + m[1] * g + m[2] * b).clamp(0.0, 1.0)
    })
}

/// 亮度+对比度单通道链。灰度像素(r=g=b)可直接用它:saturate 矩阵每行系数和恒为
/// `0.213+0.715+0.072 + k_s·0 = 1`,对灰像素是严格恒等,不必展开成 RGB 走矩阵。
#[inline]
fn adjust_luma(v: f32, k: &AdjustCoefficients) -> f32 {
    let bright = (v * k.brightness).clamp(0.0, 1.0);
    ((bright - 0.5) * k.contrast + 0.5).clamp(0.0, 1.0)
}

// 量化契约(前端 adjustFormula.ts 同款,黄金向量 expected8/expected16 依此断言):
// round(clamp(v)·(2^n−1)),round half away from zero(正值域内与 JS Math.round 一致)。
#[inline]
fn to_f32_u8(v: u8) -> f32 {
    f32::from(v) / 255.0
}
#[inline]
fn from_f32_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}
#[inline]
fn to_f32_u16(v: u16) -> f32 {
    f32::from(v) / 65535.0
}
#[inline]
fn from_f32_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// §4.4 输出策略:调色后像素已在 sRGB,输出嵌显式 sRGB profile(不回写源 ICC——像素已经
/// 转换,回写源 profile 会让阅读器按错误空间解释)。
pub fn srgb_profile_bytes() -> Result<Vec<u8>, AppError> {
    ColorProfile::new_srgb()
        .encode()
        .map_err(|_| AppError::Edit {
            code: super::io::CODE_ENCODE_FAILED,
            message: "sRGB profile 序列化失败 | failed to serialize sRGB profile".into(),
        })
}

/// 调色主入口(D-107 链序最末步,crop 之后):`icc_profile` 为源图原始 ICC。
/// 返回的图像位深与源一致;调用方负责把输出 ICC 换成 [`srgb_profile_bytes`]。
pub fn apply_adjust(
    img: DynamicImage,
    icc_profile: Option<&[u8]>,
    ops: &AdjustOps,
) -> Result<DynamicImage, AppError> {
    let k = coefficients(ops);
    let Some(icc) = icc_profile else {
        // untagged 按业界惯例视为 sRGB 直入(与 E0 预览、浏览器行为一致),保持原变体。
        return Ok(adjust_untagged(img, &k));
    };
    let profile = ColorProfile::new_from_slice(icc).map_err(|_| color::cms_error())?;
    // CMYK JPEG:image 解码器已转 RGB 且 ICC 未参与(与 color.rs 同款边界),按 sRGB 直入。
    if profile.color_space == DataColorSpace::Cmyk {
        return Ok(adjust_untagged(img, &k));
    }

    match (profile.color_space, img) {
        (DataColorSpace::Gray, DynamicImage::ImageLuma8(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                1,
                Layout::Gray,
                3,
                Layout::Rgb,
                to_f32_u8,
                from_f32_u8,
                &profile,
                &k,
            )?;
            image::RgbImage::from_raw(w, h, out)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Gray, DynamicImage::ImageLumaA8(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                2,
                Layout::GrayAlpha,
                4,
                Layout::Rgba,
                to_f32_u8,
                from_f32_u8,
                &profile,
                &k,
            )?;
            image::RgbaImage::from_raw(w, h, out)
                .map(DynamicImage::ImageRgba8)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Gray, DynamicImage::ImageLuma16(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                1,
                Layout::Gray,
                3,
                Layout::Rgb,
                to_f32_u16,
                from_f32_u16,
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgb16)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Gray, DynamicImage::ImageLumaA16(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                2,
                Layout::GrayAlpha,
                4,
                Layout::Rgba,
                to_f32_u16,
                from_f32_u16,
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgba16)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Rgb, DynamicImage::ImageRgb8(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                3,
                Layout::Rgb,
                3,
                Layout::Rgb,
                to_f32_u8,
                from_f32_u8,
                &profile,
                &k,
            )?;
            image::RgbImage::from_raw(w, h, out)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Rgb, DynamicImage::ImageRgba8(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                4,
                Layout::Rgba,
                4,
                Layout::Rgba,
                to_f32_u8,
                from_f32_u8,
                &profile,
                &k,
            )?;
            image::RgbaImage::from_raw(w, h, out)
                .map(DynamicImage::ImageRgba8)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Rgb, DynamicImage::ImageRgb16(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                3,
                Layout::Rgb,
                3,
                Layout::Rgb,
                to_f32_u16,
                from_f32_u16,
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgb16)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Rgb, DynamicImage::ImageRgba16(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                4,
                Layout::Rgba,
                4,
                Layout::Rgba,
                to_f32_u16,
                from_f32_u16,
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgba16)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Rgb, DynamicImage::ImageRgb32F(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                3,
                Layout::Rgb,
                3,
                Layout::Rgb,
                |v: f32| v,
                |v: f32| v.clamp(0.0, 1.0),
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgb32F)
                .ok_or_else(color::cms_error)
        }
        (DataColorSpace::Rgb, DynamicImage::ImageRgba32F(buffer)) => {
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                4,
                Layout::Rgba,
                4,
                Layout::Rgba,
                |v: f32| v,
                |v: f32| v.clamp(0.0, 1.0),
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgba32F)
                .ok_or_else(color::cms_error)
        }
        // DynamicImage 是 non_exhaustive:未来未知变体先升 RGBA16 再走 RGB profile 路径,
        // 不静默退化 8-bit(与 geometry::apply_fine_rotation 同款兜底)。
        (DataColorSpace::Rgb, other) => {
            let buffer = other.into_rgba16();
            let (w, h) = buffer.dimensions();
            let out = cms_adjust_strips(
                buffer.as_raw(),
                w,
                4,
                Layout::Rgba,
                4,
                Layout::Rgba,
                to_f32_u16,
                from_f32_u16,
                &profile,
                &k,
            )?;
            image::ImageBuffer::from_raw(w, h, out)
                .map(DynamicImage::ImageRgba16)
                .ok_or_else(color::cms_error)
        }
        // profile 空间与解码缓冲布局不符:不能猜通道,按稳定解码失败拒绝(color.rs 同款)。
        _ => Err(color::cms_error()),
    }
}

/// 无 ICC(或 CMYK 边界)路径:就地改写像素,保持变体与位深,零额外整图分配。
fn adjust_untagged(img: DynamicImage, k: &AdjustCoefficients) -> DynamicImage {
    match img {
        DynamicImage::ImageLuma8(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                pixel.0[0] = from_f32_u8(adjust_luma(to_f32_u8(pixel.0[0]), k));
            }
            DynamicImage::ImageLuma8(buffer)
        }
        DynamicImage::ImageLumaA8(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                pixel.0[0] = from_f32_u8(adjust_luma(to_f32_u8(pixel.0[0]), k));
            }
            DynamicImage::ImageLumaA8(buffer)
        }
        DynamicImage::ImageRgb8(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                let rgb = adjust_rgb(pixel.0.map(to_f32_u8), k);
                pixel.0 = rgb.map(from_f32_u8);
            }
            DynamicImage::ImageRgb8(buffer)
        }
        DynamicImage::ImageRgba8(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                let [r, g, b] = adjust_rgb([pixel.0[0], pixel.0[1], pixel.0[2]].map(to_f32_u8), k);
                pixel.0[0] = from_f32_u8(r);
                pixel.0[1] = from_f32_u8(g);
                pixel.0[2] = from_f32_u8(b);
            }
            DynamicImage::ImageRgba8(buffer)
        }
        DynamicImage::ImageLuma16(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                pixel.0[0] = from_f32_u16(adjust_luma(to_f32_u16(pixel.0[0]), k));
            }
            DynamicImage::ImageLuma16(buffer)
        }
        DynamicImage::ImageLumaA16(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                pixel.0[0] = from_f32_u16(adjust_luma(to_f32_u16(pixel.0[0]), k));
            }
            DynamicImage::ImageLumaA16(buffer)
        }
        DynamicImage::ImageRgb16(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                let rgb = adjust_rgb(pixel.0.map(to_f32_u16), k);
                pixel.0 = rgb.map(from_f32_u16);
            }
            DynamicImage::ImageRgb16(buffer)
        }
        DynamicImage::ImageRgba16(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                let [r, g, b] = adjust_rgb([pixel.0[0], pixel.0[1], pixel.0[2]].map(to_f32_u16), k);
                pixel.0[0] = from_f32_u16(r);
                pixel.0[1] = from_f32_u16(g);
                pixel.0[2] = from_f32_u16(b);
            }
            DynamicImage::ImageRgba16(buffer)
        }
        DynamicImage::ImageRgb32F(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                pixel.0 = adjust_rgb(pixel.0, k);
            }
            DynamicImage::ImageRgb32F(buffer)
        }
        DynamicImage::ImageRgba32F(mut buffer) => {
            for pixel in buffer.pixels_mut() {
                let [r, g, b] = adjust_rgb([pixel.0[0], pixel.0[1], pixel.0[2]], k);
                pixel.0[0] = r;
                pixel.0[1] = g;
                pixel.0[2] = b;
            }
            DynamicImage::ImageRgba32F(buffer)
        }
        // non_exhaustive 兜底:与 geometry 同款,升 RGBA16 防静默 8-bit 退化。
        other => {
            let mut buffer = other.into_rgba16();
            for pixel in buffer.pixels_mut() {
                let [r, g, b] = adjust_rgb([pixel.0[0], pixel.0[1], pixel.0[2]].map(to_f32_u16), k);
                pixel.0[0] = from_f32_u16(r);
                pixel.0[1] = from_f32_u16(g);
                pixel.0[2] = from_f32_u16(b);
            }
            DynamicImage::ImageRgba16(buffer)
        }
    }
}

/// 分条 CMS→公式→量化管线:`raw` 为源通道数据,按 [`STRIP_ROWS`] 行一条做
/// f32 归一化 → moxcms transform(源空间→sRGB)→ 同源公式 → 目标位深量化。
/// alpha 通道由 moxcms 透传(GrayAlpha/Rgba layout),公式不触碰,仅 clamp 量化。
#[allow(clippy::too_many_arguments)]
fn cms_adjust_strips<S: Copy, O>(
    raw: &[S],
    width: u32,
    src_channels: usize,
    src_layout: Layout,
    dst_channels: usize,
    dst_layout: Layout,
    to_f32: impl Fn(S) -> f32,
    from_f32: impl Fn(f32) -> O,
    profile: &ColorProfile,
    k: &AdjustCoefficients,
) -> Result<Vec<O>, AppError> {
    let srgb = ColorProfile::new_srgb();
    let transform = profile
        .create_transform_f32(src_layout, &srgb, dst_layout, color::options())
        .map_err(|_| color::cms_error())?;

    let strip_pixels = (width.max(1) as usize) * STRIP_ROWS;
    let total_pixels = raw.len() / src_channels;
    let mut out: Vec<O> = Vec::with_capacity(total_pixels * dst_channels);
    let mut src_f32 = vec![0.0f32; strip_pixels * src_channels];
    let mut dst_f32 = vec![0.0f32; strip_pixels * dst_channels];

    for chunk in raw.chunks(strip_pixels * src_channels) {
        let pixels = chunk.len() / src_channels;
        let src = &mut src_f32[..pixels * src_channels];
        for (dst, &value) in src.iter_mut().zip(chunk) {
            *dst = to_f32(value);
        }
        let dst = &mut dst_f32[..pixels * dst_channels];
        transform
            .transform(src, dst)
            .map_err(|_| color::cms_error())?;
        for pixel in dst.chunks_exact_mut(dst_channels) {
            let [r, g, b] = adjust_rgb([pixel[0], pixel[1], pixel[2]], k);
            pixel[0] = r;
            pixel[1] = g;
            pixel[2] = b;
        }
        out.extend(dst.iter().map(|&value| from_f32(value)));
    }
    Ok(out)
}
