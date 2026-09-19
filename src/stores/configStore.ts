// src/stores/configStore.ts
// 设置页消费的配置值(缩略图/视频/AI/查看器色域等)：全部读中央设置快照(config.toml 唯一真源)。
//
// 存储姿态(设置集中保存,2026-09-16):本 store 不再自持持久化,也不再逐键 get_app_config。
// 取值经 readSetting* 读中央快照(响应式,重置/外部编辑/跨窗口同步后自动跟新);提交经
// writeSettings 走统一批量入口;派生流水线重启由后端按批量差异统一触发,故这里不再逐 setter 重启。
import { computed, watch } from 'vue'
import { defineStore } from 'pinia'
import { logger, setLoggerEnabled } from '../utils/logger'
import {
  initializeSettings,
  readSetting,
  refreshSettingsFromBackend,
  settingsConfirmedValues,
  settingsReady,
  writeSettings,
} from './settingsPersistence'

/** 读**已确认**枚举值:未确认(尚未落盘)时回落默认,供不能先于提交切换的消费点使用。 */
function readConfirmedEnum<T extends string>(
  key: string,
  allowed: readonly T[],
  fallback: T,
): T {
  const raw = settingsConfirmedValues.value[key]
  return raw !== undefined && (allowed as readonly string[]).includes(raw) ? (raw as T) : fallback
}
import {
  readSettingBool,
  readSettingEnum,
  readSettingNumber,
} from '../composables/settingsValues'
import {
  applyUiFontSize,
  applyTimelineScrollWidth,
  applyTimelineAxisWidth,
  applyAxisViewportOpacity,
} from '../utils/uiScale'

/** 缩略图解码策略候选(与后端 schema 枚举同值)。 */
const THUMB_STRATEGIES = ['cpu', 'gpu', 'direct'] as const
/** GPU 引擎候选。 */
const GPU_ENGINES = ['wic'] as const
/** AI 硬件策略候选。 */
const AI_PROVIDER_OVERRIDES = ['auto', 'cpu'] as const
/** AI 模型下载源候选。 */
const AI_DOWNLOAD_SOURCES = ['official', 'mirror'] as const
/** 查看器渲染色域候选。 */
const VIEWER_COLOR_TARGETS = ['srgb', 'display-p3', 'dci-p3', 'custom'] as const
/** 日志级别候选(与后端 tracing 级别同名)。 */
const LOG_LEVELS = ['trace', 'debug', 'info', 'warn', 'error', 'off'] as const

export const useConfigStore = defineStore('config', () => {
  // ── 中央设置的单键读取(响应式) ─────────────────────────────────────────
  // 数值键用 readSettingNumber(非有限文本回落默认),枚举键用 readSettingEnum(陌生值回落默认)。
  const thumbSkipMaxKb = computed(() => readSettingNumber('thumb_skip_max_kb', 200))
  // 缓存 LRU 预算默认 10GB(2026-07-19 裁决:不做访问追踪,放宽预算降驱逐频率;
  // 与后端 DEFAULT_THUMB_CACHE_MAX_MB 同值)。
  const thumbCacheMaxMb = computed(() => readSettingNumber('thumb_cache_max_mb', 10240))
  const thumbSize = computed(() => readSettingNumber('thumb_size', 512))
  // 缩略图 WebP 编码质量:1..=99 有损(默认 80),100=无损。变更后后端会把存量已生成项
  // 与封面派生一并复位,按需以新质量重生成。
  const thumbWebpQuality = computed(() => readSettingNumber('thumb_webp_quality', 80))
  const timelineScrollWidth = computed(() => readSettingNumber('timeline_scroll_width', 8))
  const timelineAxisWidth = computed(() => readSettingNumber('timeline_axis_width', 44))
  // 滚动条 thumb 与时间轴视窗共享的最小高度(px):二者同高、同 thumbGeometry 几何。纯 JS 数值,
  // 经 MediaGrid 以 prop 下发(非 CSS 变量);默认 48。
  const scrollThumbMinHeight = computed(() => readSettingNumber('scroll_thumb_min_height', 48))
  // 轴视窗不透明度缩放(百分比,100=默认):时间轴/minimap 半透明拖动视窗共享,经 CSS 变量
  // --axis-viewport-opacity(pct/100 无量纲乘数)下发。范围 20–200(上限保 color-mix ≤100%)。
  const axisViewportOpacity = computed(() => readSettingNumber('axis_viewport_opacity', 100))
  const uiFontSize = computed(() => readSettingNumber('ui_font_size', 13))
  const enableHoverScale = computed(() => readSettingBool('enable_thumb_hover_scale', true))
  const logLevel = computed(() => readSettingEnum('log_level', LOG_LEVELS, 'info'))
  const thumbStrategy = computed(() => readSettingEnum('thumb_strategy', THUMB_STRATEGIES, 'cpu'))
  const gpuEngine = computed(() => readSettingEnum('gpu_engine', GPU_ENGINES, 'wic'))
  const aiProviderOverride = computed(() =>
    readSettingEnum('ai_provider_override', AI_PROVIDER_OVERRIDES, 'auto'),
  )
  const aiBatchSize = computed(() => readSettingNumber('ai_batch_size', 0))
  // AI 模型下载首选源:'official'=官方 HuggingFace 优先,'mirror'=国内镜像 hf-mirror.com 优先。
  const aiDownloadSource = computed(() =>
    readSettingEnum('ai_download_source', AI_DOWNLOAD_SOURCES, 'official'),
  )
  // 视频派生开关:是否提取视频封面 / 关键帧雪碧图(默认开启)。
  const enableVideoCover = computed(() => readSettingBool('enable_video_cover', true))
  const enableVideoKeyframes = computed(() => readSettingBool('enable_video_keyframes', true))
  // AI 高清缓存开关(opt-in,默认关):开启后后台静默为每张图生成短边≥336 的 WebP 缓存,
  // 使 CLIP 分析解码该小缓存而非全分辨率原图,大幅降低分析时的 CPU 占用。
  const aiHqCache = computed(() => readSettingBool('ai_hq_cache_enabled', false))
  // 查看器渲染色域(自定义 ICC 与色域切换,方案 B §0⑤):target 枚举 srgb/display-p3/dci-p3/custom,
  // 默认 srgb(=零派生,直显原图);custom 时 customId 指向已导入 ICC 的 16-hex id。
  //
  // 这两项读**已确认值**(settingsConfirmedValues)而非即时预览:后端按 ConfigManager 单源解析
  // 色域 URL,若前端在提交前就切显示态,useViewerColorSource 的 watch 会立刻发渲染请求,与写盘
  // 竞速、偶发按旧色域渲染(旧 setter 的「先 await 保存、后改本地」正是为此)。确认值在写盘
  // 回执或外部编辑事件到达时才更新,故显示与渲染都不会跑到提交前面;失败时保持原值。
  const viewerColorTarget = computed(() =>
    readConfirmedEnum('viewer_color_target', VIEWER_COLOR_TARGETS, 'srgb'),
  )
  const viewerColorCustomId = computed(
    () => settingsConfirmedValues.value['viewer_color_custom_id'] ?? '',
  )
  // OCR 文字提取(B′ 路线,T10):当前使用中的档位 id,默认标准档 pp-ocrv5-mobile。
  const ocrTier = computed(() => readSetting('ocr_active_tier') ?? 'pp-ocrv5-mobile')

  function applyHoverScale(enabled: boolean) {
    if (enabled) document.documentElement.classList.remove('disable-hover-scale')
    else document.documentElement.classList.add('disable-hover-scale')
  }

  /**
   * 把「影响 DOM 的配置值」推到界面:字号、悬停缩放类、滚动条宽/轴宽 CSS 变量、轴视窗不透明度。
   *
   * 单一应用点:启动水合、恢复默认、外部编辑与用户改动都只是「快照变了」,故一次 watch 收敛;
   * 各 setter 不再各自推 DOM(此前每个 setter 各推一份,漏一个就静默不生效)。Canvas 不经过
   * `.media-card:hover` CSS,悬停缩放类必须同样在此应用。
   */
  function applyDomAffectingValues() {
    applyUiFontSize(uiFontSize.value)
    applyHoverScale(enableHoverScale.value)
    document.documentElement.style.setProperty(
      '--scrollbar-width',
      `${timelineScrollWidth.value}px`,
    )
    document.documentElement.style.setProperty(
      '--timeline-axis-width',
      `${timelineAxisWidth.value}px`,
    )
    applyAxisViewportOpacity(axisViewportOpacity.value)
  }

  watch(
    [uiFontSize, enableHoverScale, timelineScrollWidth, timelineAxisWidth, axisViewportOpacity],
    applyDomAffectingValues,
  )

  /**
   * 兼容既有调用点:配置已由中央服务在启动时水合,这里只确保快照到位并应用 CSS 变量。
   * 保留 async 与幂等语义,消费方(SettingsView / ModelLibrary / OcrModelSection)无需改动。
   */
  async function loadConfig() {
    if (settingsReady.value) return
    try {
      await initializeSettings()
    } catch (e) {
      logger.error('Failed to load config for settings view', { error: e })
    }
  }

  /**
   * 提交单个设置键(规范文本)。走统一批量入口:合并、防抖、代次守卫与失败提示都由中央服务负责。
   * 保留本方法供设置页的路径类输入(thumb_cache_dir / log_dir)与备份分节使用。
   */
  async function saveConfig(key: string, value: string) {
    await writeSettings({ [key]: value })
  }

  /** 外置配置文件热更新:中央服务已统一应用快照,DOM 副作用由 applyDomAffectingValues 的 watch 收敛。 */
  async function refreshFromBackend() {
    await refreshSettingsFromBackend()
  }

  // ── 各设置项的提交入口 ─────────────────────────────────────────────────
  // 统一姿态:只提交,不本地赋值——显示値由中央快照驱动(采集即预览由中央服务负责),
  // 因此不存在「本地先改、后端后改」的抢跑,失败时也不会留下与后端不一致的本地值。

  function setThumbSkipMaxKb(val: number) {
    return writeSettings({ thumb_skip_max_kb: val.toString() })
  }
  function setThumbCacheMaxMb(val: number) {
    return writeSettings({ thumb_cache_max_mb: val.toString() })
  }
  function setThumbSize(val: number) {
    return writeSettings({ thumb_size: val.toString() })
  }
  /** 编码质量(1-100,100=无损):先收敛再提交。 */
  function setThumbWebpQuality(val: number) {
    const clamped = Math.min(100, Math.max(1, Math.round(val)))
    return writeSettings({ thumb_webp_quality: clamped.toString() })
  }
  function setTimelineScrollWidth(val: number) {
    applyTimelineScrollWidth(val)
    return writeSettings({ timeline_scroll_width: val.toString() })
  }
  function setTimelineAxisWidth(val: number) {
    applyTimelineAxisWidth(val)
    return writeSettings({ timeline_axis_width: val.toString() })
  }
  function setScrollThumbMinHeight(val: number) {
    // 纯 JS 数值(非 CSS 变量):快照变化经 MediaGrid prop 实时下发到组件。
    return writeSettings({ scroll_thumb_min_height: val.toString() })
  }
  /** 轴视窗不透明度:先收敛再提交,与 applyAxisViewportOpacity 的区间一致(20–200)。 */
  function setAxisViewportOpacity(val: number) {
    const clamped = Math.min(200, Math.max(20, Math.round(val)))
    applyAxisViewportOpacity(clamped)
    return writeSettings({ axis_viewport_opacity: clamped.toString() })
  }
  function setUiFontSize(val: number) {
    // 单一事实源(P1-18):与 App.vue 启动应用共用同一函数,消除 1px 漂移。
    applyUiFontSize(val)
    return writeSettings({ ui_font_size: val.toString() })
  }
  function setEnableHoverScale(val: boolean) {
    applyHoverScale(val)
    return writeSettings({ enable_thumb_hover_scale: val.toString() })
  }
  function setLogLevel(val: string) {
    // 切换态同步(方案 §4):off 时立刻关掉前端队列开关,不等落盘往返;
    // 启动水合与外部编辑路径同样经 uiStore 的快照 watch 同步,口径一致。
    setLoggerEnabled(val !== 'off')
    return writeSettings({ log_level: val })
  }
  function setThumbStrategy(val: string) {
    return writeSettings({ thumb_strategy: val })
  }
  function setGpuEngine(val: string) {
    return writeSettings({ gpu_engine: val })
  }
  function setAiProviderOverride(val: string) {
    return writeSettings({ ai_provider_override: val })
  }
  function setAiBatchSize(val: number) {
    return writeSettings({ ai_batch_size: val.toString() })
  }
  function setAiDownloadSource(val: string) {
    return writeSettings({ ai_download_source: val })
  }
  function setEnableVideoCover(val: boolean) {
    return writeSettings({ enable_video_cover: val.toString() })
  }
  function setEnableVideoKeyframes(val: boolean) {
    return writeSettings({ enable_video_keyframes: val.toString() })
  }
  /** AI 高清缓存开关:开启 → 后台静默 backfill 并生成 ai_thumb 缓存;关闭 → 生产者排除 ai_thumb。 */
  function setAiHqCache(val: boolean) {
    return writeSettings({ ai_hq_cache_enabled: val.toString() })
  }
  function setViewerColorTarget(val: string) {
    return writeSettings({ viewer_color_target: val })
  }
  function setViewerColorCustomId(val: string) {
    return writeSettings({ viewer_color_custom_id: val })
  }
  function setOcrTier(val: string) {
    return writeSettings({ ocr_active_tier: val })
  }

  return {
    // 读値(响应式,来自中央快照)
    thumbSkipMaxKb,
    thumbCacheMaxMb,
    thumbSize,
    thumbWebpQuality,
    timelineScrollWidth,
    timelineAxisWidth,
    scrollThumbMinHeight,
    axisViewportOpacity,
    uiFontSize,
    enableHoverScale,
    logLevel,
    thumbStrategy,
    gpuEngine,
    aiProviderOverride,
    aiBatchSize,
    aiDownloadSource,
    enableVideoCover,
    enableVideoKeyframes,
    aiHqCache,
    viewerColorTarget,
    viewerColorCustomId,
    ocrTier,
    // 生命周期与提交
    loadConfig,
    saveConfig,
    refreshFromBackend,
    applyDomAffectingValues,
    setThumbSkipMaxKb,
    setThumbCacheMaxMb,
    setThumbSize,
    setThumbWebpQuality,
    setTimelineScrollWidth,
    setTimelineAxisWidth,
    setScrollThumbMinHeight,
    setAxisViewportOpacity,
    setUiFontSize,
    setEnableHoverScale,
    setLogLevel,
    setThumbStrategy,
    setGpuEngine,
    setAiProviderOverride,
    setAiBatchSize,
    setAiDownloadSource,
    setEnableVideoCover,
    setEnableVideoKeyframes,
    setAiHqCache,
    setViewerColorTarget,
    setViewerColorCustomId,
    setOcrTier,
  }
})
