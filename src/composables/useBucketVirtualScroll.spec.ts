// 核心回归：连续远跳有界、布局换代隔离，以及超大库滚动端点可达。
// node 环境,无 DOM:容器用普通对象伪造;composable 在组件外调用时 onMounted/
// onBeforeUnmount 为 no-op(仅 [Vue warn],ResizeObserver 永不构造),与方案 A spec 同法。
// 真机四根因(2026-07-04 诊断)在此的对应锁定:根因 A→「远跳后终点段最先取」;
// 根因 B→「离窗段在途应答丢弃、无幽灵挂载」;根因 C/D 由架构消除(无观察器/无全量占位)。
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { ref, nextTick } from 'vue'
import { useBucketVirtualScroll } from './useBucketVirtualScroll'
import type { LayoutRow } from '../types/layout'

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(() => {
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
  const container = {
    scrollTop: init?.scrollTop ?? 0,
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

  const bs = useBucketVirtualScroll({
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
  })

  return { bs, version, totalHeight, container, fetchCalls, pendingFetches, state, canvas }
}

describe('useBucketVirtualScroll:有界最新优先/丢弃/换代', () => {

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

// ── 映射态(B3/B3.1):总高 > 16M 的段级坐标映射 ──────────────────────────────
// B3.1 输入源分类锁定:有 1:1 印记(onWheel/onKeydown/onTouchmove/程序化局部滚动)的
// 手势局部 1:1;无印记滚动链 = 滚动条拖动 → **逐事件**比例重锚(拖到边 = 逻辑边);
// 物理钉边由 onWheel 推锚差续滚到逻辑边缘,到边即硬停;偿债仅停稳后,手势中零 scrollTop
// 写入(真机「到边一跳一跳还能继续滚」回归锁)。手势链沿用起点分类——fake timers 驱动
// Date.now,用 advanceTimersByTimeAsync(150) 断链(< SETTLE_MS,不误触偿债)。

describe('映射态(B3):段级坐标映射', () => {
  const PHYS_MAX = 16_000_000 - 1000 // spacer 封顶 − viewH
  const LOG_MAX = 40_000_000 - 1000
  const gLogical = (p: number) => (p / PHYS_MAX) * LOG_MAX
  const wheel = (dy: number) => ({ deltaY: dy, deltaMode: 0 }) as unknown as WheelEvent

  it('无印记慢速滚动链 = 滚动条拖动:逐事件比例重锚,拖到底即逻辑底(真机回归)', async () => {
    vi.useFakeTimers()
    try {
      const h = makeHarness({ totalHeight: 40_000_000 })
      // 从静止慢拖:每事件 2000px ≪ 巨跳阈值——B3 初版误判 1:1,B3.1 链内沿用「比例」
      for (let p = 2000; p <= 10_000; p += 2000) {
        h.container.scrollTop = p
        h.bs.onScroll()
        expect(h.bs.logicalScrollTop.value).toBeCloseTo(gLogical(p), 5)
      }
      // 慢拖到物理底 → 逻辑恰为库底,不存在「还能继续滚」的钉住态
      h.container.scrollTop = PHYS_MAX
      h.bs.onScroll()
      expect(h.bs.logicalScrollTop.value).toBeCloseTo(LOG_MAX, 5)
      // 逆向慢拖离底:仍是链内比例,而非 1:1
      h.container.scrollTop = PHYS_MAX - 3000
      h.bs.onScroll()
      expect(h.bs.logicalScrollTop.value).toBeCloseTo(gLogical(PHYS_MAX - 3000), 5)
    } finally {
      vi.useRealTimers()
    }
  })

  it('物理钉底后滚轮续滚:直达逻辑底后硬停,不再「还能继续滚」', async () => {
    vi.useFakeTimers()
    try {
      const h = makeHarness({ totalHeight: 40_000_000 })
      h.container.scrollTop = PHYS_MAX - 5000
      h.bs.onScroll() // 巨跳 → 比例重锚
      await vi.advanceTimersByTimeAsync(150)
      h.bs.onWheel(wheel(120)) // 印记
      h.container.scrollTop = PHYS_MAX // 1:1 下滚 5000 → 钉底
      h.bs.onScroll()
      expect(h.bs.logicalScrollTop.value).toBeLessThan(LOG_MAX - 1) // 钉底但逻辑未到底
      let guard = 0
      while (h.bs.logicalScrollTop.value < LOG_MAX && ++guard < 10_000) {
        h.bs.onWheel(wheel(40_000))
      }
      expect(h.bs.logicalScrollTop.value).toBe(LOG_MAX)
      // 到达逻辑底后再滚:钉边条件不再成立 → no-op 硬停
      const dAtEnd = h.bs.anchorDelta.value
      h.bs.onWheel(wheel(1000))
      expect(h.bs.anchorDelta.value).toBe(dAtEnd)
      expect(h.bs.logicalScrollTop.value).toBe(LOG_MAX)
      expect(h.container.scrollTop).toBe(PHYS_MAX) // 全程未写 scrollTop
    } finally {
      vi.useRealTimers()
    }
  })

  it('换代总高缩水回非映射态:anchorDelta 归零、spacer 随新几何(不越界)', async () => {
    const h = makeHarness({ totalHeight: 40_000_000 })
    await flush()
    h.container.scrollTop = 8_000_000
    h.bs.onScroll() // 比例重锚 → 锚差非零
    expect(h.bs.anchorDelta.value).toBeGreaterThan(0)

    // 布局换代:总高缩回非映射区间(< 16M spacer 上限)。
    h.totalHeight.value = 15_000
    h.version.value++
    await nextTick()
    await flush()
    expect(h.bs.anchorDelta.value).toBe(0)
    expect(h.bs.spacerHeight.value).toBe(15_000)
  })
})
