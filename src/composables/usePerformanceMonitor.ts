import { ref, shallowRef } from 'vue'
import {
  performanceRecorder,
  type PerformanceSessionContext,
  type PerformanceSessionSummary,
} from '../perf/performanceRecorder'
import { estimateFrameBudget } from '../perf/performanceStats'

export type PerformanceMonitorPhase = 'idle' | 'arming' | 'recording'

export interface GalleryBenchmarkAdapter {
  getScroller: () => HTMLElement | null
  getContext: () => PerformanceSessionContext
}

const panelOpen = ref(false)
const phase = ref<PerformanceMonitorPhase>('idle')
const durationSeconds = ref(10)
const liveMode = ref(false)
const errorMessage = ref('')
const sessions = shallowRef<PerformanceSessionSummary[]>([])
const liveSummary = shallowRef<PerformanceSessionSummary | null>(null)

let galleryAdapter: GalleryBenchmarkAdapter | null = null
let stopTimer: ReturnType<typeof setTimeout> | null = null
let liveTimer: ReturnType<typeof setInterval> | null = null
let operationToken = 0
let benchmarkScroller: HTMLElement | null = null
let benchmarkOrigin = 0

function clearTimers(): void {
  if (stopTimer !== null) clearTimeout(stopTimer)
  if (liveTimer !== null) clearInterval(liveTimer)
  stopTimer = null
  liveTimer = null
}

function nextFrame(): Promise<number> {
  return new Promise((resolve) => requestAnimationFrame(resolve))
}

async function waitFrames(count: number): Promise<void> {
  for (let i = 0; i < count; i++) await nextFrame()
}

/** 录制前采 24 个空闲间隔；P20 + 常见刷新率吸附逻辑在纯 helper 内。 */
async function calibrateFrameBudget(token: number): Promise<number> {
  const timestamps: number[] = []
  for (let i = 0; i < 25; i++) {
    if (token !== operationToken) throw new Error('cancelled')
    timestamps.push(await nextFrame())
  }
  const intervals = timestamps.slice(1).map((timestamp, index) => timestamp - timestamps[index])
  return estimateFrameBudget(intervals)
}

function currentContext(scenario: string): PerformanceSessionContext {
  const adapterContext = galleryAdapter?.getContext() ?? {}
  return {
    scenario,
    route: `${window.location.pathname}${window.location.search}${window.location.hash}`,
    viewportWidth: window.innerWidth,
    viewportHeight: window.innerHeight,
    dpr: window.devicePixelRatio || 1,
    ...adapterContext,
  }
}

function publishFinal(summary: PerformanceSessionSummary | null): void {
  if (summary) {
    sessions.value = [summary, ...sessions.value].slice(0, 12)
    liveSummary.value = summary
  }
  phase.value = 'idle'
  panelOpen.value = true
  benchmarkScroller = null
}

function startLiveRefresh(): void {
  if (!liveMode.value) return
  liveTimer = setInterval(() => {
    // 该排序/分配只存在于用户明确开启的“实时观察”模式，静默基准绝不调用 snapshot。
    liveSummary.value = performanceRecorder.snapshot()
  }, 500)
}

async function startManualRecording(): Promise<void> {
  if (phase.value !== 'idle') return
  const token = ++operationToken
  errorMessage.value = ''
  liveSummary.value = null
  phase.value = 'arming'
  try {
    const budget = await calibrateFrameBudget(token)
    if (token !== operationToken) return
    if (!liveMode.value) {
      panelOpen.value = false
      // 面板卸载后再空过两帧，避免关闭动画/布局收尾落入样本。
      await waitFrames(2)
    }
    if (token !== operationToken) return
    performanceRecorder.start(
      currentContext(liveMode.value ? 'live-observation' : 'manual'),
      budget,
    )
    phase.value = 'recording'
    startLiveRefresh()
    stopTimer = setTimeout(stopRecording, Math.max(1, durationSeconds.value) * 1000)
  } catch (error) {
    if (token === operationToken && String(error) !== 'Error: cancelled') {
      errorMessage.value = error instanceof Error ? error.message : String(error)
      phase.value = 'idle'
      panelOpen.value = true
    }
  }
}

function animateScroll(
  element: HTMLElement,
  from: number,
  to: number,
  durationMs: number,
  token: number,
): Promise<boolean> {
  return new Promise((resolve) => {
    let startedAt: number | null = null
    const step = (timestamp: number) => {
      if (token !== operationToken) {
        resolve(false)
        return
      }
      if (startedAt === null) startedAt = timestamp
      const progress = Math.min(1, (timestamp - startedAt) / durationMs)
      element.scrollTop = from + (to - from) * progress
      if (progress < 1) requestAnimationFrame(step)
      else resolve(true)
    }
    requestAnimationFrame(step)
  })
}

async function runGalleryRoundTrip(): Promise<void> {
  if (phase.value !== 'idle') return
  const scroller = galleryAdapter?.getScroller() ?? null
  if (!scroller) {
    errorMessage.value = '当前页面没有可测试的画廊滚动容器'
    panelOpen.value = true
    return
  }
  const maxScroll = Math.max(0, scroller.scrollHeight - scroller.clientHeight)
  const origin = Math.min(maxScroll, Math.max(0, scroller.scrollTop))
  const distance = Math.min(Math.max(scroller.clientHeight * 3, 2_400), maxScroll)
  const target = origin + distance <= maxScroll ? origin + distance : Math.max(0, origin - distance)
  if (Math.abs(target - origin) < Math.min(400, scroller.clientHeight)) {
    errorMessage.value = '当前画廊可滚动距离不足，无法运行标准往返基准'
    panelOpen.value = true
    return
  }

  const token = ++operationToken
  errorMessage.value = ''
  liveSummary.value = null
  phase.value = 'arming'
  benchmarkScroller = scroller
  benchmarkOrigin = origin
  const legDuration = Math.min(2_500, Math.max(900, (Math.abs(target - origin) / 2_200) * 1000))
  try {
    const budget = await calibrateFrameBudget(token)
    panelOpen.value = false
    await waitFrames(2)
    // 预热一趟不入样本，让缩略图/布局进入稳定状态；正式三趟均从同一原点出发。
    if (!(await animateScroll(scroller, origin, target, legDuration, token))) return
    if (!(await animateScroll(scroller, target, origin, legDuration, token))) return
    await new Promise((resolve) => setTimeout(resolve, 250))
    if (token !== operationToken) return

    performanceRecorder.start(currentContext('gallery-roundtrip'), budget)
    performanceRecorder.count('benchmark.roundTrips', 3)
    phase.value = 'recording'
    for (let pass = 0; pass < 3; pass++) {
      if (!(await animateScroll(scroller, origin, target, legDuration, token))) return
      if (!(await animateScroll(scroller, target, origin, legDuration, token))) return
    }
    clearTimers()
    const summary = performanceRecorder.stop()
    scroller.scrollTop = origin
    publishFinal(summary)
  } catch (error) {
    if (performanceRecorder.isActive()) performanceRecorder.stop()
    scroller.scrollTop = origin
    if (token === operationToken && String(error) !== 'Error: cancelled') {
      errorMessage.value = error instanceof Error ? error.message : String(error)
      phase.value = 'idle'
      panelOpen.value = true
    }
  } finally {
    if (benchmarkScroller === scroller && phase.value !== 'recording') benchmarkScroller = null
  }
}

function stopRecording(): void {
  const wasArming = phase.value === 'arming'
  if (phase.value === 'idle') return
  ++operationToken
  clearTimers()
  const summary = performanceRecorder.stop()
  if (benchmarkScroller) benchmarkScroller.scrollTop = benchmarkOrigin
  if (wasArming && !summary) {
    phase.value = 'idle'
    panelOpen.value = true
    benchmarkScroller = null
    return
  }
  publishFinal(summary)
}

function openPanel(): void {
  panelOpen.value = true
}

function closePanel(): void {
  panelOpen.value = false
}

function togglePanel(): void {
  if (phase.value !== 'idle') stopRecording()
  else panelOpen.value = !panelOpen.value
}

function clearHistory(): void {
  sessions.value = []
  liveSummary.value = null
}

export function registerGalleryBenchmark(adapter: GalleryBenchmarkAdapter): () => void {
  galleryAdapter = adapter
  return () => {
    if (galleryAdapter === adapter) galleryAdapter = null
  }
}

export function usePerformanceMonitor() {
  return {
    panelOpen,
    phase,
    durationSeconds,
    liveMode,
    errorMessage,
    sessions,
    liveSummary,
    openPanel,
    closePanel,
    togglePanel,
    startManualRecording,
    runGalleryRoundTrip,
    stopRecording,
    clearHistory,
  }
}

// DEV 基准桥(仅 Vite dev;生产构建该分支死代码消除,零暴露面)。
// headless Chrome + CDP 基准脚本的启停入口(镜像 useGalleryPerfProbe 的 __scrolleryPerf
// 惯例):面板 UI 仍是真机主入口,本桥只为 ?ui-harness 场景下的脚本化 A/B 采样服务——
// 模块内部函数否则无法从页面上下文触达。
interface BenchBridge {
  /** 手动录制(durationSeconds 秒后自动停,结果进 sessions)。 */
  start: (durationSeconds?: number) => Promise<void>
  stop: () => void
  /** 内建画廊三往返基准(自驱滚动+录制)。 */
  roundTrip: () => Promise<void>
  phase: () => PerformanceMonitorPhase
  sessions: () => PerformanceSessionSummary[]
  /** 录制期低成本时序采样:仅 gauges/counters 浅拷贝,未录制返回 null(基线测量基建,§6)。 */
  sampleGauges: () => { gauges: Record<string, number>; counters: Record<string, number> } | null
}

declare global {
  interface Window {
    __scrolleryBench?: BenchBridge
  }
}

if (import.meta.env.DEV && typeof window !== 'undefined') {
  window.__scrolleryBench = {
    start: async (seconds = 10) => {
      durationSeconds.value = seconds
      await startManualRecording()
    },
    stop: stopRecording,
    roundTrip: runGalleryRoundTrip,
    phase: () => phase.value,
    sessions: () => sessions.value,
    sampleGauges: () => performanceRecorder.currentCounters(),
  }
}
