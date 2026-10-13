// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect, vi } from 'vitest'
import { CanvasRenderLifecycle, CanvasRafScheduler } from './mediaGridCanvas.helpers'
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

  describe('failLoad 裁决(0597a19 回归钉)', () => {

    it('陈旧 sig 的迟到失败 → stale:不标失败、不耗自愈额度——新代次不被毒化(0597a19)', () => {
      const { st } = harness()
      st.syncSig(1, 'sigA')
      st.markLoading(1, 'sigA')
      st.syncSig(1, 'sigB') // 在途中换代(自愈复位 → 重生成回填)
      expect(st.failLoad(1, 1, 'sigA')).toBe('stale')
      // 新 sig 不得被旧失败毒化成 failed 短路
      expect(st.isFailed(1)).toBe(false)
      expect(st.isLoading(1)).toBe(false)
      // 新代次的真实失败仍可正常上抛自愈。
      st.markLoading(1, 'sigB')
      expect(st.failLoad(1, 1, 'sigB')).toBe('heal')
    })
  })

})
