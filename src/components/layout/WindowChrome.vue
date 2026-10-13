<template>
  <!-- 自绘标题栏由 useWindowDrag 委托处理拖窗；交互项越过阈值才拖动，表单控件与窗口三键排除。
       mac 使用系统窗口按钮，其他平台显示自绘三键。 -->
  <div ref="rootRef" class="window-chrome theme-shell-surface" :class="{ 'is-mac': isMac }">
    <!-- 内容区空隙可立即拖窗；子项使用拖动阈值。mac 左侧留出系统窗口按钮空间。 -->
    <div class="window-chrome__content" data-window-drag-surface>
      <slot />
    </div>

    <!-- 非 mac 平台显示自绘窗口三键，mac 保留系统窗口按钮。
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

// useWindowMode 是窗口三态的唯一来源，并负责与系统窗口状态同步。
const controlsRef = ref<HTMLElement | null>(null)

// 标题栏根承载整行拖窗的委托监听。
// 全屏态自动惰性——闸在 useWindowDrag 内直读 useWindowMode.canDragWindow,本组件无需传参。
const rootRef = ref<HTMLElement | null>(null)
useWindowDrag(rootRef)

// 窗口操作失败不可静默:frameless 下三键是唯一关闭/最小化入口,权限回归时「点了没反应」
// 必须留诊断信号。
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
