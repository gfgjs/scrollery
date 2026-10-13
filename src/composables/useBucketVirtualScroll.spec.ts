// bucket 取数与真实映射状态流：覆盖并发、换代、封顶恢复及输入分类。
// Node 环境使用伪容器；组件外生命周期钩子不运行，不构造 ResizeObserver。
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { effectScope, nextTick, ref, type EffectScope } from 'vue'
import { BUCKET_NATIVE_MAX, useBucketVirtualScroll } from './useBucketVirtualScroll'
import type { LayoutRow } from '../types/layout'

vi.mock('../utils/logger', () => ({ logger: { info: vi.fn(), error: vi.fn() } }))

const scopes: EffectScope[] = []

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(() => {
  scopes.splice(0).forEach((scope) => scope.stop())
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

// ── 取数管线 ─────────────────────────────────────────────────────────────────

type Deferred<T> = { promise: Promise<T>; resolve: (v: T) => void; reject: (e: unknown) => void }
function deferred<T>(): Deferred<T> {
  let resolve!: (v: T) => void
  let reject!: (e: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function normalRow(y: number, height: number): LayoutRow {
  return { rowType: 'normal', y, height, items: [] }
}

/** 等待泵循环消化 resolve/reject(宏任务一跳,保证 await 链走完)。 */
function flush() {
  return new Promise((r) => setTimeout(r, 0))
}

function makeHarness(init?: {
  createScroll?: typeof useBucketVirtualScroll
  spacerCap?: number
  totalHeight?: number
  scrollTop?: number
  deferred?: boolean
  rows?: LayoutRow[]
  /// 默认 200 → clampSegmentPx(200)=4000=SEGMENT_PX,保持既有段边界(全部历史用例零改)。
  rowHeight?: number
  /// Canvas 渲染模式开关(§4.3 S3):true 时愿望窗口至少覆盖面图预取的几何范围。
  canvasMode?: boolean
  /// 视口高(px):默认 1000(既有用例);高视口用例传 2160。
  viewportHeight?: number
  resolveLayoutTarget?: (version: number, isCurrent: () => boolean) => Promise<{ y: number } | null>
}) {
  const version = ref(1)
  const totalHeight = ref(init?.totalHeight ?? 15_000)
  const canvas = ref(init?.canvasMode ?? false)
  let physicalTop = init?.scrollTop ?? 0
  const container = {
    // 仅模拟浏览器的 scrollTop 范围钳制；映射与输入分类仍由真实 composable 执行。
    get scrollTop() {
      physicalTop = Math.max(0, Math.min(physicalTop, Math.min(totalHeight.value, init?.spacerCap ?? Infinity) - this.clientHeight))
      return physicalTop
    },
    set scrollTop(value: number) { physicalTop = value },
    clientHeight: init?.viewportHeight ?? 1000,
    // scrollToLogicalY 的局部/非映射路径经 el.scrollTo 落位(behavior 在 node 无意义)。
    scrollTo(o: { top: number }) {
      container.scrollTop = o.top
    },
  }
  const fetchCalls: Array<[number, number]> = []
  const pendingFetches: Array<Deferred<LayoutRow[]>> = []
  // 注意:非 deferred 模式下构造即取数(immediate watch),初始应答须经 init.rows 注入。
  const state = { useDeferred: init?.deferred ?? false, rowsToReturn: init?.rows ?? [] }

  const scope = effectScope()
  scopes.push(scope)
  const bs = scope.run(() => (init?.createScroll ?? useBucketVirtualScroll)({
    totalHeight: () => totalHeight.value,
    layoutVersion: () => version.value,
    fetchBucketRows: (s, e) => {
      fetchCalls.push([s, e])
      if (state.useDeferred) {
        const d = deferred<LayoutRow[]>()
        pendingFetches.push(d)
        return d.promise
      }
      return Promise.resolve(state.rowsToReturn)
    },
    containerRef: () => container as unknown as HTMLElement,
    rowHeight: () => init?.rowHeight ?? 200, // 200 → segPx=4000(既有段边界)
    canvasMode: () => canvas.value,
    resolveLayoutTarget: init?.resolveLayoutTarget,
  }))!

  return { bs, version, totalHeight, container, fetchCalls, pendingFetches, state, canvas }
}

describe('useBucketVirtualScroll:有界最新优先/丢弃/换代', () => {

  it.each(['Home', 'End'])('映射态方向键后紧接 %s，小位移也须抵达图库边界', async (key) => {
    const totalHeight = BUCKET_NATIVE_MAX * 2
    const viewportHeight = 1000
    const logicalMax = totalHeight - viewportHeight
    const physicalMax = BUCKET_NATIVE_MAX - viewportHeight
    const h = makeHarness({ totalHeight, viewportHeight })
    await h.bs.scrollToLogicalY(key === 'Home' ? 4000 : logicalMax - 4000)
    h.bs.onScroll()

    // 方向键产生的局部滚动仍在同一手势链内；到物理边界的位移不足巨跳判定阈值。
    h.bs.onKeydown({ key: key === 'Home' ? 'ArrowUp' : 'ArrowDown' } as KeyboardEvent)
    h.container.scrollTop += key === 'Home' ? -100 : 100
    h.bs.onScroll()
    h.bs.onKeydown({ key } as KeyboardEvent)
    h.container.scrollTop = key === 'Home' ? 0 : physicalMax
    h.bs.onScroll()

    expect(h.bs.logicalScrollTop.value).toBe(key === 'Home' ? 0 : logicalMax)
    await flush()
    expect(
      h.bs.segments.value.some((segment) =>
        key === 'Home' ? segment.start === 0 : segment.end === totalHeight,
      ),
    ).toBe(true)
  })

  it('远跳(根因 A/B 锁定):飞掠段被丢弃、其应答不落地(无幽灵),终点段最先取', async () => {
    const h = makeHarness({ deferred: true })
    expect(h.fetchCalls).toEqual([[0, 4000]]) // 段 0 在途

    // 滚动条拖到 12000:愿望变 [2,3],段 0 离窗被丢弃
    h.container.scrollTop = 12_000
    h.bs.onScroll()
    expect(h.bs.segments.value.map((s) => s.index)).toEqual([2, 3])
    // 段 0 已离开愿望集，最新视口段立即占旁路第 2 槽，不等陈旧 IPC 返回。
    expect(h.fetchCalls).toEqual([
      [0, 4000],
      [12_000, 15_000],
    ])
    expect(h.pendingFetches.length).toBe(2)

    // 陈旧应答落地 → 复核失败被丢弃:段表无 index 0、无行
    h.pendingFetches[0].resolve([normalRow(0, 100)])
    await flush()
    expect(h.bs.segments.value.map((s) => s.index)).toEqual([2, 3])
    expect(h.bs.mountedRows().length).toBe(0)

    // 泵继续:距视口中心(12500)更近的段 3(中心 13500)先于段 2(中心 10000)
    expect(h.fetchCalls[1]).toEqual([12_000, 15_000])
    h.pendingFetches[1].resolve([normalRow(12_000, 100)])
    await flush()
    expect(h.fetchCalls[2]).toEqual([8000, 12_000])
  })

  it('连续远跳最多两个 IPC 在途，槽位释放后只追最新视口', async () => {
    const h = makeHarness({ totalHeight: 40_000, deferred: true })
    expect(h.fetchCalls).toEqual([[0, 4000]])

    h.container.scrollTop = 12_000
    h.bs.onScroll()
    expect(h.fetchCalls[1]).toEqual([12_000, 16_000])

    // 两个请求都已陈旧，但硬上限为 2；第三个目标先登记愿望，不继续扩并发。
    h.container.scrollTop = 24_000
    h.bs.onScroll()
    expect(h.fetchCalls.length).toBe(2)

    h.pendingFetches[0].resolve([normalRow(0, 100)])
    await flush()
    // 任一陈旧槽释放后重新按当前中心挑选，只追 24k 所在最新段。
    expect(h.fetchCalls[2]).toEqual([24_000, 28_000])
    expect(h.fetchCalls.length).toBe(3)
  })

  it('布局换代:先解析最新恢复位再取目标段,迟到锚点与旧行均不得落地', async () => {
    const second = deferred<{ y: number }>()
    const third = deferred<{ y: number }>()
    const h = makeHarness({
      totalHeight: 40_000,
      deferred: true,
      resolveLayoutTarget: async (version) => version === 1 ? { y: 0 } : version === 2 ? second.promise : third.promise,
    })
    await flush()
    expect(h.fetchCalls).toEqual([[0, 4000]])
    h.version.value = 2
    await flush()
    h.pendingFetches[0].resolve([normalRow(0, 100)])
    await flush()
    expect(h.fetchCalls).toEqual([[0, 4000]])
    expect(h.bs.mountedRows()).toEqual([])
    h.version.value = 3
    await flush()
    third.resolve({ y: 24_000 })
    await flush()
    expect(h.fetchCalls[1]).toEqual([24_000, 28_000])
    expect(h.container.scrollTop).toBe(24_000)
    second.resolve({ y: 12_000 })
    await flush()
    expect(h.container.scrollTop).toBe(24_000)
    expect(h.fetchCalls.length).toBe(2)
    h.pendingFetches[1].resolve([normalRow(24_000, 100)])
    await flush()
    expect(h.bs.mountedRows()).toEqual([normalRow(24_000, 100)])
  })
})


describe('useBucketVirtualScroll:调试封顶下的真实映射状态流', () => {
  let createScroll: typeof useBucketVirtualScroll

  beforeAll(async () => {
    // SPACER_CAP 在模块初始化时读取；沿用产品调试入口，不替换映射实现。
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => key === 'scrollery.debug.bucketSpacer' ? '10000' : null,
    })
    try {
      vi.resetModules()
      const module = await import('./useBucketVirtualScroll')
      expect(module.resolveBucketSpacerCap()).toBe(10_000)
      createScroll = module.useBucketVirtualScroll
    } finally {
      vi.unstubAllGlobals()
    }
  })

  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-10-11T00:00:00Z'))
  })

  function mappedHarness(init?: Parameters<typeof makeHarness>[0]) {
    return makeHarness({ totalHeight: 20_000, ...init, createScroll, spacerCap: 10_000 })
  }

  async function settled(h: ReturnType<typeof makeHarness>) {
    await nextTick()
    await h.bs.whenSettled()
    await nextTick()
  }

  function wheel(h: ReturnType<typeof makeHarness>, dy: number) {
    h.bs.onWheel({ deltaY: dy, deltaMode: 0 } as WheelEvent)
  }

  it('T4-01:布局恢复进入再退出映射态，目标段和后续滚轮位置一致', async () => {
    let restoreY = 6000
    const h = mappedHarness({ totalHeight: 9000, resolveLayoutTarget: async () => ({ y: restoreY }) })
    await settled(h)
    h.bs.onScroll()
    expect(h.container.scrollTop).toBe(6000)
    expect(h.bs.anchorDelta.value).toBe(0)

    h.fetchCalls.length = 0
    h.totalHeight.value = 20_000
    h.version.value++
    await settled(h)
    expect(h.bs.spacerHeight.value).toBe(10_000)
    expect(h.bs.logicalScrollTop.value).toBe(6000)
    expect(h.container.scrollTop).toBe(2842)
    expect(h.bs.anchorDelta.value).toBe(3158)
    expect(h.fetchCalls[0]).toEqual([4000, 8000])
    expect(h.bs.onScroll()).toBe(true)
    wheel(h, 80)
    h.container.scrollTop += 80
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(6080)

    restoreY = 6080
    h.totalHeight.value = 9000
    h.version.value++
    await settled(h)
    expect(h.bs.spacerHeight.value).toBe(9000)
    expect(h.bs.anchorDelta.value).toBe(0)
    expect(h.container.scrollTop).toBe(6080)
    expect(h.bs.logicalScrollTop.value).toBe(6080)
    expect(h.bs.onScroll()).toBe(true)
    h.container.scrollTop += 80
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(6160)
  })

  it('T4-02:恢复解析返回空时，退出封顶仍保留原逻辑位置', async () => {
    const h = mappedHarness({ resolveLayoutTarget: async () => null })
    await settled(h)
    await h.bs.scrollToLogicalY(7000)
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(7000)

    h.totalHeight.value = 9000
    h.version.value++
    await settled(h)
    expect(h.bs.logicalScrollTop.value).toBe(7000)
    expect(h.container.scrollTop).toBe(7000)
    expect(h.bs.anchorDelta.value).toBe(0)
    h.bs.onScroll()
    wheel(h, 50)
    h.container.scrollTop += 50
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(7050)
  })

  it('T4-03:连续滚轮在物理钉边后续滚，停稳偿债保持逻辑位置', async () => {
    const h = mappedHarness()
    await settled(h)
    await h.bs.scrollToLogicalY(18_000)
    h.bs.onScroll()
    wheel(h, 474)
    h.container.scrollTop = 9000
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(18_474)

    await vi.advanceTimersByTimeAsync(50)
    wheel(h, 100)
    expect(h.container.scrollTop).toBe(9000)
    expect(h.bs.logicalScrollTop.value).toBe(18_574)
    await vi.advanceTimersByTimeAsync(199)
    expect(h.container.scrollTop).toBe(9000)
    await vi.advanceTimersByTimeAsync(1)
    expect(h.container.scrollTop).toBe(8798)
    expect(h.bs.logicalScrollTop.value).toBe(18_574)
    expect(h.bs.onScroll()).toBe(true)

    wheel(h, 100)
    h.container.scrollTop += 100
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(18_674)
    wheel(h, 102)
    h.container.scrollTop = 9000
    h.bs.onScroll()
    wheel(h, 1000)
    expect(h.bs.logicalScrollTop.value).toBe(19_000)
    wheel(h, 100)
    expect(h.bs.logicalScrollTop.value).toBe(19_000)
  })

  it('T4-04:连续滚动条拖动按比例重锚并到达两端逻辑边界', async () => {
    const h = mappedHarness()
    await settled(h)
    h.container.scrollTop = 4500
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(9500)
    await vi.advanceTimersByTimeAsync(16)
    h.container.scrollTop = 9000
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(19_000)
    await settled(h)
    expect(h.bs.segments.value[h.bs.segments.value.length - 1].end).toBe(20_000)
    h.container.scrollTop = 0
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(0)
    expect(h.bs.anchorDelta.value).toBe(0)
    expect(h.bs.segments.value[0].start).toBe(0)
  })

  it.each([
    { key: 'Home', physical: 0, logical: 0 },
    { key: 'End', physical: 9000, logical: 19_000 },
  ])('T4-05:$key 在滚轮手势之后仍定位逻辑边界', async ({ key, physical, logical }) => {
    const h = mappedHarness()
    await settled(h)
    await h.bs.scrollToLogicalY(9500)
    h.bs.onScroll()
    wheel(h, 100)
    h.container.scrollTop += 100
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(9600)

    h.bs.onKeydown({ key } as KeyboardEvent)
    h.container.scrollTop = physical
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(logical)
    expect(h.container.scrollTop + h.bs.anchorDelta.value).toBe(logical)
  })

  it.each([
    { key: 'Home', start: 1000, dy: -474, physical: 0, before: 526, logical: 0 },
    { key: 'End', start: 18_000, dy: 474, physical: 9000, before: 18_474, logical: 19_000 },
  ])('T4-06:$key 在物理已钉边且没有新 scroll 事件时立即到达逻辑边界', async ({ key, start, dy, physical, before, logical }) => {
    const h = mappedHarness({ resolveLayoutTarget: async () => ({ y: start }) })
    await settled(h)
    h.bs.onScroll()
    wheel(h, dy)
    h.container.scrollTop = physical
    h.bs.onScroll()
    expect(h.bs.logicalScrollTop.value).toBe(before)

    h.bs.onKeydown({ key } as KeyboardEvent)
    await settled(h)
    expect(h.bs.logicalScrollTop.value).toBe(logical)
    expect(h.container.scrollTop + h.bs.anchorDelta.value).toBe(logical)
  })

})
