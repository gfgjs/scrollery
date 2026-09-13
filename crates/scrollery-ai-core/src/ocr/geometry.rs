// crates/scrollery-ai-core/src/ocr/geometry.rs
//! 旋转矩形透视裁剪(`get_rotate_crop_image`)。
//!
//! 从原图按检测 quad(顺序 `[TL, TR, BR, BL]`)透视 warp 出一张摆正的文本行 crop:
//! 目标宽 = 上下边长均值取整,高 = 左右边长均值取整;若 `h/w ≥ 1.5`(竖条)顺时针旋 90° 转横。

use image::RgbImage;
use imageproc::geometric_transformations::{warp_into, Border, Interpolation, Projection};

/// 两点欧氏距离。
fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    (dx * dx + dy * dy).sqrt()
}

/// 从 quad(`[TL, TR, BR, BL]`,原图坐标)透视裁出摆正 crop。
pub(crate) fn get_rotate_crop_image(img: &RgbImage, quad: &[[f32; 2]; 4]) -> RgbImage {
    let (tl, tr, br, bl) = (quad[0], quad[1], quad[2], quad[3]);
    // 目标宽 = 上下边长均值;高 = 左右边长均值。
    let w = (0.5 * (dist(tl, tr) + dist(bl, br))).round().max(1.0) as u32;
    let h = (0.5 * (dist(tl, bl) + dist(tr, br))).round().max(1.0) as u32;

    // 前向投影 quad → 目标矩形四角;warp_into 内部取逆按输出像素回采源图。
    let from = [
        (tl[0], tl[1]),
        (tr[0], tr[1]),
        (br[0], br[1]),
        (bl[0], bl[1]),
    ];
    let to = [
        (0.0, 0.0),
        (w as f32, 0.0),
        (w as f32, h as f32),
        (0.0, h as f32),
    ];

    let mut out = RgbImage::new(w, h);
    if let Some(proj) = Projection::from_control_points(from, to) {
        warp_into(
            img,
            proj,
            Interpolation::Bilinear,
            Border::Constant(image::Rgb([0, 0, 0])),
            &mut out,
        );
    }
    // else:四点共线等奇异 quad,输出保持黑图(rec 会以低置信度丢行)。

    // 竖条转横:h/w ≥ 1.5 顺时针旋 90°。
    if h as f32 >= 1.5 * w as f32 {
        image::imageops::rotate90(&out)
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在大图上画一块纯色矩形,以其四角作 quad → warp 出目标尺寸,角点像素为该色。
    #[test]
    fn warp_target_size_and_color() {
        let mut img = RgbImage::new(100, 100);
        for y in 20..60 {
            for x in 10..90 {
                img.put_pixel(x, y, image::Rgb([200, 100, 50]));
            }
        }
        // quad 宽 80、高 40 → w/h 不触发旋转。
        let quad = [[10.0, 20.0], [90.0, 20.0], [90.0, 60.0], [10.0, 60.0]];
        let crop = get_rotate_crop_image(&img, &quad);
        assert_eq!(crop.dimensions(), (80, 40));
        // 中心像素应为矩形色。
        let c = crop.get_pixel(40, 20).0;
        assert!(
            c[0] > 150 && c[1] > 50 && c[2] > 20,
            "中心应采到矩形色: {c:?}"
        );
    }

    /// h/w ≥ 1.5 触发旋转:竖条(宽 20、高 60)→ 输出转横(宽 60、高 20)。
    #[test]
    fn vertical_strip_rotated() {
        let img = RgbImage::new(100, 100);
        let quad = [[10.0, 10.0], [30.0, 10.0], [30.0, 70.0], [10.0, 70.0]];
        let crop = get_rotate_crop_image(&img, &quad);
        // 原始 w=20 h=60 → 旋转后 (w,h)=(60,20)。
        assert_eq!(crop.dimensions(), (60, 20));
    }
}
