import {
  summarizeDistribution,
  summarizeFrameHealth,
  type DistributionSummary,
  type FrameHealthSummary,
} from './performanceStats'

export const PERFORMANCE_SPAN_NAMES = [
  'gallery.draw',
  'gallery.bitmapLoad',
  'gallery.imageFallback',
  // Canvas 缩略图分段(密集缩略图方案 §7.3):候选等槽 → 字节 → 源解码 → 裁剪缩放 → 入屏首绘代理。
  'gallery.thumbWait',
  'gallery.thumbFetch',
  'gallery.thumbSourceDecode',
  'gallery.thumbPrep',
  'gallery.thumbFirstPaint',
  'timeline.draw',
  'timeline.static',
  'timeline.loupe',
] as const

/** 冷格暴露积分(§7.3)所用的两个 gauge 与累计计数器:消费方按 coldCellMs/visibleCellMs 之比
 *  得到「平均冷格比例」。积分在 **gauge 写入时**按 performance.now() 结算(见 setGauge):
 *  Canvas draw 与录制器帧回调的先后顺序无保证,若等帧回调再读 gauge,先跑 draw 的那一帧会把
 *  本帧新冷格数乘上过去的间期。非录制态零成本,跨会话由 start 重置。 */
const COLD_CELLS_GAUGE = 'gallery.visibleColdCells'
const VISIBLE_CELLS_GAUGE = 'gallery.visibleCells'
const COLD_CELL_MS_COUNTER = 'gallery.coldCellMs'
const VISIBLE_CELL_MS_COUNTER = 'gallery.visibleCellMs'

export type PerformanceSpanName = (typeof PERFORMANCE_SPAN_NAMES)[number]
export type PerformanceContextValue = string | number | boolean | null
export type PerformanceSessionContext = Record<string, PerformanceContextValue>

export interface SampleSummary extends DistributionSummary {
  overwritten: number
}

export interface PerformanceCapabilities {
  longFrameSource: 'long-animation-frame' | 'longtask' | null
  eventTiming: boolean
  jsHeap: boolean
}

export interface PerformanceSessionSummary {
  id: string
  startedAt: string
  durationMs: number
  context: PerformanceSessionContext
  frame: FrameHealthSummary
  spans: Record<PerformanceSpanName, SampleSummary>
  longFrames: SampleSummary
  inputDelay: SampleSummary
  counters: Record<string, number>
  gauges: Record<string, number>
  heapStartBytes: number | null
  heapEndBytes: number | null
  heapDeltaBytes: number | null
  capabilities: PerformanceCapabilities
}

/**
 * 预分配环形缓冲：录制热路径只写 TypedArray，不 push 普通数组，也不因长会话无界增长。
 * 满容量后覆盖最老样本，并单独累计 overwritten 让报告显式暴露截断。
 */
export class FixedSampleBuffer {
  private readonly values: Float32Array
  private writeIndex = 0
  private sampleCount = 0
  overwritten = 0

  constructor(capacity: number) {
    if (!Number.isInteger(capacity) || capacity <= 0) throw new Error('capacity 必须是正整数')
    this.values = new Float32Array(capacity)
  }

  get length(): number {
    return this.sampleCount
  }

  push(value: number): void {
    this.values[this.writeIndex] = value
    this.writeIndex = (this.writeIndex + 1) % this.values.length
    if (this.sampleCount < this.values.length) this.sampleCount++
    else this.overwritten++
  }

  clear(): void {
    this.writeIndex = 0
    this.sampleCount = 0
    this.overwritten = 0
  }

  /** 仅在停止或显式实时观察时分配普通数组，顺序始终为最老→最新。 */
  toArray(): number[] {
    const out = new Array<number>(this.sampleCount)
    const start = this.sampleCount === this.values.length ? this.writeIndex : 0
    for (let i = 0; i < this.sampleCount; i++)
      out[i] = this.values[(start + i) % this.values.length]
    return out
  }
}

interface ChromiumPerformanceMemory {
  usedJSHeapSize: number
}

type PerformanceWithMemory = Performance & { memory?: ChromiumPerformanceMemory }

const FRAME_CAPACITY = 36_000 // 240Hz 下可保留 150 秒
const SPAN_CAPACITY = 8_192
const OBSERVER_CAPACITY = 4_096

function createSpanBuffers(): Record<PerformanceSpanName, FixedSampleBuffer> {
  return Object.fromEntries(
    PERFORMANCE_SPAN_NAMES.map((name) => [name, new FixedSampleBuffer(SPAN_CAPACITY)]),
  ) as Record<PerformanceSpanName, FixedSampleBuffer>
}

function sampleSummary(buffer: FixedSampleBuffer): SampleSummary {
  return { ...summarizeDistribution(buffer.toArray()), overwritten: buffer.overwritten }
}

function readHeapBytes(): number | null {
  const memory = (performance as PerformanceWithMemory).memory
  return memory && Number.isFinite(memory.usedJSHeapSize) ? memory.usedJSHeapSize : null
}

/** 非 Vue 单例采集器：UI 仅负责启停和读取快照，Canvas 热路径不建立响应式依赖。 */
export class PerformanceRecorder {
  private readonly frameIntervals = new FixedSampleBuffer(FRAME_CAPACITY)
  private readonly spanBuffers = createSpanBuffers()
  private readonly longFrames = new FixedSampleBuffer(OBSERVER_CAPACITY)
  private readonly inputDelay = new FixedSampleBuffer(OBSERVER_CAPACITY)
  private counters: Record<string, number> = Object.create(null) as Record<string, number>
  private gauges: Record<string, number> = Object.create(null) as Record<string, number>
  private context: PerformanceSessionContext = {}
  private active = false
  private frameRaf: number | null = null
  private lastFrameTs = 0
  private startedAtMs = 0
  private startedAtIso = ''
  private budgetMs = 1000 / 60
  private heapStartBytes: number | null = null
  private longFrameObserver: PerformanceObserver | null = null
  private eventObserver: PerformanceObserver | null = null
  private longFrameSource: PerformanceCapabilities['longFrameSource'] = null
  private eventTimingEnabled = false
  private sequence = 0
  private epoch = 0
  /** 暴露积分的上次结算时刻(performance.now(),与 gauge 写入同一时钟)。 */
  private exposureAtMs = 0
  /** 本会话是否已有 draw 写过冷格/可见格 gauge(未写过则不计账,避免会话边界残留)。 */
  private exposureHasGauge = false

  isActive(): boolean {
    return this.active
  }

  /**
   * 录制会话代次:每次 start 递增(从未开始过为 0)。供电/管线侧的「跨会话中间账本」清理判定
   * ——可连续 start/stop,上一会话残留的等待起点/位图状态不得污染新会话;未录制时不递增,
   * 避免把非录制期的加载误判成新会话样本。
   */
  recordingEpoch(): number {
    return this.epoch
  }

  start(context: PerformanceSessionContext, budgetMs: number): void {
    if (this.active) throw new Error('性能录制已经开始')
    this.resetBuffers()
    this.epoch++
    this.context = { ...context }
    this.budgetMs = Number.isFinite(budgetMs) && budgetMs > 0 ? budgetMs : 1000 / 60
    this.startedAtMs = performance.now()
    this.exposureAtMs = this.startedAtMs
    this.exposureHasGauge = false
    this.startedAtIso = new Date().toISOString()
    this.heapStartBytes = readHeapBytes()
    this.active = true
    this.startObservers()
    this.frameRaf = requestAnimationFrame(this.onFrame)
  }

  stop(): PerformanceSessionSummary | null {
    if (!this.active) return null
    const stoppedAt = performance.now()
    this.active = false
    if (this.frameRaf !== null) cancelAnimationFrame(this.frameRaf)
    this.frameRaf = null
    this.longFrameObserver?.disconnect()
    this.eventObserver?.disconnect()
    this.longFrameObserver = null
    this.eventObserver = null
    // 尾部收口:最后一次结算到停止之间仍按当前状态积分,否则尾部冷格暴露整段丢失。
    this.settleCellExposure(stoppedAt)
    return this.buildSummary(stoppedAt)
  }

  /** 实时观察模式专用；会排序和分配，基准录制不得调用。 */
  snapshot(): PerformanceSessionSummary | null {
    return this.active ? this.buildSummary(performance.now()) : null
  }

  /**
   * 低成本实时读数(DEV 基准桥的时序采样专用):仅浅拷贝聚合的 gauges/counters,不排序、
   * 不分配样本数组——与 snapshot 的全量汇总有别,录制期间低频轮询(如 250ms)可承受。
   * 生产代码不得依赖(gauge 本身 last-write-wins,只反映最近一次写入)。
   */
  currentCounters(): { gauges: Record<string, number>; counters: Record<string, number> } | null {
    if (!this.active) return null
    return { gauges: { ...this.gauges }, counters: { ...this.counters } }
  }

  recordSpan(name: PerformanceSpanName, durationMs: number): void {
    if (!this.active || !Number.isFinite(durationMs) || durationMs < 0) return
    this.spanBuffers[name].push(durationMs)
  }

  count(name: string, amount = 1): void {
    if (!this.active || !Number.isFinite(amount)) return
    this.counters[name] = (this.counters[name] ?? 0) + amount
  }

  setGauge(name: string, value: number): void {
    if (!this.active || !Number.isFinite(value)) return
    // 冷格/可见格的状态改变前,先用同一时钟把**旧状态**在该时刻之前的暴露量结算掉:
    // draw 可能先于录制器帧回调执行,等回调再读 gauge 会让本帧新冷格乘上过去的间期。
    if (name === COLD_CELLS_GAUGE || name === VISIBLE_CELLS_GAUGE) {
      this.settleCellExposure(performance.now())
      this.exposureHasGauge = true
    }
    this.gauges[name] = value
  }

  private readonly onFrame = (timestamp: number): void => {
    if (!this.active) return
    if (this.lastFrameTs > 0) this.frameIntervals.push(timestamp - this.lastFrameTs)
    this.lastFrameTs = timestamp
    // 帧回调只按同一时钟 flush,不自行推断间期——rAF 时间戳只用于帧间隔统计。
    this.settleCellExposure(performance.now())
    this.frameRaf = requestAnimationFrame(this.onFrame)
  }

  /**
   * 冷格暴露积分:把「当前 gauge 状态 × 自上次结算以来的实际持续时间」累加为
   * Σ(冷格数×持续毫秒)与 Σ(可见格数×持续毫秒)。结算点只有三个且同用 performance.now:
   * gauge 写入前、录制器帧回调、停止;本会话尚未写过 gauge 时只推进计时不记账。
   */
  private settleCellExposure(nowMs: number): void {
    const dt = nowMs - this.exposureAtMs
    this.exposureAtMs = nowMs
    if (!(dt > 0) || !this.exposureHasGauge) return
    const cold = this.gauges[COLD_CELLS_GAUGE]
    const visible = this.gauges[VISIBLE_CELLS_GAUGE]
    if (cold) {
      this.counters[COLD_CELL_MS_COUNTER] =
        (this.counters[COLD_CELL_MS_COUNTER] ?? 0) + cold * dt
    }
    if (visible) {
      this.counters[VISIBLE_CELL_MS_COUNTER] =
        (this.counters[VISIBLE_CELL_MS_COUNTER] ?? 0) + visible * dt
    }
  }

  private startObservers(): void {
    this.longFrameSource = null
    this.eventTimingEnabled = false
    if (typeof PerformanceObserver === 'undefined') return
    const supported = PerformanceObserver.supportedEntryTypes ?? []
    const longType = supported.includes('long-animation-frame')
      ? 'long-animation-frame'
      : supported.includes('longtask')
        ? 'longtask'
        : null
    if (longType) {
      try {
        this.longFrameObserver = new PerformanceObserver((list) => {
          if (!this.active) return
          for (const entry of list.getEntries()) this.longFrames.push(entry.duration)
        })
        this.longFrameObserver.observe({ type: longType, buffered: false })
        this.longFrameSource = longType
      } catch {
        this.longFrameObserver = null
        this.longFrameSource = null
      }
    }
    if (supported.includes('event')) {
      try {
        this.eventObserver = new PerformanceObserver((list) => {
          if (!this.active) return
          for (const entry of list.getEntries()) this.inputDelay.push(entry.duration)
        })
        // durationThreshold 属于 Event Timing 扩展，当前 TypeScript DOM 声明未必包含，故窄化断言。
        this.eventObserver.observe({
          type: 'event',
          buffered: false,
          durationThreshold: 16,
        } as PerformanceObserverInit)
        this.eventTimingEnabled = true
      } catch {
        this.eventObserver = null
        this.eventTimingEnabled = false
      }
    }
  }

  private buildSummary(stoppedAt: number): PerformanceSessionSummary {
    const heapEndBytes = readHeapBytes()
    const spans = Object.fromEntries(
      PERFORMANCE_SPAN_NAMES.map((name) => [name, sampleSummary(this.spanBuffers[name])]),
    ) as Record<PerformanceSpanName, SampleSummary>
    return {
      id: `perf-${Date.now().toString(36)}-${++this.sequence}`,
      startedAt: this.startedAtIso,
      durationMs: Math.max(0, Math.round((stoppedAt - this.startedAtMs) * 10) / 10),
      context: { ...this.context },
      frame: summarizeFrameHealth(this.frameIntervals.toArray(), this.budgetMs),
      spans,
      longFrames: sampleSummary(this.longFrames),
      inputDelay: sampleSummary(this.inputDelay),
      counters: { ...this.counters },
      gauges: { ...this.gauges },
      heapStartBytes: this.heapStartBytes,
      heapEndBytes,
      heapDeltaBytes:
        this.heapStartBytes !== null && heapEndBytes !== null
          ? heapEndBytes - this.heapStartBytes
          : null,
      capabilities: {
        longFrameSource: this.longFrameSource,
        eventTiming: this.eventTimingEnabled,
        jsHeap: heapEndBytes !== null,
      },
    }
  }

  private resetBuffers(): void {
    this.frameIntervals.clear()
    for (const name of PERFORMANCE_SPAN_NAMES) this.spanBuffers[name].clear()
    this.longFrames.clear()
    this.inputDelay.clear()
    this.counters = Object.create(null) as Record<string, number>
    this.gauges = Object.create(null) as Record<string, number>
    this.lastFrameTs = 0
    this.exposureAtMs = 0
    this.exposureHasGauge = false
  }
}

export const performanceRecorder = new PerformanceRecorder()
