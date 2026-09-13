// src-tauri/src/thumbnail/cache.rs
//! 尺寸分桶的缩略图缓存管理。
//!
//! 缓存布局（§ 8.2）：
//! `{app_data_dir}/cache/thumbnails/{size}/{2-char-prefix}/{cache_key_hex}.webp`
//! e.g. `cache/thumbnails/300/a3/a3f4b2c1d0e9f7a1.webp`
//! 例如 `cache/thumbnails/300/a3/a3f4b2c1d0e9f7a1.webp`

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::utils::hash::cache_key_to_hex;

/// 构建缩略图文件的完整路径。
pub fn thumb_path(cache_dir: &Path, size: u32, cache_key: i64) -> PathBuf {
    debug_assert!(
        crate::thumbnail::generator::THUMB_TIERS.contains(&size),
        "Thumbnail size {} is not a valid tier | 缩略图尺寸 {} 不是有效档位",
        size,
        size
    );
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    cache_dir
        .join("thumbnails")
        .join(size.to_string())
        .join(prefix)
        .join(format!("{hex}.webp"))
}

/// 检查磁盘上是否已经存在缩略图。
pub fn thumb_exists(cache_dir: &Path, size: u32, cache_key: i64) -> bool {
    thumb_path(cache_dir, size, cache_key).exists()
}

/// 存储在数据库中的相对路径：`"{size}/{prefix}/{hex}.webp"`。
pub fn thumb_db_path(size: u32, cache_key: i64) -> String {
    debug_assert!(
        crate::thumbnail::generator::THUMB_TIERS.contains(&size),
        "Thumbnail size {} is not a valid tier | 缩略图尺寸 {} 不是有效档位",
        size,
        size
    );
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    format!("{size}/{prefix}/{hex}.webp")
}

/// Short edge (px) of the AI-analysis cache. Covers every built-in CLIP model since analysis
/// only ever downscales the short edge to `image_size` (B/16·L/14=224, L/14@336=336), never up.
/// Kept here so both the derivation backend (`derive/image.rs`) and the thumbnail pipeline's
/// one-decode-two-outputs path (`generator.rs`) agree on the size.
/// AI 分析缓存短边（像素）。覆盖所有内置 CLIP 模型（分析只下采样短边到 image_size、绝不上采样）。
/// 放此处使派生后端与缩略图「一次解码两份产物」路径对尺寸保持一致。
/// 注:336 = 当前内置模型集的最大输入边(L/14@336),非随意魔数;但**绑定当前模型集**——
/// 若将来接入需 >336 输入的模型,此值须同步上调,否则该模型的 AI 缓存会偏小(改它使全库 ai_cache 作废)。
pub const AI_CACHE_SHORT_EDGE: u32 = 336;

/// Absolute path of the AI-analysis cache for an image: `cache/ai_thumbs/{prefix}/{hex}.webp`.
/// A short-edge≥336 WebP that CLIP analysis decodes instead of the full-resolution original
/// (keyed by `cache_key`, same prefix scheme as thumbnails). Lives in its own dir so the
/// thumbnail LRU (`enforce_cache_limit`, which only walks `thumbnails/`) never evicts it.
/// 图像 AI 分析缓存的绝对路径：`cache/ai_thumbs/{prefix}/{hex}.webp`。一份短边≥336 的 WebP，
/// 供 CLIP 分析解码以替代全分辨率原图（按 `cache_key` 命名，与缩略图同前缀方案）。独立目录使
/// 缩略图 LRU（`enforce_cache_limit` 只遍历 `thumbnails/`）不会误删它。
pub fn ai_cache_path(cache_dir: &Path, cache_key: i64) -> PathBuf {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    cache_dir
        .join("ai_thumbs")
        .join(prefix)
        .join(format!("{hex}.webp"))
}

/// Relative DB path of the AI cache (relative to `cache_dir`): `"ai_thumbs/{prefix}/{hex}.webp"`.
/// Stored in `media_derivations.payload_path`; the AI pipeline resolves it under `cache_dir`.
/// AI 缓存的相对 DB 路径（相对 `cache_dir`）：`"ai_thumbs/{prefix}/{hex}.webp"`。
/// 存入 `media_derivations.payload_path`；AI 流水线在 `cache_dir` 下解析。
pub fn ai_cache_db_path(cache_key: i64) -> String {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    format!("ai_thumbs/{prefix}/{hex}.webp")
}

/// 确保 AI 缓存文件所在目录存在。
pub fn ensure_ai_cache_dir(cache_dir: &Path, cache_key: i64) -> std::io::Result<()> {
    let p = ai_cache_path(cache_dir, cache_key);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// 人脸分析缓存短边(像素)= YuNet detect_size(640)。T16-R2 方案 A:缩略图档位短边不足时,
/// host 预解码(WIC 优先)一份短边 640 的 WebP,worker 只解这份小图——镜像 ai_cache 之于 CLIP。
/// 注:绑定当前 face 模型集(yunet-sface);派发侧有防呆(detect_size 超此值即不吃缓存,
/// 见 face_pipeline::face_cache_applies),但升值会使全库 face_thumbs 作废,须一并清理。
pub const FACE_CACHE_SHORT_EDGE: u32 = 640;

/// 人脸分析缓存的绝对路径:`cache/face_thumbs/{prefix}/{hex}.webp`(键与前缀方案同缩略图)。
/// 与 ai_thumbs(336,CLIP)分目录:两者输入尺寸不同不能互用,独立目录也便于按类清理。
pub fn face_cache_path(cache_dir: &Path, cache_key: i64) -> PathBuf {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    cache_dir
        .join("face_thumbs")
        .join(prefix)
        .join(format!("{hex}.webp"))
}

/// 构建动态视频缓存目录的绝对路径。
pub fn motion_video_cache_path(cache_dir: &Path, cache_key: i64) -> PathBuf {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    cache_dir
        .join("motion_videos")
        .join(prefix)
        .join(format!("{hex}.mp4"))
}

/// Absolute path of a video keyframe sprite (§3.3): `cache/sprites/{prefix}/{hex}.webp`.
/// One horizontal strip per video, keyed by `cache_key` (same scheme as thumbnails).
/// 视频关键帧雪碧图的绝对路径（§3.3）：`cache/sprites/{prefix}/{hex}.webp`。
/// 每个视频一张水平条带，按 `cache_key` 命名（与缩略图同方案）。
pub fn keyframe_sprite_path(cache_dir: &Path, cache_key: i64) -> PathBuf {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    cache_dir
        .join("sprites")
        .join(prefix)
        .join(format!("{hex}.webp"))
}

/// Relative DB path of a keyframe sprite (relative to `cache_dir`): `"sprites/{prefix}/{hex}.webp"`.
/// Stored in `media_derivations.payload_path`; the frontend resolves it under `cache_dir`.
/// 关键帧雪碧图的相对 DB 路径（相对 `cache_dir`）：`"sprites/{prefix}/{hex}.webp"`。
/// 存入 `media_derivations.payload_path`；前端在 `cache_dir` 下解析。
pub fn keyframe_sprite_db_path(cache_key: i64) -> String {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    format!("sprites/{prefix}/{hex}.webp")
}

/// 查看器渲染色域派生(B 线,2026-07-23,方案 §0②)的绝对路径:
/// `cache/viewer_color/{target_id}/{prefix}/{hex}.{ext}`。`target_id` 见
/// `viewer_color::target::ViewerColorTarget::target_id`(`display-p3` / `dci-p3` /
/// `icc-{profile_id}`);`ext` 为 `"jpg"`(无 alpha)或 `"png"`(有 alpha,由 render.rs 判定)。
/// 与其余产物路径助手同源:按 `cache_key` 命名,mtime 变化即 key 变化,旧文件成孤儿归
/// [`reconcile_orphan_gc`] 收敛。
pub fn viewer_color_path(cache_dir: &Path, target_id: &str, cache_key: i64, ext: &str) -> PathBuf {
    let hex = cache_key_to_hex(cache_key);
    let prefix = &hex[..2];
    cache_dir
        .join("viewer_color")
        .join(target_id)
        .join(prefix)
        .join(format!("{hex}.{ext}"))
}

/// 确保查看器色域派生文件所在目录存在。
pub fn ensure_viewer_color_dir(
    cache_dir: &Path,
    target_id: &str,
    cache_key: i64,
    ext: &str,
) -> std::io::Result<()> {
    let p = viewer_color_path(cache_dir, target_id, cache_key, ext);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// 确保给定缩略图路径的目录存在。
pub fn ensure_thumb_dir(cache_dir: &Path, size: u32, cache_key: i64) -> std::io::Result<()> {
    let p = thumb_path(cache_dir, size, cache_key);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// 确保关键帧雪碧图所在目录存在。
pub fn ensure_sprite_dir(cache_dir: &Path, cache_key: i64) -> std::io::Result<()> {
    let p = keyframe_sprite_path(cache_dir, cache_key);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// 被驱逐文件若属显示缩略图(`thumbnails/` 下),换算成 DB 相对路径(`{size}/{xx}/{hex}.webp`,
/// 与 `media_items.thumb_path` 同构、正斜杠)供事件驱动复位(深审 defer ⑥);其余产物目录
/// (ai/face 缓存等)返回 None——它们的命中判定只查存在性,缺失即自愈重建,不涉 thumb_status。
fn evicted_thumb_db_path(cache_dir: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let rel = path.strip_prefix(cache_dir.join("thumbnails")).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Enforce the thumbnail cache limit by LRU. Returns the **db-relative paths** of evicted
/// display thumbnails so the caller can immediately reset the affected `media_items` rows
/// (event-driven heal — the boot-time stat sweep becomes a rare fallback).
/// 强制执行缩略图缓存大小限制 (LRU)。返回被驱逐**显示缩略图**的 DB 相对路径,供调用方
/// 即时复位受影响的 `media_items` 行(事件驱动自愈——启动期全量 stat 扫描降级为兜底)。
/// 按路径(而非 cache_key)返回:同 key 另一档位文件被驱逐时,现行 thumb_path 不受误伤。
#[must_use = "被驱逐的缩略图路径须交 reset_thumbs_by_evicted_paths 复位,否则重现 LRU 驱逐永久 404(d503843)"]
pub fn enforce_cache_limit(cache_dir: &std::path::Path, max_size_mb: u64) -> Vec<String> {
    let max_size_bytes = max_size_mb.saturating_mul(1024 * 1024);
    let target_size_bytes = (max_size_bytes as f64 * 0.8) as u64;

    let mut total_size = 0;
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = Vec::new();

    // Both the display thumbnails AND the AI-analysis caches share one cache budget and one LRU
    // eviction pass. (Sprites / motion videos are deliberately excluded — they're tied to their
    // source media's lifetime, not browse-recency.) viewer_color(B 线,2026-07-23)并入同一预算
    // (2026-07-19 裁决延伸,方案 §0②):按 target_id/浏览近期性驱逐,行为与缩略图同构。
    // 显示缩略图与 AI 分析缓存（ai_thumbs + face_thumbs）共用同一缓存预算和同一次 LRU 淘汰。
    // （雪碧图/动态视频有意排除 —— 它们绑定源媒体生命周期，而非浏览近期性。）
    let scan_dirs = [
        cache_dir.join("thumbnails"),
        cache_dir.join("ai_thumbs"),
        cache_dir.join("face_thumbs"),
        cache_dir.join("viewer_color"),
    ];
    if scan_dirs.iter().all(|d| !d.exists()) {
        return Vec::new();
    }

    // Use walkdir to iterate all files | 使用 walkdir 遍历所有文件
    for dir in scan_dirs.iter().filter(|d| d.exists()) {
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Ok(metadata) = entry.metadata() {
                    let size = metadata.len();
                    total_size += size;
                    if let Ok(modified) = metadata.modified() {
                        files.push((entry.path().to_path_buf(), modified, size));
                    }
                }
            }
        }
    }

    if total_size <= max_size_bytes {
        tracing::info!(
            "Cache size {} MB is within limit {} MB | 缓存大小 {} MB 在限制 {} MB 内",
            total_size / 1024 / 1024,
            max_size_mb,
            total_size / 1024 / 1024,
            max_size_mb
        );
        return Vec::new();
    }

    tracing::info!(
        "Cache size {} MB exceeds limit {} MB, starting LRU cleanup... | 缓存大小 {} MB 超过限制 {} MB，开始 LRU 清理...",
        total_size / 1024 / 1024, max_size_mb, total_size / 1024 / 1024, max_size_mb
    );

    // Sort ascending by modified time (oldest first) | 按修改时间升序排序（最旧的在前）
    files.sort_by_key(|&(_, modified, _)| modified);

    let mut freed = 0;
    let mut deleted_count = 0;
    let mut evicted_thumb_paths = Vec::new();

    for (path, _, size) in files {
        if total_size.saturating_sub(freed) <= target_size_bytes {
            break;
        }
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(
                "Failed to delete cache file {:?} | 无法删除缓存文件 {:?}: {}",
                path,
                path,
                e
            );
        } else {
            freed += size;
            deleted_count += 1;
            if let Some(db_path) = evicted_thumb_db_path(cache_dir, &path) {
                evicted_thumb_paths.push(db_path);
            }
        }
    }

    tracing::info!(
        "Cache cleanup finished, deleted {} files, freed {} MB | 缓存清理完成，删除 {} 个文件，释放了 {} MB",
        deleted_count, freed / 1024 / 1024, deleted_count, freed / 1024 / 1024
    );
    evicted_thumb_paths
}

// ════════════════════════════════════════════════════════════════════════════
// 缓存治理：占用统计 / 手动清理 / 孤儿即时清理（Part3 §3.3 / Q6-Q8）
// ════════════════════════════════════════════════════════════════════════════

/// `thumb_cache_max_mb` 未配置时的 LRU 预算默认值(MB)= 10 GB。
///
/// 2026-07-19 裁决:LRU 排序键维持 mtime(生成序,不做访问追踪),以放宽预算降低驱逐
/// 触发频率作为替代——预算内 FIFO 与 LRU 无差别。用户显式配置值恒优先于本默认。
pub const DEFAULT_THUMB_CACHE_MAX_MB: u64 = 10 * 1024;

/// 单类目占用:字节 + 文件数(设置面板「缓存大小/数量」统计)。
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCategoryStat {
    pub bytes: u64,
    pub files: u64,
}

impl std::ops::Add for CacheCategoryStat {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            bytes: self.bytes + rhs.bytes,
            files: self.files + rhs.files,
        }
    }
}

/// 各缓存子目录的占用统计（字节+文件数）+ 上限，供设置面板展示与「清理缓存」（§3.3.3 / Q8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    /// 显示缩略图（`thumbnails/`，受 LRU 上限约束）。
    pub thumbnails: CacheCategoryStat,
    /// AI 分析缓存(`ai_thumbs/` + `face_thumbs/` 合并计;与缩略图共用 LRU 预算)。
    /// face_thumbs(T16-R2)并入本类目:同属 AI 分析缓存,免动 CacheStats 的 IPC/前端面。
    pub ai_thumbs: CacheCategoryStat,
    /// 视频关键帧雪碧图（`sprites/`，绑定源媒体生命周期、不受 LRU 淘汰）。
    pub sprites: CacheCategoryStat,
    /// 动态视频缓存（`motion_videos/`，同上）。
    pub motion_videos: CacheCategoryStat,
    /// 音频内嵌封面缓存(`audio_covers/`,同上;2026-07-06 审查 R5 纳入治理——此前游离于
    /// 统计/清理/GC 之外,只增不减)。
    pub audio_covers: CacheCategoryStat,
    /// 查看器渲染色域派生(`viewer_color/`,B 线,2026-07-23;与缩略图共用 LRU 预算,方案 §0②)。
    pub viewer_color: CacheCategoryStat,
    /// 全部类目总占用。
    pub total: CacheCategoryStat,
    /// LRU 上限（MB，仅约束 thumbnails+ai_thumbs；供前端展示「占用/上限」）。
    pub limit_mb: u64,
}

/// 递归累加目录下所有文件的字节数与个数（目录不存在记 0）。只读。
fn dir_stat(dir: &Path) -> CacheCategoryStat {
    if !dir.exists() {
        return CacheCategoryStat::default();
    }
    let mut stat = CacheCategoryStat::default();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if let Ok(meta) = entry.metadata() {
            stat.bytes += meta.len();
            stat.files += 1;
        }
    }
    stat
}

/// 统计各缓存子目录占用 + 总量 + 上限（§3.3.3）。纯只读，不改磁盘。
/// 调用方应在 `spawn_blocking` 内执行（遍历目录是阻塞 IO）。
pub fn compute_cache_stats(cache_dir: &Path, limit_mb: u64) -> CacheStats {
    let thumbnails = dir_stat(&cache_dir.join("thumbnails"));
    let ai_thumbs =
        dir_stat(&cache_dir.join("ai_thumbs")) + dir_stat(&cache_dir.join("face_thumbs"));
    let sprites = dir_stat(&cache_dir.join("sprites"));
    let motion_videos = dir_stat(&cache_dir.join("motion_videos"));
    let audio_covers = dir_stat(&cache_dir.join("audio_covers"));
    let viewer_color = dir_stat(&cache_dir.join("viewer_color"));
    CacheStats {
        total: thumbnails + ai_thumbs + sprites + motion_videos + audio_covers + viewer_color,
        thumbnails,
        ai_thumbs,
        sprites,
        motion_videos,
        audio_covers,
        viewer_color,
        limit_mb,
    }
}

/// `clear_cache(kind)` 的清理范围映射。未知 kind 与 `"all"` 一律全清（防御式：宁可全清不漏）。
pub fn cache_subdirs_for_kind(kind: &str) -> &'static [&'static str] {
    match kind {
        "thumbnails" => &["thumbnails"],
        "ai" => &["ai_thumbs", "face_thumbs"],
        "sprites" => &["sprites"],
        "motion" => &["motion_videos"],
        "audio" => &["audio_covers"],
        // 查看器渲染色域(B 线,方案 §0②):独立 kind,不与 thumbnails 混清(target_id 层级
        // 各异,清理粒度按此类目单独控制)。
        "viewer" => &["viewer_color"],
        _ => &[
            "thumbnails",
            "ai_thumbs",
            "face_thumbs",
            "sprites",
            "motion_videos",
            "audio_covers",
            "viewer_color",
        ],
    }
}

/// `clear_cache(kind)` 删文件后须同步退回 pending 的 `media_derivations.kind` 集合(2026-07-06
/// 审查 P1-4)。派生产物文件被删而 derivation 行仍 status=2 → backfill 不再入队、读取侧返回死
/// 路径 → 永不重建。返回 (需退状态的 derivation kinds, 是否同时复位 media_items.thumb_status)。
///
/// 映射依据(产物落盘位置):
/// - thumbnails/ 同时存放照片缩略图(thumb_status)与 video_cover/doc_thumb 封面(经 encode_media_step
///   回填 thumb_status=1);故清 thumbnails → 复位 thumb_status + 两个封面 derivation。
/// - motion_videos/ 无 derivation 行(get_companion_video_url 按需生成,已原子写),自愈,无需退状态。
pub fn derivations_to_reset_for_kind(kind: &str) -> (&'static [&'static str], bool) {
    match kind {
        "thumbnails" => (&["video_cover", "doc_thumb"], true),
        "ai" => (&["ai_thumb"], false),
        "sprites" => (&["video_keyframes"], false),
        "motion" => (&[], false),
        "audio" => (&["audio_cover"], false),
        // viewer_color(B 线):按需渲染的独立缓存,不经 media_derivations 记行,exists() 即
        // 命中判定——清空后自愈重渲,无需退任何 derivation kind。
        "viewer" => (&[], false),
        // "all" 与未知 kind:全清 → 复位全部封面/派生类目 + thumb_status。
        _ => (
            &[
                "video_cover",
                "doc_thumb",
                "ai_thumb",
                "video_keyframes",
                "audio_cover",
            ],
            true,
        ),
    }
}

/// 删除指定 kind 的缓存子目录（整棵子树），返回释放字节数（best-effort，失败仅 warn）。
/// 与既有 LRU 一致：只删磁盘文件、不改 DB `thumb_status`——缺图由生成流水线按需重建
/// （`enforce_cache_limit` 早已如此，系统对「DB 有记录但文件缺失」健壮）。
/// 调用方应在 `spawn_blocking` 内执行。
pub fn clear_cache_kind(cache_dir: &Path, kind: &str) -> u64 {
    let mut freed = 0;
    for sub in cache_subdirs_for_kind(kind) {
        let dir = cache_dir.join(sub);
        if !dir.exists() {
            continue;
        }
        freed += dir_stat(&dir).bytes;
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(
                "清理缓存子目录失败 {:?} | clear cache subdir failed: {}",
                dir,
                e
            );
        }
    }
    freed
}

/// 音频内嵌封面的缓存路径（`audio_covers/<cache_key:016x>.<ext>`）。与 `ipc::audio_commands::
/// write_cover_to_cache` 共用的单一事实源；ext 由封面 MIME 决定（见 `audio::cover_from`）。
pub fn audio_cover_cache_path(cache_dir: &Path, cache_key: i64, ext: &str) -> PathBuf {
    cache_dir
        .join("audio_covers")
        .join(format!("{cache_key:016x}.{ext}"))
}

/// 音频封面可能的全部扩展名（`audio::cover_from` 的映射域）。枚举清理时逐一尝试。
pub const AUDIO_COVER_EXTS: [&str; 5] = ["jpg", "png", "gif", "bmp", "tiff"];

/// 某 `cache_key` 对应的全部缓存产物绝对路径(5 档缩略图、AI 缓存、face 缓存、雪碧图、动态视频、
/// 以及 5 种扩展名的音频封面——音频封面于 2026-07-06 审查 R5 纳入)。
/// 单一事实源：硬删媒体即时清理孤儿（§3.3.2）按此枚举。纯函数——路径由 cache_key 确定，不查 DB
/// （`media_derivations.payload_path` 会被 FK CASCADE 一并删除，故不可依赖；而路径方案是确定的）。
///
/// **不含 viewer_color**(方案 §0②):其路径含 `target_id` 层(display-p3/dci-p3/icc-{id}),纯函数
/// 无法完备枚举全部动态 target 目录;该类产物的孤儿一律归 [`reconcile_orphan_gc`] 的对账 GC 收敛。
/// 故本枚举维持 14 条(测试 `enumerates_all_artifacts_for_key` 的 `len==14` 亦据此保持)。
pub fn cache_files_for_key(cache_dir: &Path, cache_key: i64) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(14);
    for tier in crate::thumbnail::generator::THUMB_TIERS {
        paths.push(thumb_path(cache_dir, tier, cache_key));
    }
    paths.push(ai_cache_path(cache_dir, cache_key));
    paths.push(face_cache_path(cache_dir, cache_key));
    paths.push(keyframe_sprite_path(cache_dir, cache_key));
    paths.push(motion_video_cache_path(cache_dir, cache_key));
    for ext in AUDIO_COVER_EXTS {
        paths.push(audio_cover_cache_path(cache_dir, cache_key, ext));
    }
    paths
}

/// 删除某 `cache_key` 的全部缓存产物（best-effort）。返回成功删除的文件数。
/// NotFound 属常态（多数产物本就不存在），不记日志；其它 IO 错误 warn 但不阻塞调用方。
pub fn remove_cache_files_for_key(cache_dir: &Path, cache_key: i64) -> usize {
    let mut removed = 0;
    for p in cache_files_for_key(cache_dir, cache_key) {
        match std::fs::remove_file(&p) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(
                "孤儿缓存删除失败 {:?} | orphan cache delete failed: {}",
                p,
                e
            ),
        }
    }
    removed
}

/// 文件名主干恰为 16 位小写 hex → 解析回 cache_key(i64);否则 None。
/// 对账 GC 的识别护栏:原子写残留的临时文件(`xxx.webp.tmp` 的 stem 是 "xxx.webp")、
/// 任何外来文件都解析失败 → 永不触碰。
fn parse_cache_key_stem(path: &Path) -> Option<i64> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() != 16 {
        return None;
    }
    // cache_key_to_hex 只产小写;大写一律视为外来文件,不冒险。
    if !stem
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    u64::from_str_radix(stem, 16).ok().map(|v| v as i64)
}

/// 后台对账 GC(Part3 §3.3.2 兜底路径):删除六个产物子目录中 DB 已无对应 cache_key 的
/// 孤儿文件。与「硬删即时清理」(`remove_cache_files_for_key`)互补——即时清理漏掉的
/// (崩溃/删除失败/历史遗留)由本函数周期收敛。
///
/// 安全护栏(复审 §467 校准):
/// ① 只删 mtime 早于 `epoch`(进程启动时刻)的文件——本会话新写的产物可能尚未落 DB 行
///    (写文件与写 DB 行之间有窗口),一律不动,留到下次会话收敛;
/// ② 只识别文件名恰为 16 位小写 hex 的产物文件(`parse_cache_key_stem`);
/// ③ best-effort:单文件删除失败仅 warn,不中断整轮。
/// 调用方应在 `spawn_blocking` 内执行(全程阻塞 IO)。返回(删除数,释放字节)。
pub fn reconcile_orphan_gc(
    cache_dir: &Path,
    live_keys: &std::collections::HashSet<i64>,
    epoch: std::time::SystemTime,
) -> (usize, u64) {
    // 六个子目录的产物都按 cache_key 的 16 位 hex 命名(thumbnails 多一层 {size} 层级,
    // walkdir 递归天然覆盖;audio_covers 扩展名多样但 stem 同为 16-hex,护栏②天然适配)。
    // 软删行的 key 在 live 集内(软删可恢复,其缓存不算孤儿)。
    let mut removed = 0usize;
    let mut freed = 0u64;
    for sub in [
        "thumbnails",
        "ai_thumbs",
        "face_thumbs",
        "sprites",
        "motion_videos",
        "audio_covers",
        // viewer_color(B 线,2026-07-23):路径 `viewer_color/{target_id}/{prefix}/{hex}.{ext}`,
        // walkdir 递归穿 target_id 与 prefix 两层;stem 仍为 16-hex(护栏②天然适配,ext=jpg/png
        // 不影响 file_stem)。mtime 变→新 key→旧派生成孤儿,由本对账 GC 收敛(方案 §0②)。
        "viewer_color",
    ] {
        let dir = cache_dir.join(sub);
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(key) = parse_cache_key_stem(entry.path()) else {
                continue;
            };
            if live_keys.contains(&key) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            // 护栏①:mtime 取不到或不早于 epoch → 保守跳过。
            match meta.modified() {
                Ok(mtime) if mtime < epoch => {}
                _ => continue,
            }
            let size = meta.len();
            match std::fs::remove_file(entry.path()) {
                Ok(()) => {
                    removed += 1;
                    freed += size;
                }
                Err(e) => tracing::warn!(
                    "对账 GC 删除孤儿失败 {:?} | orphan GC delete failed: {}",
                    entry.path(),
                    e
                ),
            }
        }
    }
    if removed > 0 {
        tracing::info!(
            "对账 GC 完成:删除 {} 个孤儿,释放 {} MB | orphan GC removed {} files, freed {} MB",
            removed,
            freed / 1024 / 1024,
            removed,
            freed / 1024 / 1024
        );
    }
    (removed, freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 唯一临时目录（按 tag + 进程号隔离并行测试），返回前清空。
    fn unique_tmp(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "scrollery_cache_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_file(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; bytes]).unwrap();
    }

    /// `cache_files_for_key` 枚举 14 条路径:5 档缩略图 + ai_thumb + face_thumb + sprite + motion
    /// + 5 种扩展名音频封面(R5),且与各 path 助手一致。
    #[test]
    fn enumerates_all_artifacts_for_key() {
        let dir = Path::new("C:/cache"); // 纯路径计算，不触磁盘
        let files = cache_files_for_key(dir, 0x1234);
        assert_eq!(files.len(), 14);
        // 与单产物助手逐一吻合（确保枚举不漏不串）。
        assert!(files.contains(&thumb_path(dir, 64, 0x1234)));
        assert!(files.contains(&thumb_path(dir, 1024, 0x1234)));
        assert!(files.contains(&ai_cache_path(dir, 0x1234)));
        assert!(files.contains(&face_cache_path(dir, 0x1234)));
        assert!(files.contains(&keyframe_sprite_path(dir, 0x1234)));
        assert!(files.contains(&motion_video_cache_path(dir, 0x1234)));
        for ext in AUDIO_COVER_EXTS {
            assert!(files.contains(&audio_cover_cache_path(dir, 0x1234, ext)));
        }
    }

    /// 即时孤儿清理：删除该 key 落在磁盘上的产物，返回删除数；不存在的产物不计入、不报错。
    #[test]
    fn remove_cache_files_deletes_existing_only() {
        let dir = unique_tmp("orphan");
        let key = 0xABCD_i64;
        // 落 3 个产物（64 档缩略图 + ai + sprite），另 4 个不存在。
        write_file(&thumb_path(&dir, 64, key), 10);
        write_file(&ai_cache_path(&dir, key), 10);
        write_file(&keyframe_sprite_path(&dir, key), 10);
        let removed = remove_cache_files_for_key(&dir, key);
        assert_eq!(removed, 3);
        // 再删一次：全不存在 → 0，且不 panic。
        assert_eq!(remove_cache_files_for_key(&dir, key), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 占用统计：分目录字节+文件数累加 + total 求和 + limit 透传。
    #[test]
    fn stats_sum_per_subdir() {
        let dir = unique_tmp("stats");
        write_file(&thumb_path(&dir, 128, 1), 100);
        write_file(&thumb_path(&dir, 256, 2), 200);
        write_file(&ai_cache_path(&dir, 3), 50);
        write_file(&keyframe_sprite_path(&dir, 4), 30);
        write_file(&motion_video_cache_path(&dir, 5), 70);
        write_file(&audio_cover_cache_path(&dir, 6, "jpg"), 40);
        // viewer_color(B 线,方案 §0⑧-5 原要求补测):独立类目须计入分项 + total(施工时漏补)。
        write_file(&viewer_color_path(&dir, "display-p3", 7, "jpg"), 25);
        let s = compute_cache_stats(&dir, 512);
        assert_eq!((s.thumbnails.bytes, s.thumbnails.files), (300, 2));
        assert_eq!((s.ai_thumbs.bytes, s.ai_thumbs.files), (50, 1));
        assert_eq!((s.sprites.bytes, s.sprites.files), (30, 1));
        assert_eq!((s.motion_videos.bytes, s.motion_videos.files), (70, 1));
        assert_eq!((s.audio_covers.bytes, s.audio_covers.files), (40, 1));
        assert_eq!((s.viewer_color.bytes, s.viewer_color.files), (25, 1));
        assert_eq!((s.total.bytes, s.total.files), (515, 7));
        assert_eq!(s.limit_mb, 512);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 分类清理：clear "ai" 只删 ai_thumbs、返回其字节；其余子目录不动。
    #[test]
    fn clear_kind_removes_only_target_subdir() {
        let dir = unique_tmp("clear");
        write_file(&thumb_path(&dir, 64, 1), 100);
        write_file(&ai_cache_path(&dir, 2), 60);
        let freed = clear_cache_kind(&dir, "ai");
        assert_eq!(freed, 60);
        assert!(!dir.join("ai_thumbs").exists());
        assert!(
            dir.join("thumbnails").exists(),
            "thumbnails 不应被 ai 清理触及"
        );
        // "all" 清掉剩余。
        let freed_all = clear_cache_kind(&dir, "all");
        assert_eq!(freed_all, 100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 分类清理:clear "viewer" 只清 viewer_color 子树(独立 kind,不与 thumbnails/ai 混清,
    /// 方案 §0⑧-5 原要求补测)。
    #[test]
    fn clear_kind_viewer_removes_only_viewer_color_subtree() {
        let dir = unique_tmp("clear_viewer");
        write_file(&thumb_path(&dir, 64, 1), 100);
        write_file(&viewer_color_path(&dir, "display-p3", 2, "jpg"), 60);
        let freed = clear_cache_kind(&dir, "viewer");
        assert_eq!(freed, 60);
        assert!(!dir.join("viewer_color").exists());
        assert!(
            dir.join("thumbnails").exists(),
            "thumbnails 不应被 viewer 清理触及"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 对账 GC:孤儿(不在 live 集)删除;在册文件、epoch 之后的新文件、非 16-hex 文件不动。
    #[test]
    fn orphan_gc_respects_liveset_epoch_and_naming() {
        let dir = unique_tmp("gc");
        let live_key = 0x11_i64;
        let orphan_key = 0x22_i64;
        write_file(&thumb_path(&dir, 64, live_key), 10);
        write_file(&thumb_path(&dir, 64, orphan_key), 10);
        write_file(&ai_cache_path(&dir, orphan_key), 10);
        // 外来文件:文件名非 16 位 hex,任何情况下不触碰。
        let alien = dir.join("thumbnails").join("readme.txt");
        write_file(&alien, 5);
        let live: std::collections::HashSet<i64> = [live_key].into_iter().collect();

        // epoch = 远古 → 所有文件 mtime 都不早于它 → 一个不删(护栏①:会话内新文件保护)。
        let past = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(reconcile_orphan_gc(&dir, &live, past), (0, 0));
        assert!(thumb_path(&dir, 64, orphan_key).exists());

        // epoch = 未来 → 两个孤儿(64 档缩略图 + ai 缓存)删除;在册与外来文件保留。
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        let (removed, freed) = reconcile_orphan_gc(&dir, &live, future);
        assert_eq!(removed, 2);
        assert_eq!(freed, 20);
        assert!(thumb_path(&dir, 64, live_key).exists());
        assert!(!thumb_path(&dir, 64, orphan_key).exists());
        assert!(!ai_cache_path(&dir, orphan_key).exists());
        assert!(alien.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 对账 GC 覆盖 viewer_color 的深层布局(`{target_id}/{prefix}/{hex}.{ext}`,比其余子目录多一层
    /// target_id):孤儿在深层路径下仍被识别删除;在册 key 不误删(方案 §0⑧-5 原要求补测)。
    #[test]
    fn orphan_gc_handles_viewer_color_deep_target_prefix_layout() {
        let dir = unique_tmp("gc_viewer");
        let live_key = 0x33_i64;
        let orphan_key = 0x44_i64;
        write_file(&viewer_color_path(&dir, "display-p3", live_key, "jpg"), 10);
        write_file(&viewer_color_path(&dir, "dci-p3", orphan_key, "png"), 10);
        let live: std::collections::HashSet<i64> = [live_key].into_iter().collect();

        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        let (removed, freed) = reconcile_orphan_gc(&dir, &live, future);
        assert_eq!(removed, 1);
        assert_eq!(freed, 10);
        assert!(
            viewer_color_path(&dir, "display-p3", live_key, "jpg").exists(),
            "在册 key 不应被误删"
        );
        assert!(
            !viewer_color_path(&dir, "dci-p3", orphan_key, "png").exists(),
            "深层 target_id/prefix 下的孤儿应被收敛"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
