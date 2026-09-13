// crates/scrollery-ai-core/src/ocr_profile.rs
//! OCR 模型契约（`OcrProfile`）+ 内置注册表（纯数据，零 ort）。
//!
//! 仿 [`crate::face_profile`] 之法：把「换 OCR 模型档位」变成 DATA 而非 CODE——
//! 文件名、几何、阈值、通道序、字典全部来自 profile。
//!
//! # 单源约定
//! **最终文件名以 `docs/planning/2026-07-23-OCR文字提取/model-assets.md` 为单源**：
//! host 描述符、worker 校验、下载清单全部从这里取名，改名只动本文件单点。
//!
//! # 档位（PP-OCRv5）
//! - `pp-ocrv5-mobile`：轻量，det/cls/rec 合计约 30MB，交互默认。
//! - `pp-ocrv5-server`：高精度，约 190MB。
//!
//! # cls 不共用（主线复核修正）
//! PP-OCRv5 两档各带**专属** textline 方向分类模型（PP-LCNet_x0_25 / x1_0），
//! 已非旧版 `ch_ppocr_mobile_v2.0_cls`；`cls_file` 两档互异，仅 `dict_file` 共用。

use serde::{Deserialize, Serialize};

/// 完整 OCR 模型契约：det→cls→rec 三段所需几何/阈值 + 文件名/元数据。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrProfile {
    /// 稳定 id：`"pp-ocrv5-mobile"` | `"pp-ocrv5-server"`。
    pub id: String,
    pub display_name: String,
    pub description: String,

    // ── 文件名（相对 models 目录；单源见模块头）────────────────────────────────
    pub det_file: String,
    pub cls_file: String,
    pub rec_file: String,
    /// 字典文件；两档共用（内容按上游惯例同一份 PP-OCRv5 通用字典）。
    pub dict_file: String,

    // ── 检测（DB 后处理）─────────────────────────────────────────────────────
    /// 检测输入长边上限（mobile 960 / server 1280，D-OCR-7 的大图精度一期答案）。
    pub det_limit_side_len: u32,
    /// 概率图二值化阈值。
    pub det_thresh: f32,
    /// 框保留分数阈值（框内概率均值低于此丢弃）。
    pub det_box_thresh: f32,
    /// unclip 外扩比例。
    pub det_unclip_ratio: f32,
    /// 最小框短边（**原图坐标像素**）。
    pub det_min_box_side: f32,

    // ── 方向分类 ──────────────────────────────────────────────────────────────
    /// 判为 180° 倒置的概率阈值。
    pub cls_thresh: f32,
    /// cls 输入 `(H, W)`。
    /// **2026-07-23 真机对拍定案**:原按旧版 `ch_ppocr_mobile_v2.0_cls` 的
    /// `cls_image_shape = [3, 48, 192]` 惯例猜值,真机推理时 ONNX Runtime 报维度不符
    /// (`Got: 48 Expected: 80` / `Got: 192 Expected: 160`)——PP-OCRv5 的 `textline_ori`
    /// 分类器（新架构,非旧版整图方向分类器）固定输入实为 `[3, 80, 160]`,已按此回填。
    /// mobile/server 差异在网络宽度(x0_25 vs x1_0)非输入尺寸,故两档同值（server 档
    /// 尚未真机验证,理论同架构应同值,若日后实测不符按对拍值单点回写）。
    pub cls_input_hw: (u32, u32),

    // ── 识别 + CTC ────────────────────────────────────────────────────────────
    /// 识别输入高度（等比缩放到此高）。
    pub rec_input_h: u32,
    /// 识别输入宽度安全帽（超宽长条截断，防显存/内存爆）。
    pub rec_max_w: u32,
    /// 行置信度下限（低于即丢行）。
    pub rec_min_conf: f32,
    /// 单图最大行数（防密集文字页撑爆）。
    pub max_lines_per_image: usize,

    /// 喂模型前是否 R/B 互换(PaddleOCR DecodeImage img_mode=BGR 惯例;det/cls/rec 统一)。
    /// 默认 true;golden 对拍测试若证明 RGB 才对,单点改此处(见 construction-plan 边界1)。
    pub swap_rb: bool,

    /// 体积提示（MB，UI 展示）。
    pub size_mb: u32,
}

/// 默认 OCR profile id（交互默认档）。
pub const DEFAULT_OCR_PROFILE_ID: &str = "pp-ocrv5-mobile";

/// 字典文件名（两档共用；单源见模块头）。
const DICT_FILE: &str = "ppocrv5_dict.txt";

/// 所有已知 OCR 档位（第一条 = 默认）。cls_file 两档互异，dict_file 共用。
pub fn ocr_profiles() -> Vec<OcrProfile> {
    vec![
        OcrProfile {
            id: DEFAULT_OCR_PROFILE_ID.to_string(), // "pp-ocrv5-mobile"
            display_name: "PP-OCRv5 标准".to_string(),
            description: "PP-OCRv5 mobile · 轻量约 30MB · 中英混排印刷体，交互默认档。".to_string(),
            det_file: "ch_PP-OCRv5_det_mobile.onnx".to_string(),
            cls_file: "ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx".to_string(),
            rec_file: "ch_PP-OCRv5_rec_mobile.onnx".to_string(),
            dict_file: DICT_FILE.to_string(),
            det_limit_side_len: 960,
            det_thresh: 0.3,
            det_box_thresh: 0.6,
            det_unclip_ratio: 1.5,
            det_min_box_side: 3.0,
            cls_thresh: 0.9,
            cls_input_hw: (80, 160),
            rec_input_h: 48,
            rec_max_w: 3200,
            rec_min_conf: 0.5,
            max_lines_per_image: 1000,
            swap_rb: true,
            size_mb: 30,
        },
        OcrProfile {
            id: "pp-ocrv5-server".to_string(),
            display_name: "PP-OCRv5 高精度".to_string(),
            description: "PP-OCRv5 server · 约 190MB · 精度更高、更慢，可选高精度档。".to_string(),
            det_file: "ch_PP-OCRv5_det_server.onnx".to_string(),
            cls_file: "ch_PP-LCNet_x1_0_textline_ori_cls_server.onnx".to_string(),
            rec_file: "ch_PP-OCRv5_rec_server.onnx".to_string(),
            dict_file: DICT_FILE.to_string(),
            det_limit_side_len: 1280,
            det_thresh: 0.3,
            det_box_thresh: 0.6,
            det_unclip_ratio: 1.5,
            det_min_box_side: 3.0,
            cls_thresh: 0.9,
            cls_input_hw: (80, 160),
            rec_input_h: 48,
            rec_max_w: 3200,
            rec_min_conf: 0.5,
            max_lines_per_image: 1000,
            swap_rb: true,
            size_mb: 190,
        },
    ]
}

/// 按稳定 id 查 OCR profile。
pub fn find_ocr_profile(id: &str) -> Option<OcrProfile> {
    ocr_profiles().into_iter().find(|p| p.id == id)
}

/// 默认 OCR profile（始终存在）。
pub fn default_ocr_profile() -> OcrProfile {
    find_ocr_profile(DEFAULT_OCR_PROFILE_ID)
        .expect("default ocr profile must exist | 默认 OCR profile 必须存在")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tiers_present_and_default_resolves() {
        let ps = ocr_profiles();
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].id, DEFAULT_OCR_PROFILE_ID);
        assert!(find_ocr_profile("pp-ocrv5-server").is_some());
        assert!(find_ocr_profile("nope").is_none());
        // default_ocr_profile 不 panic。
        assert_eq!(default_ocr_profile().id, DEFAULT_OCR_PROFILE_ID);
    }

    #[test]
    fn det_cls_rec_file_names_differ_dict_shared() {
        let ps = ocr_profiles();
        let (m, s) = (&ps[0], &ps[1]);
        // det/cls/rec 三件两档互异。
        assert_ne!(m.det_file, s.det_file);
        assert_ne!(m.cls_file, s.cls_file, "cls 两档专属,不共用(主线修正)");
        assert_ne!(m.rec_file, s.rec_file);
        // dict 共用。
        assert_eq!(m.dict_file, s.dict_file);
        // 单档内四文件名互异(避免同名 dest 覆盖)。
        assert_ne!(m.det_file, m.cls_file);
        assert_ne!(m.det_file, m.rec_file);
        assert_ne!(m.cls_file, m.rec_file);
    }
}
