// src/stores/uiStore.ts
// 全局 UI 状态 — 在有说明的地方持久化到 app_config。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getAppWindow } from '../utils/appWindow'
import { isMac, isWindows } from '../utils/platform'
import type { AppearanceMode } from '../types/ui'
import type { TreeDisplayMode } from '../types/media'
import { IPC } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'
import { logger, setLoggerEnabled } from '../utils/logger'
import i18n from '../i18n'
import {
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  DEFAULT_THEME_STYLE,
  getTheme,
  normalizeThemeId,
  themeIdForStyle,
} from '../themes/registry'
import type { ThemeStyle } from '../themes/registry'
import { DEFAULTS } from '../constants/defaults'
import {
  applyGlassGalleryOpacity,
  applyGlassOpacityScale,
  clampGlassGalleryOpacity,
  clampGlassOpacity,
  GLASS_GALLERY_OPACITY_DEFAULT,
  GLASS_OPACITY_DEFAULT,
} from '../utils/uiScale'
import {
  applyThemeTextStrength,
  applyThemeTintStrength,
  clampThemeText,
  clampThemeTint,
  THEME_TEXT_DEFAULT,
  THEME_TINT_DEFAULT,
} from '../themes/strength'

export type MinimapRenderMode = 'colors' | 'thumbnails'

/**
 * 记录一次「用户主动选择的画廊组内排序模式」使用计数(埋点)——服务「先测 filename 使用频率」
 * 决策门(是否值得为 filename 序做专门优化,见 docs/planning/2026-07-14-统一文件树与画廊目录排序/
 * 方案B §15)。**仅统计真实用户动作**:唯一来源是排序下拉的 change → `setSortWithinGroup` 的
 * `persist===true` 路径;AI 切 similarity、URL/pref 恢复均走 `persist=false`,不计(否则每次启动/
 * 导航都虚增,污染频率)。
 *
 * **异步、绝不阻塞交互**(按用户要求做成异步避免影响性能):读-改-写 localStorage 放进
 * `requestIdleCallback` 空闲帧执行(同 thumbhash.ts 惯用法),失败静默(隐私模式/配额满时
 * localStorage 抛错不得影响排序)。数据留本地(单机、无遥测),devtools 读 `localStorage.sortModeUsage`
 * 即得 `{datetime,filename,similarity}` 分布。
 */
function recordSortModeUsage(mode: 'datetime' | 'filename' | 'similarity') {
  const write = () => {
    try {
      const KEY = 'sortModeUsage'
      const raw = localStorage.getItem(KEY)
      const counts = raw ? (JSON.parse(raw) as Record<string, number>) : {}
      counts[mode] = (counts[mode] ?? 0) + 1
      localStorage.setItem(KEY, JSON.stringify(counts))
    } catch {
      // 埋点尽力而为:localStorage 不可用(隐私模式/配额)时静默放弃,绝不影响主流程。
    }
  }
  // 空闲帧异步写,避免读-改-写 localStorage 触碰排序交互关键路径;无 requestIdleCallback
  // (旧 webview / SSR 单测)时用宏任务兜底,仍不阻塞当前调用栈。
  if (typeof window !== 'undefined' && 'requestIdleCallback' in window) {
    window.requestIdleCallback(write)
  } else {
    setTimeout(write, 0)
  }
}

/** `SET_APP_CONFIG` 失败的统一记录口(本文件近 20 处 `.catch(console.error)` 的公共尾巴;
 *  失败静默不阻断交互,但须留证据——带 key 便于在 JSONL 里按配置项定位)。 */
function logConfigSaveError(key: string) {
  return (e: unknown) => logger.error(`setAppConfig failed: ${key}`, { key, error: e })
}

// get_startup_config 的载荷(R2-4:14 键单次往返;与后端 config_commands.rs StartupConfig 同步)。
export interface StartupConfig {
  language: string | null
  timelineScrollWidth: string | null
  timelineAxisWidth: string | null
  scrollThumbMinHeight: string | null
  uiFontSize: string | null
  enableThumbHoverScale: string | null
  gridRowHeight: string | null
  groupBy: string | null
  sortWithinGroup: string | null
  layoutMode: string | null
  closeBehavior: string | null
  pinnedSettings: string | null
  showThumbInfo: string | null
  thumbInfoElements: string | null
  hoverAutoplay: string | null
  bucketSegmentedScroll: string | null
  // 多主题 S1(2026-07-06):外观三键 + legacy theme(迁移只读)→ 19 键,与后端同步。
  theme: string | null
  appearance: string | null
  themeLight: string | null
  themeDark: string | null
  firstLaunch: string | null
  // 详细首次使用引导手册(设计文档):独立于首启向导 first_launch,首次关闭手册后写 true 不再自动弹,
  // 设置入口仍可随时强开(35 键,与后端同步)。
  guideSeen: string | null
  // 文件树显示范围 S 线(2026-07-16)→ 20 键,与后端同步。
  treeDisplayMode: string | null
  // 选择态缩略图拖拽手柄显隐(2026-07-17 #5)→ 21 键,与后端同步。
  showDragHandle: string | null
  // 无缝分组(2026-07-17 #1)→ 22 键,与后端同步。
  seamlessGroups: string | null
  // 无缝 minimap 轴显隐(2026-07-17)+ 渲染模式(2026-07-19)→ 24 键,与后端同步。
  seamlessMinimap: string | null
  minimapRenderMode: string | null
  // 日志能力重构 S3(2026-07-20):logger.ts 的 off 档联动须在启动早期就位(不能等设置页才读)
  // → 25 键,与后端同步。
  logLevel: string | null
  // 配置重构批次C(2026-07-22):5 个前端阈值键(advanced,新增字段,不改旧字段位置/类型)
  // → 30 键,与后端同步。均为 hot——config-file-changed 后 refreshFromBackend 重拉即生效,
  // 消费点(useHoverPreview.ts / useJustifiedLayout.ts / AppToolbar.vue / HGalleryLabView.vue)
  // 一律响应式读取(函数体内取值,非组合式函数初始化时快照),无需重启应用。
  heavyVideoMaxPixels: string | null
  heavyVideoMaxBytes: string | null
  hoverDelayMs: string | null
  searchDebounceMs: string | null
  resizeDebounceMs: string | null
  // 小批 C2(2026-07-22):雪碧图切帧列数,须与后端提取帧数(video_keyframe_count)同源,
  // 否则用户改键后前端仍按旧硬编码列数切新雪碧图 → 画面错位 → 31 键,与后端同步。
  videoKeyframeCount: string | null
  // 窗口化沉浸模式(2026-07-23):非全屏时自动隐藏顶栏/底栏,鼠标移边缘唤出 → 32 键,与后端同步。
  autoHideChromeWindowed: string | null
  // 轴视窗不透明度缩放(2026-07-24):时间轴/minimap 拖动视窗,百分比 → 33 键,与后端同步。
  axisViewportOpacity: string | null
  // 轴形态偏好(2026-07-24 画廊轴/minimap 重构):timeline|minimap,两分组模式通用,
  // 持久化替代原会话态 preferredAxis → 34 键,与后端同步。
  axisMode: string | null
  // 窗口材质(毛玻璃,2026-08-24):mica|acrylic|none,与后端 window_material 键同值,
  // 仅 Windows 生效(Rust 侧 window_vibrancy DWM 背板)→ 35 键,与后端同步。
  windowMaterial: string | null
  // 毛玻璃分层不透明度缩放(2026-08-25):100=各材质当前默认观感,20–120,与后端同步。
  glassChromeOpacity: string | null
  glassStickyOpacity: string | null
  glassSurfaceOpacity: string | null
  glassControlOpacity: string | null
  // 内容底面缩放(2026-09-06):文字密集视图根承重面,20–120,与后端同步。
  glassContentOpacity: string | null
  // 画廊大底/图片间隙遮罩(2026-08-25):0=完全透出 DWM 材质,与四组表面缩放独立。
  glassGalleryOpacity: string | null
  // 主题色浓度(2026-09-06):底色 wash token 的 color-mix 缩放,20–100,与后端同步。
  themeTintStrength: string | null
  // 文字浓度(2026-09-06):文字 ramp 的 color-mix 缩放,40–100,与后端同步。
  themeTextStrength: string | null
}

export const useUiStore = defineStore('ui', () => {
  // ── 外观与语言 ──────────────────────────────────────────────────────────
  // 多主题三键模型:appearance = 亮/暗/跟随系统(外观模式);lightThemeId/darkThemeId =
  // 亮暗两个槽位各自选定的主题 id(指向 src/themes/registry 注册表)。
  // data-theme 从此单源:只有 applyAppearance 写 documentElement,AppShell 不再二次绑定。
  const appearance = ref<AppearanceMode>('system')
  const lightThemeId = ref<string>(DEFAULT_LIGHT_THEME)
  const darkThemeId = ref<string>(DEFAULT_DARK_THEME)
  const language = ref<string>('zh-CN')

  const systemIsDark = ref(window.matchMedia('(prefers-color-scheme: dark)').matches)

  // Listen for OS theme changes globally
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
    systemIsDark.value = e.matches
    if (appearance.value === 'system') {
      applyAppearance()
    }
  })

  // 亮槽主题恒为 light kind、暗槽恒为 dark kind(模型不变量),故明暗判定只看外观模式。
  const isDark = computed(() =>
    appearance.value === 'system' ? systemIsDark.value : appearance.value === 'dark',
  )

  // 当前应落到 data-theme 的主题 id(外观模式 → 槽位 → id)。
  const resolvedThemeId = computed(() => (isDark.value ? darkThemeId.value : lightThemeId.value))

  // 当前生效主题所属风格。启动水合会把两个槽位收敛成同一风格，
  // 这里仍保留 fresh 回退以防运行时收到外部注入的陌生 id。
  const themeStyle = computed<ThemeStyle>(
    () => getTheme(resolvedThemeId.value)?.style ?? DEFAULT_THEME_STYLE,
  )

  function applyLanguage(lang: string) {
    language.value = lang
    document.documentElement.setAttribute('lang', lang)
    if (i18n.global.locale.value !== lang) {
      i18n.global.locale.value = lang as typeof i18n.global.locale.value
    }
  }

  function setLanguage(lang: string) {
    applyLanguage(lang)
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'language', value: lang }).catch(
      logConfigSaveError('language'),
    )
  }

  // ── 窗口材质(毛玻璃,2026-08-24)─────────────────────────────────────────
  // 与 Rust 侧 window_material.rs 的 DWM 背板(mica→Win10 blur 退化 / acrylic / none 清除)
  // 配对:本 ref 是前端单源,applyWindowMaterial 是 html[data-glass] 属性唯一写点
  // (仅 isWindows 且值非 none 时设置,glass.css 消费),原生层由
  // watcher→apply_setting_effects 同步翻转,两侧同值同拍。
  const windowMaterial = ref<'mica' | 'acrylic' | 'none'>('mica')

  /** 单源写点:macOS/Linux 恒移除(原生背板不存在,页面根须保持不透明)。 */
  function applyWindowMaterial() {
    const el = document.documentElement
    if (isWindows && windowMaterial.value !== 'none') {
      el.setAttribute('data-glass', windowMaterial.value)
    } else {
      el.removeAttribute('data-glass')
    }
  }

  function setWindowMaterial(v: 'mica' | 'acrylic' | 'none') {
    windowMaterial.value = v
    applyWindowMaterial()
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'window_material', value: v }).catch(
      logConfigSaveError('window_material'),
    )
  }

  // 毛玻璃各层保留 Mica/Acrylic 的相对配方,这里只调整对应层的整体缩放。
  const glassChromeOpacity = ref(GLASS_OPACITY_DEFAULT)
  const glassStickyOpacity = ref(GLASS_OPACITY_DEFAULT)
  const glassSurfaceOpacity = ref(GLASS_OPACITY_DEFAULT)
  const glassControlOpacity = ref(GLASS_OPACITY_DEFAULT)
  const glassContentOpacity = ref(GLASS_OPACITY_DEFAULT)
  const glassGalleryOpacity = ref(GLASS_GALLERY_OPACITY_DEFAULT)

  function applyGlassOpacityConfig() {
    applyGlassOpacityScale('chrome', glassChromeOpacity.value)
    applyGlassOpacityScale('sticky', glassStickyOpacity.value)
    applyGlassOpacityScale('surface', glassSurfaceOpacity.value)
    applyGlassOpacityScale('control', glassControlOpacity.value)
    applyGlassOpacityScale('content', glassContentOpacity.value)
    applyGlassGalleryOpacity(glassGalleryOpacity.value)
  }

  function setGlassOpacityValue(
    key: string,
    target: { value: number },
    value: number,
  ) {
    target.value = clampGlassOpacity(value)
    applyGlassOpacityConfig()
    invokeIpc(IPC.SET_APP_CONFIG, { key, value: String(target.value) }).catch(
      logConfigSaveError(key),
    )
  }

  function setGlassChromeOpacity(value: number) {
    setGlassOpacityValue('glass_chrome_opacity', glassChromeOpacity, value)
  }

  function setGlassStickyOpacity(value: number) {
    setGlassOpacityValue('glass_sticky_opacity', glassStickyOpacity, value)
  }

  function setGlassSurfaceOpacity(value: number) {
    setGlassOpacityValue('glass_surface_opacity', glassSurfaceOpacity, value)
  }

  function setGlassControlOpacity(value: number) {
    setGlassOpacityValue('glass_control_opacity', glassControlOpacity, value)
  }

  function setGlassContentOpacity(value: number) {
    setGlassOpacityValue('glass_content_opacity', glassContentOpacity, value)
  }

  function setGlassGalleryOpacity(value: number) {
    glassGalleryOpacity.value = clampGlassGalleryOpacity(value)
    applyGlassOpacityConfig()
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'glass_gallery_opacity',
      value: String(glassGalleryOpacity.value),
    }).catch(logConfigSaveError('glass_gallery_opacity'))
  }

  // ── 主题色浓度(2026-09-06)────────────────────────────────────────────────
  // 底色 wash token 的 color-mix 缩放,消费见 src/themes/tint.ts;100=满浓度出厂锚点,
  // 默认 60(用户反馈出厂配色整体偏深,默认即调浅,想要原观感自行调回 100)。
  const themeTintStrength = ref(THEME_TINT_DEFAULT)

  function setThemeTintStrength(value: number) {
    themeTintStrength.value = clampThemeTint(value)
    applyThemeTintStrength(themeTintStrength.value)
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'theme_tint_strength',
      value: String(themeTintStrength.value),
    }).catch(logConfigSaveError('theme_tint_strength'))
    // 快照携带浓度,首帧(pre-Vue)即可按同一浓度着色,避免启动时底色闪一下满浓度。
    writeThemeSnapshot()
  }

  // ── 文字浓度(2026-09-06)──────────────────────────────────────────────────
  // 文字 ramp 的 color-mix 缩放,消费见 src/themes/strength.ts;100=满浓度出厂文字色(默认 100,清晰锐利)。
  const themeTextStrength = ref(THEME_TEXT_DEFAULT)

  function setThemeTextStrength(value: number) {
    themeTextStrength.value = clampThemeText(value)
    applyThemeTextStrength(themeTextStrength.value)
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'theme_text_strength',
      value: String(themeTextStrength.value),
    }).catch(logConfigSaveError('theme_text_strength'))
    writeThemeSnapshot()
  }

  // ── 详细首次使用引导手册 ────────────────────────────────────────────────
  // 与首启向导(OnboardingWizard/first_launch)独立:向导管「首启三步硬门控」,
  // 本手册管「随时可重开的分章图文说明」。
  const userGuideOpen = ref(false)
  const guideSeen = ref(false)

  /** 强开手册——忽略 guideSeen 标记,供设置页入口随时重开。 */
  function openUserGuide() {
    userGuideOpen.value = true
  }

  /** 关闭手册:首次关闭才写 guide_seen=true(避免每次重开关闭都重复 IPC)。 */
  function dismissUserGuide() {
    userGuideOpen.value = false
    if (!guideSeen.value) {
      guideSeen.value = true
      invokeIpc(IPC.SET_APP_CONFIG, { key: 'guide_seen', value: 'true' }).catch(
        logConfigSaveError('guide_seen'),
      )
    }
  }

  /** 启动配置批水合:guideSeen 与 R2-4 一样只读不写。 */
  function hydrateGuideSeen(val: string | null) {
    guideSeen.value = val === 'true'
  }

  // FOUC 快照:index.html 内联脚本在样式解析前读它给首帧着色;权威源仍是 app_config
  // (启动批校正),快照仅是首帧加速缓存,损坏/缺失时内联脚本回退 matchMedia。
  const THEME_SNAPSHOT_KEY = 'scrollery.themeSnapshot.v1'

  /** FOUC 快照唯一写点:主题三键 + 底色/文字浓度,首帧脚本(public/theme-snapshot.js)据此着色。 */
  function writeThemeSnapshot() {
    try {
      localStorage.setItem(
        THEME_SNAPSHOT_KEY,
        JSON.stringify({
          appearance: appearance.value,
          light: lightThemeId.value,
          dark: darkThemeId.value,
          tint: themeTintStrength.value,
          text: themeTextStrength.value,
        }),
      )
    } catch {
      // localStorage 不可用只损失首帧加速,静默降级
    }
  }

  function applyAppearance() {
    const kind = isDark.value ? 'dark' : 'light'
    document.documentElement.setAttribute('data-theme', resolvedThemeId.value)
    // 供确需按明暗分支的选择器使用([data-color-scheme='dark']),组件禁止再对
    // 具体主题 id 写选择器——那是主题文件的领地。
    document.documentElement.setAttribute('data-color-scheme', kind)
    writeThemeSnapshot()

    // 自绘标题栏后(顶栏重构 L1):Windows DWM 染色链已废弃——decorations:false 无原生 caption,
    // 标题栏底色/文字由 WindowChrome 的 CSS 变量直接绘制。此处仅保留「窗口明暗同步」:mac 保留
    // 原生红绿灯,其 hover 态与原生弹层吃 window theme,须按主题 kind 同步 setTheme,否则夜间主题
    // 下红绿灯区域可能出白底 hover(设计文档 §3.2)。system 模式传 null 跟随 OS(与旧 Rust 命令
    // theme='system'→None 同义),light/dark 显式设。
    // 仅 mac 需要(设计文档 §3.2 明确 Windows frameless 无原生 caption「无需」);失败留诊断信号。
    if (isMac) {
      const winTheme = appearance.value === 'system' ? null : kind
      getAppWindow()
        .setTheme(winTheme)
        .catch((err) => logger.warn('[uiStore] setTheme failed', { error: err }))
    }
  }

  function setAppearance(mode: AppearanceMode) {
    appearance.value = mode
    applyAppearance()
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'appearance', value: mode }).catch(
      logConfigSaveError('appearance'),
    )
  }

  /** 选择一个风格，同时更新并持久化它的亮暗主题配对。 */
  function setThemeStyle(style: ThemeStyle) {
    const lightId = themeIdForStyle(style, 'light')
    const darkId = themeIdForStyle(style, 'dark')
    lightThemeId.value = lightId
    darkThemeId.value = darkId
    applyAppearance()
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'theme_light', value: lightId }).catch(
      logConfigSaveError('theme_light'),
    )
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'theme_dark', value: darkId }).catch(
      logConfigSaveError('theme_dark'),
    )
  }

  // 三态循环(P2 修复:原实现只在 light/dark 二态打转,system 从侧栏不可达)。
  function cycleAppearance() {
    const order: AppearanceMode[] = ['light', 'dark', 'system']
    setAppearance(order[(order.indexOf(appearance.value) + 1) % order.length])
  }

  // 注：thumbStrategy / gpuEngine 此前在此双持（configStore 也持有并镜像至此），但 uiStore 这份
  // 只被写、从不被读——已删，单一来源归 configStore（S5/T19 去重）。

  // ── 侧边栏 ────────────────────────────────────────────────────────────
  const sidebarWidth = ref(260)
  // 侧栏显隐(用户裁决 2026-07-14):画廊与查看器各持独立开关且默认不同——画廊默认展开(主导航
  // 入口),查看器默认收起(内容优先)。一经用户点击即固化,仅手动切换;不随翻页/切图复位——此前
  // AppShell 用 route.path watch 复位,导致「切下一张图侧栏自动收起」,本次根治。AppShell 是根壳
  // 永不卸载,故会话级 ref 天然跨导航存活;重启回默认(无需后端持久化)。
  const gallerySidebarVisible = ref(true)
  const viewerSidebarVisible = ref(false)
  // 查看器返回画廊时，侧栏 margin-left 的过渡会使主区宽度经历中间值。该运行时信号只给
  // AppShell 与 MediaGrid 协作冻结几何采样；代次保证上一轮 transitionend 不会提前解锁新一轮。
  const routeReturnSidebarTransitioning = ref(false)
  const routeReturnSidebarTransitionGeneration = ref(0)

  function beginRouteReturnSidebarTransition(): number {
    routeReturnSidebarTransitionGeneration.value += 1
    routeReturnSidebarTransitioning.value = true
    return routeReturnSidebarTransitionGeneration.value
  }

  function finishRouteReturnSidebarTransition(generation: number): boolean {
    if (
      !routeReturnSidebarTransitioning.value ||
      routeReturnSidebarTransitionGeneration.value !== generation
    ) {
      return false
    }
    routeReturnSidebarTransitioning.value = false
    return true
  }

  function toggleGallerySidebar() {
    gallerySidebarVisible.value = !gallerySidebarVisible.value
  }
  function toggleViewerSidebar() {
    viewerSidebarVisible.value = !viewerSidebarVisible.value
  }

  // 大图查看器「文件信息」面板显隐(用户裁决 2026-07-14):从 useMediaDetail 上收到此处,与查看器
  // 侧栏同为会话级——跨翻页、跨「退出再进查看器」保留;只手动开合(控制条 ⓘ / 面板 ✕),不再点图
  // 自动收起。面板从覆盖层改为停靠挤压式(ContentViewer 用 padding-right 动画让出图片区)。
  const viewerInfoVisible = ref(false)
  function toggleViewerInfo() {
    viewerInfoVisible.value = !viewerInfoVisible.value
  }

  function setSidebarWidth(w: number) {
    sidebarWidth.value = Math.max(180, Math.min(400, w))
    document.documentElement.style.setProperty('--sidebar-width', `${sidebarWidth.value}px`)
  }

  function persistSidebarWidth() {
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'sidebar_width',
      value: String(sidebarWidth.value),
    }).catch(logConfigSaveError('sidebar_width'))
  }

  // ── Active view:已拆出至 stores/viewStore.ts(P1-21 渐进拆分第二刀)。消费方改用 useViewStore()。 ──

  // ── 排序 ───────────────────────────────────────────────────────────────
  const sortOrder = ref<'asc' | 'desc'>('desc')

  // ── 网格显示设置 ────────────────────────────────────────────────
  const gridRowHeight = ref(200)

  function setGridRowHeight(h: number) {
    gridRowHeight.value = h
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'grid_row_height', value: String(h) }).catch(
      logConfigSaveError('grid_row_height'),
    )
  }

  // ── 分组和排序设置 ──────────────────────────────────────────────
  const groupBy = ref<'date' | 'folder' | 'none'>('date')
  const sortWithinGroup = ref<'datetime' | 'filename' | 'similarity'>('datetime')

  // persist 参数(2026-07-06 审查 P1-19):语义搜索等**临时态**切换分组/排序时须传 persist=false,
  // 否则会把临时的 'none'/'similarity' 写进持久化配置——用户重启后原分组偏好被永久覆盖。
  // 用户经设置/工具栏的显式切换走默认 persist=true。
  function setGroupBy(mode: 'date' | 'folder' | 'none', persist = true) {
    groupBy.value = mode
    if (persist) {
      invokeIpc(IPC.SET_APP_CONFIG, { key: 'group_by', value: mode }).catch(
        logConfigSaveError('group_by'),
      )
    }
  }

  function setSortWithinGroup(sort: 'datetime' | 'filename' | 'similarity', persist = true) {
    sortWithinGroup.value = sort
    if (persist) {
      // 埋点仅在真实用户选择(persist===true)时计数,异步不阻塞(见 recordSortModeUsage 注释)。
      recordSortModeUsage(sort)
      invokeIpc(IPC.SET_APP_CONFIG, { key: 'sort_within_group', value: sort }).catch(
        logConfigSaveError('sort_within_group'),
      )
    }
  }

  // ── 文件树显示范围（S 线 §3 / D-009）──────────────────────────────────────
  //
  // 三态互斥、按本机持久化。**没有第四态**「已注册格式 + 隐藏项」：扫描器 walker.rs 整棵剪掉
  // 隐藏目录，只含隐藏/未知文件的目录连 directories 行都不存在 —— 那一态在当前扫描契约下
  // 兑现不出来，不是没做而是做不出（§3）。
  //
  // 放 uiStore（而非 configStore）：这是显示偏好，与 groupBy/layoutMode 同类；更实在的理由是
  // 它必须在**首次展开目录前**就位，而 uiStore 的启动批是单次往返 + 有 startupConfigPromise
  // 这个现成的水合门。configStore.loadConfig 是设置页的惰性 N 次往返，来不及。
  const treeDisplayMode = ref<TreeDisplayMode>('registeredOnly')

  function setTreeDisplayMode(mode: TreeDisplayMode) {
    treeDisplayMode.value = mode
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'tree_display_mode', value: mode }).catch(
      logConfigSaveError('tree_display_mode'),
    )
  }

  // ── Layout mode（T20）：'justified' 等高行（默认）/ 'grid' 均匀宫格 ───────────────
  // 后端按此切换排版算法（compute_layout 的 layoutMode 参数）；前端据此切单元方图裁切。
  const layoutMode = ref<'justified' | 'grid'>('justified')

  function setLayoutMode(mode: 'justified' | 'grid') {
    layoutMode.value = mode
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'layout_mode', value: mode }).catch(
      logConfigSaveError('layout_mode'),
    )
  }

  // ── 无缝分组（#1,2026-07-17）───────────────────────────────────────────────
  // 开 = 排序仍按 groupBy 聚合(folder DFS / date 日桶),但打包无分隔符、行跨组连续——
  // 视觉如不分组、数据保留组序。groupBy='none' 时无效果(本就全局序)。
  // 无分隔符行 → 时间轴 scrubber 失据自动隐藏，右轴槽位由 minimap 自动接管。
  const seamlessGroups = ref<boolean>(false)

  function setSeamlessGroups(val: boolean) {
    seamlessGroups.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'seamless_groups', value: String(val) }).catch(
      logConfigSaveError('seamless_groups'),
    )
  }

  // ── 轴开合(两模式通用)────────────────────────────────────────────────────
  // 所有非空画廊都可挂时间轴 scrubber 或 VSCode 式微缩预览(MinimapAxis)；两形态
  // 间由轴形态钮切换。本开关只管轴整体显隐，由 chevron 收合钮驱动并持久化。
  const axisVisible = ref<boolean>(true)

  function setAxisVisible(val: boolean) {
    axisVisible.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'seamless_minimap', value: String(val) }).catch(
      logConfigSaveError('seamless_minimap'),
    )
  }

  // ── 轴形态偏好(2026-07-24)────────────────────────────────────────────────
  // timeline|minimap,两分组模式通用;持久化替代原会话态 preferredAxis(重载不粘的根因)。
  const axisMode = ref<'timeline' | 'minimap'>('timeline')

  function setAxisMode(val: 'timeline' | 'minimap') {
    axisMode.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'axis_mode', value: val }).catch(
      logConfigSaveError('axis_mode'),
    )
  }

  // 启动批返回前先守在 colors,避免已选择“只渲染色块”的用户重启时抢跑最多 8 个图片请求;
  // 配置缺省(旧版本升级/新装)在水合分支恢复既有默认 thumbnails。
  const minimapRenderMode = ref<MinimapRenderMode>('colors')

  function setMinimapRenderMode(val: MinimapRenderMode) {
    minimapRenderMode.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'minimap_render_mode', value: val }).catch(
      logConfigSaveError('minimap_render_mode'),
    )
  }

  // ── Bucket 分段虚拟滚动(T16 方案B;B0-B3.2 真机验收后转默认引擎)──────────────
  // 开(默认)= 画廊用 bucket 分段引擎(零坐标平移,useBucketVirtualScroll + 自研逻辑
  // 滚动条);关 = 回退方案 A 线性平移。运行时即切即生效(MediaGrid 双引擎互斥)。
  const bucketSegmentedScroll = ref<boolean>(true)

  function setBucketSegmentedScroll(val: boolean) {
    bucketSegmentedScroll.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'bucket_segmented_scroll', value: String(val) }).catch(
      logConfigSaveError('bucket_segmented_scroll'),
    )
  }

  // ── Toasts:已拆出至 stores/toastStore.ts(P1-21 渐进拆分)。消费方改用 useToastStore()。 ──

  // ── 搜索 ─────────────────────────────────────────────────────────────
  const searchQuery = ref('')
  const searchScope = ref<string>('filename')
  // isSearching 幽灵态已删(2026-07-10 审查 B14):全库从无写入者,恒 false 使搜索空态
  // 分支死路(无结果时误显「空库,添加文件夹」)。「搜索中」判据统一用 searchQuery !== ''。

  // ── 滚动目标 ────────────────────────────────────────────────────────
  // 要滚动到的目录 id（点击侧边栏文件夹时设置）。用唯一 id 而非名字，使同名文件夹也能跳到正确位置。
  const pendingScrollDirId = ref<number | null>(null)
  const scrolledDirectoryId = ref<number | null>(null)

  // ── 媒体拖到文件夹 ────────────────────────────────────────────────────────
  // 拖动画廊媒体到文件夹树时当前悬停的目录 id。MediaGrid 拖拽时设置；FoldersSection 读取以
  // 高亮放置目标（拖拽始于画廊、目标在侧栏 —— 问题5）。
  const mediaDragHoverDirId = ref<number | null>(null)

  // ── 全屏 ─────────────────────────────────────────────────────────
  // 已于 2026-07-16 迁出至 composables/useWindowMode(窗口三态 normal/maximized/fullscreen 的唯一所有者)。
  // 迁出理由不是「拆 store」,而是这里的实现**结构上无法正确**:它持有的 isFullscreen 是个只写 ref
  // (乐观赋值、无 OS 回同步、出错也不纠正),且完全不知道最大化态的存在——而最大化与全屏互斥。
  // 真机 round11 #8 的四个症状(最大化+F11 黑边 / 全屏仍可拖走窗口 / 拖出全屏后贴顶失效 / 全屏仍留标题栏)
  // 全部是「三态无主」的直接推论。消费方改 import { isFullscreen, toggleFullscreen } from useWindowMode。

  // ── Close Behavior ───────────────────────────────────────────────────────
  const closeBehavior = ref<'ask' | 'minimize_to_tray' | 'exit'>('ask')
  const showCloseConfirmDialog = ref(false)

  function setCloseBehavior(behavior: 'ask' | 'minimize_to_tray' | 'exit') {
    closeBehavior.value = behavior
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'close_behavior', value: behavior }).catch(
      logConfigSaveError('close_behavior'),
    )
  }

  // 关窗请求单源分派(#12):标题栏 ✕(后端 window-close-requested → App.vue 监听)与
  // mod+W(AppShell 全局键)共用——按 closeBehavior 走 托盘隐藏/直接退出/弹确认框。
  async function requestAppClose() {
    if (closeBehavior.value === 'minimize_to_tray') {
      await invokeIpc(IPC.HIDE_WINDOW)
    } else if (closeBehavior.value === 'exit') {
      await invokeIpc(IPC.EXIT_APP)
    } else {
      showCloseConfirmDialog.value = true
    }
  }

  // ── Pinned Settings ──────────────────────────────────────────────────────
  const pinnedSettings = ref<string[]>([])

  // The "全量 AI 分析" tool is a permanent pinned entry (not a Settings-page item),
  // rendered specially. We keep it inside `pinnedSettings` so it can be drag-sorted
  // together with the other tools.
  // 「全量 AI 分析」是常驻置顶项（非设置页条目），特殊渲染。将其纳入 `pinnedSettings`
  // 以便与其他工具一起拖拽排序。
  const AI_FULL_ANALYSIS_KEY = 'aiFullAnalysis'
  // 「全量人脸识别」同为常驻置顶项（F5），与 AI 分析并列、可一起拖拽排序。
  const FACE_FULL_ANALYSIS_KEY = 'faceFullAnalysis'

  function persistPinned() {
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'pinned_settings',
      value: JSON.stringify(pinnedSettings.value),
    }).catch(logConfigSaveError('pinned_settings'))
  }

  function togglePinnedSetting(key: string) {
    const idx = pinnedSettings.value.indexOf(key)
    if (idx >= 0) {
      pinnedSettings.value.splice(idx, 1)
    } else {
      pinnedSettings.value.push(key)
    }
    persistPinned()
  }

  // 将置顶工具从一个位置移动到另一个位置（拖拽排序）并持久化。
  function reorderPinnedSetting(fromIndex: number, toIndex: number) {
    const arr = pinnedSettings.value
    if (
      fromIndex < 0 ||
      fromIndex >= arr.length ||
      toIndex < 0 ||
      toIndex >= arr.length ||
      fromIndex === toIndex
    )
      return
    const [moved] = arr.splice(fromIndex, 1)
    arr.splice(toIndex, 0, moved)
    persistPinned()
  }

  // ── Thumbnail Info Overlays ──────────────────────────────────────────────
  // 默认开(S7,用户 2026-07-15 裁决):此前默认关,而元素勾选面板由 `v-if="ui.showThumbInfo"`
  // 门控 → 整套逐元素配置对没主动开过总开关的用户不可见,功能事实上被藏起来。默认开只让
  // 面板可发现;thumbInfoElements 仍默认空,故缩略图观感不变(无元素勾选=无徽章无信息行)。
  // 显式存过 'false' 的用户不受影响(配置读回覆盖默认)。
  const showThumbInfo = ref<boolean>(true)
  const thumbInfoElements = ref<string[]>([])

  function setShowThumbInfo(val: boolean) {
    showThumbInfo.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'show_thumb_info', value: String(val) }).catch(
      logConfigSaveError('show_thumb_info'),
    )
  }

  function setThumbInfoElements(elements: string[]) {
    thumbInfoElements.value = elements
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'thumb_info_elements',
      value: JSON.stringify(elements),
    }).catch(logConfigSaveError('thumb_info_elements'))
  }

  // ── 悬停自动播放（需求1） ──────────────────────────────────────────────────
  // 鼠标移入视频/动态照片格子 → 自动静音循环预览。默认开启，持久化到 app_config。
  const hoverAutoplay = ref<boolean>(true)

  function setHoverAutoplay(val: boolean) {
    hoverAutoplay.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'hover_autoplay', value: String(val) }).catch(
      logConfigSaveError('hover_autoplay'),
    )
  }

  // ── 配置重构批次C:5 个前端阈值键(advanced,只读——无设置页 UI,无 setter) ──────────
  // 悬停预览「重」视频判据(与既有 useHoverPreview.ts::HEAVY_VIDEO_PIXELS/HEAVY_VIDEO_BYTES
  // 同默认值,批次C起改由此处响应式下发,消费点不再自持模块级常量)。
  const heavyVideoMaxPixels = ref<number>(3840 * 2160)
  const heavyVideoMaxBytes = ref<number>(10 * 1024 * 1024 * 1024)
  // 悬停延迟(与既有 useHoverPreview.ts::HOVER_DELAY_MS 同默认值)。
  const hoverDelayMs = ref<number>(200)
  // 搜索框混合/语义模式提交防抖(默认值单源 constants/defaults.ts::SEARCH_DEBOUNCE_MS)。
  const searchDebounceMs = ref<number>(DEFAULTS.SEARCH_DEBOUNCE_MS)
  // 布局重算防抖(默认值单源 constants/defaults.ts::RESIZE_DEBOUNCE_MS)。
  const resizeDebounceMs = ref<number>(DEFAULTS.RESIZE_DEBOUNCE_MS)
  // 小批 C2:雪碧图切帧列数(与既有 useHoverPreview.ts::KEYFRAME_COUNT 同默认值,消费点改
  // 响应式读取此 ref、不再自持模块级常量)。
  const videoKeyframeCount = ref<number>(10)

  // ── 选择态拖拽手柄显隐(2026-07-17 #5) ────────────────────────────────────
  // 选中缩略图左上角的拖拽手柄(拖入文件夹用)可选隐藏。默认开;关闭后 DOM 不渲染、
  // Canvas 不绘制**且命中判定同步跳过**(只藏不停用会出现「看不见却能拖」的幽灵手柄)。
  const showDragHandle = ref<boolean>(true)

  function setShowDragHandle(val: boolean) {
    showDragHandle.value = val
    invokeIpc(IPC.SET_APP_CONFIG, { key: 'show_drag_handle', value: String(val) }).catch(
      logConfigSaveError('show_drag_handle'),
    )
  }

  // ── 窗口化沉浸模式(2026-07-23) ──────────────────────────────────────────
  // 非全屏(且非查看器沉浸)时,用户可选让顶栏/底栏自动隐藏、鼠标移到窗口边缘再唤出
  // (useChromeReveal.chromeAutoHidden 的第三来源)。默认关:这是可选的沉浸偏好,不该
  // 悄悄改变新用户的默认交互。
  const autoHideChromeWindowed = ref<boolean>(false)

  function setAutoHideChromeWindowed(val: boolean) {
    // 92d8396 钉定的次序:先 await 持久化、后改本地 state,消 IPC 竞速(与旧式 showDragHandle
    // “先改 state 再落盘”不同——那是本设置项之前就存在的旧代码,不因本次新增而回填修正)。
    invokeIpc(IPC.SET_APP_CONFIG, {
      key: 'auto_hide_chrome_windowed',
      value: String(val),
    })
      .then(() => {
        autoHideChromeWindowed.value = val
      })
      .catch(logConfigSaveError('auto_hide_chrome_windowed'))
  }

  // ── 启动配置批量读(R2-4) ────────────────────────────────────────────────
  // 原 9 处模块初始化各发一次 get_app_config(N+1);现并入 get_startup_config 单次往返。
  // promise 共享给 App.vue(其全局项 language/字号/滚动条宽 + first_launch 同批),
  // 整个启动阶段的配置 IPC 由 11 次归 1 次。各键的解析与守卫逻辑原样保留。
  /**
   * 启动配置批的水合逻辑(R2-4 批量读取的应用侧)。抽成具名函数供 refreshFromBackend 复用——
   * 外置配置文件（config.toml，批次B）热更新到达时(composables/useConfigFile.ts)重新走同一条
   * 取值路径，不新写并行取数逻辑。**全程只 `.value =` 直接赋值，绝不调用任何 setXxx(...)**——
   * 那些 setter 会连带 invokeIpc(SET_APP_CONFIG) 写回，刷新路径必须绕过写回，否则会把「外部
   * 编辑器刚改的值」用内存里的旧值立即覆盖回写，形成假循环。
   */
  function hydrateFromStartupConfig(cfg: StartupConfig) {
    // 解析防线(2026-07-10 审查 B14):持久化值是外部输入——枚举键一律白名单(对齐下方
    // layoutMode/closeBehavior 的既有姿态,原 as-cast 会把损坏值直灌类型化 ref);JSON 键
    // 除 parse 异常外还须校验形状,否则合法 JSON 非数组('{}')在后续 .includes/.push 抛
    // TypeError,**连锁中断本 then 尾部的主题水合**(applyAppearance 整段跳过 → FOUC 兜底裸奔)。
    const isStringArray = (v: unknown): v is string[] =>
      Array.isArray(v) && v.every((x) => typeof x === 'string')
    if (cfg.gridRowHeight) gridRowHeight.value = parseInt(cfg.gridRowHeight, 10) || 200
    if (cfg.groupBy === 'date' || cfg.groupBy === 'folder' || cfg.groupBy === 'none')
      groupBy.value = cfg.groupBy
    if (
      cfg.sortWithinGroup === 'datetime' ||
      cfg.sortWithinGroup === 'filename' ||
      cfg.sortWithinGroup === 'similarity'
    )
      sortWithinGroup.value = cfg.sortWithinGroup
    if (cfg.layoutMode === 'grid' || cfg.layoutMode === 'justified')
      layoutMode.value = cfg.layoutMode
    // 白名单同上:损坏/陌生值一律留默认 registeredOnly ——「回落到只看已注册格式」是安全的那一侧,
    // 而灌进一个非法模式会让 list_tree_entries 直接报 invalid_mode,树整棵展不开。
    if (
      cfg.treeDisplayMode === 'registeredOnly' ||
      cfg.treeDisplayMode === 'allFiles' ||
      cfg.treeDisplayMode === 'allFilesWithHidden'
    )
      treeDisplayMode.value = cfg.treeDisplayMode
    if (cfg.closeBehavior && ['ask', 'minimize_to_tray', 'exit'].includes(cfg.closeBehavior)) {
      closeBehavior.value = cfg.closeBehavior as typeof closeBehavior.value
    }
    if (cfg.pinnedSettings) {
      try {
        const parsed: unknown = JSON.parse(cfg.pinnedSettings)
        if (isStringArray(parsed)) pinnedSettings.value = parsed
      } catch {}
    }
    // Back-compat:AI/人脸全量分析常驻项确保存在(老用户的持久化列表里没有此二键)。
    if (!pinnedSettings.value.includes(AI_FULL_ANALYSIS_KEY)) {
      pinnedSettings.value.push(AI_FULL_ANALYSIS_KEY)
    }
    if (!pinnedSettings.value.includes(FACE_FULL_ANALYSIS_KEY)) {
      pinnedSettings.value.push(FACE_FULL_ANALYSIS_KEY)
    }
    if (cfg.showThumbInfo) showThumbInfo.value = cfg.showThumbInfo === 'true'
    if (cfg.thumbInfoElements) {
      try {
        const parsed: unknown = JSON.parse(cfg.thumbInfoElements)
        if (isStringArray(parsed)) thumbInfoElements.value = parsed
      } catch {}
    }
    // 日志能力重构 S3(2026-07-20 reviewer 深审修复):off 档联动此前只在 configStore.loadConfig
    // (设置页/模型库懒路由才调)里同步——典型会话从未触发,off 档形同虚设。并入本批,
    // 启动早期(App.vue setup 即触发 useUiStore())就位;configStore.loadConfig 内的同名调用
    // 仍保留(设置页切换时的即时同步事实源,二者写的是同一个模块级 `enabled` 标志,不冲突)。
    setLoggerEnabled((cfg.logLevel ?? 'info') !== 'off')
    if (cfg.hoverAutoplay != null) hoverAutoplay.value = cfg.hoverAutoplay !== 'false'
    if (cfg.showDragHandle != null) showDragHandle.value = cfg.showDragHandle !== 'false'
    // 默认关(与 showDragHandle 的「默认开」方向相反):仅显式存过 'true' 才开启窗口化沉浸。
    if (cfg.autoHideChromeWindowed != null)
      autoHideChromeWindowed.value = cfg.autoHideChromeWindowed === 'true'
    // 默认关:仅显式存过 'true' 才开(与 showDragHandle 的「默认开」相反方向)。
    if (cfg.seamlessGroups != null) seamlessGroups.value = cfg.seamlessGroups === 'true'
    // 默认开(保轴可发现性):仅显式 'false'(chevron 收起过)才隐藏。
    if (cfg.seamlessMinimap != null) axisVisible.value = cfg.seamlessMinimap !== 'false'
    // 枚举守卫:仅认可的两值才赋,损坏/陌生值保默认 'timeline'。
    if (cfg.axisMode === 'timeline' || cfg.axisMode === 'minimap') axisMode.value = cfg.axisMode
    // 枚举配置按外部输入处理:损坏/陌生值回退 thumbnails,兼容升级前的既有行为。
    if (cfg.minimapRenderMode === 'colors' || cfg.minimapRenderMode === 'thumbnails')
      minimapRenderMode.value = cfg.minimapRenderMode
    else minimapRenderMode.value = 'thumbnails'
    // 默认开(T16 转正):仅显式 'false' 才回退方案 A——历史上显式开过的 'true'
    // 与未配置的新装置都落在 bucket 引擎。
    if (cfg.bucketSegmentedScroll != null)
      bucketSegmentedScroll.value = cfg.bucketSegmentedScroll !== 'false'

    // 配置重构批次C:5 个前端阈值键——解析失败/缺位保留 ref 已有默认值(不覆盖成 NaN/0)。
    const parseUint = (v: string | null): number | null => {
      if (v == null) return null
      const n = parseInt(v, 10)
      return Number.isFinite(n) && n >= 0 ? n : null
    }
    const heavyPixels = parseUint(cfg.heavyVideoMaxPixels)
    if (heavyPixels != null) heavyVideoMaxPixels.value = heavyPixels
    const heavyBytes = parseUint(cfg.heavyVideoMaxBytes)
    if (heavyBytes != null) heavyVideoMaxBytes.value = heavyBytes
    const hoverDelay = parseUint(cfg.hoverDelayMs)
    if (hoverDelay != null) hoverDelayMs.value = hoverDelay
    const searchDebounce = parseUint(cfg.searchDebounceMs)
    if (searchDebounce != null) searchDebounceMs.value = searchDebounce
    const resizeDebounce = parseUint(cfg.resizeDebounceMs)
    if (resizeDebounce != null) resizeDebounceMs.value = resizeDebounce
    const keyframeCount = parseUint(cfg.videoKeyframeCount)
    if (keyframeCount != null) videoKeyframeCount.value = keyframeCount

    const glassChromeOpacityValue = parseUint(cfg.glassChromeOpacity)
    if (glassChromeOpacityValue != null)
      glassChromeOpacity.value = clampGlassOpacity(glassChromeOpacityValue)
    const glassStickyOpacityValue = parseUint(cfg.glassStickyOpacity)
    if (glassStickyOpacityValue != null)
      glassStickyOpacity.value = clampGlassOpacity(glassStickyOpacityValue)
    const glassSurfaceOpacityValue = parseUint(cfg.glassSurfaceOpacity)
    if (glassSurfaceOpacityValue != null)
      glassSurfaceOpacity.value = clampGlassOpacity(glassSurfaceOpacityValue)
    const glassControlOpacityValue = parseUint(cfg.glassControlOpacity)
    if (glassControlOpacityValue != null)
      glassControlOpacity.value = clampGlassOpacity(glassControlOpacityValue)
    const glassContentOpacityValue = parseUint(cfg.glassContentOpacity)
    if (glassContentOpacityValue != null)
      glassContentOpacity.value = clampGlassOpacity(glassContentOpacityValue)
    const glassGalleryOpacityValue = parseUint(cfg.glassGalleryOpacity)
    if (glassGalleryOpacityValue != null)
      glassGalleryOpacity.value = clampGlassGalleryOpacity(glassGalleryOpacityValue)
    applyGlassOpacityConfig()
    // 主题色浓度:损坏/缺位保默认 60;须在 applyAppearance(写快照)之前就位,快照才带得上新值。
    const themeTintStrengthValue = parseUint(cfg.themeTintStrength)
    if (themeTintStrengthValue != null)
      themeTintStrength.value = clampThemeTint(themeTintStrengthValue)
    applyThemeTintStrength(themeTintStrength.value)
    // 文字浓度:同上,与主题色浓度同批就位。
    const themeTextStrengthValue = parseUint(cfg.themeTextStrength)
    if (themeTextStrengthValue != null)
      themeTextStrength.value = clampThemeText(themeTextStrengthValue)
    applyThemeTextStrength(themeTextStrength.value)

    // 多主题初始化:新键 appearance 优先,缺位读 legacy 'theme' 迁移；
    // 两者皆无 → 保持默认 'system'。
    const isMode = (v: string | null): v is AppearanceMode =>
      v === 'light' || v === 'dark' || v === 'system'
    if (isMode(cfg.appearance)) appearance.value = cfg.appearance
    else if (isMode(cfg.theme)) appearance.value = cfg.theme
    // 槽位主题 id:注册表归一化(旧 id 映射新风格;未注册/kind 不符落回默认)。
    const hasStoredTheme = cfg.themeLight != null || cfg.themeDark != null
    lightThemeId.value = normalizeThemeId(cfg.themeLight, 'light')
    darkThemeId.value = normalizeThemeId(cfg.themeDark, 'dark')
    // 两个旧槽位可能来自不同风格。以当前实际生效的槽位为准配对另一侧，
    // 让启动后随系统切换时仍保持同一套风格。
    const activeId = isDark.value ? darkThemeId.value : lightThemeId.value
    const activeStyle = getTheme(activeId)?.style ?? DEFAULT_THEME_STYLE
    lightThemeId.value = themeIdForStyle(activeStyle, 'light')
    darkThemeId.value = themeIdForStyle(activeStyle, 'dark')
    // 首次权威应用(校正 index.html 内联脚本按快照画的首帧,并同步原生标题栏)。
    applyAppearance()
    // 只要旧配置曾存过任一槽位，就把归一化后的风格配对写回两个持久键；
    // 新安装两槽位都缺位时保持默认值，不制造无意义的配置文件写入。
    if (hasStoredTheme) {
      if (cfg.themeLight !== lightThemeId.value) {
        invokeIpc(IPC.SET_APP_CONFIG, {
          key: 'theme_light',
          value: lightThemeId.value,
        }).catch(logConfigSaveError('theme_light'))
      }
      if (cfg.themeDark !== darkThemeId.value) {
        invokeIpc(IPC.SET_APP_CONFIG, {
          key: 'theme_dark',
          value: darkThemeId.value,
        }).catch(logConfigSaveError('theme_dark'))
      }
    }
    // 窗口材质白名单守卫:持久化值是外部输入——仅认可三值(与后端 schema 枚举一致),
    // 损坏/陌生值保默认 'mica'(对齐上方 axisMode 的既有姿态);随后应用 CSS 玻璃层。
    // 原生侧由 watcher→apply_setting_effects 同步翻转,启动与 config-file-changed
    // 热更新(refreshFromBackend 复用此函数)一次覆盖,两侧同值同拍。
    if (
      cfg.windowMaterial === 'mica' ||
      cfg.windowMaterial === 'acrylic' ||
      cfg.windowMaterial === 'none'
    )
      windowMaterial.value = cfg.windowMaterial
    applyWindowMaterial()
  }

  const startupConfigPromise = invokeIpc<StartupConfig>(IPC.GET_STARTUP_CONFIG)
  startupConfigPromise
    .then(hydrateFromStartupConfig)
    .catch((e) => logger.error('uiStore startup config hydration failed', { error: e }))

  /**
   * 外置配置文件（config.toml，批次B）热更新到达时刷新（composables/useConfigFile.ts 消费）：
   * 重新拉一次 GET_STARTUP_CONFIG，复用 hydrateFromStartupConfig 同一条水合逻辑（只 get 不 set，
   * 见其注释）。与 configStore.refreshFromBackend 对称，二者由 useConfigFile 的
   * config-file-changed 监听并发调用。
   */
  async function refreshFromBackend() {
    try {
      const cfg = await invokeIpc<StartupConfig>(IPC.GET_STARTUP_CONFIG)
      hydrateFromStartupConfig(cfg)
    } catch (e) {
      logger.error('uiStore refreshFromBackend failed', { error: e })
    }
  }

  return {
    // 外观、主题与语言
    appearance,
    lightThemeId,
    darkThemeId,
    resolvedThemeId,
    themeStyle,
    isDark,
    setAppearance,
    setThemeStyle,
    cycleAppearance,
    applyAppearance,
    language,
    applyLanguage,
    setLanguage,
    // 窗口材质(毛玻璃)
    windowMaterial,
    setWindowMaterial,
    glassChromeOpacity,
    glassStickyOpacity,
    glassSurfaceOpacity,
    glassControlOpacity,
    glassContentOpacity,
    glassGalleryOpacity,
    setGlassChromeOpacity,
    setGlassStickyOpacity,
    setGlassSurfaceOpacity,
    setGlassControlOpacity,
    setGlassContentOpacity,
    setGlassGalleryOpacity,
    themeTintStrength,
    setThemeTintStrength,
    themeTextStrength,
    setThemeTextStrength,
    // 详细首次使用引导手册
    userGuideOpen,
    guideSeen,
    openUserGuide,
    dismissUserGuide,
    hydrateGuideSeen,
    // R2-4:共享给 App.vue 的启动配置批(单次 IPC)。
    startupConfigPromise,
    // 外置配置文件（config.toml，批次B）热更新到达时刷新（composables/useConfigFile.ts 消费）。
    refreshFromBackend,
    // 侧边栏
    sidebarWidth,
    gallerySidebarVisible,
    viewerSidebarVisible,
    routeReturnSidebarTransitioning,
    routeReturnSidebarTransitionGeneration,
    viewerInfoVisible,
    toggleGallerySidebar,
    toggleViewerSidebar,
    beginRouteReturnSidebarTransition,
    finishRouteReturnSidebarTransition,
    toggleViewerInfo,
    setSidebarWidth,
    persistSidebarWidth,
    // 网格显示
    gridRowHeight,
    setGridRowHeight,
    // 分组和排序
    groupBy,
    setGroupBy,
    treeDisplayMode,
    setTreeDisplayMode,
    sortWithinGroup,
    setSortWithinGroup,
    // layout mode（T20）
    layoutMode,
    setLayoutMode,
    seamlessGroups,
    setSeamlessGroups,
    axisVisible,
    setAxisVisible,
    axisMode,
    setAxisMode,
    minimapRenderMode,
    setMinimapRenderMode,
    // 排序
    sortOrder,
    // 搜索
    searchQuery,
    searchScope,
    // 滚动目标
    pendingScrollDirId,
    scrolledDirectoryId,
    // 媒体拖到文件夹
    mediaDragHoverDirId,
    // 全屏已迁出 → composables/useWindowMode(窗口三态唯一所有者,见上方 Fullscreen 段注)
    // 关闭行为
    closeBehavior,
    showCloseConfirmDialog,
    requestAppClose,
    setCloseBehavior,
    // 置顶设置
    pinnedSettings,
    togglePinnedSetting,
    reorderPinnedSetting,
    // 缩略图信息
    showThumbInfo,
    setShowThumbInfo,
    thumbInfoElements,
    setThumbInfoElements,
    // 悬停自动播放
    hoverAutoplay,
    setHoverAutoplay,
    showDragHandle,
    setShowDragHandle,
    autoHideChromeWindowed,
    setAutoHideChromeWindowed,
    // bucket 分段虚拟滚动(T16 方案B)
    bucketSegmentedScroll,
    setBucketSegmentedScroll,
    // 配置重构批次C:5 个前端阈值键(只读,无 setter——见其声明处注释)
    heavyVideoMaxPixels,
    heavyVideoMaxBytes,
    hoverDelayMs,
    searchDebounceMs,
    resizeDebounceMs,
    videoKeyframeCount,
  }
})
