// src/composables/useSidebarResize.ts
// 侧边栏拖拽调整大小 (§14.2)

import { ref, onBeforeUnmount } from 'vue'
import { useUiStore } from '../stores/uiStore'

export function useSidebarResize() {
  const ui = useUiStore()
  const isResizing = ref(false)
  let startX = 0
  let startW = 0

  function onMouseDown(e: MouseEvent) {
    isResizing.value = true
    startX = e.clientX
    startW = ui.sidebarWidth
    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup', onMouseUp)
    document.body.style.cursor = 'ew-resize'
    document.body.style.userSelect = 'none'
  }

  function onMouseMove(e: MouseEvent) {
    if (!isResizing.value) return
    const delta = e.clientX - startX
    ui.setSidebarWidth(startW + delta)
  }

  function onMouseUp() {
    isResizing.value = false
    document.removeEventListener('mousemove', onMouseMove)
    document.removeEventListener('mouseup', onMouseUp)
    document.body.style.cursor = ''
    document.body.style.userSelect = ''
    ui.persistSidebarWidth()
  }

  onBeforeUnmount(() => {
    document.removeEventListener('mousemove', onMouseMove)
    document.removeEventListener('mouseup', onMouseUp)
  })

  return { isResizing, onMouseDown }
}
