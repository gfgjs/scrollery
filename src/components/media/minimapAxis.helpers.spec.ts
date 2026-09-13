// 无缝 minimap 轴几何单测:滑窗/非滑窗两形态、框-内容对齐不变量、拖拽映射互逆、点击居中。
import { describe, it, expect } from 'vitest'
import {
  minimapScale,
  minimapWindow,
  minimapSlider,
  minimapThumbSig,
  sliderTopToLogicalY,
  clickToLogicalY,
  keyboardToLogicalY,
  minimapThumbRetryDelay,
  MIN_SLIDER_PX,
  normalizeWheelDelta,
  wheelToLogicalY,
} from './minimapAxis.helpers'

// 公共几何:轴宽 96 / 内容宽 1200 → scale 0.08;轨道 900;视口 1000。
const SCALE = minimapScale(96, 1200)
const TRACK = 900
const VIEW = 1000

describe('minimapAxis.helpers', () => {
  it('minimapScale:无效入参(0/负/NaN)→ 0;有效 → 比值', () => {
    expect(SCALE).toBeCloseTo(0.08, 12)
    expect(minimapScale(0, 1200)).toBe(0)
    expect(minimapScale(96, 0)).toBe(0)
    expect(minimapScale(NaN, 1200)).toBe(0)
  })

  it('minimapWindow:无效入参 → null', () => {
    expect(minimapWindow(0, 5000, VIEW, 0, SCALE)).toBeNull()
    expect(minimapWindow(0, 0, VIEW, TRACK, SCALE)).toBeNull()
    expect(minimapWindow(0, 5000, VIEW, TRACK, 0)).toBeNull()
  })

  it('非滑动形态(mini内容 ≤ 轨道):窗覆盖全量', () => {
    // 5000×0.08 = 400 ≤ 900
    const w = minimapWindow(2000, 5000, VIEW, TRACK, SCALE)!
    expect(w.topY).toBe(0)
    expect(w.bottomY).toBe(5000)
  })

  it('滑动形态:顶端窗顶=0,底端窗底=总高,窗跨度恒 = 轨道/scale', () => {
    // 100000×0.08 = 8000 > 900 → 滑窗
    const total = 100_000
    const top = minimapWindow(0, total, VIEW, TRACK, SCALE)!
    expect(top.topY).toBe(0)
    const bottom = minimapWindow(total - VIEW, total, VIEW, TRACK, SCALE)!
    expect(bottom.bottomY).toBeCloseTo(total, 6)
    for (const y of [0, 33_000, total - VIEW]) {
      const w = minimapWindow(y, total, VIEW, TRACK, SCALE)!
      expect(w.bottomY - w.topY).toBeCloseTo(TRACK / SCALE, 6)
    }
  })

  it('框-内容对齐不变量:slider.top = (currentY − 窗顶)×scale(未钳高形态)', () => {
    for (const [total, y] of [
      [5000, 2000], // 非滑动
      [100_000, 50_000], // 滑动
    ]) {
      const w = minimapWindow(y, total, VIEW, TRACK, SCALE)!
      const g = minimapSlider(y, total, VIEW, TRACK, SCALE)!
      expect(g.height).toBeCloseTo(VIEW * SCALE, 8) // 80 > MIN_SLIDER_PX,未钳
      expect(g.top).toBeCloseTo((y - w.topY) * SCALE, 8)
    }
  })

  it('minimapSlider:内容不足一屏 → null(无可滚动即无框)', () => {
    expect(minimapSlider(0, 800, VIEW, TRACK, SCALE)).toBeNull()
    expect(minimapSlider(0, VIEW, VIEW, TRACK, SCALE)).toBeNull()
  })

  it('深库:框高钳到 MIN_SLIDER_PX,框仍在轨道内', () => {
    // 30M 高、scale 6.4e-4 模拟窄内容宽 → viewport×scale < 16
    const s = minimapScale(96, 150_000)
    const g = minimapSlider(15_000_000, 30_000_000, VIEW, TRACK, s)!
    expect(g.height).toBe(MIN_SLIDER_PX)
    expect(g.top).toBeGreaterThanOrEqual(0)
    expect(g.top + g.height).toBeLessThanOrEqual(TRACK)
  })

  it('拖拽映射互逆:两形态 round-trip(未钳高)', () => {
    for (const [total, y] of [
      [5000, 0],
      [5000, 2000],
      [5000, 4000], // 非滑动含端点
      [100_000, 0],
      [100_000, 50_000],
      [100_000, 99_000], // 滑动含端点
    ]) {
      const g = minimapSlider(y, total, VIEW, TRACK, SCALE)!
      expect(sliderTopToLogicalY(g.top, total, VIEW, TRACK, SCALE)).toBeCloseTo(y, 4)
    }
  })

  it('拖拽映射:越界钳制 + 钳高形态端点仍可达全程', () => {
    expect(sliderTopToLogicalY(-50, 100_000, VIEW, TRACK, SCALE)).toBe(0)
    expect(sliderTopToLogicalY(1e9, 100_000, VIEW, TRACK, SCALE)).toBe(100_000 - VIEW)
    // 钳高深库:拖到轨道底(trackH − 钳制框高)逆映射经钳制到达可滚动量末端
    const s = minimapScale(96, 150_000)
    const total = 30_000_000
    expect(sliderTopToLogicalY(TRACK - MIN_SLIDER_PX, total, VIEW, TRACK, s)).toBe(total - VIEW)
    // 无效/无行程 → 0
    expect(sliderTopToLogicalY(100, 800, VIEW, TRACK, SCALE)).toBe(0)
    expect(sliderTopToLogicalY(100, 100_000, VIEW, TRACK, 0)).toBe(0)
  })

  it('点击:点击处内容居中,端点钳制', () => {
    // 非滑动:点击 160px → 内容 y 2000 → 目标 2000 − 500 = 1500
    const w = minimapWindow(0, 5000, VIEW, TRACK, SCALE)!
    expect(clickToLogicalY(160, w, SCALE, VIEW, 5000)).toBeCloseTo(1500, 8)
    expect(clickToLogicalY(0, w, SCALE, VIEW, 5000)).toBe(0) // −500 钳到 0
    expect(clickToLogicalY(TRACK, w, SCALE, VIEW, 5000)).toBe(4000) // 超底钳到 maxScroll
    // 滑动形态:窗顶参与换算
    const total = 100_000
    const w2 = minimapWindow(50_000, total, VIEW, TRACK, SCALE)!
    const mid = clickToLogicalY(TRACK / 2, w2, SCALE, VIEW, total)
    expect(mid).toBeCloseTo(w2.topY + TRACK / 2 / SCALE - VIEW / 2, 6)
  })

  it('滚轮:像素/行/页单位统一换算为像素', () => {
    expect(normalizeWheelDelta(120, 0, VIEW)).toBe(120)
    expect(normalizeWheelDelta(3, 1, VIEW)).toBe(48)
    expect(normalizeWheelDelta(1, 2, VIEW)).toBe(VIEW)
    expect(normalizeWheelDelta(1, 2, 0)).toBe(800)
  })

  it('滚轮:同帧多次输入可在待提交位置上连续累加,并钳制两端', () => {
    const total = 5000
    const first = wheelToLogicalY(100, 3, 1, total, VIEW)
    expect(first).toBe(148)
    expect(wheelToLogicalY(first, 3, 1, total, VIEW)).toBe(196)
    expect(wheelToLogicalY(3990, 3, 1, total, VIEW)).toBe(4000)
    expect(wheelToLogicalY(10, -3, 1, total, VIEW)).toBe(0)
  })

  it('缩略图签名:缓存目录、代次、状态或路径变化都会失效', () => {
    const base = minimapThumbSig(1, 'C:/cache-a', 1, 'x.webp')
    expect(minimapThumbSig(1, 'C:/cache-a', 1, 'x.webp')).toBe(base)
    expect(minimapThumbSig(1, 'D:/cache-b', 1, 'x.webp')).not.toBe(base)
    expect(minimapThumbSig(2, 'C:/cache-a', 1, 'x.webp')).not.toBe(base)
    expect(minimapThumbSig(1, 'C:/cache-a', 0, 'x.webp')).not.toBe(base)
    expect(minimapThumbSig(1, 'C:/cache-a', 1, 'y.webp')).not.toBe(base)
  })

  it('缩略图失败:仅前两次按 400ms/1200ms 有限退避', () => {
    expect(minimapThumbRetryDelay(1)).toBe(400)
    expect(minimapThumbRetryDelay(2)).toBe(1200)
    expect(minimapThumbRetryDelay(3)).toBeNull()
    expect(minimapThumbRetryDelay(0)).toBeNull()
    expect(minimapThumbRetryDelay(1.5)).toBeNull()
  })

  it('键盘:方向键/Page/Home/End 导航并钳制端点', () => {
    const total = 5000
    expect(keyboardToLogicalY('ArrowDown', 100, total, VIEW)).toBe(140)
    expect(keyboardToLogicalY('ArrowUp', 20, total, VIEW)).toBe(0)
    expect(keyboardToLogicalY('PageDown', 100, total, VIEW)).toBe(1100)
    expect(keyboardToLogicalY('PageUp', 100, total, VIEW)).toBe(0)
    expect(keyboardToLogicalY('Home', 2000, total, VIEW)).toBe(0)
    expect(keyboardToLogicalY('End', 0, total, VIEW)).toBe(4000)
    expect(keyboardToLogicalY('Enter', 100, total, VIEW)).toBeNull()
  })
})
