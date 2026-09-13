// src/composables/useFolderTreeKeyboardNav.ts
// 文件树键盘导航的副作用层——从 FoldersSection.vue 域 5 迁出(见
// docs/planning/2026-07-25-超长文件拆分方案/analysis/FoldersSection-vue.md §2.1)。
// 决策在 treeKeyTarget 纯函数(已单测);此处只做副作用:移 active 行(即时滚入视口,虚拟化
// 窗口随之渲染该行)、toggle 展开、按行类分发激活。
import { ref, watch, type ComputedRef, type Ref, type ShallowRef } from 'vue'
import {
  treeScrollTopForIndex,
  treeKeyTarget,
} from '../components/sidebar/sections/folderTree.helpers'
import type { DirNode, DirFile } from '../types/media'
import type { TreeRow } from '../components/sidebar/sections/folderTree.helpers'

export interface FolderTreeKeyboardNavDeps {
  displayRows: ComputedRef<TreeRow[]>
  treeRef: Ref<HTMLElement | null>
  scrollAreaEl: ShallowRef<HTMLElement | null>
  rowH: Ref<number>
  stackTopPx: ComputedRef<number>
  stackBottomPx: ComputedRef<number>
  treeOffsetTop: () => number
  toggleNode: (node: DirNode) => void | Promise<void>
  onNodeClick: (node: DirNode, idx?: number) => void
  onFileClick: (file: DirFile, idx?: number) => void
  onFileDblClick: (file: DirFile) => void
  onLoadMore: (dirKey: string, idx?: number) => void | Promise<void>
  isOpenableInApp: (file: DirFile) => boolean
  isTextPreviewable: (fileName: string) => boolean
}

export function useFolderTreeKeyboardNav(deps: FolderTreeKeyboardNavDeps) {
  const {
    displayRows,
    treeRef,
    scrollAreaEl,
    rowH,
    stackTopPx,
    stackBottomPx,
    treeOffsetTop,
    toggleNode,
    onNodeClick,
    onFileClick,
    onFileDblClick,
    onLoadMore,
    isOpenableInApp,
    isTextPreviewable,
  } = deps

  const activeIndex = ref(-1)
  // 行集变化(折叠/重载)后钳位,防 active 行下标越界指向不存在的行。
  watch(displayRows, (rows) => {
    if (activeIndex.value >= rows.length) activeIndex.value = rows.length - 1
  })
  function setActiveIndex(i: number) {
    activeIndex.value = i
    scrollRowIntoView(i)
  }
  // 键盘步进即时滚动(不用 smooth:按键自动重复率下平滑动画追不上);insets 同 scrollTreeToDirId
  // ——上=粘顶标题堆叠+滚动到位后将钉住的祖先链,下=粘底标题堆叠,否则行停在不透明浮层底下。
  function scrollRowIntoView(idx: number) {
    if (!scrollAreaEl.value) return
    const row = displayRows.value[idx]
    if (!row) return
    const depth = row.kind === 'dir' ? row.node.depth : row.depth
    const chainRows = Math.min(depth, 6)
    const target = treeScrollTopForIndex(
      idx,
      treeOffsetTop(),
      scrollAreaEl.value.scrollTop,
      Math.min(scrollAreaEl.value.clientHeight, window.innerHeight),
      rowH.value,
      stackTopPx.value + chainRows * rowH.value,
      stackBottomPx.value,
    )
    if (target !== scrollAreaEl.value.scrollTop) scrollAreaEl.value.scrollTop = target
  }
  // Tab 落焦且尚无 active 行 → 落首行(APG:初次聚焦 focus 第一项)。
  function onTreeFocus() {
    if (activeIndex.value < 0 && displayRows.value.length) setActiveIndex(0)
  }
  function onTreeKeydown(e: KeyboardEvent) {
    const action = treeKeyTarget(displayRows.value, activeIndex.value, e.key)
    if (!action) return
    e.preventDefault() // 消费掉的键不再滚动侧栏
    // 鼠标点行后焦点在行内 button(tabindex=-1 可点击聚焦),键盘继续操作时把真实焦点收回
    // 容器——:focus-visible 行环依赖容器持焦;keydown 中程序化 focus 会被 UA 判为键盘交互。
    if (document.activeElement !== treeRef.value) treeRef.value?.focus({ preventScroll: true })
    if (action.kind === 'move') {
      setActiveIndex(action.index)
    } else if (action.kind === 'toggle') {
      void toggleNode(action.node)
    } else {
      const row = displayRows.value[activeIndex.value]
      if (!row) return
      if (row.kind === 'dir') onNodeClick(row.node)
      else if (row.kind === 'file') {
        onFileClick(row.file)
        // 键盘可达性(问题②方案 B):预览若只挂双击,键盘用户够不着——Enter 对「不可打开
        // 但可预览」的文件同双击弹预览。reveal 不接 Enter:回车突然弹文件管理器过于突兀,
        // 且 fileTitle 只对预览承诺了双击语义。
        if (!isOpenableInApp(row.file) && isTextPreviewable(row.file.fileName)) {
          onFileDblClick(row.file)
        }
      } else void onLoadMore(row.dirKey)
    }
  }

  return { activeIndex, setActiveIndex, onTreeFocus, onTreeKeydown }
}
