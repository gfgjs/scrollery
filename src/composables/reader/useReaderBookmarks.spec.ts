import { beforeEach, expect, it, vi } from 'vitest'
import { effectScope, ref, shallowRef } from 'vue'
import { IPC } from '../../constants/ipc'
import { useReaderBookmarks } from './useReaderBookmarks'
import type { ReaderApi } from './readerApiTypes'
import type { ReaderBookmark } from '../../types/reader'

const { invoke, addToast } = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  addToast: vi.fn(),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc: invoke, ipcErrorMessage: () => '读取失败' }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

function bookmark(id: number): ReaderBookmark {
  return { id, locator: `cfi:${id}`, label: `第 ${id} 章`, fraction: id / 10, createdAt: 1 }
}

function setup() {
  const id = ref(41)
  const reader: ReaderApi = {
    next() {}, prev() {}, getScrollEl: () => null,
    getCurrentLocation: () => ({ locator: 'cfi:2', label: '第 2 章', fraction: 0.2 }),
    goToLocator: vi.fn(),
  }
  const scope = effectScope()
  const bookmarks = scope.run(() => useReaderBookmarks({
    id, readerRef: shallowRef(reader), closeOtherPanels: vi.fn(),
  }))!
  return { id, reader, scope, ...bookmarks }
}

beforeEach(() => vi.resetAllMocks())

it('新增成功后的列表读取失败保留已有书签并提示，重新打开可恢复完整列表和导航', async () => {
  const original = bookmark(1)
  const added = bookmark(2)
  invoke.mockResolvedValueOnce([original]).mockResolvedValueOnce(2).mockRejectedValueOnce(new Error('read failed'))
  const state = setup()
  try {
    await state.loadBookmarks()
    state.showBookmarks.value = true
    await state.onAddBookmark()
    expect(invoke).toHaveBeenCalledWith(IPC.ADD_READER_BOOKMARK, {
      itemId: 41, locator: added.locator, label: added.label, fraction: added.fraction,
    })
    expect(state.bookmarks.value).toEqual([original])
    expect(addToast).toHaveBeenCalledExactlyOnceWith('error', 'doc.openFailed')

    state.toggleBookmarks()
    invoke.mockResolvedValueOnce([original, added])
    state.toggleBookmarks()
    await vi.waitFor(() => expect(state.bookmarks.value).toEqual([original, added]))
    state.onBookmarkNavigate(added.locator)
    expect(state.reader.goToLocator).toHaveBeenCalledExactlyOnceWith(added.locator)
    expect(state.showBookmarks.value).toBe(false)
  } finally {
    state.scope.stop()
  }
})

it('删除成功后刷新失败仍移除已删条目并保留其余书签', async () => {
  const removed = bookmark(1)
  const remaining = bookmark(2)
  invoke.mockResolvedValueOnce([removed, remaining]).mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('read failed'))
  const state = setup()
  try {
    await state.loadBookmarks()
    await state.onDeleteBookmark(removed.id)
    expect(invoke).toHaveBeenCalledWith(IPC.DELETE_READER_BOOKMARK, { id: removed.id })
    expect(state.bookmarks.value).toEqual([remaining])
    expect(addToast).toHaveBeenCalledExactlyOnceWith('error', 'doc.openFailed')
  } finally {
    state.scope.stop()
  }
})

it('切书后旧列表读取失败不会清空新书签或弹出旧错误', async () => {
  let failOld!: (error: Error) => void
  const oldRead = new Promise<ReaderBookmark[]>((_, reject) => { failOld = reject })
  invoke.mockReturnValueOnce(oldRead).mockResolvedValueOnce([bookmark(3)])
  const state = setup()
  try {
    const loadingOld = state.loadBookmarks()
    state.id.value = 42
    await state.loadBookmarks()
    failOld(new Error('old read failed'))
    await loadingOld
    expect(state.bookmarks.value).toEqual([bookmark(3)])
    expect(invoke).toHaveBeenLastCalledWith(IPC.LIST_READER_BOOKMARKS, { itemId: 42 })
    expect(addToast).not.toHaveBeenCalled()
  } finally {
    state.scope.stop()
  }
})
