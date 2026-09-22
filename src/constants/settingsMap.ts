import {
  Sun,
  Globe,
  Type,
  Maximize,
  XSquare,
  MessageSquare,
  Monitor,
  Gauge,
  Image,
  Cpu,
  HardDrive,
  Database,
  Settings,
  Terminal,
  Map,
  Trash2,
  RotateCcw,
  Shield,
  Play,
  Video,
  Film,
  Rows3,
  GripVertical,
  PanelBottom,
  AlignCenter,
  AlignHorizontalJustifyCenter,
  FileCog,
  Palette,
} from '@lucide/vue'
import type { Component } from 'vue'

/** 控件注册段；面向用户的页面分组与顺序见 settingsLayout。 */
export type SettingsSection = 'general' | 'thumbnails' | 'video' | 'aiModels' | 'debug' | 'danger'

/** select 类控件的选项:labelKey 走 i18n;语言名等「自名不随界面语言变」的场景用 label 原文。 */
export interface SettingOptionSpec {
  value: string
  labelKey?: string
  label?: string
}

/**
 * 单个设置项的声明式注册(设计 §8):新增常规设置只需在此登记一行 + i18n 文案,
 * SettingsView 的行体与 DynamicSettingControl 的控件形态均由注册表驱动。
 */
export interface SettingSpec {
  icon: Component
  /** 标签 i18n key(历史命名不规则——如 showThumbInfo→thumbInfoHover,须显式声明,不可由 key 推导)。 */
  label: string
  /** 行描述 i18n key;customRow 行的描述结构由 SettingsView 特例模板自带时缺省。 */
  descKey?: string
  /** 短说明；null 表示仅显示标题，完整 descKey 仍可通过「说明」展开。 */
  summaryKey?: string | null
  /** 改名后仍可检索的原术语。 */
  searchTermsKey?: string
  /** 数字控件旁的单位。 */
  unit?: 'px' | '%' | 'KB' | 'MB'
  section: SettingsSection
  /** 控件形态:toggle/select/number 走 DynamicSettingControl 通用分派;
   *  button=动作按钮;segmented/custom=特例控件。 */
  control: 'select' | 'number' | 'toggle' | 'button' | 'segmented' | 'custom'
  /** select 类的选项表(顺序即下拉顺序)。 */
  options?: SettingOptionSpec[]
  /** number 类的输入边界。 */
  min?: number
  max?: number
  /** true=特例行:行体(描述/控件)由 SettingsView 内嵌模板提供,pin/标签仍走注册表。 */
  customRow?: boolean
}

/**
 * 注册表本体。
 * 插入顺序是注册段的默认行序；页面按 settingsLayout 中的功能分组展示。
 *
 * 类型锁(2026-07-06 审查 P1-20):用 `satisfies` 而非 `: Record<string, SettingSpec>` 注解——
 * 后者把键宽化为 string,`keyof typeof` 拿不到具体键集,拼错 settingKey 编译期无感(运行时
 * `undefined.label` 崩)。`satisfies` 既校验每项符合 SettingSpec,又保留字面键集,导出 SettingKey
 * 联合供 props/绑定表用于编译期对账。不加 `as const`:避免 options 变 readonly 波及消费方。
 */
export const SETTINGS_MAP = {
  /* ── 外观 general ─────────────────────────────────────────── */
  theme: {
    icon: Sun,
    label: 'settings.theme',
    descKey: 'settings.themeDesc',
    summaryKey: null,
    section: 'general',
    control: 'select',
    // 特例行:设置页控件为下方 ThemeSettings;select 声明仅供侧栏钉住区 compact 控件使用。
    customRow: true,
    options: [
      { value: 'system', labelKey: 'settings.themeSystem' },
      { value: 'light', labelKey: 'settings.themeLight' },
      { value: 'dark', labelKey: 'settings.themeDark' },
    ],
  },
  language: {
    icon: Globe,
    label: 'settings.language',
    descKey: 'settings.languageDesc',
    summaryKey: null,
    section: 'general',
    control: 'select',
    options: [
      { value: 'zh-CN', label: '简体中文' },
      { value: 'en-US', label: 'English' },
    ],
  },
  // 窗口材质(毛玻璃,2026-08-24):仅 Windows 生效(Rust 侧 DWM 背板,与 uiStore
  // 窗口材质、不透明度与两套配色同属一份主题草稿,控件在 ThemeSettings 内整体呈现;
  // 旧窗口玻璃浓度项(glass*Opacity)随浓度模型一并删除,不再在注册表登记。
  uiFontSize: {
    icon: Type,
    label: 'settings.uiFontSize',
    descKey: 'settings.uiFontSizeDesc',
    summaryKey: null,
    unit: 'px',
    section: 'general',
    control: 'number',
    min: 12,
    max: 24,
  },
  // 标题栏与 Gallery 工具栏合并/分离(useTitlebarMode):开=工具栏并入自绘标题栏同一行(默认,Phase G);
  // 关=独立成标题栏下方第二条 bar(S3 分层)。切换 live 生效。
  titlebarMerged: {
    icon: Rows3,
    label: 'settings.titlebarMerged',
    descKey: 'settings.titlebarMergedDesc',
    summaryKey: 'settings.titlebarSummary',
    section: 'general',
    control: 'toggle',
  },
  // 顶栏中部 chips 簇水平对齐(useToolbarAlign):居中默认/靠左/靠右。仅挪中部折叠区,标题恒左、搜索恒右。
  toolbarAlign: {
    icon: AlignCenter,
    label: 'settings.toolbarAlign',
    descKey: 'settings.toolbarAlignDesc',
    summaryKey: null,
    section: 'general',
    control: 'select',
    options: [
      { value: 'center', labelKey: 'settings.alignCenter' },
      { value: 'left', labelKey: 'settings.alignLeft' },
      { value: 'right', labelKey: 'settings.alignRight' },
    ],
  },
  // 选区操作条停靠状态栏(useSelectionBarMode):开=多选动作并入底部状态栏(替换式);关=浮动可拖胶囊(默认)。
  // 切换 live 生效(经 useSelectionBarMode 共享单例)。
  selectionBarDocked: {
    icon: PanelBottom,
    label: 'settings.selectionBarDocked',
    descKey: 'settings.selectionBarDockedDesc',
    summaryKey: 'settings.selectionBarSummary',
    section: 'general',
    control: 'toggle',
  },
  // 选区浮动胶囊水平对齐(useSelectionBarMode.align):居中默认/靠左/靠右=默认停靠位,拖拽仍可覆盖。
  selectionBarAlign: {
    icon: AlignHorizontalJustifyCenter,
    label: 'settings.selectionBarAlign',
    descKey: 'settings.selectionBarAlignDesc',
    summaryKey: 'settings.selectionAlignSummary',
    section: 'general',
    control: 'select',
    options: [
      { value: 'center', labelKey: 'settings.alignCenter' },
      { value: 'left', labelKey: 'settings.alignLeft' },
      { value: 'right', labelKey: 'settings.alignRight' },
    ],
  },
  // 时间轴两项宽度:轴宽(.timeline-sidebar 列宽,决定密度带/条可见性)与滚动条 thumb 宽相互独立,
  // 同置「界面」节紧挨字号(UI 尺度类),避免落在「缩略图」节难找(原 timelineScrollWidth 之误)。
  timelineAxisWidth: {
    icon: Map,
    label: 'settings.timelineAxisWidth',
    descKey: 'settings.timelineAxisDesc',
    summaryKey: null,
    unit: 'px',
    section: 'general',
    control: 'number',
    min: 32,
    max: 80,
  },
  timelineScrollWidth: {
    icon: Maximize,
    label: 'settings.timelineScrollWidth',
    descKey: 'settings.timelineScrollDesc',
    summaryKey: null,
    unit: 'px',
    section: 'general',
    control: 'number',
    min: 2,
    max: 40,
  },
  scrollThumbMinHeight: {
    icon: Rows3,
    label: 'settings.scrollThumbMinHeight',
    descKey: 'settings.scrollThumbMinHeightDesc',
    summaryKey: 'settings.scrollHeightSummary',
    searchTermsKey: 'settings.scrollHeightSearchTerms',
    unit: 'px',
    section: 'general',
    control: 'number',
    min: 24,
    max: 120,
  },
  // 轴视窗不透明度缩放(2026-07-24):时间轴/minimap 半透明拖动视窗共享,100=默认观感。
  // 上限 200 保时间轴 color-mix 乘后 ≤100%(见 uiScale.applyAxisViewportOpacity)。
  axisViewportOpacity: {
    icon: Map,
    label: 'settings.axisViewportOpacity',
    descKey: 'settings.axisViewportOpacityDesc',
    summaryKey: 'settings.opacitySummary',
    searchTermsKey: 'settings.opacitySearchTerms',
    unit: '%',
    section: 'general',
    control: 'number',
    min: 20,
    max: 200,
  },
  hoverScale: {
    icon: Maximize,
    label: 'settings.hoverScale',
    descKey: 'settings.hoverScaleDesc',
    summaryKey: null,
    searchTermsKey: 'settings.hoverScaleSearchTerms',
    section: 'general',
    control: 'toggle',
  },
  hoverAutoplay: {
    icon: Play,
    label: 'settings.hoverAutoplay',
    descKey: 'settings.hoverAutoplayDesc',
    summaryKey: 'settings.hoverPreviewSummary',
    section: 'general',
    control: 'toggle',
  },
  // 窗口化沉浸模式(2026-07-23):非全屏(且非查看器沉浸)时自动隐藏顶栏/底栏,鼠标移到窗口
  // 边缘唤出。与 F11 全屏沉浸独立——不改全屏行为,只是把同一套“隐藏+唤出”机制挪进窗口态。
  autoHideChromeWindowed: {
    icon: Maximize,
    label: 'settings.autoHideChromeWindowed',
    descKey: 'settings.autoHideChromeWindowedDesc',
    summaryKey: 'settings.autoHideSummary',
    section: 'general',
    control: 'toggle',
  },
  closeBehavior: {
    icon: XSquare,
    label: 'settings.closeBehavior',
    descKey: 'settings.closeBehaviorDesc',
    summaryKey: null,
    section: 'general',
    control: 'select',
    options: [
      { value: 'ask', labelKey: 'settings.closeBehaviorAsk' },
      { value: 'minimize_to_tray', labelKey: 'settings.closeBehaviorMinimize' },
      { value: 'exit', labelKey: 'settings.closeBehaviorExit' },
    ],
  },
  // 查看器渲染色域(2026-07-23 自定义 ICC 与色域切换,方案 B §0⑤):仅作用大图查看器(ContentViewer),
  // 桌面先行、移动端锁 sRGB(D-414,SettingsView 侧以 v-if="!isMobilePlatform" 隐藏本行 + 管理行)。
  viewerColorTarget: {
    icon: Palette,
    label: 'settings.viewerColorTarget',
    descKey: 'settings.viewerColorTargetDesc',
    summaryKey: 'settings.viewerColorSummary',
    searchTermsKey: 'settings.viewerColorSearchTerms',
    section: 'general',
    control: 'select',
    options: [
      { value: 'srgb', labelKey: 'settings.viewerColorSrgb' },
      { value: 'display-p3', labelKey: 'settings.viewerColorDisplayP3' },
      { value: 'dci-p3', labelKey: 'settings.viewerColorDciP3' },
      { value: 'custom', labelKey: 'settings.viewerColorCustom' },
    ],
  },
  // 特例行:自定义 ICC 导入按钮 + 已导入列表(单选/删除),行体由 SettingsView 内嵌模板提供。
  viewerIccManager: {
    icon: Palette,
    label: 'settings.viewerIccManager',
    descKey: 'settings.viewerIccManagerDesc',
    summaryKey: null,
    section: 'general',
    control: 'custom',
    customRow: true,
  },

  /* ── 缩略图 thumbnails ────────────────────────────────────── */
  showDragHandle: {
    icon: GripVertical,
    label: 'settings.showDragHandle',
    descKey: 'settings.showDragHandleDesc',
    summaryKey: null,
    section: 'thumbnails',
    control: 'toggle',
  },
  minimapRenderMode: {
    icon: Map,
    label: 'settings.minimapRenderMode',
    descKey: 'settings.minimapRenderModeDesc',
    summaryKey: 'settings.minimapSummary',
    section: 'thumbnails',
    control: 'select',
    options: [
      { value: 'colors', labelKey: 'settings.minimapRenderColors' },
      { value: 'thumbnails', labelKey: 'settings.minimapRenderThumbnails' },
    ],
  },
  showThumbInfo: {
    icon: MessageSquare,
    label: 'settings.thumbInfoHover',
    descKey: 'settings.thumbInfoHoverDesc',
    summaryKey: null,
    section: 'thumbnails',
    control: 'toggle',
    // 特例行:开关下挂信息元素多选面板。
    customRow: true,
  },
  thumbDecodeStrategy: {
    icon: Cpu,
    label: 'settings.thumbDecodeStrategy',
    descKey: 'settings.thumbDecodeDesc',
    summaryKey: 'settings.decodeSummary',
    section: 'thumbnails',
    control: 'select',
    options: [
      { value: 'cpu', labelKey: 'settings.thumbStrategyCpu' },
      { value: 'gpu', labelKey: 'settings.thumbStrategyGpu' },
      { value: 'direct', labelKey: 'settings.thumbStrategyDirect' },
    ],
  },
  gpuEngine: {
    icon: Monitor,
    label: 'settings.gpuEngine',
    descKey: 'settings.gpuEngineDesc',
    summaryKey: null,
    section: 'thumbnails',
    control: 'select',
    options: [{ value: 'wic', labelKey: 'settings.gpuEngineWic' }],
  },
  thumbCacheDir: {
    icon: HardDrive,
    label: 'settings.thumbCacheDir',
    section: 'thumbnails',
    control: 'button',
    // 特例行:描述=可点击路径,控件=换目录按钮。
    customRow: true,
  },
  thumbSize: {
    icon: Image,
    label: 'settings.thumbSize',
    descKey: 'settings.thumbSizeHint',
    summaryKey: 'settings.thumbSizeSummary',
    section: 'thumbnails',
    control: 'segmented',
  },
  // WebP 编码质量:1..=99 有损,100=无损(encode_as_webp 契约);变更即复位存量重生成。
  thumbWebpQuality: {
    icon: Image,
    label: 'settings.thumbWebpQuality',
    descKey: 'settings.thumbWebpQualityDesc',
    summaryKey: 'settings.thumbQualitySummary',
    section: 'thumbnails',
    control: 'number',
    min: 1,
    max: 100,
  },
  thumbSkipMaxKb: {
    icon: Shield,
    label: 'settings.thumbSkipMaxKb',
    descKey: 'settings.thumbSkipDesc',
    summaryKey: 'settings.thumbSkipSummary',
    unit: 'KB',
    section: 'thumbnails',
    control: 'number',
    min: 0,
    max: 1000000,
  },
  thumbCacheMaxMb: {
    icon: HardDrive,
    label: 'settings.thumbCacheMaxMb',
    descKey: 'settings.thumbCacheDesc',
    summaryKey: 'settings.thumbCacheSummary',
    unit: 'MB',
    section: 'thumbnails',
    control: 'number',
    min: 100,
    max: 100000,
  },
  // 缓存占用统计(§3.3.3):后端 get_cache_stats 遍历缓存目录,分类目字节+文件数;
  // 特例行=进入自动拉取一次 + 手动刷新按钮(遍历大缓存有秒级 IO,不做轮询)。
  cacheStats: {
    icon: Database,
    label: 'settings.cacheStats',
    section: 'thumbnails',
    control: 'custom',
    customRow: true,
  },
  fullThumbGen: {
    icon: Image,
    label: 'settings.fullThumbGen',
    descKey: 'settings.fullThumbGenDesc',
    summaryKey: 'settings.fullThumbSummary',
    section: 'thumbnails',
    control: 'custom',
    // 特例行:生成进度条 + 启停按钮。
    customRow: true,
  },

  /* ── 视频 video ───────────────────────────────────────────── */
  enableVideoCover: {
    icon: Video,
    label: 'settings.enableVideoCover',
    descKey: 'settings.enableVideoCoverDesc',
    summaryKey: null,
    section: 'video',
    control: 'toggle',
  },
  enableVideoKeyframes: {
    icon: Film,
    label: 'settings.enableVideoKeyframes',
    descKey: 'settings.enableVideoKeyframesDesc',
    summaryKey: 'settings.videoKeyframesSummary',
    section: 'video',
    control: 'toggle',
  },
  videoDeriveGen: {
    icon: Video,
    label: 'settings.videoDeriveGen',
    descKey: 'settings.videoDeriveGenDesc',
    summaryKey: 'settings.videoGenerationSummary',
    section: 'video',
    control: 'custom',
    // 特例行:视频封面/关键帧手动提取(增量/全量/停止 + 进度),镜像 fullThumbGen。
    customRow: true,
  },

  /* ── AI 模型 aiModels ─────────────────────────────────────── */
  aiEngineStatus: {
    icon: Cpu,
    label: 'settings.aiEngineStatus',
    section: 'aiModels',
    control: 'button',
    // 特例行:描述=引擎/显存/模型加载状态,控件=测试加载按钮。
    customRow: true,
  },
  aiHqCache: {
    icon: Image,
    label: 'settings.aiHqCache',
    descKey: 'settings.aiHqCacheDesc',
    summaryKey: 'settings.aiCacheSummary',
    section: 'aiModels',
    control: 'toggle',
  },
  aiBatchSize: {
    icon: Database,
    label: 'settings.aiBatchSize',
    descKey: 'settings.aiBatchSizeDesc',
    summaryKey: 'settings.aiBatchSummary',
    section: 'aiModels',
    // custom:数字输入外挂固定 batch 钳制与风险提示(DynamicSettingControl 按键特判)。
    control: 'custom',
    min: 0,
    max: 512,
  },
  aiHardwareStrategy: {
    icon: Cpu,
    label: 'settings.aiHardwareStrategy',
    descKey: 'settings.aiHardwareDesc',
    summaryKey: 'settings.aiHardwareSummary',
    section: 'aiModels',
    control: 'select',
    options: [
      { value: 'auto', labelKey: 'settings.aiAutoHardware' },
      { value: 'cpu', labelKey: 'settings.aiForceCpu' },
    ],
  },

  /* ── 开发者工具 debug(非破坏性诊断项)──────────────────────── */
  // 画廊 / 时间轴 DOM↔Canvas 渲染引擎(实验性):canvas 为原型,超大库 / iOS 会自动回退 DOM。
  // 状态经 useRenderMode 共享单例(存 config.toml),切换 live 生效。
  performancePanel: {
    icon: Gauge,
    label: 'settings.performancePanel',
    descKey: 'settings.performancePanelDesc',
    section: 'debug',
    control: 'button',
  },

  galleryRenderMode: {
    icon: Monitor,
    label: 'settings.galleryRenderMode',
    descKey: 'settings.galleryRenderModeDesc',
    section: 'debug',
    control: 'select',
    options: [
      { value: 'dom', labelKey: 'settings.renderModeDom' },
      { value: 'canvas', labelKey: 'settings.renderModeCanvas' },
    ],
  },
  timelineRenderMode: {
    icon: Monitor,
    label: 'settings.timelineRenderMode',
    descKey: 'settings.timelineRenderModeDesc',
    section: 'debug',
    control: 'select',
    options: [
      { value: 'dom', labelKey: 'settings.renderModeDom' },
      { value: 'canvas', labelKey: 'settings.renderModeCanvas' },
    ],
  },
  logLevel: {
    icon: Terminal,
    label: 'settings.logLevel',
    descKey: 'settings.logLevelDesc',
    section: 'debug',
    control: 'select',
    options: [
      { value: 'trace', labelKey: 'settings.logLevelTrace' },
      { value: 'debug', labelKey: 'settings.logLevelDebug' },
      { value: 'info', labelKey: 'settings.logLevelInfo' },
      { value: 'warn', labelKey: 'settings.logLevelWarn' },
      { value: 'error', labelKey: 'settings.logLevelError' },
      { value: 'off', labelKey: 'settings.logLevelOff' },
    ],
  },
  logDir: {
    icon: HardDrive,
    label: 'settings.logDir',
    section: 'debug',
    control: 'button',
    // 特例行:描述=可点击路径,控件=换目录按钮。
    customRow: true,
  },
  // 外置配置文件(config.toml,批次B):路径展示 + 用外部编辑器打开按钮。
  configFile: {
    icon: FileCog,
    label: 'settings.configFile',
    section: 'debug',
    control: 'button',
    // 特例行:描述=说明文案+当前路径,控件=打开编辑器按钮。
    customRow: true,
  },
  // 独立日志窗口入口(日志能力重构 S4,方案 §5「设置区入口+独立日志窗口」)。
  openLogWindow: {
    icon: Terminal,
    label: 'settings.openLogWindow',
    descKey: 'settings.openLogWindowDesc',
    section: 'debug',
    control: 'button',
  },
  // 语义是纯前端 cache-busting 重载,不销毁任何数据——不归 danger(设计 §8.4:Error 只用于 destructive)。
  clearBrowserCache: {
    icon: RotateCcw,
    label: 'settings.clearBrowserCache',
    descKey: 'settings.clearBrowserCacheDesc',
    section: 'debug',
    control: 'button',
  },

  /* ── 危险操作 danger(设计 §7.2:破坏性项不与普通设置混排;行序=破坏性递增,清库最后)── */
  clearAllThumbnails: {
    icon: Trash2,
    label: 'settings.clearAllThumbnails',
    descKey: 'settings.clearAllThumbnailsDesc',
    section: 'danger',
    control: 'button',
  },
  clearLogs: {
    icon: Trash2,
    label: 'settings.clearLogs',
    descKey: 'settings.clearLogsDesc',
    section: 'danger',
    control: 'button',
  },
  clearSettings: {
    icon: Settings,
    label: 'settings.resetSettings',
    descKey: 'settings.resetSettingsDesc',
    section: 'danger',
    control: 'button',
  },
  clearDb: {
    icon: Database,
    label: 'settings.clearDb',
    descKey: 'settings.clearDbDesc',
    section: 'danger',
    control: 'button',
  },
} satisfies Record<string, SettingSpec>

/** 注册表的合法键联合(P1-20):props / 绑定表用它替代宽 string,拼错键即编译期红。 */
export type SettingKey = keyof typeof SETTINGS_MAP

/**
 * 按运行时字符串键安全检索(P1-20):键可能来自持久化的置顶列表(可含已废弃键),
 * 故返回 `SettingSpec | undefined`——消费方必须判空,杜绝原先 `SETTINGS_MAP[typo].label`
 * 的运行时 `undefined.label` 崩溃。编译期已知键的场景直接用 `SETTINGS_MAP[key as SettingKey]`。
 */
export function getSettingSpec(key: string): SettingSpec | undefined {
  return (SETTINGS_MAP as Record<string, SettingSpec>)[key]
}

/** 按注册表顺序读取该段的设置键，供页面分组声明复用。 */
export function sectionSettingKeys(section: SettingsSection): SettingKey[] {
  return (Object.entries(SETTINGS_MAP) as [SettingKey, SettingSpec][])
    .filter(([, spec]) => spec.section === section)
    .map(([key]) => key)
}
