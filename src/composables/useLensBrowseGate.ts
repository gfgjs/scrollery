// src/composables/useLensBrowseGate.ts
// 重复镜头 browse-only gate 的宿主侧接线（2026-09-02 方案 §8.1/§13 P2）。
//
// 🔴 §8.1 明文（勿凭直觉放松）：「MVP 是明确的浏览态：进入镜头立即清空普通画廊选区，并隐藏选择
// 工具栏、checkbox、卡片收藏/评分快捷动作、拖拽和右键菜单」「Ctrl/Cmd、Shift 与 Ctrl/Cmd+A 在
// 镜头中不进入选择，也不触发批量操作」「镜头不自动选中整组、整文件夹或所谓副本」。
//
// 各阻断点落点（谓词统一单源 duplicateLensStore.isLensActive，勿各处重写 `mode !== null`）：
//  - 选区清空：本 composable 的 lensActive watcher（MediaGrid 挂载）；
//  - SelectionToolbar：SelectionToolbar.vue 两形态显隐条件（forceHidden prop,组件保持挂载）；
//  - checkbox（DOM/Canvas）：MediaGrid 传给 MediaGridRow/MediaGridCanvas 的 selection-mode prop；
//  - 收藏/评分快捷动作 + 拖拽手柄：MediaThumb 的 browseOnly prop（MediaGridRow/悬停卡透传）；
//  - 框选/拖图起手：useMediaDragToFolder.onCardPointerDown（lensActive dep）；
//  - 右键菜单：useGalleryContextMenu.onContextMenu（lensActive dep）；
//  - Ctrl/Shift 单击：cardPointerAction.resolveCardClickAction（穷举单测）；
//  - Ctrl+A/ESC 等 document 级选择语义：useGalleryKeyboard.onKeyDown 尾部旁路（lensActive dep）。
import { computed, watch } from 'vue'
import { useDuplicateLensStore } from '../stores/duplicateLensStore'
import { useSelection } from './useSelection'

/**
 * 重复镜头 browse-only gate。必须在画廊宿主（MediaGrid）setup 中调用——watcher 随宿主作用域销毁。
 * 返回的 lensActive 与 duplicateLensStore.isLensActive 同源，供模板各显隐点消费。
 */
export function useLensBrowseGate() {
  const lens = useDuplicateLensStore()
  const selection = useSelection()

  const lensActive = computed(() => lens.isLensActive)

  // §8.1：进入镜头立即清空普通画廊选区（useSelection 是模块级单例,直调 clearSelection）。
  // immediate：深链直开镜头 / KeepAlive 失活期经 URL 同步进入镜头时,宿主挂载即处于镜头态,
  // 非 immediate watcher 不会触发;对空选区 clear 是幂等 no-op。
  watch(
    lensActive,
    (active) => {
      if (active) selection.clearSelection()
    },
    { immediate: true },
  )

  return { lensActive }
}
