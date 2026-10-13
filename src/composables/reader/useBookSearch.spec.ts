import { expect, it, vi } from 'vitest'
import { effectScope, shallowRef } from 'vue'
import { useBookSearch } from './useBookSearch'
import type { ReaderApi } from './readerApiTypes'
import type { FoliateSearchResult } from '../../vendor/foliate-js/view.js'

function result(cfi: string): FoliateSearchResult {
  return { label: cfi, subitems: [{ cfi, excerpt: { pre: '', match: '正文', post: '' } }] }
}

it.each(['completed', 'late-result', 'late-error'] as const)(
  '阅读器重建时清除旧版本的 %s 搜索，旧回执不再污染当前状态',
  async (phase) => {
    let finish!: () => void
    const pending = new Promise<void>(resolve => { finish = resolve })
    const oldReader: ReaderApi = {
      next() {}, prev() {}, getScrollEl: () => null,
      clearSearch: vi.fn(), goToHref: vi.fn(),
      async *searchBook(): AsyncGenerator<FoliateSearchResult, void, unknown> {
        yield { progress: 0.25 }
        yield result('old-version-cfi')
        if (phase !== 'completed') {
          await pending
          if (phase === 'late-error') throw new Error('old renderer closed')
          yield result('late-old-version-cfi')
        }
        yield 'done'
      },
    }
    const newReader: ReaderApi = {
      next() {}, prev() {}, getScrollEl: () => null,
      clearSearch: vi.fn(), goToHref: vi.fn(),
      async *searchBook(): AsyncGenerator<FoliateSearchResult, void, unknown> {
        yield result('new-version-cfi')
        yield 'done'
      },
    }
    const readerRef = shallowRef<ReaderApi | null>(oldReader)
    const scope = effectScope()
    const search = scope.run(() => useBookSearch({ readerRef, closeOtherPanels: vi.fn() }))!
    search.toggleSearch()
    const searching = search.onSearch('正文')
    try {
      await vi.waitFor(() => expect(search.searchResults.value).toHaveLength(1))
      expect(search.searching.value).toBe(phase !== 'completed')

      // VersionPanel 切换版本只重建 BookReader，不经过路由 load() 的 search.reset()。
      readerRef.value = null
      readerRef.value = newReader
      expect(search.searchResults.value).toEqual([])
      expect(search.showSearch.value).toBe(false)
      expect(search.searching.value).toBe(false)
      expect(search.searchProgress.value).toBe(0)
      expect(oldReader.clearSearch).toHaveBeenCalledTimes(1)
      expect(newReader.clearSearch).not.toHaveBeenCalled()

      finish()
      await searching
      expect(search.searchResults.value).toEqual([])
      expect(search.searchFailed.value).toBe(false)
      expect(search.searchProgress.value).toBe(0)

      search.toggleSearch()
      await search.onSearch('正文')
      expect(search.searchResults.value[0]?.matches[0]?.cfi).toBe('new-version-cfi')
      search.onSearchNavigate('new-version-cfi')
      expect(newReader.goToHref).toHaveBeenCalledExactlyOnceWith('new-version-cfi')
      expect(oldReader.goToHref).not.toHaveBeenCalled()
    } finally {
      finish()
      await searching
      scope.stop()
    }
  },
)
