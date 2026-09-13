// src/utils/lrc.ts
// LRC 歌词解析（P3, §3.6）：把带 `[mm:ss.xx]` 时间轴的 LRC 文本解析为按时间排序的行，
// 供 AudioPlayer 随播放同步高亮/滚动。无时间轴的纯文本歌词由调用方按普通文本展示。

export interface LrcLine {
  /** 该行起始时间（秒）。 */
  time: number
  text: string
}

// 单个时间标签 `[mm:ss]` / `[mm:ss.xx]` / `[mm:ss.xxx]`（全局匹配，一行可有多个）。
const TIME_TAG = /\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g
// 元数据标签如 `[offset:+250]`（毫秒，正=提前/负=延后，遵循 LRC 约定）。
const OFFSET_TAG = /\[offset:\s*([+-]?\d+)\s*\]/i

/**
 * 解析 LRC 文本。返回按时间升序排序的行 + 全局 offset（毫秒）；无时间戳的行丢弃
 *（调用方在 `synced` 为 false 时按纯文本渲染）。
 */
export function parseLrc(raw: string): LrcLine[] {
  let offsetSec = 0
  const offsetMatch = raw.match(OFFSET_TAG)
  if (offsetMatch) {
    // LRC offset: 正值表示歌词提前显示 → 时间减小。
    offsetSec = -Number(offsetMatch[1]) / 1000
  }

  const lines: LrcLine[] = []
  for (const rawLine of raw.split(/\r?\n/)) {
    TIME_TAG.lastIndex = 0
    const stamps: number[] = []
    let m: RegExpExecArray | null
    let lastEnd = 0
    while ((m = TIME_TAG.exec(rawLine)) !== null) {
      const mm = Number(m[1])
      const ss = Number(m[2])
      const frac = m[3] ? Number(`0.${m[3]}`) : 0
      stamps.push(mm * 60 + ss + frac + offsetSec)
      lastEnd = m.index + m[0].length
    }
    if (stamps.length === 0) continue
    const text = rawLine.slice(lastEnd).trim()
    for (const t of stamps) {
      lines.push({ time: Math.max(0, t), text })
    }
  }
  lines.sort((a, b) => a.time - b.time)
  return lines
}

/**
 * 给定当前播放时间（秒），返回应高亮的行索引（最后一个 time <= now 的行），无则 -1。
 * 二分查找，适配长歌词。
 */
export function activeLineIndex(lines: LrcLine[], now: number): number {
  let lo = 0
  let hi = lines.length - 1
  let ans = -1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (lines[mid].time <= now) {
      ans = mid
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  return ans
}
