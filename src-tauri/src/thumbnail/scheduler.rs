//! 缩略图第一版成本策略：只依赖扫描已知字段，不在快速队列中打开媒体文件。

use super::generator::ThumbConfig;
use crate::db::queries::ThumbnailLane;

/// 任务配置摘要及尺寸无关的缓存家族摘要；各为 128 位，持久化为小写 hex。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputFingerprint([u8; 16], Option<[u8; 16]>);

impl OutputFingerprint {
    /// 只对真正影响缩略图产物和直显选择的配置求摘要；源版本单独属于任务键。
    pub fn for_image(config: &ThumbConfig) -> Self {
        let canonical = format!(
            "image:v1:size={}:quality={}:strategy={}:direct_max={}:gpu_engine={}:ai_hq={}:ai_edge={}",
            config.size,
            config.webp_quality,
            config.strategy,
            config.skip_max_bytes,
            "wic", // 旧设置仅有此值，保留既有缓存指纹而不再让它影响路由。
            config.ai_hq_cache,
            config.ai_cache_short_edge,
        );
        let hash = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash.as_ref()[..16]);
        let family_canonical = canonical.replacen(&format!("size={}", config.size), "size=0", 1);
        let family_hash = ring::digest::digest(&ring::digest::SHA256, family_canonical.as_bytes());
        let mut family = [0u8; 16];
        family.copy_from_slice(&family_hash.as_ref()[..16]);
        Self(bytes, Some(family))
    }

    /// 视频第一帧封面只依赖输出尺寸、质量与原生后端政策。
    pub fn for_native_video_cover(config: &ThumbConfig) -> Self {
        let canonical = format!(
            "video-cover:v1:backend_policy=native_only:size={}:quality={}",
            config.size, config.webp_quality
        );
        let hash = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash.as_ref()[..16]);
        let family_canonical = canonical.replacen(&format!("size={}", config.size), "size=0", 1);
        let family_hash = ring::digest::digest(&ring::digest::SHA256, family_canonical.as_bytes());
        let mut family = [0u8; 16];
        family.copy_from_slice(&family_hash.as_ref()[..16]);
        Self(bytes, Some(family))
    }

    /// 接受新任务摘要及旧 128 位摘要；DB/路径输入无法注入路径组件。
    pub fn from_hex(hex: &str) -> Option<Self> {
        if !matches!(hex.len(), 32 | 64)
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        Some(Self(
            u128::from_str_radix(&hex[..32], 16).ok()?.to_be_bytes(),
            if hex.len() == 64 {
                Some(u128::from_str_radix(&hex[32..], 16).ok()?.to_be_bytes())
            } else {
                None
            },
        ))
    }

    /// 缓存家族不含尺寸；旧摘要继续使用旧命名空间。
    pub fn cache_namespace(self) -> String {
        match self.1 {
            Some(family) => format!("family-{}", Self(family, None).hex()),
            None => self.hex(),
        }
    }

    pub fn hex(self) -> String {
        let mut out = String::with_capacity(if self.1.is_some() { 64 } else { 32 });
        for byte in self.0.into_iter().chain(self.1.into_iter().flatten()) {
            use std::fmt::Write;
            write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
        }
        out
    }
}

/// 分级策略版本写入轮次日志，阈值尚未据真机样本调优。
pub const COST_POLICY_VERSION: u32 = 1;

/// 全库轮次阶段。阶段只前进；视口任务在每一阶段都可独立提升优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailRunPhase {
    Fast,
    Heavy,
    Exception,
    Complete,
}

/// 候选发现结束后才可离开快速批；一个阶段的在途任务仍计入 open_counts。
pub fn advance_run_phase(
    current: ThumbnailRunPhase,
    discovery_complete: bool,
    open_counts: [u64; 3],
) -> ThumbnailRunPhase {
    match current {
        ThumbnailRunPhase::Fast if discovery_complete && open_counts[0] == 0 => {
            ThumbnailRunPhase::Heavy
        }
        ThumbnailRunPhase::Heavy if open_counts[1] == 0 => ThumbnailRunPhase::Exception,
        ThumbnailRunPhase::Exception if open_counts[2] == 0 => ThumbnailRunPhase::Complete,
        _ => current,
    }
}

/// 连续视口派发的计数由 Coordinator 持有；后台仍处快速批时不会借公平名额提前执行重型项。
pub fn choose_viewport_dispatch(viewport_ready: bool, background_ready: bool, streak: u8) -> bool {
    viewport_ready && (!background_ready || streak < 4)
}

const HEAVY_FILE_BYTES: i64 = 32 * 1024 * 1024;
const HEAVY_PIXELS: u128 = 64_000_000;
const HEAVY_WORKSET_BYTES: u128 = 256 * 1024 * 1024;

/// 已知普通图片先入 Q1；尺寸/内存过大、未知成本或非常规格式入 Q2。
/// 解码后的实际耗时可在后续运行中把同源版本提升到重型类。
pub fn classify_image_cost(
    file_format: &str,
    file_size: i64,
    width: i64,
    height: i64,
) -> ThumbnailLane {
    if file_size <= 0 || width <= 0 || height <= 0 {
        return ThumbnailLane::Heavy;
    }
    if !matches!(
        file_format.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tif" | "tiff" | "heic" | "heif" | "avif"
    ) {
        return ThumbnailLane::Heavy;
    }
    let pixels = (width as u128) * (height as u128);
    // 已知元数据不足以推断实际 plane/位深，8B/px 只作保守预估；实际解码前再核预算。
    let estimated_workset = pixels.saturating_mul(8).saturating_add(4 * 1024 * 1024);
    if file_size > HEAVY_FILE_BYTES
        || pixels > HEAVY_PIXELS
        || estimated_workset > HEAVY_WORKSET_BYTES
    {
        ThumbnailLane::Heavy
    } else {
        ThumbnailLane::Fast
    }
}
