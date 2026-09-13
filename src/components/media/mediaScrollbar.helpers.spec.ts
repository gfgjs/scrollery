// T16 B3.2:自研逻辑滚动条几何单测——拇指几何/最小高钳制/拖拽映射互逆。
import { describe, it, expect } from 'vitest'
import { thumbGeometry, thumbTopToLogicalY, MIN_THUMB_PX } from './mediaScrollbar.helpers'

describe('mediaScrollbar.helpers', () => {
  it('内容不足一屏/轨道无效 → null(含 NaN 防御)', () => {
    expect(thumbGeometry(0, 500, 1000)).toBeNull()
    expect(thumbGeometry(0, 1000, 1000)).toBeNull()
    expect(thumbGeometry(0, 1000, 0)).toBeNull()
    expect(thumbGeometry(0, NaN, 1000)).toBeNull()
  })

  it('比例拇指:高 = 轨道 × 视口/总高;顶/底精确贴轨道两端', () => {
    const top = thumbGeometry(0, 4000, 1000, 0)!
    expect(top.height).toBeCloseTo(250, 8) // 1000/4000 × 1000
    expect(top.top).toBe(0)
    const bottom = thumbGeometry(3000, 4000, 1000, 0)! // maxY = 4000 − 1000
    expect(bottom.top + bottom.height).toBeCloseTo(1000, 8)
  })

  it('百万级库:拇指钳到最小高,位置仍为行程比例(中点居中)', () => {
    const total = 30_000_000
    const trackH = 1000
    const mid = (total - trackH) / 2
    const g = thumbGeometry(mid, total, trackH)!
    expect(g.height).toBe(MIN_THUMB_PX) // 纯比例高 0.03px → 钳制
    expect(g.top).toBeCloseTo((trackH - MIN_THUMB_PX) / 2, 6)
  })

  it('越界钳制:负逻辑位贴顶、超底逻辑位贴底', () => {
    const top = thumbGeometry(-100, 4000, 1000, 0)!
    expect(top.top).toBe(0)
    const over = thumbGeometry(999_999, 4000, 1000, 0)!
    expect(over.top + over.height).toBeCloseTo(1000, 8)
  })

  it('拖拽映射与拇指几何互逆(round-trip,含钳高形态)', () => {
    const total = 30_000_000
    const trackH = 900
    for (const y of [0, 123_456, 15_000_000, total - trackH]) {
      const g = thumbGeometry(y, total, trackH)!
      expect(thumbTopToLogicalY(g.top, total, trackH, g.height)).toBeCloseTo(y, 4)
    }
  })

  it('拖拽映射:越界钳制与退化(拇指占满轨道 → 恒 0)', () => {
    expect(thumbTopToLogicalY(-50, 30_000_000, 900, 32)).toBe(0)
    expect(thumbTopToLogicalY(1e9, 30_000_000, 900, 32)).toBe(30_000_000 - 900)
    expect(thumbTopToLogicalY(10, 2000, 1000, 1000)).toBe(0)
  })

  it('固定指针模式(fixedThumb):高度恒定,不随比例/最小高计算', () => {
    const g = thumbGeometry(0, 10000, 500, MIN_THUMB_PX, 48)!
    expect(g.height).toBe(48)
  })

  it('固定指针模式:高度钳到轨道高上限', () => {
    const g = thumbGeometry(0, 100, 40, MIN_THUMB_PX, 48)!
    expect(g.height).toBe(40)
  })

  it('固定指针模式:round-trip 与比例模式互逆', () => {
    const total = 30_000_000
    const trackH = 900
    for (const y of [0, 123_456, 15_000_000, total - trackH]) {
      const g = thumbGeometry(y, total, trackH, MIN_THUMB_PX, 48)!
      expect(g.height).toBe(48)
      expect(thumbTopToLogicalY(g.top, total, trackH, g.height)).toBeCloseTo(y, 4)
    }
  })

  it('固定指针模式:内容不足一屏仍返回 null', () => {
    expect(thumbGeometry(0, 500, 1000, MIN_THUMB_PX, 48)).toBeNull()
  })
})
