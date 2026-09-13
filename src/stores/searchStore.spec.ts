// searchStore 单测（S2-a 搜索单源门面）。
// 锁两件事：①读表面（mode/scope/committedQuery）忠实投影 ui/ai 原字段；②写表面
// （commit*/apply/clear/setMode）按当前模式派发到正确的执行入口。门面职责是「路由」而非
// 复刻 aiStore 内部协调，故写路径用 spy 断言派发目标，不真跑语义搜索 / group-sort 副作用。

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

// 中和各 store 的 setup 期 IPC（主要是 uiStore 的 get_startup_config）。返回永不 resolve 的 promise，
// 使 uiStore 启动 .then 不执行 → 规避 node 环境下 applyAppearance 触碰 document 的噪声。
vi.mock('../utils/ipc', () => ({
  invokeIpc: vi.fn(() => new Promise(() => {})),
  invokeIpcRaw: vi.fn(() => new Promise(() => {})),
  ipcErrorMessage: (e: unknown) => String(e),
}))

import { useSearchStore } from './searchStore'
import { useUiStore } from './uiStore'
import { useAiStore } from './aiStore'

describe('searchStore（搜索单源门面 S2-a）', () => {
  beforeEach(() => {
    // node 环境无 window，而 uiStore setup 同步读 window.matchMedia（uiStore.ts:52/55，唯一的
    // setup 期 DOM 依赖）。给最小桩即可实例化真实 uiStore；harness/runtime 的 window 读取发生在
    // import 期（早于此桩），不受影响。document.* 仅在 uiStore 的函数体内，本测试路径不触发。
    vi.stubGlobal('window', {
      matchMedia: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
      location: { search: '' },
      addEventListener: () => {},
      removeEventListener: () => {},
    })
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('读表面：投影自 ui/ai 原字段', () => {
    it('mode 投影 aiStore.searchMode', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'semantic'
      expect(s.mode).toBe('semantic')
    })

    it('scope 投影 uiStore.searchScope', () => {
      const ui = useUiStore()
      const s = useSearchStore()
      ui.searchScope = 'folder'
      expect(s.scope).toBe('folder')
    })

    it('normal 模式 committedQuery 取 uiStore.searchQuery', () => {
      const ui = useUiStore()
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'normal'
      ui.searchQuery = 'sunset'
      expect(s.committedQuery).toBe('sunset')
      expect(s.hasCommittedQuery).toBe(true)
    })

    it('semantic 模式 committedQuery 取 aiStore.semanticQuery', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'semantic'
      ai.semanticQuery = 'a cat on the sofa'
      expect(s.committedQuery).toBe('a cat on the sofa')
    })

    it('mixed 模式按 activeMixedQueryType 归一；未决草稿不计入 committedQuery', () => {
      const ui = useUiStore()
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'mixed'
      s.draftMixedQuery = 'draft-not-committed'
      ai.activeMixedQueryType = 'semantic'
      ai.semanticQuery = 'dog'
      expect(s.committedQuery).toBe('dog')
      ai.activeMixedQueryType = 'normal'
      ui.searchQuery = 'dog.jpg'
      expect(s.committedQuery).toBe('dog.jpg')
    })

    it('纯空白已提交查询 hasCommittedQuery 为假', () => {
      const ui = useUiStore()
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'normal'
      ui.searchQuery = '   '
      expect(s.hasCommittedQuery).toBe(false)
    })
  })

  describe('写表面：commit/apply/clear 派发到对应执行入口', () => {
    it('commitNormal 写 uiStore.searchQuery', () => {
      const ui = useUiStore()
      const s = useSearchStore()
      s.commitNormal('beach')
      expect(ui.searchQuery).toBe('beach')
    })

    it('setScope 写 uiStore.searchScope', () => {
      const ui = useUiStore()
      const s = useSearchStore()
      s.setScope('device')
      expect(ui.searchScope).toBe('device')
    })

    it('commitSemantic 委托 aiStore.runSemanticSearch', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      const spy = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      s.commitSemantic('mountain')
      expect(spy).toHaveBeenCalledWith('mountain')
    })

    it('setMode 委托 setSearchMode；进入 mixed 清空草稿', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      const spy = vi.spyOn(ai, 'setSearchMode').mockImplementation(() => {})
      s.draftMixedQuery = 'stale'
      s.setMode('mixed')
      expect(spy).toHaveBeenCalledWith('mixed')
      expect(s.draftMixedQuery).toBe('')
    })

    it('setMode 切到非 mixed 不动草稿', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      vi.spyOn(ai, 'setSearchMode').mockImplementation(() => {})
      s.draftMixedQuery = 'keep'
      s.setMode('normal')
      expect(s.draftMixedQuery).toBe('keep')
    })

    it('commitMixed(0) 走语义并把 query 落为草稿', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      const sem = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      const norm = vi.spyOn(ai, 'setNormalSearchQueryInMixedMode').mockImplementation(() => {})
      s.commitMixed(0, 'red car')
      expect(sem).toHaveBeenCalledWith('red car')
      expect(norm).not.toHaveBeenCalled()
      expect(s.draftMixedQuery).toBe('red car')
    })

    it('commitMixed(1) 走文件名', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      const sem = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      const norm = vi.spyOn(ai, 'setNormalSearchQueryInMixedMode').mockImplementation(() => {})
      s.commitMixed(1, 'IMG_0001')
      expect(norm).toHaveBeenCalledWith('IMG_0001')
      expect(sem).not.toHaveBeenCalled()
    })

    it('commitMixed 空串复位（setNormalSearchQueryInMixedMode("")）', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      const sem = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      const norm = vi.spyOn(ai, 'setNormalSearchQueryInMixedMode').mockImplementation(() => {})
      s.commitMixed(0, '   ')
      expect(norm).toHaveBeenCalledWith('')
      expect(sem).not.toHaveBeenCalled()
    })

    it('commitMixed 缺省 query 取当前草稿', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      const norm = vi.spyOn(ai, 'setNormalSearchQueryInMixedMode').mockImplementation(() => {})
      s.draftMixedQuery = 'from-draft'
      s.commitMixed(1)
      expect(norm).toHaveBeenCalledWith('from-draft')
    })

    it('apply 在 normal 模式写 uiStore.searchQuery', () => {
      const ui = useUiStore()
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'normal'
      s.apply('lake')
      expect(ui.searchQuery).toBe('lake')
    })

    it('apply 在 semantic 模式走 runSemanticSearch', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'semantic'
      const spy = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      s.apply('forest at dusk')
      expect(spy).toHaveBeenCalledWith('forest at dusk')
    })

    it('apply 在 mixed 模式默认走语义(index 0)，可显式指定文件名', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'mixed'
      const sem = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      const norm = vi.spyOn(ai, 'setNormalSearchQueryInMixedMode').mockImplementation(() => {})
      s.apply('q1')
      expect(sem).toHaveBeenCalledWith('q1')
      s.apply('q2', { mixedIndex: 1 })
      expect(norm).toHaveBeenCalledWith('q2')
    })

    it('clear 在 normal 模式清 uiStore.searchQuery 与草稿', () => {
      const ui = useUiStore()
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'normal'
      ui.searchQuery = 'x'
      s.draftMixedQuery = 'y'
      s.clear()
      expect(ui.searchQuery).toBe('')
      expect(s.draftMixedQuery).toBe('')
    })

    it('clear 在 semantic 模式走 runSemanticSearch("")', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'semantic'
      const spy = vi.spyOn(ai, 'runSemanticSearch').mockResolvedValue(undefined)
      s.clear()
      expect(spy).toHaveBeenCalledWith('')
    })

    it('clear 在 mixed 模式走 setNormalSearchQueryInMixedMode("")', () => {
      const ai = useAiStore()
      const s = useSearchStore()
      ai.searchMode = 'mixed'
      const spy = vi.spyOn(ai, 'setNormalSearchQueryInMixedMode').mockImplementation(() => {})
      s.clear()
      expect(spy).toHaveBeenCalledWith('')
    })
  })
})
