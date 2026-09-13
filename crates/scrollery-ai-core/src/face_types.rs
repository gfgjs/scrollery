// crates/scrollery-ai-core/src/face_types.rs
//! 人脸检测的纯几何类型(T16 准备:自 face.rs 外移)。
//!
//! 本模块**永远可用**(不在 `inference` feature 门内):host 在 T16 删 ort 后,
//! worker 派发路径仍需 `DetectedFace`(协议 FaceDet 搬运回本类型走共用落库映射
//! `faces_to_records`,quality 派生同源)。路径兼容:face.rs 以 `pub use` 原位再导出。

/// 一张检测到的人脸，坐标均在**输入 `DecodedImage` 的像素坐标系**。
#[derive(Clone, Debug)]
pub struct DetectedFace {
    /// 框 [x, y, w, h]（左上角 + 宽高，像素）。
    pub bbox: [f32; 4],
    /// 5 关键点（左眼/右眼/鼻/左嘴角/右嘴角），像素坐标。
    pub landmarks: [[f32; 2]; 5],
    /// 检测置信度 `√(cls·obj)`。
    pub score: f32,
}

impl DetectedFace {
    /// 综合质量分（F2 粗版：score × 框短边占比惩罚小脸）；F4 聚类时再精化（清晰度/正脸）。
    pub fn quality(&self, img_w: u32, img_h: u32) -> f32 {
        let short = self.bbox[2].min(self.bbox[3]);
        let base = img_w.min(img_h).max(1) as f32;
        let size_factor = (short / base * 4.0).min(1.0); // 占短边 ≥25% 即满分
        self.score * size_factor
    }
}
