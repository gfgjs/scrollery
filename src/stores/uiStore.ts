// src/stores/uiStore.ts
// 全局 UI 状态 — 在有说明的地方持久化到 app_config。

import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import type { TreeDisplayMode } from '../types/media'
import { IPC } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'
import { logger, setLoggerEnabled } from '../utils/logger'
import i18n from '../i18n'
import { useToastStore } from './toastStore'
import {
  initializeSettings,
  readSetting,
  refreshSettingsFromBackend,
  settingsGeneration,
  settingsValues,
  writeSettings,
} from './settingsPersistence'
import {
  parseSettingJson,
  readSettingBool,
  readSettingEnum,
  readSettingNumber,
} from '../composables/settingsValues'
import type { StartupPayload } from '../types/config'
import { DEFAULTS } from '../constants/defaults'

export type MinimapRenderMode = 'colors' | 'thumbnails'

/**
 * 「用户主动选择的画廊组内排序模式」使用计数(仅开发构建、仅内存,不落盘、不进配置文件)。
 * 服务「先测 filename 使用频率」决策门(见 docs/planning/2026-07-14-统一文件树与画廊目录排序/
 * 方案B §15)。**仅统计真实用户动作**:唯一来源是排序下拉的 change → `setSortWithinGroup` 的
 * `persist===true` 路径;AI 切 similarity、URL/pref 恢复均走 `persist=false`,不计(否则每次启动/
 * 导航都虚增,污染频率)。开发期在 devtools 里读 `__scrollerySortModeUsage` 查看分布。
 */
const sortModeUsage: Record<string, number> = {}

function recordSortModeUsage(mode: 'datetime' | 'filename' | 'similarity') {
  if (!import.meta.env.DEV) return
  sortModeUsage[mode] = (sortModeUsage[mode] ?? 0) + 1
  // 暴露到全局便于开发期查看;生产构建不执行本分支。
  ;(globalThis as { __scrollerySortModeUsage?: Record<string, number> }).__scrollerySortModeUsage =
    sortModeUsage
}

/** 内部状态键(留 DB)的写入口:设置类键一律走中央服务,此处只剩 `guide_seen` 这类业务标记。 */
function logStateSaveError(key: string) {
  return (e: unknown) => logger.error(`setAppConfig failed: ${key}`, { key, error: e })
}

// ── 中央设置键名(照抄后端 schema,不做转写)──────────────────────────────────
// 常量而非散落字面量:键名是本模块与 config.toml 的唯一联系纽带,拼错等于让偏好静默失联。
const KEY_LANGUAGE = 'language'
const KEY_SIDEBAR_WIDTH = 'sidebar_width'
const KEY_GRID_ROW_HEIGHT = 'grid_row_height'
const KEY_GROUP_BY = 'group_by'
const KEY_SORT_WITHIN_GROUP = 'sort_within_group'
const KEY_TREE_DISPLAY_MODE = 'tree_display_mode'
const KEY_LAYOUT_MODE = 'layout_mode'
const KEY_SEAMLESS_GROUPS = 'seamless_groups'
const KEY_SEAMLESS_MINIMAP = 'seamless_minimap'
const KEY_AXIS_MODE = 'axis_mode'
const KEY_MINIMAP_RENDER_MODE = 'minimap_render_mode'
const KEY_CLOSE_BEHAVIOR = 'close_behavior'
const KEY_PINNED_SETTINGS = 'pinned_settings'
const KEY_SHOW_THUMB_INFO = 'show_thumb_info'
const KEY_THUMB_INFO_ELEMENTS = 'thumb_info_elements'
const KEY_HOVER_AUTOPLAY = 'hover_autoplay'
const KEY_SHOW_DRAG_HANDLE = 'show_drag_handle'
const KEY_AUTO_HIDE_CHROME_WINDOWED = 'auto_hide_chrome_windowed'
const KEY_HEAVY_VIDEO_MAX_PIXELS = 'heavy_video_max_pixels'
const KEY_HEAVY_VIDEO_MAX_BYTES = 'heavy_video_max_bytes'
const KEY_HOVER_DELAY_MS = 'hover_delay_ms'
const KEY_SEARCH_DEBOUNCE_MS = 'search_debounce_ms'
const KEY_RESIZE_DEBOUNCE_MS = 'resize_debounce_ms'
const KEY_VIDEO_KEYFRAME_COUNT = 'video_keyframe_count'
const KEY_GUIDE_SEEN = 'guide_seen'

/** 枚举候选集(与后端 schema 同值;损坏/陌生文本经 readSettingEnum 回落默认)。 */
const TREE_DISPLAY_MODES = ['registeredOnly', 'allFiles', 'allFilesWithHidden'] as const
const LAYOUT_MODES = ['justified', 'grid'] as const
const GROUP_BY_MODES = ['date', 'folder', 'none'] as const
const SORT_WITHIN_GROUP_MODES = ['datetime', 'filename', 'similarity'] as const
const AXIS_MODES = ['timeline', 'minimap'] as const
const MINIMAP_RENDER_MODES = ['colors', 'thumbnails'] as const
const CLOSE_BEHAVIORS = ['ask', 'minimize_to_tray', 'exit'] as const

/** 后端退出被「设置未落盘」拦下时的稳定 code(lifecycle.rs);前端据此给重试/放弃出口。 */
const SETTINGS_FLUSH_FAILED_CODE = 'settings_flush_failed'

/**
 * 把「本地预览态」与中央值单向对齐:仅当**中央值本身发生变化**时才回灌本地。
 *
 * 为什么需要比较旧值:部分偏好有会话内的临时覆盖(URL view-pref 恢复、拖拽中的预览),
 * 它们刻意不写盘也不改变中央值。若每逢快照替换就无条件回灌,任何其他设置的保存都会把
 * 这些临时覆盖挤掉——故只在中央值真的换了(启动水合 / 恢复默认 / 外部编辑)时同步。
 *
 * 但**代次推进(恢复默认)必须无条件回灌**:重置后即使该键的权威值与重置前逐字相同
 * (例如 group_by 中央值一直是默认 date,而本地被 URL 覆盖成 folder),会话覆盖也必须失效,
 * 否则重置后界面仍停在 folder。故 generation 变化是独立的一条强制同步触发源。
 */
function syncLocalFromSetting<T>(
  key: string,
  target: { value: T },
  parse: () => T,
): void {
  const sync = () => {
    target.value = parse()
  }
  watch(() => readSetting(key), sync)
  // 代次推进 = 发生过重置:强制以权威值覆盖会话内的临时覆盖。
  watch(settingsGeneration, sync)
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
}

export const useUiStore = defineStore('ui', () => {
  // 退出被「设置未落盘」拦下时的用户可见反馈(经 toast 给重试/放弃两个出口)。
  const toast = useToastStore()
  // ── 语言 ────────────────────────────────────────────────────────────────
  // 外观模式、主题参数、窗口材质与首帧缓存已迁出本 store(见 stores/themeStore):本文件不再
  // 转发旧主题 API,也不再写 data-theme / data-glass —— 主题域的呈现属性与色板由 themeStore
  // 单源发布,DOM 与 Canvas 消费同一份 currentPalette。
  //
  // 存储(设置集中保存):读中央设置快照,写入经 writeSettings——本 store 不再自持持久化,
  // 也不读旧 localStorage;恢复默认后经同一快照自动回落默认值。
  const language = computed<string>(() => readSetting(KEY_LANGUAGE) ?? 'zh-CN')

  function applyLanguage(lang: string) {
    document.documentElement.setAttribute('lang', lang)
    if (i18n.global.locale.value !== lang) {
      i18n.global.locale.value = lang as typeof i18n.global.locale.value
    }
  }

  /** 语言切换:先即时应用(界面立刻跟手),再经中央服务提交;失败由中央服务统一提示并回滚。 */
  function setLanguage(lang: string) {
    applyLanguage(lang)
    writeSettings({ [KEY_LANGUAGE]: lang }).catch(() => {})
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
      // guide_seen 是**内部状态**(留 DB):重置设置不得重放引导,故不经中央设置服务写。
      invokeIpc(IPC.SET_APP_CONFIG, { key: KEY_GUIDE_SEEN, value: 'true' }).catch(
        logStateSaveError(KEY_GUIDE_SEEN),
      )
    }
  }

  /** 启动配置批水合:guideSeen 与 R2-4 一样只读不写。 */
  function hydrateGuideSeen(val: string | null) {
    guideSeen.value = val === 'true'
  }

  // 注：thumbStrategy / gpuEngine 此前在此双持（configStore 也持有并镜像至此），但 uiStore 这份
  // 只被写、从不被读——已删，单一来源归 configStore（S5/T19 去重）。

  // ── 侧边栏 ────────────────────────────────────────────────────────────
  // 侧栏宽度:拖拽期是高频预览(每帧改),故用本地 ref 跟手,松手才经中央服务提交;
  // 快照变化(启动水合/重置/外部编辑)经下方 watch 回灌本地值。
  const sidebarWidth = ref(readSettingNumber(KEY_SIDEBAR_WIDTH, DEFAULTS.SIDEBAR_WIDTH))
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
    // 拖拽结束提交一次(拖动期只走 setSidebarWidth 的本地预览)。
    writeSettings({ [KEY_SIDEBAR_WIDTH]: String(sidebarWidth.value) }).catch(() => {})
  }

  // ── Active view:已拆出至 stores/viewStore.ts(P1-21 渐进拆分第二刀)。消费方改用 useViewStore()。 ──

  // ── 排序 ───────────────────────────────────────────────────────────────
  const sortOrder = ref<'asc' | 'desc'>('desc')

  // ── 网格显示设置 ────────────────────────────────────────────────
  // 行高是连续拖动控件:本地 ref 跟手,提交经中央服务(防抖),快照变化回灌。
  const gridRowHeight = ref(readSettingNumber(KEY_GRID_ROW_HEIGHT, DEFAULTS.GRID_ROW_HEIGHT))

  function setGridRowHeight(h: number) {
    gridRowHeight.value = h
    writeSettings({ [KEY_GRID_ROW_HEIGHT]: String(h) }, { debounce: true }).catch(() => {})
  }

  // ── 分组和排序设置 ──────────────────────────────────────────────
  // groupBy/sortWithinGroup 有「临时态」语义(语义搜索期间切 'none'/'similarity' 但不持久化),
  // 故本地 ref 是会话态:读取以本地为准(persist=false 时不得写盘),快照变化只在不冲突时回灌。
  const groupBy = ref<'date' | 'folder' | 'none'>(
    readSettingEnum(KEY_GROUP_BY, GROUP_BY_MODES, 'date'),
  )
  const sortWithinGroup = ref<'datetime' | 'filename' | 'similarity'>(
    readSettingEnum(KEY_SORT_WITHIN_GROUP, SORT_WITHIN_GROUP_MODES, 'datetime'),
  )

  // persist 参数(2026-07-06 审查 P1-19):语义搜索等**临时态**切换分组/排序时须传 persist=false,
  // 否则会把临时的 'none'/'similarity' 写进持久化配置——用户重启后原分组偏好被永久覆盖。
  // 用户经设置/工具栏的显式切换走默认 persist=true。
  function setGroupBy(mode: 'date' | 'folder' | 'none', persist = true) {
    groupBy.value = mode
    if (persist) {
      writeSettings({ [KEY_GROUP_BY]: mode }).catch(() => {})
    }
  }

  function setSortWithinGroup(sort: 'datetime' | 'filename' | 'similarity', persist = true) {
    sortWithinGroup.value = sort
    if (persist) {
      // 埋点仅在真实用户选择(persist===true)时计数,异步不阻塞(见 recordSortModeUsage 注释)。
      recordSortModeUsage(sort)
      writeSettings({ [KEY_SORT_WITHIN_GROUP]: sort }).catch(() => {})
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
  const treeDisplayMode = computed<TreeDisplayMode>(() =>
    readSettingEnum(KEY_TREE_DISPLAY_MODE, TREE_DISPLAY_MODES, 'registeredOnly'),
  )

  function setTreeDisplayMode(mode: TreeDisplayMode) {
    writeSettings({ [KEY_TREE_DISPLAY_MODE]: mode }).catch(() => {})
  }

  // ── Layout mode（T20）：'justified' 等高行（默认）/ 'grid' 均匀宫格 ───────────────
  // 后端按此切换排版算法（compute_layout 的 layoutMode 参数）；前端据此切单元方图裁切。
  // 本地预览态:URL view-pref 恢复会直接赋值它(会话内覆盖,不写盘),故用本地 ref 并只在
  // 中央值本身变化时回灌(见 syncLocalFromSetting)。
  const layoutMode = ref<'justified' | 'grid'>(
    readSettingEnum(KEY_LAYOUT_MODE, LAYOUT_MODES, 'justified'),
  )

  function setLayoutMode(mode: 'justified' | 'grid') {
    writeSettings({ [KEY_LAYOUT_MODE]: mode }).catch(() => {})
  }

  // ── 无缝分组（#1,2026-07-17）───────────────────────────────────────────────
  // 开 = 排序仍按 groupBy 聚合(folder DFS / date 日桶),但打包无分隔符、行跨组连续——
  // 视觉如不分组、数据保留组序。groupBy='none' 时无效果(本就全局序)。
  // 无分隔符行 → 时间轴 scrubber 失据自动隐藏，右轴槽位由 minimap 自动接管。
  const seamlessGroups = computed(() => readSettingBool(KEY_SEAMLESS_GROUPS, false))

  function setSeamlessGroups(val: boolean) {
    writeSettings({ [KEY_SEAMLESS_GROUPS]: String(val) }).catch(() => {})
  }

  // ── 轴开合(两模式通用)────────────────────────────────────────────────────
  // 所有非空画廊都可挂时间轴 scrubber 或 VSCode 式微缩预览(MinimapAxis)；两形态
  // 间由轴形态钮切换。本开关只管轴整体显隐，由 chevron 收合钮驱动并持久化。
  // 默认开(保轴可发现性):仅显式存过 'false'(chevron 收起过)才隐。
  const axisVisible = computed(() => readSettingBool(KEY_SEAMLESS_MINIMAP, true))

  function setAxisVisible(val: boolean) {
    writeSettings({ [KEY_SEAMLESS_MINIMAP]: String(val) }).catch(() => {})
  }

  // ── 轴形态偏好(2026-07-24)────────────────────────────────────────────────
  // timeline|minimap,两分组模式通用;持久化替代原会话态 preferredAxis(重载不粘的根因)。
  const axisMode = computed<'timeline' | 'minimap'>(() =>
    readSettingEnum(KEY_AXIS_MODE, AXIS_MODES, 'timeline'),
  )

  function setAxisMode(val: 'timeline' | 'minimap') {
    writeSettings({ [KEY_AXIS_MODE]: val }).catch(() => {})
  }

  // 配置缺省回落 thumbnails(既有行为:未持久化时用缩略图预览)。
  const minimapRenderMode = computed<MinimapRenderMode>(() =>
    readSettingEnum(KEY_MINIMAP_RENDER_MODE, MINIMAP_RENDER_MODES, 'thumbnails'),
  )

  function setMinimapRenderMode(val: MinimapRenderMode) {
    writeSettings({ [KEY_MINIMAP_RENDER_MODE]: val }).catch(() => {})
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
  const closeBehavior = computed<'ask' | 'minimize_to_tray' | 'exit'>(() =>
    readSettingEnum(KEY_CLOSE_BEHAVIOR, CLOSE_BEHAVIORS, 'ask'),
  )
  const showCloseConfirmDialog = ref(false)

  function setCloseBehavior(behavior: 'ask' | 'minimize_to_tray' | 'exit') {
    writeSettings({ [KEY_CLOSE_BEHAVIOR]: behavior }).catch(() => {})
  }

  // 关窗请求单源分派(#12):标题栏 ✕(后端 window-close-requested → App.vue 监听)与
  // mod+W(AppShell 全局键)共用——按 closeBehavior 走 托盘隐藏/直接退出/弹确认框。
  async function requestAppClose() {
    if (closeBehavior.value === 'minimize_to_tray') {
      await invokeIpc(IPC.HIDE_WINDOW)
    } else if (closeBehavior.value === 'exit') {
      await exitAppWithFlushGuard()
    } else {
      showCloseConfirmDialog.value = true
    }
  }

  /**
   * 退出应用,并处理「退出前落盘失败」这一真实结果。
   *
   * 后端 EXIT_APP 会先等前端在途设置落盘;写盘失败时它不退出,而是返回稳定 code
   * settings_flush_failed。此时若什么都不做,用户点了 ✕ 会**看似毫无反应**——故给出
   * 「重试」与「放弃未保存修改并退出」两个明确出口:既不静默丢失改动,也不把失败当成已保存。
   * 放弃走 force 分支(后端跳过 flush 直接退),是用户显式承担丢改动的选择。
   */
  async function exitAppWithFlushGuard(): Promise<void> {
    try {
      await invokeIpc(IPC.EXIT_APP)
    } catch (e) {
      const code = (e as { code?: string }).code
      if (code !== SETTINGS_FLUSH_FAILED_CODE) throw e
      logger.warn('exit blocked: settings flush failed', { code })
      toast.addToast('error', i18n.global.t('closeConfirm.flushFailed'), 8000, [
        {
          label: i18n.global.t('closeConfirm.retry'),
          onClick: () => void exitAppWithFlushGuard(),
        },
        {
          label: i18n.global.t('closeConfirm.exitAnyway'),
          onClick: () => {
            void invokeIpc(IPC.EXIT_APP, { force: true }).catch((err) =>
              logger.error('forced exit failed', { error: err }),
            )
          },
        },
      ])
    }
  }

  // ── Pinned Settings ──────────────────────────────────────────────────────
  // 置顶清单:可从中央快照重建(结构类设置,值表里是规范 JSON 文本),故用 computed 读;
  // 两个常驻工具项由默认值保证存在(schema 默认已含),无需再补种写盘。
  const pinnedSettings = computed<string[]>(() =>
    parseSettingJson<string[]>(readSetting(KEY_PINNED_SETTINGS), []).filter(
      (k): k is string => typeof k === 'string',
    ),
  )

  function togglePinnedSetting(key: string) {
    const idx = pinnedSettings.value.indexOf(key)
    const next = [...pinnedSettings.value]
    if (idx >= 0) next.splice(idx, 1)
    else next.push(key)
    writeSettings({ [KEY_PINNED_SETTINGS]: JSON.stringify(next) }).catch(() => {})
  }

  // 将置顶工具从一个位置移动到另一个位置（拖拽排序）并持久化。
  function reorderPinnedSetting(fromIndex: number, toIndex: number) {
    const arr = [...pinnedSettings.value]
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
    writeSettings({ [KEY_PINNED_SETTINGS]: JSON.stringify(arr) }).catch(() => {})
  }

  // ── Thumbnail Info Overlays ──────────────────────────────────────────────
  // 默认开(S7,用户 2026-07-15 裁决):此前默认关,而元素勾选面板由 `v-if="ui.showThumbInfo"`
  // 门控 → 整套逐元素配置对没主动开过总开关的用户不可见,功能事实上被藏起来。默认开只让
  // 面板可发现;thumbInfoElements 仍默认空,故缩略图观感不变(无元素勾选=无徽章无信息行)。
  // 显式存过 'false' 的用户不受影响(配置读回覆盖默认)。
  const showThumbInfo = computed(() => readSettingBool(KEY_SHOW_THUMB_INFO, true))
  const thumbInfoElements = computed<string[]>(() =>
    parseSettingJson<string[]>(readSetting(KEY_THUMB_INFO_ELEMENTS), []).filter(
      (el): el is string => typeof el === 'string',
    ),
  )

  function setShowThumbInfo(val: boolean) {
    writeSettings({ [KEY_SHOW_THUMB_INFO]: String(val) }).catch(() => {})
  }

  function setThumbInfoElements(elements: string[]) {
    writeSettings({ [KEY_THUMB_INFO_ELEMENTS]: JSON.stringify(elements) }).catch(() => {})
  }

  // ── 悬停自动播放（需求1） ──────────────────────────────────────────────────
  // 鼠标移入视频/动态照片格子 → 自动静音循环预览。默认开启，持久化到 app_config。
  const hoverAutoplay = computed(() => readSettingBool(KEY_HOVER_AUTOPLAY, true))

  function setHoverAutoplay(val: boolean) {
    writeSettings({ [KEY_HOVER_AUTOPLAY]: String(val) }).catch(() => {})
  }

  // ── 配置重构批次C:5 个前端阈值键(advanced,只读——无设置页 UI,无 setter) ──────────
  // 悬停预览「重」视频判据(与既有 useHoverPreview.ts::HEAVY_VIDEO_PIXELS/HEAVY_VIDEO_BYTES
  // 同默认值,批次C起改由此处响应式下发,消费点不再自持模块级常量)。
  const heavyVideoMaxPixels = computed(() =>
    readSettingNumber(KEY_HEAVY_VIDEO_MAX_PIXELS, 3840 * 2160),
  )
  const heavyVideoMaxBytes = computed(() =>
    readSettingNumber(KEY_HEAVY_VIDEO_MAX_BYTES, 10 * 1024 * 1024 * 1024),
  )
  // 悬停延迟(与既有 useHoverPreview.ts::HOVER_DELAY_MS 同默认值)。
  const hoverDelayMs = computed(() => readSettingNumber(KEY_HOVER_DELAY_MS, 200))
  // 搜索框混合/语义模式提交防抖(默认值单源 constants/defaults.ts::SEARCH_DEBOUNCE_MS)。
  const searchDebounceMs = computed(() =>
    readSettingNumber(KEY_SEARCH_DEBOUNCE_MS, DEFAULTS.SEARCH_DEBOUNCE_MS),
  )
  // 布局重算防抖(默认值单源 constants/defaults.ts::RESIZE_DEBOUNCE_MS)。
  const resizeDebounceMs = computed(() =>
    readSettingNumber(KEY_RESIZE_DEBOUNCE_MS, DEFAULTS.RESIZE_DEBOUNCE_MS),
  )
  // 小批 C2:雪碧图切帧列数(与既有 useHoverPreview.ts::KEYFRAME_COUNT 同默认值,消费点改
  // 响应式读取此 ref、不再自持模块级常量)。
  const videoKeyframeCount = computed(() => readSettingNumber(KEY_VIDEO_KEYFRAME_COUNT, 10))

  // ── 选择态拖拽手柄显隐(2026-07-17 #5) ────────────────────────────────────
  // 选中缩略图左上角的拖拽手柄(拖入文件夹用)可选隐藏。默认开;关闭后 DOM 不渲染、
  // Canvas 不绘制**且命中判定同步跳过**(只藏不停用会出现「看不见却能拖」的幽灵手柄)。
  const showDragHandle = computed(() => readSettingBool(KEY_SHOW_DRAG_HANDLE, true))

  function setShowDragHandle(val: boolean) {
    writeSettings({ [KEY_SHOW_DRAG_HANDLE]: String(val) }).catch(() => {})
  }

  // ── 窗口化沉浸模式(2026-07-23) ──────────────────────────────────────────
  // 非全屏(且非查看器沉浸)时,用户可选让顶栏/底栏自动隐藏、鼠标移到窗口边缘再唤出
  // (useChromeReveal.chromeAutoHidden 的第三来源)。默认关:这是可选的沉浸偏好,不该
  // 悄悄改变新用户的默认交互。
  // 本地预览态(同 layoutMode:测试与会话逻辑会直接赋值)。
  const autoHideChromeWindowed = ref(readSettingBool(KEY_AUTO_HIDE_CHROME_WINDOWED, false))

  function setAutoHideChromeWindowed(val: boolean) {
    // 采集即预览(经中央服务的本地即时预览),故不再需要「先落盘后改 state」的次序技巧:
    // 消费点读的是同一个中央值,不存在本地旧值抢跑。
    writeSettings({ [KEY_AUTO_HIDE_CHROME_WINDOWED]: String(val) }).catch(() => {})
  }

  // ── 中央设置快照的运行时应用 ──────────────────────────────────────────────
  // 启动水合、恢复默认、外部编辑与跨窗口同步都以「设置快照变化」这一件事收敛到下方 watch:
  // 应用副作用只有这一处,消费点读的是同一份中央值。故此处**只应用、不写回**——刷新路径若
  // 触发保存,会把「外部编辑器刚改的值」用内存旧值覆盖成假循环。
  watch(
    settingsValues,
    () => {
      // 日志 off 档联动:启动早期(useUiStore 在 App.vue setup 即实例化)就该位,不等设置页。
      setLoggerEnabled((readSetting('log_level') ?? 'info') !== 'off')
      // 主题参数、材质与色板的应用副作用归 themeStore(它监听同一份快照):本 store 只负责
      // 自己的设置项,不再写 data-theme / data-color-scheme / data-glass 或首帧缓存。
      // 侧栏宽度与行高是本地预览态(拖动跟手):快照给出权威值时回灌,但仅在水合/重置这类
      // 整份替换时,故不与拖拽中的本地值打架——拖拽结束才提交,提交回执即同值。
      sidebarWidth.value = readSettingNumber(KEY_SIDEBAR_WIDTH, DEFAULTS.SIDEBAR_WIDTH)
      document.documentElement.style.setProperty('--sidebar-width', `${sidebarWidth.value}px`)
      gridRowHeight.value = readSettingNumber(KEY_GRID_ROW_HEIGHT, DEFAULTS.GRID_ROW_HEIGHT)
    },
  )

  // 本地预览态与中央值对齐(仅中央值变化时回灌,保住会话内的临时覆盖)。
  syncLocalFromSetting(KEY_GROUP_BY, groupBy, () =>
    readSettingEnum(KEY_GROUP_BY, GROUP_BY_MODES, 'date'),
  )
  syncLocalFromSetting(KEY_SORT_WITHIN_GROUP, sortWithinGroup, () =>
    readSettingEnum(KEY_SORT_WITHIN_GROUP, SORT_WITHIN_GROUP_MODES, 'datetime'),
  )
  syncLocalFromSetting(KEY_LAYOUT_MODE, layoutMode, () =>
    readSettingEnum(KEY_LAYOUT_MODE, LAYOUT_MODES, 'justified'),
  )
  syncLocalFromSetting(KEY_AUTO_HIDE_CHROME_WINDOWED, autoHideChromeWindowed, () =>
    readSettingBool(KEY_AUTO_HIDE_CHROME_WINDOWED, false),
  )

  /**
   * 兼容既有调用点(useConfigFile 的 config-file-changed):中央服务已统一应用快照,
   * 此处保留为显式刷新入口,供需要主动重取的场景使用。
   */
  async function refreshFromBackend() {
    await refreshSettingsFromBackend()
  }

  /**
   * 共享的启动 Promise(既有消费方:App.vue 的首启/引导判定、useGalleryQuerySync 的水合门)。
   * 实际实现经中央服务的 initializeSettings——全应用只发一次请求,失败时 resolve 为 null 而不是
   * 抛出,使排在它后面的水合门/首屏逻辑仍能继续(读取失败的可重试提示由 App 层给出)。
   */
  const startupConfigPromise: Promise<StartupPayload | null> = initializeSettings().catch((e) => {
    logger.error('uiStore startup settings initialization failed', { error: e })
    return null
  })

  return {
    // 语言(外观模式、主题与材质见 stores/themeStore)
    language,
    applyLanguage,
    setLanguage,
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
    // 配置重构批次C:5 个前端阈值键(只读,无 setter——见其声明处注释)
    heavyVideoMaxPixels,
    heavyVideoMaxBytes,
    hoverDelayMs,
    searchDebounceMs,
    resizeDebounceMs,
    videoKeyframeCount,
  }
})
