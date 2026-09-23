<template>
  <!-- 自绘标题栏(顶栏重构 L1)。承载「标题栏 = 工具栏容器」的物理层:整条为拖拽区。
       拖拽机制(2026-07-14 改)不再用 data-tauri-drag-region(它只作用于命中元素本身、按钮不在拖拽面上,
       且 mousedown 即拖无阈值→无法区分点击/拖动),改由 useWindowDrag 在本根上委托监听:按下越阈值才移窗,
       未越阈值即松手 → 按钮/控件照常响应点击。整行任意处(含按钮)可拖,原生表单控件(行高滑块/搜索框/
       下拉)与窗口三键(data-no-window-drag)排除。双击裸露标题面最大化/还原由 useWindowDrag 复刻。
       平台分叉:mac 用系统红绿灯(不渲染三键),非 mac 自绘三键。 -->
  <div ref="rootRef" class="window-chrome theme-shell-surface" :class="{ 'is-mac': isMac }">
    <!-- 内容区:P1 为空(仅拖拽);P3 换 ContextualToolbar。mac 左侧 padding 避让红绿灯。
         data-window-drag-surface:内容区本体(槽位内容之间的空隙/padding)是「纯空隙拖拽面」——命中它
         本体即 pointerdown 即时移窗(VSCode 级,useWindowDrag 路径 B);槽内后代按钮/可点 div 靠 matches
         判定天然落进阈值路径,不受影响(见 useWindowDrag 顶注)。 -->
    <div class="window-chrome__content" data-window-drag-surface>
      <slot />
    </div>

    <!-- 非 mac:自绘窗口三键(靠右)。mac 分支不渲染(保系统红绿灯,需求 8)。
         data-no-window-drag:三键区仅点击不参与移窗拖拽(用户裁决,合原生标题栏惯例)。 -->
    <div v-if="!isMac" ref="controlsRef" class="window-chrome__controls" data-no-window-drag>
      <button
        type="button"
        class="win-ctrl"

        :title="t('toolbar.windowMinimize')"
        @click="onMinimize"
      >
        <Minus :size="15" />
      </button>
      <button
        type="button"
        class="win-ctrl"

        :title="isMaximized ? t('toolbar.windowRestore') : t('toolbar.windowMaximize')"
        @click="onToggleMaximize"
      >
        <Copy v-if="isMaximized" :size="13" />
        <Square v-else :size="13" />
      </button>
      <button
        type="button"
        class="win-ctrl win-ctrl--close"

        :title="t('toolbar.windowClose')"
        @click="onClose"
      >
        <X :size="16" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { Minus, Square, Copy, X } from '@lucide/vue'
import { getAppWindow } from '../../utils/appWindow'
import { logger } from '../../utils/logger'
import { isMac } from '../../utils/platform'
import { useWindowDrag } from '../../composables/useWindowDrag'
import { isMaximized, toggleMaximize } from '../../composables/useWindowMode'

const { t } = useI18n()
const appWindow = getAppWindow()

// 最大化态(驱动最大化键图标 方块↔还原)不再由本组件自持:窗口三态的唯一所有者是 useWindowMode
// (2026-07-16)。此前这里是个局部 ref + 自己的 DOM resize 监听,与 uiStore.isFullscreen 互不知情
// ——「既全屏又最大化」于是可表示,真机四症状由此而来。OS 回同步现统一在 useWindowMode(initWindowMode)。
const controlsRef = ref<HTMLElement | null>(null)

// 标题栏根:承载「整行按住拖动移窗」的委托监听(替代 data-tauri-drag-region,见 template 顶注)。
// 全屏态自动惰性——闸在 useWindowDrag 内直读 useWindowMode.canDragWindow,本组件无需传参。
const rootRef = ref<HTMLElement | null>(null)
useWindowDrag(rootRef)

// 窗口操作失败不可静默:frameless 下三键是唯一关闭/最小化入口,权限回归时「点了没反应」
// 必须留诊断信号(2026-07-10 深审 LOW-3)。
function warnWindowOp(op: string) {
  return (err: unknown) => logger.warn(`[WindowChrome] ${op} failed`, { error: err })
}

function onMinimize() {
  appWindow.minimize().catch(warnWindowOp('minimize'))
}

function onToggleMaximize() {
  // 最大化键的显式点击 → 委托 useWindowMode(它保证三态互斥:全屏态下这一键语义为「还原」,
  // 会退出全屏并恢复入全屏前的态)。标题面双击的最大化/还原不走这里,由 useWindowDrag 手动判定
  // (2026-07-14 起弃 data-tauri-drag-region 改阈值委托拖拽后,首击的 startDragging 会打断
  // dblclick/detail 序列,故 Tauri 内建的 detail===2 路径已不可达),两处互不重叠、无双 toggle。
  void toggleMaximize()
}

function onClose() {
  // 复用现有自定义关闭流:close() 触发 CloseRequested → Rust on_window_event prevent_close +
  // emit window-close-requested → App.vue 按 closeBehavior(托盘/退出/确认弹窗)处理。与旧原生
  // 关闭键完全同流,无回归(见 lib.rs on_window_event)。
  appWindow.close().catch(warnWindowOp('close'))
}

// --titlebar-controls-inset:标题栏内容避让「控件占位宽」。写到 documentElement 覆写
// variables.css 的 0px 默认,供全局(ContextualToolbar / AppToolbar 溢出预算)读取。
function setInset(px: number) {
  document.documentElement.style.setProperty('--titlebar-controls-inset', `${px}px`)
}

let controlsObserver: ResizeObserver | null = null

onMounted(() => {
  if (isMac) {
    // 红绿灯避让区固定 78px(与 tauri.conf trafficLightPosition x:16 + 三灯宽度匹配,真机可调)。
    setInset(78)
    return
  }
  // 最大化态的 DOM resize 回同步已迁入 useWindowMode(initWindowMode,AppShell 装配一次):
  // 一个窗口一份真相,且顺带把「逐 resize 事件直调 IPC」收敛为 rAF 合并。
  // 观测三键实际宽度写 inset(随 DPI/字号自适应),而非硬编码宽度。
  if (controlsRef.value) {
    controlsObserver = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0
      if (w > 0) setInset(w)
    })
    controlsObserver.observe(controlsRef.value)
  }
})

onBeforeUnmount(() => {
  controlsObserver?.disconnect()
})
</script>

<style scoped>
.window-chrome {
  display: flex;
  align-items: stretch;
  height: var(--titlebar-height);
  min-height: var(--titlebar-height);
  background-color: var(--color-bg-secondary);
  /* chrome 材质叠层:仅「宣」等有纸纹的主题非 none;与 sidebar/toolbar 同款 */
  background-image: none;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  user-select: none;
  color: var(--color-text-primary);
}

.window-chrome__content {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  /* Phase G: 画廊/上下文工具栏并入后, 各根间留间距(边缘/三键留白 G3 细调) */
  gap: var(--spacing-sm);
  /* 非 mac:右侧三键由 flex 兄弟占位,内容不需右 padding;mac:左侧留红绿灯位(下条) */
}
.window-chrome.is-mac .window-chrome__content {
  padding-left: var(--titlebar-controls-inset);
}

.window-chrome__controls {
  display: flex;
  align-items: stretch;
  /* Phase G G3: 三键置顶且不透明——极窄窗内容若仍溢出, 藏其后而不盖住三键(非裁剪, 不影响下拉弹层) */
  position: relative;
  z-index: 2;
  background-color: var(--color-bg-secondary);
  background-image: none;
  flex-shrink: 0;
}

.win-ctrl {
  width: 46px; /* Windows 标准窗口控件宽 */
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}
.win-ctrl:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
.win-ctrl:focus-visible {
  outline: var(--focus-ring-width) solid var(--color-accent);
  outline-offset: calc(-1 * var(--focus-ring-width));
}
.win-ctrl--close:hover {
  /* Windows 关闭键红:系统语义色,硬编码不随主题(同 AppShell fs-hint HUD 豁免先例) */
  background: #e81123;
  color: #fff;
}
</style>
