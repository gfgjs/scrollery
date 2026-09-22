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
import { useSelection } from '../composables/useSelection'

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

  it('同一 epoch 内在飞期间的并发调用合并:仅一个在飞 + 至多一个尾随,调用方等到尾随快照', async () => {
    const first = deferred<AppStats>()
    const second = deferred<AppStats>()
    invokeIpc.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const store = useMediaStore()

    const a = store.loadStats()
    const b = store.loadStats()
    const c = store.loadStats()
    // 并发数=1:三次调用只发出一个在飞请求(共用同一 drain;pinia 会把返回值再包一层
    // promise,故不能断言 promise 引用相等,改由「都等到尾随快照」证明合并)。
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    expect(invokeIpc).toHaveBeenCalledWith(IPC.GET_STATS)

    first.resolve(appStats(1))
    // trailing=1:在飞期间的所有请求只多排一次尾随请求。
    await vi.waitFor(() => expect(invokeIpc).toHaveBeenCalledTimes(2))
    // 尾随在飞期间先提交上一拍快照;调用方的 await 仍要等到尾随落定的新快照。
    expect(store.stats?.totalItems).toBe(1)

    second.resolve(appStats(2))
    await Promise.all([a, b, c])
    expect(store.stats?.totalItems).toBe(2)
    expect(invokeIpc).toHaveBeenCalledTimes(2)
  })

  it('尾随请求期间到达的调用沿用同一 drain,并等自己之后的新快照', async () => {
    const first = deferred<AppStats>()
    const second = deferred<AppStats>()
    const third = deferred<AppStats>()
    invokeIpc
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockReturnValueOnce(third.promise)
    const store = useMediaStore()

    const a = store.loadStats()
    const b = store.loadStats()
    first.resolve(appStats(1))
    await vi.waitFor(() => expect(invokeIpc).toHaveBeenCalledTimes(2))

    const c = store.loadStats()
    second.resolve(appStats(2))
    await vi.waitFor(() => expect(invokeIpc).toHaveBeenCalledTimes(3))

    third.resolve(appStats(3))
    await Promise.all([a, b, c])
    expect(store.stats?.totalItems).toBe(3)
  })

  it('失败抛给调用方并释放槽位,后续请求可重新发起', async () => {
    invokeIpc.mockRejectedValueOnce(new Error('stats down'))
    const store = useMediaStore()

    await expect(store.loadStats()).rejects.toThrow('stats down')
    expect(store.stats).toBeNull()

    invokeIpc.mockResolvedValueOnce(appStats(3))
    await store.loadStats()
    expect(store.stats?.totalItems).toBe(3)
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

  it('作用域销毁后旧快照不得回写(越代丢弃)', async () => {
    const pending = deferred<AppStats>()
    invokeIpc.mockReturnValueOnce(pending.promise)
    const store = useMediaStore()

    const inflight = store.loadStats()
    store.$dispose()
    pending.resolve(appStats(9))
    await inflight

    expect(store.stats).toBeNull()
  })

  it('invalidateStats 丢弃在飞快照并允许跨 epoch 重叠,新请求照常回写(清库路径)', async () => {
    const stale = deferred<AppStats>()
    invokeIpc.mockReturnValueOnce(stale.promise)
    const store = useMediaStore()

    const inflight = store.loadStats()
    // 清库在旧请求在飞时 invalidate:旧快照不得回写。
    store.invalidateStats()
    const fresh = deferred<AppStats>()
    invokeIpc.mockReturnValueOnce(fresh.promise)
    const afterClear = store.loadStats()

    stale.resolve(appStats(99))
    await inflight
    expect(store.stats).toBeNull()

    fresh.resolve(appStats(0))
    await afterClear
    expect(store.stats?.totalItems).toBe(0)
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

  it('关闭查看器会使仍在途的打开请求失效', async () => {
    const pending = deferred<MediaDetail>()
    invokeIpc.mockReturnValueOnce(pending.promise)
    const store = useMediaStore()

    const opening = store.openDetail(1)
    store.closeDetail()
    pending.resolve(detail(1))
    await opening

    expect(store.detailItem).toBeNull()
    expect(store.isDetailOpen).toBe(false)
  })

  it('普通导航立即替换旧搜索上下文,同顺序几何重排可续用,换顺序的迟到邻居不得提交', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    invokeIpc.mockReturnValueOnce(Promise.resolve(detail(1)))
    const store = useMediaStore()
    await store.openDetailFromSearch(1, [1, 2, 3])
    expect(store.navContext?.type).toBe('search')

    invokeIpc.mockResolvedValueOnce(layoutSummary(10))
    await store.computeLayout({ containerWidth: 800, rowHeight: 200 })

    const pending = deferred<MediaDetail>()
    invokeIpc.mockReturnValueOnce(pending.promise)
    const opening = store.openDetail(2, true)
    expect(store.navContext).toMatchObject({ type: 'layout', orderVersion: 10 })
    pending.resolve(detail(2))
    await opening
    expect(store.detailItem?.id).toBe(2)
    const context = store.navContext
    invokeIpc.mockResolvedValueOnce({ ...layoutSummary(11), orderVersion: 10 })
    await store.computeLayout({ containerWidth: 900, rowHeight: 200 })
    expect(store.navContext).toBe(context)
    const neighbor = deferred<MediaDetail>()
    invokeIpc.mockReturnValueOnce(neighbor.promise)
    const navigating = store.navigateDetail(1)
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_ADJACENT_MEDIA, { currentId: 2, offset: 1, orderVersion: 10 })
    store.invalidateViewOrder()
    neighbor.resolve(detail(3))
    await navigating
    expect(store.detailItem?.id).toBe(2)
    expect(store.isNavContextCurrent(context)).toBe(false)
  })

  it('openDetail(fromLayout) IPC 失败也不残留旧导航上下文', async () => {
    invokeIpc.mockReturnValueOnce(Promise.resolve(detail(1)))
    const store = useMediaStore()
    await store.openDetailFromSearch(1, [1, 2, 3])

    invokeIpc.mockReturnValueOnce(Promise.reject(new Error('disk error')))
    await expect(store.openDetail(2, true)).rejects.toThrow('disk error')
    // 失败后 navContext 仍是清空态(此前 bug:只在成功路径清空,失败永久残留旧上下文)。
    expect(store.navContext).toBeNull()
    expect(store.detailItem?.id).toBe(1)
  })

  it('navigateDetail 的 currentIndex 只在响应提交时推进(在途窗口不超前)', async () => {
    invokeIpc.mockReturnValueOnce(Promise.resolve(detail(1)))
    const store = useMediaStore()
    await store.openDetailFromSearch(1, [1, 2, 3])

    const pending = deferred<MediaDetail>()
    invokeIpc.mockReturnValueOnce(pending.promise)
    const nav = store.navigateDetail(1)
    // fetch 在途:索引不得超前于展示项(否则第三次按键以错误基准导航)。
    expect(store.navContext?.currentIndex).toBe(0)
    pending.resolve(detail(2))
    await nav
    expect(store.navContext?.currentIndex).toBe(1)
    expect(store.detailItem?.id).toBe(2)
  })

  it('镜头导航按 layoutVersion 请求后端相邻项，边界不回退普通邻接', async () => {
    const store = useMediaStore()
    store.layoutSummary = layoutSummary(42)
    store.layoutSemanticKey = 'lens:groups'
    store.detailItem = detail(10)
    store.setLensNavContext(42, 3)
    invokeIpc.mockResolvedValueOnce({ detail: detail(11), index: 1, totalCount: 3 })

    await store.navigateDetail(1)

    expect(invokeIpc).toHaveBeenCalledWith(IPC.GET_LENS_ADJACENT_MEDIA, {
      currentId: 10,
      offset: 1,
      layoutVersion: 42,
    })
    expect(invokeIpc).not.toHaveBeenCalledWith(
      IPC.GET_ADJACENT_MEDIA,
      expect.anything(),
    )
    expect(store.detailItem?.id).toBe(11)
    expect(store.navContext).toMatchObject({
      type: 'lens',
      currentIndex: 1,
      totalCount: 3,
    })

    invokeIpc.mockResolvedValueOnce(null)
    await store.navigateDetail(1)
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_LENS_ADJACENT_MEDIA, {
      currentId: 11,
      offset: 1,
      layoutVersion: 42,
    })
    expect(store.detailItem?.id).toBe(11)

    invokeIpc.mockRejectedValueOnce(new Error('ViewStale'))
    await store.navigateDetail(-1)
    expect(store.detailItem?.id).toBe(11)
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_LENS_ADJACENT_MEDIA, {
      currentId: 11,
      offset: -1,
      layoutVersion: 42,
    })
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

  it('同语义重算保留旧布局，跨普通/groups/folders 切换立即清空', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    const store = useMediaStore()
    store.layoutSummary = layoutSummary(20)
    store.layoutSemanticKey = buildLayoutContentKey({ filters: {} })

    const normal = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(normal.promise)
    const resize = store.computeLayout({ containerWidth: 900 })
    expect(store.layoutSummary?.layoutVersion).toBe(20)
    expect(store.layoutSemanticKey).toBe(buildLayoutContentKey({ filters: {} }))
    normal.resolve(layoutSummary(21))
    await resize
    const viewIds = useViewIds()
    invokeIpc.mockResolvedValueOnce([1, 2, 3])
    await viewIds.ensureFresh(store.orderVersion)
    const selection = useSelection()
    selection.clearSelection()
    selection.selectRange(1, 3)
    expect(selection.materializeIds()).toEqual([1, 2, 3])
    selection.selectAll()
    const selectedEpoch = selection.selectionEpoch.value

    const groups = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(groups.promise)
    const enterGroups = store.computeLayout({
      containerWidth: 900,
      duplicateLens: { mode: 'groups', showUniqueItems: false, orderingVersion: 1 },
    })
    expect(store.layoutSummary).toBeNull()
    expect(store.layoutSemanticKey).toBeNull()
    expect(viewIds.allIds()).toEqual([])
    expect(selection.selectedCount.value).toBe(0)
    expect(selection.selectionEpoch.value).toBeGreaterThan(selectedEpoch)
    expect(selection.toBackendDescriptor()).toBeNull()
    selection.clearSelection()
    selection.toggleSelect(1)
    selection.selectRange(1, 3)
    selection.invertSelection()
    selection.selectAll()
    expect(selection.materializeIds()).toEqual([1])
    selection.clearSelection()
    expect(selection.materializeIds()).toEqual([])
    groups.resolve(layoutSummary(22))
    await enterGroups
    expect(store.layoutSummary?.layoutVersion).toBe(22)
    expect(store.layoutSemanticKey).toBe(
      buildLayoutContentKey({
        duplicateLens: { mode: 'groups', showUniqueItems: false, orderingVersion: 1 },
      }),
    )

    const folders = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(folders.promise)
    const enterFolders = store.computeLayout({
      containerWidth: 900,
      duplicateLens: { mode: 'folders', showUniqueItems: false, orderingVersion: 1 },
    })
    expect(store.layoutSummary).toBeNull()
    folders.resolve(layoutSummary(23))
    await enterFolders
    expect(store.layoutSemanticKey).toBe(
      buildLayoutContentKey({
        duplicateLens: { mode: 'folders', showUniqueItems: false, orderingVersion: 1 },
      }),
    )
  })

  it('排队请求被替代时明确结束，最新请求失败不提交中间布局且释放槽位', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    const store = useMediaStore()
    const first = deferred<LayoutSummary>()
    invokeIpc.mockReturnValueOnce(first.promise).mockRejectedValueOnce(new Error('query failed'))
    const initial = store.computeLayout({ containerWidth: 800 })
    const replaced = store.computeLayout({ containerWidth: 900 })
    const latest = store.computeLayout({ containerWidth: 1000 })
    expect(await replaced).toBe('superseded')
    first.resolve(layoutSummary(10))
    expect(await initial).toBe('superseded')
    expect(await latest).toBe('failed')
    expect(store.layoutSummary).toBeNull()
    expect(store.isComputingLayout).toBe(false)
    expect(invokeIpc).toHaveBeenCalledTimes(2)

    invokeIpc.mockResolvedValueOnce(layoutSummary(12))
    expect(await store.computeLayout({ containerWidth: 1100 })).toBe('committed')
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

  it('内容键区分 folders 独有项与普通目录/筛选，并忽略对象键顺序', () => {
    expect(
      buildLayoutContentKey({
        duplicateLens: { mode: 'folders', showUniqueItems: false, orderingVersion: 1 },
      }),
    ).not.toBe(
      buildLayoutContentKey({
        duplicateLens: { mode: 'folders', showUniqueItems: true, orderingVersion: 1 },
      }),
    )
    expect(buildLayoutContentKey({ directoryId: 1, filters: {} })).not.toBe(
      buildLayoutContentKey({ directoryId: 2, filters: {} }),
    )
    expect(buildLayoutContentKey({ filters: { rating: 3, favorite: true } })).toBe(
      buildLayoutContentKey({ filters: { favorite: true, rating: 3 } }),
    )
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
