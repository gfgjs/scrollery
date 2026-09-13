import { describe, expect, it } from 'vitest'
import {
  estimateFrameBudget,
  summarizeDistribution,
  summarizeFrameHealth,
} from './performanceStats'

describe('summarizeDistribution', () => {
  it('空样本与无效样本返回稳定零值', () => {
    expect(summarizeDistribution([])).toEqual({
      samples: 0,
      avg: 0,
      min: 0,
      p50: 0,
      p95: 0,
      p99: 0,
      max: 0,
    })
    expect(summarizeDistribution([NaN, Infinity, -1])).toEqual(summarizeDistribution([]))
  })

  it('计算平均值、分位数和边界', () => {
    const summary = summarizeDistribution([5, 1, 4, 2, 3])
    expect(summary).toEqual({ samples: 5, avg: 3, min: 1, p50: 3, p95: 4.8, p99: 4.96, max: 5 })
  })
})

describe('estimateFrameBudget', () => {
  it('把 60Hz 浮点抖动吸附到 16.667ms', () => {
    expect(estimateFrameBudget([16.6, 16.7, 16.8, 33.4])).toBeCloseTo(1000 / 60, 2)
  })

  it('用较快分位识别 120Hz，不被偶发长帧误判成 60Hz', () => {
    expect(estimateFrameBudget([8.2, 8.3, 8.4, 16.7, 25])).toBeCloseTo(1000 / 120, 2)
  })

  it('没有有效样本时回退 60Hz', () => {
    expect(estimateFrameBudget([0, NaN, 200])).toBeCloseTo(1000 / 60, 2)
  })
})

describe('summarizeFrameHealth', () => {
  it('稳定 60Hz 不产生卡顿或 missed VSync', () => {
    const intervals = Array.from({ length: 60 }, () => 1000 / 60)
    const summary = summarizeFrameHealth(intervals, 1000 / 60)
    expect(summary.frames).toBe(61)
    expect(summary.avgFps).toBeCloseTo(60, 0)
    expect(summary.jankFrames).toBe(0)
    expect(summary.missedVsync).toBe(0)
  })

  it('长帧同时进入高分位、卡顿数和 missed VSync', () => {
    const summary = summarizeFrameHealth([16.67, 16.67, 50, 16.67], 1000 / 60)
    expect(summary.frameTime.max).toBe(50)
    expect(summary.jankFrames).toBe(1)
    expect(summary.jankRatio).toBe(0.25)
    expect(summary.missedVsync).toBe(2)
  })

  it('120Hz 预算下能识别 60Hz 交付为持续错过 VSync', () => {
    const summary = summarizeFrameHealth([16.67, 16.67, 16.67], 1000 / 120)
    expect(summary.refreshRate).toBe(120)
    expect(summary.jankFrames).toBe(3)
    expect(summary.missedVsync).toBe(3)
  })
})
