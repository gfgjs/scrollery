// src/composables/reader/useReaderBookmarks.ts
// 书签。
import { onScopeDispose, ref, watch, type Ref } from 'vue'
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
  let documentGeneration = 0
  let loadGeneration = 0
  // 路由切走又返回时 id 相同，仍须使旧读取与增删回执失效。
  watch(deps.id, reset, { flush: 'sync' })
  onScopeDispose(reset)

  // 打开面板即加载列表；增/删后刷新。定位复用 goToLocator（剥 cfi: 前缀交 foliate）。
  async function loadBookmarks() {
    const document = documentGeneration
    const request = ++loadGeneration
    try {
      const result = await invokeIpc<ReaderBookmark[]>(IPC.LIST_READER_BOOKMARKS, {
        itemId: deps.id.value,
      })
      if (document === documentGeneration && request === loadGeneration) bookmarks.value = result
    } catch (e) {
      // 读取失败不等于没有书签；保留已知列表，避免增删后的刷新错误把其余书签隐藏。
      if (document === documentGeneration && request === loadGeneration) {
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
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
    const generation = documentGeneration
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
      if (generation === documentGeneration) await loadBookmarks()
    } catch (e) {
      if (generation === documentGeneration) {
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
    }
  }
  function onBookmarkNavigate(locator: string) {
    deps.readerRef.value?.goToLocator?.(locator)
    showBookmarks.value = false
  }
  async function onDeleteBookmark(bmId: number) {
    const generation = documentGeneration
    try {
      await invokeIpc(IPC.DELETE_READER_BOOKMARK, { id: bmId })
      if (generation === documentGeneration) {
        // 删除已被后端确认，即使随后读取失败，也不能继续展示该条目。
        bookmarks.value = bookmarks.value.filter(bookmark => bookmark.id !== bmId)
        await loadBookmarks()
      }
    } catch (e) {
      if (generation === documentGeneration) {
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
    }
  }
  /** 换文档时关闭面板并清空旧书签。 */
  function reset() {
    documentGeneration++
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
