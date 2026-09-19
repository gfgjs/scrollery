//! 设置类键定义单源(A1)。新增/改名/调整任何用户可配置的设置项,只改本文件——
//! `file.rs` 的校验/渲染都从 `SETTING_DEFS` 派生,不重复枚举键名。
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
//!   比不收录更误导人,故不收录(`ai_backend` 现已惰性无读,不驱动任何行为,同归此类)。
//! - **状态类键的补充排除**:任务给定的排除清单(schema_version 等 12 项)之外,又找到 6 个
//!   同属"应用自己记账、非用户表单可编辑"的键,按同一判据补充排除,理由随每项列在下方
//!   "排除清单"小节:`ai_provider`(worker 会话回声,写者与 `ai_gpu_name` 同一函数
//!   `persist_provider_echo`)、`ai_analysis_active`/`face_analysis_active`/
//!   `derivation_active`(启停时置位的"期望运行"标志,与已排除的 `exotic_paused` 同构)、
//!   `backup_last_success_at`/`last_cover_stat_reconcile`(后台任务自记的时间戳,无表单)。
//!
//! # 排除清单(内部状态类,留 DB `app_config` 表,不进本 schema)
//! 2026-09-16(设置集中保存实施)后只剩 12 个**应用自己记账**的内部键:`schema_version`
//! `last_directory_id` `first_launch` `guide_seen` `ai_gpu_name` `ai_provider`
//! `exotic_paused` `ai_analysis_active` `face_analysis_active` `derivation_active`
//! `backup_last_success_at` `last_cover_stat_reconcile`。
//! 原先存在 DB `app_config` 表或 localStorage 的用户偏好(`layout_mode`/`group_by`/
//! `sort_within_group`/`last_sort_*` `sidebar_width` `pinned_settings` 与 17 项 `doc_*`
//! 阅读器偏好)**改为归入本 schema**,唯一真源即 config.toml——判据是「全局范围决定应用外观/
//! 交互方式或用户选项」,它们与设置页参数同类;**不读取也不转存旧存储里的值**,未在文件中出现
//! 的键直接用 schema 默认值(2026-09-16 方案 §8)。而开机引导标记、任务启停标志、环境探测结果
//! 与后台时间戳属应用状态,留 DB,重置设置不得清除(方案 §3.2)。
//!
//! # section 划分
//! `ui`/`gallery`/`thumbnails`/`video`/`ai`/`face`/`exotic`/`logging`/`advanced` 九段来自
//! 任务给定分类;另加 `backup`(备份目的地/自动策略)与 `proofread`(远程 AI 校对端点)——
//! 这两组键在现有代码里自成一体、塞进 `advanced` 反而降低 config.toml 的可读性,故加两段
//! 而非削足适履(与"每键必填/回执列出假设"的方针一致,已在批次回执里注明此判断)。
//! 2026-07-23(B 线)再加 `viewer`(查看器渲染色域目标/自定义 ICC id)——同一判据:自成一体,
//! 不与缩略图/视频等既有段混淆。

/// 结构体字段的取值类型(`Struct`/`StructMap`/`StructList` 内部使用)。
///
/// 只收录实际存在的结构种类:不开放任意对象、任意字段名。`Int`/`Float` 带区间,数值约束
/// 集中在此(消费侧控件与业务函数的既有约束在此固化,不再各写一份)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldType {
    Bool,
    /// 有符号整数(窗口坐标可为负)。
    Int {
        min: i64,
        max: i64,
    },
    /// 有限小数:拒绝 NaN / 无穷(音量、倍速、行高、行距、缩放因子等不能截为整数)。
    Float {
        min: f64,
        max: f64,
    },
    Str,
    Enum(&'static [&'static str]),
    /// 字符串数组(如日志预设的级别清单)。
    StrList,
    /// 颜色字面量:规范 `#rrggbb`;`allow_auto` 为真时额外接受 `auto`(画廊底色「跟随界面
    /// 底面」)。只校验形状,不判可读性——用户自由选色,对比不足由编辑器提示而非改写输入。
    Color {
        allow_auto: bool,
    },
}

/// 一个字段的声明:名字 + 类型。名字即规范 JSON 文本里的键名(结构类值在 IPC 用 JSON,落盘用
/// TOML 内联表,两侧键名一致)。
///
/// 派生 `PartialEq` 是 `SettingKind` 派生同 trait 的必需条件(其 `Struct` 变体持有
/// `&'static StructDef`);`FieldType` 已含 `f64` 区间,故本层不派生 `Eq`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldDef {
    pub name: &'static str,
    pub ty: FieldType,
}

/// 数组内条目的唯一性约束(仅 `StructList` 有意义:所列字段在同一数组内两两不得重复)。
/// 声明式而非按键特判:规则与字段表同居一处,加载校验与提交校验共用同一份判据。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniqueField {
    /// 字段名(必须在同一 `StructDef` 的 `fields` 内)。
    pub name: &'static str,
    /// 名称类字段:`true` 表示用户可编辑的展示名——比较前去除首尾空白(「 暖纸 」与「暖纸」
    /// 是同一个名字),去空白后不得为空。
    ///
    /// `false` 表示机器生成的**稳定 ID**:必须非空、且自身不得含首尾空白。这条比名称更严是有
    /// 意的——ID 是唯一定位键,若允许 `" a "`,它在后端与 `"a"` 是两条不同记录,而前端读取时
    /// 会 trim 并丢弃空 ID,同一条记录在前端就此消失;两侧口径必须一致,故在边界直接拒绝。
    pub is_name: bool,
}

/// 固定字段集的结构体定义(无嵌套结构体:当前不存在需要嵌套的结构值)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructDef {
    pub fields: &'static [FieldDef],
    /// 数组内条目的唯一性约束(结构类单值键传空表)。
    pub unique: &'static [UniqueField],
}

impl StructDef {
    pub fn field(&self, name: &str) -> Option<&'static FieldDef> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// 单个设置键的取值类型;决定 TOML 字面量渲染形态与加载期校验规则。
///
/// **规范文本形态(与消费方的接口契约)**:标量类键的规范文本就是 TOML 标量的等价文本
/// (`true`/`123`/`1.75`/枚举字面量);结构类键(StrList / Struct / BoolMap / StructMap /
/// StructList)的规范文本是**规范 JSON 文本**——IPC 与前端只用 JSON 文本,磁盘一律写原生
/// TOML 数组/内联表(转换集中在 `config::value`,禁止在文件里嵌套 JSON 字符串)。
///
/// 不派生 `Eq`:`Float` 带 `f64` 约束,只满足 `PartialEq`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    /// TOML 布尔字面量 `true`/`false`(裸词,不加引号)。
    Bool,
    /// 非负整数(TOML integer 字面量,裸词)。存量历史键全部是非负量(计数/字节/像素/毫秒等)。
    UInt,
    /// 有符号整数(带区间约束),用于窗口坐标等可为负的量。
    Int { min: i64, max: i64 },
    /// 有限小数(带区间约束):拒绝 NaN 与无穷。
    Float { min: f64, max: f64 },
    /// 自由文本(TOML string 字面量,带引号,内容按 TOML 转义规则渲染)。
    Str,
    /// 枚举字符串:合法值全集,渲染注释与加载校验都据此。
    Enum(&'static [&'static str]),
    /// 文件系统路径(渲染/校验规则同 `Str`;语义上留空 = 使用代码内置的默认派生路径)。
    Path,
    /// 字符串数组:规范 JSON 文本 `["a","b"]`,落盘为原生 TOML 数组。
    StrList,
    /// 固定字段集的内联对象:规范 JSON 文本 `{"x":0.0,"y":0.0}`,落盘为 TOML 内联表。
    Struct(&'static StructDef),
    /// 固定 ID 集合 → 布尔的映射(展开态类)。规范 JSON 文本**补齐全部 ID**(缺项取本键
    /// `SettingDef.default` 里该 ID 的值),故消费方无需自行判定缺省;文件中出现的未知 ID
    /// 只忽略该条目并告警。
    BoolMap { ids: &'static [&'static str] },
    /// 受约束键集 → 结构体的映射(当前唯一用例:窗口 label → 窗口几何)。`labels` 是合法键全集:
    /// 文件中出现表外的键只忽略该条目并告警(不因一个陌生标签拒绝整份文件),提交 patch 时表外键
    /// 直接拒绝。故不是「任意动态键名」的开放容器,新增窗口须先登记进 `labels`。
    StructMap {
        labels: &'static [&'static str],
        value: &'static StructDef,
    },
    /// 结构体数组(日志筛选预设)。规范 JSON 文本 `[{...}]`,落盘为 TOML 内联表数组。
    StructList(&'static StructDef),
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
    /// 两种编码,见 `migrate.rs` 的归一化);`UInt` 恒可被 `u64::parse()` 读回;
    /// `Enum` 恒属于其选项集第一项或语义上的默认项。
    pub default: &'static str,
    /// 中文说明:写入模板文件对应键的注释行,供用户直接读文件就知道这项是什么、怎么填。
    /// 内容规约:一句话用途 + (Enum 类)可选值列全 + 默认值含义;需重启生效的键在此说明。
    pub comment_zh: &'static str,
    /// 是否无需重启应用即可生效——基于当前代码实际读取时机的摸底结果(不是设计意图):
    /// 多数键在 IPC/watcher 触发时被重新读取或经前端响应式状态即时应用;少数键只在
    /// 启动期被读入一次性局部变量,运行期改配置对本次运行无效,此类标 `false`、`comment_zh`
    /// 亦点明;watcher 据此把它们列进 `config-file-changed` 的 `restart_required`(前端提示重启)。
    pub hot: bool,
}

impl SettingDef {
    /// 按键名查找定义;`file.rs`/`migrate.rs`/`mod.rs` 共用,避免各处重复线性扫描的写法分叉。
    pub fn find(key: &str) -> Option<&'static SettingDef> {
        SETTING_DEFS.iter().find(|d| d.key == key)
    }
}

/// 内部状态类键全集(应用状态,留 DB `app_config` 表)。穷举方法论/理由见模块顶部文档
/// 「排除清单」小节。供 `ipc::config_commands` 的判定——不在本清单内的键在
/// `get_app_config`/`set_app_config` 以 `config_unknown_key` 拒绝(2026-09-16:设置类键
/// 全部改走快照/批量入口,旧通用键值 IPC 不再承担设置读写)。
pub const STATE_KEYS: &[&str] = &[
    // 数据库格式标识与库级记忆。
    "schema_version",
    "last_directory_id",
    // 首次使用与引导完成标记:重置设置**不重放引导**,故留应用状态。
    "first_launch",
    "guide_seen",
    // 运行环境探测结果(worker 会话回声)。
    "ai_gpu_name",
    "ai_provider",
    // 任务启停与续作状态。
    "exotic_paused",
    "ai_analysis_active",
    "face_analysis_active",
    "derivation_active",
    // 后台工作的时间戳。
    "backup_last_success_at",
    "last_cover_stat_reconcile",
];

// ── 结构类键的字段表与固定 ID 集合(结构值的唯一单源)─────────────────────────────

/// 选区浮动条拖拽位移(相对当前对齐基位的像素位移;两维可为负,使用侧按视口约束)。
pub const SELECTION_BAR_OFFSET_FIELDS: &[FieldDef] = &[
    FieldDef {
        name: "x",
        ty: FieldType::Float {
            min: -100_000.0,
            max: 100_000.0,
        },
    },
    FieldDef {
        name: "y",
        ty: FieldType::Float {
            min: -100_000.0,
            max: 100_000.0,
        },
    },
];
pub static SELECTION_BAR_OFFSET: StructDef = StructDef {
    fields: SELECTION_BAR_OFFSET_FIELDS,
    unique: &[],
};

/// 窗口几何(派工约定的字段顺序与语义):正常物理位置/尺寸 + 保存时 scale factor + 最大化标记。
/// 宽高下限 1、坐标允许负值(副屏在主屏左上时为负);`scaleFactor` 取 0.1–10.0 覆盖 100%–1000%。
pub const WINDOW_GEOMETRY_FIELDS: &[FieldDef] = &[
    FieldDef {
        name: "x",
        ty: FieldType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
        },
    },
    FieldDef {
        name: "y",
        ty: FieldType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
        },
    },
    FieldDef {
        name: "width",
        ty: FieldType::Float {
            min: 1.0,
            max: 100_000.0,
        },
    },
    FieldDef {
        name: "height",
        ty: FieldType::Float {
            min: 1.0,
            max: 100_000.0,
        },
    },
    FieldDef {
        name: "scaleFactor",
        ty: FieldType::Float {
            min: 0.1,
            max: 10.0,
        },
    },
    FieldDef {
        name: "maximized",
        ty: FieldType::Bool,
    },
];
pub static WINDOW_GEOMETRY: StructDef = StructDef {
    fields: WINDOW_GEOMETRY_FIELDS,
    unique: &[],
};

/// 日志筛选预设条目(对应日志窗口「保存当前筛选为预设」的字段;级别集合与后端 tracing 级别同名)。
pub const LOG_FILTER_PRESET_FIELDS: &[FieldDef] = &[
    FieldDef {
        name: "name",
        ty: FieldType::Str,
    },
    FieldDef {
        name: "levels",
        ty: FieldType::StrList,
    },
    FieldDef {
        name: "target",
        ty: FieldType::Str,
    },
    FieldDef {
        name: "text",
        ty: FieldType::Str,
    },
    FieldDef {
        name: "textMode",
        ty: FieldType::Enum(&["substring", "regex"]),
    },
];
pub static LOG_FILTER_PRESET: StructDef = StructDef {
    fields: LOG_FILTER_PRESET_FIELDS,
    unique: &[],
};

// ── 主题配色(方案 §3)──────────────────────────────────────────────────────────

/// 默认浅色/深色种子的唯一真源:`src/themes/presets/default-light.json` / `default-dark.json`,
/// 前端预设 import 同一对文件(方案 §3:权威默认种子只写一份,Rust 侧禁止另写一份 HEX 默认值)。
///
/// 这两份文件是**规范紧凑 JSON**(键序与 `THEME_SEED_FIELDS` 一致、无排版空白、无末尾换行),
/// 因此可以直接作为结构类键的规范文本使用——规范文本本就要求紧凑 JSON(`value` 的渲染产物同型),
/// 不需要任何运行期或编译期再加工。
const DEFAULT_LIGHT_SEED_JSON: &str =
    include_str!("../../../src/themes/presets/default-light.json");
const DEFAULT_DARK_SEED_JSON: &str = include_str!("../../../src/themes/presets/default-dark.json");

/// 一套配色种子的固定字段(浅色/深色两份共用本定义)。
/// 颜色一律规范 `#rrggbb`;`gallery` 额外接受 `auto`(跟随界面底面);`contrast` 为 0–100 整数。
pub const THEME_SEED_FIELDS: &[FieldDef] = &[
    FieldDef {
        name: "background",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "foreground",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "accent",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "contrast",
        ty: FieldType::Int { min: 0, max: 100 },
    },
    FieldDef {
        name: "gallery",
        ty: FieldType::Color { allow_auto: true },
    },
];
pub static THEME_SEED: StructDef = StructDef {
    fields: THEME_SEED_FIELDS,
    unique: &[],
};

/// 个人主题条目的唯一性约束:ID 为稳定定位键(精确比较),名称去首尾空白后不得重复且不得为空
/// (重名会在界面上无从区分,故不按名称自动覆盖)。两组约束都在设置边界生效。
const SAVED_THEME_UNIQUE: &[UniqueField] = &[
    UniqueField {
        name: "id",
        is_name: false,
    },
    UniqueField {
        name: "name",
        is_name: true,
    },
];

/// 个人主题的持久化条目(`theme_saved_themes` 数组元素):固定**扁平**字段,不引入嵌套表;
/// 两组颜色/对比度字段复用种子字段的取值规则,由前端 `themes/config.ts` 集中转换为具名
/// light/dark 结构,不在各组件拼接。`material`/`opacity` 与当前窗口材质字段同语义。
pub const SAVED_THEME_FIELDS: &[FieldDef] = &[
    FieldDef {
        name: "id",
        ty: FieldType::Str,
    },
    FieldDef {
        name: "name",
        ty: FieldType::Str,
    },
    FieldDef {
        name: "light_background",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "light_foreground",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "light_accent",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "light_contrast",
        ty: FieldType::Int { min: 0, max: 100 },
    },
    FieldDef {
        name: "light_gallery",
        ty: FieldType::Color { allow_auto: true },
    },
    FieldDef {
        name: "dark_background",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "dark_foreground",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "dark_accent",
        ty: FieldType::Color { allow_auto: false },
    },
    FieldDef {
        name: "dark_contrast",
        ty: FieldType::Int { min: 0, max: 100 },
    },
    FieldDef {
        name: "dark_gallery",
        ty: FieldType::Color { allow_auto: true },
    },
    FieldDef {
        name: "material",
        ty: FieldType::Enum(&["none", "mica", "acrylic"]),
    },
    FieldDef {
        name: "opacity",
        ty: FieldType::Int { min: 0, max: 100 },
    },
];
pub static SAVED_THEME: StructDef = StructDef {
    fields: SAVED_THEME_FIELDS,
    unique: SAVED_THEME_UNIQUE,
};

/// 设置页可折叠卡片的固定 ID 集(与各 `CollapsibleCard id` 一致;顺序仅为可读性,规范 JSON
/// 文本按字母序输出)。
pub const SETTINGS_CARD_IDS: &[&str] = &[
    "aiModels",
    "backup",
    "common",
    "danger",
    "debug",
    "enhanceModels",
    "faceModels",
    "galleryBehavior",
    "general",
    "knownVolumes",
    "modelLibrary",
    "networkStorage",
    "ocrModels",
    "reading",
    "rootVisibility",
    "thumbnails",
    "video",
];

/// 阅读器设置面板内的固定分组 ID(与 `ReaderSettingsGroup id` 一致)。
pub const READER_PANEL_IDS: &[&str] = &["book", "paging", "theme", "typography"];

/// 持久化窗口几何的窗口 label 全集(窗口 label 注册表)。新增一个需要记住位置/尺寸的窗口时,
/// 必须把它的 label 加进本清单——表外的 label 在加载时被忽略(其余窗口照常恢复),在提交时被拒绝。
/// 当前两个:主窗口 `main`、独立日志窗口 `logs`(窗口模块接线时复核实际 label)。
pub const WINDOW_LABELS: &[&str] = &["main", "logs"];

/// 侧栏手风琴区块 ID(与各 `<AccordionSection id>` 一致)。
pub const SIDEBAR_SECTION_IDS: &[&str] = &["folders", "library", "management", "tools"];

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
        "layout" => "布局与排序",
        "player" => "播放器",
        "reader" => "阅读器",
        "window" => "窗口",
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
    },
    SettingDef {
        section: "ui",
        key: "appearance",
        kind: SettingKind::Enum(&["system", "light", "dark"]),
        default: "system",
        comment_zh: "外观明暗模式。可选:system(跟随系统) / light(浅色) / dark(深色)。默认 system。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "theme_light_palette",
        kind: SettingKind::Struct(&THEME_SEED),
        default: DEFAULT_LIGHT_SEED_JSON,
        comment_zh: "浅色模式的配色种子(TOML 内联表)。字段:background / foreground / accent / \
gallery(颜色,规范 #rrggbb;gallery 另可填 auto 跟随界面底面)、contrast(层次对比度,0–100 整数,\
只影响派生层次,不改动你选的三个颜色)。默认取中性预设浅色种子(见 \
src/themes/presets/default-light.json)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "theme_dark_palette",
        kind: SettingKind::Struct(&THEME_SEED),
        default: DEFAULT_DARK_SEED_JSON,
        comment_zh: "深色模式的配色种子(字段与取值范围同上)。默认取中性预设深色种子(见 \
src/themes/presets/default-dark.json)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "theme_saved_themes",
        kind: SettingKind::StructList(&SAVED_THEME),
        default: "[]",
        comment_zh: "「我的主题」列表(TOML 表数组),默认空。每条为一份完整主题快照,字段:id / name、\
light_background / light_foreground / light_accent / light_contrast / light_gallery、dark_background / \
dark_foreground / dark_accent / dark_contrast / dark_gallery、material(none / mica / acrylic)、\
opacity(0–100 整数)。名称去首尾空白后不得为空且不得与已有条目重名(ID 须唯一);继续调色不会自动更新\
已保存的条目,删除条目不影响正在使用的配色。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "ui_font_size",
        kind: SettingKind::UInt,
        default: "13",
        comment_zh: "界面基准字号(像素)。典型范围 12–24。默认 13。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "timeline_axis_width",
        kind: SettingKind::UInt,
        default: "44",
        comment_zh: "时间轴侧栏宽度(像素)。典型范围 32–80。默认 44。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "timeline_scroll_width",
        kind: SettingKind::UInt,
        default: "8",
        comment_zh: "画廊滚动条滑块宽度(像素)。典型范围 2–40。默认 8。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "scroll_thumb_min_height",
        kind: SettingKind::UInt,
        default: "48",
        comment_zh: "滚动条滑块与时间轴视窗共享的最小高度(像素)。典型范围 24–120。默认 48。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "axis_viewport_opacity",
        kind: SettingKind::UInt,
        default: "100",
        comment_zh: "时间轴/minimap 半透明拖动视窗的不透明度缩放(百分比)。100=默认观感,\
调大更醒目、调小更隐形。典型范围 20–200。默认 100。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "enable_thumb_hover_scale",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "鼠标悬停缩略图时是否放大预览。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "hover_autoplay",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "鼠标悬停视频/实况照片格子时是否自动静音循环播放。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "close_behavior",
        kind: SettingKind::Enum(&["ask", "minimize_to_tray", "exit"]),
        default: "ask",
        comment_zh: "点击窗口关闭按钮时的行为。可选:ask(每次询问) / minimize_to_tray(最小化到\
托盘) / exit(直接退出)。默认 ask。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "auto_hide_chrome_windowed",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "窗口化沉浸模式:非全屏时自动隐藏顶栏与底栏,鼠标移至窗口边缘时显示。\
默认关闭。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "window_material",
        kind: SettingKind::Enum(&["none", "mica", "acrylic"]),
        default: "none",
        comment_zh: "窗口材质(毛玻璃):标题栏/侧栏/工具栏/状态栏半透明。可选:mica(采样\
壁纸,Win10 自动退化 blur) / acrylic(实时透出窗口背后内容,拖动窗口可能卡顿) / none\
(不透明,纯色,新默认)。仅 Windows 生效,平台不支持时使用纯色结果。默认 none。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "window_opacity",
        kind: SettingKind::UInt,
        default: "90",
        comment_zh: "窗口不透明度(百分比,0–100 整数)。仅在 window_material 开启(mica / acrylic)时生效,\
用于整窗共享的一层玻璃填充;控件、内容面、浮层的填充由固定角色规则派生,不再分别配置。100=最实,\
越小越透出窗口背后的内容。默认 90(none 材质下不消费本键)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "grid_row_height",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "画廊网格目标行高(像素,justified 排版算法输入)。典型范围 64–1024。默认 200。",
        hot: true,
    },
    // ── 以下为 2026-09-16 设置集中保存新纳入的界面偏好(此前散在 localStorage / DB)────
    SettingDef {
        section: "ui",
        key: "titlebar_merged",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "标题栏与画廊工具栏是否合并为同一行。true=合并(默认,竖向更省空间)、\
false=分离(工具栏独立成第二行)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "toolbar_align",
        kind: SettingKind::Enum(&["center", "left", "right"]),
        default: "center",
        comment_zh: "顶栏中部筛选/视图按钮簇的水平对齐。可选:center(居中,默认) / left / right。\
与选区浮动条对齐(selection_bar_align)各自独立。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "selection_bar_docked",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "选中若干文件时的操作条形态。false=浮动可拖胶囊(默认)、true=并入底栏状态条。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "selection_bar_align",
        kind: SettingKind::Enum(&["center", "left", "right"]),
        default: "center",
        comment_zh: "选区浮动条的水平对齐(决定其静止基位)。可选:center(默认) / left / right。\
与顶栏对齐(toolbar_align)各自独立。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "selection_bar_offset",
        kind: SettingKind::Struct(&SELECTION_BAR_OFFSET),
        default: "{\"x\":0.0,\"y\":0.0}",
        comment_zh: "选区浮动条相对对齐基位的拖拽位移(像素,两维可为负)。TOML 内联表写法:\
{ x = 0.0, y = 0.0 }。默认原点。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "gallery_render_mode",
        kind: SettingKind::Enum(&["dom", "canvas"]),
        default: "canvas",
        comment_zh: "画廊渲染引擎。可选:dom(常规 DOM 渲染) / canvas(默认,实验性画布渲染,内存更省;\
超大图库或 iOS 仍自动回退 DOM)。显式存 dom 即尊重用户选择。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "timeline_render_mode",
        kind: SettingKind::Enum(&["dom", "canvas"]),
        default: "canvas",
        comment_zh: "右侧时间轴/缩略轴的渲染引擎。可选:dom(常规 DOM 渲染) / canvas(默认,实验性,与\
画廊渲染引擎独立)。显式存 dom 即尊重用户选择。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "detail_show_faces",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "大图查看器是否显示已识别的人脸标注框。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "detail_controls_hidden",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "大图查看器控制条是否默认隐藏(鼠标移到窗口边缘时唤出)。默认关闭(即默认显示控制条)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "timeline_visual",
        kind: SettingKind::Enum(&["bars", "envelope", "heat", "spectral"]),
        default: "bars",
        comment_zh: "时间轴密度带的视觉形态。可选:bars(离散月条,默认) / envelope(填充包络) / \
heat(纯热力色带) / spectral(包络+冷暖光谱)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "timeline_coord",
        kind: SettingKind::Enum(&["item", "time"]),
        default: "item",
        comment_zh: "时间轴的坐标系。可选:item(按项目累计空间,默认) / time(按日历时间疏密, \
仅按日期分组且非 bars 形态时生效)。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "export_include_manifest",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "导出整理成果时是否附带清单文件(manifest)。默认关闭。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "demo_privacy",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "演示打码:开启后用别名显示文件夹与文件、模糊画廊内容,便于公开演示。默认关闭。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "sidebar_sections_expanded",
        kind: SettingKind::BoolMap {
            ids: SIDEBAR_SECTION_IDS,
        },
        default: "{\"folders\":true,\"library\":true,\"management\":true,\"tools\":true}",
        comment_zh: "侧栏各区块的展开状态(TOML 内联表:区块 id = true/false)。区块 id:\
library / tools / folders / management。默认全部展开。",
        hot: true,
    },
    SettingDef {
        section: "ui",
        key: "settings_cards_expanded",
        kind: SettingKind::BoolMap {
            ids: SETTINGS_CARD_IDS,
        },
        default: "{\"aiModels\":true,\"backup\":true,\"common\":true,\"danger\":false,\
\"debug\":true,\"enhanceModels\":true,\"faceModels\":true,\"galleryBehavior\":true,\
\"general\":true,\"knownVolumes\":true,\"modelLibrary\":true,\"networkStorage\":true,\
\"ocrModels\":true,\"reading\":true,\"rootVisibility\":true,\"thumbnails\":true,\
\"video\":true}",
        comment_zh: "设置页各卡片的展开状态(TOML 内联表:卡片 id = true/false)。默认仅\"危险操作\"卡\
折叠,其余展开。",
        hot: true,
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
    },
    SettingDef {
        section: "gallery",
        key: "show_drag_handle",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "选中态缩略图左上角是否显示拖拽手柄(用于拖入文件夹)。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "gallery",
        key: "seamless_groups",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "无缝分组:开启后分组排序仍按分组方式聚合,但不显示分隔符、行跨组连续排布。\
默认关闭。",
        hot: true,
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
    },
    SettingDef {
        section: "gallery",
        key: "minimap_render_mode",
        kind: SettingKind::Enum(&["colors", "thumbnails"]),
        default: "colors",
        comment_zh: "minimap 轴的渲染模式。可选:colors(色块,轻量) / thumbnails(缩略图,更直观\
但更耗资源)。默认 colors。",
        hot: true,
    },
    SettingDef {
        section: "gallery",
        key: "axis_mode",
        kind: SettingKind::Enum(&["timeline", "minimap"]),
        default: "timeline",
        comment_zh: "轴形态偏好:timeline|minimap,两分组模式通用。",
        hot: true,
    },
    SettingDef {
        section: "gallery",
        key: "show_thumb_info",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "缩略图悬停时是否显示信息叠层(具体元素见 thumb_info_elements)。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "gallery",
        key: "thumb_info_elements",
        kind: SettingKind::StrList,
        default: "[]",
        comment_zh: "缩略图信息叠层勾选的元素集合(TOML 字符串数组)。可用元素:filename(文件名)、\
date(日期)、resolution(分辨率)、path(路径)、geo(地理位置)、camera(相机参数)、params(拍摄参数);\
另有 type(类型,角标位)。默认空数组(不显示任何元素)。",
        hot: true,
    },
    // ── layout(布局与排序:画廊分组/排序、视图模式、侧栏宽度与置顶项;2026-09-16 起归入 schema)──
    SettingDef {
        section: "layout",
        key: "layout_mode",
        kind: SettingKind::Enum(&["justified", "grid"]),
        default: "justified",
        comment_zh: "画廊排版算法。可选:justified(等高行,默认) / grid(均匀宫格)。",
        hot: true,
    },
    SettingDef {
        section: "layout",
        key: "group_by",
        kind: SettingKind::Enum(&["date", "folder", "none"]),
        default: "date",
        comment_zh: "画廊分组方式。可选:date(按日期,默认) / folder(按文件夹) / none(不分组)。",
        hot: true,
    },
    SettingDef {
        section: "layout",
        key: "sort_within_group",
        kind: SettingKind::Enum(&["datetime", "filename", "similarity"]),
        default: "datetime",
        comment_zh: "组内排序依据。可选:datetime(按时间,默认) / filename(按文件名) / \
similarity(按相似度,语义搜索场景)。",
        hot: true,
    },
    SettingDef {
        section: "layout",
        key: "last_sort_by",
        kind: SettingKind::Str,
        default: "sort_datetime",
        comment_zh: "历史排序字段记忆(旧版本遗留键)。当前版本无读取点,修改不影响行为,\
保留以免旧值丢失。",
        hot: true,
    },
    SettingDef {
        section: "layout",
        key: "last_sort_order",
        kind: SettingKind::Str,
        default: "desc",
        comment_zh: "历史排序方向记忆(旧版本遗留键)。当前版本无读取点,修改不影响行为,\
保留以免旧值丢失。",
        hot: true,
    },
    SettingDef {
        section: "layout",
        key: "sidebar_width",
        kind: SettingKind::Int { min: 180, max: 400 },
        default: "260",
        comment_zh: "侧栏宽度(像素)。范围 180–400。默认 260。",
        hot: true,
    },
    SettingDef {
        section: "layout",
        key: "pinned_settings",
        kind: SettingKind::StrList,
        default: "[\"aiFullAnalysis\",\"faceFullAnalysis\"]",
        comment_zh: "置顶的设置项与常驻工具清单(TOML 字符串数组,数组顺序即显示顺序)。默认含两项\
常驻工具:aiFullAnalysis(全量 AI 分析)、faceFullAnalysis(全量人脸识别)。",
        hot: true,
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
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_strategy",
        kind: SettingKind::Enum(&["cpu", "gpu", "direct"]),
        default: "gpu",
        comment_zh: "缩略图解码策略。可选:cpu(CPU 软解) / gpu(GPU 加速,首选) / direct(直接\
显示原图,不生成缩略图)。默认 gpu。",
        hot: true,
    },
    SettingDef {
        section: "thumbnails",
        key: "gpu_engine",
        kind: SettingKind::Enum(&["wic"]),
        default: "wic",
        comment_zh: "GPU 缩略图解码引擎。当前仅 wic(Windows Imaging Component)一种可选。默认 wic。",
        hot: true,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_cache_dir",
        kind: SettingKind::Path,
        default: "",
        comment_zh: "缩略图缓存目录(绝对路径)。留空 = 使用默认路径 <app_data_dir>/cache。",
        hot: true,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_webp_quality",
        kind: SettingKind::UInt,
        default: "80",
        comment_zh: "缩略图 WebP 编码质量。取值 1–99 为有损压缩,100 为无损。默认 80。",
        hot: true,
    },
    SettingDef {
        section: "thumbnails",
        key: "thumb_skip_max_kb",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "原图体积超过该阈值(KB)时直接显示原图、不生成缩略图。0 = 不跳过。默认 200。",
        hot: true,
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
    },
    SettingDef {
        section: "viewer",
        key: "viewer_color_custom_id",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "`viewer_color_target=custom` 时生效的自定义 ICC profile id(16 位十六\
进制,由导入命令生成)。留空 = 未选择任何自定义 profile。",
        hot: true,
    },
    // ── reader(阅读器排版与阅读模式;2026-09-16 起归入 schema,默认值沿用阅读器初始化值)────
    SettingDef {
        section: "reader",
        key: "doc_reader_font_size",
        kind: SettingKind::Int { min: 12, max: 32 },
        default: "19",
        comment_zh: "阅读器正文字号(像素)。范围 12–32。默认 19。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_line_height",
        kind: SettingKind::Float { min: 1.2, max: 2.4 },
        default: "1.75",
        comment_zh: "阅读器行距倍数。范围 1.2–2.4。默认 1.75。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_font_family",
        kind: SettingKind::Enum(&["serif", "sans"]),
        default: "serif",
        comment_zh: "阅读器正文字体族。可选:serif(衬线,默认) / sans(无衬线)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_page_width",
        kind: SettingKind::Int { min: 480, max: 1040 },
        default: "720",
        comment_zh: "阅读器正文栏宽(像素)。范围 480–1040。默认 720。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_page_turn",
        kind: SettingKind::Enum(&["none", "slide", "curl"]),
        default: "slide",
        comment_zh: "阅读器翻页动画。可选:none(瞬切) / slide(滑动,默认) / curl(仿真翻页)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_font_weight",
        kind: SettingKind::Enum(&["400", "500", "700"]),
        default: "400",
        comment_zh: "阅读器正文字重。可选:400(常规,默认) / 500(中粗) / 700(粗体)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_letter_spacing",
        kind: SettingKind::Float { min: 0.0, max: 0.15 },
        default: "0.0",
        comment_zh: "阅读器字距(em)。范围 0–0.15。默认 0(不加宽)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_text_align",
        kind: SettingKind::Enum(&["justify", "left"]),
        default: "justify",
        comment_zh: "阅读器正文对齐。可选:justify(两端对齐,默认) / left(左对齐)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_title_scale",
        kind: SettingKind::Float { min: 0.8, max: 2.0 },
        default: "1.0",
        comment_zh: "阅读器章题字号相对正文的缩放。范围 0.8–2。默认 1(与正文同大小)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_para_spacing",
        kind: SettingKind::Float { min: 0.0, max: 2.0 },
        default: "0.4",
        comment_zh: "阅读器段落间距(em)。范围 0–2。默认 0.4。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_vertical",
        kind: SettingKind::Enum(&["off", "vertical-rl", "vertical-lr"]),
        default: "off",
        comment_zh: "阅读器版式。可选:off(横排,默认) / vertical-rl(竖排右起) / vertical-lr(竖排左起)。\
切换竖排会重挂载阅读视图,应用时会先保住当前阅读位置。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_autoscroll_sec",
        kind: SettingKind::Int { min: 2, max: 30 },
        default: "5",
        comment_zh: "阅读器自动翻页间隔(秒)。范围 2–30。默认 5。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_theme_light",
        kind: SettingKind::Str,
        default: "follow",
        comment_zh: "浅色模式下的阅读主题 id(见 src/themes/readerThemes.ts)。follow = 跟随应用主题。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_theme_dark",
        kind: SettingKind::Str,
        default: "follow",
        comment_zh: "深色模式下的阅读主题 id。follow = 跟随应用主题。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_pager_mode",
        kind: SettingKind::Enum(&["scroll", "wheel-snap", "keyboard"]),
        default: "scroll",
        comment_zh: "阅读器滚轮/键盘翻页方式。可选:scroll(自由滚动,默认) / wheel-snap(滚轮整屏翻页) / \
keyboard(仅键盘翻页)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_reader_flow",
        kind: SettingKind::Enum(&["paginated", "scrolled"]),
        default: "paginated",
        comment_zh: "阅读器排版流。可选:paginated(分页,默认) / scrolled(连续滚动)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "doc_epub_style_mode",
        kind: SettingKind::Enum(&["book", "reader"]),
        default: "book",
        comment_zh: "EPUB 样式模式。可选:book(保留书籍自带排版,默认) / reader(统一用阅读器排版)。",
        hot: true,
    },
    SettingDef {
        section: "reader",
        key: "reader_panel_expanded",
        kind: SettingKind::BoolMap {
            ids: READER_PANEL_IDS,
        },
        default: "{\"book\":false,\"paging\":false,\"theme\":true,\"typography\":true}",
        comment_zh: "阅读器设置面板内各分组的展开状态(TOML 内联表:分组 id = true/false)。\
分组 id:theme / typography / paging / book。默认主题与排版展开、翻页与书籍折叠。",
        hot: true,
    },
    // ── video ───────────────────────────────────────────────────────────────
    SettingDef {
        section: "video",
        key: "enable_video_cover",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否提取视频封面帧。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "video",
        key: "enable_video_keyframes",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否提取视频关键帧雪碧图(悬停预览用)。默认开启。",
        hot: true,
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
    },
    // ── player(播放器全局偏好:音量/静音/倍速/循环;2026-09-16 起归入 schema)────
    SettingDef {
        section: "player",
        key: "player_volume",
        kind: SettingKind::Float { min: 0.0, max: 1.0 },
        default: "1.0",
        comment_zh: "播放器音量。范围 0–1(0 为静音,1 为满音量)。默认 1.0。",
        hot: true,
    },
    SettingDef {
        section: "player",
        key: "player_muted",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "播放器是否静音(记忆的是静音开关本身,不影响已保存的音量)。默认关闭。",
        hot: true,
    },
    SettingDef {
        section: "player",
        key: "player_rate",
        kind: SettingKind::Float { min: 0.25, max: 3.0 },
        default: "1.0",
        comment_zh: "播放器播放倍速。范围 0.25–3。默认 1.0(原速)。",
        hot: true,
    },
    SettingDef {
        section: "player",
        key: "player_loop",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "播放器是否循环播放。默认关闭。",
        hot: true,
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
    },
    SettingDef {
        section: "ai",
        key: "ai_active_image_file",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "当前激活架构下选中的图像编码器变体文件名。留空 = 使用该架构的默认变体。",
        hot: true,
    },
    SettingDef {
        section: "ai",
        key: "ai_hq_cache_enabled",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "AI 高清缓存(opt-in):开启后后台为每张图额外生成短边≥336 的 WebP 缓存供 CLIP\
分析解码,降低分析时的 CPU/IO 占用。默认关闭。",
        hot: true,
    },
    SettingDef {
        section: "ai",
        key: "ai_provider_override",
        kind: SettingKind::Enum(&["auto", "cpu"]),
        default: "auto",
        comment_zh:
            "AI 推理硬件策略。可选:auto(自动选择,优先 GPU) / cpu(强制仅用 CPU)。默认 auto。",
        hot: true,
    },
    SettingDef {
        section: "ai",
        key: "ai_batch_size",
        kind: SettingKind::UInt,
        default: "0",
        comment_zh: "AI 分析批处理大小。0 = 按显存自动决定;上限 256,固定 batch 的模型会自动\
抬高到其所需下限。典型范围 0–512。默认 0。",
        hot: true,
    },
    SettingDef {
        section: "ai",
        key: "ai_download_source",
        kind: SettingKind::Enum(&["official", "mirror"]),
        default: "official",
        comment_zh: "AI 模型下载首选源。可选:official(官方 HuggingFace 优先) / mirror(国内镜像\
hf-mirror.com 优先)。默认 official。",
        hot: true,
    },
    SettingDef {
        section: "ai",
        key: "ocr_active_tier",
        kind: SettingKind::Str,
        default: "pp-ocrv5-mobile",
        comment_zh: "当前使用中的 OCR 档位 id(见 scrollery-ai-core::ocr_profile)。\
默认 pp-ocrv5-mobile(标准档)。",
        hot: true,
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
    },
    SettingDef {
        section: "face",
        key: "face_model_active",
        kind: SettingKind::Str,
        default: "yunet-sface",
        comment_zh:
            "当前激活的人脸识别模型 id(同时是 faces.model_name 向量空间键)。默认 yunet-sface。",
        hot: true,
    },
    // ── exotic ──────────────────────────────────────────────────────────────
    SettingDef {
        section: "exotic",
        key: "exotic_enabled",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "是否启用冷门格式插件子系统。默认开启。",
        hot: true,
    },
    SettingDef {
        section: "exotic",
        key: "exotic_auto_process",
        kind: SettingKind::Bool,
        default: "true",
        comment_zh: "冷门格式任务是否随扫描自动处理(关闭后仍可在插件页手动触发)。默认开启。",
        hot: true,
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
    },
    SettingDef {
        section: "enhance",
        key: "enhance_model_upscale",
        kind: SettingKind::Str,
        default: "realesrgan-x4plus",
        comment_zh: "影像增强·超分任务当前选用的模型档 id。默认 realesrgan-x4plus。",
        hot: true,
    },
    SettingDef {
        section: "enhance",
        key: "enhance_last_params",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "影像增强对话框上次提交的参数快照(JSON 字符串,供 Dialog 记忆上次选择)。\
留空 = 无记忆。",
        hot: true,
    },
    SettingDef {
        section: "enhance",
        key: "enhance_output_format",
        kind: SettingKind::Enum(&["follow_source", "jpeg", "png"]),
        default: "follow_source",
        comment_zh: "影像增强产物输出格式。可选:follow_source(跟随源,JPEG/PNG) / jpeg / png。\
默认 follow_source。",
        hot: true,
    },
    // ── logging ─────────────────────────────────────────────────────────────
    SettingDef {
        section: "logging",
        key: "log_level",
        kind: SettingKind::Enum(&["trace", "debug", "info", "warn", "error", "off"]),
        default: "info",
        comment_zh: "日志级别。默认 info。",
        hot: true,
    },
    SettingDef {
        section: "logging",
        key: "log_dir",
        kind: SettingKind::Path,
        default: "",
        comment_zh: "日志输出目录(绝对路径)。留空 = 使用默认路径 <app_data_dir>/logs。⚠仅在应用\
启动时读取一次,修改后需重启应用生效。",
        hot: false,
    },
    SettingDef {
        section: "logging",
        key: "log_filter_presets",
        kind: SettingKind::StructList(&LOG_FILTER_PRESET),
        default: "[]",
        comment_zh: "日志窗口的筛选预设列表(TOML 表数组)。每项字段:name(预设名)、levels(级别\
字符串数组,trace/debug/info/warn/error)、target(模块前缀过滤)、text(文本过滤)、textMode\
(substring 子串 / regex 正则)。默认空数组(无预设)。",
        hot: true,
    },
    // ── backup ──────────────────────────────────────────────────────────────
    SettingDef {
        section: "backup",
        key: "backup_dir",
        kind: SettingKind::Path,
        default: "",
        comment_zh: "备份目标目录(绝对路径)。留空 = 未设置,不会执行自动备份。",
        hot: true,
    },
    SettingDef {
        section: "backup",
        key: "backup_auto_enabled",
        kind: SettingKind::Bool,
        default: "false",
        comment_zh: "是否启用每日自动备份(需先设置 backup_dir)。默认关闭。",
        hot: true,
    },
    SettingDef {
        section: "backup",
        key: "backup_retention",
        kind: SettingKind::UInt,
        default: "5",
        comment_zh: "自动备份保留份数上限,超出按时间淘汰最旧备份。典型范围 1–100。默认 5。",
        hot: true,
    },
    // ── proofread ───────────────────────────────────────────────────────────
    SettingDef {
        section: "proofread",
        key: "proofread_base_url",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "远程 AI 校对服务的 API 端点地址。留空 = 未配置。",
        hot: true,
    },
    SettingDef {
        section: "proofread",
        key: "proofread_model",
        kind: SettingKind::Str,
        default: "",
        comment_zh: "远程 AI 校对使用的模型名称。留空 = 未配置。",
        hot: true,
    },
    // ── advanced(进阶调优;批次C接线——每项 hot 依其真实消费点而定,
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
    },
    SettingDef {
        section: "advanced",
        key: "derive_batch_size",
        kind: SettingKind::UInt,
        default: "256",
        comment_zh: "派生任务(视频封面/关键帧/文档缩略图等)流水线单批处理条数。每次派生流水线\
启动时读取一次,无需重启应用——下次手动/自动触发提取即生效。默认 256。",
        hot: true,
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
    },
    SettingDef {
        section: "advanced",
        key: "ai_cache_short_edge",
        kind: SettingKind::UInt,
        default: "336",
        comment_zh: "AI 高清缓存短边像素数。运行时立即生效(与 thumb_webp_quality 同惯例),\
无需重启应用;不触发存量 AI 缓存失效重建。默认 336。",
        hot: true,
    },
    SettingDef {
        section: "advanced",
        key: "relink_match_threshold_pct",
        kind: SettingKind::UInt,
        default: "95",
        comment_zh: "移动/改名文件重新链接时的采样匹配阈值(百分比)。每次执行「重新链接」操作时\
读取一次,无需重启应用。默认 95。",
        hot: true,
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
    },
    SettingDef {
        section: "advanced",
        key: "heavy_video_max_bytes",
        kind: SettingKind::UInt,
        default: "10737418240",
        comment_zh: "判定「重」视频的文件体积阈值(字节)。经启动配置下发到前端、运行时读取,\
无需重启应用。默认 10737418240(10GB)。",
        hot: true,
    },
    SettingDef {
        section: "advanced",
        key: "hover_delay_ms",
        kind: SettingKind::UInt,
        default: "200",
        comment_zh: "鼠标悬停缩略图后延迟多久开始预览(毫秒)。经启动配置下发到前端、运行时读取,\
无需重启应用。默认 200。",
        hot: true,
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
    },
    SettingDef {
        section: "advanced",
        key: "resize_debounce_ms",
        kind: SettingKind::UInt,
        default: "300",
        comment_zh: "窗口/容器尺寸变化后重新计算布局的防抖延迟(毫秒)。经启动配置下发到前端、\
运行时读取,无需重启应用。默认 300。",
        hot: true,
    },
    // ── window(桌面窗口几何;2026-09-16 取代 tauri-plugin-window-state)──────────────
    SettingDef {
        section: "window",
        key: "window_geometry",
        kind: SettingKind::StructMap {
            labels: WINDOW_LABELS,
            value: &WINDOW_GEOMETRY,
        },
        default: "{}",
        comment_zh: "桌面窗口的几何记忆(TOML 内联表:窗口 label = 几何结构)。每个窗口的字段:\
x / y(正常物理位置,副屏可为负)、width / height(正常物理尺寸,像素)、scaleFactor(保存时的\
屏幕缩放)、maximized(是否最大化)。删除某个 label 即让该窗口下次启动回到默认位置与尺寸;\
整表清空(或恢复默认设置)会恢复主窗口 1280×820 居中。移动端忽略本项。",
        // hot=true 的理由:本键的值由窗口模块自行按需读取并即时落位,不经过「启动期一次性读入」
        // 那条路径;而用户每拖动一次窗口都会提交本键,若标 false 就会在每次提交的
        // config-file-changed 载荷里塞进「需重启」,前端照着弹重启提示——那是假提示(窗口此刻
        // 就在用户拖到的位置上,不存在需要重启才生效的说法)。方案 §6 说的是「恢复发生在启动
        // 与重置时」,那是窗口模块的读取时机,不改变本键「无需重启」的结论。
        hot: true,
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
                SettingKind::Int { min, max } => {
                    let n = def
                        .default
                        .parse::<i64>()
                        .unwrap_or_else(|_| panic!("{} 的默认值不是整数:{}", def.key, def.default));
                    assert!(
                        n >= min && n <= max,
                        "{} 的默认值 {n} 不在 {min}–{max} 内",
                        def.key
                    );
                }
                SettingKind::Float { min, max } => {
                    let n = def
                        .default
                        .parse::<f64>()
                        .unwrap_or_else(|_| panic!("{} 的默认值不是小数:{}", def.key, def.default));
                    assert!(
                        n.is_finite() && n >= min && n <= max,
                        "{} 的默认值 {n} 不在 {min}–{max} 内或非有限值",
                        def.key
                    );
                }
                SettingKind::StrList => {
                    assert!(
                        def.default.starts_with('[') && def.default.ends_with(']'),
                        "{} 的默认值不是数组文本:{}",
                        def.key,
                        def.default
                    );
                }
                SettingKind::Struct(_) => {
                    assert!(
                        def.default.starts_with('{') && def.default.ends_with('}'),
                        "{} 的默认值不是对象文本:{}",
                        def.key,
                        def.default
                    );
                }
                SettingKind::BoolMap { ids } => {
                    // 默认值必须是**完整**映射:加载期缺项要按它补齐,漏一个 ID 就会让该 ID
                    // 悄悄变成 false。
                    for id in ids {
                        assert!(
                            def.default.contains(&format!("\"{id}\":")),
                            "{} 的默认映射缺少 ID {id}:{}",
                            def.key,
                            def.default
                        );
                    }
                }
                SettingKind::StructMap { .. } | SettingKind::StructList(_) => {
                    // 空表/空数组是合法默认(尚无窗口记录 / 尚无预设)。
                    assert!(
                        (def.default.starts_with('{') && def.default.ends_with('}'))
                            || (def.default.starts_with('[') && def.default.ends_with(']')),
                        "{} 的默认值不是映射或数组文本:{}",
                        def.key,
                        def.default
                    );
                }
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
