// src-tauri/src/derive/kind.rs
//! 派生 kind 注册表 —— 「有哪些派生、各自适用于哪些媒体、本次构建是否编入可运行后端」
//! 的唯一事实来源。
//!
//! 新增派生 = 在此加一个变体 + 在 `video/doc/audio.rs` 实现其 `run`，再翻转 `is_implemented`。
//! 流水线/续传/让步/孤儿恢复全部通用，无需改动。

use std::path::PathBuf;

use crate::error::{AppError, Result};

/// 一种派生任务类型。其字符串形式（`as_str`）即存入 `media_derivations.kind` 的值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivationKind {
    /// 视频封面帧 → WebP，复用缩略图缓存。
    VideoCover,
    /// 视频关键帧雪碧图，用于悬停/进度条 scrub。
    VideoKeyframes,
    /// remux 与 transcode 同 kind;由 `VideoWorkerService` 播放交互路径直接 upsert+claim,
    /// **不进背景 backfill/流水线**(`is_implemented`=false、`for_media` 不含它)。
    /// 按需可播产物(视频格式扩展子系统 §5.2):容器改封或全转码后落独立视频池
    /// `{cache_dir}/video/{cache_key}.mp4`,供 `<video>` 播放。
    VideoPlayable,
    /// 文档首页/封面缩略图。
    DocThumb,
    /// 音频内嵌封面图。
    AudioCover,
    /// 音频标签/歌词元数据。
    AudioMeta,
    /// AI 分析缓存：每张图一份短边≥336 的 WebP，使 CLIP 分析解码一份小缓存而非全分辨率原图
    /// （opt-in，由 `ai_hq_cache_enabled` 控制）。非显示缩略图 —— `produces_thumbnail()` 保持 false。
    AiThumb,
}

impl DerivationKind {
    /// 所有 kind，按派生优先级高→低排列（封面/元数据先于更重的关键帧雪碧图）。
    /// backfill 按此顺序入队，使高价值产物先落地。
    pub const ALL: [DerivationKind; 7] = [
        DerivationKind::AudioMeta,
        DerivationKind::AudioCover,
        DerivationKind::VideoCover,
        DerivationKind::DocThumb,
        DerivationKind::AiThumb,
        DerivationKind::VideoKeyframes,
        // 按需 kind:排 ALL 末尾供 roundtrip/枚举遍历;不进 backfill(is_implemented=false)。
        DerivationKind::VideoPlayable,
    ];

    /// 存入 DB `kind` 列的稳定字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            DerivationKind::VideoCover => "video_cover",
            DerivationKind::VideoKeyframes => "video_keyframes",
            DerivationKind::VideoPlayable => "video_playable",
            DerivationKind::DocThumb => "doc_thumb",
            DerivationKind::AudioCover => "audio_cover",
            DerivationKind::AudioMeta => "audio_meta",
            DerivationKind::AiThumb => "ai_thumb",
        }
    }

    /// 从 DB `kind` 列解析。
    // 固有 from_str：返回 Option<Self>（非 std FromStr 的 Result），语义不同；保留固有方法。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "video_cover" => DerivationKind::VideoCover,
            "video_keyframes" => DerivationKind::VideoKeyframes,
            "video_playable" => DerivationKind::VideoPlayable,
            "doc_thumb" => DerivationKind::DocThumb,
            "audio_cover" => DerivationKind::AudioCover,
            "audio_meta" => DerivationKind::AudioMeta,
            "ai_thumb" => DerivationKind::AiThumb,
            _ => return None,
        })
    }

    /// 当前二进制是否编入了该 kind 的可运行后端。
    /// P0：所有 kind 仅为脚手架 —— 真实后端（MF 视频、pdf.js/pdfium 文档、lofty 音频）
    /// 在 P2/P3/P4 落地。在某 kind 于此返回 `true` 之前，`backfill` 不会入队它，
    /// 因此不会被处理（也不会被误标为错误）。
    pub fn is_implemented(&self) -> bool {
        match self {
            // 后端落地时改为 true（并实现对应 run），框架其余部分无需改动。
            // 视频派生由 Media Foundation 后端驱动（仅 Windows）；非 Windows 平台暂无后端 → 保持 false
            // 不入队，待 AVFoundation/FFmpeg 后补。
            DerivationKind::VideoCover => cfg!(windows), // P2 — derive/video.rs（MF）
            DerivationKind::VideoKeyframes => cfg!(windows), // P2 — derive/video.rs（MF）
            // 纯按需(播放交互路径经 IPC resolve 直接 upsert+claim + 派 VideoWorkerService,§5.2);
            // 绝不进背景 backfill(全库 GB 级转码 = 磁盘爆炸)。保持 false 使 backfill 永不入队它;
            // 产物由 `ipc/video_commands` 直接写 status/payload,不经 `run()` 分发。
            DerivationKind::VideoPlayable => false, // 按需(不 backfill)— ipc/video_commands.rs
            // P4：epub 封面由后端 zip 处理（跨平台）；pdf/svg 由前端离屏渲染（见 get_pending_derivations
            // 排除 + store_doc_thumbnail）。返回 true 使 backfill 为 pdf/epub/svg 全部建行 —— 后端只跑 epub，
            // 前端经 list_pending_doc_thumbs 领取 pdf/svg。
            DerivationKind::DocThumb => true, // P4 — derive/doc.rs（epub）+ 前端（pdf/svg）
            // P3：音频内嵌封面由 lofty 提取（跨平台，纯 Rust）→ 缩略图缓存（derive/audio.rs）。
            DerivationKind::AudioCover => true, // P3 — derive/audio.rs（lofty 内嵌封面）
            // 音频元数据/歌词由 enricher 在补全阶段回填 audio_meta（与视频元数据同处理，非派生），
            // 故此 kind 不入队（保持 false）；run_meta 仅为框架占位。详见 derive/audio.rs 模块文档。
            DerivationKind::AudioMeta => false, // P3 — 由 enricher 处理（见 derive/audio.rs）
            // AI 缓存后端跨平台（WIC + image crate 回退）；实际是否入队仍由 `ai_hq_cache_enabled`
            // 开关在 pipeline 的 disabled_kinds 中 gate（opt-in，默认关）。
            DerivationKind::AiThumb => true, // derive/image.rs
        }
    }

    /// 该 kind 是否产出应回填到 `media_items.thumb_status/thumb_path/thumbhash` 的**封面缩略图**
    /// （使 `MediaThumb` 零改动即可显示，§3.2）。关键帧雪碧图不是封面 —— 是独立的 scrub 资源。
    pub fn produces_thumbnail(&self) -> bool {
        matches!(
            self,
            DerivationKind::VideoCover | DerivationKind::AudioCover | DerivationKind::DocThumb
        )
    }

    /// 会生成栅格化缩略图的文档子类型。纯文本（txt/md）由前端用 CSS「文本卡」渲染（§3.4），
    /// 故有意排除。
    pub const DOC_THUMB_FORMATS: [&'static str; 3] = ["pdf", "epub", "svg"];

    /// 某媒体项适用哪些 kind。供 `backfill` 入队行使用。
    pub fn for_media(media_type: &str, file_format: &str) -> Vec<DerivationKind> {
        match media_type {
            "video" => vec![DerivationKind::VideoCover, DerivationKind::VideoKeyframes],
            "audio" => vec![DerivationKind::AudioMeta, DerivationKind::AudioCover],
            "document" => {
                if DerivationKind::DOC_THUMB_FORMATS.contains(&file_format) {
                    vec![DerivationKind::DocThumb]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }
}

/// 某 kind 的 `run` 产出其产物所需的一切，由消费者一次性解析好。
pub struct DerivationContext {
    pub item_id: i64,
    pub kind: DerivationKind,
    /// 源媒体文件的绝对路径。
    pub abs_path: PathBuf,
    pub file_format: String,
    pub media_type: String,
    /// 运行任务时读取到的源项代次；完成阶段必须与当前 `media_items` 同时匹配。
    pub source_revision: i64,
    /// 源项的 `cache_key` —— 封面复用缩略图缓存的路径/键，使 `MediaThumb` 像普通图片缩略图一样加载
    /// （不变量 §1.3.3）。
    pub cache_key: i64,
    /// 缩略图缓存根目录 —— 封面/雪碧图写入此处（复用 `cache_key`）。
    pub cache_dir: PathBuf,
    /// 目标缩略图边长（像素），用于封面缩放。
    pub thumb_size: u32,
    /// 显示封面的 WebP 编码质量(用户设置,100=无损);雪碧图等非显示产物不使用。
    pub webp_quality: u8,
    /// 视频关键帧雪碧图采样帧数(设置键 `video_keyframe_count`,批次C接线)。仅 `VideoKeyframes`
    /// kind 使用;快照自 `state.config`,每次流水线启动读一次(见 `pipeline.rs`)。
    pub keyframe_count: usize,
    /// 视频关键帧雪碧图单格高度 px(设置键 `sprite_cell_height`,批次C接线)。仅 `VideoKeyframes`
    /// kind 使用。
    pub sprite_cell_height: u32,
    /// AI 分析缓存短边 px(设置键 `ai_cache_short_edge`,批次C接线)。仅 `AiThumb` kind 使用。
    pub ai_cache_short_edge: u32,
}

/// 派生成功的产物。
pub struct DerivationOutput {
    /// 相对产物路径（关键帧的雪碧图；封面的缩略图 db 路径）。存入 `media_derivations.payload_path`。
    pub payload_path: Option<String>,
    /// 封面类 kind：回填到 `media_items` 的 thumbhash（占位模糊色）。非封面 kind 为 `None`。
    pub thumbhash: Option<Vec<u8>>,
    /// 文档类 kind（epub doc_thumb）：upsert 进 `document_meta` 的页数（§3.8.2 / T10）。
    /// 其它 kind 一律 `None`（流水线写入器仅在 `Some` 时 upsert `document_meta`）。
    pub page_count: Option<i64>,
}

/// 将单个派生任务分发到对应 kind 的 `run`。P0 下每个 kind 都路由到 `NotImplemented` 错误；
/// 由于 `is_implemented` 门控了入队，正常运行下在后端落地前永不触达此处。
pub fn run(ctx: &DerivationContext) -> Result<DerivationOutput> {
    match ctx.kind {
        DerivationKind::VideoCover => super::video::run_cover(ctx),
        DerivationKind::VideoKeyframes => super::video::run_keyframes(ctx),
        // 按需 kind:不经通用流水线 `run` 产出(is_implemented=false 门控 backfill,永不触达此处);
        // 播放产物由 `ipc/video_commands` 派 VideoWorkerService 直接生成 + 写 DAO。
        DerivationKind::VideoPlayable => Err(not_implemented(ctx.kind)),
        DerivationKind::DocThumb => super::doc::run_thumb(ctx),
        DerivationKind::AudioCover => super::audio::run_cover(ctx),
        DerivationKind::AudioMeta => super::audio::run_meta(ctx),
        DerivationKind::AiThumb => super::image::run_ai_thumb(ctx),
    }
}

/// 各桩 kind 共用的「本次构建尚未实现」错误。
pub(crate) fn not_implemented(kind: DerivationKind) -> AppError {
    AppError::Internal(format!(
        "derivation kind '{}' has no backend in this build | 该派生 kind 在本次构建中无后端",
        kind.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::format::{classify_media_type, doc_subtype, MediaType};

    /// `DOC_THUMB_FORMATS` 三项恰好满足 `doc_subtype(ext) == ext`。
    ///
    /// 这条**巧合**正是「把 `file_format` 当 `doc_subtype` 直接写库」的 bug 长期没被发现的原因
    /// （`ipc/doc_commands.rs` 旧写法，S 线 P0-b 已改为经 `doc_subtype()` 派生）。
    /// 若将来往 `DOC_THUMB_FORMATS` 加 office/text 类格式（如 docx→"office"、txt→"text"），
    /// 恒等即刻不成立 —— 本测试会红，提醒**所有**写 `document_meta.doc_subtype` 的调用点
    /// 必须已走 `doc_subtype()`，不能再图省事传 `file_format`。
    #[test]
    fn doc_thumb_formats_subtype_identity_is_a_coincidence() {
        for ext in DerivationKind::DOC_THUMB_FORMATS {
            assert_eq!(
                doc_subtype(ext),
                ext,
                "{ext} 的 doc_subtype 不再等于扩展名：写 document_meta 的调用点必须经 doc_subtype() 派生"
            );
        }
        // 反面锚点：这些格式恒等**不**成立，正是同一 bug 的引信（它们目前不在 DOC_THUMB_FORMATS）。
        assert_eq!(doc_subtype("docx"), "office");
        assert_eq!(doc_subtype("txt"), "text");
    }

    /// `ALL` 每个 kind 的 `as_str`/`from_str` 双向 roundtrip 一致,且字符串互不重复。
    #[test]
    fn all_kinds_as_str_from_str_roundtrip() {
        let mut seen = std::collections::HashSet::new();
        for k in DerivationKind::ALL {
            let s = k.as_str();
            assert!(seen.insert(s), "as_str 重复:{s}");
            assert_eq!(DerivationKind::from_str(s), Some(k), "{s} roundtrip 失败");
        }
        assert_eq!(DerivationKind::from_str("nonexistent_kind"), None);
    }

    /// `video_playable` 按需语义锚:字符串稳定、不进 backfill(is_implemented=false)、
    /// 非封面(不镜像 media_items)、`for_media` 不为任何媒体入队它。
    #[test]
    fn video_playable_is_on_demand_only() {
        assert_eq!(DerivationKind::VideoPlayable.as_str(), "video_playable");
        assert_eq!(
            DerivationKind::from_str("video_playable"),
            Some(DerivationKind::VideoPlayable)
        );
        assert!(
            !DerivationKind::VideoPlayable.is_implemented(),
            "按需 kind 不得进背景 backfill"
        );
        assert!(!DerivationKind::VideoPlayable.produces_thumbnail());
        for mt in ["video", "audio", "document", "image"] {
            assert!(
                !DerivationKind::for_media(mt, "mkv").contains(&DerivationKind::VideoPlayable),
                "for_media({mt}) 不应入队 video_playable"
            );
        }
    }

    /// `DOC_THUMB_FORMATS` 必须全是已注册文档格式，否则 `for_media` 永远不会为其入队。
    #[test]
    fn doc_thumb_formats_are_registered_documents() {
        for ext in DerivationKind::DOC_THUMB_FORMATS {
            assert_eq!(
                classify_media_type(ext),
                Some(MediaType::Document),
                "{ext} 不是已注册文档格式"
            );
        }
    }
}
