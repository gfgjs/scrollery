/**
 * 画廊性能探针的纯计算部分:把一串逐帧时间戳聚合成帧率摘要。
 * 抽为纯函数以便单测(承接项目「纯映射进 helpers + spec」惯例);
 * 探针本体 useGalleryPerfProbe 只负责采样与 IO,不含可测逻辑。
 * 详见 docs/designs/2026-07-09-画廊极密网格渲染方案.md §10。
 */

/** 一段滚动内的帧率摘要。 */
export interface FrameSummary {
  /** 采到的帧数。 */
  frames: number
  /** 平均帧率(总时长内的平均间隔倒数)。样本不足 2 帧时为 0。 */
  avgFps: number
  /** 最低瞬时帧率(最长一帧的倒数)——掉帧体感的主因。 */
  minFps: number
  /** 掉帧数:帧间隔超过目标预算 1.5 倍的帧数。 */
  dropped: number
}

/**
 * 由逐帧时间戳(DOMHighResTimeStamp,ms)聚合帧率摘要。
 * @param timestamps 每个 requestAnimationFrame 回调记录的时间戳,单调递增。
 * @param targetFps 目标帧率(默认 60),用于判定「掉帧」阈值。
 */
export function summarizeFrames(timestamps: number[], targetFps = 60): FrameSummary {
  const frames = timestamps.length
  // 少于 2 帧无从算间隔,直接回零(防 NaN/Infinity 污染基线)。
  if (frames < 2) {
    return { frames, avgFps: 0, minFps: 0, dropped: 0 }
  }

  let maxInterval = 0
  let dropped = 0
  const budget = 1000 / targetFps // 单帧预算(ms)
  for (let i = 1; i < frames; i++) {
    const dt = timestamps[i] - timestamps[i - 1]
    if (dt > maxInterval) maxInterval = dt
    // 超过预算 1.5 倍视为一次掉帧(容忍轻微抖动,只记真正的卡顿)。
    if (dt > budget * 1.5) dropped++
  }

  const totalMs = timestamps[frames - 1] - timestamps[0]
  const avgFps = totalMs > 0 ? ((frames - 1) / totalMs) * 1000 : 0
  const minFps = maxInterval > 0 ? 1000 / maxInterval : 0

  return { frames, avgFps: round1(avgFps), minFps: round1(minFps), dropped }
}

/** 保留一位小数,让基线日志稳定可读。 */
function round1(n: number): number {
  return Math.round(n * 10) / 10
}
