// src/composables/reader/useReaderBookmarks.ts
// 书签（结构拆分自 DocumentViewer.vue §S15 + §S23）。
import { ref, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'
import type { ReaderBookmark } from '../../types/reader'
import type { ReaderApi } from './readerApiTypes'

export interface UseReaderBookmarksDeps {
  id: Ref<number>
  readerRef: Ref<ReaderApi | null>
  /** 打开书签面板前收起其余互斥面板（root 装配层回填，实参引用不复制）。 */
  closeOtherPanels: () => void
}

export function useReaderBookmarks(deps: UseReaderBookmarksDeps) {
  const showBookmarks = ref(false)
  const bookmarks = ref<ReaderBookmark[]>([])
  const toast = useToastStore()
  const { t } = useI18n()

  // 打开面板即加载列表；增/删后刷新。定位复用 goToLocator（剥 cfi: 前缀交 foliate）。
  async function loadBookmarks() {
    try {
      bookmarks.value = await invokeIpc<ReaderBookmark[]>(IPC.LIST_READER_BOOKMARKS, {
        itemId: deps.id.value,
      })
    } catch {
      bookmarks.value = []
    }
  }
  function toggleBookmarks() {
    if (showBookmarks.value) {
      showBookmarks.value = false
      return
    }
    showBookmarks.value = true
    deps.closeOtherPanels()
    loadBookmarks()
  }
  async function onAddBookmark() {
    const loc = deps.readerRef.value?.getCurrentLocation?.()
    if (!loc) {
      // 首次 relocate 前无位置可存（极少见，开卷即触发）。
      toast.addToast('info', t('doc.bookmarkNoPosition'))
      return
    }
    try {
      await invokeIpc(IPC.ADD_READER_BOOKMARK, {
        itemId: deps.id.value,
        locator: loc.locator,
        label: loc.label,
        fraction: loc.fraction,
      })
      await loadBookmarks()
    } catch (e) {
      toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
    }
  }
  function onBookmarkNavigate(locator: string) {
    deps.readerRef.value?.goToLocator?.(locator)
    showBookmarks.value = false
  }
  async function onDeleteBookmark(bmId: number) {
    try {
      await invokeIpc(IPC.DELETE_READER_BOOKMARK, { id: bmId })
      await loadBookmarks()
    } catch (e) {
      toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
    }
  }
  /** 换文档时调用（与原 load() 内联复位一致）。 */
  function reset() {
    showBookmarks.value = false
    bookmarks.value = []
  }

  return {
    showBookmarks,
    bookmarks,
    loadBookmarks,
    toggleBookmarks,
    onAddBookmark,
    onBookmarkNavigate,
    onDeleteBookmark,
    reset,
  }
}
