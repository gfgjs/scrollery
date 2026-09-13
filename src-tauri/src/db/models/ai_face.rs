//! AI(CLIP)/ 人脸识别域模型。

use serde::{Deserialize, Serialize};

// ── AI ───────────────────────────────────────────────────────────────────────

/// 存储在 `media_items.ai_status` 中的 AI 处理状态码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum AiStatus {
    /// 尚未分析
    Pending = 0,
    /// 当前正在处理
    Processing = 1,
    /// 嵌入向量已存储
    Done = 2,
    /// 分析失败（图像不可读等）
    Error = 3,
}

impl AiStatus {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(v: i64) -> Self {
        match v {
            0 => AiStatus::Pending,
            1 => AiStatus::Processing,
            2 => AiStatus::Done,
            3 => AiStatus::Error,
            _ => AiStatus::Error,
        }
    }
}

/// 存储在 `media_items.face_status` 中的人脸检测状态码（语义同 `AiStatus`，独立开关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum FaceStatus {
    /// 尚未检测
    Pending = 0,
    /// 当前正在处理
    Processing = 1,
    /// 已检测并嵌入（含零脸图）
    Done = 2,
    /// 检测失败（图像不可读等）
    Error = 3,
}

impl FaceStatus {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(v: i64) -> Self {
        match v {
            0 => FaceStatus::Pending,
            1 => FaceStatus::Processing,
            2 => FaceStatus::Done,
            _ => FaceStatus::Error,
        }
    }
}

/// 单条存储的 CLIP 嵌入向量行。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEmbedding {
    pub item_id: i64,
    pub model_name: String,
    /// 原始 f32 字节（ViT-B/16 为 512 × 4 = 2048 字节）。
    #[serde(skip)]
    pub embedding: Vec<u8>,
    pub version: i64,
    pub created_at: i64,
}

/// 带相似度分数的语义搜索结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchResult {
    pub id: i64,
    pub file_name: String,
    pub media_type: String,
    pub width: i64,
    pub height: i64,
    pub thumb_path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
    pub thumb_status: i64,
    /// [0, 1] 范围内的余弦相似度。
    pub similarity: f32,
}

/// 返回给前端的派生流水线状态摘要（视频封面/关键帧、文档缩略图、音频封面/元数据）。
/// 与 AI 三按钮状态面板同构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivationStatusSummary {
    pub pending: i64,
    pub processing: i64,
    pub done: i64,
    pub error: i64,
    /// 流水线正在运行时为真（令牌存在）。
    pub is_running: bool,
    /// 跨运行/重启持久化的「期望运行」标志（驱动续传与自动续传），与 `ai_analysis_active` 同构。
    pub active: bool,
}

/// 返回给前端的 AI 状态摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusSummary {
    pub provider: String,
    pub gpu_name: String,
    pub vram_gb: Option<i64>,
    pub batch_size: i64,
    /// 当前图像变体的固定 batch `k`（>1），动态/单批为 None。驱动设置页「batch 不得 < k」最小限制。
    pub active_fixed_batch: Option<i64>,
    pub clip_loaded: bool,
    pub total_items: i64,
    pub analyzed_items: i64,
    pub pending_items: i64,
    /// `ai_status=3`(Error)项数(2026-07-10 审查 A11):失败面此前对 UI 完全不可见——
    /// Error 项被计入 pending,进度永远到不了 100% 且无解释。现 pending 不含 error。
    pub error_items: i64,
    pub is_analyzing: bool,
    /// 分析处于「期望运行」状态——正在运行，或已暂停/中断且仍有剩余（驱动续传/三按钮 UI
    /// 与启动自动续传 —— 问题7）。
    pub analysis_active: bool,
    /// 本次轮询时的具体让步阻塞源（`ai_yield_blockers()`），仅当 `is_analyzing` 时填充——
    /// 驱动前端「等待…」提示，取代此前静默不可见的让步（可观测性三修 #2）。
    pub waiting_on: Vec<String>,
}

/// 返回给前端的人脸识别状态摘要（F5）。仿 `AiStatusSummary`，但报告人物/人脸数而非嵌入向量数，
/// 且 `processed_items` 统计 完成+错误（`face_status IN (2,3)`），使部分失败时进度条仍能到 100%。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceStatusSummary {
    pub provider: String,
    pub gpu_name: String,
    /// 人脸双 session（检测器 + 嵌入器）均已加载——对应 `clip_loaded`。
    pub face_loaded: bool,
    pub total_items: i64,
    /// 完成人脸检测的图像数（完成 或 错误），非"有脸的图像数"。
    pub processed_items: i64,
    pub pending_items: i64,
    /// 已聚类人物数（人物墙名册规模）。
    pub person_count: i64,
    /// 当前模型下跨所有图像检测到的人脸总数。
    pub face_count: i64,
    /// `face_status=3`(Error)项数(2026-07-10 审查 A11/F9):face 进度刻意含失败
    ///(能到 100%),反而掩盖失败存在——此计数让失败面可见并驱动「重试失败项」入口。
    pub error_items: i64,
    pub is_analyzing: bool,
    pub analysis_active: bool,
    /// 本次轮询时的具体让步阻塞源（可观测性三修 #2）（`ai_yield_blockers()`），仅当 `is_analyzing` 时填充——对应 `AiStatusSummary::waiting_on`。
    pub waiting_on: Vec<String>,
}

/// 一个人物簇作为人物墙卡片（F6）：身份 + 一张用于裁剪人脸的封面缩略图。`cover_thumb_path`/
/// `cover_thumb_status` 沿用 `SearchResult` 的约定（status=3 → path 为原图绝对路径；否则为分档
/// 缓存相对路径）；前端从该缩略图裁出 `cover_bbox`（归一化 [x,y,w,h]）作头像。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonSummary {
    pub id: i64,
    pub name: Option<String>,
    pub face_count: i64,
    pub is_named: bool,
    pub is_hidden: bool,
    /// 封面脸几何 + 其源图缩略图（封面悬空时为 None）。
    pub cover_item_id: Option<i64>,
    pub cover_thumb_path: Option<String>,
    pub cover_thumb_status: Option<i64>,
    pub cover_bbox: Option<[f32; 4]>,
}

/// 只读人脸模型库的一条模型轨（F7）：两条内置轨（yunet-sface 商用 / scrfd-arcface 非商用）+ 磁盘
/// 安装状态。无下载 URL——assets 待填已校验直链 + 人工确认许可，故仅供展示（UI 显示"已装 / 请放
/// 文件到此"）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceModelInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub detector: String,
    pub embedder: String,
    pub embed_dim: i64,
    pub commercial_ok: bool,
    pub license: String,
    pub size_mb: i64,
    /// 两个 onnx 文件均在磁盘上。
    pub installed: bool,
    /// 这是当前激活轨（`face_model_active`）。
    pub active: bool,
    /// 有已校验下载清单（可一键下载）。false=仅手动导入（SCRFD/ArcFace 轨：无校验值 + 非商用）。
    pub downloadable: bool,
    /// 推理输出已与上游参考实现对拍。false = 未对拍——`set_active_face_model` 拒绝激活
    ///(防静默算错门,Part4 §3.5.2)。
    pub verified: bool,
}

/// 叠加在图片详情查看器上的一张检测人脸（F6）：框 + 它归属的人物。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceBox {
    pub id: i64,
    pub person_id: Option<i64>,
    pub person_name: Option<String>,
    /// 相对图像自身尺寸归一化的 [x, y, w, h]（[0,1]）。
    pub bbox: [f32; 4],
    pub det_score: f32,
}

/// likely-match 组里的一张未确认脸（Part4 §3.5.1 / Part5 T10 批量审批）：用于裁剪人脸的缩略图 +
/// 它与候选人物的匹配强度。`thumb_path`/`thumb_status` 约定同 `PersonSummary`/`SearchResult`
///（status=3 → 经 JOIN 解析绝对源路径，否则用 thumb_path）；`bbox` 裁出人脸。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceThumb {
    pub face_id: i64,
    pub item_id: i64,
    pub thumb_path: Option<String>,
    pub thumb_status: Option<i64>,
    pub bbox: [f32; 4],
    /// 此脸与其候选人物质心的余弦相似度（单脸匹配强度）。
    pub similarity: f32,
}

/// 一组暂归于同一候选人物的未确认脸，供批量审批 UI（Part4 §3.5.1 / Part5 T10）。用户对整组一次性
/// 确认/改派/拒绝。`confidence` = 单脸相似度均值（组匹配强度）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikelyMatchGroup {
    pub person_id: i64,
    pub person_name: Option<String>,
    pub candidate_faces: Vec<FaceThumb>,
    pub confidence: f32,
}
