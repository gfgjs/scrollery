// src/composables/useLensFocusRestore.ts
// 重复镜头排列切换的键盘焦点恢复（2026-09-02 方案 §8.1/§13 P2）。
//
// 切换 groups⇄folders 或开关「显示独有项」会整体重算布局：虚拟滚动把可视窗口整窗换血,原聚焦
// 卡片随节点卸载消失,焦点掉到 body——键盘用户在重排后失位。本 composable 在切换边界捕获
// 聚焦卡片的 item id 与屏幕偏移,待新布局落地后按 id 经 GET_ITEM_Y_BY_ID 恢复滚动位,把焦点移回
// 该 id 的卡片；该 id 已不在新布局（如关闭独有项后原聚焦项是独有项）时回顶部并聚焦镜头状态条
// 主标题（§8.1 明文）。机制按通用写,不假设 P1 只有 groups——P3 folders 排列切换直接生效。
//
// 时序契约：恢复挂在宿主布局重算的滚动恢复链上（MediaGrid 把本 composable 的 restoreLensFocus
// 链在 useReflowAnchor.restoreReflowAnchor 之后,注入 useGalleryTauriSync 的 layoutVersion
// watcher）——即「reflow 锚点 → 镜头焦点锚点 → scrollCache 回退」的单一顺序。镜头排列切换不会
// 捕获 reflow 锚点（其 watch 面不含镜头键,且锚点按 viewKey 校验、排列切换即换键）,二者天然互斥；
// 返回 true 时短路调用方的 scrollCache 回退,保证「回顶部」的明文语义不被旧镜头键缓存覆盖。
import { nextTick, watch } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { scrollCache } from '../utils/scrollCache'
import { IPC } from '../constants/ipc'
import { useDuplicateLensStore } from '../stores/duplicateLensStore'

/** 镜头状态条主标题元素选择器:焦点项已不在新布局时的焦点落点（§8.1「聚焦新布局主标题」）。
 *  与 DuplicateLensStatusBar.vue 的 title span（id + tabindex="-1"）成对,勿单侧改名。 */
export const LENS_STATUS_TITLE_SELECTOR = '#lens-status-title'

/** 聚焦元素等待虚拟行回填的重试上限与间隔。行回填经 updateVisible 的取行 IPC 异步落地,
 *  每拍 nextTick + 微等;10 拍 ≈ 160ms+,远大于本地取行 IPC 延迟,足够收敛。 */
const FOCUS_RETRY_LIMIT = 10
const FOCUS_RETRY_DELAY_MS = 16

export interface LensFocusRestoreDeps {
  gridRef: () => HTMLElement | null
  bucketActive: () => boolean
  scrollToLogicalY: (y: number) => Promise<void>
  logicalToPhysical: (y: number) => number
  getViewKey: () => string
}

export function useLensFocusRestore(deps: LensFocusRestoreDeps) {
  const lens = useDuplicateLensStore()

  /** 待恢复的聚焦项:null = 无待恢复（非镜头路径恒空,恢复链上零开销直返）。 */
  let pendingFocus: { id: number; screenOffset: number } | null = null

  /**
   * 捕获:镜头激活期间 mode/showUniqueItems 变化（切换前后 mode 均非空）→ 记下当前聚焦卡片的
   * item id 与其相对网格容器顶的屏幕偏移（恢复时按「新布局 y − 同一偏移」回滚,与 useReflowAnchor
   * 的 screenOffset 语义一致）。进入（null→mode）/退出（mode→null）不捕获——前者焦点在工具栏
   * chip 上、后者由普通画廊自身的滚动恢复机制负责；焦点不在卡片上（如刚点过分段控件）也不捕获,
   * 焦点随控件自身保留,无需恢复。
   */
  watch(
    () => [lens.mode, lens.showUniqueItems] as const,
    ([mode], [prevMode]) => {
      if (mode === null || prevMode === null) {
        // 镜头退出即弃锚,防陈旧 pending 被无关布局变化消费。
        pendingFocus = null
        return
      }
      pendingFocus = captureFocusedCard()
    },
  )

  function captureFocusedCard(): { id: number; screenOffset: number } | null {
    const grid = deps.gridRef()
    const active = document.activeElement
    if (!grid || !active || !grid.contains(active)) return null
    const card = active.closest('[data-item-id]') as HTMLElement | null
    if (!card) return null
    const id = Number.parseInt(card.dataset.itemId ?? '', 10)
    if (Number.isNaN(id)) return null
    const screenOffset = card.getBoundingClientRect().top - grid.getBoundingClientRect().top
    return { id, screenOffset }
  }

  /**
   * 镜头焦点锚点恢复。返回 true = 本函数已把滚动位安排妥当,调用方跳过 scrollCache 回退;
   * false = 无待恢复（非镜头路径常态）。焦点落卡是行回填后的后续动作,fire-and-forget 不阻塞恢复链。
   */
  async function restoreLensFocus(): Promise<boolean> {
    if (pendingFocus === null) return false
    const { id, screenOffset } = pendingFocus
    pendingFocus = null
    const el = deps.gridRef()
    if (!el) return false

    let y: number | null = null
    try {
      // 新布局中该项所在行的逻辑 y（后端布局缓存已随 layoutVersion 换代,查得即新几何）。
      y = await invokeIpc<number | null>(IPC.GET_ITEM_Y_BY_ID, { itemId: id })
    } catch (e) {
      logger.warn('[LensFocus] get_item_y_by_id failed', { error: e })
    }

    if (y === null) {
      // 焦点项已不在新布局（§8.1）:回顶部 + 聚焦主标题。返回 true 短路 scrollCache 回退——
      // 旧镜头键可能有历史缓存位,不能让它把「回顶部」顶掉。
      if (deps.bucketActive()) await deps.scrollToLogicalY(0)
      else el.scrollTop = 0
      scrollCache.set(deps.getViewKey(), 0)
      void focusTitleFallback()
      return true
    }

    // 滚动位:把该项钉回其切换前的屏幕纵向位置;顺带写入新镜头键的滚动缓存（与重排锚点同惯例）。
    const targetY = Math.max(0, y - screenOffset)
    if (deps.bucketActive()) {
      await deps.scrollToLogicalY(targetY)
      scrollCache.set(deps.getViewKey(), targetY)
    } else {
      const physY = deps.logicalToPhysical(targetY)
      el.scrollTop = physY
      scrollCache.set(deps.getViewKey(), physY)
    }
    void focusItemWhenRendered(id)
    return true
  }

  /** 等虚拟行回填后把焦点移回该 id 的卡片;始终不出现则退而聚焦网格容器,保住键盘滚动能力。 */
  async function focusItemWhenRendered(id: number): Promise<void> {
    const selector = `[data-item-id="${id}"]`
    for (let i = 0; i < FOCUS_RETRY_LIMIT; i++) {
      // nextTick:让调用方恢复链尾随的 updateVisible → 渲染 flush 先落地。
      await nextTick()
      const card = deps.gridRef()?.querySelector(selector) as HTMLElement | null
      if (card) {
        // preventScroll:滚动位已按 item y 精确设定,交浏览器重滚会与虚拟行回填竞态。
        card.focus({ preventScroll: true })
        return
      }
      await new Promise((r) => setTimeout(r, FOCUS_RETRY_DELAY_MS))
    }
    deps.gridRef()?.focus({ preventScroll: true })
  }

  /** 焦点项已不在新布局的兜底:聚焦镜头状态条主标题（常驻 DOM,非虚拟化）。 */
  async function focusTitleFallback(): Promise<void> {
    await nextTick()
    const title = document.querySelector(LENS_STATUS_TITLE_SELECTOR) as HTMLElement | null
    title?.focus({ preventScroll: true })
  }

  return { restoreLensFocus }
}
