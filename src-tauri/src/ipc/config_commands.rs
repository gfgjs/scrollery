//! 应用配置命令(§ 6.1 — config)。
//!
//! # 两条互不相通的读写路径
//! - **用户设置**:唯一真源是 <app_data_dir>/config.toml(config::SETTING_DEFS 登记的全部键)。
//!   读写走 get_settings_snapshot / set_app_settings / clear_settings(实现见 config::settings),
//!   启动另有一次合并入口 get_startup_config。
//! - **内部状态**:应用自己记账的键(schema_version、首次引导标记、任务启停标志、环境探测结果、
//!   后台时间戳)仍留 SQLite app_config 表,经 get_app_config / set_app_config 逐键访问。
//!
//! 两条路径**互不兜底**:设置类键送到通用键值命令会被明确拒绝(稳定码
//! config_setting_key_requires_snapshot),而不是悄悄走回旧路径;反之亦然。这是「设置只有一个入口、
//! 一个真源」的强制条件(2026-09-16 设置集中保存,方案 §5.1)。

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::config::{SettingDef, SettingsChange, SettingsSnapshot, STATE_KEYS};
use crate::db::queries::{get_config, set_config};
use crate::error::{AppError, Result};
use crate::state::AppState;

/// 日志级别热重载句柄的再导出:logging 模块在装配 subscriber 时经此处写入。实现已随设置影响
/// 逻辑一起迁到 config::effects——它按批归约应用,不再散在各命令里。
pub use crate::config::effects::LOG_RELOAD;

/// 状态类键边界:设置类键与两清单外的键一律以稳定 code 拒绝。读写两个入口共用(未知键既不读 DB
/// 也不写 DB)。
fn ensure_state_key(key: &str) -> Result<()> {
    if SettingDef::find(key).is_some() {
        return Err(AppError::Config {
            code: "config_setting_key_requires_snapshot",
            message: format!(
                "{key} 是用户设置项,请改用 get_settings_snapshot / set_app_settings 读写(唯一真源为 config.toml)"
            ),
        });
    }
    if STATE_KEYS.contains(&key) {
        return Ok(());
    }
    Err(AppError::Config {
        code: "config_unknown_key",
        message: format!("未知配置键 {key}:既不是设置类键也不是状态类键,拒绝访问"),
    })
}

// ── 用户设置:统一快照 / 批量提交 / 完整重置 ────────────────────────────────────

/// 取全部已注册设置的生效值 + 本进程 revision/generation(内存读,一次返回,供运行时刷新使用)。
#[tauri::command]
pub async fn get_settings_snapshot(state: State<'_, Arc<AppState>>) -> Result<SettingsSnapshot> {
    Ok(state.config.snapshot())
}

/// 批量提交设置:patch 为具名键值(结构类键传规范 JSON 文本),generation 为调用方见到的进程代次。
/// 任一项非法即**整批拒绝**;写盘成功才返回新的快照与变化键。
///
/// generation 过期(重置之后)以稳定码 config_stale_generation 拒绝——前端据此重新取快照,
/// 而不是把旧值覆盖回去。
#[tauri::command]
pub async fn set_app_settings(
    app: AppHandle,
    patch: BTreeMap<String, String>,
    generation: u64,
    state: State<'_, Arc<AppState>>,
) -> Result<SettingsChange> {
    crate::config::settings::submit_settings_patch(&app, state.inner(), patch, generation).await
}

/// 恢复默认设置:用 schema 生成的全默认模板原子替换 config.toml,返回与批量写入同型的应用结果。
///
/// 只重置用户设置:数据库中的内部状态(资产、阅读进度、扫描根、任务启停标志、首次引导标记)
/// 不参与重置(方案 §3.2/§7)。
#[tauri::command]
pub async fn clear_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<SettingsChange> {
    crate::config::settings::reset_settings(&app, state.inner()).await
}

/// 启动所需内部状态(只读一次批量 DB 读取;设置值另从快照构造函数取,不在这里逐字段列举)。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupState {
    pub first_launch: Option<String>,
    pub guide_seen: Option<String>,
}

/// get_startup_config 的返回值:设置快照 + 启动内部状态,一次往返拿齐。
///
/// 为什么把设置与状态合并在一个命令里:启动阶段前端需要同时拿到两者才能解除设置交互门控;
/// 分两次调用会让「先应用设置、再应用内部状态、最后解除门控」的顺序更难保证,而它们本就是同一次
/// 初始化里的两半。
#[derive(Serialize)]
pub struct StartupConfig {
    pub settings: SettingsSnapshot,
    pub state: StartupState,
}

#[tauri::command]
pub async fn get_startup_config(state: State<'_, Arc<AppState>>) -> Result<StartupConfig> {
    // 设置:内存读,零 IO。
    let settings = state.config.snapshot();

    // 内部状态:单次 spawn_blocking 往返(硬约束:rusqlite 同步阻塞不占 UI 线程)。
    let state_arc = Arc::clone(&state);
    let (first_launch, guide_seen) = tokio::task::spawn_blocking(move || -> Result<_> {
        let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
        Ok((
            get_config(&pool, "first_launch")?,
            get_config(&pool, "guide_seen")?,
        ))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    Ok(StartupConfig {
        settings,
        state: StartupState {
            first_launch,
            guide_seen,
        },
    })
}

// ── 内部状态逐键读写(不再承担设置读写)──────────────────────────────────────────

/// 按状态键读取内部状态值。设置类键走 get_settings_snapshot,未知键直接拒绝。
#[tauri::command]
pub async fn get_app_config(
    key: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>> {
    ensure_state_key(&key)?;

    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let pool = state.db_read_pool.get().map_err(AppError::from)?;
        get_config(&pool, &key)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 按状态键写入内部状态值。设置类键走 set_app_settings,未知键直接拒绝。
#[tauri::command]
pub async fn set_app_config(
    key: String,
    value: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    ensure_state_key(&key)?;
    let (k, v) = (key.clone(), value.clone());
    super::blocking::write_blocking(&state, move |c| set_config(c, &k, &v)).await
}

/// 获取解析后的绝对路径缩略图缓存目录。
#[tauri::command]
pub async fn get_thumb_cache_dir(state: State<'_, Arc<AppState>>) -> Result<String> {
    let path = state
        .thumb_config
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .cache_dir
        .clone();
    Ok(path.to_string_lossy().to_string())
}

/// 获取解析后的绝对路径日志目录。
#[tauri::command]
pub async fn get_log_dir(state: State<'_, Arc<AppState>>) -> Result<String> {
    let path = state.log_dir.clone();
    Ok(path.to_string_lossy().to_string())
}

/// 缓存占用统计(各子目录字节 + 总量 + LRU 上限),供设置面板展示(Part3 §3.3.3 / Q8)。
/// 遍历缓存目录是阻塞 IO,走 spawn_blocking。上限键 thumb_cache_max_mb 从 ConfigManager
/// 取(内存读,不必借读池连接查 DB)。
#[tauri::command]
pub async fn get_cache_stats(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::thumbnail::cache::CacheStats> {
    let limit_mb = state
        .config
        .get("thumb_cache_max_mb")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(crate::thumbnail::cache::DEFAULT_THUMB_CACHE_MAX_MB);
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let cache_dir = state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone();
        Ok(crate::thumbnail::cache::compute_cache_stats(
            &cache_dir, limit_mb,
        ))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// ── 外置配置文件支持(打开外部编辑器 / 状态查询)────────────────────────────────

/// get_config_status 的最近一次加载错误。**字段名精确按前端已实现的契约拼写**
/// (message/line,非 camelCase)——本结构体不加 serde 的 rename_all。
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ConfigLastError {
    pub message: String,
    pub line: Option<usize>,
}

/// get_config_status 的返回值。**字段名精确按前端已实现的契约拼写**
/// (path/exists/last_error,last_error 刻意保留下划线、不转 camelCase)。
#[derive(Debug, Serialize)]
pub struct ConfigStatus {
    pub path: String,
    pub exists: bool,
    pub last_error: Option<ConfigLastError>,
}

/// 查询 config.toml 的路径 / 是否存在 / 最近一次加载错误,供设置页展示「打开配置文件」入口与
/// 错误提示。exists() 是阻塞 stat 调用,下沉 spawn_blocking(硬约束:IO 不占 UI 线程)。
#[tauri::command]
pub async fn get_config_status(state: State<'_, Arc<AppState>>) -> Result<ConfigStatus> {
    let config = Arc::clone(&state.config);
    let path = config.path().to_path_buf();
    let path_str = path.to_string_lossy().to_string();
    let last_error = config
        .load_error_status()
        .map(|(message, line)| ConfigLastError { message, line });

    let exists = tokio::task::spawn_blocking(move || path.exists())
        .await
        .unwrap_or(false);

    Ok(ConfigStatus {
        path: path_str,
        exists,
        last_error,
    })
}

/// 用外部编辑器打开 config.toml;若系统没有关联 .toml 的默认程序(open_path 失败),回退为在
/// 文件管理器中显示该文件,方便用户至少能定位到它。两级均失败才返回结构化错误(硬约束:IPC 错误
/// 不得泄漏内部原始字符串,message 不透传上游异常文案)。
///
/// tauri_plugin_opener 的 open_path / reveal_item_in_dir 是**独立于 Tauri IPC/ACL 的自由函数**
/// ——本命令在 Rust 侧直接调用,不需要 webview 一侧的 opener 权限即可工作(D-001:有意不注册
/// opener 插件、不授 capability)。
#[tauri::command]
pub async fn open_config_file(state: State<'_, Arc<AppState>>) -> Result<()> {
    let path = state.config.path().to_path_buf();

    let open_res = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || tauri_plugin_opener::open_path(p, None::<&str>))
            .await
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    };
    if open_res.is_ok() {
        return Ok(());
    }
    tracing::warn!(
        "open_config_file: 打开外部编辑器失败,回退为在文件管理器中显示 | opening external editor failed, falling back to reveal: {}",
        open_res.unwrap_err()
    );

    let reveal_res = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || tauri_plugin_opener::reveal_item_in_dir(&p))
            .await
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
    };
    reveal_res.map(|_| ()).map_err(|e| {
        tracing::warn!(
            "open_config_file: 回退定位也失败 | fallback reveal also failed: {}",
            e
        );
        AppError::Config {
            code: "config_open_failed",
            message: "无法打开或定位配置文件,请手动前往应用数据目录查找 config.toml".into(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SETTING_DEFS;

    // ── ensure_state_key:设置类键与未知键都不许走通用键值命令 ──────────────────────

    /// 阅读器排版、画廊布局、侧栏宽度等偏好自 2026-09-16 起是设置类键:通用键值命令必须拒绝,
    /// 并指向快照/批量入口。
    #[test]
    fn moved_preference_keys_are_rejected_with_snapshot_hint() {
        for key in [
            "doc_reader_font_size",
            "doc_reader_vertical",
            "doc_pager_mode",
            "layout_mode",
            "group_by",
            "sidebar_width",
            "pinned_settings",
            "window_geometry",
        ] {
            assert!(SettingDef::find(key).is_some(), "{key} 应已登记为设置类键");
            match ensure_state_key(key) {
                Err(AppError::Config { code, .. }) => {
                    assert_eq!(code, "config_setting_key_requires_snapshot", "{key}");
                }
                other => panic!("{key} 应被拒并给出快照入口提示,实得 {other:?}"),
            }
        }
    }

    /// 两清单外的键一律以同一稳定 code 拒绝(读与写共用此判定)。
    #[test]
    fn unknown_key_is_rejected_with_stable_code() {
        for key in ["no_such_key", "theme", "bucket_segmented_scroll", ""] {
            assert!(SettingDef::find(key).is_none());
            match ensure_state_key(key) {
                Err(AppError::Config { code, .. }) => assert_eq!(code, "config_unknown_key"),
                other => panic!("期望 Config 稳定 code,得到 {other:?}"),
            }
        }
    }

    /// 内部状态键必须放行——重置设置不会动它们,通用键值命令仍是它们的唯一入口。
    #[test]
    fn internal_state_keys_pass_the_boundary() {
        for key in [
            "schema_version",
            "last_directory_id",
            "first_launch",
            "guide_seen",
            "ai_gpu_name",
            "ai_provider",
            "exotic_paused",
            "ai_analysis_active",
            "face_analysis_active",
            "derivation_active",
            "backup_last_success_at",
            "last_cover_stat_reconcile",
        ] {
            assert!(ensure_state_key(key).is_ok(), "{key} 是已知状态类键");
        }
    }

    /// schema 设置类键集合与状态类键集合互斥(与 schema.rs 的同名不变量互为独立复核:
    /// 两处都断言,才能防「改一边忘改另一边」)。
    #[test]
    fn routing_schema_and_state_keys_are_mutually_exclusive() {
        for def in SETTING_DEFS {
            assert!(
                !STATE_KEYS.contains(&def.key),
                "{} 同时被判定为设置键与状态键,路由会产生歧义",
                def.key
            );
        }
    }

    /// 状态键清单里不得混入用户偏好:重置设置之所以不动状态,前提是状态里没有设置项。
    #[test]
    fn state_key_list_holds_no_user_preferences() {
        for key in STATE_KEYS {
            assert!(
                !key.starts_with("doc_"),
                "{key} 是用户偏好,应作为设置类键登记"
            );
            assert!(!key.starts_with("player_"), "{key} 是用户偏好");
        }
    }

    // ── get_config_status 序列化字段名快照 ──────────────────────────────────────

    /// 锁住 get_config_status 的 IPC 字段拼写——前端已按此实现,任何一处误改字段名/加
    /// camelCase 转换都会静默破坏前端消费。
    #[test]
    fn config_status_serializes_with_contract_field_names() {
        let status = ConfigStatus {
            path: "C:/x/config.toml".to_string(),
            exists: true,
            last_error: Some(ConfigLastError {
                message: "示例错误".to_string(),
                line: Some(7),
            }),
        };
        let v = serde_json::to_value(&status).unwrap();
        assert_eq!(v["path"], "C:/x/config.toml");
        assert_eq!(v["exists"], true);
        assert_eq!(v["last_error"]["message"], "示例错误");
        assert_eq!(v["last_error"]["line"], 7);
        assert!(v.get("lastError").is_none());
    }

    #[test]
    fn config_status_serializes_null_last_error_when_absent() {
        let status = ConfigStatus {
            path: "C:/x/config.toml".to_string(),
            exists: false,
            last_error: None,
        };
        let v = serde_json::to_value(&status).unwrap();
        assert_eq!(v["last_error"], serde_json::Value::Null);
    }

    // ── 启动入口字段名快照(前端启动水合直接读这些名字)────────────────────────────

    /// settings 是快照本体,state 只带两个启动内部状态;state 的两个字段是 camelCase
    /// (与前端既有 firstLaunch/guideSeen 读法一致),快照三字段保持原样拼写。
    #[test]
    fn startup_config_serializes_settings_and_state_shapes() {
        let payload = StartupConfig {
            settings: SettingsSnapshot {
                values: BTreeMap::from([("language".to_string(), "zh-CN".to_string())]),
                revision: 3,
                generation: 1,
            },
            state: StartupState {
                first_launch: Some("false".to_string()),
                guide_seen: None,
            },
        };
        let v = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["settings"]["values"]["language"], "zh-CN");
        assert_eq!(v["settings"]["revision"], 3);
        assert_eq!(v["settings"]["generation"], 1);
        assert_eq!(v["state"]["firstLaunch"], "false");
        assert!(v["state"]["guideSeen"].is_null());
        // 快照三字段不带 camelCase(键名本身即全小写单词)。
        assert!(v["settings"].get("snapshot").is_none());
    }
}
