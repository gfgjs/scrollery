// crates/scrollery-ai-core/src/ocr/cls.rs
//! 文本行方向分类(0° / 180°)+ 按需 180° 翻转。
//!
//! 每 crop 等比缩至 `profile.cls_input_hw`(左对齐零填充),`(px/255-0.5)/0.5` 归一化,
//! **逐张跑**(cls 极小,毫秒级,不组批);输出 `[1,2]` 概率,`argmax==1 && p1 ≥ cls_thresh`
//! → crop 旋 180°。通道序随 `profile.swap_rb`(与 det/rec 统一,PaddleOCR DecodeImage BGR 惯例)。

use image::RgbImage;
use ort::value::Tensor;

use super::chw_tensor;
use crate::engine::SessionPool;
use crate::error::{AiError, Result};
use crate::ocr_profile::OcrProfile;

/// 方向翻转决策:输出两类概率(0°/180°),第 1 类(180°)胜出且 ≥ 阈值即翻转。
pub(crate) fn cls_decision(p0: f32, p1: f32, thresh: f32) -> bool {
    p1 >= p0 && p1 >= thresh
}

/// 等比缩至 `(th, tw)` 高、左对齐,右侧零填充到 tw,构造 CHW 归一化张量。
fn cls_preprocess(crop: &RgbImage, tw: u32, th: u32, swap_rb: bool) -> Vec<f32> {
    let (w, h) = crop.dimensions();
    // 等比缩到高 th,宽按比例并 cap 到 tw。
    let scaled_w = if h == 0 {
        1
    } else {
        (((w as f32) * (th as f32) / (h as f32)).round() as u32)
            .max(1)
            .min(tw)
    };
    let resized =
        image::imageops::resize(crop, scaled_w, th, image::imageops::FilterType::Triangle);
    // 左对齐贴入 tw×th 黑底画布。
    let mut canvas = RgbImage::new(tw, th);
    for y in 0..th {
        for x in 0..scaled_w {
            canvas.put_pixel(x, y, *resized.get_pixel(x, y));
        }
    }
    chw_tensor(&canvas, swap_rb, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5])
}

/// 分类并按需翻转:返回摆正后的 crop(消费入参 crop)。
pub(crate) fn classify_and_maybe_flip(
    pool: &SessionPool,
    crop: RgbImage,
    profile: &OcrProfile,
) -> Result<RgbImage> {
    let (th, tw) = profile.cls_input_hw;
    let flat = cls_preprocess(&crop, tw, th, profile.swap_rb);

    let mut guard = pool
        .get()
        .ok_or_else(|| AiError::Ocr("OCR cls pool disconnected".into()))?;
    let input_name = guard
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| AiError::Ocr("OCR cls has no input".into()))?;
    let tensor =
        Tensor::from_array(([1i64, 3, th as i64, tw as i64], flat)).map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), tensor)])
        .map_err(AiError::Ort)?;
    if outputs.len() == 0 {
        return Err(AiError::Ocr("cls model returned no output".into()));
    }
    let (_shape, slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(AiError::Ort)?;
    // 假定导出图末层含 softmax(RapidOCR/PP-OCR 惯例),slice 即两类概率 [p(0°), p(180°)]。
    // 若 golden 全丢行/conf 异常,先查此假设——未归一 logits 会致 cls_thresh 系统性失准。
    let p0 = slice.first().copied().unwrap_or(0.0);
    let p1 = slice.get(1).copied().unwrap_or(0.0);
    drop(outputs);
    drop(guard);

    if cls_decision(p0, p1, profile.cls_thresh) {
        Ok(image::imageops::rotate180(&crop))
    } else {
        Ok(crop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_table() {
        // p1 胜且 ≥ 阈值 → 翻转。
        assert!(cls_decision(0.1, 0.95, 0.9));
        // p1 胜但 < 阈值 → 不翻转。
        assert!(!cls_decision(0.3, 0.7, 0.9));
        // p0 胜 → 不翻转。
        assert!(!cls_decision(0.99, 0.01, 0.9));
        // 边界:恰等于阈值 → 翻转。
        assert!(cls_decision(0.1, 0.9, 0.9));
    }
}
