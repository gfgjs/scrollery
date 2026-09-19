// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect, vi } from 'vitest'
import { CanvasRenderLifecycle, CanvasRafScheduler, canvasPrefetchItems, runPrefetchWalk, type PrefetchWalkIo } from './mediaGridCanvas.helpers'
import { type LayoutRow, type LayoutRowItem } from '../../types/layout'
import { createCanvasThumbState } from './canvasThumbState'

describe('帧调度与预取', () => {
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
})

describe('缩略图缓存生命周期', () => {
  interface FakeSrc {
    tag: string
  }

  function harness(maxCache = 3) {
    const closed: string[] = []
    const closeSrc = vi.fn((s: FakeSrc) => closed.push(s.tag))
    const st = createCanvasThumbState<FakeSrc>(maxCache, closeSrc)
    return { st, closed, closeSrc }
  }

  const ent = (tag: string, renderSig = 'r1') => ({ src: { tag }, renderSig })

  describe('LRU 语义', () => {
    it('命中即重插队尾:被 get 触碰的条目躲过驱逐,最久未用者出局并被释放', () => {
      const { st, closed } = harness(3)
      st.syncSig(1, 's')
      st.syncSig(2, 's')
      st.syncSig(3, 's')
      st.syncSig(4, 's')
      expect(st.commitLoad(1, 's', ent('a'))).toBe(true)
      expect(st.commitLoad(2, 's', ent('b'))).toBe(true)
      expect(st.commitLoad(3, 's', ent('c'))).toBe(true)
      st.get(1) // 触碰 1 → 最久未用变成 2
      st.commitLoad(4, 's', ent('d'))
      expect(st.size()).toBe(3)
      // 驱逐最久未用的 2 并 closeSrc
      expect(closed).toEqual(['b'])
      expect(st.get(1)?.src.tag).toBe('a')
      expect(st.get(2)).toBeUndefined()
    })
  })

  describe('sig 换代硬失效', () => {
    it('sig 变化:清失败/请求/自愈标记,关闭并作废缓存;同 sig 幂等不动', () => {
      const { st, closed } = harness()
      st.syncSig(1, 'sigA')
      st.commitLoad(1, 'sigA', ent('a'))
      expect(st.requestThumbOnce(1)).toBe(true)
      st.failLoad(1, 1, 'sigA')
      expect(st.isFailed(1)).toBe(true)

      st.syncSig(1, 'sigA') // 同 sig:全部保持
      expect(st.isFailed(1)).toBe(true)
      expect(st.requestThumbOnce(1)).toBe(false)
      expect(closed).toEqual([])

      st.syncSig(1, 'sigB') // 换代:全清
      expect(closed).toEqual(['a'])
      expect(st.get(1)).toBeUndefined()
      expect(st.isFailed(1)).toBe(false)
      expect(st.requestThumbOnce(1)).toBe(true)
      // 换代后 status=1 失败可再次上抛自愈(requestedHealSet 已清)。
      expect(st.failLoad(1, 1, 'sigB')).toBe('heal')
    })
  })

  describe('commitLoad 陈旧守卫', () => {
    it('现行 sig:清在途、落缓存、返回 true;陈旧 sig:关闭产物、拒收、返回 false', () => {
      const { st, closed } = harness()
      st.syncSig(1, 'sigA')
      st.markLoading(1)
      st.syncSig(1, 'sigB') // 在途中数据换代
      expect(st.commitLoad(1, 'sigA', ent('stale'))).toBe(false)
      expect(closed).toEqual(['stale'])
      expect(st.get(1)).toBeUndefined()
      // 无论新旧,在途标记都清(该 id 的加载已终结)
      expect(st.isLoading(1)).toBe(false)

      st.markLoading(1)
      expect(st.commitLoad(1, 'sigB', ent('fresh'))).toBe(true)
      expect(st.get(1)?.src.tag).toBe('fresh')
      expect(st.isLoading(1)).toBe(false)
    })
  })

  describe('loadingCount', () => {
    it('主动取消只释放在途槽，不污染失败态', () => {
      const st = createCanvasThumbState<FakeSrc>(10, () => {})
      st.syncSig(1, 'a')
      st.markLoading(1)
      expect(st.cancelLoad(1)).toBe(true)
      expect(st.loadingCount()).toBe(0)
      expect(st.isFailed(1)).toBe(false)
      expect(st.cancelLoad(1)).toBe(false)
    })
  })

  describe('failLoad 裁决(0597a19 回归钉)', () => {
    it('status=1 首败 → heal(上抛自愈)且标失败;复败 → fail(自愈只上抛一次)', () => {
      const { st } = harness()
      st.syncSig(1, 's')
      expect(st.failLoad(1, 1, 's')).toBe('heal')
      expect(st.isFailed(1)).toBe(true)
      expect(st.failLoad(1, 1, 's')).toBe('fail')
    })

    it('陈旧 sig 的迟到失败 → stale:不标失败、不耗自愈额度——新代次不被毒化(0597a19)', () => {
      const { st } = harness()
      st.syncSig(1, 'sigA')
      st.markLoading(1)
      st.syncSig(1, 'sigB') // 在途中换代(自愈复位 → 重生成回填)
      expect(st.failLoad(1, 1, 'sigA')).toBe('stale')
      // 新 sig 不得被旧失败毒化成 failed 短路
      expect(st.isFailed(1)).toBe(false)
      expect(st.isLoading(1)).toBe(false)
      // 新代次的真实失败仍可正常上抛自愈。
      expect(st.failLoad(1, 1, 'sigB')).toBe('heal')
    })
  })

  // ── 字节预算(2026-07-10 审查 B11)+ syncSig 清在途(C24) ─────────────────────────
  interface SizedSrc {
    tag: string
    cost: number
  }

  function budgetHarness(maxCache: number, maxBytes: number) {
    const closed: string[] = []
    const st = createCanvasThumbState<SizedSrc>(maxCache, (s) => closed.push(s.tag), {
      maxBytes,
      byteCost: (s) => s.cost,
    })
    return { st, closed }
  }

  const sent = (tag: string, cost: number, renderSig = 'r1') => ({ src: { tag, cost }, renderSig })

  describe('字节预算双约束(B11)', () => {
    it('累计超 maxBytes:条目数未超也从队首驱逐最久未用,账面同步扣减', () => {
      const { st, closed } = budgetHarness(10, 100)
      st.syncSig(1, 's')
      st.syncSig(2, 's')
      st.syncSig(3, 's')
      st.commitLoad(1, 's', sent('a', 60))
      st.commitLoad(2, 's', sent('b', 30))
      expect(st.bytes()).toBe(90)
      st.commitLoad(3, 's', sent('c', 30)) // 120 > 100 → 驱逐队首 a 后 60 ≤ 100 停
      expect(closed).toEqual(['a'])
      expect(st.size()).toBe(2)
      expect(st.bytes()).toBe(60)
    })

    it('单条即超预算:绝不驱逐刚插入项(独存),下一条插入时才轮到它出局', () => {
      const { st, closed } = budgetHarness(10, 100)
      st.syncSig(1, 's')
      st.syncSig(2, 's')
      st.commitLoad(1, 's', sent('big', 150))
      expect(closed).toEqual([]) // 独存,否则该格永远画不出来
      expect(st.bytes()).toBe(150)
      st.commitLoad(2, 's', sent('d', 10))
      expect(closed).toEqual(['big'])
      expect(st.bytes()).toBe(10)
    })
  })

  describe('clear', () => {
    it('释放全部 src 并清空所有状态(卸载路径)', () => {
      const { st, closed } = harness()
      st.syncSig(1, 's')
      st.syncSig(2, 's')
      st.commitLoad(1, 's', ent('a'))
      st.commitLoad(2, 's', ent('b'))
      st.markLoading(3)
      st.clear()
      expect(closed.sort()).toEqual(['a', 'b'])
      expect(st.size()).toBe(0)
      expect(st.isLoading(3)).toBe(false)
      // sigMap 亦清:同 id 再入场按新代次从头来。
      expect(st.requestThumbOnce(1)).toBe(true)
    })
  })
})
