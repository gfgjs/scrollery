<template>
  <AppShell>
    <!-- 自绘标题栏(顶栏重构 L1)。全宽置顶,承载窗口三键 + 拖拽区(P1);内容槽内嵌 ContextualToolbar(P3)。
         沉浸模式(2026-07-10 用户裁决):不再整条隐藏(frameless 下这是唯一拖拽区/三键,藏死则窗口
         不可移动/关闭),改「鼠标移入顶部滑入显示」——host 变 fixed 出流(内容全屏)+ translateY 收起,
         指针贴顶滑入、移出滑走。
         2026-07-16:自动隐藏的**来源**从「仅查看器沉浸」推广为 chromeAutoHidden(查看器沉浸 ∪ F11 全屏),
         底栏(AppShell 的 app-statusbar)同款对称处理。唤出态跨组件,故迁入 useChromeReveal 单例。
         唤出后不可拖窗:那是窗口三态的推论(全屏 → canDragWindow=false),由 useWindowDrag 直读
         useWindowMode 落实,此处无需也不应再管一份。 -->
    <template #titlebar>
      <!-- inert:收起态标题栏虽 translateY 出视口,控件仍在 Tab 序——键盘用户会 Tab 进
           一排看不见的按钮;inert 一并移出焦点序,F10 唤出即解除。 -->
      <div
        ref="titlebarHostEl"
        class="titlebar-host"
        data-chrome-top
        :class="{
          'titlebar-host--immersive': chromeAutoHidden,
          'titlebar-host--revealed': chromeAutoHidden && topRevealed,
        }"
        :inert="chromeAutoHidden && !topRevealed"
        @focusout="onTitlebarFocusOut"
      >
        <WindowChrome>
          <!-- 品牌标识常驻标题栏左端(Phase G, logo 从侧栏移入) -->
          <TitlebarBrand />
          <!-- 标题栏承载窗口级/上下文导航命令。 -->
          <ContextualToolbar />
          <!-- merged 模式(默认,Phase G 合并):Gallery 工具栏并入标题栏同一行,与窗口三键共享拖拽区;
               分离模式(S3 分层)走下方独立 #toolbar 槽。二者由 useTitlebarMode 的 merged 开关互斥切换。
               AppToolbar 是多根 fragment(.toolbar__left + .toolbar__foldable〔flex:1〕),两根直接作 flex 子项
               流入 .window-chrome__content——不加 class(多根无法继承,且会触发 Vue 警告),布局同 Phase G。 -->
          <AppToolbar v-if="showGalleryToolbar && titlebarMerged" />
          <span
            v-if="viewer.activeViewer"
            class="titlebar-viewer-title"
            :title="viewer.activeViewer.title"
          >
            {{ viewer.activeViewer.title }}
          </span>
        </WindowChrome>
      </div>
    </template>

    <template #sidebar>
      <!-- 设置快照到达前不挂载侧栏:文件夹树与库统计都是用户资产信息(演示打码未就位时不得先露出)。 -->
      <AppSidebar v-if="settingsReady" />
    </template>

    <!-- 分离模式(!titlebarMerged):Gallery 工具栏独立成标题栏下方第二条 bar(S3 分层)。
         合并模式则 AppToolbar 已并入上方 WindowChrome,此槽不渲染。
         **常驻**(2026-07-16 用户裁决):此前本槽还要 showGalleryToolbar 为真才给,于是进合集/人物等非画廊页
         时整条消失、内容上跳 48px——分离模式的价值恰是「有一条稳定的第二轨道」。设置工作区是专用
         chrome，因此刻意不继承这条全局页面标题轨道；其余非查看器路由只渲染页面标题。
         查看器路由(/view /doc /audio)仍不给:那里要的是最大内容面积,且各查看器自持局部控制条。
         AppToolbar 本身仍严格 v-if 在画廊路由——它挂 document 级 keydown 分发(注册表),若在总览/设置页
         也挂载,会与 AppShell 的兜底分发**双执行**(见 AppShell onKeyDown ①)。 -->
    <template v-if="showToolbarBar" #toolbar>
      <AppToolbar v-if="showGalleryToolbar" />
      <span v-else class="toolbar-page-title">{{ pageTitleKey ? $t(pageTitleKey) : '' }}</span>
    </template>

    <!-- 默认插槽：媒体内容或语义搜索面板 -->
    <!-- 语义搜索面板显示在主库血统视图(smart-album + folder 筛选)。S2-c 前这些视图全停在 '/',故此前判据是
         route.path === '/';拆分为多路径后平移到 isPrimaryGalleryRoute 以保持同一集合(不含 collection/person 详情)。 -->
    <!-- 启动设置就绪门控(设置集中保存 §5.1 + 演示打码回归):中央快照到达前不渲染任何会展示用户
         资产的内容——演示打码等「默认安全」值在快照就位前只是占位,此时渲染画廊会先露一帧真实内容;
         读取失败同样不放行,只给可重试的错误面板。门内只有资产内容:窗口外壳(标题栏/侧栏/底栏)与
         全局关闭确认、通知层都留在门外,故启动失败时用户仍能正常关窗。 -->
    <template v-if="settingsReady">
      <SemanticSearchPanel v-show="isPrimaryGalleryRoute(route.path)" />
      <!-- 图/视频查看器保留 `/view/:id` 的 URL/历史/深链语义，但不再替换 MediaGrid 的 DOM。
           该路由期间同一画廊实例继续作为底层帧，ContentViewer 由下方绝对覆盖层呈现；关闭时
           立即撤掉覆盖层，不经历 KeepAlive 激活、首帧或 route Transition 等待。 -->
      <RouterView v-slot="{ Component }">
        <KeepAlive :include="['GalleryRouteLayer']">
          <component :is="contentViewerRoute ? GalleryRouteLayer : Component" />
        </KeepAlive>
      </RouterView>
      <div v-if="contentViewerRoute" class="content-viewer-route-overlay">
        <ContentViewerOverlay />
      </div>
    </template>
    <div v-else class="settings-gate">
      <div v-if="startupError" class="settings-gate__panel" role="alert">
        <h2 class="settings-gate__title">{{ t('settings.loadFailedTitle') }}</h2>
        <p class="settings-gate__hint">{{ t('settings.loadFailedHint') }}</p>
        <button class="btn btn-primary" @click="retryStartupSettings">
          {{ t('settings.loadFailedRetry') }}
        </button>
      </div>
    </div>

    <template #statusbar>
      <AppStatusBar />
    </template>
  </AppShell>
  <PerformancePanel />

  <!-- 顶栏重构 P4-b:详情覆盖层已退役,图/视迁为 /view/:id 路由(ContentViewer),不再全局挂载。 -->

  <!-- 文档缩略图离屏渲染器（P4, §3.4）—— 隐藏、节流 -->
  <DocThumbRenderer />

  <!-- 吐司通知 -->
  <ToastContainer />

  <!-- 关闭确认弹窗 -->
  <CloseConfirmDialog />

  <!-- 首启向导（T17，§3.8）：3 步引导（目录/主题/语言），first_launch 缺省时显示,完成或跳过后写 false 不再弹 -->
  <OnboardingWizard v-if="showOnboarding" @done="onOnboardingDone" />

  <!-- 详细首次使用引导手册:首启向导完成/跳过后若未看过手册,自动接续弹出;设置页头部随时可重开。 -->
  <UserGuide v-if="ui.userGuideOpen" />

  <!-- 导出整理成果对话框(方案 A §4):全局单例,任意入口经 exportStore.openExportDialog 唤出。 -->
  <ExportDialog />
</template>

<script setup lang="ts">
import {
  defineAsyncComponent,
  onMounted,
  onBeforeUnmount,
  ref,
  computed,
  watch,
  nextTick,
} from 'vue'
import { invokeIpc } from './utils/ipc'
import { logger } from './utils/logger'
import { dismissStartupLayer } from './utils/startupLayer'
import { IPC, EVENTS } from './constants/ipc'
import { useTauriListen } from './composables/useTauriListen'
import { resetOcrStatusCache } from './composables/useOcr'
import { useEnhanceStore } from './stores/enhanceStore'
import {
  applyTimelineScrollWidth,
  applyTimelineAxisWidth,
  applyAxisViewportOpacity,
} from './utils/uiScale'
import { useDerivationAutoStart } from './composables/useDerivationAutoStart'
import { useConfigFile } from './composables/useConfigFile'
import { useToastStore } from './stores/toastStore'
import { useI18n } from 'vue-i18n'
import { installSettingsLifecycle } from './composables/useSettingsLifecycle'
import {
  initializeSettings,
  readSetting,
  setSettingsApplyFailedFormatter,
  setSettingsErrorReporter,
  setSettingsWriteFailedFormatter,
  settingsReady,
} from './stores/settingsPersistence'
import type { StartupPayload } from './types/config'
import { useTitlebarMode } from './composables/useTitlebarMode'
import {
  chromeAutoHidden,
  topRevealed,
  collapseTop,
  shouldCollapseOnFocusOut,
  useChromeRevealListeners,
} from './composables/useChromeReveal'
import { useUiStore } from './stores/uiStore'
import { useConfigStore } from './stores/configStore'
import { useViewerStore } from './stores/viewerStore'
import { useDirectoryMoveRecoveryStore } from './stores/directoryMoveRecoveryStore'

import AppShell from './components/layout/AppShell.vue'
import WindowChrome from './components/layout/WindowChrome.vue'
import ContextualToolbar from './components/layout/ContextualToolbar.vue'
import TitlebarBrand from './components/layout/TitlebarBrand.vue'
import AppSidebar from './components/sidebar/AppSidebar.vue'
import AppToolbar from './components/layout/AppToolbar.vue'
import AppStatusBar from './components/layout/AppStatusBar.vue'
import SemanticSearchPanel from './components/media/SemanticSearchPanel.vue'
import GalleryRouteLayer from './components/media/GalleryRouteLayer.vue'
import ToastContainer from './components/common/ToastContainer.vue'
import CloseConfirmDialog from './components/common/CloseConfirmDialog.vue'
import OnboardingWizard from './components/common/OnboardingWizard.vue'
import UserGuide from './components/common/UserGuide.vue'
import DocThumbRenderer from './components/media/DocThumbRenderer.vue'
import PerformancePanel from './components/devtools/PerformancePanel.vue'
import ExportDialog from './components/media/ExportDialog.vue'
import { useAiStore } from './stores/aiStore'
import { useFaceStore } from './stores/faceStore'
import { useScanStore } from './stores/scanStore'
import { useCollectionStore } from './stores/collectionStore'
import { useViewStore } from './stores/viewStore'
import { useRoute } from 'vue-router'
import { listenAppEvent } from './utils/appEvents'
import { isGalleryRoute } from './utils/galleryQuery'
import { isContentViewerRoute, isViewerRoute } from './utils/mediaRoute'
import { routeToView, isPrimaryGalleryRoute } from './utils/viewRoute'
import { useGalleryQuerySync } from './composables/useGalleryQuerySync'
import { loadViewerComponent } from './router/viewerRouteLoader'

const ui = useUiStore()
const config = useConfigStore()
const viewer = useViewerStore()
const ai = useAiStore()
const face = useFaceStore()
const collections = useCollectionStore()
const viewStore = useViewStore()
const route = useRoute()
const contentViewerRoute = computed(() => isContentViewerRoute(route.path))
// `loadViewerComponent` 与路由记录共用同一个 Promise：画廊的空闲预取、路由解析和覆盖层挂载
// 不会重复下载查看器 chunk。
const ContentViewerOverlay = defineAsyncComponent(loadViewerComponent)

// S2-b:Gallery 筛选 ↔ URL query 双向同步（画廊路由内生效,深链/刷新可恢复筛选）。
useGalleryQuerySync()

// 参数化视图 route 是 Collection/Person 当前内容的单一事实源。Collection 需要实体元数据用于
// backend filter，深链首次进入时先加载列表再水合；token 防快速导航时旧请求回写新 route。
// 监听 route.path 而非 fullPath：视图维度只由路径段（collection/person id）决定，与 query 无关。
// 若监听 fullPath，S2-b 的筛选 query 同步（router.replace 改 query）会误触发本 watcher →
// 在收藏夹/人物视图切一个筛选就 setActiveX(同一个) → clearSelection + 重跑水合（S2-b 引入的回归）。
let viewRouteToken = 0
watch(
  () => route.path,
  async () => {
    const token = ++viewRouteToken
    const collectionMatch = route.path.match(/^\/collections\/(\d+)$/)
    if (collectionMatch) {
      const id = Number(collectionMatch[1])
      let collection = collections.collections.find((item) => item.id === id)
      if (!collection) {
        await collections.load()
        collection = collections.collections.find((item) => item.id === id)
      }
      if (token === viewRouteToken) viewStore.setActiveCollection(collection ?? null)
      return
    }

    const personMatch = route.path.match(/^\/persons\/(\d+)$/)
    if (personMatch) {
      viewStore.setActivePerson(Number(personMatch[1]))
      return
    }

    // S2-c:smart-album / folder 筛选态从 path 回填 viewStore(深链/刷新恢复)。侧栏点击已同步设过 viewStore,
    // 相等守卫使此处跳过、不重复 clearSelection;完备守卫须连同 collection/person 一起比对——例如从
    // /collections/5 导航到 '/' 时 activeSmartAlbum 恰为 'all'(setActiveCollection 会置 'all'),若只比 album
    // 会漏清 activeCollection。folder 仅模式B(/folder/:id)进此分支;模式A 滚动锚点态停在 '/'(album 'all'),
    // 其滚动位置由 pendingScrollDirId 承载不进 URL(见 findings 会话续23)。
    const rv = routeToView(route.path)
    if (rv) {
      if (rv.kind === 'smartAlbum') {
        const alreadyThisAlbum =
          viewStore.activeSmartAlbum === rv.album &&
          viewStore.activeDirectoryId === null &&
          viewStore.activeCollection === null &&
          viewStore.activePersonId === null
        if (!alreadyThisAlbum) viewStore.setSmartAlbum(rv.album)
      } else {
        // setActiveDirectory 已保证互斥(清 collection/person、album 置 'all'),故只需比对 directoryId。
        if (viewStore.activeDirectoryId !== rv.id) viewStore.setActiveDirectory(rv.id)
      }
      return
    }

    if (route.path === '/collections') viewStore.setActiveCollection(null)
    if (route.path === '/persons') viewStore.setActivePerson(null)
  },
  { immediate: true },
)

// 画廊页面工具栏仅主视图显示；标题栏保留 brand/window/context navigation，富筛选与搜索
// 独立成第二层，避免与窗口三键争抢横向空间。内容页上下文命令由 ContextualToolbar 提供。
// 查看器路由判据统一收敛到 utils/mediaRoute(与 openMediaRoute 的 push/replace 分流同源)。
// 画廊路由判据抽入 utils/galleryQuery.isGalleryRoute 作单源,与 useGalleryQuerySync 共用,避免谓词漂移。
const showGalleryToolbar = computed(() => isGalleryRoute(route.path))
// 设置页是独立工作区：路径前缀覆盖所有设置子路由，避免分离模式残留全局页面标题 toolbar。
const settingsRoute = computed(() => route.path.startsWith('/settings'))
// 标题栏与 Gallery 工具栏「合并/分离」布局开关(默认合并)。merged=工具栏并入标题栏;分离=独立第二条 bar。
const { merged: titlebarMerged } = useTitlebarMode()
// 分离模式第二条 bar 是否给槽(常驻契约,见模板注释):分离模式 ∧ 非查看器路由。
// AppShell 据 $slots.toolbar 决定是否渲染该条,故这个判据同时就是「这条存在吗」的单一事实源。
const showToolbarBar = computed(
  () => !titlebarMerged.value && !isViewerRoute(route.path) && !settingsRoute.value,
)
// 非画廊页在该条上显示的页面标题(i18n 键存在 route.meta.title,与窗口标题同源;取法同 router/index.ts)。
const pageTitleKey = computed(() => route.meta.title as string | undefined)

useTauriListen(EVENTS.OFFICIAL_LICENSE_CHANGED, () => {
  resetOcrStatusCache()
  void useEnhanceStore().fetchStatus()
})

// 首启向导显隐（T17）：onMounted 检测 first_launch 配置缺省时置真。
const showOnboarding = ref(false)

/** 首启向导完成/跳过的收尾:向导自身仅负责 first_launch,这里接续判断是否需要自动弹出详细引导手册
 *  (guideSeen 已 true 则不弹,用户可从设置头部随时强开)。 */
function onOnboardingDone() {
  showOnboarding.value = false
  if (!ui.guideSeen) ui.openUserGuide()
}

// ── 沉浸态标题栏 hover 显隐 ────────────────────────────────────────────────────
// 触发/收起/F10 键盘路径的机制已迁入 useChromeReveal(2026-07-16:顶栏底栏对称、来源扩到 F11 全屏,
// 且底栏宿主在 AppShell 内部——跨组件,局部 ref 承载不了)。本处只剩「顶栏宿主」这一侧的绑定。
const titlebarHostEl = ref<HTMLElement | null>(null)

// F10 键盘唤出后把焦点送进顶栏:inert 是响应式解除,须等 DOM 更新后才可聚焦;焦点给条内首个控件。
useChromeRevealListeners(() => {
  void nextTick(() => {
    titlebarHostEl.value
      ?.querySelector<HTMLElement>('button, [href], input, select, [tabindex]:not([tabindex="-1"])')
      ?.focus()
  })
})

// 指针收起走 useChromeReveal 内的几何判据(不再靠 mouseleave —— 那条与几何唤出不对称,是真机
// 「移入变隐藏、移出变显示」的病根,见该文件顶注)。此处只留焦点路径。
// 焦点离开标题栏(Tab 穿出/点击查看器)→ 收起;inert 化会自动把残留焦点逐出,无需手动还焦。
function onTitlebarFocusOut(e: FocusEvent) {
  if (shouldCollapseOnFocusOut('top', e.relatedTarget as Node | null)) collapseTop()
}

// 主题初始化已并入 uiStore 的启动配置批(appearance 三键随 get_startup_config 一次读取,
// R2-4 纪律);原 useTheme composable 的独立 IPC 已删除。
// 自动启动派生流水线（视频封面/关键帧、音频封面、epub 封面）。
// 此前无任何触发入口 → 流水线从未运行 → 视频封面不出现，本调用即根因修复。
useDerivationAutoStart()

// 外置配置文件（config.toml，批次B）：在 App 根组件（等价于 App 生命周期，永不卸载）挂全局
// config-file-changed/config-file-error 监听——热更新须任意页面都生效，不局限于设置页
// 打开期间。设置页（SettingsView.vue）再次调用同一 composable 只是复用同一份共享状态，
// 不会重复注册监听（见 useConfigFile.ts 头注）。
useConfigFile()

// 退出前 flush 协议的前端一侧(设置集中保存 §5.4):后端在退出/关窗前经事件请求一次落盘,
// 前端 flush 完再回执。装配点必须在**首次 await 之前**(见 onMounted 顶注),否则启动 IPC
// 卡住时退出收不到请求、后端只能等超时。
//
// 中央设置的失败提示也在此接一次口:写盘失败与「已保存但部分项未应用」经 toast 统一可见,
// 消费方各处 `void writeSettings(...)` 不必自己弹错,也不会静默显示成已保存。
const toast = useToastStore()
const { t } = useI18n()
setSettingsErrorReporter((message) => toast.addToast('error', message, 5000))
setSettingsWriteFailedFormatter(() => t('settings.saveFailedNotice'))
setSettingsApplyFailedFormatter((keys) => t('settings.applyFailedNotice', { keys: keys.join(', ') }))

let frontendHeartbeatTimer: number | undefined

/** 启动设置读取失败(主内容不渲染,给可重试入口;见模板的 ready 门控)。 */
const startupError = ref(false)

/**
 * 应用启动批里的**全局项与内部状态**。设置值本身已由中央服务应用(uiStore 的 watch),此处只处理
 * 不属设置快照的启动期项:语言、界面尺度(CSS 变量)与首启/引导标记。
 *
 * 首启/引导标记只在此处消费——快照刷新(resetSettings/外部编辑)不经过它,故重置设置不会重放
 * 首次使用引导。
 */
function applyStartupPayload(payload: StartupPayload) {
  const language = readSetting('language') ?? ui.language
  ui.applyLanguage(language)

  // 界面尺度:单一事实源(P1-18),与设置页 setter 共用同一应用函数。
  const timelineScrollWidth = readSetting('timeline_scroll_width')
  if (timelineScrollWidth != null) applyTimelineScrollWidth(Number(timelineScrollWidth))
  const timelineAxisWidth = readSetting('timeline_axis_width')
  if (timelineAxisWidth != null) applyTimelineAxisWidth(Number(timelineAxisWidth))
  const axisViewportOpacity = readSetting('axis_viewport_opacity')
  if (axisViewportOpacity != null) applyAxisViewportOpacity(Number(axisViewportOpacity))

  // 滚动条/视窗最小高、字号与悬停缩放的 DOM 副作用统一由 configStore 收敛(见
  // applyDomAffectingValues):启动这里显式推一次(首帧值可能恰好等于默认值,靠 watch 差分不会触发),
  // 之后的改动由它的快照 watch 跟进。
  config.applyDomAffectingValues()

  // 首启检测(T17,§3.8):仅当 first_launch 被显式写为 'false'(用户完成/跳过过引导)才抑制;
  // 其余情形(缺省 null/空串/意外值)都视为「未走过引导」→ 显示向导。
  if (payload.state.firstLaunch !== 'false') showOnboarding.value = true
  ui.hydrateGuideSeen(payload.state.guideSeen)
}

/** 设置就绪门控:读取失败时可重试一次(不改用占位默认值放行)。 */
async function retryStartupSettings() {
  startupError.value = false
  try {
    const payload = await initializeSettings()
    applyStartupPayload(payload)
  } catch (e) {
    startupError.value = true
    logger.error('Failed to load startup settings on retry', { error: e })
  }
}

function sendFrontendHeartbeat(): void {
  // 心跳失败意味着后端已不可达或页面正在销毁；不能在这里反复刷用户可见错误。
  void invokeIpc(IPC.FRONTEND_HEARTBEAT).catch(() => {})
}

onMounted(async () => {
  // 关闭监听和心跳必须在首次 await 前装配。启动配置 IPC 卡住或失败时，任务栏关闭仍应可用。
  void listenAppEvent('window-close-requested', () => ui.requestAppClose())
  // 退出前 flush 协议同样必须在首次 await 前装配:启动 IPC 卡住时,退出仍应能请求落盘。
  void installSettingsLifecycle().catch(() => {})
  sendFrontendHeartbeat()
  frontendHeartbeatTimer = window.setInterval(sendFrontendHeartbeat, 1000)
  // 窗口重新可见/聚焦时立即补发心跳：Chromium 对隐藏页计时器节流(隐藏约 5 分钟后约 1 次/分)，
  // 节流期的心跳无法反映真实存活；回前台瞬间刷新，ask 关闭确认不会被陈旧心跳拖入原生直退。
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) sendFrontendHeartbeat()
  })
  window.addEventListener('focus', sendFrontendHeartbeat)

  // 仅初始化主题 — 数据加载在 AppSidebar.vue 的 onMounted 中处理

  // 启动初始化:设置快照与内部状态经 uiStore 的唯一一次 get_startup_config 取回(全应用共享
  // 同一 Promise)。设置值本身已由中央服务应用;此处只消费全局项(语言/字号/时间轴尺度)与
  // 内部状态(首启/引导标记)——这两个标记只在启动生效,快照刷新时不重放,故重置设置不会
  // 重放首启向导。
  try {
    const payload = await ui.startupConfigPromise
    if (payload) {
      applyStartupPayload(payload)
    } else {
      // 读取失败:启动层已经解除,但**设置就绪门控**仍为假 → 主内容不渲染(见模板),用户看到
      // 的是可重试的初始化错误提示,而不是未受保护的界面。
      startupError.value = true
      logger.error('Failed to load startup settings')
    }
  } finally {
    // 关键启动配置流程到此结束;实际放开主内容由 settingsReady 决定(见模板 v-if)。
    dismissStartupLayer()
  }

  // AI 状态在窗口显示后再获取（不影响启动速度），并自动续传被中断（崩溃/强退/暂停）且仍有
  // 剩余的分析，使断点续传能跨程序重启（问题7）。
  ai.maybeAutoResume().catch(() => {})
  // 人脸自动续传与 AI 并列触发——若两者都「期望运行且有剩余」，后端 GPU 分析门闩（单一持有者）
  // 将其串行化：先占到槽的运行，另一个进「资源等待」队列（P1-1）——按稳定 code 分流、有界重试，
  // 对端释放会话即自动续跑，不报错也不需重启。互斥由后端保证，而非此处的顺序。
  face.maybeAutoResume().catch(() => {})

  // 缩略图生成进度恢复(2026-07-18):注册进度事件监听 + 查后端快照回填——webview 刷新时
  // 后台生成照跑,不恢复则进度条永远消失(Channel 传输随旧 webview 一起死,根因见 scanStore)。
  void useScanStore().restoreThumbGenProgress()

  // 目录移动半完成清单（P0-1）：启动读一次，让「文件真实位置 + 仍需收尾」在刷新/重启后照常
  // 出现。只读清单，不触发任何物理动作——收尾必须由用户在管理区点重试。
  void useDirectoryMoveRecoveryStore().load()
})

onBeforeUnmount(() => {
  if (frontendHeartbeatTimer !== undefined) {
    window.clearInterval(frontendHeartbeatTimer)
    frontendHeartbeatTimer = undefined
  }
})
</script>

<style scoped>
.settings-gate {
  display: grid;
  place-items: center;
  height: 100vh;
  background-color: var(--color-bg-primary);
  color: var(--color-text-primary);
}

.settings-gate__panel {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-sm);
  max-width: 420px;
  padding: var(--spacing-lg);
  text-align: center;
}

.settings-gate__title {
  margin: 0;
  font-size: var(--font-size-lg);
}

.settings-gate__hint {
  margin: 0;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  line-height: 1.5;
}

.titlebar-viewer-title {
  min-width: 0;
  max-width: min(36vw, 420px);
  overflow: hidden;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
  pointer-events: none;
}

/* 分离模式第二条 bar 在**非画廊页**的内容:只一条页面标题(画廊控件是画廊态,那里无意义)。
   与 AppToolbar 的 .toolbar__title 同款排版——两处各自 scoped、无法共享选择器,故对齐值而非提取
   (只 4 个 token 引用,抽公共类的耦合成本高于重复)。 */
.toolbar-page-title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
}

/* 沉浸模式标题栏 hover 显隐:host 出流(fixed)使内容占满全屏,translateY 收起在视口外;
   贴顶滑入。非沉浸态无类,零影响正常布局。 */
.titlebar-host--immersive {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  /* 高于内容/浮动退出钮,低于 toast(--z-toast:300);沉浸期无画廊弹层/backdrop 之虞 */
  z-index: var(--z-overlay);
  transform: translateY(-100%);
  transition: transform var(--duration-normal) var(--ease-out);
  /* 收起时自身在视口外,无需 pointer-events 处理;滑入后正常可交互 */
  box-shadow: var(--shadow-lg);
}
.titlebar-host--immersive.titlebar-host--revealed {
  transform: translateY(0);
}

/* `/view/:id` 仍是路由，呈现上却是画廊内容区的覆盖层：底下 MediaGrid 从未失活，返回即露出
   已有帧。AppShell 的 .app-content 是 position:relative，故不覆盖侧栏/标题栏/底栏。 */
.content-viewer-route-overlay {
  position: absolute;
  inset: 0;
  /* 高于画廊自身的悬停/时间轴层，低于 AppShell 的查看器侧栏开关(--z-overlay)。 */
  z-index: calc(var(--z-overlay) - 1);
}
</style>
