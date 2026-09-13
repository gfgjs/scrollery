// crates/scrollery-ai-core/src/ocr/det.rs
//! 文本检测:DB(Differentiable Binarization)前处理 + 概率图后处理。
//!
//! 前处理:RGB8 → 按 `swap_rb` 通道序 → `scale = min(1, limit_side_len/max(h,w))` →
//! 目标边取 32 的倍数 resize(bilinear)→ `[1,3,H,W]` f32,`(px/255-mean)/std`
//! (ImageNet mean/std,与通道序同序应用)。
//!
//! 后处理(输出 `[1,1,H,W]` 概率图):阈值二值化 → `find_contours` → `min_area_rect` →
//! 短边/框分数过滤 → 确定性 unclip 外扩(不引 clipper)→ clamp → 除 ratio 回原图坐标 →
//! 阅读序(按质心 y 分行、行内按 x)。参数全部取自 [`OcrProfile`]。

use image::{GrayImage, Luma, RgbImage};
use imageproc::contours::{find_contours, BorderType};
use imageproc::geometry::min_area_rect;
use imageproc::point::Point;
use ort::value::Tensor;

use super::chw_tensor;
use crate::engine::SessionPool;
use crate::error::{AiError, Result};
use crate::ocr_profile::OcrProfile;

/// ImageNet 归一化(DB 检测惯例)。
const DET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// 计算 resize 目标边(32 的倍数,≥32),按 `scale = min(1, limit/max(w,h))` 等比。
pub(crate) fn resize_dims(w: u32, h: u32, limit_side_len: u32) -> (u32, u32) {
    let max_side = w.max(h) as f32;
    let scale = if max_side > 0.0 {
        (limit_side_len as f32 / max_side).min(1.0)
    } else {
        1.0
    };
    let round32 = |e: u32| -> u32 {
        let v = (e as f32 * scale / 32.0).round() as u32;
        v.max(1) * 32
    };
    (round32(w), round32(h))
}

/// 前处理:返回 (CHW f32 张量, resized_w, resized_h)。
fn preprocess(img: &RgbImage, profile: &OcrProfile) -> (Vec<f32>, u32, u32) {
    let (w, h) = img.dimensions();
    let (rw, rh) = resize_dims(w, h, profile.det_limit_side_len);
    let resized = image::imageops::resize(img, rw, rh, image::imageops::FilterType::Triangle);
    let flat = chw_tensor(&resized, profile.swap_rb, DET_MEAN, DET_STD);
    (flat, rw, rh)
}

/// quad(`[TL,TR,BR,BL]`)的宽/高:宽=上下边长均值,高=左右边长均值。
fn box_wh(q: &[[f32; 2]; 4]) -> (f32, f32) {
    let d = |a: [f32; 2], b: [f32; 2]| {
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        (dx * dx + dy * dy).sqrt()
    };
    let w = 0.5 * (d(q[0], q[1]) + d(q[3], q[2]));
    let h = 0.5 * (d(q[0], q[3]) + d(q[1], q[2]));
    (w, h)
}

/// 多边形面积(shoelace,绝对值)。
fn poly_area(q: &[[f32; 2]; 4]) -> f32 {
    let mut s = 0.0;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        s += a[0] * b[1] - b[0] * a[1];
    }
    0.5 * s.abs()
}

/// 多边形周长。
fn poly_perimeter(q: &[[f32; 2]; 4]) -> f32 {
    let mut p = 0.0;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        p += (dx * dx + dy * dy).sqrt();
    }
    p
}

/// 两条参数直线(点 + 方向)求交,平行返回 None。
fn intersect(p0: [f32; 2], d0: [f32; 2], p1: [f32; 2], d1: [f32; 2]) -> Option<[f32; 2]> {
    let denom = d0[0] * d1[1] - d0[1] * d1[0];
    if denom.abs() < 1e-6 {
        return None;
    }
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let t = (dx * d1[1] - dy * d1[0]) / denom;
    Some([p0[0] + t * d0[0], p0[1] + t * d0[1]])
}

/// 确定性 unclip 外扩:`d = area·ratio/perimeter`,每边沿外法向平移 d,相邻平移线求交得新角。
pub(crate) fn unclip(q: &[[f32; 2]; 4], ratio: f32) -> [[f32; 2]; 4] {
    let peri = poly_perimeter(q);
    if peri < 1e-6 {
        return *q;
    }
    let d = poly_area(q) * ratio / peri;
    let cx = q.iter().map(|p| p[0]).sum::<f32>() / 4.0;
    let cy = q.iter().map(|p| p[1]).sum::<f32>() / 4.0;

    // 每条边 i→i+1 的外向平移线(点 + 单位方向)。
    let mut lines = [([0.0f32; 2], [0.0f32; 2]); 4];
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let ex = b[0] - a[0];
        let ey = b[1] - a[1];
        let len = (ex * ex + ey * ey).sqrt().max(1e-6);
        let (dx, dy) = (ex / len, ey / len);
        // 法向候选,选指向远离质心者(外向)。
        let (mut nx, mut ny) = (-dy, dx);
        let mx = 0.5 * (a[0] + b[0]);
        let my = 0.5 * (a[1] + b[1]);
        if (mx - cx) * nx + (my - cy) * ny < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        lines[i] = ([a[0] + nx * d, a[1] + ny * d], [dx, dy]);
    }
    // 新角 i = 边(i-1)与边 i 的平移线交点(均过原顶点 q[i])。
    // miter join:角点沿对角外扩 ≈d√2,较 PaddleOCR 的 round join 框略大——有意简化
    //(确定性、免引 clipper),对 rec 裁剪无害;勿当 bug 修。
    let mut res = *q;
    for i in 0..4 {
        let (p_prev, d_prev) = lines[(i + 3) % 4];
        let (p_cur, d_cur) = lines[i];
        if let Some(pt) = intersect(p_prev, d_prev, p_cur, d_cur) {
            res[i] = pt;
        }
    }
    res
}

/// 点是否落在凸 quad 内(min_area_rect 恒凸):逐边叉积符号一致即内部(容 CW/CCW)。
fn point_in_quad(px: f32, py: f32, q: &[[f32; 2]; 4]) -> bool {
    let mut pos = false;
    let mut neg = false;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let cross = (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0]);
        if cross > 1e-6 {
            pos = true;
        } else if cross < -1e-6 {
            neg = true;
        }
        if pos && neg {
            return false;
        }
    }
    true
}

/// 框分数:quad **多边形掩膜内**的概率均值(PaddleOCR box_score_fast 的 poly 填充语义)。
/// 轴对齐包围盒仅作遍历窗,窗内逐点 point-in-quad 过滤——斜置行的分数不被包围盒角落背景稀释
/// (换轴对齐直接均值会把斜行误判低于 box_thresh 而丢框)。
fn box_score(prob: &[f32], pw: u32, ph: u32, q: &[[f32; 2]; 4]) -> f32 {
    let xs = [q[0][0], q[1][0], q[2][0], q[3][0]];
    let ys = [q[0][1], q[1][1], q[2][1], q[3][1]];
    let xmin = xs.iter().cloned().fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let xmax = (xs.iter().cloned().fold(f32::MIN, f32::max).ceil() as i64).clamp(0, (pw - 1) as i64)
        as u32;
    let ymin = ys.iter().cloned().fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let ymax = (ys.iter().cloned().fold(f32::MIN, f32::max).ceil() as i64).clamp(0, (ph - 1) as i64)
        as u32;
    if xmax < xmin || ymax < ymin {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut cnt = 0u64;
    let mut win_sum = 0.0f64;
    let mut win_cnt = 0u64;
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let p = prob[(y * pw + x) as usize] as f64;
            win_sum += p;
            win_cnt += 1;
            // 像素中心在 quad 内才计入掩膜均值。
            if point_in_quad(x as f32 + 0.5, y as f32 + 0.5, q) {
                sum += p;
                cnt += 1;
            }
        }
    }
    if cnt > 0 {
        (sum / cnt as f64) as f32
    } else if win_cnt > 0 {
        // 极薄 quad 掩膜采样为空 → 退化为窗均值兜底(不静默返 0)。
        (win_sum / win_cnt as f64) as f32
    } else {
        0.0
    }
}

/// 从概率图产出原图坐标 quad(未排序)。`ratio_*` 把 probmap 坐标乘回原图。
pub(crate) fn boxes_from_prob(
    prob: &[f32],
    pw: u32,
    ph: u32,
    ratio_w: f32,
    ratio_h: f32,
    profile: &OcrProfile,
) -> Vec<[[f32; 2]; 4]> {
    // 二值 mask。
    let mut mask = GrayImage::new(pw, ph);
    for y in 0..ph {
        for x in 0..pw {
            if prob[(y * pw + x) as usize] > profile.det_thresh {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
    }
    let contours: Vec<_> = find_contours::<i32>(&mask);
    // probmap 每原图像素的比例(把 min_box_side 换算到 probmap 坐标)。两轴 round32 比例可不等,
    // 取均值避免竖向偏差(单用 ratio_w 会让高瘦框在两轴比例悬殊时误过/误丢)。
    let ratio_avg = 0.5 * (ratio_w + ratio_h);
    let pm_per_orig = 1.0 / ratio_avg.max(1e-6);
    let min_side_pm = profile.det_min_box_side * pm_per_orig;

    let mut out: Vec<[[f32; 2]; 4]> = Vec::new();
    for c in &contours {
        if c.border_type != BorderType::Outer {
            continue;
        }
        if c.points.len() < 4 {
            continue;
        }
        let rect: [Point<i32>; 4] = min_area_rect(&c.points);
        let mut quad = [[0.0f32; 2]; 4];
        for (dst, r) in quad.iter_mut().zip(rect.iter()) {
            *dst = [r.x as f32, r.y as f32];
        }
        let (bw, bh) = box_wh(&quad);
        if bw.min(bh) < min_side_pm {
            continue;
        }
        if box_score(prob, pw, ph, &quad) < profile.det_box_thresh {
            continue;
        }
        let mut uq = unclip(&quad, profile.det_unclip_ratio);
        for p in uq.iter_mut() {
            p[0] = p[0].clamp(0.0, (pw.saturating_sub(1)) as f32);
            p[1] = p[1].clamp(0.0, (ph.saturating_sub(1)) as f32);
            // 回映原图坐标。
            p[0] *= ratio_w;
            p[1] *= ratio_h;
        }
        out.push(uq);
    }
    out
}

/// 阅读序排序:按质心 y 分行(容差 = 框高中位数一半),行内按质心 x 升序。
pub(crate) fn sort_reading_order(quads: &mut Vec<[[f32; 2]; 4]>) {
    if quads.len() < 2 {
        return;
    }
    // (cy, cx, quad)
    let mut items: Vec<(f32, f32, [[f32; 2]; 4])> = quads
        .iter()
        .map(|q| {
            let cx = q.iter().map(|p| p[0]).sum::<f32>() / 4.0;
            let cy = q.iter().map(|p| p[1]).sum::<f32>() / 4.0;
            (cy, cx, *q)
        })
        .collect();
    // 框高中位数。
    let mut heights: Vec<f32> = quads.iter().map(|q| box_wh(q).1).collect();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_h = heights[heights.len() / 2];
    let tol = (median_h * 0.5).max(1.0);

    // 先按 y 排,聚成行,行内按 x 排。
    items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut result: Vec<[[f32; 2]; 4]> = Vec::with_capacity(items.len());
    let mut row: Vec<(f32, [[f32; 2]; 4])> = Vec::new();
    let mut row_y = items[0].0;
    for (cy, cx, q) in items {
        if (cy - row_y).abs() > tol && !row.is_empty() {
            row.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            result.extend(row.drain(..).map(|(_, q)| q));
            row_y = cy;
        }
        row.push((cx, q));
    }
    row.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    result.extend(row.drain(..).map(|(_, q)| q));
    *quads = result;
}

/// 检测全链:前处理 → session 推理 → 概率图后处理 → 阅读序。返回原图坐标 quad。
pub(crate) fn detect(
    pool: &SessionPool,
    img: &RgbImage,
    profile: &OcrProfile,
) -> Result<Vec<[[f32; 2]; 4]>> {
    let (flat, rw, rh) = preprocess(img, profile);

    let mut guard = pool
        .get()
        .ok_or_else(|| AiError::Ocr("OCR det pool disconnected".into()))?;
    let input_name = guard
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| AiError::Ocr("OCR det has no input".into()))?;
    let tensor =
        Tensor::from_array(([1i64, 3, rh as i64, rw as i64], flat)).map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), tensor)])
        .map_err(AiError::Ort)?;
    if outputs.len() == 0 {
        return Err(AiError::Ocr("det model returned no output".into()));
    }
    let (shape, slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(AiError::Ort)?;
    let dims = shape.len();
    if dims < 2 {
        return Err(AiError::Ocr(format!("det output rank {dims} < 2")));
    }
    let ph = shape[dims - 2] as u32;
    let pw = shape[dims - 1] as u32;
    let prob = slice.to_vec();
    drop(outputs);
    drop(guard);

    let (ow, oh) = img.dimensions();
    let ratio_w = ow as f32 / (pw.max(1)) as f32;
    let ratio_h = oh as f32 / (ph.max(1)) as f32;
    let mut quads = boxes_from_prob(&prob, pw, ph, ratio_w, ratio_h, profile);
    sort_reading_order(&mut quads);
    Ok(quads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr_profile::default_ocr_profile;

    #[test]
    fn resize_dims_multiple_of_32_and_ratio() {
        // 大图触发降采样:1920×1080,limit 960。
        let (rw, rh) = resize_dims(1920, 1080, 960);
        assert_eq!(rw % 32, 0);
        assert_eq!(rh % 32, 0);
        assert!((928..=992).contains(&rw), "rw={rw}");
        // 小图不放大:scale=min(1,..)=1。100×50。
        let (sw, sh) = resize_dims(100, 50, 960);
        assert_eq!(sw % 32, 0);
        assert_eq!(sh % 32, 0);
        assert!(sw <= 128 && sh <= 64);
    }

    #[test]
    fn single_box_from_prob_rectangle() {
        // 100×100 概率图,内画 (30,30)-(70,70) 一块 p=1 矩形。
        let (pw, ph) = (100u32, 100u32);
        let mut prob = vec![0.0f32; (pw * ph) as usize];
        for y in 30..70 {
            for x in 30..70 {
                prob[(y * pw + x) as usize] = 1.0;
            }
        }
        let profile = default_ocr_profile();
        let boxes = boxes_from_prob(&prob, pw, ph, 1.0, 1.0, &profile);
        assert_eq!(boxes.len(), 1, "应恰产 1 框");
        let q = boxes[0];
        // 角点应接近原矩形(unclip 外扩后)。
        let xs = [q[0][0], q[1][0], q[2][0], q[3][0]];
        let ys = [q[0][1], q[1][1], q[2][1], q[3][1]];
        let xmin = xs.iter().cloned().fold(f32::MAX, f32::min);
        let xmax = xs.iter().cloned().fold(f32::MIN, f32::max);
        let ymin = ys.iter().cloned().fold(f32::MAX, f32::min);
        let ymax = ys.iter().cloned().fold(f32::MIN, f32::max);
        // unclip 外扩 → 包围盒应大于原始 40×40(d = area·ratio/peri = 1600·1.5/160 = 15/边)。
        assert!(xmax - xmin > 40.0, "unclip 应外扩: w={}", xmax - xmin);
        assert!(ymax - ymin > 40.0, "unclip 应外扩: h={}", ymax - ymin);
        // 原矩形 (30,30)-(70,70),外扩 ≈15px → xmin≈15、xmax≈85(±5px 容差)。
        assert!((10.0..20.0).contains(&xmin), "xmin={xmin}");
        assert!((80.0..90.0).contains(&xmax), "xmax={xmax}");
    }

    #[test]
    fn box_score_poly_mask_not_diluted_by_bbox() {
        // 斜置概率块(45° 菱形,质心 (50,50)、半对角 30),仅块内 p=1。
        let (pw, ph) = (100u32, 100u32);
        let q = [[50.0, 20.0], [80.0, 50.0], [50.0, 80.0], [20.0, 50.0]];
        let mut prob = vec![0.0f32; (pw * ph) as usize];
        for y in 0..ph {
            for x in 0..pw {
                if point_in_quad(x as f32 + 0.5, y as f32 + 0.5, &q) {
                    prob[(y * pw + x) as usize] = 1.0;
                }
            }
        }
        // poly 掩膜均值 ≈ 1.0。
        let poly = box_score(&prob, pw, ph, &q);
        // 对照:轴对齐包围盒直接均值(菱形面积约为 bbox 一半 → 被背景稀释到 ≈0.5)。
        let mut s = 0.0f64;
        let mut n = 0u64;
        for y in 20..=80u32 {
            for x in 20..=80u32 {
                s += prob[(y * pw + x) as usize] as f64;
                n += 1;
            }
        }
        let bbox_mean = (s / n as f64) as f32;
        assert!(poly > bbox_mean + 0.2, "poly={poly} bbox={bbox_mean}");
        assert!(poly >= 0.6, "斜置块不应被 box_thresh 误丢: poly={poly}");
        assert!(
            bbox_mean < 0.6,
            "对照:bbox 稀释均值低于阈值 bbox={bbox_mean}"
        );
    }

    #[test]
    fn unclip_expands_area() {
        let q = [[30.0, 30.0], [70.0, 30.0], [70.0, 70.0], [30.0, 70.0]];
        let uq = unclip(&q, 1.5);
        assert!(poly_area(&uq) > poly_area(&q), "unclip 后面积应更大");
    }

    #[test]
    fn reading_order_two_rows_three_boxes() {
        // 两行三框乱序输入 → 阅读序(上行先、行内左先)。
        // 行1 y≈10: 框A(x≈10)、框B(x≈50);行2 y≈60: 框C(x≈10)。
        let mk = |cx: f32, cy: f32| {
            [
                [cx - 5.0, cy - 5.0],
                [cx + 5.0, cy - 5.0],
                [cx + 5.0, cy + 5.0],
                [cx - 5.0, cy + 5.0],
            ]
        };
        let mut quads = vec![
            mk(50.0, 10.0), // B
            mk(10.0, 60.0), // C(下行)
            mk(10.0, 10.0), // A
        ];
        sort_reading_order(&mut quads);
        let cx = |q: &[[f32; 2]; 4]| q.iter().map(|p| p[0]).sum::<f32>() / 4.0;
        let cy = |q: &[[f32; 2]; 4]| q.iter().map(|p| p[1]).sum::<f32>() / 4.0;
        // 顺序应为 A(10,10) → B(50,10) → C(10,60)。
        assert!((cx(&quads[0]) - 10.0).abs() < 1.0 && (cy(&quads[0]) - 10.0).abs() < 1.0);
        assert!((cx(&quads[1]) - 50.0).abs() < 1.0 && (cy(&quads[1]) - 10.0).abs() < 1.0);
        assert!((cy(&quads[2]) - 60.0).abs() < 1.0);
    }
}
