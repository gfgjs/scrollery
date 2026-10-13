// src/composables/reader/useReaderNav.ts
// 导航/TOC。
import { ref, type Ref } from 'vue'
import type { FoliateTocItem } from '../../vendor/foliate-js/view.js'
import type { ReaderApi } from './readerApiTypes'

export interface UseReaderNavDeps {
  readerRef: Ref<ReaderApi | null>
  /** 打开 TOC 前收起其余互斥面板（root 装配层回填，实参引用不复制）。 */
  closeOtherPanels: () => void
}

export function useReaderNav(deps: UseReaderNavDeps) {
  // foliate 渲染器的全书进度为 0..1。
  const readerFraction = ref<number | null>(null)
  const readerTocLabel = ref('')
  // 当前章 href（TOC 面板高亮当前章用；由 BookReader locate 事件透传 foliate 的 tocItem.href）。
  const readerTocHref = ref('')
  const readerToc = ref<FoliateTocItem[]>([])
  const showToc = ref(false)

  function onLocate(v: { fraction: number; tocLabel: string; tocHref: string }) {
    readerFraction.value = v.fraction
    readerTocLabel.value = v.tocLabel
    readerTocHref.value = v.tocHref
  }
  function onToc(toc: FoliateTocItem[]) {
    readerToc.value = toc
  }
  function onTocNavigate(href: string) {
    deps.readerRef.value?.goToHref?.(href)
  }
  function toggleToc() {
    showToc.value = !showToc.value
    if (showToc.value) deps.closeOtherPanels()
  }
  /** 换文档时复位目录数据，保留面板开合状态。 */
  function reset() {
    readerFraction.value = null
    readerTocLabel.value = ''
    readerTocHref.value = ''
    readerToc.value = []
  }

  return {
    readerFraction,
    readerTocLabel,
    readerTocHref,
    readerToc,
    showToc,
    onLocate,
    onToc,
    onTocNavigate,
    toggleToc,
    reset,
  }
}
