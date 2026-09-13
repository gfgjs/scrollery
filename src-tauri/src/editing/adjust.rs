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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, RgbImage, Rgba, RgbaImage};

    #[derive(Deserialize)]
    struct GoldenVector {
        brightness: i8,
        contrast: i8,
        saturation: i8,
        input: [f32; 3],
        expected: [f32; 3],
        expected8: [u8; 3],
        expected16: [u16; 3],
    }

    fn golden_vectors() -> Vec<GoldenVector> {
        serde_json::from_str(include_str!("../../../src/fixtures/adjustGolden.json")).unwrap()
    }

    fn ops(brightness: i8, contrast: i8, saturation: i8) -> AdjustOps {
        AdjustOps {
            brightness,
            contrast,
            saturation,
        }
    }

    #[test]
    fn shared_golden_vectors_match_f32_formula_and_quantization() {
        for vector in golden_vectors() {
            let k = coefficients(&ops(vector.brightness, vector.contrast, vector.saturation));
            let actual = adjust_rgb(vector.input, &k);
            for (channel, &value) in actual.iter().enumerate() {
                assert!(
                    (value - vector.expected[channel]).abs() <= 1e-5,
                    "f32 channel {channel}: actual={value} expected={}",
                    vector.expected[channel]
                );
                assert_eq!(from_f32_u8(value), vector.expected8[channel]);
                assert_eq!(from_f32_u16(value), vector.expected16[channel]);
            }
        }
    }

    #[test]
    fn adjust_domain_is_closed_and_rejects_out_of_range() {
        for valid in [ADJUST_MIN, 0, ADJUST_MAX] {
            assert!(validate_adjust(&ops(valid, valid, valid)).is_ok());
        }
        for invalid in [
            ops(101, 0, 0),
            ops(0, -101, 0),
            ops(0, 0, 127),
            ops(-128, 0, 0),
        ] {
            let err = validate_adjust(&invalid).unwrap_err();
            match err {
                AppError::Edit { code, .. } => assert_eq!(code, CODE_INVALID_OPS),
                other => panic!("expected AppError::Edit, got {other:?}"),
            }
        }
    }

    #[test]
    fn neutral_adjust_is_filtered_to_none() {
        assert_eq!(effective_adjust(None), None);
        assert_eq!(effective_adjust(Some(ops(0, 0, 0))), None);
        assert_eq!(effective_adjust(Some(ops(0, 0, 1))), Some(ops(0, 0, 1)));
    }

    #[test]
    fn untagged_rgb8_matches_shared_golden_endpoint() {
        // 黄金向量 saturation=100 组:输入 [0.6,0.4,0.2] 恰为 8-bit 码值 153/102/51。
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 1, Rgb([153, 102, 51])));
        let out = apply_adjust(source, None, &ops(0, 0, 100)).unwrap();
        let out = match out {
            DynamicImage::ImageRgb8(buffer) => buffer,
            other => panic!("variant changed: {other:?}"),
        };
        assert_eq!(out.get_pixel(0, 0).0, [197, 95, 0]);
    }

    #[test]
    fn untagged_rgba8_leaves_alpha_untouched() {
        let source =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([153, 102, 51, 77])));
        let out = apply_adjust(source, None, &ops(0, 0, 100)).unwrap();
        let out = match out {
            DynamicImage::ImageRgba8(buffer) => buffer,
            other => panic!("variant changed: {other:?}"),
        };
        assert_eq!(out.get_pixel(0, 0).0, [197, 95, 0, 77]);
    }

    #[test]
    fn untagged_luma_keeps_gray_variant_and_saturation_is_identity_on_gray() {
        // 亮度 +100(×2):60 → 120;饱和度对灰像素恒等(矩阵行和为 1),不应升 RGB。
        let source = DynamicImage::ImageLuma8(ImageBuffer::from_pixel(1, 1, image::Luma([60])));
        let out = apply_adjust(source, None, &ops(100, 0, 87)).unwrap();
        match out {
            DynamicImage::ImageLuma8(buffer) => assert_eq!(buffer.get_pixel(0, 0).0, [120]),
            other => panic!("variant changed: {other:?}"),
        }
    }

    #[test]
    fn untagged_rgb16_preserves_depth_and_quantizes_at_16_bit() {
        // 亮度 -100(×0.5):精确半分,26214 → 13107。
        let source =
            DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([26214u16, 32768, 65535])));
        let out = apply_adjust(source, None, &ops(-100, 0, 0)).unwrap();
        match out {
            DynamicImage::ImageRgb16(buffer) => {
                assert_eq!(buffer.get_pixel(0, 0).0, [13107, 16384, 32768]);
            }
            other => panic!("variant changed: {other:?}"),
        }
    }

    #[test]
    fn tagged_srgb_matches_untagged_within_two_code_values() {
        // sRGB→sRGB 的 CMS 接近恒等(moxcms f32 以 14-bit 精度实现):与 untagged 直算
        // 的差应在 2 个 8-bit 码值内,证明 CMS 路径没有引入错误的空间映射。
        let icc = srgb_profile_bytes().unwrap();
        let pixels = [
            Rgb([153u8, 102, 51]),
            Rgb([12, 250, 128]),
            Rgb([0, 255, 33]),
        ];
        for pixel in pixels {
            let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, pixel));
            let expected = match apply_adjust(source.clone(), None, &ops(25, -30, 60)).unwrap() {
                DynamicImage::ImageRgb8(buffer) => buffer.get_pixel(0, 0).0,
                other => panic!("variant changed: {other:?}"),
            };
            let actual = match apply_adjust(source, Some(&icc), &ops(25, -30, 60)).unwrap() {
                DynamicImage::ImageRgb8(buffer) => buffer.get_pixel(0, 0).0,
                other => panic!("variant changed: {other:?}"),
            };
            for channel in 0..3 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 2,
                    "channel {channel}: tagged={} untagged={}",
                    actual[channel],
                    expected[channel]
                );
            }
        }
    }

    #[test]
    fn tagged_rgb16_preserves_bit_depth_through_cms() {
        let icc = srgb_profile_bytes().unwrap();
        let source =
            DynamicImage::ImageRgb16(ImageBuffer::from_pixel(3, 2, Rgb([26214u16, 39321, 52428])));
        let out = apply_adjust(source, Some(&icc), &ops(10, 10, 10)).unwrap();
        assert!(matches!(out, DynamicImage::ImageRgb16(_)));
    }

    #[test]
    fn tagged_gray_alpha_expands_to_rgba_and_preserves_alpha() {
        let curve = lcms2::ToneCurve::new(2.2);
        let profile = lcms2::Profile::new_gray(
            &lcms2::CIExyY {
                x: 0.3127,
                y: 0.3290,
                Y: 1.0,
            },
            &curve,
        )
        .expect("gray profile");
        let icc = profile.icc().expect("serialize gray profile");
        let source = DynamicImage::ImageLumaA16(
            ImageBuffer::from_raw(2, 1, vec![13107u16, 21845, 52428, 43690])
                .expect("gray alpha source"),
        );
        let out = apply_adjust(source, Some(&icc), &ops(20, 0, 0)).unwrap();
        let out = match out {
            DynamicImage::ImageRgba16(buffer) => buffer,
            other => panic!("expected RGBA16, got {other:?}"),
        };
        // alpha 由 moxcms 透传且公式不触碰;f32 往返量化应保持原码值。
        assert_eq!(out.get_pixel(0, 0).0[3], 21845);
        assert_eq!(out.get_pixel(1, 0).0[3], 43690);
        // 亮度 +20% 应确实提亮 luma 通道。
        assert!(out.get_pixel(1, 0).0[0] > out.get_pixel(0, 0).0[0]);
        assert!(out.get_pixel(0, 0).0[0] > 13107);
    }

    #[test]
    fn malformed_profile_is_stable_decode_error() {
        let source = DynamicImage::ImageRgb8(RgbImage::new(1, 1));
        let err = apply_adjust(source, Some(b"broken"), &ops(1, 0, 0)).unwrap_err();
        match err {
            AppError::Edit { code, .. } => assert_eq!(code, "edit_decode_failed"),
            other => panic!("expected AppError::Edit, got {other:?}"),
        }
    }

    #[test]
    fn srgb_profile_bytes_roundtrips_through_moxcms() {
        let bytes = srgb_profile_bytes().unwrap();
        let parsed = ColorProfile::new_from_slice(&bytes).expect("parse serialized sRGB");
        assert_eq!(parsed.color_space, DataColorSpace::Rgb);
    }

    #[test]
    fn strip_pipeline_is_consistent_across_strip_boundaries() {
        // 高度超过 STRIP_ROWS,强制多条:每行同像素,输出必须全图一致(条界无缝)。
        let icc = srgb_profile_bytes().unwrap();
        let height = (STRIP_ROWS * 2 + 7) as u32;
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(3, height, Rgb([153, 102, 51])));
        let out = match apply_adjust(source, Some(&icc), &ops(0, 0, 100)).unwrap() {
            DynamicImage::ImageRgb8(buffer) => buffer,
            other => panic!("variant changed: {other:?}"),
        };
        let first = out.get_pixel(0, 0).0;
        for (_, _, pixel) in out.enumerate_pixels() {
            assert_eq!(pixel.0, first);
        }
    }
}
