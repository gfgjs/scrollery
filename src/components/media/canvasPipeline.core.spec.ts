// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect, vi } from 'vitest'
import { CanvasRenderLifecycle, CanvasRafScheduler,  } from './mediaGridCanvas.helpers'
import { createCanvasThumbState } from './canvasThumbState'

describe('帧调度与预取', () => {

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
