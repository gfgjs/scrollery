// src-tauri/src/config/boot.rs
//! 启动期配置装配:`ConfigManager` 构造(含降级)+ 启动期一次性读取的设置键。
//!
//! 自 `lib.rs::run()` 的 setup 段 h 迁出(D-450 纯结构移动,行为不变)。
//!

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// [`init`] 的产出:配置门面 + 启动期定死的一批设置值。
pub struct ConfigBoot {
    /// `<app_data_dir>/config.toml`。
    pub path: PathBuf,
    pub manager: Arc<crate::config::ConfigManager>,
    pub thumb_size: u32,
    pub thumb_skip_max_kb: u64,
    pub thumb_strategy: String,
    pub gpu_engine: String,
    /// `thumb_cache_dir`;空串已归一为 `None`(= 用内置派生路径)。
    pub custom_cache_dir: Option<String>,
    pub log_level: String,
    /// `log_dir`;空串已归一为 `None`。
    pub custom_log_dir: Option<String>,
    pub thumb_cache_max_mb: u64,
    pub ai_hq_cache: bool,
    pub thumb_webp_quality: u8,
}

/// 配置文件初始化 + 读取持久化配置(setup 段 h)。
///
/// 语法错时的契约(硬约束):**绝不覆盖/重写用户的坏文件**,回退全部键为 schema
/// 默认值(`ConfigManager::degraded`),错误记入 `last_error` 供 `get_config_status`
/// IPC 展示,应用照常启动——不得因用户手改配置文件写错就拒绝启动。
pub fn init(app_data_dir: &Path) -> ConfigBoot {
    let config_path = app_data_dir.join("config.toml");
    let config_manager = {
        match crate::config::ConfigManager::load_or_init(config_path.clone()) {
            Ok((manager, warnings)) => {
                for w in &warnings {
                    tracing::warn!(
                        key = %w.key,
                        message = %w.message,
                        "config.toml 键警告(已跳过,回退默认值) | config.toml key warning, skipped"
                    );
                }
                manager
            }
            Err(e) => {
                tracing::error!(
                    "config.toml 加载失败,已回退全部键为默认值 | config.toml load failed, falling back to defaults: {e}"
                );
                crate::config::ConfigManager::degraded(config_path.clone(), e)
            }
        }
    };
    let config_manager = Arc::new(config_manager);

    // ── 读取持久化配置 ─────────────────────────────────────
    // A2:以下设置类键改从 `config_manager` 读(不再查 DB)。`ConfigManager::get` 对已知
    // 键恒返回 `Some`(文件缺省则回退 schema 默认值)——`.unwrap_or(x)` 分支理论不可达,
    // 保留纯为防御性兜底(与改动前的容错姿态一致)。Path/Str 类默认值为空字符串时语义
    // 是「未设置,使用代码内置派生路径」,故用 `.filter(|s| !s.is_empty())` 把默认空串
    // 归一为 `None`,保留原 DB 路径「行不存在→None」的下游分支行为。
    let size: u32 = config_manager
        .get("thumb_size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(480);
    let skip: u64 = config_manager
        .get("thumb_skip_max_kb")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let strategy: String = config_manager
        .get("thumb_strategy")
        .unwrap_or_else(|| "cpu".to_string());
    let gpu_eng: String = config_manager
        .get("gpu_engine")
        .unwrap_or_else(|| "wic".to_string());
    let cache_dir: Option<String> = config_manager
        .get("thumb_cache_dir")
        .filter(|s| !s.is_empty());
    // 默认级别裁决(方案 §7 Q2):info——业务事件默认可见,debug 噪音过大。
    let lvl: String = config_manager
        .get("log_level")
        .unwrap_or_else(|| "info".to_string());
    let l_dir: Option<String> = config_manager.get("log_dir").filter(|s| !s.is_empty());
    let max_mb: u64 = config_manager
        .get("thumb_cache_max_mb")
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::thumbnail::cache::DEFAULT_THUMB_CACHE_MAX_MB);
    // AI 高清缓存开关（opt-in，默认关）；驱动缩略图流水线是否顺带产出 AI 缓存。
    let ai_hq: bool = config_manager
        .get("ai_hq_cache_enabled")
        .map(|v| v == "true")
        .unwrap_or(false);
    // 缩略图 WebP 编码质量(1..=99 有损,100=无损),默认 DEFAULT_WEBP_QUALITY。
    let webp_q: u8 = config_manager
        .get("thumb_webp_quality")
        .and_then(|v| v.parse::<u8>().ok())
        .map(|v| v.clamp(1, 100))
        .unwrap_or(crate::thumbnail::exif_thumb::DEFAULT_WEBP_QUALITY);
    // 注:thumb_use_pipeline 配置键已退役(2026-07-10 A/B 裁决删方案一,流水线成唯一
    // 实现,7.3s vs 16-20s);旧库残留的该键无读者,无害。

    ConfigBoot {
        path: config_path,
        manager: config_manager,
        thumb_size: size,
        thumb_skip_max_kb: skip,
        thumb_strategy: strategy,
        gpu_engine: gpu_eng,
        custom_cache_dir: cache_dir,
        log_level: lvl,
        custom_log_dir: l_dir,
        thumb_cache_max_mb: max_mb,
        ai_hq_cache: ai_hq,
        thumb_webp_quality: webp_q,
    }
}
