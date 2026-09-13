// src/composables/useFolderTreeAutoLoadMore.ts
// 「加载更多」自动接力(T2 单目录分页免手点)——从 FoldersSection.vue 域 14 迁出(见
// docs/planning/2026-07-25-超长文件拆分方案/analysis/FoldersSection-vue.md §2.1)。
//
// more 行进入渲染窗口(含 buffer,相当于就近预取)即自动追加下一页;一页完成后拍平行变化、
// 窗口重算,行若仍在窗口内则继续接力,直到该目录取尽或行滚出窗口。防线:
//  - loadingMoreDirId 单飞:全局同一时刻至多一个在途分页请求(自动/手点共用);
//  - autoLoadBlocked:加载失败或无进展(成功但 hasMore 未收敛)的目录拉黑,只允许手点重试,
//    防「滚进视口→失败→再触发」的无限循环。手点即视为用户要求重试,先解除拉黑。
// 单飞/拉黑均按**路径身份**(D-013):分页是结构操作,行的归属目录只有路径身份是恒有的。
import {
  ref,
  watch,
  nextTick,
  onBeforeUnmount,
  type ComputedRef,
  type Ref,
  type ShallowRef,
} from 'vue'
import {
  firstAutoLoadTarget,
  type TreeRow,
} from '../components/sidebar/sections/folderTree.helpers'
import type { DirNode } from '../types/media'

export interface FolderTreeAutoLoadMoreDeps {
  displayRows: ComputedRef<TreeRow[]>
  visibleRows: ComputedRef<TreeRow[]>
  nodesByKey: ComputedRef<Map<string, DirNode>>
  treeRef: Ref<HTMLElement | null>
  scrollAreaEl: ShallowRef<HTMLElement | null>
  rowH: Ref<number>
  firstVisibleIndex: Ref<number>
  updateWindow: () => void
  getLastScrollTs: () => number
  loadMoreFiles: (node: DirNode) => Promise<void>
  /** 鼠标点击「加载更多」行同步键盘 active 行(activeIndex 现归 useFolderTreeKeyboardNav 持有)。 */
  syncActiveIndex: (idx: number) => void
}

export function useFolderTreeAutoLoadMore(deps: FolderTreeAutoLoadMoreDeps) {
  const {
    displayRows,
    visibleRows,
    nodesByKey,
    treeRef,
    scrollAreaEl,
    rowH,
    firstVisibleIndex,
    updateWindow,
    getLastScrollTs,
    loadMoreFiles,
    syncActiveIndex,
  } = deps

  const SETTLE_MS = 150
  let settleTimer: number | null = null

  const loadingMoreDirKey = ref<string | null>(null)
  const autoLoadBlocked = new Set<string>()

  async function loadMore(dirKey: string) {
    if (loadingMoreDirKey.value !== null) return
    const target = nodesByKey.value.get(dirKey)
    // 只按**路径身份**反查归属目录。取数走 DB 还是 FS 由 useFolderTree 按模式分发(单一分发点),
    // 组件侧不再判 id——FS 模式下 FS-only 目录的文件分页正是要能翻的那批。
    if (!target) return
    loadingMoreDirKey.value = dirKey
    const before = target.files?.length ?? 0
    // 锚点补偿(问题2):记录该 dir 的 more 行在拍平序中的插入位——新文件将插在此处并把 more 行下推。
    const insertIdx = displayRows.value.findIndex((r) => r.kind === 'more' && r.dirKey === dirKey)
    try {
      await loadMoreFiles(target)
      const node = nodesByKey.value.get(dirKey)
      const after = node?.files?.length ?? 0
      const inserted = after - before
      // 插入点在当前视口顶行「之上或恰在其上」(insertIdx ≤ firstVisibleIndex)→ 这些新行会把可见内容
      // 整体下推,补偿 scrollTop += 插入行数×行高,使可见内容原地不动(消除刷屏跳动)。
      // firstVisibleIndex=-1(未滚进树)时 insertIdx(≥0)不满足 ≤,自然不补偿;插在视口下方也不补偿。
      if (
        inserted > 0 &&
        insertIdx >= 0 &&
        insertIdx <= firstVisibleIndex.value &&
        scrollAreaEl.value
      ) {
        scrollAreaEl.value.scrollTop += inserted * rowH.value
        updateWindow() // 立即用新 scrollTop 重算窗口,避免一帧错位
      }
      // 防御:成功但文件数没涨且仍报 hasMore(后端分页异常)→ 拉黑防原地打转。
      if (node?.filesHasMore && after <= before) autoLoadBlocked.add(dirKey)
    } catch {
      autoLoadBlocked.add(dirKey)
    } finally {
      loadingMoreDirKey.value = null
    }
  }

  // 手点兜底入口(失败重试/拉黑解除)。
  async function onLoadMore(dirKey: string, idx?: number) {
    if (idx !== undefined) syncActiveIndex(idx) // 鼠标点击同步键盘 active 行
    autoLoadBlocked.delete(dirKey)
    await loadMore(dirKey)
    maybeAutoLoadMore() // 重试成功且还有更多 → 交回自动接力
  }

  function maybeAutoLoadMore() {
    if (loadingMoreDirKey.value !== null) return
    // 区块折叠时树 display:none(offsetParent 为 null),rect 全零会得到假窗口 → 不自动加载。
    if (!treeRef.value || treeRef.value.offsetParent === null) return
    // 静止门(问题2):快滚期间(距上次用户滚动 < SETTLE_MS)不自动加载,挂尾随重探;等滚动停下再
    // 加载视口内目标,避免给飞掠过的一长串目录发 IPC + 在视口上方插行刷屏。手点(onLoadMore)不经此门。
    const sinceScroll = performance.now() - getLastScrollTs()
    if (sinceScroll < SETTLE_MS) {
      scheduleSettleRetry(SETTLE_MS - sinceScroll)
      return
    }
    const dirKey = firstAutoLoadTarget(visibleRows.value, autoLoadBlocked)
    if (dirKey !== null) void loadMore(dirKey).then(() => nextTick(maybeAutoLoadMore))
  }
  // 尾随重探:滚动停下后再跑一次 maybeAutoLoadMore(去重,同一时刻至多一个在挂)。
  function scheduleSettleRetry(delay: number) {
    if (settleTimer !== null) return
    settleTimer = window.setTimeout(
      () => {
        settleTimer = null
        maybeAutoLoadMore()
      },
      Math.max(0, delay),
    )
  }
  // visibleRows 随滚动(窗口平移)与拍平行变化(追加一页)重算 → 两类时机都会重新探测。
  watch(visibleRows, maybeAutoLoadMore)

  onBeforeUnmount(() => {
    if (settleTimer !== null) clearTimeout(settleTimer) // 静止门尾随定时器兜底清理
  })

  return { loadingMoreDirKey, onLoadMore }
}
