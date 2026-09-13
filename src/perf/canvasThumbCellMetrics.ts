// canvasThumbCellMetrics —— Canvas 网格的有限录制期指标(密集缩略图方案 §7.3):
//   · 首次入屏命中/未命中:格子在「上一帧不可见」的帧首次进入可见集时,是否已有位图可画;
//   · gallery.thumbFirstPaint:首次入屏(缺图)到首次绘出其位图的代理时延——真实呈现时刻要
//     浏览器 trace,这里只提供同条件可比的代理量。
// 仅录制期维护状态:非录制态 beginFrame 直接返回 false,noteCell/endFrame 全 no-op,零分配;
// 会话代次(recordingEpoch)变化时整体重置,上一次录制的可见集与入场时刻不污染新会话。待绘出表
// 有硬上限(超出按插入序淘汰最老),长时间录制也不无界增长。
import { performanceRecorder } from './performanceRecorder'

const MAX_PENDING_FIRST_PAINT = 2048

export interface CanvasThumbCellMetrics {
  /** draw 开头调用:false = 本帧不记(非录制态);每次会话切换后的第一帧只建基线不计数。 */
  beginFrame(now: number): boolean
  /** 可见格逐个上报:draw 是否画出了位图 / 是否已判加载失败。 */
  noteCell(id: number, drewImage: boolean, failed: boolean): void
  /** draw 收尾调用:把当前可见集转为「上一帧」基线(双缓冲复用,无逐帧分配)。 */
  endFrame(): void
  /** 失活/重挂载:丢弃可见集与待绘出表——后台不再积累积分,返回画廊时首帧重新建基线。 */
  reset(): void
}

export function createCanvasThumbCellMetrics(): CanvasThumbCellMetrics {
  let epoch = -1
  let seeded = false
  let frameAt = 0
  let prev = new Set<number>()
  let cur = new Set<number>()
  const pending = new Map<number, number>()

  return {
    beginFrame(now: number): boolean {
      if (!performanceRecorder.isActive()) return false
      const current = performanceRecorder.recordingEpoch()
      if (current !== epoch) {
        epoch = current
        seeded = false
        prev.clear()
        cur.clear()
        pending.clear()
      }
      frameAt = now
      return true
    },

    noteCell(id: number, drewImage: boolean, failed: boolean): void {
      cur.add(id)
      if (drewImage) {
        const since = pending.get(id)
        if (since !== undefined) {
          pending.delete(id)
          performanceRecorder.recordSpan('gallery.thumbFirstPaint', frameAt - since)
        } else if (seeded && !prev.has(id)) {
          performanceRecorder.count('gallery.thumbFirstEntryHit')
        }
        return
      }
      if (failed) {
        pending.delete(id) // 判失败不再等它绘出,避免失败格留下永不结算的入场时刻
        return
      }
      if (seeded && !prev.has(id)) performanceRecorder.count('gallery.thumbFirstEntryMiss')
      if (!pending.has(id)) {
        if (pending.size >= MAX_PENDING_FIRST_PAINT) {
          const oldest = pending.keys().next()
          if (!oldest.done) pending.delete(oldest.value)
        }
        pending.set(id, frameAt)
      }
    },

    endFrame(): void {
      const retired = prev
      prev = cur
      cur = retired
      cur.clear()
      // 待绘出表只保留仍可见的格:滚出屏的格不再等「首次绘制」,否则它下次入屏才画出来时,
      // 计时会把整段屏外停留也算进去。再入屏会按新的一次入屏重新起算。
      if (pending.size > 0) {
        for (const id of pending.keys()) if (!prev.has(id)) pending.delete(id)
      }
      seeded = true
    },

    reset(): void {
      prev.clear()
      cur.clear()
      pending.clear()
      seeded = false
    },
  }
}
