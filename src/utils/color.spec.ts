// color 单测（R3）。颜色解析 + 感知亮度 + WCAG 对比度，纯函数 node 可测。
import { describe, it, expect } from 'vitest'
import {
  parseColorToRgb,
  parseOklchToRgb,
  isDarkColor,
  relativeLuminance,
  contrastRatio,
} from './color'

describe('parseColorToRgb', () => {
  it('解析 #rgb / #rrggbb / rgb() / rgba()（含空格分隔）', () => {
    expect(parseColorToRgb('#000')).toEqual({ r: 0, g: 0, b: 0 })
    expect(parseColorToRgb('#ffffff')).toEqual({ r: 255, g: 255, b: 255 })
    expect(parseColorToRgb('#1a2b3c')).toEqual({ r: 26, g: 43, b: 60 })
    expect(parseColorToRgb('rgb(20, 40, 60)')).toEqual({ r: 20, g: 40, b: 60 })
    expect(parseColorToRgb('rgb(240 240 240)')).toEqual({ r: 240, g: 240, b: 240 })
    expect(parseColorToRgb('rgba(0,0,0,1)')).toEqual({ r: 0, g: 0, b: 0 })
  })

  it('解析 color(srgb ...)(0-1 通道)', () => {
    expect(parseColorToRgb('color(srgb 1 0 0)')).toEqual({ r: 255, g: 0, b: 0 })
    expect(parseColorToRgb('color(srgb 0.5 0.5 0.5)')).toEqual({ r: 128, g: 128, b: 128 })
  })

  it('解析 oklch()(getComputedStyle 对主题 color-mix token 的典型回读形态)', () => {
    expect(parseOklchToRgb('oklch(1 0 0)')).toEqual({ r: 255, g: 255, b: 255 })
    expect(parseOklchToRgb('oklch(0 0 0)')).toEqual({ r: 0, g: 0, b: 0 })
    // CSSWG 参考值:sRGB 红/蓝的 oklch 坐标,往返误差允许四舍五入 ±1。
    const red = parseOklchToRgb('oklch(0.627955 0.257683 29.2338)')
    expect(red?.r).toBeGreaterThanOrEqual(254)
    expect(red?.g).toBeLessThanOrEqual(1)
    expect(red?.b).toBeLessThanOrEqual(1)
    const blue = parseOklchToRgb('oklch(0.451826 0.313214 264.052)')
    expect(blue?.r).toBeLessThanOrEqual(1)
    expect(blue?.g).toBeLessThanOrEqual(1)
    expect(blue?.b).toBeGreaterThanOrEqual(254)
    // 百分比明度 / deg 后缀 / alpha 后缀的变体写法。
    const pct = parseOklchToRgb('oklch(62.796% 0.257683 29.2338deg / 1)')
    expect(pct?.r).toBeGreaterThanOrEqual(253)
    expect(parseOklchToRgb('oklch(abc 0 0)')).toBeNull()
  })

  it('oklch() 色相缺失(none):按 CSS Color 4 §4.4 以 0deg 参与转换,chroma 原样保留', () => {
    // 真实回读串:浅色主题画布底色 color-mix 经 getComputedStyle 序列化为 hue=none。
    expect(parseColorToRgb('oklch(0.977804 0.005983 none)')).toEqual({ r: 251, g: 246, b: 247 })
    // 大小写不敏感 + alpha 后缀 + none 混写同样成立。
    expect(parseOklchToRgb('oklch(0.5 0.1 NONE / 1)')).toEqual({ r: 144, g: 73, b: 97 })
    // none 只补色相角,不是"无色":若误把 a/b 一并归零会得到中性灰 99/99/99。
    expect(parseOklchToRgb('oklch(0.5 0.1 none)')).not.toEqual({ r: 99, g: 99, b: 99 })
    // 数值色相回归:0deg 与 none 等价是规范行为,带色相与零 chroma 的既有结果不变。
    expect(parseOklchToRgb('oklch(0.5 0.1 0)')).toEqual({ r: 144, g: 73, b: 97 })
    expect(parseOklchToRgb('oklch(0.5 0 0)')).toEqual({ r: 99, g: 99, b: 99 })
    expect(parseOklchToRgb('oklch(0.5 0 none)')).toEqual({ r: 99, g: 99, b: 99 })
    // none 不是数值:其后跟 deg 属非法语法,须拒绝而非误读。
    expect(parseOklchToRgb('oklch(0.5 0.1 none deg)')).toBeNull()
    // 畸形数值色相不得被前缀解析成 12.3,带不带 deg 后缀都须整体拒绝。
    expect(parseOklchToRgb('oklch(0.5 0.1 12.3.4)')).toBeNull()
    expect(parseOklchToRgb('oklch(0.5 0.1 12.3.4deg)')).toBeNull()
  })

  it('无法解析 → null（空 / 具名色 / 非法 hex / 非数通道）', () => {
    for (const c of ['', 'transparent', 'blue', 'red', '#12', 'not-a-color', '#gggggg']) {
      expect(parseColorToRgb(c), c).toBeNull()
    }
  })
})

describe('isDarkColor（感知亮度 < 0.5）', () => {
  it('暗色 → true', () => {
    for (const c of ['#000', '#000000', '#1a1a1a', 'rgb(20, 20, 20)', 'rgba(0,0,0,1)', '#222']) {
      expect(isDarkColor(c)).toBe(true)
    }
  })
  it('亮色 → false', () => {
    for (const c of ['#fff', '#ffffff', 'rgb(255,255,255)', '#f5f0e8', '#eae6da', 'rgb(240 240 240)']) {
      expect(isDarkColor(c)).toBe(false)
    }
  })
  it('无法解析 → false（默认按亮色）', () => {
    for (const c of ['', 'transparent', 'blue', '#12', 'not-a-color']) {
      expect(isDarkColor(c)).toBe(false)
    }
  })
})

describe('relativeLuminance（WCAG 线性化）', () => {
  it('黑 = 0、白 = 1（近似）', () => {
    expect(relativeLuminance({ r: 0, g: 0, b: 0 })).toBeCloseTo(0, 5)
    expect(relativeLuminance({ r: 255, g: 255, b: 255 })).toBeCloseTo(1, 5)
  })
})

describe('contrastRatio（WCAG）', () => {
  it('黑白对比 = 21', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 1)
  })
  it('同色对比 = 1', () => {
    expect(contrastRatio('#777777', '#777777')).toBeCloseTo(1, 5)
  })
  it('对称（换序不变）', () => {
    expect(contrastRatio('#123456', '#abcdef')).toBeCloseTo(contrastRatio('#abcdef', '#123456'), 5)
  })
  it('任一色不可解析 → 0（供门禁「解析失败即红」）', () => {
    expect(contrastRatio('nope', '#fff')).toBe(0)
    expect(contrastRatio('#fff', 'transparent')).toBe(0)
  })
})
