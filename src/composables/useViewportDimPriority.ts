// src/composables/useViewportDimPriority.ts
// 可视窗口优先取尺寸（从 MediaGrid 抽出的自包含关注点，T18 轨道 B）。
//
// 拖动跳转时用户可能落在尚未补全真实尺寸的项上（按占位比例渲染）。本 composable 在滚动停稳后
// 即时测量可视区的占位项（originalWidth/Height ≤ 0），调用后端 PRIORITIZE_DIMENSIONS 抢在自上
// 而下的后台 enrichment 之前补全，再重算使其贴回正确比例。视口上方的项未改动，故重算后视图保持
// 锚定不跳。
//
// 无模板耦合、不向外漏 state：自持去重集 requestedDimIds + 定时器 + 在途标志，仅靠注入的
// visibleRows / isScrolling（输入）与 recompute（回调）工作，resetKey 变化（切视图）时清去重集。
// 故是真解耦的 feature 抽取，而非耦合搬运。补全尺寸后的行回填由布局换代（layoutVersion）
// 驱动引擎重建段表，本 feature 不需另开刷新缝。

import { watch, type Ref } from 'vue'
import type { LayoutRow } from '../types/layout'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'

interface UseViewportDimPriorityOptions {
  /** 当前可视行（虚拟滚动窗口）；其变化触发一次（防抖的）测量调度。 */
  visibleRows: Ref<LayoutRow[]>
  /** 是否正在滚动——滚动中推迟测量，待停稳再测落点窗口而非掠过帧。 */
  isScrolling: Ref<boolean>
  /** 补全尺寸后重算布局（使占位项贴回正确比例）。 */
  recompute: () => Promise<void>
  /** 视图标识 getter；变化（切文件夹/相册）即清去重集，允许新视图重新测量。 */
  resetKey: () => unknown
}

export function useViewportDimPriority(opts: UseViewportDimPriorityOptions) {
  // 已请求测量的 id 去重集——避免对同一占位项重复触发后端测量。
  const requestedDimIds = new Set<number>()
  let dimPriorityTimer: ReturnType<typeof setTimeout> | null = null
  let dimPriorityInFlight = false

  function scheduleDimPriority() {
    if (dimPriorityTimer !== null) clearTimeout(dimPriorityTimer)
    dimPriorityTimer = setTimeout(prioritizeVisibleDimensions, 200)
  }

  async function prioritizeVisibleDimensions() {
    dimPriorityTimer = null
    if (dimPriorityInFlight) return
    // 等滚动停稳再测量落点窗口，而非掠过的中间帧。
    if (opts.isScrolling.value) {
      scheduleDimPriority()
      return
    }

    const ids: number[] = []
    for (const row of opts.visibleRows.value) {
      if (row.rowType !== 'normal') continue
      for (const it of row.items) {
        if ((it.originalWidth <= 0 || it.originalHeight <= 0) && !requestedDimIds.has(it.id)) {
          requestedDimIds.add(it.id)
          ids.push(it.id)
        }
      }
    }
    if (ids.length === 0) return

    dimPriorityInFlight = true
    try {
      const measured = await invokeIpc<number>(IPC.PRIORITIZE_DIMENSIONS, { itemIds: ids })
      if (measured > 0) {
        await opts.recompute()
      }
    } catch (e) {
      for (const id of ids) requestedDimIds.delete(id) // allow a later retry | 允许之后重试
      logger.error('[useViewportDimPriority] prioritize_dimensions failed', { error: e })
    } finally {
      dimPriorityInFlight = false
    }
  }

  // 每当可见窗口变化就测量其中的占位尺寸；切换视图时清去重集（允许新视图重新测量）。
  watch(opts.visibleRows, () => scheduleDimPriority())
  watch(opts.resetKey, () => requestedDimIds.clear())
}
