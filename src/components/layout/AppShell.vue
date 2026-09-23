<template>
  <!-- data-theme 单源在 documentElement(uiStore.applyAppearance 唯一写点);此处
       不得再绑一份——双源曾导致 system 模式规则不匹配(Part5 F1)与主题切换脱同步。 -->
  <div
    class="app-shell"
    :class="{
      'app-shell--maximized': isMaximized,
      'app-shell--flat': viewerRoute || viewer.isImmersive || isFullscreen,
    }"
  >
    <!-- 自绘标题栏：只承载品牌、窗口三键、拖拽区与当前视图的 navigation 命令。 -->
    <slot name="titlebar" />

    <!-- Body: sidebar + main (row below the titlebar) -->
    <!-- 主体:侧栏 + 主区(标题栏下方的行) -->
    <div class="app-body">
      <!-- Sidebar -->
      <!-- 侧边栏 -->
      <!-- 收起走 margin-left 平滑滑出(而非 v-show 瞬隐):元素常挂载(保 AppSidebar 树展开/滚动/已载
           数据不丢),负 margin 把整栏拉出视口左侧、由 .app-body overflow:hidden 裁掉,内容区 flex:1 补位。
           内容恒宽(width 不动、只动 margin)故不逐帧重排。收起态挂 inert,把视口外的树/导航
           移出焦点序(同 titlebar-host 沉浸收起手法)。 -->
      <aside
        ref="sidebarRef"
        class="app-sidebar theme-shell-surface"
        :style="{
          width: ui.sidebarWidth + 'px',
          marginLeft: sidebarVisible ? '0' : `-${ui.sidebarWidth}px`,
        }"
        :inert="!sidebarVisible"
        @transitionend="onSidebarTransitionEnd"
      >
        <slot name="sidebar" />
        <!-- Drag handle -->
        <!-- 拖拽手柄 -->
        <div
          class="sidebar-resize-handle"
          @mousedown="resizer.onMouseDown"
          :class="{ resizing: resizer.isResizing.value }"
        />
      </aside>

      <!-- 查看器内容优先:默认收起侧栏(viewerSidebarVisible=false);用户点击即固化,翻页/切图不复位
           (状态在 uiStore,不再随 route.path 复位——修「切下一张图侧栏自动收起」)。
           本浮动钮是给**没有 chrome 可放**的全幅表面(大图 /view/)兜底的;自持开关的视图(阅读页)
           不再渲染它——否则它会与那些视图工具栏左上角的控件几何重叠(见 mediaRoute.viewerHostsSidebarToggle)。 -->
      <button
        v-if="viewerRoute && !viewerHostsSidebarToggle(route.path) && !viewer.isImmersive"
        type="button"
        class="viewer-sidebar-toggle"
        :class="{ 'is-open': ui.viewerSidebarVisible }"
        :style="{ left: (ui.viewerSidebarVisible ? ui.sidebarWidth + 12 : 12) + 'px' }"
        :title="ui.viewerSidebarVisible ? $t('sidebar.hideSidebar') : $t('sidebar.showSidebar')"

        @click="ui.toggleViewerSidebar()"
      >
        <PanelLeftClose v-if="ui.viewerSidebarVisible" :size="17" />
        <PanelLeftOpen v-else :size="17" />
      </button>

      <!-- Main area -->
      <!-- 主区域 -->
      <div class="app-main">
        <!-- 页面级工具栏：承载 Gallery 搜索、筛选与视图控制。
             **本槽只由分离模式提供**(合并模式下 AppToolbar 已并入标题栏，App.vue 不给此槽)，故
             「这条存在」⟺「用户选了分离」——分离模式的契约就是「画廊控件独立成一条**恒在**的轨道」。
             据此(2026-07-16 用户裁决)沉浸态**不隐本条**：F11 只隐标题栏与底栏，本条常驻不 inert。
             唤出的标题栏是 fixed、会铺在视口顶 40px 上，故本条在其唤出期整体让位下移(CSS 详)。 -->
        <header
          v-if="$slots.toolbar && !settingsRoute"
          class="app-toolbar"
          data-chrome-top
          :class="{
            'app-toolbar--immersive': chromeAutoHidden,
            'app-toolbar--pushed': chromeAutoHidden && topRevealed,
          }"
          @focusout="onToolbarFocusOut"
        >
          <slot name="toolbar" />
        </header>

        <main class="app-content">
          <slot />
        </main>

        <!-- 底栏。设置工作区不承载全局图库状态，因此整条底栏不占位；离开设置后由路由计算恢复。
             其余情况下沉浸态(查看器沉浸 ∪ F11 全屏)自动隐藏 + 贴底唤出——与顶栏对称
             (2026-07-16 用户新需求「无顶栏+无底栏，仅鼠标移入才显示顶栏底栏」)。
             inert：收起态它虽 translateY 出视口，其控件(如扫描中的「停止生成缩略图」)仍在 Tab 序，
             键盘用户会 Tab 进一条看不见的栏；与标题栏同款处理，F10 唤出即解除。 -->
        <footer
          v-if="!settingsRoute"
          class="app-statusbar"
          data-chrome-bottom
          :class="{
            'app-statusbar--immersive': chromeAutoHidden,
            'app-statusbar--revealed': chromeAutoHidden && bottomRevealed,
          }"
          :inert="chromeAutoHidden && !bottomRevealed"
          @focusout="onStatusbarFocusOut"
        >
          <slot name="statusbar" />
        </footer>
      </div>
    </div>

    <!-- Hold-Esc-to-exit-fullscreen hint (browser-style) | 按住 Esc 退出全屏提示（浏览器风格） -->
    <Teleport to="body">
      <Transition name="fs-hint">
        <div v-if="fsHintVisible" class="fs-exit-hint">
          <span class="fs-exit-hint__text">{{ $t('toolbar.holdEscToExit') }}</span>
          <div class="fs-exit-hint__track">
            <div
              class="fs-exit-hint__bar"
              :style="{
                width: fsHolding ? '100%' : '0%',
                transitionDuration: fsHolding ? fsHoldMs + 'ms' : '120ms',
              }"
            />
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { PanelLeftClose, PanelLeftOpen } from '@lucide/vue'
import { useUiStore } from '../../stores/uiStore'
import { useViewerStore } from '../../stores/viewerStore'
import { useSidebarResize } from '../../composables/useSidebarResize'
import { useFullscreenExitGuard } from '../../composables/useFullscreenExitGuard'
import { initWindowMode, toggleFullscreen, isFullscreen, isMaximized } from '../../composables/useWindowMode'
import {
  chromeAutoHidden,
  topRevealed,
  bottomRevealed,
  collapseTop,
  collapseBottom,
  shouldCollapseOnFocusOut,
} from '../../composables/useChromeReveal'
import { isViewerRoute, viewerHostsSidebarToggle } from '../../utils/mediaRoute'
import { isGalleryRoute } from '../../utils/galleryQuery'
import { dispatchKeybinding } from '../../commands/keybinding'
import { buildCommandContext } from '../../commands/context'

const ui = useUiStore()
// 沉浸模式(顶栏重构 P4-c):图片查看器 setImmersive → 隐 chrome(本组件侧栏/工具栏/状态栏 v-show;
// 标题栏在 App.vue 对 WindowChrome v-show),只留内容区全屏图。activeViewer 卸载即自然退出。
const viewer = useViewerStore()
const resizer = useSidebarResize()
const route = useRoute()
const viewerRoute = computed(() => isViewerRoute(route.path))
// 设置路由使用独立工作区 chrome；统一按路径前缀覆盖所有设置子路由，避免全局图库 chrome 串入。
const settingsRoute = computed(() => route.path.startsWith('/settings'))
const sidebarRef = ref<HTMLElement | null>(null)

// 侧栏是否可见:沉浸态全隐;否则查看器路由看 viewerSidebarVisible、画廊路由看 gallerySidebarVisible
// (二者会话级、默认不同,见 uiStore)。设置工作区也收起全局图库侧栏；返回其它路由时自动恢复原会话态。
// 收起时经负 margin 滑出而非 v-show 瞬隐(模板注释详)。
const sidebarVisible = computed(
  () =>
    !settingsRoute.value &&
    !viewer.isImmersive &&
    (viewerRoute.value ? ui.viewerSidebarVisible : ui.gallerySidebarVisible),
)

// 查看器返回画廊时，侧栏的 margin-left 可能正处于切换两种会话态的动画中。画廊据共享
// generation 冻结几何采样；transitionend 是主收尾，300ms 仅兜住无动画/被浏览器打断的情况。
const ROUTE_RETURN_SIDEBAR_FALLBACK_MS = 300
let routeReturnSidebarGeneration: number | null = null
let routeReturnSidebarTimer: ReturnType<typeof setTimeout> | null = null

function clearRouteReturnSidebarTimer(): void {
  if (routeReturnSidebarTimer === null) return
  clearTimeout(routeReturnSidebarTimer)
  routeReturnSidebarTimer = null
}

function finishRouteReturnSidebarTransition(generation: number): void {
  if (routeReturnSidebarGeneration !== generation) return
  clearRouteReturnSidebarTimer()
  ui.finishRouteReturnSidebarTransition(generation)
  routeReturnSidebarGeneration = null
}

function beginRouteReturnSidebarTransition(): void {
  clearRouteReturnSidebarTimer()
  const generation = ui.beginRouteReturnSidebarTransition()
  routeReturnSidebarGeneration = generation
  routeReturnSidebarTimer = setTimeout(
    () => finishRouteReturnSidebarTransition(generation),
    ROUTE_RETURN_SIDEBAR_FALLBACK_MS,
  )
}

function isCurrentSidebarTransition(event: TransitionEvent): boolean {
  const sidebar = sidebarRef.value
  if (event.target !== sidebar || event.propertyName !== 'margin-left' || !sidebar) return false
  // 用户可能在旧动画尚未结束时就关闭查看器；只有 margin 已落到当前路由要求的终点，
  // 该 transitionend 才属于本轮 route-return。其余情况交给新的 transitionend 或 fallback。
  const actual = Number.parseFloat(getComputedStyle(sidebar).marginLeft)
  const expected = sidebarVisible.value ? 0 : -ui.sidebarWidth
  return Number.isFinite(actual) && Math.abs(actual - expected) <= 1
}

function onSidebarTransitionEnd(event: TransitionEvent): void {
  if (!isCurrentSidebarTransition(event) || routeReturnSidebarGeneration === null) return
  finishRouteReturnSidebarTransition(routeReturnSidebarGeneration)
}

watch(
  () => route.path,
  (toPath, fromPath) => {
    if (fromPath && isViewerRoute(fromPath) && isGalleryRoute(toPath)) {
      beginRouteReturnSidebarTransition()
    }
  },
  { flush: 'sync' },
)

// 浏览器风格的「按住 Esc 退出全屏」守卫（问题4）：单击 Esc 不再退出；按住时下方提示进度条
// 填充，填满才退出。
const {
  hintVisible: fsHintVisible,
  holding: fsHolding,
  holdMs: fsHoldMs,
} = useFullscreenExitGuard()

// 沉浸态自动隐藏的两条 chrome:页面工具栏(分离模式的顶栏下半)与底栏。
// 指针的唤出**与收起**都在 useChromeReveal 内按几何判据统一处理(window 级 capture,App.vue 装配一次),
// 此处只绑焦点路径 —— 键盘 Tab 穿出时收起。判据只需 relatedTarget + data-chrome-* 标记,不需要宿主 ref。
function onToolbarFocusOut(e: FocusEvent) {
  if (shouldCollapseOnFocusOut('top', e.relatedTarget as Node | null)) collapseTop()
}
function onStatusbarFocusOut(e: FocusEvent) {
  if (shouldCollapseOnFocusOut('bottom', e.relatedTarget as Node | null)) collapseBottom()
}

function onKeyDown(e: KeyboardEvent) {
  // 网格态下 AppToolbar 的注册表分发(document 层,先于本 window 层收到冒泡)已处理并 preventDefault,
  // 此处让行防双执行。
  if (e.defaultPrevented) return

  // ① 注册表兜底分发：AppToolbar 仅在 isGalleryRoute 为真时挂载，故**合集/人物总览页、设置页没有它**
  //    ——那里 mod+z 收不到任何分发（真机 round10 #7：删收藏夹恰恰发生在总览页）。
  //    仅在**无查看器**时兜底：查看器内的键位(Escape/方向键/i 等)由 ContentViewer 自持，
  //    在此重复分发会双执行（见 viewer-image.ts 头注：keybinding 目前是元数据，实际监听在查看器内）。
  //    无查看器 → ctx.view==='grid' → 只有 grid.undo/redo/toggleFullscreen 的 when 成立，面收敛。
  const tgt = e.target as HTMLElement | null
  const tag = tgt?.tagName
  const inEditable =
    tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tgt?.isContentEditable === true
  // 输入态不拦：文本框内 mod+z 应是文本撤销（与 AppToolbar 的分发守卫同源）。
  if (!viewer.activeViewer && !inEditable && dispatchKeybinding(e, buildCommandContext())) {
    e.preventDefault()
    return
  }

  // ② F11 裸键兜底：查看器内 ctx.view==='viewer'，grid.toggleFullscreen 的 when 不成立、注册表不匹配，
  //    故仍需这条(键位与其 keybinding 'F11' 同字面，P5-6)。
  //    e.repeat 让行:按住 F11 时自动重复 ~30ms 一发、而窗口转换约 ~100ms，不挡则全屏/退出来回抽搐。
  //    (useWindowMode 的转换闸只挡**转换进行中**的重入，两次转换之间的重复键它挡不住——故此闸不可省。
  //     注册表路径①的同一策略由 grid.toggleFullscreen 的 ignoreKeyRepeat 在 dispatchKeybinding 内落实。)
  if (e.key === 'F11' && !e.repeat) {
    e.preventDefault()
    void toggleFullscreen()
  }

  // ③ mod+W 关窗(#12):与标题栏 ✕ 同语义,走 closeBehavior(托盘隐藏/退出/询问)而非
  //    直接 EXIT_APP——尊重「最小化到托盘」偏好。刻意独立于①的守卫:查看器内、输入框内
  //    也生效(桌面惯例 mod+W 是窗口指令非文本键,WebView2 无默认行为,无冲突)。
  //    ESC 兜底隐藏未做(留决策):ESC 已被查看器/全屏守卫/对话框/选区/backBar 五处强消费,
  //    全局兜底极易误触,方案留档 docs/planning/2026-07-17-批量15项问题清单。
  if (
    (e.ctrlKey || e.metaKey) &&
    !e.shiftKey &&
    !e.altKey &&
    !e.repeat &&
    e.key.toLowerCase() === 'w'
  ) {
    e.preventDefault()
    void ui.requestAppClose()
  }
}

onMounted(() => {
  // 窗口三态(normal/maximized/fullscreen)的 OS 回同步在此装配一次(全应用唯一装配点)。
  void initWindowMode()
  window.addEventListener('keydown', onKeyDown)
})

onBeforeUnmount(() => {
  clearRouteReturnSidebarTimer()
  if (routeReturnSidebarGeneration !== null) {
    ui.finishRouteReturnSidebarTransition(routeReturnSidebarGeneration)
    routeReturnSidebarGeneration = null
  }
  window.removeEventListener('keydown', onKeyDown)
})
</script>

<style scoped>
.app-shell {
  display: flex;
  /* 列向:自绘标题栏(titlebar 槽)置顶全宽,下方 .app-body 承载 侧栏|主区 行(顶栏重构 L1) */
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
  background-color: var(--color-shell-bg-primary, var(--color-bg-primary));
  color: var(--color-text-primary);
}

/* 标题栏下方的主体行:侧栏 + 主区。min-height:0 让内嵌虚拟滚动区正确 overflow(flex 列子项
   默认 min-height:auto 会撑破)。 */
.app-body {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.viewer-sidebar-toggle {
  position: absolute;
  top: 8px;
  z-index: var(--z-overlay);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  border: none;
  border-radius: var(--radius-sm);
  /* 静息态刻意低调(用户裁:看图台上此钮不得喧宾夺主):无边框无投影、底色减淡、
     整体半透明,hover/键盘聚焦才显形——同 immersive-exit 的「悬停显现」语义,
     但保持 token 化随主题(非 S5 恒深色豁免面)。34px 触达目标不缩,只降视觉重量。 */
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  opacity: 0.55;
  border: 1px solid var(--color-border-strong);
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  /* left 与侧栏 margin-left 同用 --transition-normal → 侧栏滑动时浮动钮同步跟随、不抢跑;
     hover 变色仍走 fast。 */
  transition:
    left var(--transition-normal),
    opacity var(--transition-fast),
    background-color var(--transition-fast),
    color var(--transition-fast);
}
.viewer-sidebar-toggle:hover,
.viewer-sidebar-toggle:focus-visible {
  opacity: 1;
  color: var(--color-text-primary);
  background: var(--color-bg-elevated);
}

.app-sidebar {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 180px;
  max-width: 400px;
  background-color: var(--color-bg-secondary);
  /* chrome 材质叠层:仅「宣」等有纸纹的主题非 none;画布区永不消费此 token */
  background-image: none;
  border-right: 1px solid var(--color-border);
  overflow: hidden;
  flex-shrink: 0;
  /* 收起动画:只过渡 margin-left(内容恒宽不重排);拖拽调宽改的是 width、无过渡故实时跟手。
     与查看器浮动开关钮的 left 同用 --transition-normal → 钮与侧栏同步移动不抢跑。 */
  transition: margin-left var(--transition-normal);
}

.sidebar-resize-handle {
  position: absolute;
  top: 0;
  right: 0;
  width: 4px;
  height: 100%;
  cursor: ew-resize;
  z-index: 10;
  background: transparent;
  transition: background var(--transition-fast);
}
.sidebar-resize-handle:hover,
.sidebar-resize-handle.resizing {
  background: var(--color-accent);
}

.app-main {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
  min-width: 0;
  /* 沉浸态下 app-toolbar / app-statusbar 出流叠加,以本元素为包含块 —— 从而 left:0/right:0 天然
     避开侧栏(本元素起点即侧栏右缘),不像 fixed 那样会铺到侧栏底下。overflow:hidden(上一行,原有)
     顺带把收起态平移出去的两条裁掉。 */
  position: relative;
  margin: var(--theme-main-inset-top) var(--theme-main-inset-right)
    var(--theme-main-inset-bottom) var(--theme-main-inset-left);
  border: var(--theme-main-border-width) solid var(--theme-main-border-color);
  border-radius: var(--theme-main-radius);
  box-shadow: var(--theme-main-shadow);
  background: transparent;
}

:global(html[data-visual-style]) .app-main {
  background: var(--color-bg-primary);
}

.app-shell--maximized:not(.app-shell--flat) {
  --theme-main-inset-top: 3px;
  --theme-main-inset-right: 4px;
  --theme-main-inset-bottom: 4px;
  --theme-main-inset-left: 3px;
}

@media (max-width: 760px) {
  .app-shell--maximized:not(.app-shell--flat) {
    --theme-main-inset-top: 0px;
    --theme-main-inset-right: 0px;
    --theme-main-inset-bottom: 0px;
    --theme-main-inset-left: 0px;
  }
}

.app-shell--flat {
  --theme-main-inset-top: 0px;
  --theme-main-inset-right: 0px;
  --theme-main-inset-bottom: 0px;
  --theme-main-inset-left: 0px;
  --theme-main-radius: 0px;
  --theme-main-border-width: 0px;
  --theme-main-shadow: none;
}

.app-toolbar {
  height: var(--toolbar-height);
  min-height: var(--toolbar-height);
  display: flex;
  align-items: center;
  padding: 0 var(--spacing-md);
  background-color: var(--color-bg-secondary);
  background-image: none;
  border-bottom: 1px solid var(--color-border);
  gap: var(--toolbar-gap);
  flex-shrink: 0;
}

.app-content {
  flex: 1;
  overflow: hidden;
  position: relative;
  display: flex;
  flex-direction: column;
}

.app-statusbar {
  height: var(--statusbar-height);
  min-height: var(--statusbar-height);
  display: flex;
  align-items: center;
  padding: 0 var(--spacing-md);
  background-color: var(--color-bg-secondary);
  background-image: none;
  border-top: 1px solid var(--color-border);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  flex-shrink: 0;
}

/* ── 沉浸态(查看器沉浸 ∪ F11 全屏)的底栏自动隐藏 ───────────────────────────────
   底栏**必须出流**(absolute),不能靠 height:0 / v-show 之类改变布局的手法:唤出是高频 hover 行为,
   若它改变 .app-content 的高度,画廊每次悬停边缘都要整表重算布局。出流后 .app-content 恒占满
   .app-main,唤出/收起只动 transform —— 零重排,合成器就能跑完。
   进出沉浸态本身确实会让 .app-content 长高/变矮一次并触发一次重算,但那一刻窗口尺寸本就在变
   (F11),重算不可避免也不额外。
   与标题栏(App.vue 的 titlebar-host--immersive)同款手法、同款时长。 */

/* ── 沉浸态下页面工具栏**常驻**(2026-07-16 用户裁决,见模板注释)────────────────────
   本条不出流、不隐藏,只做一件事:给唤出的标题栏**让位**。标题栏沉浸态是 fixed 铺在视口顶
   --titlebar-height 上(App.vue titlebar-host--immersive),会盖住本条上半,故其唤出期本条整体下移
   一个标题栏高,与它拼成 40+48 的两条栈——恰好等于 useChromeReveal.sideChromeExtent('top') 量到的
   收起边界(两条都带 data-chrome-top),故指针在栈内移动不会误收。
   下移用 transform 而非 margin/padding:唤出是高频 hover 行为,transform 只走合成器,不推动
   .app-content 重排(与底栏出流同一理由)。让位期本条压在内容上(z-index),内容不动;收起即复位。
   z-index/transition 挂在 --immersive 而非 --pushed:否则去 --pushed 类的瞬间 z-index 一并消失,
   本条会在 0.18s 回程动画里被后置兄弟 .app-content 盖住(两者都 position:relative,DOM 序决定层叠)。 */
.app-toolbar--immersive {
  position: relative;
  z-index: var(--z-overlay);
  transition: transform var(--duration-normal) var(--ease-out);
}
.app-toolbar--immersive.app-toolbar--pushed {
  transform: translateY(var(--titlebar-height));
  /* 让位期本条浮在画廊内容之上,用投影读出「这是压着内容的浮层」(与底栏唤出同款)。 */
  box-shadow: var(--shadow-sm);
}

.app-statusbar--immersive {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  z-index: var(--z-overlay);
  transform: translateY(100%);
  transition: transform var(--duration-normal) var(--ease-out);
  /* 阴影朝上(与顶栏朝下镜像),使唤出时能从画廊内容里读出这是一条浮层。 */
  box-shadow: var(--shadow-sm);
}
.app-statusbar--immersive.app-statusbar--revealed {
  transform: translateY(0);
}

/* ── Hold-Esc-to-exit-fullscreen hint ──────────────────────────────────────── */
/* ── 按住 Esc 退出全屏提示 ─────────────────────────────────────────────────── */
.fs-exit-hint {
  position: fixed;
  top: 24px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 99999;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 10px 18px;
  /* 硬编码豁免(S5,设计 §6.2):全屏 HUD 语义为「永远深色玻璃浮层」(视频播放器
     OSD 同款),不随主题——亮色主题下亮底 HUD 反而在全屏媒体上不可读。 */
  background: rgba(20, 20, 20, 0.82);
  color: #fff;
  border-radius: 12px;
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  pointer-events: none;
  user-select: none;
}
.fs-exit-hint__text {
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.2px;
  white-space: nowrap;
}
.fs-exit-hint__track {
  width: 160px;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.22);
  overflow: hidden;
}
.fs-exit-hint__bar {
  height: 100%;
  width: 0;
  border-radius: 2px;
  background: #fff;
  transition-property: width;
  transition-timing-function: linear;
}

.fs-hint-enter-active,
.fs-hint-leave-active {
  transition:
    opacity 160ms ease,
    transform 160ms ease;
}
.fs-hint-enter-from,
.fs-hint-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-8px);
}
</style>
