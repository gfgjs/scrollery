// 侧栏点击文件夹 → 画廊滚动定位(自 MediaGrid.vue 结构拆分抽出,时序与守卫逐字保留)。
import { onBeforeUnmount, watch } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { scrollCache } from '../utils/scrollCache'
import { IPC } from '../constants/ipc'
import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'

export interface GalleryScrollToDirDeps {
  gridRef: () => HTMLElement | null
  scrollToLogicalY: (y: number, o?: { smooth?: boolean }) => Promise<void>
  getViewKey: () => string
  beginProgrammaticScroll: () => void
  endProgrammaticScroll: () => void
}

export function useGalleryScrollToDir(deps: GalleryScrollToDirDeps) {
  const ui = useUiStore()
  const media = useMediaStore()

  // 按唯一目录 id（分组 id）滚动，而非文件夹名字：不同路径下的同名文件夹各自定位到自己的分隔符。
  async function scrollToDir(dirId: number) {
    // 先把侧栏钉到被点击的文件夹并抑制飞滚联动，使树只定位/高亮目标一次并保持可见（问题2/3）。
    // 这里设置的 scrolledDirectoryId 会触发 FoldersSection 的 expandToNode → scrollTreeToDirId(虚拟化索引滚动)。
    deps.beginProgrammaticScroll()
    if (ui.groupBy === 'folder') ui.scrolledDirectoryId = dirId
    try {
      // 子树感知目标：该文件夹有直接媒体则用其分隔符，否则用首个有媒体的后代子文件夹——
      // 这样点击「空」父文件夹会跳到首个含媒体的子项，而非毫无反应（问题1）。
      const target = await invokeIpc<{ dirId: number; y: number } | null>(
        IPC.GET_SUBTREE_SCROLL_TARGET,
        { dirId },
      )
      if (target && deps.gridRef()) {
        // `y` 是逻辑坐标;scrollCache 在 bucket 引擎下同样存逻辑 y(物理位在映射态不自足)。
        // 落到后代 → 高亮/展开该子文件夹，而非空父文件夹。
        if (ui.groupBy === 'folder' && target.dirId !== dirId) ui.scrolledDirectoryId = target.dirId
        const targetY = Math.max(0, target.y)
        void deps.scrollToLogicalY(targetY, { smooth: true })
        scrollCache.set(deps.getViewKey(), targetY)
      } else {
        // 无需滚动 — 立即放下守卫，不必等待停稳。
        deps.endProgrammaticScroll()
      }
    } catch (e) {
      logger.error('Failed to get scroll target', { error: e })
      deps.endProgrammaticScroll()
    } finally {
      ui.pendingScrollDirId = null
    }
  }

  // 布局计算期间点侧栏文件夹:先把 dirId 记进挂起变量,交由下方顶层 watcher 在计算完成后消费。
  // P1-13:原实现在此异步回调里 new 一个嵌套 watch,该 watch 脱离组件 effect scope(setup 已返回)
  // → 卸载不回收;若计算中途离开画廊,未来 compute 完成时会对已死组件跑 scrollToDir 干扰活组件。
  let pendingScrollWhileComputing: number | null = null
  let deferredScrollTimer: ReturnType<typeof setTimeout> | null = null

  watch(
    () => ui.pendingScrollDirId,
    async (dirId) => {
      if (dirId === null) return
      if (media.isComputingLayout) {
        // 挂起(后到覆盖先到:计算期间连点多个文件夹,只滚最后一个,符合用户意图)。
        pendingScrollWhileComputing = dirId
      } else {
        await scrollToDir(dirId)
      }
    },
  )

  // 顶层静态 watcher(随 setup scope 卸载自动销毁):布局计算结束时消费挂起的滚动目标。
  watch(
    () => media.isComputingLayout,
    (isComp) => {
      if (isComp) return
      const dirId = pendingScrollWhileComputing
      if (dirId === null) return
      pendingScrollWhileComputing = null
      // 稍等一拍,让 layoutVersion watcher 先应用其默认滚动。
      if (deferredScrollTimer) clearTimeout(deferredScrollTimer)
      deferredScrollTimer = setTimeout(() => {
        deferredScrollTimer = null
        void scrollToDir(dirId)
      }, 50)
    },
  )

  onBeforeUnmount(() => {
    // 卸载时清理挂起的延迟滚动定时器(P1-13),避免 50ms 窗口内卸载后仍对死组件滚动。
    if (deferredScrollTimer) clearTimeout(deferredScrollTimer)
  })

  return { scrollToDir }
}
