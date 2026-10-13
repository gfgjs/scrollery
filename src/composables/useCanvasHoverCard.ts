// Canvas 悬停只管理单例交互和进退场，静态内容始终由 Canvas 管线绘制。
import { ref, shallowRef, computed, onScopeDispose } from 'vue'
import type { Ref } from 'vue'
import { isThumbLoadDeferred } from './useThumbLoadGate'
import { computeHoverRect, type HoverRect } from '../components/media/mediaGridCanvas.helpers'
import type { LayoutRowItem } from '../types/layout'
import type { CanvasHitTest } from './useCanvasHitTest'

export const HOVER_ENTER_MS = 180
export const HOVER_LEAVE_MS = 140

interface HoverCard {
  item: LayoutRowItem
  base: { x: number; y: number; w: number; h: number }
  rect: HoverRect
  ready: boolean
  expanded: boolean
  leaving: boolean
}

/** 悬停卡与主画布共用同一格级事件出口。 */
export interface CanvasHoverCardEmit {
  (event: 'cell-click', item: LayoutRowItem, e: MouseEvent): void
  (event: 'cell-contextmenu', item: LayoutRowItem, e: MouseEvent): void
  (event: 'cell-pointerdown', id: number, e: PointerEvent, onHandle: boolean): void
}

export interface CanvasHoverCardDeps {
  canvasRef: Ref<HTMLCanvasElement | null>
  scrolling: () => boolean
  isSelectionMode: () => boolean
  enableHoverScale: () => boolean
  demoPrivacy: () => boolean
  isPendingDelete: (id: number) => boolean
  isSelected: (id: number) => boolean
  currentY: () => number
  selectionVersion: () => number
  viewport: () => { w: number; h: number }
  hitTest: Pick<CanvasHitTest, 'pickWithRow' | 'hitHandleAt'>
  requestRedraw: () => void
  emit: CanvasHoverCardEmit
}

export function useCanvasHoverCard(deps: CanvasHoverCardDeps) {
  const hoverLayerRef = ref<HTMLElement | null>(null)
  const hoverCard = shallowRef<HoverCard | null>(null)
  const hoverSelected = computed(() => {
    void deps.selectionVersion()
    return hoverCard.value ? deps.isSelected(hoverCard.value.item.id) : false
  })
  let enterFrame: number | null = null
  let leaveTimer: ReturnType<typeof setTimeout> | null = null

  function cancelAnimation() {
    if (enterFrame !== null) cancelAnimationFrame(enterFrame)
    enterFrame = null
    if (leaveTimer !== null) clearTimeout(leaveTimer)
    leaveTimer = null
  }

  function clearHover() {
    cancelAnimation()
    if (hoverCard.value) deps.requestRedraw()
    hoverCard.value = null
    if (deps.canvasRef.value) deps.canvasRef.value.style.cursor = 'default'
  }

  /** 宿主已画好单例内容面后才接管原格，下一帧只启动合成器变换。 */
  function markHoverReady() {
    const card = hoverCard.value
    if (!card || card.ready) return
    hoverCard.value = { ...card, ready: true }
    enterFrame = requestAnimationFrame(() => {
      enterFrame = null
      if (hoverCard.value?.item.id === card.item.id && !hoverCard.value.leaving) {
        hoverCard.value = { ...hoverCard.value, expanded: true }
      }
    })
  }

  function leaveHover() {
    const card = hoverCard.value
    if (!card || card.leaving) return
    if (!card.ready) {
      clearHover()
      return
    }
    cancelAnimation()
    hoverCard.value = { ...card, leaving: true, expanded: false }
    deps.requestRedraw()
    leaveTimer = setTimeout(() => {
      leaveTimer = null
      clearHover()
    }, HOVER_LEAVE_MS)
  }

  function resumeHover() {
    const card = hoverCard.value
    if (!card?.leaving) return
    cancelAnimation()
    hoverCard.value = { ...card, leaving: false, expanded: true }
    deps.requestRedraw()
  }

  function handleHoverTracking(e: PointerEvent, fromCard: boolean) {
    if (deps.scrolling() || isThumbLoadDeferred() || deps.demoPrivacy() || e.buttons !== 0) {
      clearHover()
      return
    }
    if (fromCard) {
      resumeHover()
      return
    }
    const hit = deps.hitTest.pickWithRow(e)
    if (deps.canvasRef.value) deps.canvasRef.value.style.cursor = hit ? 'pointer' : 'default'
    if (!hit) {
      leaveHover()
      return
    }
    if (deps.isPendingDelete(hit.item.id)) {
      clearHover()
      return
    }
    if (hit.item.id === hoverCard.value?.item.id) {
      resumeHover()
      return
    }
    cancelAnimation()
    const base = { x: hit.item.x, y: hit.rowY - deps.currentY(), w: hit.item.w, h: hit.item.h }
    const view = deps.viewport()
    const rect = computeHoverRect(
      base,
      view.w,
      view.h,
      120,
      1.06,
      1.2,
      deps.enableHoverScale() && !deps.isSelectionMode(),
    )
    hoverCard.value = { item: hit.item, base, rect, ready: false, expanded: false, leaving: false }
    deps.requestRedraw()
  }

  function onPointerMove(e: PointerEvent) {
    handleHoverTracking(e, false)
  }
  function onHoverCardPointerMove(e: PointerEvent) {
    handleHoverTracking(e, true)
  }
  function onCanvasPointerLeave(e: PointerEvent) {
    if (e.relatedTarget && hoverLayerRef.value?.contains(e.relatedTarget as Node)) return
    leaveHover()
  }
  function onHoverCardPointerLeave(e: PointerEvent) {
    const hit = deps.hitTest.pickWithRow(e)
    if (hit?.item.id === hoverCard.value?.item.id) return
    handleHoverTracking(e, false)
  }
  function onHoverCardClick(e: MouseEvent) {
    const card = hoverCard.value
    if (card?.ready && !card.leaving) deps.emit('cell-click', card.item, e)
  }
  function onHoverCardContextMenu(e: MouseEvent) {
    const card = hoverCard.value
    if (card?.ready && !card.leaving) deps.emit('cell-contextmenu', card.item, e)
  }
  function onHoverCardPointerDown(e: PointerEvent) {
    const card = hoverCard.value
    if (!card?.ready || card.leaving) return
    // 放大后的手柄按实际 DOM 目标命中，不再拿原格坐标推算放大位置。
    const target = e.target as HTMLElement | null
    const onHandle = !!target?.closest?.('.media-thumb__drag-handle') || deps.hitTest.hitHandleAt(e)
    deps.emit('cell-pointerdown', card.item.id, e, onHandle)
  }
  onScopeDispose(clearHover)
  return {
    hoverLayerRef,
    hoverCard,
    hoverSelected,
    clearHover,
    markHoverReady,
    onPointerMove,
    onHoverCardPointerMove,
    onCanvasPointerLeave,
    onHoverCardPointerLeave,
    onHoverCardClick,
    onHoverCardContextMenu,
    onHoverCardPointerDown,
  }
}
