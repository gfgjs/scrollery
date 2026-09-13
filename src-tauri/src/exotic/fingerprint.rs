// src-tauri/src/exotic/fingerprint.rs
//! 输入指纹（v3 总纲 §5.3 / Part2 §4.3 / 勘误 R5）。
//!
//! 指纹决定「旧 done 是否失效、是否重做」。组成：源 cache_key + 插件 + worker 版本 + 能力 +
//! 能力 API 版本 + **规范化设置**。源文件变化、Worker 升级、能力 API 或渲染参数变化都使旧 done 失效。
//!
//! R5 要点：
//!   - 不再用未定义的字符串拼接；对**固定字段顺序**的版本化结构做规范 JSON 序列化后 SHA-256。
//!   - `settings.target_tier` = `snap_to_tier(requested_size)`——**复用** generator 的吸附函数，
//!     不在 exotic 侧另写档位算法。Request 帧的 `target_long_edge` 也必须传该吸附值，使
//!     请求/输出/指纹三者一致；否则同档不同请求 size 会算出不同指纹、反复重做。
//!   - 新增会改变输出的渲染参数（WebP quality、色彩意图、旋转）时追加进 `settings` 并 bump
//!     `capability_api_version`。
//!   - serde 结构体字段顺序即序列化顺序（serde_json 保序）；禁止对无序 HashMap 直接哈希。

use serde::Serialize;

use crate::thumbnail::generator::snap_to_tier;

/// 指纹结构版本（结构本身变化时 bump）。
const FINGERPRINT_SCHEMA: u32 = 1;

/// thumbnail 能力的输出契约版本。改变输出语义/参数（新增渲染参数等）时 bump → 触发全量失效。
pub const THUMBNAIL_CAPABILITY_API_VERSION: u32 = 1;

/// 缩略图固定输出 MIME。
const THUMBNAIL_OUTPUT_MIME: &str = "image/webp";

/// 规范化设置（进入指纹的、会改变输出且不被 worker_version/capability_api_version 完整覆盖的参数）。
#[derive(Serialize)]
struct ThumbnailSettings {
    /// 吸附后档位（snap_to_tier 结果）。
    target_tier: u32,
    /// 输出 MIME。
    output_mime: &'static str,
}

/// 指纹载荷（字段顺序 = 序列化顺序，冻结）。
#[derive(Serialize)]
struct FingerprintPayload<'a> {
    fingerprint_schema: u32,
    /// i64 十进制字符串（二选一冻结为十进制）。
    media_cache_key: String,
    plugin_id: &'a str,
    worker_version: &'a str,
    capability: &'a str,
    capability_api_version: u32,
    settings: ThumbnailSettings,
}

/// 缩略图指纹计算结果：指纹串 + 吸附后档位（caller 用同一档位填 Request.target_long_edge）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailFingerprint {
    pub fingerprint: String,
    pub tier: u32,
}

/// 计算缩略图指纹。`requested_size` 为原始请求尺寸（内部吸附档位，R5）。
pub fn thumbnail_fingerprint(
    cache_key: i64,
    plugin_id: &str,
    worker_version: &str,
    requested_size: u32,
) -> ThumbnailFingerprint {
    let tier = snap_to_tier(requested_size);
    let payload = FingerprintPayload {
        fingerprint_schema: FINGERPRINT_SCHEMA,
        media_cache_key: cache_key.to_string(),
        plugin_id,
        worker_version,
        capability: "thumbnail",
        capability_api_version: THUMBNAIL_CAPABILITY_API_VERSION,
        settings: ThumbnailSettings {
            target_tier: tier,
            output_mime: THUMBNAIL_OUTPUT_MIME,
        },
    };
    // serde_json 对结构体保序 → 规范序列化；SHA-256 → 十六进制。
    let json = serde_json::to_vec(&payload).expect("指纹载荷序列化不应失败");
    let fingerprint = crate::utils::hash::sha256_hex(&json);
    ThumbnailFingerprint { fingerprint, tier }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail::generator::THUMB_TIERS;

    const PID: &str = "exotic-image-psd";
    const WV: &str = "1.0.0";

    /// 测试用档位：**绑 THUMB_TIERS 事实源，不硬编码数值**。
    ///
    /// b554aa5「档位重定」把梯从 120/240/480/960 改成 64/128/256/512/1024，却漏改了本文件
    /// 及 sink/pipeline 里硬编码的 480 —— 三个测试自那时起一直红着没人发现（CI 因主机不稳
    /// 冻结，本地只跑过滤子集）。绑事实源后，下次改梯这里自动跟随，不会再悄悄失配。
    const TIER: u32 = THUMB_TIERS[3];
    const LOWER_TIER: u32 = THUMB_TIERS[2];

    #[test]
    fn same_tier_different_raw_size_same_fingerprint() {
        // 略小于档位的原始尺寸与档位本身都吸附到同一档 → 指纹相同（R5 核心：避免同档反复重做）。
        let a = thumbnail_fingerprint(123, PID, WV, TIER - 4);
        let b = thumbnail_fingerprint(123, PID, WV, TIER);
        assert_eq!(a.tier, TIER);
        assert_eq!(b.tier, TIER);
        assert_eq!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn cross_tier_different_fingerprint() {
        let a = thumbnail_fingerprint(123, PID, WV, LOWER_TIER);
        let b = thumbnail_fingerprint(123, PID, WV, TIER);
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn any_input_change_changes_fingerprint() {
        let base = thumbnail_fingerprint(123, PID, WV, TIER).fingerprint;
        // 不同 cache_key
        assert_ne!(base, thumbnail_fingerprint(124, PID, WV, TIER).fingerprint);
        // 不同 worker_version（Worker 升级 → 失效）
        assert_ne!(
            base,
            thumbnail_fingerprint(123, PID, "1.0.1", TIER).fingerprint
        );
        // 不同 plugin_id
        assert_ne!(
            base,
            thumbnail_fingerprint(123, "other", WV, TIER).fingerprint
        );
    }

    #[test]
    fn deterministic() {
        // 同输入多次计算稳定（保序序列化）。
        let a = thumbnail_fingerprint(999, PID, WV, 960).fingerprint;
        let b = thumbnail_fingerprint(999, PID, WV, 960).fingerprint;
        assert_eq!(a, b);
        assert_eq!(a.len(), 64); // SHA-256 hex
    }
}
