// src/composables/reader/useBookSearch.ts
// 书内搜索。
// 搜索代次仅用于异步有效性校验，无需响应式状态。
import { ref, shallowRef, watch, type Ref } from 'vue'
import type { ReaderApi } from './readerApiTypes'
import type { SearchSection } from '../../components/doc/SearchPanel.vue'

export interface UseBookSearchDeps {
  readerRef: Ref<ReaderApi | null>
  /** 打开搜索前收起其余互斥面板（root 装配层回填，实参引用不复制）。 */
  closeOtherPanels: () => void
}

export function useBookSearch(deps: UseBookSearchDeps) {
  const showSearch = ref(false)
  const searchResults = shallowRef<SearchSection[]>([])
  const searching = ref(false)
  const searchProgress = ref(0)
  const searchFailed = ref(false)
  // 代次令牌：换查询/换书时递增，令旧的异步迭代产出作废（防串档进新查询结果）。
  let searchGen = 0

  function toggleSearch() {
    if (showSearch.value) {
      closeSearch()
      return
    }
    showSearch.value = true
    deps.closeOtherPanels()
  }

  // 迭代 BookReader 暴露的 foliate 原生 search 生成器，逐章流式收集命中喂给面板。
  // 代次令牌确保换查询/换书后旧迭代产出被丢弃；命中由 foliate 在渲染视图内自动高亮。
  async function onSearch(query: string) {
    const gen = ++searchGen
    deps.readerRef.value?.clearSearch?.()
    searchResults.value = []
    searchProgress.value = 0
    searchFailed.value = false
    const q = query.trim()
    if (!q) {
      searching.value = false
      return
    }
    searching.value = true
    try {
      const iter = deps.readerRef.value?.searchBook?.(q)
      if (!iter) return
      for await (const r of iter) {
        if (gen !== searchGen) return // 已被更新的搜索 / 换书作废
        if (r === 'done') break
        if ('progress' in r) {
          searchProgress.value = r.progress
        } else if (r.subitems.length) {
          searchResults.value = [...searchResults.value, { label: r.label, matches: r.subitems }]
        }
      }
    } catch {
      // 章节读取/解析可能失败，保留已有命中，但不能把不完整搜索报成无结果或完成。
      if (gen === searchGen) searchFailed.value = true
    } finally {
      if (gen === searchGen) searching.value = false
    }
  }
  function onSearchNavigate(cfi: string) {
    deps.readerRef.value?.goToHref?.(cfi) // foliate goTo 对 CFI 与 href 均可解析
  }
  // 关闭搜索：作废进行中迭代 + 清高亮 + 清状态（无可清则早退，避免多余 clearSearch）。
  function closeSearch() {
    if (!showSearch.value && !searching.value && !searchResults.value.length && !searchFailed.value) return
    showSearch.value = false
    searchGen++
    searching.value = false
    searchResults.value = []
    searchProgress.value = 0
    searchFailed.value = false
    deps.readerRef.value?.clearSearch?.()
  }
  /** 换文档时复位状态并作废旧搜索，不调用旧渲染器的 clearSearch。 */
  function reset() {
    showSearch.value = false
    searchResults.value = []
    searching.value = false
    searchProgress.value = 0
    searchFailed.value = false
    searchGen++
  }

  // 同文档切版本也会重建阅读器；旧 CFI 与在途搜索不能沿用到新的正文实例。
  watch(deps.readerRef, reset, { flush: 'sync' })

  return {
    showSearch,
    searchResults,
    searching,
    searchProgress,
    searchFailed,
    toggleSearch,
    onSearch,
    onSearchNavigate,
    closeSearch,
    reset,
  }
}
