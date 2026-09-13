// crates/scrollery-ai-core/src/ocr/mod.rs
//! 自研 OCR 管线(PP-OCRv5,det→cls→rec→CTC)。
//!
//! 编排:检测(DB)得文本行 quad → 逐框透视裁剪 → 方向分类(按需 180° 翻转)→ 识别 + CTC 解码。
//! 三模型 **EP 恒 CPU**(D-OCR-2:rec 是 SVTR/transformer 系,与 engine.rs 坑9 同型风险),
//! 三 SessionPool 各容量 1(交互单发,控内存)。字典在 init 时加载,契约自检在首次 rec 推理时
//! (rec 输出末维 C 对拍 dict.len()+1/+2,dict.rs 坑8 同型防线)。
//!
//! 参数全部取自 [`OcrProfile`];通道序单点开关 `swap_rb`(golden 对拍定案,见 dict/det 注释)。

mod cls;
mod det;
pub mod dict;
mod geometry;
mod rec;

use std::path::Path;

use image::{DynamicImage, RgbImage};

use crate::engine::{load_session_pool, SessionPool};
use crate::error::{AiError, Result};
use crate::ocr_profile::OcrProfile;
use crate::provider::AiProvider;

use dict::OcrDict;

/// 一条识别结果行(原图坐标系四点框)。
#[derive(Debug, Clone)]
pub struct OcrLineOut {
    pub text: String,
    /// 四点框 `[TL,TR,BR,BL]`,原图像素坐标。
    pub quad: [[f32; 2]; 4],
    pub confidence: f32,
}

/// 一张图的识别输出。`width/height` = 解码图实际尺寸(quad 坐标系)。
#[derive(Debug, Clone)]
pub struct OcrOutput {
    pub lines: Vec<OcrLineOut>,
    pub width: u32,
    pub height: u32,
}

/// OCR 引擎:三 session 池 + 字典 + profile。
pub struct OcrEngine {
    det: SessionPool,
    cls: SessionPool,
    rec: SessionPool,
    dict: OcrDict,
    pub profile: OcrProfile,
}

impl OcrEngine {
    /// 加载 det/cls/rec 三模型(CPU EP,各池容量 1)+ 字典。
    /// 任一模型缺失/加载失败 → `Err`(会话语义:声明即必须就绪)。
    pub fn init(models_dir: &Path, profile: &OcrProfile) -> Result<Self> {
        let load = |file: &str, label: &str, stage: &str| -> Result<SessionPool> {
            let path = models_dir.join(file);
            load_session_pool(&path, &AiProvider::Cpu, label, 1, stage, None)
                .ok_or_else(|| AiError::Ocr(format!("OCR {label} load failed: {path:?}")))
        };
        let det = load(&profile.det_file, "det", "ocr_det")?;
        let cls = load(&profile.cls_file, "cls", "ocr_cls")?;
        let rec = load(&profile.rec_file, "rec", "ocr_rec")?;
        let dict = OcrDict::load(&models_dir.join(&profile.dict_file))?;
        if dict.is_empty() {
            return Err(AiError::Ocr("OCR dict is empty".into()));
        }
        Ok(Self {
            det,
            cls,
            rec,
            dict,
            profile: profile.clone(),
        })
    }

    /// det→(逐框 crop)→cls→rec 全链;输出行按阅读序。
    pub fn recognize(&self, img: &DynamicImage) -> Result<OcrOutput> {
        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();
        // 0 尺寸护栏:det 前处理/张量构造对空图无意义,直接回空(避免下游几何除零)。
        if w == 0 || h == 0 {
            return Ok(OcrOutput {
                lines: Vec::new(),
                width: w,
                height: h,
            });
        }
        let quads = det::detect(&self.det, &rgb, &self.profile)?;

        let mut lines: Vec<OcrLineOut> = Vec::new();
        for quad in quads {
            let crop = geometry::get_rotate_crop_image(&rgb, &quad);
            let crop = cls::classify_and_maybe_flip(&self.cls, crop, &self.profile)?;
            let (text, confidence) =
                rec::recognize_text(&self.rec, &crop, &self.dict, &self.profile)?;
            if text.is_empty() || confidence < self.profile.rec_min_conf {
                continue;
            }
            lines.push(OcrLineOut {
                text,
                quad,
                confidence,
            });
            if lines.len() >= self.profile.max_lines_per_image {
                break;
            }
        }
        Ok(OcrOutput {
            lines,
            width: w,
            height: h,
        })
    }
}

/// 把 RGB 图转 CHW f32 张量:按 `swap_rb` 决定通道序(true=BGR),逐通道 `(px/255-mean)/std`
/// **与通道序同序应用**(mean[c] 施于 channel c,PaddleOCR 惯例)。det/cls/rec 共用。
pub(crate) fn chw_tensor(img: &RgbImage, swap_rb: bool, mean: [f32; 3], std: [f32; 3]) -> Vec<f32> {
    let (w, h) = img.dimensions();
    let (wu, hu) = (w as usize, h as usize);
    let mut flat = vec![0.0f32; 3 * wu * hu];
    let plane = hu * wu;
    for y in 0..hu {
        for x in 0..wu {
            let p = img.get_pixel(x as u32, y as u32).0;
            let ch = if swap_rb {
                [p[2], p[1], p[0]]
            } else {
                [p[0], p[1], p[2]]
            };
            for c in 0..3 {
                flat[c * plane + y * wu + x] = (ch[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
    }
    flat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chw_tensor_layout_and_swap() {
        let mut img = RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0])); // R
        img.put_pixel(1, 0, image::Rgb([0, 0, 255])); // B
                                                      // 无 swap,mean 0 std 1:channel0 = R 通道。
        let t = chw_tensor(&img, false, [0.0; 3], [1.0; 3]);
        // 布局 [C=3][H=1][W=2]:idx c*2 + x。
        assert!((t[0] - 1.0).abs() < 1e-6); // ch0(R), x0 = 255/255
        assert!((t[1] - 0.0).abs() < 1e-6); // ch0(R), x1 = 0
                                            // swap_rb=true:channel0 取 B 分量。
        let ts = chw_tensor(&img, true, [0.0; 3], [1.0; 3]);
        assert!((ts[0] - 0.0).abs() < 1e-6); // ch0=B of pixel0(B=0)
        assert!((ts[1] - 1.0).abs() < 1e-6); // ch0=B of pixel1(B=255)
    }
}
