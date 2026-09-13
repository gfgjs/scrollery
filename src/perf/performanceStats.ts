/** 固定容量性能采样停止后的纯统计结果；热路径只记数字，不在录制中排序或分配。 */
export interface DistributionSummary {
  samples: number
  avg: number
  min: number
  p50: number
  p95: number
  p99: number
  max: number
}

/** 一次录制的帧健康摘要。 */
export interface FrameHealthSummary {
  /** rAF 回调数；帧间隔数量加一。 */
  frames: number
  /** 根据录制前校准得到的单帧预算。 */
  budgetMs: number
  /** 与预算对应的显示刷新率。 */
  refreshRate: number
  avgFps: number
  frameTime: DistributionSummary
  /** 超过 1.5 倍预算的帧数。 */
  jankFrames: number
  jankRatio: number
  /** 按帧预算估算错过的 VSync 总数。 */
  missedVsync: number
}

const EMPTY_DISTRIBUTION: DistributionSummary = {
  samples: 0,
  avg: 0,
  min: 0,
  p50: 0,
  p95: 0,
  p99: 0,
  max: 0,
}

/** 常见刷新率用于把 rAF 浮点抖动吸附到可读的显示器档位。 */
const COMMON_REFRESH_RATES = [240, 200, 180, 165, 144, 120, 100, 90, 75, 60, 50, 48, 40, 30]

function round(value: number, digits = 2): number {
  const factor = 10 ** digits
  return Math.round(value * factor) / factor
}

function percentileSorted(sorted: readonly number[], percentile: number): number {
  if (sorted.length === 0) return 0
  if (sorted.length === 1) return sorted[0]
  const rank = (sorted.length - 1) * Math.min(1, Math.max(0, percentile))
  const lo = Math.floor(rank)
  const hi = Math.ceil(rank)
  if (lo === hi) return sorted[lo]
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo)
}

/** 汇总非负有限耗时；无效样本在停止后统一剔除，避免污染报告。 */
export function summarizeDistribution(values: readonly number[]): DistributionSummary {
  const sorted = values
    .filter((value) => Number.isFinite(value) && value >= 0)
    .sort((a, b) => a - b)
  if (sorted.length === 0) return { ...EMPTY_DISTRIBUTION }
  let total = 0
  for (const value of sorted) total += value
  return {
    samples: sorted.length,
    avg: round(total / sorted.length),
    min: round(sorted[0]),
    p50: round(percentileSorted(sorted, 0.5)),
    p95: round(percentileSorted(sorted, 0.95)),
    p99: round(percentileSorted(sorted, 0.99)),
    max: round(sorted[sorted.length - 1]),
  }
}

/**
 * 从录制前的空闲 rAF 间隔估算刷新预算。
 *
 * 取 P20 而非平均值：校准期偶发长帧只会抬高右尾，较快的一组间隔才代表显示器真实 VSync。
 * 若与常见刷新率误差不超过 12%，吸附到标准档；否则保留设备实测值。
 */
export function estimateFrameBudget(intervals: readonly number[], fallbackMs = 1000 / 60): number {
  const sorted = intervals
    .filter((value) => Number.isFinite(value) && value >= 2 && value <= 100)
    .sort((a, b) => a - b)
  if (sorted.length === 0) return round(fallbackMs, 3)
  const candidate = percentileSorted(sorted, 0.2)
  let bestBudget = candidate
  let bestError = Infinity
  for (const hz of COMMON_REFRESH_RATES) {
    const budget = 1000 / hz
    const error = Math.abs(candidate - budget) / budget
    if (error < bestError) {
      bestError = error
      bestBudget = budget
    }
  }
  return round(bestError <= 0.12 ? bestBudget : Math.min(40, Math.max(4, candidate)), 3)
}

/** 由逐帧间隔聚合刷新率感知的卡顿、分位数与 missed VSync。 */
export function summarizeFrameHealth(
  intervals: readonly number[],
  budgetMs = estimateFrameBudget(intervals),
): FrameHealthSummary {
  const valid = intervals.filter((value) => Number.isFinite(value) && value > 0)
  const frameTime = summarizeDistribution(valid)
  const safeBudget = budgetMs > 0 && Number.isFinite(budgetMs) ? budgetMs : 1000 / 60
  let jankFrames = 0
  let missedVsync = 0
  for (const interval of valid) {
    if (interval > safeBudget * 1.5) jankFrames++
    missedVsync += Math.max(0, Math.round(interval / safeBudget) - 1)
  }
  return {
    frames: valid.length > 0 ? valid.length + 1 : 0,
    budgetMs: round(safeBudget, 3),
    refreshRate: round(1000 / safeBudget, 1),
    avgFps: frameTime.avg > 0 ? round(1000 / frameTime.avg, 1) : 0,
    frameTime,
    jankFrames,
    jankRatio: valid.length > 0 ? round(jankFrames / valid.length, 4) : 0,
    missedVsync,
  }
}
