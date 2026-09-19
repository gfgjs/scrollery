//! 媒体项 / 图像 / 音频 / 视频 / 详情域模型。

use serde::{Deserialize, Serialize};

// ── 媒体项 ───────────────────────────────────────────────────────────────

/// 核心媒体项（来自 `media_items` 表的所有字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: i64,
    pub directory_id: i64,
    pub file_name: String,
    pub file_size: i64,
    pub file_mtime: i64,
    pub file_format: String,
    pub media_type: String,
    pub width: i64,
    pub height: i64,
    pub duration_ms: Option<i64>,
    pub sort_datetime: i64,
    pub cache_key: i64,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
    pub is_favorited: bool,
    pub is_deleted: bool,
    pub deleted_at: Option<i64>,
    pub rating: i64,
    /// 用户颜色标签 0-7（0=未标）。与 rating 同类的逐项小标量，供详情页单项设色（T16）。
    pub color_label: i64,
    /// 用户在看图台施加的展示旋转（归一化 0/90/180/270，顺时针；V20）。与拍摄内在方向
    /// (`video_meta.rotation` / EXIF orientation) 正交——此为用户偏好、可改回，非文件固有属性。
    pub view_rotation: i64,
    /// 播放器上次退出时的播放进度(毫秒;V23)。重开同一视频时据此续播,与 `view_rotation`
    /// 同姿态——用户会话偏好、随时可覆写,非文件固有属性。
    pub playback_position_ms: i64,
    pub is_live_photo: bool,
    pub has_embedded_video: bool,
    pub companion_of: Option<i64>,
    pub content_hash: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 媒体源的逻辑代次。缩略图等异步生产者用它与 `cache_key` 一起做条件写，
    /// 不允许旧源的结果覆盖扫描器已经推进代次的新源。
    #[serde(skip)]
    pub source_revision: i64,
}

/// Minimal item used for layout computation (only fields Justified Layout +
/// card-skeleton rendering need). Heavy metadata (file name, dir path, EXIF, GPS)
/// is intentionally excluded and fetched on demand via `get_meta_for_viewport`.
///
/// 用于布局计算的最小化项（仅 Justified Layout 与卡片骨架渲染所需字段）。
/// 重型元数据（文件名、目录路径、EXIF、GPS）有意排除，按需经
/// `get_meta_for_viewport` 拉取。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutItem {
    pub id: i64,
    pub width: i64,
    pub height: i64,
    pub file_size: i64,
    pub sort_datetime: i64,
    pub file_format: String,
    pub media_type: String,
    pub is_live_photo: bool,
    pub duration_ms: Option<i64>,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
    pub is_favorited: bool,
    /// 用户评分 0-5（0 = 未评分）。与 is_favorited 同为逐项小标量，随布局行常驻，
    /// 供网格直接显示星级 / hover 快捷评分 / 「≥N 星」筛选，无需懒加载重元数据。
    pub rating: i64,
    /// 用户颜色标签 0-7（0 = 未标）。与 rating 同为逐项小标量，随布局行常驻，供网格 swatch 显示
    /// 与按色筛选（T16）。色档颜色映射在前端（schema.rs:582 仅存档位）。
    pub color_label: i64,
    /// 系统可用态：'online' | 'offline' | 'missing'（卷/扫描驱动，与 is_deleted 正交）。
    /// 前端据此置灰 + 角标（缺失检测 Part2 §3.2）。与 media_type 同为逐项小串。
    pub availability: String,
    // 分组字段 — 供布局算法使用（folder 分隔符）。S2 消脂：目录路径/名称不再逐行携带
    //(百万行 × 双 JOIN + 字符串拼接实测占查询成本 2/3),只存 dir_id,标签经 DirLabel
    // 映射（query_dir_labels，量级 10^3）在分组边界处还原。
    pub dir_id: Option<i64>,
    pub similarity: Option<f64>,
    /// 缩略图缓存键(multi-tier serving,2026-08-16 阶段 2):档位文件路径
    /// `{tier}/{xx}/{hex}.webp` 的确定性输入——出口拼装按需重写 thumb_path 时用,
    /// 不入线上载荷(HydratedRow 不含此字段)。
    pub cache_key: i64,
}

/// 目录标签映射值（S2 布局查询消脂）：布局查询不再逐行 JOIN directories/scan_roots 拼接
/// 路径，folder 分组标签与 folder 轴排序改经 `dir_id → DirLabel` 映射还原（全库目录量级
/// 10^3，一次小查询，见 `query_dir_labels`）。
#[derive(Debug, Clone)]
pub struct DirLabel {
    /// 目录相对路径 —— folder 轴目录序的前序 DFS 键来源（`encode_tree_sort_key(rel_path)`）。
    pub rel_path: String,
    /// 展示路径，folder 分隔符标签。语义 = 原 SQL
    /// `CASE WHEN d.rel_path='' THEN r.path ELSE r.path||'/'||d.rel_path END`。
    pub display: String,
    /// 目录名（display 为空时的回退标签，沿旧 group_key 语义）。
    pub name: String,
    /// 所属 scan root 的 `created_at` —— folder 轴目录序的最高位键（root 序 = created_at, id），
    /// 使不同根整棵子树连续、不按相同 rel_path 跨根交错。
    pub root_created_at: i64,
    /// 所属 scan root 的 `id` —— root 序的稳定 tiebreaker（同秒创建的多根定序）。
    pub root_id: i64,
}

/// 仅为可视区按需拉取的逐项重型元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMeta {
    pub id: i64,
    pub file_name: String,
    pub dir_path: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lng: Option<f64>,
    pub exif_make: Option<String>,
    pub exif_model: Option<String>,
    pub exif_lens: Option<String>,
    pub exif_focal_length: Option<f64>,
    pub exif_aperture: Option<f64>,
    pub exif_shutter: Option<String>,
    pub exif_iso: Option<i64>,
}

// ── 图像元数据 ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageMeta {
    pub item_id: i64,
    pub orientation: i64,
    pub exif_datetime: Option<i64>,
    pub exif_make: Option<String>,
    pub exif_model: Option<String>,
    pub exif_lens: Option<String>,
    pub exif_focal_length: Option<f64>,
    pub exif_aperture: Option<f64>,
    pub exif_shutter: Option<String>,
    pub exif_iso: Option<i64>,
    pub exif_gps_lat: Option<f64>,
    pub exif_gps_lng: Option<f64>,
    pub dominant_hue: Option<i64>,
    pub dominant_sat: Option<i64>,
    pub dominant_lum: Option<i64>,
    pub dominant_hex: Option<String>,
    pub is_monochrome: bool,
}

// ── 音频元数据（§3.6） ──────────────────────────────────────────────────────────

/// 来自 `audio_meta` 的标签/属性行（艺术家/专辑/音轨/年份/流派 + 歌词来源）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeta {
    pub item_id: i64,
    pub audio_codec: Option<String>,
    pub artist: Option<String>,
    pub album_title: Option<String>,
    pub track_title: Option<String>,
    pub track_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    /// 'embedded' | 'lrc' | 'none' —— 歌词来源（文本按来源懒加载）。
    pub lyrics_source: Option<String>,
    pub lyrics_path: Option<String>,
}

/// 播放器（`/audio/:id`）的完整音频详情：核心项 + 绝对路径 + 标签 + 封面 + 歌词。
/// 标签/歌词按需从文件读取，使既有库无需重扫即可工作（§3.6）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDetail {
    #[serde(flatten)]
    pub item: MediaItem,
    pub abs_path: String,
    pub meta: AudioMeta,
    /// 全分辨率内嵌封面，按需抽取至缓存（可 `convertFileSrc`）。`None` → 无内嵌封面（前端显示占位）。
    pub cover_path: Option<String>,
    /// 解析出的歌词文本（内嵌或 `.lrc`）；都没有则为 `None`。
    pub lyrics: Option<String>,
    /// `lyrics` 是否带 `[mm:ss]` LRC 时间轴（前端随播放同步）为真。
    pub lyrics_synced: bool,
}

// ── 媒体详情（完整） ──────────────────────────────────────────────────────

/// 用户打开媒体项时返回给前端的完整详情。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDetail {
    #[serde(flatten)]
    pub item: MediaItem,
    pub abs_path: String,
    pub image_meta: Option<ImageMeta>,
    /// 系统可用态（缺失检测 Part2 §3.2）：'online' | 'offline' | 'missing'。
    /// 大图查看器据此对「卷离线/文件缺失」给出明确提示，而非任由 <img> 显示 broken 图标。
    pub availability: String,
    /// 视频元数据(播放器线,2026-07-22)：`None`=非视频或尚未 enrichment 探测出 `video_meta` 行。
    pub video_meta: Option<VideoMeta>,
}

/// 视频元数据投影(播放器线):字段对齐 `video_meta` 表实际列(`db::queries::metadata::upsert_video_meta`)。
/// `cover_time_ms` 有意不带出——播放器 IPC 层当前无消费方,按需再补。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMeta {
    pub video_codec: Option<String>,
    pub fps: Option<f64>,
    pub bitrate: Option<i64>,
    pub rotation: i64,
    pub has_audio: bool,
}
