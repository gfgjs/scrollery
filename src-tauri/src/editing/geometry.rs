//! 编辑几何处理链(方案 C §3.1/§6):纯函数,不碰 IO/DB,便于用小图逐像素单测穷举。
//!
//! 固定顺序(方案 §3.1):
//! 1. 解码并只应用一次文件 EXIF orientation;
//! 2. 合入原 item 的 V20 `view_rotation` 与本次 `rotate`;
//! 3. flip;
//! 4. fine rotate(展开后取保持原宽高比的最大居中内接矩形);
//! 5. crop(前端最终预览坐标系里的源像素矩形,后端只需 clamp,不需要反推——因为 crop 在
//!    orientation/rotate/flip/fine rotate 之后执行,此时图像已经处于与前端预览完全一致的坐标系)。

use image::DynamicImage;
use imageproc::geometric_transformations::{rotate_about_center_no_crop, Border, Interpolation};
use serde::Deserialize;

use crate::error::AppError;

pub const CODE_CROP_EMPTY: &str = "edit_crop_empty";
/// D-010:非法编辑参数(rotate 不在 0|90|180|270)的稳定码。方案 §6 码集是「至少包括」,
/// 此码为 2026-07-19 复审新增——静默取模归 0 会保存出「没转的图」,必须显式拒绝。
pub const CODE_INVALID_OPS: &str = "edit_invalid_ops";
/// D-111：拉直角度使用闭区间，两个端点都合法。
pub const STRAIGHTEN_MIN_DEGREES: f64 = -45.0;
pub const STRAIGHTEN_MAX_DEGREES: f64 = 45.0;

/// 前端最终预览坐标系里的源像素裁剪矩形(方案 §6 `EditOps::crop`)。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// 编辑操作输入(方案 §6 `EditOps`)。`rotate` 为相对当前所见顺时针角度。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditOps {
    pub rotate: u16,
    pub flip_h: bool,
    pub flip_v: bool,
    pub crop: Option<CropRect>,
    /// D-106：v1 载荷没有此字段时反序列化为 `None`，且与显式 0 一样完全跳过插值路径。
    #[serde(default)]
    pub rotate_fine: Option<f64>,
    /// E3 调色(设计 §4.3)。v1 载荷缺省为 `None`;全零载荷由命令层
    /// [`super::adjust::effective_adjust`] 视同 `None`(D-106),不进入调色管线。
    #[serde(default)]
    pub adjust: Option<super::adjust::AdjustOps>,
}

/// `rotate` 合法值校验(方案契约:`0|90|180|270`)。
pub fn is_valid_rotate_step(rotate: u16) -> bool {
    matches!(rotate, 0 | 90 | 180 | 270)
}

/// 统一校验 v1 四态旋转、D-111 拉直闭区间与 E3 调色域。非有限值不能进入三角函数或像素
/// 尺寸计算;调色域越界与几何非法同在解码前拒绝。
pub fn validate_ops(ops: &EditOps) -> Result<f64, AppError> {
    if let Some(adjust) = &ops.adjust {
        super::adjust::validate_adjust(adjust)?;
    }
    let fine = ops.rotate_fine.unwrap_or(0.0);
    if !is_valid_rotate_step(ops.rotate)
        || !fine.is_finite()
        || !(STRAIGHTEN_MIN_DEGREES..=STRAIGHTEN_MAX_DEGREES).contains(&fine)
    {
        return Err(AppError::Edit {
            code: CODE_INVALID_OPS,
            message: "旋转或拉直角度非法 | invalid rotate or straighten angle".into(),
        });
    }
    Ok(fine)
}

/// imageproc `rotate_about_center_no_crop` 的展开尺寸公式。返回 `None` 表示尺寸溢出 u32，
/// 调用方必须按超出内存预算处理，不能继续分配。
pub fn expanded_dimensions(width: u32, height: u32, angle_degrees: f64) -> Option<(u32, u32)> {
    if angle_degrees == 0.0 {
        return Some((width, height));
    }
    // 刻意与 imageproc 0.27 的实现同用 f32，预算尺寸必须与真实分配逐像素一致。
    let radians = angle_degrees.to_radians() as f32;
    let cos = radians.cos().abs();
    let sin = radians.sin().abs();
    let expanded_width = (height as f32 * sin + width as f32 * cos).ceil();
    let expanded_height = (height as f32 * cos + width as f32 * sin).ceil();
    if !expanded_width.is_finite()
        || !expanded_height.is_finite()
        || expanded_width > u32::MAX as f32
        || expanded_height > u32::MAX as f32
    {
        return None;
    }
    Some((expanded_width as u32, expanded_height as u32))
}

/// 求旋转后仍保持原宽高比的最大居中内接矩形。候选矩形为 `s·w × s·h`；将其角点
/// 逆旋转回原矩形即可得到两个 `s` 上界，取较小者。最终统一向下取整，避免采到边界外像素。
pub fn maximum_aspect_inscribed_size(width: u32, height: u32, angle_degrees: f64) -> (u32, u32) {
    if width == 0 || height == 0 || angle_degrees == 0.0 {
        return (width, height);
    }
    let radians = angle_degrees.abs().to_radians();
    let cos = radians.cos().abs();
    let sin = radians.sin().abs();
    let width_f = f64::from(width);
    let height_f = f64::from(height);
    let width_scale = width_f / (width_f * cos + height_f * sin);
    let height_scale = height_f / (width_f * sin + height_f * cos);
    let scale = width_scale.min(height_scale);
    (
        (width_f * scale).floor().max(1.0) as u32,
        (height_f * scale).floor().max(1.0) as u32,
    )
}

/// 合入 V20 `view_rotation`(数据库存的用户看图台旋转,取值同样是 0/90/180/270)与本次
/// `ops.rotate`,取模 360 落回 0/90/180/270 四态之一。两个输入均已在各自写入路径校验过是
/// 四态之一,这里再用取模防御——万一 DB 里存在历史脏值也不会越界索引到不存在的分支。
pub fn combine_rotation(view_rotation: i64, ops_rotate: u16) -> u16 {
    let combined = (view_rotation.rem_euclid(360) as u32 + u32::from(ops_rotate)) % 360;
    match combined {
        90 => 90,
        180 => 180,
        270 => 270,
        _ => 0,
    }
}

/// 按几何链变换像素:参数校验 → orientation(一次)→ 合并旋转 → flip → fine rotate → crop。
/// `ops.rotate` 非法直接拒 [`CODE_INVALID_OPS`](D-010,不静默归 0);crop 越界先 clamp 到
/// 图像边界,clamp 后零面积返回 [`CODE_CROP_EMPTY`]。
pub fn apply_geometry(
    mut img: DynamicImage,
    file_orientation: image::metadata::Orientation,
    view_rotation: i64,
    ops: &EditOps,
) -> Result<DynamicImage, AppError> {
    let fine = validate_ops(ops)?;
    img.apply_orientation(file_orientation);

    match combine_rotation(view_rotation, ops.rotate) {
        90 => img = img.rotate90(),
        180 => img = img.rotate180(),
        270 => img = img.rotate270(),
        _ => {}
    }
    if ops.flip_h {
        img = img.fliph();
    }
    if ops.flip_v {
        img = img.flipv();
    }

    // D-106：缺省或 0° 必须完全绕过 imageproc，保证 v1 像素路径没有一次多余量化。
    if fine != 0.0 {
        img = apply_fine_rotation(img, fine);
    }

    if let Some(rect) = ops.crop {
        img = apply_crop(img, rect)?;
    }

    Ok(img)
}

fn apply_fine_rotation(img: DynamicImage, angle_degrees: f64) -> DynamicImage {
    let (source_width, source_height) = (img.width(), img.height());
    let theta = angle_degrees.to_radians() as f32;
    let rotated = match img {
        DynamicImage::ImageLuma8(buffer) => DynamicImage::ImageLuma8(rotate_about_center_no_crop(
            &buffer,
            theta,
            Interpolation::Bicubic,
            Border::Replicate,
        )),
        DynamicImage::ImageLumaA8(buffer) => DynamicImage::ImageLumaA8(
            rotate_about_center_no_crop(&buffer, theta, Interpolation::Bicubic, Border::Replicate),
        ),
        DynamicImage::ImageRgb8(buffer) => DynamicImage::ImageRgb8(rotate_about_center_no_crop(
            &buffer,
            theta,
            Interpolation::Bicubic,
            Border::Replicate,
        )),
        DynamicImage::ImageRgba8(buffer) => DynamicImage::ImageRgba8(rotate_about_center_no_crop(
            &buffer,
            theta,
            Interpolation::Bicubic,
            Border::Replicate,
        )),
        DynamicImage::ImageLuma16(buffer) => DynamicImage::ImageLuma16(
            rotate_about_center_no_crop(&buffer, theta, Interpolation::Bicubic, Border::Replicate),
        ),
        DynamicImage::ImageLumaA16(buffer) => DynamicImage::ImageLumaA16(
            rotate_about_center_no_crop(&buffer, theta, Interpolation::Bicubic, Border::Replicate),
        ),
        DynamicImage::ImageRgb16(buffer) => DynamicImage::ImageRgb16(rotate_about_center_no_crop(
            &buffer,
            theta,
            Interpolation::Bicubic,
            Border::Replicate,
        )),
        DynamicImage::ImageRgba16(buffer) => DynamicImage::ImageRgba16(
            rotate_about_center_no_crop(&buffer, theta, Interpolation::Bicubic, Border::Replicate),
        ),
        DynamicImage::ImageRgb32F(buffer) => DynamicImage::ImageRgb32F(
            rotate_about_center_no_crop(&buffer, theta, Interpolation::Bicubic, Border::Replicate),
        ),
        DynamicImage::ImageRgba32F(buffer) => DynamicImage::ImageRgba32F(
            rotate_about_center_no_crop(&buffer, theta, Interpolation::Bicubic, Border::Replicate),
        ),
        // DynamicImage 是 non_exhaustive；当前 image 0.25 的全部变体已在上面逐一保位深处理。
        // 若未来新增变体，先提升到 RGBA16，避免静默退化为 8-bit。
        other => {
            let buffer = other.into_rgba16();
            DynamicImage::ImageRgba16(rotate_about_center_no_crop(
                &buffer,
                theta,
                Interpolation::Bicubic,
                Border::Replicate,
            ))
        }
    };
    let (inner_width, inner_height) =
        maximum_aspect_inscribed_size(source_width, source_height, angle_degrees);
    let x = rotated.width().saturating_sub(inner_width) / 2;
    let y = rotated.height().saturating_sub(inner_height) / 2;
    rotated.crop_imm(x, y, inner_width, inner_height)
}

fn apply_crop(img: DynamicImage, rect: CropRect) -> Result<DynamicImage, AppError> {
    let (w, h) = (img.width(), img.height());
    let x = rect.x.min(w);
    let y = rect.y.min(h);
    let width = rect.width.min(w.saturating_sub(x));
    let height = rect.height.min(h.saturating_sub(y));
    if width == 0 || height == 0 {
        return Err(AppError::Edit {
            code: CODE_CROP_EMPTY,
            message: "裁剪区域为空 | crop region is empty".into(),
        });
    }
    Ok(img.crop_imm(x, y, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::metadata::Orientation;
    use image::{Rgb, RgbImage};

    fn ops(rotate: u16, flip_h: bool, flip_v: bool, crop: Option<CropRect>) -> EditOps {
        EditOps {
            rotate,
            flip_h,
            flip_v,
            crop,
            rotate_fine: None,
            adjust: None,
        }
    }

    /// 3x2(宽×高)标记图:四角与中心像素各不相同,足以在旋转/翻转后按坐标断言,不需要
    /// 逐像素比对整幅图。
    fn marker_image() -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(3, 2, |x, y| {
            Rgb([(x * 50) as u8, (y * 80) as u8, 200])
        }))
    }

    #[test]
    fn combine_rotation_wraps_mod_360() {
        assert_eq!(combine_rotation(0, 0), 0);
        assert_eq!(combine_rotation(90, 90), 180);
        assert_eq!(combine_rotation(270, 180), 90); // 450 % 360 = 90
        assert_eq!(combine_rotation(270, 270), 180); // 540 % 360 = 180
        assert_eq!(combine_rotation(-90, 0), 270); // 历史脏值防御:rem_euclid 折回正值
    }

    #[test]
    fn orientation_applied_exactly_once_then_view_rotation_combines() {
        // file orientation=Rotate90(EXIF 6)已经把 3x2 转成 2x3;之后再合入 view_rotation=90
        // 应再转一次回到 3x2——验证「先 orientation 再合并旋转」是两次独立生效,不是被吞掉一次。
        let img = marker_image();
        let out =
            apply_geometry(img, Orientation::Rotate90, 90, &ops(0, false, false, None)).unwrap();
        assert_eq!((out.width(), out.height()), (3, 2));
    }

    #[test]
    fn fliph_mirrors_horizontally() {
        let img = marker_image();
        let out = apply_geometry(
            img,
            Orientation::NoTransforms,
            0,
            &ops(0, true, false, None),
        )
        .unwrap();
        // fliph 保持尺寸,像素左右镜像:原 (0,0) 到 (2,0)。
        let flipped = out.to_rgb8();
        let original = marker_image().to_rgb8();
        assert_eq!(*flipped.get_pixel(0, 0), *original.get_pixel(2, 0));
        assert_eq!(*flipped.get_pixel(2, 0), *original.get_pixel(0, 0));
    }

    #[test]
    fn flip_is_applied_after_rotation_not_before() {
        // 顺序判别:rotate90(顺时针)后接 fliph 恰是转置 out(x,y)=src(y,x);若顺序颠倒
        // (先 fliph 再 rotate90)则 out(x,y)=src(w-1-y, h-1-x),在非对称标记图上两者可区分。
        // 原「flip 在 rotate 之后」只靠链式代码顺序保证,这里用像素级断言钉死。
        let out = apply_geometry(
            marker_image(),
            Orientation::NoTransforms,
            0,
            &ops(90, true, false, None),
        )
        .unwrap();
        assert_eq!((out.width(), out.height()), (2, 3));
        let out = out.to_rgb8();
        let src = marker_image().to_rgb8();
        for (x, y) in [(0u32, 0u32), (0, 2), (1, 1), (1, 2)] {
            assert_eq!(*out.get_pixel(x, y), *src.get_pixel(y, x), "at ({x},{y})");
        }
    }

    #[test]
    fn crop_is_in_post_transform_coordinate_space() {
        // rotate90 后图像变 2x3;crop 矩形按变换后的坐标系给出,应直接生效不再反推。
        let img = marker_image();
        let out = apply_geometry(
            img,
            Orientation::NoTransforms,
            0,
            &ops(
                90,
                false,
                false,
                Some(CropRect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                }),
            ),
        )
        .unwrap();
        assert_eq!((out.width(), out.height()), (2, 1));
    }

    #[test]
    fn crop_out_of_bounds_is_clamped_not_rejected() {
        let img = marker_image();
        let out = apply_geometry(
            img,
            Orientation::NoTransforms,
            0,
            &ops(
                0,
                false,
                false,
                Some(CropRect {
                    x: 1,
                    y: 1,
                    width: 100,
                    height: 100,
                }),
            ),
        )
        .unwrap();
        assert_eq!((out.width(), out.height()), (2, 1)); // 3-1, 2-1
    }

    #[test]
    fn crop_fully_out_of_bounds_is_zero_area_error() {
        let img = marker_image();
        let err = apply_geometry(
            img,
            Orientation::NoTransforms,
            0,
            &ops(
                0,
                false,
                false,
                Some(CropRect {
                    x: 3, // == 图像宽度,clamp 后 width=0
                    y: 0,
                    width: 5,
                    height: 1,
                }),
            ),
        )
        .unwrap_err();
        match err {
            AppError::Edit { code, .. } => assert_eq!(code, CODE_CROP_EMPTY),
            other => panic!("expected AppError::Edit, got {other:?}"),
        }
    }

    #[test]
    fn zero_area_crop_rect_is_rejected() {
        let img = marker_image();
        let err = apply_geometry(
            img,
            Orientation::NoTransforms,
            0,
            &ops(
                0,
                false,
                false,
                Some(CropRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 2,
                }),
            ),
        )
        .unwrap_err();
        match err {
            AppError::Edit { code, .. } => assert_eq!(code, CODE_CROP_EMPTY),
            other => panic!("expected AppError::Edit, got {other:?}"),
        }
    }

    #[test]
    fn invalid_rotate_is_rejected_with_stable_code() {
        // D-010:非法 rotate 显式拒绝,不再静默取模归 0(那会保存出「没转的图」)。
        for bad in [1u16, 45, 91, 360] {
            let err = apply_geometry(
                marker_image(),
                Orientation::NoTransforms,
                0,
                &ops(bad, false, false, None),
            )
            .unwrap_err();
            match err {
                AppError::Edit { code, .. } => assert_eq!(code, CODE_INVALID_OPS),
                other => panic!("expected AppError::Edit, got {other:?}"),
            }
        }
    }

    #[test]
    fn valid_rotate_steps() {
        for v in [0, 90, 180, 270] {
            assert!(is_valid_rotate_step(v));
        }
        for v in [1, 45, 89, 91, 360] {
            assert!(!is_valid_rotate_step(v));
        }
    }

    #[test]
    fn v1_payload_without_new_fields_remains_compatible() {
        let decoded: EditOps =
            serde_json::from_str(r#"{"rotate":90,"flipH":true,"flipV":false,"crop":null}"#)
                .unwrap();
        assert_eq!(decoded.rotate_fine, None);
        assert_eq!(decoded.adjust, None);
    }

    #[test]
    fn adjust_domain_is_validated_alongside_geometry() {
        let mut input = ops(0, false, false, None);
        input.adjust = Some(crate::editing::adjust::AdjustOps {
            brightness: 101,
            contrast: 0,
            saturation: 0,
        });
        let err = validate_ops(&input).unwrap_err();
        match err {
            AppError::Edit { code, .. } => assert_eq!(code, CODE_INVALID_OPS),
            other => panic!("expected AppError::Edit, got {other:?}"),
        }
        input.adjust = Some(crate::editing::adjust::AdjustOps {
            brightness: -100,
            contrast: 100,
            saturation: 0,
        });
        assert!(validate_ops(&input).is_ok());
    }

    #[test]
    fn fine_rotation_domain_is_closed_and_rejects_non_finite_values() {
        for valid in [STRAIGHTEN_MIN_DEGREES, 0.0, STRAIGHTEN_MAX_DEGREES] {
            let mut input = ops(0, false, false, None);
            input.rotate_fine = Some(valid);
            assert!(validate_ops(&input).is_ok(), "angle={valid}");
        }
        for invalid in [45.0 + 1e-9, f64::NAN, f64::INFINITY] {
            let mut input = ops(0, false, false, None);
            input.rotate_fine = Some(invalid);
            let err = validate_ops(&input).unwrap_err();
            match err {
                AppError::Edit { code, .. } => assert_eq!(code, CODE_INVALID_OPS),
                other => panic!("expected AppError::Edit, got {other:?}"),
            }
        }
    }

    #[test]
    fn zero_fine_rotation_preserves_v1_pixels_exactly() {
        let source = marker_image();
        let mut explicit_zero = ops(90, true, false, None);
        explicit_zero.rotate_fine = Some(0.0);
        let old = apply_geometry(
            source.clone(),
            Orientation::NoTransforms,
            0,
            &ops(90, true, false, None),
        )
        .unwrap();
        let new = apply_geometry(source, Orientation::NoTransforms, 0, &explicit_zero).unwrap();
        assert_eq!(old, new);
    }

    #[test]
    fn fine_rotation_uses_bicubic_and_crops_to_aspect_inscribed_size() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_fn(400, 300, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }));
        let mut input = ops(0, false, false, None);
        input.rotate_fine = Some(45.0);
        let out = apply_geometry(source, Orientation::NoTransforms, 0, &input).unwrap();
        assert_eq!((out.width(), out.height()), (242, 181));
    }

    #[test]
    fn fine_rotation_preserves_16_bit_variant() {
        let source = DynamicImage::ImageRgb16(image::ImageBuffer::from_pixel(
            40,
            30,
            image::Rgb([10_000, 20_000, 30_000]),
        ));
        let mut input = ops(0, false, false, None);
        input.rotate_fine = Some(10.0);
        let out = apply_geometry(source, Orientation::NoTransforms, 0, &input).unwrap();
        assert!(matches!(out, DynamicImage::ImageRgb16(_)));
    }

    #[test]
    fn shared_straighten_golden_vectors_match_closed_form() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Golden {
            width: u32,
            height: u32,
            angle: f64,
            inner_width: u32,
            inner_height: u32,
        }
        let vectors: Vec<Golden> = serde_json::from_str(include_str!(
            "../../../src/fixtures/straightenGeometryGolden.json"
        ))
        .unwrap();
        for vector in vectors {
            assert_eq!(
                maximum_aspect_inscribed_size(vector.width, vector.height, vector.angle),
                (vector.inner_width, vector.inner_height)
            );
        }
    }
}
