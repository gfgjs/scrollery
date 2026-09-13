import { describe, it, expect } from 'vitest'
import { summarizeFrames } from './galleryPerfProbe.helpers'

/** 构造理想等间隔时间戳(fps 恒定),用于确定性断言。 */
function evenFrames(count: number, fps: number): number[] {
  const dt = 1000 / fps
  return Array.from({ length: count }, (_, i) => i * dt)
}

describe('summarizeFrames', () => {
  it('样本不足 2 帧回零,不产生 NaN/Infinity', () => {
    expect(summarizeFrames([])).toEqual({ frames: 0, avgFps: 0, minFps: 0, dropped: 0 })
    expect(summarizeFrames([12.3])).toEqual({ frames: 1, avgFps: 0, minFps: 0, dropped: 0 })
  })

  it('稳定 60fps → avg/min≈60,零掉帧', () => {
    const s = summarizeFrames(evenFrames(61, 60))
    expect(s.frames).toBe(61)
    expect(s.avgFps).toBeCloseTo(60, 0)
    expect(s.minFps).toBeCloseTo(60, 0)
    expect(s.dropped).toBe(0)
  })

  it('稳定 30fps(默认目标 60)→ avg/min≈30,且每帧相对 60 目标都算掉帧', () => {
    const s = summarizeFrames(evenFrames(31, 30))
    expect(s.avgFps).toBeCloseTo(30, 0)
    expect(s.minFps).toBeCloseTo(30, 0)
    // 33.3ms/帧 > (1000/60)*1.5=25ms → 30 个间隔全判掉帧;dropped 语义即「离 60fps 目标有多远」。
    expect(s.dropped).toBe(30)
  })

  it('稳定 30fps 且目标也设 30 → 零掉帧', () => {
    const s = summarizeFrames(evenFrames(31, 30), 30)
    expect(s.dropped).toBe(0)
  })

  it('夹一长帧 → minFps 反映最慢帧且计一次掉帧,minFps < avgFps', () => {
    // 0, 16.7, 100(长约 83.3ms 帧), 116.7 —— 仅那一长帧超阈
    const s = summarizeFrames([0, 1000 / 60, 100, 100 + 1000 / 60])
    expect(s.dropped).toBe(1)
    expect(s.minFps).toBeCloseTo(1000 / (100 - 1000 / 60), 0) // ≈12
    expect(s.minFps).toBeLessThan(s.avgFps)
  })

  it('targetFps 可调:120 目标下 60fps 的每一帧都判掉帧', () => {
    // 9 个间隔各 ~16.7ms > (1000/120)*1.5=12.5ms → 全部掉帧
    const s = summarizeFrames(evenFrames(10, 60), 120)
    expect(s.dropped).toBe(9)
  })
})
