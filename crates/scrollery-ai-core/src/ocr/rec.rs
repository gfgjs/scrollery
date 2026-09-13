// crates/scrollery-ai-core/src/ocr/rec.rs
//! 文本识别 + CTC 贪心解码。
//!
//! 每 crop `w' = min(rec_max_w, ceil(w·H/h))` 等比缩至 `H×w'`(H=rec_input_h),
//! `(px/255-0.5)/0.5` CHW 张量,逐张跑(一期不组批);输出 `[1,T,C]`。
//! CTC 贪心:逐 t argmax(idx,prob),折叠连续重复、丢 blank(idx 0),字符 = `dict.char(idx)`;
//! conf = 保留字符 prob 均值。文本空或 conf 低由上层(mod::recognize)按 `rec_min_conf` 丢行。

use image::RgbImage;
use ort::value::Tensor;

use super::chw_tensor;
use super::dict::OcrDict;
use crate::engine::SessionPool;
use crate::error::{AiError, Result};
use crate::ocr_profile::OcrProfile;

/// CTC 贪心解码:`logits` 为 `[T, C]` 行主序(T 个时间步各 C 类)。
/// 返回 `(text, conf)`;`class_check` 内含 rec/dict 契约自检(坑8 同型防线)。
pub(crate) fn ctc_greedy_decode(
    logits: &[f32],
    t: usize,
    c: usize,
    dict: &OcrDict,
) -> Result<(String, f32)> {
    let use_space = dict.class_check(c)?;
    // conf 取每步 argmax 的原始输出值,假定导出图末层含 softmax(RapidOCR/PP-OCR 惯例),
    // 该值即概率。若 golden 全丢行/conf 异常,先查此假设——未归一 logits 会致 rec_min_conf
    // 系统性失准(阈值按概率标定)。
    let mut text = String::new();
    let mut confs: Vec<f32> = Vec::new();
    let mut prev = usize::MAX;
    for ti in 0..t {
        let base = ti * c;
        let mut best = 0usize;
        let mut best_v = f32::MIN;
        for ci in 0..c {
            let v = logits[base + ci];
            if v > best_v {
                best_v = v;
                best = ci;
            }
        }
        // 折叠连续重复(prev 记录上一步 argmax,含 blank);非 blank 且非重复才发射。
        if best != 0 && best != prev {
            if let Some(ch) = dict.char(best, use_space) {
                text.push_str(ch);
                confs.push(best_v);
            }
        }
        prev = best;
    }
    let conf = if confs.is_empty() {
        0.0
    } else {
        confs.iter().sum::<f32>() / confs.len() as f32
    };
    Ok((text, conf))
}

/// 识别单个 crop → `(text, conf)`。
pub(crate) fn recognize_text(
    pool: &SessionPool,
    crop: &RgbImage,
    dict: &OcrDict,
    profile: &OcrProfile,
) -> Result<(String, f32)> {
    let (w, h) = crop.dimensions();
    if w == 0 || h == 0 {
        return Ok((String::new(), 0.0));
    }
    let th = profile.rec_input_h;
    let tw = (((w as f32) * (th as f32) / (h as f32)).ceil() as u32)
        .max(1)
        .min(profile.rec_max_w);
    let resized = image::imageops::resize(crop, tw, th, image::imageops::FilterType::Triangle);
    let flat = chw_tensor(&resized, profile.swap_rb, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]);

    let mut guard = pool
        .get()
        .ok_or_else(|| AiError::Ocr("OCR rec pool disconnected".into()))?;
    let input_name = guard
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| AiError::Ocr("OCR rec has no input".into()))?;
    let tensor =
        Tensor::from_array(([1i64, 3, th as i64, tw as i64], flat)).map_err(AiError::Ort)?;
    let outputs = guard
        .run(vec![(input_name.as_str(), tensor)])
        .map_err(AiError::Ort)?;
    if outputs.len() == 0 {
        return Err(AiError::Ocr("rec model returned no output".into()));
    }
    let (shape, slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(AiError::Ort)?;
    // 形状 [1, T, C]。
    let dims = shape.len();
    if dims < 2 {
        return Err(AiError::Ocr(format!("rec output rank {dims} < 2")));
    }
    let t = shape[dims - 2] as usize;
    let c = shape[dims - 1] as usize;
    let logits = slice.to_vec();
    drop(outputs);
    drop(guard);

    ctc_greedy_decode(&logits, t, c, dict)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造 [T, C] logits:让指定时间步某类胜出。C = N+1(无 space)。
    fn make_logits(t: usize, c: usize, argmax_per_t: &[(usize, f32)]) -> Vec<f32> {
        let mut v = vec![0.0f32; t * c];
        for (ti, &(cls, prob)) in argmax_per_t.iter().enumerate() {
            v[ti * c + cls] = prob;
        }
        v
    }

    #[test]
    fn collapse_dedup_and_blank() {
        let dict = OcrDict::from_str_contents("a\nb\nc"); // N=3 → idx a=1,b=2,c=3
        let c = 4; // N+1
                   // 时序: a a blank a → 折叠为 "aa"(blank 分隔重复)。
        let logits = make_logits(4, c, &[(1, 0.9), (1, 0.8), (0, 0.7), (1, 0.6)]);
        let (text, conf) = ctc_greedy_decode(&logits, 4, c, &dict).unwrap();
        assert_eq!(text, "aa");
        // conf = (0.9 + 0.6)/2。
        assert!((conf - 0.75).abs() < 1e-5, "conf={conf}");
    }

    #[test]
    fn all_blank_drops_to_empty() {
        let dict = OcrDict::from_str_contents("a\nb\nc");
        let c = 4;
        let logits = make_logits(3, c, &[(0, 0.9), (0, 0.8), (0, 0.7)]);
        let (text, conf) = ctc_greedy_decode(&logits, 3, c, &dict).unwrap();
        assert!(text.is_empty());
        assert_eq!(conf, 0.0);
    }

    #[test]
    fn sequential_chars() {
        let dict = OcrDict::from_str_contents("a\nb\nc");
        let c = 4;
        // a b c → "abc"。
        let logits = make_logits(3, c, &[(1, 0.5), (2, 0.6), (3, 0.7)]);
        let (text, _) = ctc_greedy_decode(&logits, 3, c, &dict).unwrap();
        assert_eq!(text, "abc");
    }

    #[test]
    fn class_mismatch_errors() {
        let dict = OcrDict::from_str_contents("a\nb\nc"); // N=3 → 合法 C=4/5
        let logits = vec![0.0f32; 3 * 3];
        assert!(ctc_greedy_decode(&logits, 3, 3, &dict).is_err());
    }
}
