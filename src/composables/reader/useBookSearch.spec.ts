// src/composables/reader/useBookSearch.spec.ts
// characterization:searchGen 非响应式代际丢弃 + closeSearch 早退判据(方案 §5 风险面 2)。
// searchGen 是模块内闭包 let(源文件头注释红线:不可改 ref),故只能经行为侧面断言——
// 用可控的 async iterable 模拟 foliate 迭代器,验证「换查询/换书」丢弃旧迭代产出。
import { describe, it, expect, vi } from 'vitest'
import { ref } from 'vue'
import type { ReaderApi } from './readerApiTypes'

import { useBookSearch } from './useBookSearch'

// 手动可控的 async generator:外部逐步 push 值,onSearch 内的 for-await 逐个消费。
function deferredIter<T>() {
  const queue: T[] = []
  const waiters: ((v: IteratorResult<T>) => void)[] = []
  return {
    push(v: T) {
      const w = waiters.shift()
      if (w) w({ value: v, done: false })
      else queue.push(v)
    },
    [Symbol.asyncIterator]() {
      return {
        next(): Promise<IteratorResult<T>> {
          if (queue.length) return Promise.resolve({ value: queue.shift()!, done: false })
          return new Promise((resolve) => waiters.push(resolve))
        },
      }
    },
  }
}

describe('useBookSearch:searchGen 代际丢弃 + closeSearch 早退(characterization)', () => {
  it('换查询后旧迭代产出被丢弃:代次不符时 gen!==searchGen 早退,不写入 searchResults', async () => {
    const clearSearch = vi.fn()
    const readerRef = ref<ReaderApi | null>({
      clearSearch,
      searchBook: () => undefined,
    } as unknown as ReaderApi)
    const closeOtherPanels = vi.fn()
    const { onSearch, searchResults } = useBookSearch({ readerRef, closeOtherPanels })

    const iter1 = deferredIter<{ subitems: { cfi: string; excerpt: string }[]; label: string }>()
    ;(readerRef.value as unknown as { searchBook: () => unknown }).searchBook = () => iter1

    const p1 = onSearch('foo') // gen=1,挂起在 for-await 首次 next()

    // 换查询:第二次 onSearch 递增 searchGen,令上面挂起的迭代作废。
    const iter2 = deferredIter<{ subitems: { cfi: string; excerpt: string }[]; label: string }>()
    ;(readerRef.value as unknown as { searchBook: () => unknown }).searchBook = () => iter2
    const p2 = onSearch('bar') // gen=2

    // 先喂旧迭代一条结果,再喂新迭代一条结果。
    iter1.push({ subitems: [{ cfi: '#1', excerpt: 'old' }], label: 'old-chapter' })
    iter2.push('done' as unknown as { subitems: never[]; label: string })

    await Promise.all([p1, p2])

    // 旧迭代的结果被 gen!==searchGen 早退丢弃,不应出现在 searchResults。
    expect(searchResults.value.some((s) => s.label === 'old-chapter')).toBe(false)
  })

  it('closeSearch 早退判据:全空闲态(未显示/未搜索/无结果)调用不触发 clearSearch', () => {
    const clearSearch = vi.fn()
    const readerRef = ref<ReaderApi | null>({ clearSearch } as unknown as ReaderApi)
    const closeOtherPanels = vi.fn()
    const { closeSearch } = useBookSearch({ readerRef, closeOtherPanels })

    closeSearch()

    expect(clearSearch).not.toHaveBeenCalled()
  })

  it('closeSearch 非空闲态(showSearch=true)：清状态并调用 clearSearch', () => {
    const clearSearch = vi.fn()
    const readerRef = ref<ReaderApi | null>({ clearSearch } as unknown as ReaderApi)
    const closeOtherPanels = vi.fn()
    const { toggleSearch, closeSearch, showSearch } = useBookSearch({ readerRef, closeOtherPanels })

    toggleSearch() // showSearch=true,触发 closeOtherPanels
    expect(closeOtherPanels).toHaveBeenCalledTimes(1)
    expect(showSearch.value).toBe(true)

    closeSearch()

    expect(showSearch.value).toBe(false)
    expect(clearSearch).toHaveBeenCalledTimes(1)
  })

  it('toggleSearch 在已展开时走 closeSearch 分支,不重复调用 closeOtherPanels', () => {
    const clearSearch = vi.fn()
    const readerRef = ref<ReaderApi | null>({ clearSearch } as unknown as ReaderApi)
    const closeOtherPanels = vi.fn()
    const { toggleSearch, showSearch } = useBookSearch({ readerRef, closeOtherPanels })

    toggleSearch() // 打开
    toggleSearch() // 关闭:走 closeSearch,不应再调 closeOtherPanels

    expect(showSearch.value).toBe(false)
    expect(closeOtherPanels).toHaveBeenCalledTimes(1)
  })

  it('reset 不经 closeSearch,不触发 clearSearch(与原 load() 内联复位一致)', () => {
    const clearSearch = vi.fn()
    const readerRef = ref<ReaderApi | null>({ clearSearch } as unknown as ReaderApi)
    const closeOtherPanels = vi.fn()
    const { toggleSearch, reset, showSearch } = useBookSearch({ readerRef, closeOtherPanels })

    toggleSearch()
    reset()

    expect(showSearch.value).toBe(false)
    expect(clearSearch).not.toHaveBeenCalled()
  })
})
