//! 普通图片产物身份。媒体行、源代次和输出策略隔离文件及数据库引用，档位可并存。

use super::generator::{snap_to_tier, ThumbConfig};

impl ThumbConfig {
    /// 与尺寸无关的输出策略指纹，允许服务层在同策略下选取其它尺寸。
    pub(crate) fn policy_fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        // 版本同时隔离旧物理命名空间；同路径被新媒体行占用时不能复用旧任务的产物。
        hash.update(b"scrollery-image-v3-item\0");
        if self.strategy == "cpu" {
            // 旧 cpu 产物由 WIC 优先生成；改变实际解码顺序后必须隔离旧缓存。
            hash.update(b"image-rs-first-v1\0");
        }
        for value in [&self.strategy, &self.gpu_engine] {
            hash.update(&(value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        hash.update(&self.skip_max_bytes.to_le_bytes());
        hash.update(&[self.webp_quality, u8::from(self.ai_hq_cache)]);
        hash.update(&self.ai_cache_short_edge.to_le_bytes());
        hash.finalize().to_hex().to_string()
    }

    /// 数据库显示产物指纹：算法与策略指纹再加输出档位。
    pub(crate) fn display_fingerprint(&self) -> String {
        format!("{}:{}", self.policy_fingerprint(), snap_to_tier(self.size))
    }

    /// AI 产物独立于显示质量和尺寸，避免重复生成相同分析输入。
    pub(crate) fn ai_fingerprint(&self) -> String {
        format!("image-ai-v3:{}", self.ai_cache_short_edge)
    }

    /// 视口可指定尺寸；其它生产配置必须仍属于当前生效的策略及缓存目录。
    pub(crate) fn same_policy(&self, current: &Self) -> bool {
        self.cache_dir == current.cache_dir
            && self.policy_fingerprint() == current.policy_fingerprint()
    }

    /// 显示路径相对 thumbnails；媒体 ID 防止位置重新占用，文件名保留缓存键用于孤儿清理。
    pub(crate) fn display_path(
        &self,
        item_id: i64,
        source_revision: i64,
        cache_key: i64,
    ) -> String {
        let hex = format!("{:016x}", cache_key as u64);
        format!(
            "{}/{}/i{}/r{}-{}/{}.webp",
            snap_to_tier(self.size),
            &hex[..2],
            item_id,
            source_revision,
            self.policy_fingerprint(),
            hex
        )
    }

    /// AI 路径相对缓存根，同一源和分析配置可被多个显示档位共享。
    pub(crate) fn ai_path(&self, item_id: i64, source_revision: i64, cache_key: i64) -> String {
        let hex = format!("{:016x}", cache_key as u64);
        format!(
            "ai_thumbs/v3/{}/{}/i{}/r{}/{}.webp",
            self.ai_cache_short_edge,
            &hex[..2],
            item_id,
            source_revision,
            hex
        )
    }
}
