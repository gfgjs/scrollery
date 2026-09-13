// useGalleryTauriSync 增强刷新节流测试:MEDIA_ENRICHED 的 2s 窗口是**首沿**而非尾沿防抖——
// 持续富化期间事件不断,旧尾沿写法会把重算一路推后到富化结束才发生一次。此处锁住:
//   · 持续事件流下窗口自首事件起算,事件不延后;
//   · 离屏(rootId=0 哨兵与 requestCompute 的 deferred 路径)不在本层另造 pending;
//   · 作用域销毁清定时器,不再触发重算。

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { effectScope, ref } from 'vue'
import { EVENTS } from '../constants/ipc'

const holders = vi.hoisted(() => ({
  media: null as unknown as {
    totalItems: { value: number }
    layoutDirty: { value: boolean }
    layoutVersion: { value: number }
    consumeLayoutDirty: () => void
    ensureMeta: (ids: number[]) => void
  },
  dedup: null as unknown as { status: { status: string; runId: string | null } },
}))

const eventListeners = new Map<string, (e: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    eventListeners.set(name, cb)
    return Promise.resolve(() => {})
  },
}))

vi.mock('../stores/mediaStore', async () => {
  const { ref: createRef } = await import('vue')
  holders.media = {
    totalItems: createRef(0),
    layoutDirty: createRef(false),
    layoutVersion: createRef(0),
    consumeLayoutDirty: vi.fn(),
    ensureMeta: vi.fn(),
  }
  return { useMediaStore: () => holders.media }
})

vi.mock('../stores/dedupStore', () => {
  holders.dedup = { status: { status: 'idle', runId: null } }
  return { useDedupStore: () => holders.dedup }
})

vi.mock('../stores/uiStore', () => ({
  useUiStore: () => ({ showThumbInfo: false, thumbInfoElements: [] }),
}))

vi.mock('./useViewIds', () => ({ useViewIds: () => ({ ensureFresh: vi.fn() }) }))

import { useGalleryTauriSync, type GalleryTauriSyncDeps } from './useGalleryTauriSync'

function makeDeps(): GalleryTauriSyncDeps & { requestCompute: ReturnType<typeof vi.fn> } {
  return {
    requestCompute: vi.fn(() => true),
    updateVisible: vi.fn(async () => {}),
    refreshCacheDir: vi.fn(async () => {}),
    restoreReflowAnchor: vi.fn(async () => false),
    gridRef: () => null,
    bucketActive: () => false,
    scrollToLogicalY: vi.fn(async () => {}),
    getViewKey: () => 'test',
    shouldLoadViewIds: () => false,
    isLensActive: () => false,
    visibleRows: ref([]),
    mountedRows: () => [],
    activeRows: () => [],
  }
}

function mount(deps: GalleryTauriSyncDeps) {
  const scope = effectScope()
  scope.run(() => useGalleryTauriSync(deps))
  return scope
}

beforeEach(() => {
  eventListeners.clear()
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useGalleryTauriSync 增强刷新节流(首沿 2s 窗口)', () => {
  it('持续事件流下窗口自首事件起算:每 2s 一次,事件不延后', async () => {
    const deps = makeDeps()
    const scope = mount(deps)
    const emitEnriched = eventListeners.get(EVENTS.MEDIA_ENRICHED)!

    // 每 1s 一批的持续富化流:若为尾沿防抖,定时器会不断重置,4s 内一次都不该刷。
    emitEnriched({ payload: {} })
    await vi.advanceTimersByTimeAsync(1000)
    emitEnriched({ payload: {} })
    await vi.advanceTimersByTimeAsync(1000) // t=2s:首沿窗口到期
    expect(deps.requestCompute).toHaveBeenCalledTimes(1)

    emitEnriched({ payload: {} })
    await vi.advanceTimersByTimeAsync(1000)
    emitEnriched({ payload: {} })
    await vi.advanceTimersByTimeAsync(1000) // t=4s:第二个窗口到期
    expect(deps.requestCompute).toHaveBeenCalledTimes(2)
    scope.stop()
  })

  it('离屏(requestCompute 返回 false)只调用一次,不在本层留永久 pending', async () => {
    const deps = makeDeps()
    deps.requestCompute.mockReturnValue(false)
    const scope = mount(deps)
    const emitEnriched = eventListeners.get(EVENTS.MEDIA_ENRICHED)!

    // 本轮无论是否在屏都照常结束窗口;离屏补算由 requestCompute 内部的 deferred 负责。
    emitEnriched({ payload: {} })
    await vi.advanceTimersByTimeAsync(2000)
    expect(deps.requestCompute).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(10_000)
    expect(deps.requestCompute).toHaveBeenCalledTimes(1)

    // 后续事件重新起表,不被上一窗口的离屏状态卡住。
    emitEnriched({ payload: {} })
    await vi.advanceTimersByTimeAsync(2000)
    expect(deps.requestCompute).toHaveBeenCalledTimes(2)
    scope.stop()
  })

  it('作用域销毁清掉尾 timer,销毁后不再触发重算', async () => {
    const deps = makeDeps()
    const scope = mount(deps)
    const emitEnriched = eventListeners.get(EVENTS.MEDIA_ENRICHED)!

    emitEnriched({ payload: {} })
    scope.stop()
    await vi.advanceTimersByTimeAsync(5000)
    expect(deps.requestCompute).not.toHaveBeenCalled()
  })
})
