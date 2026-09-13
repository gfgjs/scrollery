// src/composables/reader/useReaderPanels.spec.ts
// characterization:面板互斥总控(方案 §5 风险面 5)。此文件本身是薄编排(汇总 show* ref 到
// anyPanelOpen + closeAllPanels)——「开一关他」的实际互斥逻辑在装配层(DocumentViewer.vue 的
// closeOtherPanels 闭包，回填给各面板 composable),不在本文件内。故本 spec 断言本文件实际持有的
// 契约:anyPanelOpen 对 showSearch ∪ flags 求或、closeAllPanels 收拢全部 flags + 调用 closeSearch。
import { describe, it, expect, vi } from 'vitest'
import { ref } from 'vue'

import { useReaderPanels } from './useReaderPanels'

describe('useReaderPanels:anyPanelOpen 求或 + closeAllPanels 收拢(characterization)', () => {
  it('全部 flags 与 showSearch 均为 false 时 anyPanelOpen=false', () => {
    const flagA = ref(false)
    const flagB = ref(false)
    const showSearch = ref(false)
    const closeSearch = vi.fn()
    const { anyPanelOpen } = useReaderPanels([flagA, flagB], closeSearch, showSearch)
    expect(anyPanelOpen.value).toBe(false)
  })

  it('showSearch 单独 true 时 anyPanelOpen=true(showSearch 不入 flags 数组,单独参与判据)', () => {
    const flagA = ref(false)
    const showSearch = ref(true)
    const closeSearch = vi.fn()
    const { anyPanelOpen } = useReaderPanels([flagA], closeSearch, showSearch)
    expect(anyPanelOpen.value).toBe(true)
  })

  it('任一 flag 为 true(如书签面板)时 anyPanelOpen=true', () => {
    const flagBookmarks = ref(true)
    const flagToc = ref(false)
    const showSearch = ref(false)
    const closeSearch = vi.fn()
    const { anyPanelOpen } = useReaderPanels([flagBookmarks, flagToc], closeSearch, showSearch)
    expect(anyPanelOpen.value).toBe(true)
  })

  it('closeAllPanels:收拢全部 flags 为 false + 调用 closeSearch 一次(不直接改 showSearch)', () => {
    const flagBookmarks = ref(true)
    const flagToc = ref(true)
    const showSearch = ref(true)
    const closeSearch = vi.fn()
    const { closeAllPanels } = useReaderPanels([flagBookmarks, flagToc], closeSearch, showSearch)

    closeAllPanels()

    expect(flagBookmarks.value).toBe(false)
    expect(flagToc.value).toBe(false)
    expect(closeSearch).toHaveBeenCalledTimes(1)
    // showSearch 本身由 closeSearch 内部负责关闭(此处传入的是 mock,不会真的置 false)——
    // 断言 closeAllPanels 未绕过 closeSearch 直接摸 showSearch.value。
    expect(showSearch.value).toBe(true)
  })

  it('closeAllPanels 对空 flags 数组仍安全:只调用 closeSearch', () => {
    const showSearch = ref(false)
    const closeSearch = vi.fn()
    const { closeAllPanels } = useReaderPanels([], closeSearch, showSearch)
    expect(() => closeAllPanels()).not.toThrow()
    expect(closeSearch).toHaveBeenCalledTimes(1)
  })
})
