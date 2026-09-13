//! 设置类键定义单源(A1)。新增/改名/调整任何用户可配置的设置项,只改本文件——
//! `file.rs` 的校验/渲染与 `migrate.rs` 的一次性迁移都从 `SETTING_DEFS` 派生,不重复枚举键名。
//!
//! # 键全集的来源与口径(穷举方法论,供后续维护者复核)
//! 本表是对代码实际读写路径穷举后的结果,并集自三处:
//! 1. `db/schema.rs` 各版本 `INSERT OR IGNORE INTO app_config` 的种子键;
//! 2. `ipc/config_commands.rs::set_app_config` 里带热应用副作用的 match 分支;
//! 3. `ipc/config_commands.rs::get_startup_config` / `StartupConfig` 返回的键。
//!
//! 在此之上按代码实际 `get_config`/`set_config` 调用点(含前端 `saveConfig`/`fetchStr` 等
//! 泛化 IPC 调用里的字面量键)做了两项修正,均在本文件对应条目的注释里可查:
//! - **剔除死键**:`thumb_format`/`thumb_quality`/`ai_enabled`/`ai_auto_analyze`/
//!   `clip_model`/`face_auto_analyze`/`exotic_max_workers`/`ai_backend` 仅在种子表出现,
//!   全仓无任何 `get_config` 读取点消费——写进 schema 会让用户以为改了会生效,实际无效果,
//!   比不收录更误导人,故不收录(`ai_backend` 额外被 `ai/runtime_config.rs` 读到但只用于
//!   打印一条"该值已退役"的日志,不驱动任何行为,同归此类)。
//! - **状态类键的补充排除**:任务给定的排除清单(schema_version 等 12 项)之外,又找到 6 个
//!   同属"应用自己记账、非用户表单可编辑"的键,按同一判据补充排除,理由随每项列在下方
//!   "排除清单"小节:`ai_provider`(worker 会话回声,写者与 `ai_gpu_name` 同一函数
//!   `persist_provider_echo`)、`ai_analysis_active`/`face_analysis_active`/
//!   `derivation_active`(启停时置位的"期望运行"标志,与已排除的 `exotic_paused` 同构)、
//!   `backup_last_success_at`/`last_cover_stat_reconcile`(后台任务自记的时间戳,无表单)。
//!
//! # 排除清单(状态类,留 DB `app_config` 表,不进本 schema)
//! 给定 12 项:`schema_version` `last_directory_id` `last_sort_by` `last_sort_order`
//! `sidebar_width` `first_launch` `pinned_settings` `ai_gpu_name` `exotic_paused`
//! `layout_mode` `group_by` `sort_within_group`。
//! 补充 6 项(理由见上一节):`ai_provider` `ai_analysis_active` `face_analysis_active`
//! `derivation_active` `backup_last_success_at` `last_cover_stat_reconcile`。
//!
//! # section 划分
//! `ui`/`gallery`/`thumbnails`/`video`/`ai`/`face`/`exotic`/`logging`/`advanced` 九段来自
//! 任务给定分类;另加 `backup`(备份目的地/自动策略)与 `proofread`(远程 AI 校对端点)——
//! 这两组键在现有代码里自成一体、塞进 `advanced` 反而降低 config.toml 的可读性,故加两段
//! 而非削足适履(与"每键必填/回执列出假设"的方针一致,已在批次回执里注明此判断)。
//! 2026-07-23(B 线)再加 `viewer`(查看器渲染色域目标/自定义 ICC id)——同一判据:自成一体,
//! 不与缩略图/视频等既有段混淆。

/// 单个设置键的取值类型;决定 TOML 字面量渲染形态与加载期校验规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// TOML 布尔字面量 `true`/`false`(裸词,不加引号)。
    Bool,
    /// 非负整数(TOML integer 字面量,裸词)。存量历史键全部是非负量(计数/字节/像素/毫秒等),
    /// 故不单开 `Int`——真需要负数时再扩此枚举,不为当前不存在的用例预留分支。
    UInt,
    /// 浮点数(TOML float 字面量,裸词)。当前 schema 暂无实际使用者,为完整性保留。
    Float,
    /// 自由文本(TOML string 字面量,带引号,内容按 TOML 转义规则渲染)。
    Str,
    /// 枚举字符串:合法值全集,渲染注释与加载校验都据此。
    Enum(&'static [&'static str]),
    /// 文件系统路径(渲染/校验规则同 `Str`;语义上留空 = 使用代码内置的默认派生路径)。
    Path,
}

/// 单个设置键的声明式定义。
#[derive(Debug, Clone, Copy)]
pub struct SettingDef {
    /// TOML 文件里的分段名(仅用于渲染时的中文分段横幅注释与模板内键序分组——键名本身仍在
    /// 文件顶层平铺,不嵌套 `[section]` 表,与历史 `app_config` 单一扁平命名空间保持一致)。
    pub section: &'static str,
    /// 键名。必须与历史 `app_config.key` / 前端 IPC 调用的字面量完全一致——这是迁移与
    /// 前端消费方的唯一联系纽带,改名等于让旧值与旧前端代码同时失联。
    pub key: &'static str,
    pub kind: SettingKind,
    /// 默认值的规范文本形式。`Bool` 恒为 `"true"`/`"false"`(即使历史 DB 用过 `"1"`/`"0"`
    /// 两种编码,见 `migrate.rs` 的归一化);`UInt`/`Float` 恒可被对应类型 `parse()`;
    /// `Enum` 恒属于其选项集第一项或语义上的默认项。
    pub default: &'static str,
    /// 中文说明:写入模板文件对应键的注释行,供用户直接读文件就知道这项是什么、怎么填。
    /// 内容规约:一句话用途 + (Enum 类)可选值列全 + 默认值含义;需重启生效的键在此说明。
    pub comment_zh: &'static str,
    /// 是否无需重启应用即可生效——基于当前代码实际读取时机的摸底结果(不是设计意图):
    /// 多数键在 IPC/watcher 触发时被重新读取或经前端响应式状态即时应用;少数键只在
    /// 启动期被读入一次性局部变量,运行期改配置对本次运行无效,此类标 `false` 并在
    /// `restart_required` 对称置真,`comment_zh` 亦点明。
    pub hot: bool,
    pub restart_required: bool,
}

impl SettingDef {
    /// 按键名查找定义;`file.rs`/`migrate.rs`/`mod.rs` 共用,避免各处重复线性扫描的写法分叉。
    pub fn find(key: &str) -> Option<&'static SettingDef> {
        SETTING_DEFS.iter().find(|d| d.key == key)
    }
}

/// A2:状态类键全集(留 DB `app_config` 表的 12+6 项,穷举方法论/理由见模块顶部文档
/// 「排除清单」小节)。供 `ipc::config_commands` 的三路路由(schema 键 / 状态键 / 未知键)
/// 判定——不在 `SETTING_DEFS` 内、又不在本表内的键视为「未知」,按状态键同样走 DB 但额外
/// warn 日志(兜 A1 枚举误判,不拒绝写入)。
pub const STATE_KEYS: &[&str] = &[
    // 给定 12 项。
    "schema_version",
    "last_directory_id",
    "last_sort_by",
    "last_sort_order",
    "sidebar_width",
    "first_launch",
    "guide_seen",
    "pinned_settings",
    "ai_gpu_name",
    "exotic_paused",
    "layout_mode",
    "group_by",
    "sort_within_group",
    // 补充 6 项。
    "ai_provider",
    "ai_analysis_active",
    "face_analysis_active",
    "derivation_active",
    "backup_last_success_at",
    "last_cover_stat_reconcile",
];

/// section id → 中文分段标题(仅供 `file.rs` 渲染横幅注释使用)。
pub fn section_title(section: &str) -> &'static str {
    match section {
        "ui" => "界面",
        "gallery" => "画廊",
        "thumbnails" => "缩略图",
        "video" => "视频",
        "ai" => "AI 分析",
        "face" => "人脸识别",
        "exotic" => "冷门格式插件",
        "enhance" => "影像增强",
        "logging" => "日志",
        "backup" => "备份",
        "viewer" => "查看器",
        "proofread" => "AI 校对(远程)",
        "advanced" => "高级(进阶调优,一般无需修改;是否需重启见各项说明)",
        _ => "其他",
    }
}

/// 设置类键全集(穷举方法论见模块顶部文档)。**数组顺序即渲染进 config.toml 模板的顺序**
/// (同 section 的项连续排列,便于分段横幅正确分组——新增键时请插入到所属 section 的项之间,
/// 不要整表打散)。
pub const SETTING_DEFS: &[SettingDef] = &[
    // ── ui ──────────────────────────────────────────────────────────────────
    SettingDef {
        section: "ui",
        key: "language",
        kind: SettingKind::Enum(&["zh-CN", "en-US"]),
        default: "zh-CN",
        comment_zh: "界面语言。可选:zh-CN(简体中文) / en-US(English)。默认 zh-CN。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "appearance",
        kind: SettingKind::Enum(&["system", "light", "dark"]),
        default: "system",
        comment_zh: "外观明暗模式。可选:system(跟随系统) / light(浅色) / dark(深色)。默认 system。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "theme_light",
        kind: SettingKind::Str,
        default: "moonlight",
        comment_zh:
            "浅色外观下使用的主题包 id(主题注册表 src/themes/registry.ts 定义)。默认 moonlight。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "theme_dark",
        kind: SettingKind::Str,
        default: "ink",
        comment_zh: "深色外观下使用的主题包 id。默认 ink。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "theme_tint_strength",
        kind: SettingKind::UInt,
        default: "60",
        comment_zh: "主题色浓度(百分比)。作用于全部主题的底色 wash token(bg 五层/canvas/\
doc-paper):100=满浓度(出厂配色锚点),调小则底色向中性色稀释、主题色越淡。\
典型范围 0–100。默认 60。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "theme_text_strength",
        kind: SettingKind::UInt,
        default: "75",
        comment_zh: "文字浓度(百分比)。作用于全部主题的文字 ramp(text-primary/secondary/\
tertiary/placeholder):100=满浓度(出厂文字色),调小则文字向底色靠拢、对比更柔和。\
范围 40–100,下限防正文不可读。默认 75。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "theme",
        kind: SettingKind::Str,
        default: "system",
        comment_zh: "【历史遗留字段,只读迁移用】旧版本的外观设置;仅当 appearance/theme_light/\
theme_dark 尚未生成时被读取一次作迁移兜底,新版本不再写入此键。请改用上面三项,一般无需理会本项。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "ui_font_size",
        kind: SettingKind::UInt,
        default: "13",
        comment_zh: "界面基准字号(像素)。典型范围 12–24。默认 13。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "timeline_axis_width",
        kind: SettingKind::UInt,
        default: "44",
        comment_zh: "时间轴侧栏宽度(像素)。典型范围 32–80。默认 44。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "timeline_scroll_width",
        kind: SettingKind::UInt,
        default: "8",
        comment_zh: "画廊滚动条滑块宽度(像素)。典型范围 2–40。默认 8。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "scroll_thumb_min_height",
        kind: SettingKind::UInt,
        default: "48",
        comment_zh: "滚动条滑块与时间轴视窗共享的最小高度(像素)。典型范围 24–120。默认 48。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "axis_viewport_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "时间轴/minimap 半透明拖动视窗的不透明度缩放(百分比)。100=默认观感,\
调大更醒目、调小更隐形。典型范围 20–200。默认 100。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "enable_thumb_hover_scale",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "鼠标悬停缩略图时是否放大预览。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "hover_autoplay",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "鼠标悬停视频/实况照片格子时是否自动静音循环播放。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "close_behavior",
        kind: SettingKind::Enum(&["ask", "minimize_to_tray", "exit"]),
        default: "ask",
        comment_zh: "点击窗口关闭按钮时的行为。可选:ask(每次询问) / minimize_to_tray(最小化到\
托盘) / exit(直接退出)。默认 ask。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "auto_hide_chrome_windowed",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "窗口化沉浸模式:非全屏时自动隐藏顶栏与底栏,鼠标移至窗口边缘时显示。\
默认关闭。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "window_material",
        kind: SettingKind::Enum(&["mica", "acrylic", "none"]),
        default: "mica",
        comment_zh: "窗口材质(毛玻璃):标题栏/侧栏/工具栏/状态栏半透明。可选:mica(采样\
壁纸,Win10 自动退化 blur) / acrylic(实时透出窗口背后内容,拖动窗口可能卡顿) / none\
(不透明)。仅 Windows 生效。默认 mica。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "glass_chrome_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "窗口栏毛玻璃不透明度缩放(百分比)。100=保持当前 Mica/Acrylic 默认观感,\
调小更透明、调大更不透明。典型范围 20–120。默认 100。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "glass_sticky_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "分组标题/路径遮罩毛玻璃不透明度缩放(百分比)。100=默认观感,调小更透明。\
典型范围 20–120。默认 100。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "glass_surface_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "卡片表面毛玻璃不透明度缩放(百分比)。100=默认观感,调小更透明。\
典型范围 20–120。默认 100。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "glass_control_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "控件表面毛玻璃不透明度缩放(百分比)。100=默认观感,调小更透明。\
典型范围 20–120。默认 100。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "glass_content_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "内容底面(设置/收藏/人物/插件商店/文档阅读器等文字页)毛玻璃不透明度缩放\
(百分比)。100=基准(约 90% 主题色),调大更实(120 起钳到全不透明)、调小更透。\
典型范围 20–120。默认 100。非毛玻璃模式不消费该键。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "glass_gallery_opacity",
        kind: SettingKind::UInt,
        default: "0",
        comment_zh: "画廊大底与图片间隙的主题色遮罩不透明度(百分比)。0=完全透出 Windows\
毛玻璃,数值越大底色越明显。范围 0–100。默认 0。非毛玻璃模式不改变原有底色。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ui",
        key: "grid_row_height",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "画廊网格目标行高(像素,justified 排版算法输入)。典型范围 64–1024。默认 200。",
        hot: true,
        restart_required: false,
    },
    // ── gallery ─────────────────────────────────────────────────────────────
    SettingDef {
        section: "gallery",
        key: "tree_display_mode",
        kind: SettingKind::Enum(&["registeredOnly", "allFiles", "allFilesWithHidden"]),
        default: "registeredOnly",
        comment_zh: "侧栏文件树显示范围。可选:registeredOnly(仅已扫描登记的文件) / allFiles\
(磁盘全部文件) / allFilesWithHidden(含隐藏文件)。默认 registeredOnly。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "show_drag_handle",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "选中态缩略图左上角是否显示拖拽手柄(用于拖入文件夹)。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "seamless_groups",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "无缝分组:开启后分组排序仍按分组方式聚合,但不显示分隔符、行跨组连续排布。\
默认关闭。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "seamless_minimap",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh:
            "是否展开画廊右侧轴(时间轴与 minimap 通用的整体开合)。键名沿用历史,\
2026-07-24 轴/minimap 解耦后语义已由「仅无缝 minimap」扩为「轴开合」,形态另见 axis_mode。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "minimap_render_mode",
        kind: SettingKind::Enum(&["colors", "thumbnails"]),
        default: "colors",
        comment_zh: "minimap 轴的渲染模式。可选:colors(色块,轻量) / thumbnails(缩略图,更直观\
但更耗资源)。默认 colors。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "axis_mode",
        kind: SettingKind::Enum(&["timeline", "minimap"]),
        default: "timeline",
        comment_zh: "轴形态偏好:timeline|minimap,两分组模式通用。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "bucket_segmented_scroll",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "画廊是否使用 bucket 分段虚拟滚动引擎(关闭则回退线性平移引擎)。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "show_thumb_info",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "缩略图悬停时是否显示信息叠层(具体元素见 thumb_info_elements)。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "gallery",
        key: "thumb_info_elements",
        kind: SettingKind::Str,
        default: "[]",
        comment_zh:
            "缩略图信息叠层勾选的元素集合,JSON 字符串数组(如 [\"fileName\",\"fileSize\"])。\
默认空数组(不显示任何元素)。",
        hot: true,
        restart_required: false,
    },
    // ── thumbnails ──────────────────────────────────────────────────────────
    SettingDef {
        section: "thumbnails",
        key: "thumb_size",
        kind: SettingKind::UInt,
        default: "512",
        comment_zh: "缩略图目标尺寸(长边像素)。有效档位 [64,128,256,512,1024],其他取值会被\
自动吸附到最近档位。默认 512。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_strategy",
        kind: SettingKind::Enum(&["cpu", "gpu", "direct"]),
        default: "gpu",
        comment_zh: "缩略图解码策略。可选:cpu(CPU 软解) / gpu(GPU 加速,首选) / direct(直接\
显示原图,不生成缩略图)。默认 gpu。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "thumbnails",
        key: "gpu_engine",
        kind: SettingKind::Enum(&["wic"]),
        default: "wic",
        comment_zh: "GPU 缩略图解码引擎。当前仅 wic(Windows Imaging Component)一种可选。默认 wic。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_cache_dir",
        kind: SettingKind::Path,
        default: "",
        comment_zh: "缩略图缓存目录(绝对路径)。留空 = 使用默认路径 <app_data_dir>/cache。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_webp_quality",
        kind: SettingKind::UInt,
        default: "80",
        comment_zh: "缩略图 WebP 编码质量。取值 1–99 为有损压缩,100 为无损。默认 80。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_skip_max_kb",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "原图体积超过该阈值(KB)时直接显示原图、不生成缩略图。0 = 不跳过。默认 200。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_cache_max_mb",
        kind: SettingKind::UInt,
        default: "10240",
        comment_zh: "缩略图缓存 LRU 预算(MB)。默认 10240(10GB)。⚠当前实现仅在应用启动时读取\
一次用于后台清理任务,运行期修改本项需重启应用才会影响实际清理上限(设置页展示的当前上限\
数值会实时刷新,但清理动作按启动时读到的值执行——发现的既有限制,非本次改动引入)。",
        hot: false,
        restart_required: true,
    },
    // ── viewer ──────────────────────────────────────────────────────────────
    SettingDef {
        section: "viewer",
        key: "viewer_color_target",
        kind: SettingKind::Enum(&["srgb", "display-p3", "dci-p3", "custom"]),
        default: "srgb",
        comment_zh: "查看器大图渲染的目标色域。可选:srgb(不派生,直显原图) / display-p3 / \
dci-p3(主要用于影院素材核对) / custom(使用下方导入的自定义 ICC)。默认 srgb。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "viewer",
        key: "viewer_color_custom_id",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "`viewer_color_target=custom` 时生效的自定义 ICC profile id(16 位十六\
进制,由导入命令生成)。留空 = 未选择任何自定义 profile。",
        hot: true,
        restart_required: false,
    },
    // ── video ───────────────────────────────────────────────────────────────
    SettingDef {
        section: "video",
        key: "enable_video_cover",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否提取视频封面帧。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "video",
        key: "enable_video_keyframes",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否提取视频关键帧雪碧图(悬停预览用)。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "video",
        key: "video_cache_max_mb",
        kind: SettingKind::UInt,
        default: "20480",
        comment_zh: "视频可播产物(remux/转码)独立缓存池 LRU 预算(MB)。默认 20480(20GB)。\
与缩略图池分开——转码产物是 GB 级,共池会驱逐殆尽全库缩略图。按最近播放时间 LRU 驱逐,\
在播文件跳过。单文件预估超预算 50% 时播放前弹确认框。禁 0(0 被消费侧当无效值忽略回退默认)。\
消费面每次 resolve 现读,改后即时生效、无需重启。",
        hot: true,
        restart_required: false,
    },
    // ── ai ──────────────────────────────────────────────────────────────────
    SettingDef {
        section: "ai",
        key: "ai_active_model",
        kind: SettingKind::Str,
        default: "cn-clip-vit-b16",
        comment_zh: "当前激活的 CLIP 模型架构 id(同时是 ai_embeddings.model_name 向量空间键)。\
默认 cn-clip-vit-b16。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ai",
        key: "ai_active_image_file",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "当前激活架构下选中的图像编码器变体文件名。留空 = 使用该架构的默认变体。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ai",
        key: "ai_hq_cache_enabled",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "AI 高清缓存(opt-in):开启后后台为每张图额外生成短边≥336 的 WebP 缓存供 CLIP\
分析解码,降低分析时的 CPU/IO 占用。默认关闭。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ai",
        key: "ai_provider_override",
        kind: SettingKind::Enum(&["auto", "cpu"]),
        default: "auto",
        comment_zh:
            "AI 推理硬件策略。可选:auto(自动选择,优先 GPU) / cpu(强制仅用 CPU)。默认 auto。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ai",
        key: "ai_batch_size",
        kind: SettingKind::UInt,
        default: "0",
        comment_zh: "AI 分析批处理大小。0 = 按显存自动决定;上限 256,固定 batch 的模型会自动\
抬高到其所需下限。典型范围 0–512。默认 0。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ai",
        key: "ai_download_source",
        kind: SettingKind::Enum(&["official", "mirror"]),
        default: "official",
        comment_zh: "AI 模型下载首选源。可选:official(官方 HuggingFace 优先) / mirror(国内镜像\
hf-mirror.com 优先)。默认 official。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "ai",
        key: "ocr_active_tier",
        kind: SettingKind::Str,
        default: "pp-ocrv5-mobile",
        comment_zh: "当前使用中的 OCR 档位 id(见 scrollery-ai-core::ocr_profile)。\
默认 pp-ocrv5-mobile(标准档)。",
        hot: true,
        restart_required: false,
    },
    // ── face ────────────────────────────────────────────────────────────────
    SettingDef {
        section: "face",
        key: "face_enabled",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否启用人脸识别功能(关闭后引擎完全跳过加载人脸模型,省加载时间与显存)。\
默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "face",
        key: "face_model_active",
        kind: SettingKind::Str,
        default: "yunet-sface",
        comment_zh:
            "当前激活的人脸识别模型 id(同时是 faces.model_name 向量空间键)。默认 yunet-sface。",
        hot: true,
        restart_required: false,
    },
    // ── exotic ──────────────────────────────────────────────────────────────
    SettingDef {
        section: "exotic",
        key: "exotic_enabled",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否启用冷门格式插件子系统。默认开启。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "exotic",
        key: "exotic_auto_process",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "冷门格式任务是否随扫描自动处理(关闭后仍可在插件页手动触发)。默认开启。",
        hot: true,
        restart_required: false,
    },
    // ── enhance(影像增强;前端绑定归批 5,后端此处仅登记键+默认值,不进 STATE_KEYS)──
    SettingDef {
        section: "enhance",
        key: "enhance_model_denoise",
        kind: SettingKind::Str,
        default: "scunet",
        comment_zh: "影像增强·降噪任务当前选用的模型档 id(见 scrollery-ai-core::enhance_profile)。\
默认 scunet。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "enhance",
        key: "enhance_model_upscale",
        kind: SettingKind::Str,
        default: "realesrgan-x4plus",
        comment_zh: "影像增强·超分任务当前选用的模型档 id。默认 realesrgan-x4plus。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "enhance",
        key: "enhance_last_params",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "影像增强对话框上次提交的参数快照(JSON 字符串,供 Dialog 记忆上次选择)。\
留空 = 无记忆。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "enhance",
        key: "enhance_output_format",
        kind: SettingKind::Enum(&["follow_source", "jpeg", "png"]),
        default: "follow_source",
        comment_zh: "影像增强产物输出格式。可选:follow_source(跟随源,JPEG/PNG) / jpeg / png。\
默认 follow_source。",
        hot: true,
        restart_required: false,
    },
    // ── logging ─────────────────────────────────────────────────────────────
    SettingDef {
        section: "logging",
        key: "log_level",
        kind: SettingKind::Enum(&["trace", "debug", "info", "warn", "error", "off"]),
        default: "info",
        comment_zh: "日志级别。默认 info。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "logging",
        key: "log_dir",
        kind: SettingKind::Path,
        default: "",
        comment_zh: "日志输出目录(绝对路径)。留空 = 使用默认路径 <app_data_dir>/logs。⚠仅在应用\
启动时读取一次,修改后需重启应用生效。",
        hot: false,
        restart_required: true,
    },
    // ── backup ──────────────────────────────────────────────────────────────
    SettingDef {
        section: "backup",
        key: "backup_dir",
        kind: SettingKind::Path,
        default: "",
        comment_zh: "备份目标目录(绝对路径)。留空 = 未设置,不会执行自动备份。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "backup",
        key: "backup_auto_enabled",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "是否启用每日自动备份(需先设置 backup_dir)。默认关闭。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "backup",
        key: "backup_retention",
        kind: SettingKind::UInt,
        default: "5",
        comment_zh: "自动备份保留份数上限,超出按时间淘汰最旧备份。典型范围 1–100。默认 5。",
        hot: true,
        restart_required: false,
    },
    // ── proofread ───────────────────────────────────────────────────────────
    SettingDef {
        section: "proofread",
        key: "proofread_base_url",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "远程 AI 校对服务的 API 端点地址。留空 = 未配置。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "proofread",
        key: "proofread_model",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "远程 AI 校对使用的模型名称。留空 = 未配置。",
        hot: true,
        restart_required: false,
    },
    // ── advanced(进阶调优;批次C接线——每项 hot/restart_required 依其真实消费点而定,
    // 不再全部「需重启」,见各项注释)──────────────────────────────────────────
    SettingDef {
        section: "advanced",
        key: "max_log_dir_bytes",
        kind: SettingKind::UInt,
        default: "512",
        comment_zh: "日志目录大小上限(MB),超出后按时间淘汰最旧日志文件。⚠仅在应用启动时读取\
一次用于清理,修改后需重启应用才影响下次启动的清理上限。对应现有硬编码常量 logging::\
MAX_LOG_DIR_BYTES(config.toml 缺省时的回退值)。默认 512(512MB)。",
        hot: false,
        restart_required: true,
    },
    SettingDef {
        section: "advanced",
        key: "log_ring_buffer_capacity",
        kind: SettingKind::UInt,
        default: "20000",
        comment_zh: "内存日志环形缓冲区容量(条数,供日志窗口/诊断包读取)。⚠仅在应用启动时读取\
一次(环形缓冲挂在 tracing 全局 subscriber 上,构造后不可替换),修改后需重启应用生效。对应\
现有硬编码常量 logging::RING_BUFFER_CAPACITY(config.toml 缺省时的回退值)。默认 20000。",
        hot: false,
        restart_required: true,
    },
    SettingDef {
        section: "advanced",
        key: "derive_batch_size",
        kind: SettingKind::UInt,
        default: "256",
        comment_zh: "派生任务(视频封面/关键帧/文档缩略图等)流水线单批处理条数。每次派生流水线\
启动时读取一次,无需重启应用——下次手动/自动触发提取即生效。默认 256。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "video_keyframe_count",
        kind: SettingKind::UInt,
        default: "10",
        comment_zh: "视频关键帧雪碧图提取帧数。每次派生流水线启动时读取一次,无需重启应用,\
经启动配置下发到前端、悬停 scrub 的雪碧图列数换算(useHoverPreview.ts)响应式读取本值。\
⚠改动后需重建视频派生(清除并重新生成关键帧/雪碧图)——既有雪碧图是按旧帧数切好的图,不重建\
则前端会按新列数切旧图,画面错位。默认 10。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "sprite_cell_height",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "视频关键帧雪碧图单格高度(像素)。每次派生流水线启动时读取一次,无需重启应用。\
⚠改动后需重建视频派生(清除并重新生成关键帧/雪碧图)——既有雪碧图是按旧格高切好的图,不重建\
则既有雪碧图按旧参数切帧错位。默认 200。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "ai_cache_short_edge",
        kind: SettingKind::UInt,
        default: "336",
        comment_zh: "AI 高清缓存短边像素数。运行时立即生效(与 thumb_webp_quality 同惯例),\
无需重启应用;不触发存量 AI 缓存失效重建。默认 336。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "relink_match_threshold_pct",
        kind: SettingKind::UInt,
        default: "95",
        comment_zh: "移动/改名文件重新链接时的采样匹配阈值(百分比)。每次执行「重新链接」操作时\
读取一次,无需重启应用。默认 95。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "heavy_video_max_pixels",
        kind: SettingKind::UInt,
        default: "8294400",
        comment_zh: "判定「重」视频的像素面积阈值(宽×高),超过则悬停预览降级为静态擦拭浏览。\
经启动配置下发到前端、运行时读取(悬停触发时取值),无需重启应用。3840×2160=8294400,即 4K。\
默认 8294400。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "heavy_video_max_bytes",
        kind: SettingKind::UInt,
        default: "10737418240",
        comment_zh: "判定「重」视频的文件体积阈值(字节)。经启动配置下发到前端、运行时读取,\
无需重启应用。默认 10737418240(10GB)。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "hover_delay_ms",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "鼠标悬停缩略图后延迟多久开始预览(毫秒)。经启动配置下发到前端、运行时读取,\
无需重启应用。默认 200。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "search_debounce_ms",
        kind: SettingKind::UInt,
        default: "500",
        comment_zh: "搜索框混合/语义模式提交前的防抖延迟(毫秒;普通文件名搜索按键即查、不走本项)。\
经启动配置下发到前端、运行时读取,无需重启应用。默认 500(与既有 AppToolbar.vue 硬编码行为一致,\
非本批新引入的默认值)。",
        hot: true,
        restart_required: false,
    },
    SettingDef {
        section: "advanced",
        key: "resize_debounce_ms",
        kind: SettingKind::UInt,
        default: "300",
        comment_zh: "窗口/容器尺寸变化后重新计算布局的防抖延迟(毫秒)。经启动配置下发到前端、\
运行时读取,无需重启应用。默认 300。",
        hot: true,
        restart_required: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 键名全局唯一(扁平命名空间,重名会导致 file.rs 的加载/回填逻辑互相覆盖)。
    #[test]
    fn keys_are_unique() {
        let mut seen = HashSet::new();
        for def in SETTING_DEFS {
            assert!(seen.insert(def.key), "重复键:{}", def.key);
        }
    }

    /// 默认值必须能通过自身声明的 kind 校验(否则模板首次渲染出的"注释态默认值"本身就不合法)。
    #[test]
    fn defaults_are_valid_for_their_kind() {
        for def in SETTING_DEFS {
            match def.kind {
                SettingKind::Bool => {
                    assert!(
                        def.default == "true" || def.default == "false",
                        "{} 的默认值不是规范 bool 文本:{}",
                        def.key,
                        def.default
                    );
                }
                SettingKind::UInt => {
                    assert!(
                        def.default.parse::<u64>().is_ok(),
                        "{} 的默认值不是非负整数:{}",
                        def.key,
                        def.default
                    );
                }
                SettingKind::Float => {
                    assert!(
                        def.default.parse::<f64>().is_ok(),
                        "{} 的默认值不是数字:{}",
                        def.key,
                        def.default
                    );
                }
                SettingKind::Enum(options) => {
                    assert!(
                        options.contains(&def.default),
                        "{} 的默认值 {} 不在其枚举选项 {:?} 内",
                        def.key,
                        def.default,
                        options
                    );
                }
                SettingKind::Str | SettingKind::Path => {} // 自由文本,无额外约束。
            }
        }
    }

    /// A2:schema 设置类键与状态类键两清单互斥——否则路由判定会产生「既是设置又是状态」的
    /// 歧义键,`ipc::config_commands` 的三路由无法正确分流。
    #[test]
    fn state_keys_and_setting_defs_are_disjoint() {
        for k in STATE_KEYS {
            assert!(
                SettingDef::find(k).is_none(),
                "{k} 同时出现在 STATE_KEYS 与 SETTING_DEFS,路由判定会产生歧义"
            );
        }
    }

    #[test]
    fn find_by_key_roundtrips() {
        for def in SETTING_DEFS {
            let found = SettingDef::find(def.key).expect("必须能按键名查回自身");
            assert_eq!(found.key, def.key);
        }
        assert!(SettingDef::find("no_such_key").is_none());
    }
}
