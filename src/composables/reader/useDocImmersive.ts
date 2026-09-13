// src/composables/reader/useDocImmersive.ts
// 沉浸态代理 + Esc 分层退出（结构拆分自 DocumentViewer.vue §S16 + §S25）。
// window keydown 监听器的挂载/卸载仍由 root 的 onMounted/onBeforeUnmount 注册（本 composable
// 只导出 onGlobalKeydown 处理函数），保持与原文件完全一致的注册时机（红线，§3 风险 6：
// 不能提前到某个子 composable 的更早期生命周期钩子，避免影响与 useFullscreenExitGuard 等其它
// window 级 Escape 监听器的相对顺序）。
import { computed, type ComputedRef } from 'vue'
import type { useViewerStore } from '../../stores/viewerStore'

export interface UseDocImmersiveDeps {
  viewer: ReturnType<typeof useViewerStore>
  anyPanelOpen: ComputedRef<boolean>
  closeAllPanels: () => void
  goBack: () => void
}

export function useDocImmersive(deps: UseDocImmersiveDeps) {
  // 沉浸模式（R4 + P5 统一）：隐藏工具栏专注阅读，Esc / 浮动按钮退出。
  // P5: 并入 viewerStore.immersive——可写 computed 代理：所有既有用点（v-show/v-if/= true/= false）
  // 零改动透传到 store（setImmersive 仅在 activeViewer 存在时生效，enterImmersive 均在文档
  // populate 后触发，安全）。换文档 load() 复位同旧。
  const immersive = computed<boolean>({
    get: () => deps.viewer.isImmersive,
    set: (v) => deps.viewer.setImmersive(v),
  })

  // 沉浸模式（R4）：进入即隐藏工具栏并收起所有侧栏（避免残留面板遮挡专注阅读）。
  function enterImmersive() {
    immersive.value = true
    deps.closeAllPanels()
  }

  // Esc 分层退出（2026-07-16 补齐 MediaGrid 的不变量）。四层由**外向内**逐层剥，与 ContentViewer
  // 同款语义：
  //   ① 全屏 → useFullscreenExitGuard 在 window 捕获期先手（本函数收不到，即「全屏优先于关阅读器」）；
  //   ② 沉浸 → 退沉浸；
  //   ③ 面板（搜索/目录/书签/校对…）开着 → 先关面板（必须由本函数显式做，无面板自持 Escape）；
  //   ④ 常规 → 退出阅读器回来处。
  function onGlobalKeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return
    if (immersive.value) {
      immersive.value = false
      return
    }
    if (deps.anyPanelOpen.value) {
      deps.closeAllPanels()
      return
    }
    deps.goBack()
  }

  return { immersive, enterImmersive, onGlobalKeydown }
}
