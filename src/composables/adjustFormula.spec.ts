import { describe, expect, it } from 'vitest'
import golden from '../fixtures/adjustGolden.json'
import {
  ADJUST_MAX,
  ADJUST_MIN,
  adjustCoefficients,
  adjustRgb,
  applyAdjustToPixels,
  isNeutralAdjust,
} from './adjustFormula'

describe('adjustFormula', () => {
  it('与 Rust 消费同一组调色黄金向量(f32 容差 + 量化端点精确)', () => {
    for (const vector of golden) {
      const k = adjustCoefficients(vector)
      const [r, g, b] = adjustRgb(vector.input[0], vector.input[1], vector.input[2], k)
      const actual = [r, g, b]
      for (let channel = 0; channel < 3; channel++) {
        expect(Math.abs(actual[channel] - vector.expected[channel])).toBeLessThanOrEqual(1e-5)
        expect(Math.round(Math.min(1, Math.max(0, actual[channel])) * 255)).toBe(
          vector.expected8[channel],
        )
        expect(Math.round(Math.min(1, Math.max(0, actual[channel])) * 65535)).toBe(
          vector.expected16[channel],
        )
      }
    }
  })

  it('像素缓冲整体应用按量化契约写回且 alpha 不动', () => {
    // 黄金向量 saturation=100 组:输入 0.6/0.4/0.2 恰为 8-bit 码值 153/102/51。
    const data = new Uint8ClampedArray([153, 102, 51, 77, 153, 102, 51, 200])
    applyAdjustToPixels(data, adjustCoefficients({ brightness: 0, contrast: 0, saturation: 100 }))
    expect([...data]).toEqual([197, 95, 0, 77, 197, 95, 0, 200])
  })

  it('全零参数判定与端点常量', () => {
    expect(isNeutralAdjust({ brightness: 0, contrast: 0, saturation: 0 })).toBe(true)
    expect(isNeutralAdjust({ brightness: 0, contrast: 0, saturation: 1 })).toBe(false)
    expect(ADJUST_MIN).toBe(-100)
    expect(ADJUST_MAX).toBe(100)
  })
})
