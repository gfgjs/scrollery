// src/composables/useMediaDragToFolder.ts
// 画廊媒体拖拽到文件夹树（T18 巨组件拆分：从 MediaGrid 抽出的自包含特性簇）。
//
// 设计（保持原行为逐字不变）：
//  - 浮动幽灵以**命令式**定位（对模板 ref 写 transform / textContent），使 120fps 拖拽绝不触发
//    巨大虚拟网格的 Vue 重渲染（问题3）；只有悬停目标 dir id 是响应式，且仅在真正变化时写。
//  - 尾随 click 抑制统一走 selection 的拖拽标志（T5，消 mediaWasDrag 双轨）。
//  - 模式遵循 OS 习惯：左键拖=移动；Shift+左键=复制；右键拖=落点弹「移动/复制」菜单。
//
// 幽灵 DOM 与右键菜单容器仍留在 MediaGrid 模板（视图归属），经 deps 注入其 ref/state——
// 本 composable 只持有拖拽状态机与命中/落点逻辑，不漏 state 回模板（真解耦，非 coupling-relocation）。

import { markRaw } from 'vue'
import type { Ref } from 'vue'
import { Copy, FolderInput } from '@lucide/vue'

import { beginPointerDrag } from './usePointerDrag'
import { resolveCardPointerAction } from './cardPointerAction'
import { useSelection } from './useSelection'
import i18n from '../i18n'
import { useUiStore } from '../stores/uiStore'
import { useToastStore } from '../stores/toastStore'
import { useHistoryStore } from '../stores/historyStore'
import type { ContextMenuItem } from '../components/common/ContextMenu.vue'

/**
 * 卡片手柄拖拽的起拖阈值(px):0 = 首次移动即起拖(≈选择态胶囊手柄的零打滑手感)。
 * 手柄按下无点击语义(已选中格的手柄按下只为拖拽,原地松手=空操作),故无需距离阈值护点击。
 * 特意与 usePointerDrag 的 DRAG_THRESHOLD(=5,选区框选 / 侧栏拖入文件夹共享)**解耦**:归零手柄拖拽
 * 不牵连那些仍需 5px 防误触的手势。
 */
const HANDLE_DRAG_THRESHOLD = 0

/** 右键菜单容器状态（与 MediaGrid 的 ctxMenu ref 同形；drop 菜单复用它）。 */
interface CtxMenuState {
  visible: boolean
  x: number
  y: number
  items: ContextMenuItem[]
  targetId: number | null
}

interface MediaDragToFolderDeps {
  selection: ReturnType<typeof useSelection>
  ui: ReturnType<typeof useUiStore>
  history: ReturnType<typeof useHistoryStore>
  /** §8.1 browse-only 谓词:重复镜头激活时禁用框选(反转扫选)与拖图起手（宿主从 duplicateLensStore 注入）。 */
  lensActive: () => boolean
  /** 右键菜单容器（drop 菜单写入 x/y/items/visible）。 */
  ctxMenu: Ref<CtxMenuState>
  /** 浮动幽灵根元素（命令式定位）。 */
  ghostEl: Ref<HTMLElement | null>
  /** 幽灵内「移动/复制/?」徽标。 */
  ghostBadgeEl: Ref<HTMLElement | null>
  /** 幽灵内「N 项」文本。 */
  ghostTextEl: Ref<HTMLElement | null>
}

export function useMediaDragToFolder(deps: MediaDragToFolderDeps) {
  const { selection, ui, history, ctxMenu } = deps
  const toast = useToastStore()
  // 映射回原变量名，使下方函数体与 MediaGrid 原实现逐字一致（零转写风险）。
  const mediaGhostEl = deps.ghostEl
  const mediaGhostBadgeEl = deps.ghostBadgeEl
  const mediaGhostTextEl = deps.ghostTextEl

  // rAF 批处理的拖拽帧状态 —— 全命令式，每次 pointermove 不写响应式（问题3）。
  let dragRaf: number | null = null
  let dragX = 0,
    dragY = 0,
    dragShift = false
  let dragKind: 'left' | 'right' = 'left'

  function ghostModeLabel(): string {
    if (dragKind === 'right') return '?'
    return dragShift ? i18n.global.t('common.copy') : i18n.global.t('common.move')
  }

  function paintDragFrame() {
    dragRaf = null
    if (mediaGhostEl.value)
      mediaGhostEl.value.style.transform = `translate3d(${dragX + 14}px, ${dragY + 10}px, 0)`
    if (mediaGhostBadgeEl.value) mediaGhostBadgeEl.value.textContent = ghostModeLabel()
    // 命中文件夹树；仅在目标变化时写响应式状态，使侧栏最多每越过一个文件夹重渲染一次（非每帧）。
    const el = (document.elementFromPoint(dragX, dragY) as HTMLElement | null)?.closest(
      '[data-dir-id]',
    ) as HTMLElement | null
    const targetId = el ? Number(el.dataset.dirId) : null
    if (targetId !== ui.mediaDragHoverDirId) ui.mediaDragHoverDirId = targetId
  }

  // 右键拖拽期间吞掉系统右键菜单（落点处弹我们自己的菜单）。
  function suppressContextMenu(e: Event) {
    e.preventDefault()
    e.stopPropagation()
  }

  function startMediaDrag(e: PointerEvent, kind: 'left' | 'right') {
    const ids = selection.materializeIds()
    if (ids.length === 0) return
    const startX = e.clientX,
      startY = e.clientY
    let dragging = false
    dragKind = kind
    dragShift = e.shiftKey

    beginPointerDrag(
      (ev) => {
        dragX = ev.clientX
        dragY = ev.clientY
        dragShift = ev.shiftKey
        if (!dragging) {
          // 阈值 0 → 首移即拖(≈胶囊手柄零打滑);>0 时为曼哈顿距离门限。见 HANDLE_DRAG_THRESHOLD 说明。
          if (
            Math.abs(ev.clientX - startX) + Math.abs(ev.clientY - startY) <
            HANDLE_DRAG_THRESHOLD
          )
            return
          dragging = true
          selection.markDragMoved()
          document.body.style.userSelect = 'none'
          document.body.style.cursor = 'grabbing'
          if (kind === 'right') window.addEventListener('contextmenu', suppressContextMenu, true)
          const g = mediaGhostEl.value
          if (g) {
            g.classList.add('is-active')
            if (mediaGhostTextEl.value)
              mediaGhostTextEl.value.textContent = i18n.global.t('common.itemCount', {
                count: ids.length,
              })
          }
        }
        if (dragRaf === null) dragRaf = requestAnimationFrame(paintDragFrame)
      },
      (ev, cancelled) => {
        if (dragRaf !== null) {
          cancelAnimationFrame(dragRaf)
          dragRaf = null
        }
        const targetDirId = ui.mediaDragHoverDirId
        const shift = ev.shiftKey
        ui.mediaDragHoverDirId = null
        mediaGhostEl.value?.classList.remove('is-active')
        if (kind === 'right') {
          // 下一拍再移除抑制器，确保落点自身的 contextmenu 仍被吞掉。
          setTimeout(() => window.removeEventListener('contextmenu', suppressContextMenu, true), 0)
        }
        if (cancelled || !dragging || targetDirId == null) return
        if (kind === 'right') {
          showDropMenu(ev.clientX, ev.clientY, ids, targetDirId)
        } else {
          performMediaDrop(ids, targetDirId, shift ? 'copy' : 'move')
        }
      },
    )
  }

  // 右键拖拽落点菜单 —— 让用户选择移动或复制（系统右键拖拽习惯）。
  function showDropMenu(x: number, y: number, ids: number[], targetDirId: number) {
    ctxMenu.value.x = x
    ctxMenu.value.y = y
    ctxMenu.value.items = [
      {
        id: 'move_here',
        label: i18n.global.t('dragDrop.moveHere'),
        icon: markRaw(FolderInput),
        action: () => performMediaDrop(ids, targetDirId, 'move'),
      },
      {
        id: 'copy_here',
        label: i18n.global.t('dragDrop.copyHere'),
        icon: markRaw(Copy),
        action: () => performMediaDrop(ids, targetDirId, 'copy'),
      },
    ]
    ctxMenu.value.visible = true
  }

  async function performMediaDrop(ids: number[], targetDirId: number, mode: 'move' | 'copy') {
    try {
      const n =
        mode === 'copy'
          ? await history.copyMedia(ids, targetDirId, `复制 ${ids.length} 项`)
          : await history.moveMedia(ids, targetDirId, `移动 ${ids.length} 项`)
      if (n > 0) {
        if (mode === 'move') selection.clearSelection()
        toast.addToast(
          'success',
          mode === 'copy'
            ? i18n.global.t('common.copiedCount', { count: n })
            : i18n.global.t('common.movedCount', { count: n }),
        )
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      toast.addToast(
        'error',
        mode === 'copy'
          ? i18n.global.t('common.copyFailed', { error: msg })
          : i18n.global.t('common.moveFailed', { error: msg }),
      )
    }
  }

  // Card pointerdown 路由:media-drag vs selection(问题5/问题2)。拖到文件夹按按钮分流:左键仅「已选中格手柄」触发(本体左键=反转扫选);右键「已选中格整卡」
  // (手柄或本体,保持旧行为)。左键拖=移动到;Shift+左键拖=复制到;右键拖=落点弹菜单(移动/复制);
  // 右键单击(未越阈)照常弹常规右键菜单。
  function onCardPointerDown(id: number, e: PointerEvent, onHandle?: boolean) {
    // 交互起手:统一复位拖拽标志（覆盖下面分流的两条路径——拖图 / 框选），尾随 click 据此抑制。
    selection.beginInteraction()
    // §8.1 browse-only:重复镜头不进入选择,框选(反转扫选)与拖图一律禁用;起手复位照常执行,
    // 保证尾随 click(打开查看器)不被上一次交互残留的拖拽标志误吞。
    if (deps.lensActive()) return
    // 手柄命中:Canvas 路径由宿主 hitHandle 显式传入 onHandle;DOM 路径缺省 → e.target 就近判定
    // .media-thumb__drag-handle(悬停卡走的也是真实 MediaThumb DOM,同一兜底覆盖)。
    const target = e.target as HTMLElement | null
    const handle = onHandle ?? !!target?.closest('.media-thumb__drag-handle')
    const onSelectedItem = selection.isSelectionMode.value && selection.isSelected(id)
    // 分流决策抽为纯函数 resolveCardPointerAction(穷举单测钉死,防右键拖拽这类回归复发):
    //  - 左键:手柄→drag,本体→sweep(反转扫选);右键:已选中整卡→drag(落点菜单,保持旧行为),否则→none。
    const action = resolveCardPointerAction(e.button, onSelectedItem, handle)
    if (action === 'drag') {
      // 左键：阻止原生图片拖拽/文本选择；右键不拦,使未移动的右键单击仍弹出常规右键菜单。
      if (e.button === 0) e.preventDefault()
      startMediaDrag(e, e.button === 2 ? 'right' : 'left')
    } else if (action === 'sweep') {
      selection.onPointerDown(id, e)
    }
    // action==='none'(未选中格右键/中键等):不处理,交给 @contextmenu 弹常规菜单。
  }

  return { onCardPointerDown }
}
