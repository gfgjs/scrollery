// galleryPrefetchWindow(§4.3 S3:位图预取窗 ↔ 行数据窗对齐契约)单测。
//
// 锁三件事:
//  1) 前向覆盖 ≥ 1.25 屏、后向 ≥ 0.5 屏,且随 CSS 视口高线性放大——H=2160 时前向 ≥ 2700px,
//     正是 bucket 既有基线边距(1000px)与方案 A compact 缓冲都给不到的距离;
//  2) 余量恰一行行高(跨界行归其行首 y 所在段),并向上取整;
//  3) 非法输入(视口 ≤ 0 / 行高非法)退化到「不干预」——消费方只做 max,不做反向缩小。
//
// 环境:vitest node,纯函数无 DOM。
import { describe, expect, it } from 'vitest'
import {
  CANVAS_PREFETCH_AHEAD_FACTOR,
  CANVAS_PREFETCH_BEHIND_FACTOR,
  canvasPrefetchCoveragePx,
} from './galleryPrefetchWindow'

describe('canvasPrefetchCoveragePx', () => {
  it('常量契约:前向 1.25 屏 / 后向 0.5 屏(pipeline 位图预取同值)', () => {
    expect(CANVAS_PREFETCH_AHEAD_FACTOR).toBe(1.25)
    expect(CANVAS_PREFETCH_BEHIND_FACTOR).toBe(0.5)
  })

  it('H=2160、行高 64:前向 2700+64,后向 1080+64(余量恰一行)', () => {
    const c = canvasPrefetchCoveragePx(2160, 64)
    expect(c.aheadPx).toBe(2700 + 64)
    expect(c.behindPx).toBe(1080 + 64)
    expect(c.aheadPx).toBeGreaterThan(1000) // 既有 bucket 基线边距给不到
  })

  it('H=2160、行高 200(默认):前向 2900', () => {
    expect(canvasPrefetchCoveragePx(2160, 200).aheadPx).toBe(2700 + 200)
  })

  it('H=1440、行高 64:前向 1800+64 ≥ 1.25 屏', () => {
    const c = canvasPrefetchCoveragePx(1440, 64)
    expect(c.aheadPx).toBe(1800 + 64)
    expect(c.behindPx).toBe(720 + 64)
  })

  it('H=800(普通视口)与 H=2160(4K):单调不减', () => {
    const small = canvasPrefetchCoveragePx(800, 64)
    const large = canvasPrefetchCoveragePx(2160, 64)
    expect(small.aheadPx).toBe(1000 + 64)
    expect(large.aheadPx).toBeGreaterThan(small.aheadPx)
    expect(large.behindPx).toBeGreaterThan(small.behindPx)
  })

  it('小数视口高向上取整(不因取整少于 1.25 屏)', () => {
    expect(canvasPrefetchCoveragePx(1000.5, 64).aheadPx).toBe(Math.ceil(1000.5 * 1.25) + 64)
  })

  it('视口 ≤ 0 / NaN → 0(未测量或失活时不干预既有边距)', () => {
    for (const h of [0, -1, NaN]) {
      expect(canvasPrefetchCoveragePx(h, 64)).toEqual({ aheadPx: 0, behindPx: 0 })
    }
  })

  it('行高非法(≤0 / NaN)只退掉余量,不丢 1.25 屏覆盖', () => {
    for (const rh of [0, -64, NaN]) {
      expect(canvasPrefetchCoveragePx(2160, rh).aheadPx).toBe(2700)
    }
  })
})
