import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const { invokeIpc } = vi.hoisted(() => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('./uiStore', () => ({ useUiStore: () => ({ thumbInfoElements: [] }) }))

import { buildLayoutContentKey, useMediaStore } from './mediaStore'
import type { LayoutSummary } from '../types/layout'
import type { AppStats, MediaDetail } from '../types/media'
import { IPC } from '../constants/ipc'
import { useViewIds } from '../composables/useViewIds'

function detail(id: number): MediaDetail {
  return { id } as MediaDetail
}

function layoutSummary(version: number): LayoutSummary {
  return {
    totalRows: 1,
    totalHeight: 200,
    layoutVersion: version,
    orderVersion: version,
    totalItems: 1,
    separators: [],
    monthBuckets: [],
  }
}

function appStats(totalItems: number): AppStats {
  return {
    totalItems,
    totalImages: totalItems,
    totalVideos: 0,
    totalAudios: 0,
    totalDocuments: 0,
    totalFavorited: 0,
    totalDeleted: 0,
    totalLivePhotos: 0,
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

describe('mediaStore 统计取数 single-flight', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    invokeIpc.mockReset()
  })

  it('失败时同批调用者共享同一拒绝,已排的尾随被丢弃,槽位释放后后续调用照常', async () => {
    invokeIpc.mockRejectedValueOnce(new Error('stats down'))
    const store = useMediaStore()
    // fire-and-forget 形态(扫描进度的调用点):合并进同一 drain,共用同一次拒绝。
    const fireAndForget = store.loadStats().catch(() => {})
    await expect(store.loadStats()).rejects.toThrow('stats down')
    await fireAndForget
    // 失败路径不保证尾随新快照:整个 drain 拒绝,尾随请求不再发出。
    expect(invokeIpc).toHaveBeenCalledTimes(1)

    invokeIpc.mockResolvedValueOnce(appStats(4))
    await store.loadStats()
    expect(store.stats?.totalItems).toBe(4)
  })
})

describe('mediaStore 详情请求时序', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    invokeIpc.mockReset()
  })

  it('较晚返回的旧 openDetail 不得覆盖较新的条目', async () => {
    const first = deferred<MediaDetail>()
    const second = deferred<MediaDetail>()
    invokeIpc.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const store = useMediaStore()

    const openFirst = store.openDetail(1)
    const openSecond = store.openDetail(2)
    second.resolve(detail(2))
    await openSecond
    first.resolve(detail(1))
    await openFirst

    expect(store.detailItem?.id).toBe(2)
    expect(store.isDetailOpen).toBe(true)
  })

  it('连续 resize 计算只提交最新结果,旧布局在最新结果完成前保持可见', async () => {
    const first = deferred<LayoutSummary>()
    const second = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const store = useMediaStore()
    store.layoutSummary = layoutSummary(10)
    store.layoutSemanticKey = buildLayoutContentKey({ filters: {} })
    vi.stubGlobal('window', { devicePixelRatio: 1 })

    const firstCompute = store.computeLayout({ containerWidth: 800 })
    const queuedCompute = store.computeLayout({ containerWidth: 1000 })
    let queuedFinished = false
    void queuedCompute.then(() => { queuedFinished = true })
    await Promise.resolve()
    await Promise.resolve()
    expect(queuedFinished).toBe(false)
    expect(invokeIpc).toHaveBeenCalledTimes(1)

    first.resolve(layoutSummary(11))
    expect(await firstCompute).toBe('superseded')
    expect(invokeIpc).toHaveBeenCalledTimes(2)
    expect(store.layoutSummary?.layoutVersion).toBe(10)
    expect(queuedFinished).toBe(false)

    second.resolve(layoutSummary(12))
    await firstCompute
    await queuedCompute
    expect(queuedFinished).toBe(true)
    expect(store.layoutSummary?.layoutVersion).toBe(12)
  })

  it('新的查询意图尚未发出布局时，旧计算完成也不能重新开放旧全集', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    const store = useMediaStore()
    const ids = useViewIds()
    const response = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(response.promise)
    const pending = store.computeLayout({ containerWidth: 800 })
    // 模拟语义查询启动/离屏切条件，新条件尚未达到布局入口。
    store.invalidateViewOrder()
    response.resolve(layoutSummary(80))
    await pending
    invokeIpc.mockResolvedValueOnce([7, 8])
    await ids.ensureFresh(80)
    expect(ids.isReady()).toBe(false)
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.GET_VIEW_IDS, expect.anything())
  })

  it('watchdog 后新会话接管时丢弃旧 IPC 的迟到结果', async () => {
    vi.useFakeTimers()
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    const first = deferred<LayoutSummary>()
    const second = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const store = useMediaStore()

    const firstCompute = store.computeLayout({ containerWidth: 800 })
    void store.computeLayout({ containerWidth: 900 })
    await vi.advanceTimersByTimeAsync(30000)
    expect(invokeIpc).toHaveBeenCalledTimes(2)
    expect(await firstCompute).toBe('superseded')

    second.resolve(layoutSummary(32))
    await Promise.resolve()
    await Promise.resolve()
    first.resolve(layoutSummary(31))
    await firstCompute

    expect(store.layoutSummary?.layoutVersion).toBe(32)
    vi.useRealTimers()
  })
})
