// crates/scrollery-ai-core/src/face_profile.rs
//! 人脸模型契约（`FaceProfile`）+ 内置注册表。
//! 仿 [`crate::profile`]（CLIP）之法，把人脸推理路径与具体模型解耦：检测器/嵌入器种类、
//! 文件名、几何尺寸、嵌入维度、对齐模板、归一化、同人阈值全部来自 profile 而非写死常量。
//!
//! # 双轨与商用合规（关键）
//! - **默认轨 `yunet-sface`**：YuNet(MIT) + SFace(Apache-2.0)，`commercial_ok=true`，商业发行默认。
//! - **可选轨 `scrfd-arcface-*`**：InsightFace 系，**权重非商用** → `commercial_ok=false`，
//!   仅测试渠道 / 用户手动导入时显现（商业渠道按 `commercial_ok` 过滤，同 CLIP profile）。
//!
//! # 不变量
//! - `id` **同时是 `faces.model_name` 主键**：换 id = 换一套向量空间，不同模型向量不可互比，
//!   切换后须对该空间缺失向量的项重新检测+嵌入（同 `ai_embeddings.model_name` 之于 CLIP）。
//!
//! # 阶段
//! - F1（本文件）：只立**契约 + 注册表**（数据），供引擎按文件名加载 session 插槽。
//! - F2：`face.rs` 按 `DetectorKind`/`EmbedderKind` 分派检测后处理（YuNet 格式 vs SCRFD anchor）
//!   与嵌入预处理（对齐模板 + 归一化）。新增枚举变体会让分派 `match` 非穷尽编译报错，强制补全。
//! - F7+：默认轨 `assets` 已填已校验直链（可一键下载）；SCRFD 轨 `assets` 仍留空（无校验值 +
//!   非商用 = 仅手动导入）。

use serde::{Deserialize, Serialize};

use crate::profile::ModelAsset;

/// 人脸检测器种类（决定 onnx 输出的解码后处理）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorKind {
    /// YuNet（OpenCV Zoo，MIT）：输出 bbox + 5 关键点 + score，priors/strides 解码 + NMS。
    YuNet,
    /// SCRFD（InsightFace，权重非商用）：多 stride(8/16/32) anchor，distance2bbox/kps + NMS。
    /// Part4-T1:仅 face-noncommercial build 编译——变体的 serde 字面量("scrfd")也不进商业二进制。
    #[cfg(feature = "face-noncommercial")]
    Scrfd,
}

/// 人脸嵌入器种类（决定对齐模板 + 输入归一化 + 输出维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedderKind {
    /// SFace（OpenCV Zoo，Apache-2.0）：128 维。
    SFace,
    /// ArcFace（InsightFace，权重非商用）：512 维，`(x-127.5)/127.5` 归一化。
    /// Part4-T1:仅 face-noncommercial build 编译(同 DetectorKind::Scrfd)。
    #[cfg(feature = "face-noncommercial")]
    ArcFace,
}

/// 嵌入器输入归一化方式（对齐后 112×112 图 → 张量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceNorm {
    /// `(x-127.5)/127.5`，RGB（ArcFace 标准）。
    /// Part4-T1:仅 face-noncommercial build 编译(唯一消费者是 ArcFace 轨)。
    #[cfg(feature = "face-noncommercial")]
    ArcFaceStd,
    /// SFace：RGB、0-255、无归一化（F8 对拍坐实：feature() 内部 swapRB=true，喂 RGB→cosine 1.0）。
    SFace,
}

/// 完整人脸模型契约：检测 + 嵌入两段所需的一切 + 目录/许可元数据。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceProfile {
    /// 稳定 id；**也是 `faces.model_name` 主键**（向量空间身份）。
    pub id: String,
    pub display_name: String,
    pub description: String,

    // ── 检测器 ────────────────────────────────────────────────────────────────
    pub detector: DetectorKind,
    /// 检测 onnx 落地文件名（相对 models 目录）。
    pub detect_file: String,
    /// 检测输入边长（YuNet 动态，相册大图找小脸宜 640；SCRFD 640）。
    pub detect_size: u32,
    /// 检测置信度阈值：低于此的候选直接丢弃。YuNet 取 OpenCV 默认 0.9（0.6 太低 → 手机/猫耳误检蓝框）；
    /// SCRFD 沿用 InsightFace 默认 0.5。NMS 前生效。
    /// ⚠️ **未经本项目实测标定**——是上游默认值,漏检/误检率未在本库照片分布上验证(开发期可改)。
    pub det_score_thresh: f32,
    /// 最小脸过滤：框短边（原图坐标系，像素）小于此值的检测丢弃，剔除远景噪点小框。
    pub min_face_px: u32,

    // ── 嵌入器 ────────────────────────────────────────────────────────────────
    pub embedder: EmbedderKind,
    /// 嵌入 onnx 落地文件名（相对 models 目录）。
    pub embed_file: String,
    /// 嵌入输入边长（SFace/ArcFace 皆 112）。
    pub embed_size: u32,
    /// 嵌入维度（SFace=128 / ArcFace=512）；= `faces.embedding` 的 f32 元素数。
    pub embed_dim: usize,
    pub embed_norm: FaceNorm,

    // ── 对齐 / 阈值 ───────────────────────────────────────────────────────────
    /// 5 点相似变换目标模板（112×112 坐标系，顺序：左眼/右眼/鼻/左嘴角/右嘴角）。
    pub align_template: [[f32; 2]; 5],
    /// 同人 cosine 阈值（SFace≈0.363 / ArcFace≈0.40）；聚类按此建边。
    /// ⚠️ **未经本项目百万级增量聚类实测标定**——搬自上游(OpenCV/InsightFace),非按本库验证。
    /// 运行期可经 `app_config` 键 `face_same_threshold` override(见 `face_cluster::effective_thresholds`),
    /// 便于不重编译做实测比较;改它影响已聚类结果,需重聚。
    pub same_face_threshold: f32,
    /// 参与聚类的质量下限（低于此的脸仍存库、详情可见，但不参与聚类以减噪）。
    /// ⚠️ 当前两轨默认 **0.0(即不过滤)**,未验证——模糊/侧脸全参与聚类可能污染质心。
    /// 运行期可经 `app_config` 键 `face_min_quality` override(见 `face_cluster::effective_thresholds`)。
    pub min_quality: f32,

    // ── 目录 / 许可 / 下载 ────────────────────────────────────────────────────
    pub license: String,
    /// 是否允许商业使用；商业发行版按构建渠道过滤掉 `false` 的条目（同 CLIP profile）。
    pub commercial_ok: bool,
    /// 推理输出已与上游参考实现对拍验证(同图同输出)。false = UNVERIFIED——激活即可能
    /// 静默算错,`set_active_face_model` 拒绝激活未对拍轨(Part4 §3.5.2 前置门)。
    pub verified: bool,
    /// 体积提示（MB，UI 展示；0=未知）。
    pub size_mb: u32,
    /// 下载资产；**空 = 仅支持手动导入**（F7 由模型库填充已校验直链）。
    pub assets: Vec<ModelAsset>,
}

/// 默认（商用友好）人脸 profile id —— 其 `faces.model_name` 向量须保持有效。
pub const DEFAULT_FACE_PROFILE_ID: &str = "yunet-sface";

/// InsightFace ArcFace 标准 5 点对齐模板（112×112，`arcface_dst`）。
/// 顺序：左眼、右眼、鼻尖、左嘴角、右嘴角。SFace 暂复用之，F2 对拍 OpenCV 后坐实。
const ARCFACE_DST: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// 所有已知人脸模型（第一条 = 默认）。
pub fn face_profiles() -> Vec<FaceProfile> {
    // mut 仅在 face-noncommercial 下被 push 用到;默认 build 精准豁免 unused_mut。
    #[cfg_attr(not(feature = "face-noncommercial"), allow(unused_mut))]
    let mut profiles = vec![
        // ── 默认轨：商用友好（YuNet MIT + SFace Apache-2.0）──────────────────────
        FaceProfile {
            id: DEFAULT_FACE_PROFILE_ID.to_string(), // "yunet-sface"
            display_name: "YuNet + SFace (商用)".to_string(),
            description:
                "MIT/Apache-2.0 · 商业可用 · 轻量。检测 YuNet + 识别 SFace(128维)。精度逊于 ArcFace，\
                 但许可干净，为商业发行默认。"
                    .to_string(),
            detector: DetectorKind::YuNet,
            detect_file: "face_detection_yunet_2023mar.onnx".to_string(),
            detect_size: 640,
            det_score_thresh: 0.9, // OpenCV YuNet 默认；0.6 太低导致误检
            min_face_px: 24,
            embedder: EmbedderKind::SFace,
            embed_file: "face_recognition_sface_2021dec.onnx".to_string(),
            embed_size: 112,
            embed_dim: 128,
            embed_norm: FaceNorm::SFace,
            align_template: ARCFACE_DST,
            same_face_threshold: 0.363, // OpenCV FaceRecognizerSF 标定（cosine）
            min_quality: 0.0,
            license: "MIT / Apache-2.0".to_string(),
            commercial_ok: true,
            verified: true, // F8 对拍坐实(SFace swapRB 语义,与 OpenCV 参考 cosine 1.0)
            size_mb: 38,
            // 已校验直链（opencv_zoo raw；文件名带日期版本，上游不原地改）。size/sha256 实算自盘内文件，
            // download_file 的 size+sha256 校验是安全网：URL 错或 LFS 返回 pointer 文本即报错，绝不落坏文件。
            assets: vec![
                ModelAsset {
                    url: "https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx".to_string(),
                    mirror_url: None,
                    dest: "face_detection_yunet_2023mar.onnx".to_string(),
                    size_bytes: 232_589,
                    sha256: Some(
                        "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4".to_string(),
                    ),
                },
                ModelAsset {
                    url: "https://github.com/opencv/opencv_zoo/raw/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx".to_string(),
                    mirror_url: None,
                    dest: "face_recognition_sface_2021dec.onnx".to_string(),
                    size_bytes: 38_696_353,
                    sha256: Some(
                        "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79".to_string(),
                    ),
                },
            ],
        },
    ];
    // ── 可选轨：高精度（InsightFace，权重非商用;Part4-T1 cfg 物理隔离）──────────────
    // commercial_ok=false → 商业渠道过滤;#[cfg] 进一步使默认/商业 build **物理不编译**
    // 本轨——profile、onnx 文件名常量、"scrfd-arcface-r50" 字面量都不进二进制(§3.10.1,
    // 不能只靠运行时不可达)。仅研究/自用 build(--features face-noncommercial)显现。
    #[cfg(feature = "face-noncommercial")]
    profiles.push(FaceProfile {
        id: "scrfd-arcface-r50".to_string(),
        display_name: "SCRFD + ArcFace R50 (非商用)".to_string(),
        description:
            "InsightFace · 高精度 · **权重仅限非商业研究**。检测 SCRFD + 识别 ArcFace w600k_r50(512维)。\
             测试或个人用途可选；商业发行禁用。"
                .to_string(),
        detector: DetectorKind::Scrfd,
        detect_file: "scrfd_10g_bnkps.onnx".to_string(),
        detect_size: 640,
        det_score_thresh: 0.5, // InsightFace SCRFD 默认
        min_face_px: 24,
        embedder: EmbedderKind::ArcFace,
        embed_file: "w600k_r50.onnx".to_string(),
        embed_size: 112,
        embed_dim: 512,
        embed_norm: FaceNorm::ArcFaceStd,
        align_template: ARCFACE_DST,
        same_face_threshold: 0.40,
        min_quality: 0.0,
        license: "InsightFace (non-commercial)".to_string(),
        commercial_ok: false,
        verified: false, // detect_scrfd UNVERIFIED:从未与 InsightFace 参考实现对拍
        size_mb: 174,
        assets: Vec::new(),
    });
    profiles
}

/// 按稳定 id 查人脸 profile。
pub fn find_face_profile(id: &str) -> Option<FaceProfile> {
    face_profiles().into_iter().find(|p| p.id == id)
}

/// 默认人脸 profile（始终存在）。
pub fn default_face_profile() -> FaceProfile {
    find_face_profile(DEFAULT_FACE_PROFILE_ID)
        .expect("default face profile must exist | 默认人脸 profile 必须存在")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Part4-T1 合规断言:默认(=商业同形)build 的注册表**物理不含**非商用轨。
    /// 这是「不能只靠运行时不可达」的编译期证据,CI 默认矩阵天然在跑;
    /// 商业流水线(Part7 T16/17)的二进制符号扫描是第二道兜底。
    #[cfg(not(feature = "face-noncommercial"))]
    #[test]
    fn noncommercial_track_absent_by_default() {
        assert!(find_face_profile("scrfd-arcface-r50").is_none());
        assert_eq!(face_profiles().len(), 1, "默认注册表只应有商用轨");
        assert_eq!(face_profiles()[0].id, DEFAULT_FACE_PROFILE_ID);
    }

    /// 研究/自用 build(--features face-noncommercial):非商用轨显现且合规标记齐全。
    #[cfg(feature = "face-noncommercial")]
    #[test]
    fn noncommercial_track_present_with_feature() {
        let p = find_face_profile("scrfd-arcface-r50").expect("feature 开启时应存在");
        assert!(!p.commercial_ok, "必须保持非商用标记");
        assert!(!p.verified, "对拍坐实前必须保持 UNVERIFIED");
    }
}
