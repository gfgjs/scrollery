// 搜索状态协调门面：普通搜索由 uiStore 持有，语义搜索由 aiStore 持有。
//
// 本 store 不做物理存储合并——committedQuery/mode/scope 投影自 ui/ai 原字段，commit*
// 委托 aiStore 的 action 保留查询代次守卫及临时分组/排序协调逻辑。
// 对外提供：①可序列化读表面（mode/scope/committedQuery，供写入 URL query）；②保证执行的
// 写表面（apply/clear/commit*，供 URL 回填时真正触发查询）。
// draftMixedQuery 在此共享。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { useUiStore } from './uiStore'
import { useAiStore } from './aiStore'
import type { SearchMode } from '../types/ai'

export const useSearchStore = defineStore('search', () => {
  // 混合模式草稿：用户在 AI / 文件名两候选间未决时的输入文本。提升自 AppToolbar.pendingMixedQuery，
  // 使其成为可被 URL 层读写的单源，而非组件局部 ref。
  const draftMixedQuery = ref('')

  // ── 读表面（单一事实源，供序列化到 URL）──────────────────────────────────
  /** 当前搜索模式（投影自 aiStore.searchMode）。 */
  const mode = computed<SearchMode>(() => useAiStore().searchMode)

  /** 文字搜索的检索范围（投影自 uiStore.searchScope）；混合模式文件名候选提交时固定为 filename。 */
  const scope = computed<string>(() => useUiStore().searchScope)

  /**
   * 当前已提交的有效查询串，按模式归一（semantic→语义查询；mixed→由
   * activeMixedQueryType 决定取语义还是文件名查询；normal→文件名查询）。
   * mixed 模式下 draftMixedQuery（未决草稿）不计入 committedQuery。
   */
  const committedQuery = computed<string>(() => {
    const ai = useAiStore()
    if (ai.searchMode === 'semantic') return ai.semanticQuery
    if (ai.searchMode === 'mixed') {
      return ai.activeMixedQueryType === 'semantic' ? ai.semanticQuery : useUiStore().searchQuery
    }
    return useUiStore().searchQuery
  })

  /** 是否存在非空的已提交查询（供 empty-state / URL 序列化判空）。 */
  const hasCommittedQuery = computed(() => committedQuery.value.trim() !== '')

  // ── 写表面（保证执行）─────────────────────────────────────────────────
  /** 切换搜索模式；进入 mixed 时清空草稿。委托 aiStore.setSearchMode（含 group/sort 协调与重算）。 */
  function setMode(next: SearchMode) {
    useAiStore().setSearchMode(next)
    if (next === 'mixed') draftMixedQuery.value = ''
  }

  /** 设置检索范围（供 URL 回填）。写入 uiStore.searchScope，触发 useJustifiedLayout 重算。 */
  function setScope(next: string) {
    useUiStore().searchScope = next
  }

  /** 普通 / 文件名搜索提交：写 uiStore.searchQuery（useJustifiedLayout 的 watch 立即重算并查询）。 */
  function commitNormal(query: string) {
    useUiStore().searchQuery = query
  }

  /** 语义搜索提交：委托 aiStore.runSemanticSearch（含 searchToken 代次守卫与 group/sort 覆盖）。 */
  function commitSemantic(query: string) {
    void useAiStore().runSemanticSearch(query)
  }

  /**
   * 混合模式提交：index 0 = AI 语义候选，1 = 文件名候选。使用给定 query，缺省取当前草稿。
   * 空串走 setNormalSearchQueryInMixedMode('') 以复位混合子类型与 group/sort（与原
   * AppToolbar.triggerMixedSearch 空串分支同义）。
   */
  function commitMixed(index: 0 | 1, query = draftMixedQuery.value) {
    const ai = useAiStore()
    draftMixedQuery.value = query
    if (!query.trim()) {
      ai.setNormalSearchQueryInMixedMode('')
      return
    }
    if (index === 0) void ai.runSemanticSearch(query)
    else {
      // 混合模式明确选择“文件名”，不能沿用普通模式隐藏的目录、位置等范围。
      useUiStore().searchScope = 'filename'
      ai.setNormalSearchQueryInMixedMode(query)
    }
  }

  /**
   * 统一应用查询（供 URL 回填等非交互路径）：按当前模式路由到对应执行。
   * mixed 模式默认走 index 0（AI 语义）；normal / semantic 忽略 mixedIndex。
   */
  function apply(query: string, opts: { mixedIndex?: 0 | 1 } = {}) {
    const ai = useAiStore()
    if (ai.searchMode === 'semantic') commitSemantic(query)
    else if (ai.searchMode === 'mixed') commitMixed(opts.mixedIndex ?? 0, query)
    else commitNormal(query)
  }

  /** 清空当前模式下的搜索并复位草稿。 */
  function clear() {
    const ai = useAiStore()
    draftMixedQuery.value = ''
    if (ai.searchMode === 'semantic') void ai.runSemanticSearch('')
    else if (ai.searchMode === 'mixed') ai.setNormalSearchQueryInMixedMode('')
    else useUiStore().searchQuery = ''
  }

  return {
    // state
    draftMixedQuery,
    // read surface
    mode,
    scope,
    committedQuery,
    hasCommittedQuery,
    // write surface
    setMode,
    setScope,
    commitNormal,
    commitSemantic,
    commitMixed,
    apply,
    clear,
  }
})
