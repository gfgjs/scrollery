// Canvas 缩略图单元格指标(密集缩略图方案 §7.3)的表征测试:首次入屏命中/未命中与「入屏 →
// 首次绘出位图」代理时延。用真实 PerformanceRecorder 单例(经 rAF 桩驱动帧循环),覆盖三件
// 容易写错的事:非录制态零记账、首帧只建基线不计数、会话切换后上一会话的入场时刻不作数。
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createCanvasThumbCellMetrics } from './canvasThumbCellMetrics'
import { performanceRecorder } from './performanceRecorder'

// 录制器 start 会排一帧 rAF;本项目单测跑 node 环境,给出空转桩(本模块不消费帧回调)。
function stubFrameClock(): void {
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn(() => 1),
  )
  vi.stubGlobal('cancelAnimationFrame', vi.fn())
}

afterEach(() => {
  if (performanceRecorder.isActive()) performanceRecorder.stop()
  vi.unstubAllGlobals()
})

describe('canvasThumbCellMetrics', () => {
  it('非录制态:beginFrame 返回 false,noteCell/endFrame 不产生任何样本', () => {
    const metrics = createCanvasThumbCellMetrics()
    expect(metrics.beginFrame(0)).toBe(false)
    metrics.noteCell(1, false, false)
    metrics.endFrame()
    expect(performanceRecorder.snapshot()).toBeNull()
  })

  it('首帧只建基线:入屏既有位图的格不计命中(避免把「录之前就在屏上」算成首次入屏)', () => {
    stubFrameClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    const metrics = createCanvasThumbCellMetrics()
    expect(metrics.beginFrame(0)).toBe(true)
    metrics.noteCell(1, true, false)
    metrics.endFrame()
    expect(performanceRecorder.snapshot()?.counters['gallery.thumbFirstEntryHit']).toBeUndefined()

    // 第二帧:1 仍在屏上是重复帧,2 首次入屏且已有位图 → 记一次命中。
    metrics.beginFrame(16)
    metrics.noteCell(1, true, false)
    metrics.noteCell(2, true, false)
    metrics.endFrame()
    const counters = performanceRecorder.snapshot()?.counters
    expect(counters?.['gallery.thumbFirstEntryHit']).toBe(1)
    expect(counters?.['gallery.thumbFirstEntryMiss']).toBeUndefined()
  })

  it('缺图入屏记未命中,绘出位图时结算为入屏→首绘代理时延;失败格不留悬空入场时刻', () => {
    stubFrameClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    const metrics = createCanvasThumbCellMetrics()
    metrics.beginFrame(0)
    metrics.endFrame() // 建基线

    metrics.beginFrame(100)
    metrics.noteCell(7, false, false) // 缺图入屏 → 起点 100
    metrics.noteCell(8, false, false) // 随后判失败 → 不结算
    metrics.endFrame()

    metrics.beginFrame(180)
    metrics.noteCell(7, true, false) // 绘出 → 180−100
    metrics.noteCell(8, false, true)
    metrics.endFrame()
    const summary = performanceRecorder.snapshot()
    expect(summary?.counters['gallery.thumbFirstEntryMiss']).toBe(2)
    expect(summary?.spans['gallery.thumbFirstPaint'].samples).toBe(1)
    expect(summary?.spans['gallery.thumbFirstPaint'].p50).toBe(80)
  })

  it('录制会话切换:上一会话的可见集与入场时刻作废,新会话首帧重新建基线', () => {
    stubFrameClock()
    performanceRecorder.start({ scenario: 'test-a' }, 1000 / 60)
    const metrics = createCanvasThumbCellMetrics()
    metrics.beginFrame(0)
    metrics.endFrame()
    metrics.beginFrame(50)
    metrics.noteCell(9, false, false) // 缺图入屏,起点 50
    metrics.endFrame()
    performanceRecorder.stop()

    performanceRecorder.start({ scenario: 'test-b' }, 1000 / 60)
    expect(metrics.beginFrame(200)).toBe(true)
    metrics.noteCell(9, true, false) // 旧入场时刻不得结算成 thumbFirstPaint
    metrics.endFrame()
    const summary = performanceRecorder.snapshot()
    expect(summary?.spans['gallery.thumbFirstPaint'].samples).toBe(0)
    expect(summary?.counters['gallery.thumbFirstEntryHit']).toBeUndefined()
  })

  it('滚出屏的待绘出格在收帧时出账:再入屏首绘不把屏外停留算进时延', () => {
    stubFrameClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    const metrics = createCanvasThumbCellMetrics()
    metrics.beginFrame(0)
    metrics.noteCell(1, true, false)
    metrics.endFrame() // 建基线,可见 {1}

    metrics.beginFrame(100)
    metrics.noteCell(2, false, false) // 2 缺图入屏(起点 100)
    metrics.endFrame() // 可见 {2},2 仍在屏上 → 保留待绘出

    metrics.beginFrame(200)
    metrics.noteCell(1, true, false) // 2 已滚出屏 → 收帧时应出账
    metrics.endFrame()

    metrics.beginFrame(300)
    metrics.noteCell(2, true, false) // 再入屏并绘出:按新的一次入屏算,不是 300−100
    metrics.endFrame()
    const summary = performanceRecorder.snapshot()
    expect(summary?.spans['gallery.thumbFirstPaint'].samples).toBe(0)
    expect(summary?.counters['gallery.thumbFirstEntryMiss']).toBe(1)
    expect(summary?.counters['gallery.thumbFirstEntryHit']).toBe(2)
  })

  it('reset(失活):丢弃可见集与待绘出表,返回后首帧重新建基线而非补记屏外停留', () => {
    stubFrameClock()
    performanceRecorder.start({ scenario: 'test' }, 1000 / 60)
    const metrics = createCanvasThumbCellMetrics()
    metrics.beginFrame(0)
    metrics.noteCell(1, true, false)
    metrics.endFrame()
    metrics.beginFrame(100)
    metrics.noteCell(2, false, false)
    metrics.endFrame()

    metrics.reset() // 失活:冷格/可见 gauge 由组件归零,追踪表在此清空
    metrics.beginFrame(200)
    metrics.noteCell(2, true, false) // 首帧只建基线:不计命中,也不结算 200−100
    metrics.endFrame()
    const summary = performanceRecorder.snapshot()
    expect(summary?.spans['gallery.thumbFirstPaint'].samples).toBe(0)
    expect(summary?.counters['gallery.thumbFirstEntryMiss']).toBe(1)
    expect(summary?.counters['gallery.thumbFirstEntryHit']).toBeUndefined()
  })
})
