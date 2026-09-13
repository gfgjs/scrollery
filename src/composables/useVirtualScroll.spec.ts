// useVirtualScroll 坐标平移数学的特性化锁测试(R2-5)。
// ⚠️ 本文件是 **pre-T16 characterization lock**:锁定现行(方案A 兜底态)行为原样,包括几处
// 刻意保留的已知瑕疵——requestBottom 不钳制(:330)、viewH=0 早退时 spacer/isTranslated
// 已半更新(:280-297)。T16 方案B 重写本模块时,这些测试应随新契约整体重写,不要为
// 「看起来更对」而修改断言。
// 2026-07-06 审查 F1:「fetch 失败毒化跳过框」已从瑕疵清单修复出列——bucket 成为默认引擎后
// 方案 A 是官方回退路径,失败永不重试属带病上路;对应断言已随行为同步改为「失败框回滚可重试」。
// node 环境,无 DOM:composable 在组件外调用时 onMounted/onBeforeUnmount 为 no-op(仅
// [Vue warn]),ResizeObserver 永不构造;容器/渲染层用普通对象伪造。
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { nextTick, ref } from 'vue'
import { useVirtualScroll, resolveSafeMax, SAFE_MAX_DEFAULT } from './useVirtualScroll'
import { canvasPrefetchCoveragePx } from '../utils/galleryPrefetchWindow'
import type { LayoutRow } from '../types/layout'

// ── 测试夹具 ─────────────────────────────────────────────────────────────────

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

interface Harness {
  vs: ReturnType<typeof useVirtualScroll>
  container: {
    clientHeight: number
    scrollTop: number
    scrollHeight: number
    addEventListener: ReturnType<typeof vi.fn>
    removeEventListener: ReturnType<typeof vi.fn>
    scrollTo: ReturnType<typeof vi.fn>
  }
  layerStyle: { transform: string }
  transformWrites: string[]
  fetchCalls: Array<[number, number]>
  /** 下一次 fetch 的应答队列:deferred 时由测试手动 settle,否则立即 resolve 传入的行。 */
  pendingFetches: Array<Deferred<LayoutRow[]>>
  setTotalHeight: (h: number) => void
  setTotalRows: (n: number) => void
  setRowHeight: (h: number) => void
  /** 为 true 时 fetch 返回 deferred(手动 settle);否则立即 resolve rowsToReturn。 */
  useDeferred: boolean
  rowsToReturn: LayoutRow[]
  /** Canvas 渲染模式开关(§4.3 S3),运行时可翻转并经 watch 触发重取。 */
  canvasMode: { value: boolean }
}

function makeHarness(init: {
  totalHeight: number
  totalRows?: number
  rowHeight?: number
}): Harness {
  let th = init.totalHeight
  let rows = init.totalRows ?? 100
  let rh = init.rowHeight ?? 100
  // 真实 ref:引擎经 watch(canvasMode) 监听运行时切换,普通对象不具响应性。
  const canvasMode = ref(false)

  const transformWrites: string[] = []
  let transformBacking = ''
  const layerStyle = {} as { transform: string }
  Object.defineProperty(layerStyle, 'transform', {
    get: () => transformBacking,
    set: (v: string) => {
      transformBacking = v
      transformWrites.push(v)
    },
  })

  const container = {
    clientHeight: 1000,
    scrollTop: 0,
    scrollHeight: 0,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    scrollTo: vi.fn(),
  }

  const h: Partial<Harness> = {
    container,
    layerStyle,
    transformWrites,
    fetchCalls: [],
    pendingFetches: [],
    useDeferred: false,
    rowsToReturn: [],
  }

  const vs = useVirtualScroll({
    totalHeight: () => th,
    totalRows: () => rows,
    fetchRowsByY: (topY: number, bottomY: number) => {
      h.fetchCalls!.push([topY, bottomY])
      if (h.useDeferred) {
        const d = deferred<LayoutRow[]>()
        h.pendingFetches!.push(d)
        return d.promise
      }
      return Promise.resolve(h.rowsToReturn!)
    },
    containerRef: () => container as unknown as HTMLElement,
    layerRef: () => ({ style: layerStyle }) as unknown as HTMLElement,
    rowHeight: () => rh,
    canvasMode: () => canvasMode.value,
  })

  h.vs = vs
  h.canvasMode = canvasMode
  h.setTotalHeight = (v) => {
    th = v
  }
  h.setTotalRows = (v) => {
    rows = v
  }
  h.setRowHeight = (v) => {
    rh = v
  }
  return h as Harness
}

beforeEach(() => {
  // 静音 [Vue warn](组件外调用生命周期钩子)与 composable 自身的诊断输出;
  // 行为断言全部走状态/调用面,不依赖 console。
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
  vi.spyOn(console, 'info').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

// ── 契约 1:logicalToPhysical / physicalToLogical 映射 ───────────────────────

describe('logicalToPhysical:恒等/压缩/边界/钳制', () => {
  it('普通模式(totalHeight ≤ SAFE_MAX)为精确恒等映射', () => {
    const h = makeHarness({ totalHeight: 500_000 })
    // physMax=logMax=499_000,(y/m)*m 浮点精确
    expect(h.vs.logicalToPhysical(0)).toBe(0)
    expect(h.vs.logicalToPhysical(123_456)).toBe(123_456)
    expect(h.vs.logicalToPhysical(499_000)).toBe(499_000)
  })

  it('平移模式:底部精确映射到 physMax,中点等比,round-trip 收敛', () => {
    const h = makeHarness({ totalHeight: 40_000_000 })
    // physicalTotal=10M,physMax=9_999_000,logMax=39_999_000
    expect(h.vs.logicalToPhysical(39_999_000)).toBe(9_999_000)
    expect(h.vs.logicalToPhysical(19_999_500)).toBe(4_999_500)
    // round-trip:physicalToLogical 未导出,经 (phys/physMax)*logMax 手算对拍
    const phys = h.vs.logicalToPhysical(12_345_678)
    expect((phys / 9_999_000) * 39_999_000).toBeCloseTo(12_345_678, 6)
  })

  it('负输入先钳 0 再缩放', () => {
    const h = makeHarness({ totalHeight: 40_000_000 })
    expect(h.vs.logicalToPhysical(-500)).toBe(0)
  })

  it('内容矮于视口(logMax≤0)时恒返回 0', () => {
    const h = makeHarness({ totalHeight: 500 })
    expect(h.vs.logicalToPhysical(123)).toBe(0)
    expect(h.vs.logicalToPhysical(0)).toBe(0)
  })
})

// ── 契约 2:spacerHeight 钳制 + isTranslated 严格大于边界 ─────────────────────

describe('spacerHeight/isTranslated:SAFE_MAX 边界(严格 >)', () => {
  it('恰好 10_000_000 不进入平移模式(严格大于)', async () => {
    const h = makeHarness({ totalHeight: 10_000_000 })
    await h.vs.updateVisible(true)
    expect(h.vs.spacerHeight.value).toBe(10_000_000)
    expect(h.vs.isTranslated.value).toBe(false)
  })

  it('10_000_001 进入平移模式且 spacer 封顶', async () => {
    const h = makeHarness({ totalHeight: 10_000_001 })
    await h.vs.updateVisible(true)
    expect(h.vs.spacerHeight.value).toBe(10_000_000)
    expect(h.vs.isTranslated.value).toBe(true)
  })

  it('40M:spacer 封顶 10M、平移开', async () => {
    const h = makeHarness({ totalHeight: 40_000_000 })
    await h.vs.updateVisible(true)
    expect(h.vs.spacerHeight.value).toBe(10_000_000)
    expect(h.vs.isTranslated.value).toBe(true)
  })

  it('空布局(totalRows=0):全量复位且不发 fetch', async () => {
    const h = makeHarness({ totalHeight: 0, totalRows: 0 })
    await h.vs.updateVisible(true)
    expect(h.vs.visibleRows.value).toEqual([])
    expect(h.vs.paddingTop.value).toBe(0)
    expect(h.vs.paddingBottom.value).toBe(0)
    expect(h.vs.renderAnchor.value).toBe(0)
    expect(h.vs.logicalScrollTop.value).toBe(0)
    expect(h.vs.spacerHeight.value).toBe(0)
    expect(h.fetchCalls.length).toBe(0)
  })

  it('viewH=0 早退:spacer/isTranslated 已更新但不 fetch(半更新态特性化)', async () => {
    const h = makeHarness({ totalHeight: 40_000_000 })
    h.container.clientHeight = 0
    await h.vs.updateVisible(true)
    expect(h.vs.spacerHeight.value).toBe(10_000_000)
    expect(h.vs.isTranslated.value).toBe(true)
    expect(h.fetchCalls.length).toBe(0)
  })
})

// ── 契约 3:自适应缓冲 + 取行窗口 + renderAnchor ─────────────────────────────

describe('缓冲窗口数学与 renderAnchor', () => {
  it('rowHeight=100 → bufferH=800:窗口 [scroll-960, scroll+viewH+960],anchor=floor(top)', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    await h.vs.updateVisible(true)
    expect(h.fetchCalls[0]).toEqual([4040, 6960])
    expect(h.vs.renderAnchor.value).toBe(4040)
  })

  it('bufferH 钳制:compact(rh=40/10→240) 非compact(rh=200→1200)(T2 §7.2 收窗)', async () => {
    // T2 收窗:rh<100(compact) 缓冲行 8→4 且下限 400→240px。rh=40/10(抬到40)均 compact → 240;
    // rh=200 非 compact → 仍 1200(证明非 compact 路径未改)。窗口再放大 1.2×(取更大逻辑盒)。
    for (const [rh, top] of [
      [40, 5000 - 240 * 1.2],
      [200, 5000 - 1200 * 1.2],
      [10, 5000 - 240 * 1.2],
    ] as const) {
      const h = makeHarness({ totalHeight: 500_000, rowHeight: rh })
      h.container.scrollTop = 5000
      await h.vs.updateVisible(true)
      expect(h.fetchCalls[0][0]).toBe(top)
    }
  })

  it('顶部附近 requestTop 钳 0,anchor=0', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 100
    await h.vs.updateVisible(true)
    expect(h.fetchCalls[0][0]).toBe(0)
    expect(h.vs.renderAnchor.value).toBe(0)
  })

  it('小数 scrollTop:fetch 收原值,anchor 取 floor', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000.5
    await h.vs.updateVisible(true)
    expect(h.fetchCalls[0][0]).toBe(4040.5)
    expect(h.vs.renderAnchor.value).toBe(4040)
  })

  it('底部 requestBottom 不钳制(特性化:可超出内容末端)', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 499_000 // = logMax
    await h.vs.updateVisible(true)
    expect(h.fetchCalls[0][1]).toBe(499_000 + 1000 + 960)
  })
})

// ── 契约 3b:Canvas 取行窗(§4.3 S3)────────────────────────────────────────
// Canvas 只枚举已就绪的挂载行备图,故取行窗必须先覆盖位图预取的几何范围;DOM 路径
// (canvasMode 恒 false)保持既有缓冲语义不变,只放大数据窗、不增挂 DOM 行。

describe('canvas 取行窗(§4.3 S3)', () => {
  const VIEW_H = 2160
  const ROW_H = 64

  // node 无 rAF:updateVisible 收尾的 pendingUpdate 派发依赖它,同步执行回调以隔离窗数学。
  beforeEach(() => {
    vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
      void cb()
      return 0
    })
  })

  it('canvas 模式:请求窗覆盖视口 ± 1.25 屏(高视口 2160 + 64px 行高)', async () => {
    const h = makeHarness({ totalHeight: 3_000_000, rowHeight: ROW_H })
    h.canvasMode.value = true
    h.container.clientHeight = VIEW_H
    h.container.scrollTop = 100_000
    await h.vs.updateVisible(true)
    const cover = canvasPrefetchCoveragePx(VIEW_H, ROW_H)
    const [top, bottom] = h.fetchCalls[0]
    expect(top).toBeLessThanOrEqual(100_000 - cover.aheadPx)
    expect(bottom).toBeGreaterThanOrEqual(100_000 + VIEW_H + cover.aheadPx)
  })

  it('DOM 模式(默认)零变化:仍是既有 compact 缓冲数学', async () => {
    const h = makeHarness({ totalHeight: 3_000_000, rowHeight: ROW_H })
    h.container.clientHeight = VIEW_H
    h.container.scrollTop = 100_000
    await h.vs.updateVisible(true)
    const cover = canvasPrefetchCoveragePx(VIEW_H, ROW_H)
    const [top, bottom] = h.fetchCalls[0]
    expect(top).toBeGreaterThan(100_000 - cover.aheadPx)
    expect(bottom).toBeLessThan(100_000 + VIEW_H + cover.aheadPx)
    // 既有 compact 缓冲数学逐字未变:rh=64 → bufferH=max(240, 64×4)=256,取更大盒再 ×1.2
    expect(top).toBe(100_000 - 256 * 1.2)
  })

  it('运行时切到 Canvas:按新窗即时重取,不等下一次滚动', async () => {
    const h = makeHarness({ totalHeight: 3_000_000, rowHeight: ROW_H })
    h.container.clientHeight = VIEW_H
    h.container.scrollTop = 100_000
    await h.vs.updateVisible(true)
    expect(h.fetchCalls.length).toBe(1)
    h.canvasMode.value = true
    await nextTick()
    await new Promise((r) => setTimeout(r, 0)) // 等 updateVisible 的 fetch 落地
    expect(h.fetchCalls.length).toBe(2)
    const cover = canvasPrefetchCoveragePx(VIEW_H, ROW_H)
    expect(h.fetchCalls[1][1]).toBeGreaterThanOrEqual(100_000 + VIEW_H + cover.aheadPx)
  })

  it('视口未测量(viewH=0)时保持既有早退语义', async () => {
    const h = makeHarness({ totalHeight: 3_000_000, rowHeight: ROW_H })
    h.canvasMode.value = true
    h.container.clientHeight = 0
    await h.vs.updateVisible(true)
    expect(h.fetchCalls.length).toBe(0)
  })
})

// ── 契约 4:跳过框 / force / 陈旧 fetch 丢弃 ─────────────────────────────────

describe('fetch 去重生命周期', () => {
  it('窗口未越出上次请求框 → 跳过;force 重置后重取', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    await h.vs.updateVisible(true)
    expect(h.fetchCalls.length).toBe(1)
    await h.vs.updateVisible(false)
    expect(h.fetchCalls.length).toBe(1) // [4200,6800] ⊂ [4040,6960] → skip
    await h.vs.updateVisible(true)
    expect(h.fetchCalls.length).toBe(2) // force 重置 lastFetchedTop
  })

  it('陈旧应答被丢弃:旧批不覆盖新批,isFetching 只由新批清除', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.useDeferred = true
    h.container.scrollTop = 5000
    const rowsA = [normalRow(4100, 200)]
    const rowsB = [normalRow(4100, 200), normalRow(4400, 200)]
    const p1 = h.vs.updateVisible(true)
    const p2 = h.vs.updateVisible(true)
    expect(h.pendingFetches.length).toBe(2)
    h.pendingFetches[0].resolve(rowsA)
    await p1
    expect(h.vs.isFetching.value).toBe(true) // 旧批不清 isFetching
    h.pendingFetches[1].resolve(rowsB)
    await p2
    expect(h.vs.visibleRows.value).toEqual(rowsB)
    expect(h.vs.isFetching.value).toBe(false)
  })

  it('fetch 失败:isFetching 复位、行不变;失败框回滚(同窗可重试,2026-07-06 F1 修复)', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.useDeferred = true
    h.container.scrollTop = 5000
    const p1 = h.vs.updateVisible(true)
    h.pendingFetches[0].reject(new Error('boom'))
    await p1
    expect(h.vs.isFetching.value).toBe(false)
    expect(h.vs.visibleRows.value).toEqual([])
    // 失败已回滚跳过框 → 非 force 同窗重试会真正发起第二次 fetch
    const p2 = h.vs.updateVisible(false)
    expect(h.fetchCalls.length).toBe(2)
    h.pendingFetches[1].resolve([normalRow(4100, 200)])
    await p2
    expect(h.vs.visibleRows.value).toEqual([normalRow(4100, 200)])
  })
})

// ── 契约 5:padding 数学 ─────────────────────────────────────────────────────

describe('padding 计算', () => {
  it('paddingTop=首行 y;paddingBottom=总高-(末行 y+height)', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    h.rowsToReturn = [normalRow(4100, 200), normalRow(6800, 150)]
    await h.vs.updateVisible(true)
    expect(h.vs.paddingTop.value).toBe(4100)
    expect(h.vs.paddingBottom.value).toBe(500_000 - (6800 + 150))
  })

  it('空应答:paddingTop 回退 requestTop', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    h.rowsToReturn = []
    await h.vs.updateVisible(true)
    expect(h.vs.paddingTop.value).toBe(4040)
    expect(h.vs.paddingBottom.value).toBe(500_000 - 4040)
  })

  it('非数值 y/height 防御性归 0', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    h.rowsToReturn = [
      { rowType: 'normal', y: undefined, height: undefined, items: [] } as unknown as LayoutRow,
    ]
    await h.vs.updateVisible(true)
    expect(h.vs.paddingTop.value).toBe(0)
    expect(h.vs.paddingBottom.value).toBe(500_000)
  })
})

// ── 契约 6:syncTransform 钉住公式 + 变更守卫 ─────────────────────────────────

describe('syncTransform:渲染层钉住', () => {
  it('普通模式 δ=0:transform=translate3d(0, renderAnchor px, 0)', async () => {
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    await h.vs.updateVisible(true)
    expect(h.layerStyle.transform).toBe('translate3d(0, 4040px, 0)')
    expect(h.vs.logicalScrollTop.value).toBe(5000)
  })

  it('平移模式 ratio=4:offset = anchor + (phys - logical)', async () => {
    // logMax=39_996_000,physMax=9_999_000 → ratio 精确 =4
    const h = makeHarness({ totalHeight: 39_997_000, rowHeight: 100 })
    h.container.scrollTop = 1_000_000
    await h.vs.updateVisible(true)
    expect(h.vs.logicalScrollTop.value).toBe(4_000_000)
    const anchor = h.vs.renderAnchor.value
    expect(h.layerStyle.transform).toBe(`translate3d(0, ${anchor + 1_000_000 - 4_000_000}px, 0)`)
  })

  it('同 offset 不重复写 style(变更守卫)', () => {
    vi.stubGlobal('requestAnimationFrame', () => 0) // scheduleUpdate no-op,隔离 syncTransform
    const h = makeHarness({ totalHeight: 500_000, rowHeight: 100 })
    h.container.scrollTop = 5000
    h.vs.onScroll()
    h.vs.onScroll()
    expect(h.transformWrites.length).toBe(1)
  })
})

// ── 契约 7:滚轮惯性补偿(仅平移模式) ─────────────────────────────────────────

describe('wheel 惯性补偿', () => {
  async function translatedHarness() {
    // onWheel 尾部也会 scheduleUpdate(rAF):stub 为 no-op,隔离补偿数学本身。
    vi.stubGlobal('requestAnimationFrame', () => 0)
    const h = makeHarness({ totalHeight: 39_997_000, rowHeight: 100 }) // ratio=4 精确
    await h.vs.updateVisible(true)
    await nextTick() // watch(isTranslated) pre-flush → 挂载 wheel 监听
    const call = h.container.addEventListener.mock.calls.find((c) => c[0] === 'wheel')
    expect(call).toBeTruthy()
    expect(call![2]).toEqual({ passive: false })
    return { h, onWheel: call![1] as (e: unknown) => void }
  }

  it('平移模式挂载 {passive:false};退出平移后卸载同一函数', async () => {
    const { h, onWheel } = await translatedHarness()
    h.setTotalHeight(500_000) // 退出平移
    await h.vs.updateVisible(true)
    await nextTick()
    const removed = h.container.removeEventListener.mock.calls.find((c) => c[0] === 'wheel')
    expect(removed).toBeTruthy()
    expect(removed![1]).toBe(onWheel)
  })

  it('deltaMode=0:scrollTop += deltaY/ratio,preventDefault,logicalScrollTop 即时同步', async () => {
    const { h, onWheel } = await translatedHarness()
    h.container.scrollTop = 1000
    const preventDefault = vi.fn()
    onWheel({ deltaY: 100, deltaMode: 0, preventDefault })
    expect(preventDefault).toHaveBeenCalled()
    expect(h.container.scrollTop).toBe(1025)
    expect(h.vs.logicalScrollTop.value).toBe(1025 * 4)
  })

  it('deltaMode=1(行)×16;deltaMode=2(页)×(containerHeight||800)', async () => {
    const { h, onWheel } = await translatedHarness()
    h.container.scrollTop = 0
    onWheel({ deltaY: 3, deltaMode: 1, preventDefault: vi.fn() })
    expect(h.container.scrollTop).toBe(12) // 3*16/4
    onWheel({ deltaY: 1, deltaMode: 2, preventDefault: vi.fn() })
    expect(h.container.scrollTop).toBe(12 + 200) // 1*800/4(containerHeight ref=0 → 800 回退)
  })

  it('普通模式零回归:ratio≤1 时监听器早退、不 preventDefault、不动 scrollTop', async () => {
    const { h, onWheel } = await translatedHarness()
    h.setTotalHeight(500_000) // 几何变普通(监听器仍被捕获在手)
    h.container.scrollTop = 1000
    const preventDefault = vi.fn()
    onWheel({ deltaY: 100, deltaMode: 0, preventDefault })
    expect(preventDefault).not.toHaveBeenCalled()
    expect(h.container.scrollTop).toBe(1000)
  })

  it('physMax≤0(内容矮于视口)同样早退', async () => {
    const { h, onWheel } = await translatedHarness()
    h.setTotalHeight(500)
    const preventDefault = vi.fn()
    onWheel({ deltaY: 100, deltaMode: 0, preventDefault })
    expect(preventDefault).not.toHaveBeenCalled()
  })
})

// ── 契约 8:resolveSafeMax dev 覆盖解析(经 export 缝测试) ─────────────────────

describe.runIf(import.meta.env.DEV)('resolveSafeMax:debug.safeMax 解析', () => {
  it('合法正数 → 覆盖生效', () => {
    vi.stubGlobal('localStorage', { getItem: () => '9000000' })
    expect(resolveSafeMax()).toBe(9_000_000)
  })

  it('null / 非数 / 0 / 负数 → 回退默认 10M', () => {
    for (const raw of [null, 'abc', '0', '-5']) {
      vi.stubGlobal('localStorage', { getItem: () => raw })
      expect(resolveSafeMax()).toBe(SAFE_MAX_DEFAULT)
    }
  })

  it('无 localStorage 全局(node 裸环境)→ try/catch 回退默认', () => {
    vi.unstubAllGlobals()
    expect(resolveSafeMax()).toBe(SAFE_MAX_DEFAULT)
  })
})
