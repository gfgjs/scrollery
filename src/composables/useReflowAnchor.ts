// 重排锚点(原「行高锚点」问题1,2026-07-17 泛化到全部整体重排)。
// 自 MediaGrid.vue 结构拆分抽出,判据/时序逐字保留。
//
// 行高滑块、分组/排序切换、无缝分组、布局模式切换、容器宽度变化都会整体重排布局:
// 旧 scrollTop(layoutVersion watcher 的 scrollCache 兜底路径)映射到完全不同的内容,
// 用户正在看的项跳走。统一做法:重排前捕获视口顶部项(pickReflowAnchor,纯函数单测),
// 重算完成后按该项在新布局的 y 回滚,钉在屏内。同一触发爆发(滑块拖动/连续 resize)
// 只捕获一次,持有到爆发停稳,避免多次重算间漂移。
import { onBeforeUnmount, watch } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { scrollCache } from '../utils/scrollCache'
import { IPC } from '../constants/ipc'
import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'
import { useViewStore } from '../stores/viewStore'
import { pickReflowAnchor } from '../components/media/mediaGrid.helpers'
import type { LayoutRow } from '../types/layout'

export interface ReflowAnchorDeps {
  gridRef: () => HTMLElement | null
  activeRows: () => LayoutRow[]
  currentLogicalY: () => number
  getViewKey: () => string
  bucketActive: () => boolean
  scrollToLogicalY: (y: number) => Promise<void>
  logicalToPhysical: (y: number) => number
}

export function useReflowAnchor(deps: ReflowAnchorDeps) {
  const ui = useUiStore()
  const media = useMediaStore()
  const viewStore = useViewStore()

  const ANCHOR_HOLD_MS = 400
  let pendingAnchor: { id: number; screenOffset: number; viewKey: string } | null = null
  let anchorClearTimer: ReturnType<typeof setTimeout> | null = null

  function scheduleAnchorClear() {
    if (anchorClearTimer !== null) clearTimeout(anchorClearTimer)
    anchorClearTimer = setTimeout(() => {
      anchorClearTimer = null
      // compute IPC 在途(大库 compute_layout 可达秒级)→ 锚点尚未被 layoutVersion watcher
      // 消费,续命等它落地——否则恰在最需要锚点的大库上,锚点先于恢复被清,静默退化走
      // scrollCache 旧坐标(捕获→异步重算→恢复型机制不能裸靠壁钟过期)。
      if (media.isComputingLayout) {
        scheduleAnchorClear()
        return
      }
      pendingAnchor = null
    }, ANCHOR_HOLD_MS)
  }

  function captureReflowAnchor() {
    // 每段爆发只捕获一次;在单段触发的多次重算间保持同一锚点,避免钉住的项漂移。
    scheduleAnchorClear()
    if (pendingAnchor !== null || !deps.gridRef()) return
    const picked = pickReflowAnchor(deps.activeRows(), deps.currentLogicalY())
    if (picked) pendingAnchor = { ...picked, viewKey: deps.getViewKey() }
  }

  async function restoreReflowAnchor(): Promise<boolean> {
    if (!pendingAnchor || !deps.gridRef()) return false
    // 持锚期间切了目录/相册:锚点押的是旧视图的项,拿到新视图里恢复会把滚动位拽去
    // 该项恰好所在的位置——弃锚走 scrollCache(新视图有自己的缓存键)。
    if (pendingAnchor.viewKey !== deps.getViewKey()) {
      pendingAnchor = null
      return false
    }
    const anchor = pendingAnchor
    try {
      const y = await invokeIpc<number | null>(IPC.GET_ITEM_Y_BY_ID, { itemId: anchor.id })
      const el = deps.gridRef()
      if (y !== null && el) {
        const targetY = Math.max(0, y - anchor.screenOffset)
        if (deps.bucketActive()) {
          // bucket:统一入口(B3 映射态重锚自处理);缓存存逻辑 y(映射态物理位不自足)。
          await deps.scrollToLogicalY(targetY)
          scrollCache.set(deps.getViewKey(), targetY)
        } else {
          const physY = deps.logicalToPhysical(targetY)
          el.scrollTop = physY
          scrollCache.set(deps.getViewKey(), physY)
        }
        return true
      }
    } catch (e) {
      logger.error('[MediaGrid] restoreReflowAnchor failed', { error: e })
    }
    return false
  }

  // 在布局重算前捕获(本 watcher 是 pre-flush;useJustifiedLayout 的重算 watcher 是
  // post-flush,因此在我们抓到锚点之后才运行)。触发面与 useJustifiedLayout 的重算源
  // 对应取「项集不变、仅几何/顺序变」的子集——filter/search/视图切换不入:项可能消失
  // 且视图切换本就该走各自的 scrollCache 键。容器宽度变化在 ResizeObserver 回调捕获。
  watch(
    [
      () => ui.gridRowHeight,
      () => ui.groupBy,
      () => ui.sortWithinGroup,
      () => ui.sortOrder,
      () => ui.seamlessGroups,
      () => ui.layoutMode,
    ],
    () => captureReflowAnchor(),
  )

  // 切视图时重置行高锚点（原与 dim-priority 去重集重置共用一个 watcher；去重集那半已随 feature 迁出）。
  watch(
    () => [viewStore.activeDirectoryId, viewStore.activeSmartAlbum],
    () => {
      pendingAnchor = null
    },
  )

  onBeforeUnmount(() => {
    // 重排锚点清除定时器会在 compute 在途时自续命(scheduleAnchorClear),卸载须显式掐断。
    if (anchorClearTimer !== null) {
      clearTimeout(anchorClearTimer)
      anchorClearTimer = null
    }
  })

  return { captureReflowAnchor, restoreReflowAnchor }
}
