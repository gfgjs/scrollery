import { expect, it, vi } from 'vitest'
import { createRenderer, h, nextTick, shallowRef, ssrContextKey } from 'vue'
import SearchPanel from './SearchPanel.vue'
import { useBookSearch } from '../../composables/reader/useBookSearch'
import type { ReaderApi } from '../../composables/reader/readerApiTypes'
import type { FoliateSearchResult } from '../../vendor/foliate-js/view.js'

vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@tanstack/vue-virtual', async () => {
  const { shallowRef } = await import('vue')
  return {
    defaultRangeExtractor: () => [],
    useVirtualizer: () => shallowRef({ scrollToOffset: vi.fn(), measure: vi.fn() }),
  }
})

// 挂载真实面板状态与搜索事件，虚拟列表测量不参与本次清空输入的行为验证。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

it.each(['completed', 'pending'] as const)('清空输入立即清除 %s 搜索，迟到结果不会重新出现', async (phase) => {
  let finish!: () => void
  const pending = new Promise<void>(resolve => { finish = resolve })
  const reader: ReaderApi = {
    next() {}, prev() {}, getScrollEl: () => null,
    clearSearch: vi.fn(),
    async *searchBook(): AsyncGenerator<FoliateSearchResult, void, unknown> {
      yield { progress: 0.5 }
      yield { label: '第一章', subitems: [{ cfi: 'first', excerpt: { pre: '', match: '书', post: '' } }] }
      if (phase === 'pending') await pending
      yield { label: '第二章', subitems: [{ cfi: 'late', excerpt: { pre: '', match: '书', post: '' } }] }
      yield 'done'
    },
  }
  const search = useBookSearch({ readerRef: shallowRef(reader), closeOtherPanels: vi.fn() })
  search.toggleSearch()
  const requests: Promise<void>[] = []
  const onSearch = vi.fn((query: string) => { requests.push(search.onSearch(query)) })
  const panel = shallowRef<InstanceType<typeof SearchPanel> | null>(null)
  const component = Object.assign({}, SearchPanel, { render: () => null })
  const app = renderer.createApp({
    setup: () => () => h(component, {
      ref: panel,
      results: search.searchResults.value,
      searching: search.searching.value,
      progress: search.searchProgress.value,
      failed: search.searchFailed.value,
      onSearch,
    }),
  })
  app.provide(ssrContextKey, {})
  app.mount({})
  const state = (panel.value!.$ as unknown as {
    setupState: { q: string; submitted: boolean; submit(event: KeyboardEvent): void }
  }).setupState
  try {
    state.q = '书'
    await nextTick()
    // 非空输入仍由回车提交，不能变成逐键全书搜索。
    expect(onSearch).not.toHaveBeenCalled()
    state.submit({ isComposing: false } as KeyboardEvent)
    await vi.waitFor(() => expect(search.searchResults.value).toHaveLength(phase === 'pending' ? 1 : 2))
    expect(search.searching.value).toBe(phase === 'pending')

    // 浏览器清除按钮或删空文字都会通过 v-model 将查询置空，无需再次回车。
    state.q = ''
    await nextTick()
    expect(onSearch).toHaveBeenLastCalledWith('')
    expect(state.submitted).toBe(false)
    expect(search.searchResults.value).toEqual([])
    expect(search.searching.value).toBe(false)
    expect(search.searchProgress.value).toBe(0)
    expect(reader.clearSearch).toHaveBeenCalledTimes(2)
    expect(search.showSearch.value).toBe(true)

    finish()
    await Promise.all(requests)
    expect(search.searchResults.value).toEqual([])
    expect(search.searchFailed.value).toBe(false)
  } finally {
    finish()
    await Promise.all(requests)
    app.unmount()
  }
})
