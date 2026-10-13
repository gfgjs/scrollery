// src/composables/reader/useDocImmersive.ts
// 沉浸态代理 + Esc 分层退出。
// root 统一挂载/卸载 window keydown，保持与全屏退出等 Escape 监听器的处理顺序。
import { computed, type ComputedRef } from 'vue'
import type { useViewerStore } from '../../stores/viewerStore'

export interface UseDocImmersiveDeps {
  viewer: ReturnType<typeof useViewerStore>
  anyPanelOpen: ComputedRef<boolean>
  closeAllPanels: () => void
  goBack: () => void
}

export function useDocImmersive(deps: UseDocImmersiveDeps) {
  // 沉浸状态由 viewerStore 统一持有，文档就绪后才可进入。
  const immersive = computed<boolean>({
    get: () => deps.viewer.isImmersive,
    set: (v) => deps.viewer.setImmersive(v),
  })

  // 进入沉浸时收起侧栏，避免残留面板遮挡正文。
  function enterImmersive() {
    immersive.value = true
    deps.closeAllPanels()
  }

  // Esc 顺序与 ContentViewer 一致：
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
