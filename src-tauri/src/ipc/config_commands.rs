//! 应用配置键值命令（§ 6.1 — config）。
//!
//! A2(配置文件重构接线):61 个设置类键(schema,`config::SETTING_DEFS`)的唯一真源已切到
//! `<app_data_dir>/config.toml`(`ConfigManager`);18 个状态类键(`config::STATE_KEYS`,
//! 应用自己记账、非用户表单可编辑)与「两清单外的未知键」仍走 SQLite `app_config` 表——
//! 三路路由（schema 键 / 状态键 / 未知键）集中在 `set_app_config`/`get_app_config`,
//! `get_startup_config`/`get_cache_stats` 按各自字段/单键分别路由到对应真源。

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::config::{SettingDef, SettingKind, STATE_KEYS};
use crate::db::queries::{get_config, set_config};
use crate::error::{AppError, Result};
use crate::state::AppState;
use std::sync::OnceLock;
use tracing_subscriber::{reload::Handle, EnvFilter, Registry};

pub static LOG_RELOAD: OnceLock<Handle<EnvFilter, Registry>> = OnceLock::new();

/// 根据键获取配置值。
///
/// A2:schema 设置类键 → `ConfigManager`(内存 `RwLock` 读,零 IO,无需 spawn_blocking);
/// 其余(状态类 / 未知键)→ DB 原路径不变(spawn_blocking,r2d2 同步阻塞,CLAUDE.md 硬约束)。
#[tauri::command]
pub async fn get_app_config(
    key: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>> {
    if SettingDef::find(&key).is_some() {
        return Ok(state.config.get(&key));
    }

    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let pool = state.db_read_pool.get().map_err(AppError::from)?;
        get_config(&pool, &key)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 前端启动时所需的所有配置，通过单次 SQLite 往返批量获取。
/// 替代 App.vue onMounted 里 4 次独立 get_app_config IPC，将开销从
/// N×（序列化 + Tokio 调度 + 连接池获取 + SQLite + 反序列化）
/// 降低到 1×相同开销 + N×SQLite 行读取(可忽略,同一连接)。
///
/// R2-4(2026-07-02):扩容为 14 键——uiStore 的 9 项模块初始化配置与 App.vue 的
/// first_launch 一并并入,整个启动阶段的配置 IPC 由 11 次归 1 次。
///
/// A2:结构体字段不变(前端零感知)——28 个字段改从 `ConfigManager` 取(schema 设置类),
/// 剩下 5 个状态类字段(`group_by`/`sort_within_group`/`layout_mode`/`pinned_settings`/
/// `first_launch`)仍走 DB。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupConfig {
    pub language: Option<String>,
    pub timeline_scroll_width: Option<String>,
    // 时间轴轴宽(.timeline-sidebar 列宽,与滚动条 thumb 宽 timeline_scroll_width 相互独立)。
    pub timeline_axis_width: Option<String>,
    // 滚动条 thumb / 时间轴视窗共享的最小高度(px);二者同高。
    pub scroll_thumb_min_height: Option<String>,
    pub ui_font_size: Option<String>,
    pub enable_thumb_hover_scale: Option<String>,
    // R2-4:uiStore 9 项模块初始化配置 + first_launch(App.vue 首启检测)。
    pub grid_row_height: Option<String>,
    pub group_by: Option<String>,
    pub sort_within_group: Option<String>,
    pub layout_mode: Option<String>,
    pub close_behavior: Option<String>,
    pub pinned_settings: Option<String>,
    pub show_thumb_info: Option<String>,
    pub thumb_info_elements: Option<String>,
    pub hover_autoplay: Option<String>,
    // T16 方案B(2026-07-04):bucket 分段虚拟滚动开关(B1)→ 共 15 键。
    pub bucket_segmented_scroll: Option<String>,
    // 多主题 S1(2026-07-06):外观三键(appearance/theme_light/theme_dark)+ legacy
    // theme(仅迁移读取,新版本不再写)→ 共 19 键。原 useTheme 独立的 get_app_config('theme')
    // 并入本批,启动配置 IPC 保持 1 次(R2-4 纪律)。
    pub theme: Option<String>,
    pub appearance: Option<String>,
    pub theme_light: Option<String>,
    pub theme_dark: Option<String>,
    pub first_launch: Option<String>,
    pub guide_seen: Option<String>,
    // 文件树显示范围(S 线 §3):registeredOnly | allFiles | allFilesWithHidden → 共 20 键。
    // 并入本批而非走 configStore 的惰性读:该值在**首次展开目录前**就必须到位,晚到会让第一次
    // 展开走错取数路径(虽有模式 watch 兜底重载,但那是白跑一趟且会丢展开态)。
    pub tree_display_mode: Option<String>,
    // 选择态缩略图拖拽手柄显隐(2026-07-17 #5)→ 共 21 键。
    pub show_drag_handle: Option<String>,
    // 无缝分组(2026-07-17 #1)→ 共 22 键。
    pub seamless_groups: Option<String>,
    // 无缝 minimap 轴显隐(2026-07-17)+ 渲染模式(2026-07-19)→ 共 24 键。
    pub seamless_minimap: Option<String>,
    pub minimap_render_mode: Option<String>,
    // 日志能力重构 S3(2026-07-20,reviewer 深审揪出):前端 logger.ts 的 off 档联动此前只在
    // configStore.loadConfig()(设置页/模型库懒路由才调)里同步,导致「off 时前端不白付 IPC」
    // 在典型会话里从未生效——并入本批,与其余 24 键同一次往返内到位 → 共 25 键。
    pub log_level: Option<String>,
    // 批次C:5 个前端阈值键(advanced,hot——config-file-changed 后 refreshFromBackend 已会重拉,
    // 消费点从 uiStore 响应式读取即时生效,不必等下次启动)→ 共 30 键。**只增字段不改旧字段**
    // (硬约束):前端类型同步新增,旧字段位置/类型不变。
    pub heavy_video_max_pixels: Option<String>,
    pub heavy_video_max_bytes: Option<String>,
    pub hover_delay_ms: Option<String>,
    pub search_debounce_ms: Option<String>,
    pub resize_debounce_ms: Option<String>,
    // 小批 C2(2026-07-22):雪碧图切帧列数须与提取帧数同源,否则用户改 video_keyframe_count 后
    // 前端悬停 scrub 仍按旧硬编码列数切新雪碧图 → 画面错位。经启动配置下发,响应式消费点见
    // useHoverPreview.ts。**只增字段不改旧字段**(硬约束,同批次C惯例)→ 共 31 键。
    pub video_keyframe_count: Option<String>,
    // 窗口化沉浸模式(2026-07-23):非全屏时自动隐藏顶栏/底栏,鼠标移边缘唤出 → 共 32 键。
    pub auto_hide_chrome_windowed: Option<String>,
    // 轴视窗不透明度缩放(2026-07-24):时间轴/minimap 半透明拖动视窗,百分比,100=默认。
    // 启动即应用 CSS 变量(与 timeline_axis_width 同路径),不能等设置页懒加载 → 共 33 键。
    pub axis_viewport_opacity: Option<String>,
    // 轴形态偏好(2026-07-24 画廊轴/minimap 重构):timeline|minimap,两分组模式通用,
    // 持久化替代原会话态 preferredAxis → 共 34 键。
    pub axis_mode: Option<String>,
    // 窗口材质(毛玻璃)(2026-08-24):mica|acrylic|none,DWM 背板仅 Windows 生效;
    // 前端 uiStore 水合白名单消费(Rust 侧由 apply_setting_effects 分支同步翻转)→ 共 35 键。
    pub window_material: Option<String>,
    // 毛玻璃分层不透明度缩放(2026-08-25):100=各材质当前默认观感,前端 CSS 即时消费→ 共 40 键。
    pub glass_chrome_opacity: Option<String>,
    pub glass_sticky_opacity: Option<String>,
    pub glass_surface_opacity: Option<String>,
    pub glass_control_opacity: Option<String>,
    // 内容底面缩放(2026-09-06):文字密集视图根的 --glass-content-fill 承重面,20–120。
    // 120 起 color-mix 钳到全不透明(预期行为:调大=更实)→ 共 41 键。
    pub glass_content_opacity: Option<String>,
    pub glass_gallery_opacity: Option<String>,
    // 主题色浓度(2026-09-06):底色 wash token 的 color-mix 缩放,20–100,100=满浓度锚点
    // → 共 42 键。
    pub theme_tint_strength: Option<String>,
    // 文字浓度(2026-09-06):文字 ramp 的 color-mix 缩放,40–100,100=满浓度出厂文字色
    // → 共 43 键。
    pub theme_text_strength: Option<String>,
}

#[tauri::command]
pub async fn get_startup_config(state: State<'_, Arc<AppState>>) -> Result<StartupConfig> {
    // schema 设置类字段:ConfigManager 内存读,零 IO,不必进 spawn_blocking。
    let language = state.config.get("language");
    let timeline_scroll_width = state.config.get("timeline_scroll_width");
    let timeline_axis_width = state.config.get("timeline_axis_width");
    let scroll_thumb_min_height = state.config.get("scroll_thumb_min_height");
    let ui_font_size = state.config.get("ui_font_size");
    let enable_thumb_hover_scale = state.config.get("enable_thumb_hover_scale");
    let grid_row_height = state.config.get("grid_row_height");
    let close_behavior = state.config.get("close_behavior");
    let show_thumb_info = state.config.get("show_thumb_info");
    let thumb_info_elements = state.config.get("thumb_info_elements");
    let hover_autoplay = state.config.get("hover_autoplay");
    let bucket_segmented_scroll = state.config.get("bucket_segmented_scroll");
    let theme = state.config.get("theme");
    let appearance = state.config.get("appearance");
    let theme_light = state.config.get("theme_light");
    let theme_dark = state.config.get("theme_dark");
    let tree_display_mode = state.config.get("tree_display_mode");
    let show_drag_handle = state.config.get("show_drag_handle");
    let seamless_groups = state.config.get("seamless_groups");
    let seamless_minimap = state.config.get("seamless_minimap");
    let minimap_render_mode = state.config.get("minimap_render_mode");
    let log_level = state.config.get("log_level");
    // 批次C:5 个前端阈值键(advanced,同上——ConfigManager 内存读)。
    let heavy_video_max_pixels = state.config.get("heavy_video_max_pixels");
    let heavy_video_max_bytes = state.config.get("heavy_video_max_bytes");
    let hover_delay_ms = state.config.get("hover_delay_ms");
    let search_debounce_ms = state.config.get("search_debounce_ms");
    let resize_debounce_ms = state.config.get("resize_debounce_ms");
    // 小批 C2:雪碧图切帧列数下发(见 StartupConfig::video_keyframe_count 字段注释)。
    let video_keyframe_count = state.config.get("video_keyframe_count");
    let auto_hide_chrome_windowed = state.config.get("auto_hide_chrome_windowed");
    let axis_viewport_opacity = state.config.get("axis_viewport_opacity");
    let axis_mode = state.config.get("axis_mode");
    let window_material = state.config.get("window_material");
    let glass_chrome_opacity = state.config.get("glass_chrome_opacity");
    let glass_sticky_opacity = state.config.get("glass_sticky_opacity");
    let glass_surface_opacity = state.config.get("glass_surface_opacity");
    let glass_control_opacity = state.config.get("glass_control_opacity");
    let glass_content_opacity = state.config.get("glass_content_opacity");
    let glass_gallery_opacity = state.config.get("glass_gallery_opacity");
    let theme_tint_strength = state.config.get("theme_tint_strength");
    let theme_text_strength = state.config.get("theme_text_strength");

    // 状态类字段(§ schema.rs STATE_KEYS):仍走 DB,单次 spawn_blocking 往返。
    let state_arc = Arc::clone(&state);
    let (group_by, sort_within_group, layout_mode, pinned_settings, first_launch, guide_seen) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            Ok((
                get_config(&pool, "group_by")?,
                get_config(&pool, "sort_within_group")?,
                get_config(&pool, "layout_mode")?,
                get_config(&pool, "pinned_settings")?,
                get_config(&pool, "first_launch")?,
                get_config(&pool, "guide_seen")?,
            ))
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    Ok(StartupConfig {
        language,
        timeline_scroll_width,
        timeline_axis_width,
        scroll_thumb_min_height,
        ui_font_size,
        enable_thumb_hover_scale,
        grid_row_height,
        group_by,
        sort_within_group,
        layout_mode,
        close_behavior,
        pinned_settings,
        show_thumb_info,
        thumb_info_elements,
        hover_autoplay,
        bucket_segmented_scroll,
        theme,
        appearance,
        theme_light,
        theme_dark,
        first_launch,
        guide_seen,
        tree_display_mode,
        show_drag_handle,
        seamless_groups,
        seamless_minimap,
        minimap_render_mode,
        log_level,
        heavy_video_max_pixels,
        heavy_video_max_bytes,
        hover_delay_ms,
        search_debounce_ms,
        resize_debounce_ms,
        video_keyframe_count,
        auto_hide_chrome_windowed,
        axis_viewport_opacity,
        axis_mode,
        window_material,
        glass_chrome_opacity,
        glass_sticky_opacity,
        glass_surface_opacity,
        glass_control_opacity,
        glass_content_opacity,
        glass_gallery_opacity,
        theme_tint_strength,
        theme_text_strength,
    })
}

/// A2:校验 IPC 传入的原始字符串值是否匹配 `SettingKind`,合法则返回规范文本形式(与
/// `SettingDef.default`/`ConfigManager::get` 同型,可直接喂 `set_and_persist`)。
///
/// 与 `config::file::validate_value` 平行而不复用:后者校验的是**已解析的 TOML `Item`**
/// (config.toml 语法层),本函数校验的是**IPC 传入的裸字符串**(前端 `invoke` 参数,不带
/// TOML 引号/字面量语法)——两者输入形态不同,強行合一反而要在调用点现造 `toml_edit::Item`
/// 包一层,得不偿失。
fn validate_ipc_value(kind: SettingKind, raw: &str) -> std::result::Result<String, String> {
    match kind {
        SettingKind::Bool => match raw {
            "true" => Ok("true".to_string()),
            "false" => Ok("false".to_string()),
            _ => Err("应为 true 或 false".to_string()),
        },
        SettingKind::UInt => raw
            .parse::<u64>()
            .map(|n| n.to_string())
            .map_err(|_| "应为非负整数".to_string()),
        SettingKind::Float => raw
            .parse::<f64>()
            .map(|f| f.to_string())
            .map_err(|_| "应为数字".to_string()),
        SettingKind::Str | SettingKind::Path => Ok(raw.to_string()),
        SettingKind::Enum(options) => {
            if options.contains(&raw) {
                Ok(raw.to_string())
            } else {
                Err(format!(
                    "取值 \"{raw}\" 不在允许范围内,允许值:{}",
                    options.join(" / ")
                ))
            }
        }
    }
}

/// 设置配置值。
///
/// A2 三路路由:
/// 1. schema 设置类键(`SETTING_DEFS`)→ 按 `SettingKind` 校验 → `ConfigManager::set_and_persist`
///    (toml_edit 就地改写、保留用户注释/排版,原子写盘)→ `apply_setting_effects`(热应用)。
/// 2. 状态类键(`STATE_KEYS`)→ DB 原路径不变。
/// 3. 两清单外的未知键 → DB 原路径 + warn 日志(兜 A1 枚举误判——历史代码可能存在未被 A1
///    穷举扫描到的键,拒绝写入代价太大,故仍放行,只是记录以便事后核实该键是否漏归类)。
#[tauri::command]
pub async fn set_app_config(
    app: AppHandle,
    key: String,
    value: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    if let Some(def) = SettingDef::find(&key) {
        let normalized = validate_ipc_value(def.kind, &value).map_err(|msg| AppError::Config {
            code: "config_invalid_value",
            message: msg,
        })?;

        let state_for_write = Arc::clone(&state);
        let key_for_write = key.clone();
        let value_for_write = normalized.clone();
        tokio::task::spawn_blocking(move || {
            state_for_write
                .config
                .set_and_persist(&key_for_write, &value_for_write)
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

        apply_setting_effects(&app, &state, &key, &normalized).await?;
        return Ok(());
    }

    if !STATE_KEYS.contains(&key.as_str()) {
        tracing::warn!(
            key = %key,
            "set_app_config: 键既不在 schema 设置类清单也不在已知状态类清单,按状态类兜底写 DB(请核实是否漏归类) | unrecognized config key, falling back to DB path"
        );
    }

    let (k, v) = (key.clone(), value.clone());
    super::blocking::write_blocking(&state, move |c| set_config(c, &k, &v)).await
}

/// A2:单键「持久化后」的热应用副作用——从原 `set_app_config` 抽出,`set_app_config`(UI 发起)
/// 与 config.toml watcher 回调(外部编辑发起)共用同一份逻辑,行为逐字保留。**只处理 schema
/// 设置类键**(状态类键从无这类热应用副作用,原实现的 match 分支恰好与 schema 键集重合)。
pub async fn apply_setting_effects(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    key: &str,
    value: &str,
) -> Result<()> {
    // 跟踪是否有缩略图配置键的变更需要重新评估直接显示项（thumb_status=3）。
    let mut needs_thumb_reset = false;

    if key == "thumb_skip_max_kb" {
        if let Ok(val) = value.parse::<u64>() {
            let mut config = state
                .thumb_config
                .write()
                .unwrap_or_else(|e| e.into_inner());
            config.skip_max_bytes = val * 1024;
            needs_thumb_reset = true;
        }
    } else if key == "thumb_size" {
        if let Ok(val) = value.parse::<u32>() {
            let changed = {
                let mut config = state
                    .thumb_config
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                let changed = config.size != val;
                config.size = val;
                changed
            };
            // 档位失效(2026-07-06 审查 R1):生成器 CACHE_HIT 只看 status=1+文件存在、不含档位,
            // 不重置则存量图**永远**端出旧档缓存,设置项对存量库形同虚设(exotic 项经指纹校验
            // 按全局档位自愈,主管线须在此显式失效)。thumbhash 与档位无关,保留避免占位闪烁;
            // 旧档文件留在 thumbnails/ 下由 LRU 淘汰回收。
            // 2026-07-10 补(深审 defer ⑧,对齐 thumb_webp_quality 分支):封面派生行必须一并
            // 复位——只退 media_items 的话,video/audio/epub 封面会被主生成器打成
            // UNSUPPORTED_TYPE,而派生行仍 done 不重跑,下次启动 reconcile 又把**旧档位**封面
            // 镜像回来,即换档位对封面类永远不生效(同 reset_cover_thumbs_for_regen 的教训)。
            if changed {
                let (items, covers) = reset_completed_thumbnail_rows(state).await?;
                tracing::info!(
                    "[Config] thumb_size changed → reset {} done items + {} cover derivations to pending | 档位变更，重置 {} 个已完成项与 {} 个封面派生待重生成",
                    items, covers, items, covers
                );
                // 失效三件套(同 clear_all_thumbnails,P1-7):items 快照携带 thumb_status/path。
                *state
                    .layout_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                crate::layout::items_cache::invalidate(&state.layout_items_cache);
                state.bump_data_version();
            }
        }
    } else if key == "thumb_webp_quality" {
        if let Ok(val) = value.parse::<u8>() {
            let val = val.clamp(1, 100); // 1..=99 有损,100=无损(encode_as_webp 契约)
            let changed = {
                let mut config = state
                    .thumb_config
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                let changed = config.webp_quality != val;
                config.webp_quality = val;
                changed
            };
            // 质量变更同档位变更(2026-07-06 审查 R1 教训:设置项对存量库不得形同虚设):
            // 已生成项(status=1)重置待重生成,按需以新质量重编码。与 thumb_size 不同的是
            // 这里**连封面派生行一并复位**(video/audio/epub 封面也是显示缩略图,只退
            // media_items 会被主生成器打成 UNSUPPORTED_TYPE、而派生行仍 done 不重跑——
            // 同 reset_cover_thumbs_for_regen 的教训)。旧文件留 thumbnails/ 由 LRU 回收;
            // 直显(3)/失败(2)项与编码质量无关,不动。
            if changed {
                let (items, covers) = reset_completed_thumbnail_rows(state).await?;
                tracing::info!(
                    "[Config] thumb_webp_quality → {} · reset {} done items + {} cover derivations to pending | 编码质量变更，重置 {} 个已完成项与 {} 个封面派生待重生成",
                    val, items, covers, items, covers
                );
                // 失效三件套(同 thumb_size,P1-7):items 快照携带 thumb_status/path。
                *state
                    .layout_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                crate::layout::items_cache::invalidate(&state.layout_items_cache);
                state.bump_data_version();
            }
        }
    } else if key == "thumb_cache_dir" {
        let new_cache_dir = std::path::PathBuf::from(value);
        let mut config = state
            .thumb_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        config.cache_dir = new_cache_dir.clone();
        std::fs::create_dir_all(&config.cache_dir).unwrap_or_default();
        if let Err(e) = app
            .asset_protocol_scope()
            .allow_directory(&new_cache_dir, true)
        {
            tracing::warn!(
                "Failed to allow updated cache_dir in asset scope | 更新后的缓存目录授权失败: {}",
                e
            );
        }
    } else if key == "thumb_strategy" {
        let mut config = state
            .thumb_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        config.strategy = value.to_string();
        needs_thumb_reset = true;
    } else if key == "gpu_engine" {
        let mut config = state
            .thumb_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        config.gpu_engine = value.to_string();
    } else if key == "ai_hq_cache_enabled" {
        // 让 AI 高清缓存开关运行时即时生效：缩略图流水线据此决定是否顺带产出 AI 缓存（无需重启）。
        let mut config = state
            .thumb_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        config.ai_hq_cache = value == "true";
    } else if key == "ai_cache_short_edge" {
        // 批次C:同 thumb_webp_quality 惯例,运行时即时生效——下一次 ai_hq_cache 路径的解码/
        // 编码即用新短边,无需重启;不触发存量缓存失效(与 thumb_size/webp_quality 不同,旧短边
        // 缓存仍是合法的「更小/更大」AI 缓存,分析侧本就按需下采样,不因短边变化而读出错误结果)。
        if let Ok(val) = value.parse::<u32>() {
            let mut config = state
                .thumb_config
                .write()
                .unwrap_or_else(|e| e.into_inner());
            config.ai_cache_short_edge = val;
        }
    } else if key == "log_level" {
        let directive = crate::logging::build_env_filter_directive(value);
        if let Some(handle) = LOG_RELOAD.get() {
            if let Ok(filter) = EnvFilter::try_new(&directive) {
                if let Err(e) = handle.modify(|f| *f = filter) {
                    tracing::warn!("Failed to reload log level: {}", e);
                }
            }
        }
        tracing::info!("Log level dynamically updated to: {}", value);
    } else if key == "window_material" {
        // 窗口材质热切换:show 后重挂(tauri#12854)之外的第三条应用路径,前端 uiStore 经
        // config-file-changed 同一时刻翻转 data-glass,两侧经同一键值天然同拍。
        crate::window_material::apply(app, value);
    }

    // 当跳过阈值或策略变更时，之前标记为"直接显示"（status=3）的项
    // 可能现在需要真正的缩略图。将它们重置为待处理（status=0），
    // 以便按需生成机制重新处理它们。
    if needs_thumb_reset {
        let affected = reset_direct_thumbnail_rows(state).await?;
        tracing::info!(
            "[Config] thumb config changed (key={}), reset {} direct-display items to pending | 缩略图配置变更，重置 {} 个直接显示项为待处理",
            key, affected, affected
        );
        // 清空布局缓存，使 compute_layout 从 DB 读取最新状态
        *state
            .layout_cache
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    Ok(())
}

/// 复位已生成的缩略图时撤销在途 worker，并让 reset 与旧结果的最终写入共享同一
/// lifecycle 写区。文件/数据库之外的 async 边界不持有任何 std 锁。
async fn reset_completed_thumbnail_rows(
    state: &State<'_, Arc<AppState>>,
) -> Result<(usize, usize)> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_thumbnail_reset_exclusive(|| -> Result<(usize, usize)> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let tx = conn.unchecked_transaction()?;
            let items = tx.execute(
                "UPDATE media_items SET thumb_status = 0, thumb_path = NULL \
                 WHERE thumb_status = 1 AND is_deleted = 0",
                [],
            )?;
            let covers = tx.execute(
                "UPDATE media_derivations SET status = 0, payload_path = NULL \
                 WHERE status = 2 AND kind IN ('video_cover','audio_cover','doc_thumb')",
                [],
            )?;
            tx.commit()?;
            Ok((items, covers))
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 跳过阈值/策略改变只需复位 direct-display 项，但同样必须撤销旧 worker，避免旧
/// 结果在新配置生效后重新写回。
async fn reset_direct_thumbnail_rows(state: &State<'_, Arc<AppState>>) -> Result<usize> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_thumbnail_reset_exclusive(|| -> Result<usize> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Ok(conn.execute(
                "UPDATE media_items SET thumb_status = 0, thumb_path = NULL, thumbhash = NULL \
                 WHERE thumb_status = 3 AND is_deleted = 0",
                [],
            )?)
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
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

/// 缓存占用统计（各子目录字节 + 总量 + LRU 上限），供设置面板展示（Part3 §3.3.3 / Q8）。
/// 遍历缓存目录是阻塞 IO，走 spawn_blocking。A2:上限键 `thumb_cache_max_mb` 改从
/// `ConfigManager` 取(内存读,不必再借读池连接查 DB)。
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

/// 手动清理缓存子集（`kind` ∈ thumbnails/ai/sprites/motion/audio/all），返回释放字节数（§3.3.3 / Q8）。
/// 删磁盘产物后**同步退回对应 DB 状态**(2026-07-06 审查 P1-4):派生行 status=2 而文件已删会
/// 导致读取侧返回死路径且永不重建,故按 kind 映射复位 derivation 行 + thumb_status。删除是阻塞
/// IO，走 spawn_blocking。
#[tauri::command]
pub async fn clear_cache(kind: String, state: State<'_, Arc<AppState>>) -> Result<u64> {
    let state = Arc::clone(&state);
    let state_for_wake = Arc::clone(&state);
    let freed = tokio::task::spawn_blocking(move || -> Result<u64> {
        let (deriv_kinds, reset_thumb) =
            crate::thumbnail::cache::derivations_to_reset_for_kind(&kind);
        let clear = || -> Result<u64> {
            let cache_dir = state
                .thumb_config
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .cache_dir
                .clone();
            let freed = crate::thumbnail::cache::clear_cache_kind(&cache_dir, &kind);

            // 删文件后按 kind 映射退回 DB 状态,使生成/派生流水线按需重建。此处只在
            // writer 锁短事务中执行数据库动作，缓存目录递归删除已经完成且不持锁。
            {
                let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                if !deriv_kinds.is_empty() {
                    crate::db::queries::reset_derivations_by_kinds(&conn, deriv_kinds)?;
                }
                if reset_thumb {
                    conn.execute(
                        "UPDATE media_items SET thumb_status = 0, thumb_path = NULL \
                         WHERE thumb_status = 1 AND is_deleted = 0",
                        [],
                    )
                    .map_err(AppError::Db)?;
                }
            }
            if reset_thumb {
                // items 快照携带 thumb_status/path,须失效三件套(同 clear_all_thumbnails,P1-7)。
                *state
                    .layout_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                crate::layout::items_cache::invalidate(&state.layout_items_cache);
                state.bump_data_version();
            }
            Ok(freed)
        };

        if reset_thumb {
            // thumbnails/all 会影响 full worker 的结果；先撤销 generation，再覆盖文件和 DB。
            state.with_thumbnail_reset_exclusive(clear)
        } else {
            clear()
        }
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // 唤醒 Coordinator 重做被退回 pending 的派生/exotic 任务。
    state_for_wake.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
    Ok(freed)
}

// ── A2:config.toml 外部编辑支持(打开外部编辑器 / 状态查询)────────────────────────

/// `get_config_status` 的最近一次加载错误。**字段名精确按前端已实现的契约拼写**
/// (`message`/`line`,非 camelCase)——本结构体不加 `#[serde(rename_all = "camelCase")]`。
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ConfigLastError {
    pub message: String,
    pub line: Option<usize>,
}

/// `get_config_status` 的返回值。**字段名精确按前端已实现的契约拼写**
/// (`path`/`exists`/`last_error`,`last_error` 刻意保留下划线、不转 camelCase)。
#[derive(Debug, Serialize)]
pub struct ConfigStatus {
    pub path: String,
    pub exists: bool,
    pub last_error: Option<ConfigLastError>,
}

/// 查询 config.toml 的路径 / 是否存在 / 最近一次加载错误,供设置页展示「打开配置文件」
/// 入口与错误提示。`exists()` 是阻塞 stat 调用,下沉 spawn_blocking(硬约束:IO 不占 UI 线程)。
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

/// 用外部编辑器打开 config.toml;若系统没有关联 `.toml` 的默认程序(`open_path` 失败),
/// 回退为在文件管理器中显示该文件,方便用户至少能定位到它。两级均失败才返回结构化错误
/// (硬约束:IPC 错误不得泄漏内部原始字符串,message 不透传上游异常文案)。
///
/// `tauri_plugin_opener::open_path`/`reveal_item_in_dir` 是**独立于 Tauri IPC/ACL 的自由
/// 函数**(不经插件命令层,见 `ipc::reveal::reveal_path` 同款用法)——本命令在 Rust 侧直接
/// 调用,不需要 webview 一侧的 opener 权限即可工作(D-001:有意不注册 opener 插件、不授
/// capability,见 `Cargo.toml` 依赖声明处注记)。
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

    // ── validate_ipc_value ───────────────────────────────────────────────────

    #[test]
    fn validate_ipc_value_accepts_canonical_bool_text() {
        assert_eq!(
            validate_ipc_value(SettingKind::Bool, "true"),
            Ok("true".to_string())
        );
        assert_eq!(
            validate_ipc_value(SettingKind::Bool, "false"),
            Ok("false".to_string())
        );
        assert!(validate_ipc_value(SettingKind::Bool, "1").is_err());
        assert!(validate_ipc_value(SettingKind::Bool, "yes").is_err());
    }

    #[test]
    fn validate_ipc_value_rejects_negative_or_non_numeric_uint() {
        assert!(validate_ipc_value(SettingKind::UInt, "-1").is_err());
        assert!(validate_ipc_value(SettingKind::UInt, "abc").is_err());
        assert_eq!(
            validate_ipc_value(SettingKind::UInt, "512"),
            Ok("512".to_string())
        );
    }

    #[test]
    fn validate_ipc_value_enforces_enum_membership() {
        let kind = SettingKind::Enum(&["a", "b"]);
        assert_eq!(validate_ipc_value(kind, "a"), Ok("a".to_string()));
        assert!(validate_ipc_value(kind, "c").is_err());
    }

    // ── 路由分类穷举(A2 任务清单 §8):schema 键 ∪ 状态键 ∪ 未知键三路互斥 ──────────────

    /// schema 设置类键集合与状态类键集合互斥(与 schema.rs 的 `state_keys_and_setting_defs_are_disjoint`
    /// 同一不变量,这里从 IPC 路由视角复核一次——两处独立断言比单处更能防「改一边忘改另一边」)。
    #[test]
    fn routing_schema_and_state_keys_are_mutually_exclusive() {
        for def in SETTING_DEFS {
            assert!(
                !STATE_KEYS.contains(&def.key),
                "{} 同时被判定为 schema 键与状态键,路由会产生歧义",
                def.key
            );
        }
    }

    /// 任取若干「两清单外」的键,路由判定既非 schema 也非已知状态键——三路穷举里第三路
    /// (未知键 + warn)的判定条件本身正确(`!SettingDef::find` && `!STATE_KEYS.contains`)。
    #[test]
    fn routing_recognizes_truly_unknown_keys() {
        for made_up in ["totally_made_up_key", "ai_backend", "thumb_format"] {
            assert!(
                SettingDef::find(made_up).is_none(),
                "{made_up} 不该出现在 schema 键集"
            );
            assert!(
                !STATE_KEYS.contains(&made_up),
                "{made_up} 不该出现在状态键集"
            );
        }
    }

    // ── get_config_status 序列化字段名快照 ──────────────────────────────────────

    /// 锁住 `get_config_status` 的 IPC 字段拼写——前端已按此实现,任何一处误改字段名/加
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
        // 契约字段名不带 camelCase(拼写精确为 last_error,非 lastError)。
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
}
