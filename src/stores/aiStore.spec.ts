// aiStore 语义搜索身份/反序一致性（P1-3）的真实调用链回归。
//
// 锁的是「前端 + IPC 契约」两侧的收尾约定，不重跑内核（内核在 search_control.rs 单测里）：
//   · 被取代的查询返回 null ⇒ 不动 matchCount、不触发 relayout、不报搜索错误；
//   · 空查询/清空 ⇒ 本地**先**收尾（令牌换代、灭 spinner、清计数）再发后端清空，
//     且后端清空晚回/失败都不会让 spinner 挂住；
//   · A 查询在途 → 清空 → B 查询：A 迟到的应答（无论 null 还是计数）都不得覆盖 B 的状态；
//   · 切模型/重建嵌入 ⇒ 后端已吊销，前端同步清语义展示态并重排布局。

import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { IPC } from '../constants/ipc'

const invokeIpc = vi.fn()
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
  ipcErrorMessage: (e: unknown) => String(e),
}))

// 分析半部（Sartre:start/restart/status 的 AnalysisBusy 时序）与本测无关：整体换桩，
// 只保留 aiStore 需要的导出，避免把轮询/会话竞争拉进这条搜索用例。
vi.mock('../composables/useAnalysisController', () => ({
  useAnalysisController: () => ({
    fetchStatus: vi.fn(async () => {}),
    analyzeProgress: { value: 0 },
    providerLabel: { value: '' },
    isWaitingBlocked: { value: false },
    waitingForSession: { value: false },
    startAnalysis: vi.fn(async () => {}),
    pauseAnalysis: vi.fn(async () => {}),
    restartAnalysis: vi.fn(async () => {}),
    stopAnalysis: vi.fn(async () => {}),
    maybeAutoResume: vi.fn(async () => {}),
  }),
}))

vi.mock('../utils/logger', () => ({
  logger: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
}))
vi.mock('./toastStore', () => ({ useToastStore: () => ({ addToast: vi.fn() }) }))

import { useAiStore } from './aiStore'
import { useMediaStore } from './mediaStore'

interface Gate<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason?: unknown) => void
}

function gate<T>(): Gate<T> {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

/** 记录语义搜索与清空命令的到达顺序，并让每个搜索命令落在自己的闸门上。 */
function wireSearchCommands() {
  const searches: Gate<number | null>[] = []
  const arrivals: string[] = []
  invokeIpc.mockImplementation((cmd: string, args?: { query?: string }) => {
    if (cmd === IPC.SEMANTIC_SEARCH_CMD) {
      arrivals.push(`search:${args?.query ?? ''}`)
      const g = gate<number | null>()
      searches.push(g)
      return g.promise
    }
    if (cmd === IPC.CLEAR_SEMANTIC_SEARCH) {
      arrivals.push('clear')
      return Promise.resolve(undefined)
    }
    return Promise.resolve(undefined)
  })
  return { searches, arrivals }
}

describe('aiStore 语义搜索身份（P1-3）', () => {
  beforeEach(() => {
    // uiStore setup 同步读 window.matchMedia（node 环境无 window，同 searchStore.spec 的桩）。
    vi.stubGlobal('window', {
      matchMedia: () => ({
        matches: false,
        addEventListener: () => {},
        removeEventListener: () => {},
      }),
      location: { search: '' },
      addEventListener: () => {},
      removeEventListener: () => {},
    })
    setActivePinia(createPinia())
    invokeIpc.mockReset()
    invokeIpc.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('被取代的查询返回 null：不动计数、不触发 relayout、不报错，spinner 收尾', async () => {
    const { searches } = wireSearchCommands()
    const ai = useAiStore()
    const media = useMediaStore()
    const invalidate = vi.spyOn(media, 'invalidateLayout')

    const run = ai.runSemanticSearch('sunset')
    expect(ai.isSearching).toBe(true)
    invalidate.mockClear()

    searches[0].resolve(null) // 后端判定:已被更新的请求取代
    await run

    expect(ai.matchCount).toBe(0)
    expect(ai.searchError).toBeNull()
    expect(invalidate).not.toHaveBeenCalled()
    expect(ai.isSearching).toBe(false)
  })

  it('A 查询在途 → 清空 → B 查询:A 的迟到应答不得覆盖 B 的计数与加载态', async () => {
    const { searches, arrivals } = wireSearchCommands()
    const ai = useAiStore()

    const runA = ai.runSemanticSearch('A')
    expect(ai.isSearching).toBe(true)

    // 用户清空:本地立刻收尾(spinner 停、计数清),后端清空命令已发出。
    ai.clearSemanticSearch()
    expect(ai.matchCount).toBe(0)
    expect(ai.isSearching).toBe(false)

    const runB = ai.runSemanticSearch('B')
    expect(ai.isSearching).toBe(true)
    expect(arrivals).toEqual(['search:A', 'clear', 'search:B'])

    // B 先回来。
    searches[1].resolve(7)
    await runB
    expect(ai.matchCount).toBe(7)
    expect(ai.isSearching).toBe(false)

    // A 迟到:普通计数与 null 两种形态都不得改写展示态。
    searches[0].resolve(99)
    await runA
    expect(ai.matchCount).toBe(7)
    expect(ai.isSearching).toBe(false)
    expect(ai.semanticQuery).toBe('B')
  })

  it('清空后端失败：本地不挂 spinner，也不回滚已清语义', async () => {
    const clearGate = gate<undefined>()
    invokeIpc.mockImplementation((cmd: string) => {
      if (cmd === IPC.CLEAR_SEMANTIC_SEARCH) return clearGate.promise
      return Promise.resolve(undefined)
    })
    const ai = useAiStore()
    ai.isSearching = true
    ai.matchCount = 42

    ai.clearSemanticSearch()
    expect(ai.isSearching).toBe(false)
    expect(ai.matchCount).toBe(0)
    expect(ai.semanticQuery).toBe('')

    // 后端擦除失败(或被吊销的旧清空晚回):前端状态不回滚。
    clearGate.reject(new Error('wipe failed'))
    await Promise.resolve()
    await Promise.resolve()
    expect(ai.isSearching).toBe(false)
    expect(ai.matchCount).toBe(0)
  })

  it('空查询等价清空:本地同步收尾并调后端清空命令', async () => {
    const { arrivals } = wireSearchCommands()
    const ai = useAiStore()

    ai.isSearching = true
    ai.matchCount = 3
    await ai.runSemanticSearch('   ')

    expect(arrivals).toEqual(['clear'])
    expect(ai.isSearching).toBe(false)
    expect(ai.matchCount).toBe(0)
    expect(ai.semanticQuery).toBe('')
  })

  it('mixed 语义在途 → 切普通文字查询:迟到语义应答不得写计数/刷布局,且后端收到吊销', async () => {
    const { searches, arrivals } = wireSearchCommands()
    const ai = useAiStore()
    const media = useMediaStore()
    const invalidate = vi.spyOn(media, 'invalidateLayout')

    ai.searchMode = 'mixed'
    const runSemantic = ai.runSemanticSearch('cat')
    expect(ai.isSearching).toBe(true)
    invalidate.mockClear()

    // 用户在 mixed 下改打普通文件名查询(候选切到 index 1 的提交路径)。
    ai.setNormalSearchQueryInMixedMode('cat.jpg')
    expect(ai.activeMixedQueryType).toBe('normal')
    expect(ai.isSearching).toBe(false)
    // 有语义在场 ⇒ 后端必须收到吊销/擦库。
    expect(arrivals).toEqual(['search:cat', 'clear'])
    expect(invalidate).toHaveBeenCalled()

    // 语义应答迟到(计数形态):不得写回 matchCount,也不得再刷布局。
    invalidate.mockClear()
    searches[0].resolve(123)
    await runSemantic
    expect(ai.matchCount).toBe(0)
    expect(invalidate).not.toHaveBeenCalled()
    expect(ai.isSearching).toBe(false)
  })

  it('mixed 下普通文字连打不重复打后端(mixed 普通查询每次防抖都会提交)', async () => {
    const { arrivals } = wireSearchCommands()
    const ai = useAiStore()

    ai.searchMode = 'mixed'
    ai.setNormalSearchQueryInMixedMode('a')
    ai.setNormalSearchQueryInMixedMode('ab')
    ai.setNormalSearchQueryInMixedMode('abc')

    expect(arrivals).toEqual([])
    expect(ai.activeMixedQueryType).toBe('normal')
  })

  it('mixed 下清空文字:语义在场时同样吊销并复位子类型', async () => {
    const { searches, arrivals } = wireSearchCommands()
    const ai = useAiStore()

    ai.searchMode = 'mixed'
    const runSemantic = ai.runSemanticSearch('cat')
    ai.setNormalSearchQueryInMixedMode('')

    expect(arrivals).toEqual(['search:cat', 'clear'])
    expect(ai.activeMixedQueryType).toBe('none')

    searches[0].resolve(5)
    await runSemantic
    expect(ai.matchCount).toBe(0)
  })

  it('切模型:清语义展示态并让画廊重排(后端已吊销旧向量空间的结果)', async () => {
    invokeIpc.mockResolvedValue(undefined)
    const ai = useAiStore()
    const media = useMediaStore()
    const invalidate = vi.spyOn(media, 'invalidateLayout')
    ai.matchCount = 5
    ai.semanticQuery = 'sunset'

    await ai.setActiveModel('cn-clip.img.fp16.onnx')

    const commands = invokeIpc.mock.calls.map((c) => c[0])
    expect(commands).toContain(IPC.SET_ACTIVE_MODEL)
    expect(commands).toContain(IPC.CLEAR_SEMANTIC_SEARCH)
    expect(ai.matchCount).toBe(0)
    expect(ai.semanticQuery).toBe('')
    expect(invalidate).toHaveBeenCalled()
  })

  it('重建嵌入:后端重置向量并吊销在途搜索后,前端清语义展示态', async () => {
    invokeIpc.mockResolvedValue(undefined)
    const ai = useAiStore()
    ai.matchCount = 5
    ai.semanticQuery = 'sunset'

    await ai.rebuildEmbeddings()

    const commands = invokeIpc.mock.calls.map((c) => c[0])
    expect(commands).toContain(IPC.REBUILD_EMBEDDINGS)
    expect(commands).toContain(IPC.CLEAR_SEMANTIC_SEARCH)
    expect(ai.matchCount).toBe(0)
  })
})
