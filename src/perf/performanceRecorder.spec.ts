import { afterEach, describe, expect, it, vi } from 'vitest'
import { FixedSampleBuffer, performanceRecorder } from './performanceRecorder'

describe('FixedSampleBuffer', () => {
  it('未满时按写入顺序输出', () => {
    const buffer = new FixedSampleBuffer(4)
    buffer.push(1)
    buffer.push(2)
    buffer.push(3)
    expect(buffer.length).toBe(3)
    expect(buffer.overwritten).toBe(0)
    expect(buffer.toArray()).toEqual([1, 2, 3])
  })

  it('满后覆盖最老样本并记录截断次数', () => {
    const buffer = new FixedSampleBuffer(3)
    for (const value of [1, 2, 3, 4, 5]) buffer.push(value)
    expect(buffer.length).toBe(3)
    expect(buffer.overwritten).toBe(2)
    expect(buffer.toArray()).toEqual([3, 4, 5])
  })

  it('clear 完整复位游标和截断计数', () => {
    const buffer = new FixedSampleBuffer(2)
    buffer.push(1)
    buffer.push(2)
    buffer.push(3)
    buffer.clear()
    expect(buffer.length).toBe(0)
    expect(buffer.overwritten).toBe(0)
    expect(buffer.toArray()).toEqual([])
  })

  it('拒绝无效容量', () => {
    expect(() => new FixedSampleBuffer(0)).toThrow('capacity 必须是正整数')
    expect(() => new FixedSampleBuffer(1.5)).toThrow('capacity 必须是正整数')
  })
})

// ── 冷格暴露积分(密集缩略图方案 §7.3)──────────────────────────────────────────
// Σ(冷格数×持续毫秒)与 Σ(可见格数×持续毫秒)按「当前 gauge 状态 × 自上次结算以来的时间」
// 积分,结算点同用 performance.now:gauge 写入前、录制器帧回调、停止。rAF 桩收集回调以便手动
// 推进帧,controlled clock 让 rAF 时间戳与 performance.now 同步(生产同源)。
describe('冷格暴露积分', () => {
  let frames: FrameRequestCallback[] = []
  let clock = 0

  function stubClock(): void {
    clock = 0
    vi.spyOn(performance, 'now').mockImplementation(() => clock)
  }

  function stubRaf(): void {
    frames = []
    vi.stubGlobal(
      'requestAnimationFrame',
      vi.fn((cb: FrameRequestCallback) => frames.push(cb)),
    )
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
  }

  function tick(timestamp: number): void {
    const pending = frames
    frames = []
    for (const cb of pending) cb(timestamp)
  }

  afterEach(() => {
    if (performanceRecorder.isActive()) performanceRecorder.stop()
    frames = []
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('draw 先于录制器帧回调:本帧新冷格不计入过去间期(结算发生在 gauge 写入时)', () => {
    stubRaf()
    stubClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    tick(0)
    // 帧 N 在 t=50 写入「全热」状态,覆盖 [50,100)。
    clock = 50
    performanceRecorder.setGauge('gallery.visibleCells', 10)
    performanceRecorder.setGauge('gallery.visibleColdCells', 0)
    // 帧 N+1 的 draw 在录制器帧回调**之前**跑了:新冷格数只对 t=100 之后有效。
    clock = 100
    performanceRecorder.setGauge('gallery.visibleColdCells', 4)
    tick(100) // 该帧的录制器回调
    clock = 160
    tick(160)
    clock = 160
    const summary = performanceRecorder.stop()
    expect(summary?.counters['gallery.coldCellMs']).toBe(240) // 4 × 60ms,不是 4 × 100ms
    expect(summary?.counters['gallery.visibleCellMs']).toBe(1100) // 10 × 50ms + 10 × 60ms
  })

  it('本会话未写过 draw gauge 时不记账(不把会话开始前的残留状态算进来)', () => {
    stubRaf()
    stubClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    tick(0)
    clock = 100
    tick(100)
    clock = 200
    tick(200)
    const summary = performanceRecorder.stop()
    expect(summary?.counters['gallery.coldCellMs']).toBeUndefined()
    expect(summary?.counters['gallery.visibleCellMs']).toBeUndefined()
  })

  it('停止时按当前状态补足尾部间期,冷格暴露不在收尾处丢失', () => {
    stubRaf()
    stubClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    tick(0)
    performanceRecorder.setGauge('gallery.visibleCells', 8)
    performanceRecorder.setGauge('gallery.visibleColdCells', 3)
    clock = 50
    tick(50) // 间期 50ms × 冷格 3 = 150ms,可见 400ms
    clock = 100
    const summary = performanceRecorder.stop() // 尾部 50ms × 冷格 3 = 150ms,可见 400ms
    expect(summary?.counters['gallery.coldCellMs']).toBe(300)
    expect(summary?.counters['gallery.visibleCellMs']).toBe(800)
  })
})
