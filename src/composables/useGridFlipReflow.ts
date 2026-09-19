// src/composables/useGridFlipReflow.ts
// 网格删除/移除后的平滑重排动画(FLIP + 淡出),从 MediaGrid 抽出的自包含 DOM 工具
// (T18 轨道 B:仅依赖段容器 getter,与 selection/mediaStore/对话框等全解耦)。
//
// 为什么需要 FLIP:justify 布局在后端算,前端无法本地增量重排,删除一项后整张布局重算,
// 若不处理幸存格子会从旧位置瞬跳到新位置。FLIP(First-Last-Invert-Play):重算前快照各格
// rect(First),重算后读新 rect(Last),先无过渡反向位移回旧位(Invert),再下一帧过渡回
// 原位(Play)。按 data-item-id 匹配——重算会销毁重建行 DOM,但 item.id 稳定,动画仍能续接。
//
// 仅用于删除/移除路径,绝不挂到滚动驱动的取行(避免与虚拟滚动取数打架)。

import { nextTick } from 'vue'

const FLIP_MS = 260
const FADE_MS = 170

/**
 * @param layerRef 渲染层元素的 getter（其下的 `[data-item-id]` 格子参与动画）。
 * @returns flipReflow（执行改布局的 mutate 并对幸存格子做 FLIP）、fadeOutCells（删除前淡出被删格子）。
 */
export function useGridFlipReflow(layerRef: () => HTMLElement | null) {
  function snapshotCellRects(): Map<number, DOMRect> {
    const m = new Map<number, DOMRect>()
    const root = layerRef()
    if (!root) return m
    root.querySelectorAll<HTMLElement>('[data-item-id]').forEach((el) => {
      const id = Number(el.dataset.itemId)
      if (!Number.isNaN(id)) m.set(id, el.getBoundingClientRect())
    })
    return m
  }

  /** 执行会改变布局的 `mutate`(删除/移除 → 重算),并对幸存格子做 FLIP 平滑过渡。 */
  async function flipReflow(mutate: () => Promise<void>) {
    const first = snapshotCellRects()

    await mutate()
    await nextTick() // 等新布局渲染出 DOM，才能读到「Last」位置

    if (!first || first.size === 0) return
    const root = layerRef()
    if (!root) return

    const moved: HTMLElement[] = []
    root.querySelectorAll<HTMLElement>('[data-item-id]').forEach((el) => {
      const id = Number(el.dataset.itemId)
      const prev = first.get(id)
      if (!prev) return // 新进入视口的项无旧位置，不参与
      const now = el.getBoundingClientRect()
      const dx = prev.left - now.left
      const dy = prev.top - now.top
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return // 位置未变，跳过
      // Invert：先无过渡地瞬移回旧位置。
      el.style.transition = 'none'
      el.style.transform = `translate(${dx}px, ${dy}px)`
      moved.push(el)
    })
    if (moved.length === 0) return

    // 强制一次 reflow 让 invert 落地，再 Play（下一帧过渡回原位 transform:''）。
    void root.offsetHeight
    requestAnimationFrame(() => {
      moved.forEach((el) => {
        el.style.transition = `transform ${FLIP_MS}ms cubic-bezier(0.22, 0.61, 0.36, 1)`
        el.style.transform = ''
      })
      // 动画结束清理 inline 样式，避免残留干扰后续渲染/滚动。
      setTimeout(() => {
        moved.forEach((el) => {
          el.style.transition = ''
          el.style.transform = ''
        })
      }, FLIP_MS + 50)
    })
  }

  /**
   * 删除前先让被删格子「淡出 + 缩小」，再由调用方重算 → 幸存格子 FLIP 滑入填补。
   * 必须在 compute() 销毁旧 DOM 之前对这些格子做动画（按 data-item-id 命中可见的被删项；
   * 不可见的（虚拟滚动窗外）本就不在屏，直接被重算移除即可）。返回淡出耗时后 resolve。
   */
  async function fadeOutCells(ids: number[]) {
    const root = layerRef()
    if (!root) return
    const idSet = new Set(ids)
    const cells: HTMLElement[] = []
    root.querySelectorAll<HTMLElement>('[data-item-id]').forEach((el) => {
      if (idSet.has(Number(el.dataset.itemId))) cells.push(el)
    })
    if (cells.length === 0) return
    cells.forEach((el) => {
      el.style.transformOrigin = 'center'
      el.style.transition = `opacity ${FADE_MS}ms ease, transform ${FADE_MS}ms ease`
      el.style.pointerEvents = 'none'
      el.style.opacity = '0'
      el.style.transform = 'scale(0.82)'
    })
    // 等淡出放完再重算（重算会销毁这些 DOM，无需手动清理 inline 样式）。
    await new Promise((r) => setTimeout(r, FADE_MS))
  }

  return { flipReflow, fadeOutCells }
}
