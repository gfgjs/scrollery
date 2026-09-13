import { describe, it, expect } from 'vitest'
import {
  hitTestCell,
  hitTestCellWithRow,
  visibleRowRange,
  CanvasRenderLifecycle,
  CanvasRafScheduler,
  canvasPrefetchBudgets,
  canvasPrefetchItems,
  runPrefetchWalk,
  type PrefetchWalkIo,
  coverRect,
  bitmapBucketH,
  bitmapPrepParams,
  BITMAP_HEIGHT_BUCKETS,
  cubicBezierEase,
  SELECT_EASE,
  SelectionAnimTracker,
  truncateToWidth,
  clampStickyLabelY,
  starPoints,
  computeHoverRect,
} from './mediaGridCanvas.helpers'
import type { LayoutRow, LayoutRowItem } from '../../types/layout'

/** 构造一个正常行的最小 item(只填命中相关字段)。 */
function item(id: number, x: number, w: number): LayoutRowItem {
  return {
    id,
    x,
    w,
    h: 60,
    fileSize: 0,
    fileFormat: '',
    mediaType: 'image',
    isLivePhoto: false,
    durationMs: null,
    thumbStatus: 1,
    thumbPath: null,
    placeholderColor: null,
    isFavorited: false,
    rating: 0,
    colorLabel: 0,
    availability: 'online',
    originalWidth: 0,
    originalHeight: 0,
    sortDatetime: 0,
  }
}

const rows: LayoutRow[] = [
  { rowType: 'separator', y: 0, height: 24, separatorLabel: '2026-07' },
  // 行内两格,格间有 4px 空隙(x=0..90, gap, x=94..184)
  { rowType: 'normal', y: 24, height: 60, items: [item(10, 0, 90), item(11, 94, 90)] },
  { rowType: 'normal', y: 84, height: 60, items: [item(20, 0, 120)] },
]

describe('hitTestCell', () => {
  it('命中行内第一格', () => {
    expect(hitTestCell(rows, 10, 30)?.id).toBe(10)
  })
  it('命中行内第二格', () => {
    expect(hitTestCell(rows, 100, 30)?.id).toBe(11)
  })
  it('落在格间空隙 → null', () => {
    expect(hitTestCell(rows, 92, 30)).toBeNull()
  })
  it('命中分隔符行 → null(无可选项)', () => {
    expect(hitTestCell(rows, 10, 5)).toBeNull()
  })
  it('y 越界(下方空白)→ null', () => {
    expect(hitTestCell(rows, 10, 500)).toBeNull()
  })
  it('x 越界(行右侧空白)→ null', () => {
    expect(hitTestCell(rows, 200, 30)).toBeNull()
  })
  it('第二行按 y 区间正确落位', () => {
    expect(hitTestCell(rows, 10, 90)?.id).toBe(20)
  })
  it('大量有序行使用二分命中，不退化为从首行扫描', () => {
    const manyRows: LayoutRow[] = Array.from({ length: 8192 }, (_, i) => ({
      rowType: 'normal' as const,
      y: i * 60,
      height: 60,
      items: [item(i, 0, 50)],
    }))
    let numericReads = 0
    const observed = new Proxy(manyRows, {
      get(target, prop, receiver) {
        if (typeof prop === 'string' && /^\d+$/.test(prop)) numericReads++
        return Reflect.get(target, prop, receiver)
      },
    })
    expect(hitTestCell(observed, 10, 7000 * 60 + 10)?.id).toBe(7000)
    expect(numericReads).toBeLessThan(50)
  })
})

describe('visibleRowRange', () => {
  it('返回与视口相交的半开行区间，并保留贴边行', () => {
    expect(visibleRowRange(rows, 24, 84)).toEqual({ start: 0, end: 3 })
    expect(visibleRowRange(rows, 25, 83)).toEqual({ start: 1, end: 2 })
  })

  it('空输入、反向范围与完全越界均返回空区间', () => {
    expect(visibleRowRange([], 0, 100)).toEqual({ start: 0, end: 0 })
    expect(visibleRowRange(rows, 100, 50)).toEqual({ start: 0, end: 0 })
    expect(visibleRowRange(rows, 500, 600)).toEqual({ start: rows.length, end: rows.length })
  })

  it('大量有序行只读取对数数量的行', () => {
    const manyRows: LayoutRow[] = Array.from({ length: 8192 }, (_, i) => ({
      rowType: 'normal' as const,
      y: i * 60,
      height: 60,
      items: [item(i, 0, 50)],
    }))
    let numericReads = 0
    const observed = new Proxy(manyRows, {
      get(target, prop, receiver) {
        if (typeof prop === 'string' && /^\d+$/.test(prop)) numericReads++
        return Reflect.get(target, prop, receiver)
      },
    })
    expect(visibleRowRange(observed, 7000 * 60 + 1, 7002 * 60 - 1)).toEqual({
      start: 7000,
      end: 7002,
    })
    expect(numericReads).toBeLessThan(50)
  })
})

describe('CanvasRenderLifecycle', () => {
  it('失活会拒绝已排队的旧回调，重新激活只接受新代次', () => {
    const lifecycle = new CanvasRenderLifecycle()
    const first = lifecycle.snapshot()
    expect(lifecycle.canDraw(first)).toBe(true)

    lifecycle.deactivate()
    expect(lifecycle.canDraw(first)).toBe(false)

    lifecycle.activate()
    const current = lifecycle.snapshot()
    expect(lifecycle.canDraw(first)).toBe(false)
    expect(lifecycle.canDraw(current)).toBe(true)
  })
})

describe('CanvasRafScheduler', () => {
  function makeHarness() {
    let nextHandle = 0
    let drawCount = 0
    const lifecycle = new CanvasRenderLifecycle()
    const callbacks = new Map<number, FrameRequestCallback>()
    const canceled: number[] = []
    const scheduler = new CanvasRafScheduler({
      snapshot: () => lifecycle.snapshot(),
      canDraw: (queuedGeneration) => lifecycle.canDraw(queuedGeneration),
      draw: () => {
        drawCount++
      },
      requestFrame: (callback) => {
        const handle = ++nextHandle
        callbacks.set(handle, callback)
        return handle
      },
      cancelFrame: (handle) => {
        canceled.push(handle)
      },
    })
    return {
      scheduler,
      lifecycle,
      callbacks,
      canceled,
      getDrawCount: () => drawCount,
    }
  }

  it('代次切换会替换旧帧，旧帧到达也不会阻塞当前代次', () => {
    const harness = makeHarness()
    expect(harness.scheduler.schedule()).toBe('queued')

    harness.lifecycle.activate()
    expect(harness.scheduler.schedule()).toBe('replaced')
    expect(harness.canceled).toEqual([1])

    harness.callbacks.get(1)?.(0)
    expect(harness.getDrawCount()).toBe(0)
    harness.callbacks.get(2)?.(0)
    expect(harness.getDrawCount()).toBe(1)
  })

  it('同一代次的多个请求仍合并为一帧', () => {
    const harness = makeHarness()
    expect(harness.scheduler.schedule()).toBe('queued')
    expect(harness.scheduler.schedule()).toBe('coalesced')
    expect(harness.canceled).toEqual([])
  })
})

describe('canvasPrefetchBudgets', () => {
  it('大卡片/小视口维持旧下限，避免预取退化', () => {
    expect(canvasPrefetchBudgets(20)).toEqual({ ahead: 96, behind: 48 })
    expect(canvasPrefetchBudgets(0)).toEqual({ ahead: 96, behind: 48 })
  })

  it('60px 极密网格按一屏容量扩到 1.25 屏前瞻与 0.5 屏回看', () => {
    expect(canvasPrefetchBudgets(1200)).toEqual({ ahead: 1500, behind: 600 })
    expect(canvasPrefetchBudgets(2001)).toEqual({ ahead: 2502, behind: 1001 })
  })

  it('异常大视口受条目硬上限约束', () => {
    expect(canvasPrefetchBudgets(10_000)).toEqual({ ahead: 4096, behind: 2048 })
  })

  it('方向系数可调:behindFactor≤0 后方严格为 0(不落旧下限),前向随系数缩放', () => {
    expect(canvasPrefetchBudgets(1200, 1.25, 0)).toEqual({ ahead: 1500, behind: 0 })
    expect(canvasPrefetchBudgets(20, 1, 0)).toEqual({ ahead: 96, behind: 0 })
    expect(canvasPrefetchBudgets(1200, 1, 0.5)).toEqual({ ahead: 1200, behind: 600 })
  })
})

describe('runPrefetchWalk', () => {
  /** 造一个可脚本化的 walk IO:items 序列 + 每项 ensure 后是否转入在途(cold=会启动)。 */
  function makeIo(
    spec: Array<'cold' | 'warm'>,
    opts: { maxInFlightNow?: number; times?: number[]; idle?: number[] } = {},
  ) {
    const loading = new Set<number>()
    let cursor = 0
    let clock = 0
    const times = opts.times ?? null
    let timeIndex = 0
    const idle = opts.idle ?? null
    let idleIndex = 0
    const io: PrefetchWalkIo = {
      next: () => (cursor < spec.length ? item(cursor++, 0, 50) : null),
      // cold 项 ensure 即「发起加载」转入在途;warm 项(已缓存/在途/失败)是 no-op。
      ensure: (it) => {
        if (spec[it.id] === 'cold') loading.add(it.id)
      },
      isLoading: (id) => loading.has(id),
      loadingCount: () => loading.size + (opts.maxInFlightNow ?? 0),
      now: () => (times ? times[Math.min(timeIndex++, times.length - 1)] : clock++ * 0.01),
      idleTimeRemaining: idle ? () => idle[Math.min(idleIndex++, idle.length - 1)] : null,
    }
    return { io, loading }
  }

  it('预算按新启动数计:暖头走查不消耗启动预算,冷尾照常够到', () => {
    const { io } = makeIo(['warm', 'warm', 'warm', 'warm', 'warm', 'cold', 'cold', 'cold'])
    const r = runPrefetchWalk(io, { maxStarts: 2, budgetMs: 100, maxInFlight: 64 })
    expect(r).toEqual({ started: 2, walked: 7, exhausted: false })
  })

  it('游标耗尽报 exhausted,已启动数如实回报', () => {
    const { io, loading } = makeIo(['warm', 'cold', 'warm'])
    const r = runPrefetchWalk(io, { maxStarts: 8, budgetMs: 100, maxInFlight: 64 })
    expect(r).toEqual({ started: 1, walked: 3, exhausted: true })
    expect(Array.from(loading)).toEqual([1])
  })

  it('全局在途上限:入口即满一项不走,途中打满立即停', () => {
    const full = makeIo(['cold', 'cold'], { maxInFlightNow: 64 })
    expect(runPrefetchWalk(full.io, { maxStarts: 8, budgetMs: 100, maxInFlight: 64 })).toEqual({
      started: 0,
      walked: 0,
      exhausted: false,
    })
    const nearFull = makeIo(['cold', 'cold', 'cold'], { maxInFlightNow: 62 })
    const r = runPrefetchWalk(nearFull.io, { maxStarts: 8, budgetMs: 100, maxInFlight: 64 })
    expect(r.started).toBe(2) // 62+2=64 → 第三项前停
    expect(r.walked).toBe(2)
  })

  it('墙钟预算到即停(即便启动预算未用完)', () => {
    // now 序列:起点 0,第一次循环检查 1,第二次 6(≥5 预算)→ 停在 walked=1。
    const { io } = makeIo(['cold', 'cold', 'cold'], { times: [0, 1, 6] })
    const r = runPrefetchWalk(io, { maxStarts: 8, budgetMs: 5, maxInFlight: 64 })
    expect(r).toEqual({ started: 1, walked: 1, exhausted: false })
  })

  it('idle deadline 余量 ≤1ms 立即停;充足则不干预', () => {
    const starved = makeIo(['cold'], { idle: [0.5] })
    expect(runPrefetchWalk(starved.io, { maxStarts: 8, budgetMs: 100, maxInFlight: 64 })).toEqual({
      started: 0,
      walked: 0,
      exhausted: false,
    })
    const roomy = makeIo(['cold'], { idle: [10] })
    expect(runPrefetchWalk(roomy.io, { maxStarts: 8, budgetMs: 100, maxInFlight: 64 }).started).toBe(1)
  })
})

describe('canvasPrefetchItems', () => {
  const prefetchRows: LayoutRow[] = [
    { rowType: 'normal', y: 0, height: 60, items: [item(1, 0, 50), item(2, 54, 50)] },
    { rowType: 'separator', y: 60, height: 24, separatorLabel: 'sep' },
    { rowType: 'normal', y: 84, height: 60, items: [item(3, 0, 50), item(4, 54, 50)] },
    { rowType: 'normal', y: 144, height: 60, items: [item(5, 0, 50), item(6, 54, 50)] },
  ]

  it('iterator 跨分片续跑，不从首行重新开始', () => {
    const iterator = canvasPrefetchItems(prefetchRows, 0, 1, 0, 60, 200, 5)
    expect([iterator.next().value?.id, iterator.next().value?.id]).toEqual([1, 2])
    expect(Array.from(iterator, (entry) => entry.id)).toEqual([3, 4, 5])
  })

  it('反向枚举并同时受像素边界和条目预算约束', () => {
    expect(
      Array.from(canvasPrefetchItems(prefetchRows, 3, -1, 144, 60, 70, 3), (entry) => entry.id),
    ).toEqual([5, 6, 3])
    expect(
      Array.from(canvasPrefetchItems(prefetchRows, 0, 1, 0, 60, 10, 99), (entry) => entry.id),
    ).toEqual([1, 2])
  })
})

describe('coverRect', () => {
  it('正方形铺正方形 → 不裁剪', () => {
    const r = coverRect(100, 100, 50, 50)
    expect(r).toEqual({ sx: 0, sy: 0, sw: 100, sh: 100 })
  })
  it('横图铺正方形 → 裁两侧,高不裁', () => {
    const r = coverRect(200, 100, 50, 50) // scale=max(0.25,0.5)=0.5 → sw=100,sh=200? no
    // w/nw=0.25, h/nh=0.5 → scale=0.5 → sw=50/0.5=100, sh=50/0.5=100
    expect(r.sh).toBeCloseTo(100)
    expect(r.sw).toBeCloseTo(100)
    expect(r.sx).toBeCloseTo(50) // (200-100)/2
    expect(r.sy).toBeCloseTo(0)
  })
  it('竖图铺正方形 → 裁上下', () => {
    const r = coverRect(100, 200, 50, 50)
    expect(r.sx).toBeCloseTo(0)
    expect(r.sy).toBeCloseTo(50)
  })
  it('零/负尺寸 → 退化不裁剪,不产生 NaN', () => {
    const r = coverRect(0, 100, 50, 50)
    expect(Number.isNaN(r.sw)).toBe(false)
    expect(r).toEqual({ sx: 0, sy: 0, sw: 0, sh: 100 })
  })
})

// 解码期预缩放(createImageBitmap 管线)的纯数学。
describe('bitmapBucketH', () => {
  it('取 ≥ 目标的最小桶(格高 × DPR)', () => {
    expect(bitmapBucketH(60, 1)).toBe(64) // 60 → 64
    expect(bitmapBucketH(60, 2)).toBe(128) // 120 → 128
    expect(bitmapBucketH(65, 1)).toBe(96) // 越过 64 边界
    expect(bitmapBucketH(100, 1.5)).toBe(192) // 150 → 192
  })
  it('恰在桶边界 → 落本桶(<= 语义)', () => {
    expect(bitmapBucketH(96, 1)).toBe(96)
    expect(bitmapBucketH(64, 2)).toBe(128)
  })
  it('超顶桶(源上限 480)→ 取顶桶', () => {
    expect(bitmapBucketH(500, 2)).toBe(480)
    expect(bitmapBucketH(480, 1)).toBe(480)
  })
  it('阶梯单调递增(卫生检查)', () => {
    for (let i = 1; i < BITMAP_HEIGHT_BUCKETS.length; i++) {
      expect(BITMAP_HEIGHT_BUCKETS[i]).toBeGreaterThan(BITMAP_HEIGHT_BUCKETS[i - 1])
    }
  })
})

describe('bitmapPrepParams', () => {
  it('横图入方格:裁两侧到方形,超桶则缩到桶高、目标为格纵横比', () => {
    // coverRect(480,320,100,100): scale=max(100/480,100/320)=0.3125 → 裁 320×320,sx=80
    const p = bitmapPrepParams(480, 320, 100, 100, 128)
    expect({ sx: p.sx, sy: p.sy, sw: p.sw, sh: p.sh }).toEqual({ sx: 80, sy: 0, sw: 320, sh: 320 })
    expect(p.outW).toBe(128) // 方格 → outW = outH
    expect(p.outH).toBe(128)
  })
  it('justified 格(纵横比=图片纵横比):全源零裁剪,等比缩到桶高', () => {
    const p = bitmapPrepParams(480, 320, 180, 120, 256) // 格 1.5 = 源 1.5
    expect({ sx: p.sx, sy: p.sy, sw: p.sw, sh: p.sh }).toEqual({ sx: 0, sy: 0, sw: 480, sh: 320 })
    expect(p.outH).toBe(256)
    expect(p.outW).toBe(384) // 256 × 180/120,纵横比保持 1.5
  })
  it('裁剪后已 ≤ 桶高 → 只裁不缩(outW/outH = null,只缩不放)', () => {
    const p = bitmapPrepParams(120, 80, 100, 100, 128)
    expect(p.outW).toBeNull()
    expect(p.outH).toBeNull()
    expect({ sx: p.sx, sy: p.sy, sw: p.sw, sh: p.sh }).toEqual({ sx: 20, sy: 0, sw: 80, sh: 80 })
  })
  it('裁剪矩形整数化且钳位在源界内(createImageBitmap 参数为 long)', () => {
    const p = bitmapPrepParams(333, 217, 90, 61, 96) // 刻意取不整除组合
    expect(Number.isInteger(p.sx) && Number.isInteger(p.sy)).toBe(true)
    expect(Number.isInteger(p.sw) && Number.isInteger(p.sh)).toBe(true)
    expect(p.sx + p.sw).toBeLessThanOrEqual(333)
    expect(p.sy + p.sh).toBeLessThanOrEqual(217)
    expect(p.sw).toBeGreaterThan(0)
    expect(p.sh).toBeGreaterThan(0)
  })
  it('格高 0(退化)→ 只裁不缩,不产生 NaN', () => {
    const p = bitmapPrepParams(480, 320, 100, 0, 128)
    expect(p.outW).toBeNull()
    expect(p.outH).toBeNull()
    expect(Number.isNaN(p.sw)).toBe(false)
  })
})

describe('hitTestCellWithRow', () => {
  it('命中返回项 + 所在行逻辑 y(悬停卡定位用)', () => {
    const hit = hitTestCellWithRow(rows, 10, 90)
    expect(hit?.item.id).toBe(20)
    expect(hit?.rowY).toBe(84)
  })
  it('未命中(空隙/分隔符/越界)→ null,与 hitTestCell 一致', () => {
    expect(hitTestCellWithRow(rows, 92, 30)).toBeNull()
    expect(hitTestCellWithRow(rows, 10, 5)).toBeNull()
    expect(hitTestCellWithRow(rows, 10, 500)).toBeNull()
  })
})

describe('cubicBezierEase', () => {
  it('端点恒等:x≤0→0,x≥1→1', () => {
    const ease = cubicBezierEase(0.34, 1.18, 0.64, 1)
    expect(ease(0)).toBe(0)
    expect(ease(1)).toBe(1)
    expect(ease(-0.5)).toBe(0)
    expect(ease(1.5)).toBe(1)
  })
  it('线性控制点 → 恒等函数(数值容差)', () => {
    const ease = cubicBezierEase(0.25, 0.25, 0.75, 0.75)
    for (const x of [0.1, 0.3, 0.5, 0.7, 0.9]) {
      expect(ease(x)).toBeCloseTo(x, 3)
    }
  })
  it('ease-out 型曲线中段快于线性', () => {
    const ease = cubicBezierEase(0, 0, 0.58, 1) // CSS ease-out
    expect(ease(0.5)).toBeGreaterThan(0.5)
  })
  it('DOM 选中曲线(y1=1.18)存在过冲(某处 y>1)', () => {
    let overshoot = false
    for (let x = 0.05; x < 1; x += 0.05) {
      if (SELECT_EASE(x) > 1) overshoot = true
    }
    expect(overshoot).toBe(true)
  })
})

describe('SelectionAnimTracker', () => {
  const isSel = (set: Set<number>) => (id: number) => set.has(id)
  const vis = (...ids: number[]) => ids.map((id) => ({ id }))
  const linear = (x: number) => x
  const DUR = 250

  it('首次入场无动画:degree 直接返回终态', () => {
    const t = new SelectionAnimTracker()
    t.sync(vis(1, 2), isSel(new Set([1])), 0)
    expect(t.degree(1, true, 0, linear, DUR)).toBe(1)
    expect(t.degree(2, false, 0, linear, DUR)).toBe(0)
    expect(t.hasActive(0, DUR)).toBe(false)
  })
  it('可见项选中翻转 → 起动画并按 ease 插值,到时收敛终态', () => {
    const t = new SelectionAnimTracker()
    t.sync(vis(1), isSel(new Set()), 0) // 基线:未选中
    t.sync(vis(1), isSel(new Set([1])), 1000) // 翻转为选中
    expect(t.degree(1, true, 1125, linear, DUR)).toBeCloseTo(0.5)
    expect(t.hasActive(1125, DUR)).toBe(true)
    expect(t.degree(1, true, 1300, linear, DUR)).toBe(1) // 超时 → 终态并出队
    expect(t.hasActive(1300, DUR)).toBe(false)
  })
  it('取消选中的反向动画:degree 从 1 落向 0', () => {
    const t = new SelectionAnimTracker()
    t.sync(vis(1), isSel(new Set([1])), 0)
    t.sync(vis(1), isSel(new Set()), 1000)
    expect(t.degree(1, false, 1125, linear, DUR)).toBeCloseTo(0.5)
    expect(t.degree(1, false, 1250, linear, DUR)).toBe(0)
  })
  it('离屏发生的选区变化不回放动画(快照纪律):滚入即终态', () => {
    const t = new SelectionAnimTracker()
    t.sync(vis(1), isSel(new Set()), 0) // id=2 离屏,不在快照
    t.sync(vis(1), isSel(new Set([2])), 1000) // 全选发生时 2 仍离屏
    t.sync(vis(1, 2), isSel(new Set([2])), 2000) // 2 滚入:首见即终态
    expect(t.degree(2, true, 2001, linear, DUR)).toBe(1)
    expect(t.hasActive(2001, DUR)).toBe(false)
  })
})

describe('truncateToWidth', () => {
  const byChars = (s: string) => s.length * 10 // 每字符 10px 的假量宽
  it('宽度足够 → 原样返回', () => {
    expect(truncateToWidth('hello', 100, byChars)).toBe('hello')
  })
  it('超宽 → 截断缀省略号且不超宽', () => {
    const out = truncateToWidth('hello world', 60, byChars)
    expect(out.endsWith('…')).toBe(true)
    expect(byChars(out)).toBeLessThanOrEqual(60)
    expect(out).toBe('hello…')
  })
  it('极窄 → 仅省略号;maxW≤0 → 空串', () => {
    expect(truncateToWidth('hello', 10, byChars)).toBe('…')
    expect(truncateToWidth('hello', 0, byChars)).toBe('')
  })
})

describe('clampStickyLabelY', () => {
  it('行在视口内 → 用流内位置', () => {
    expect(clampStickyLabelY(100, 200, 26)).toBe(100)
  })
  it('行顶滚出视口 → 钉在 0(sticky)', () => {
    expect(clampStickyLabelY(-50, 200, 26)).toBe(0)
  })
  it('行将滚尽 → 钳在行底上沿(标签不越出行)', () => {
    expect(clampStickyLabelY(-100, 20, 26)).toBe(-6) // 20-26
  })
  it('行高不足容纳标签 → 退回流内位置', () => {
    expect(clampStickyLabelY(10, 20, 26)).toBe(10)
  })
})

describe('starPoints', () => {
  it('5 角星 → 10 顶点,首点朝正上', () => {
    const pts = starPoints(50, 50, 10)
    expect(pts).toHaveLength(10)
    expect(pts[0][0]).toBeCloseTo(50)
    expect(pts[0][1]).toBeCloseTo(40) // cy - outerR
  })
  it('顶点交替落在外/内半径圆周上', () => {
    const pts = starPoints(0, 0, 10, 0.5)
    for (let i = 0; i < pts.length; i++) {
      const r = Math.hypot(pts[i][0], pts[i][1])
      expect(r).toBeCloseTo(i % 2 === 0 ? 10 : 5)
    }
  })
})

describe('computeHoverRect', () => {
  it('常规格:1.06 倍居中放大(对齐 DOM hover),基点=格中心在卡内坐标', () => {
    const r = computeHoverRect({ x: 400, y: 300, w: 200, h: 200 }, 1920, 1080)
    expect(r.w).toBeCloseTo(212)
    expect(r.h).toBeCloseTo(212)
    expect(r.x).toBeCloseTo(394) // 居中:x - 6
    expect(r.scale0).toBeCloseTo(1 / 1.06)
    expect(r.originX).toBeCloseTo(106) // 500 - 394,未钳位时=卡中心
    expect(r.originY).toBeCloseTo(106)
  })
  it('关闭悬停放大设置 → 保持原格尺寸与位置,基点=格中心', () => {
    const r = computeHoverRect(
      { x: 400, y: 300, w: 200, h: 200 },
      1920,
      1080,
      120,
      1.06,
      1.2,
      false,
    )
    expect(r).toEqual({ x: 400, y: 300, w: 200, h: 200, scale0: 1, originX: 100, originY: 100 })
  })
  it('原位态(k=1)不钳位:贴视口缘的格保持在原格上(DOM 模式 hover 从不挪格)', () => {
    // 首行滚出视口顶 → 旧实现会把卡推到 y=0,卡与画格错位 = 关闭放大仍位移的几何来源。
    const top = computeHoverRect({ x: 100, y: -30, w: 200, h: 200 }, 800, 600, 120, 1.06, 1.2, false)
    expect(top).toEqual({ x: 100, y: -30, w: 200, h: 200, scale0: 1, originX: 100, originY: 100 })
    // 尾行探出视口底同理(旧实现 y 被钳到 viewH-h,整格上移)。
    const bottom = computeHoverRect({ x: 100, y: 550, w: 200, h: 200 }, 800, 600, 120, 1.06, 1.2, false)
    expect(bottom.y).toBe(550)
  })
  it('放大态贴视口缘仍钳位(向视口内推挤),与原位态区分', () => {
    const r = computeHoverRect({ x: 100, y: -30, w: 200, h: 200 }, 800, 600)
    expect(r.y).toBeGreaterThanOrEqual(0)
    expect(r.scale0).not.toBe(1)
  })
  it('极小格:倍率封顶 1.2(真机反馈 1.5 仍突兀)', () => {
    const r = computeHoverRect({ x: 500, y: 500, w: 60, h: 60 }, 1920, 1080)
    expect(Math.min(r.w, r.h)).toBeCloseTo(72) // 60 × 1.2,未到 120 即被顶住
    expect(r.scale0).toBeCloseTo(1 / 1.2)
  })
  it('中等格:未触顶时放大到最小可用尺寸 120(短边)', () => {
    const r = computeHoverRect({ x: 500, y: 500, w: 110, h: 110 }, 1920, 1080)
    expect(Math.min(r.w, r.h)).toBeCloseTo(120) // k = 120/110 ≈ 1.09 < 1.2
  })
  it('贴边格:视口内推挤钳位,基点偏离卡中心(动画起点仍与画格重合)', () => {
    const r = computeHoverRect({ x: 0, y: 0, w: 60, h: 60 }, 1920, 1080)
    expect(r.x).toBeGreaterThanOrEqual(0)
    expect(r.y).toBeGreaterThanOrEqual(0)
    expect(r.originX).toBeCloseTo(30 - r.x) // 格中心 30 − 钳位后矩形左缘
    expect(r.originY).toBeCloseTo(30 - r.y)
    expect(r.originX).not.toBeCloseTo(r.w / 2) // 钳位后确实偏离卡中心
    const r2 = computeHoverRect({ x: 1860, y: 1020, w: 60, h: 60 }, 1920, 1080)
    expect(r2.x + r2.w).toBeLessThanOrEqual(1920)
    expect(r2.y + r2.h).toBeLessThanOrEqual(1080)
  })
  it('等比放大:纵横比不变', () => {
    const r = computeHoverRect({ x: 500, y: 500, w: 90, h: 60 }, 1920, 1080)
    expect(r.w / r.h).toBeCloseTo(1.5)
    expect(Math.min(r.w, r.h)).toBeCloseTo(72) // 短边 60 × 1.2 封顶
  })
  it('目标大于视口 → 居中', () => {
    const r = computeHoverRect({ x: 0, y: 0, w: 100, h: 100 }, 120, 120)
    expect(r.x).toBeCloseTo((120 - r.w) / 2)
  })
})
