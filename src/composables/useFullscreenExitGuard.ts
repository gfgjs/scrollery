// src/composables/useFullscreenExitGuard.ts
// 浏览器风格的「按住 Esc 退出全屏」守卫。
//
// 原生（Tauri）全屏下单击 Esc 会立即退出（WebView2 的默认全屏行为），对桌面应用而言很意外。
// 我们在 window 捕获阶段拦截 Esc，使其先于其它 Esc 消费者运行：若有弹层/选择模式，则让行
// （Esc 执行常规行为）；否则抑制原生退出，要求用户按住 Esc 达 HOLD_MS，并在顶部居中显示
// 提示——与 Chrome / Edge 一致。

import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useMediaStore } from '../stores/mediaStore'
import { useViewerStore } from '../stores/viewerStore'
import { isFullscreen, setFullscreen } from './useWindowMode'
import { useSelection } from './useSelection'

/// Esc 需按住多久才真正退出全屏。
const HOLD_MS = 1000

export function useFullscreenExitGuard() {
  const media = useMediaStore()
  const viewer = useViewerStore()
  const selection = useSelection()
  const route = useRoute()

  const hintVisible = ref(false)
  const holding = ref(false)
  const holdMs = HOLD_MS

  let holdTimer: ReturnType<typeof setTimeout> | null = null
  let hintHideTimer: ReturnType<typeof setTimeout> | null = null

  // 其它 Esc 消费者优先——详情浮层、设置、激活的选择、以及任何打开的对话框。
  // 这些情况下 Esc 执行常规行为，而非全屏的「按住退出」。
  function shouldDeferEsc(): boolean {
    if (media.isDetailOpen) return true
    // 查看器沉浸态:Esc 的职责是「退沉浸」(ContentViewer/DocumentViewer 各自监听)。图/视因写
    // mediaStore 已被上一行覆盖;文档查看器用局部 detail 不经 mediaStore,若不让行,本守卫在
    // 捕获阶段 stopPropagation 会吞掉「全屏 + 文档沉浸」下的退沉浸 Esc(2026-07-10 深审 MED-3)。
    if (viewer.isImmersive) return true
    if (route.path === '/settings') return true
    if (selection.isSelectionMode.value) return true
    if (document.querySelector('.dialog-overlay')) return true
    return false
  }

  function showHint() {
    hintVisible.value = true
    if (hintHideTimer !== null) {
      clearTimeout(hintHideTimer)
      hintHideTimer = null
    }
  }

  function scheduleHintHide(delay = 1200) {
    if (hintHideTimer !== null) clearTimeout(hintHideTimer)
    hintHideTimer = setTimeout(() => {
      hintVisible.value = false
      hintHideTimer = null
    }, delay)
  }

  function cancelHold() {
    if (holdTimer !== null) {
      clearTimeout(holdTimer)
      holdTimer = null
    }
    holding.value = false
  }

  function onKeydownCapture(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !isFullscreen.value) return
    if (shouldDeferEsc()) return
    // 抑制原生的单击退出；改为要求按住。
    e.preventDefault()
    e.stopPropagation()
    if (holdTimer !== null) return // 按住期间的自动重复
    holding.value = true
    showHint()
    holdTimer = setTimeout(() => {
      holdTimer = null
      holding.value = false
      hintVisible.value = false
      // 确认按住 → 退出。用 setFullscreen(false) 而非 toggle:本守卫的语义只有「退出」一个方向,
      // toggle 在态与 OS 短暂不一致时可能反把窗口推进全屏。
      void setFullscreen(false)
    }, HOLD_MS)
  }

  function onKeyupCapture(e: KeyboardEvent) {
    if (e.key !== 'Escape') return
    if (holdTimer === null && !holding.value) return
    // 在阈值前松开——取消退出，让提示短暂停留以便看清。
    cancelHold()
    scheduleHintHide()
  }

  // 若全屏经其它途径结束（F11、工具栏按钮、系统），清掉提示/按住状态。
  watch(
    isFullscreen,
    (fs) => {
      if (!fs) {
        cancelHold()
        hintVisible.value = false
        if (hintHideTimer !== null) {
          clearTimeout(hintHideTimer)
          hintHideTimer = null
        }
      }
    },
  )

  onMounted(() => {
    window.addEventListener('keydown', onKeydownCapture, true)
    window.addEventListener('keyup', onKeyupCapture, true)
  })

  onBeforeUnmount(() => {
    window.removeEventListener('keydown', onKeydownCapture, true)
    window.removeEventListener('keyup', onKeyupCapture, true)
    cancelHold()
    if (hintHideTimer !== null) clearTimeout(hintHideTimer)
  })

  return { hintVisible, holding, holdMs }
}
