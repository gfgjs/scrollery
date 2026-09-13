// src/composables/usePager.ts
// 与渲染器解耦的翻页输入映射（§5.1）。三种翻页模式作用于「输入 → 导航」：
//  1. scroll     —— 网页式连续滚动（滚轮原生滚动）。
//  2. wheel-snap —— 滚轮一格 = 翻一页（preventDefault + 节流吸附）。
//  3. keyboard   —— 强调键盘：↑↓ 滚屏、←→ 翻页（键盘导航在三种模式下恒可用）。
// 预留扩展（双页 / 卷轴 / 自动滚动）只需在此增 mode。
//
// 渲染器只实现 next()/prev()（一页的含义由渲染器定义：文本/pdf=滚动一屏，epub=rendition 翻页），
// usePager 负责把滚轮/键盘事件按当前模式翻译过去，并管理监听器生命周期。

export type PagerMode = 'scroll' | 'wheel-snap' | 'keyboard'

export const PAGER_MODES: PagerMode[] = ['scroll', 'wheel-snap', 'keyboard']

export interface PagerOptions {
  mode: () => PagerMode
  next: () => void
  prev: () => void
  /** 用于 ↑↓/Home/End 与默认翻页步长的滚动容器。 */
  container: () => HTMLElement | null
}

// 滚轮吸附节流（ms）：一次滚动手势内只翻一页。
const SNAP_THROTTLE_MS = 450

export function usePager(opts: PagerOptions) {
  let snapLockUntil = 0
  let attachedEl: HTMLElement | null = null

  function pageStep(): number {
    const c = opts.container()
    return c ? Math.max(80, c.clientHeight * 0.9) : 600
  }

  function onWheel(e: WheelEvent) {
    if (opts.mode() !== 'wheel-snap') return // scroll / keyboard → 原生滚动
    e.preventDefault()
    const now = Date.now()
    if (now < snapLockUntil) return
    snapLockUntil = now + SNAP_THROTTLE_MS
    if (e.deltaY > 0) opts.next()
    else if (e.deltaY < 0) opts.prev()
  }

  function onKeydown(e: KeyboardEvent) {
    // 输入框聚焦时不劫持（替换规则/搜索等）。
    const t = e.target as HTMLElement | null
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return

    const c = opts.container()
    switch (e.key) {
      case 'ArrowRight':
      case 'PageDown':
      case ' ': // Space
        e.preventDefault()
        opts.next()
        break
      case 'ArrowLeft':
      case 'PageUp':
        e.preventDefault()
        opts.prev()
        break
      case 'ArrowDown':
        if (c) {
          e.preventDefault()
          c.scrollBy({ top: pageStep() * 0.3, behavior: 'smooth' })
        }
        break
      case 'ArrowUp':
        if (c) {
          e.preventDefault()
          c.scrollBy({ top: -pageStep() * 0.3, behavior: 'smooth' })
        }
        break
      case 'Home':
        if (c) {
          e.preventDefault()
          c.scrollTo({ top: 0, behavior: 'smooth' })
        }
        break
      case 'End':
        if (c) {
          e.preventDefault()
          c.scrollTo({ top: c.scrollHeight, behavior: 'smooth' })
        }
        break
    }
  }

  /** 挂载监听。传入滚动容器以绑定滚轮（passive:false 便于吸附 preventDefault）；
   *  传 `null` 则仅键盘（如 epub 自行处理滚轮）。 */
  function attach(el: HTMLElement | null) {
    detach()
    attachedEl = el
    if (el) el.addEventListener('wheel', onWheel, { passive: false })
    window.addEventListener('keydown', onKeydown)
  }

  function detach() {
    if (attachedEl) attachedEl.removeEventListener('wheel', onWheel)
    window.removeEventListener('keydown', onKeydown)
    attachedEl = null
  }

  return { attach, detach, pageStep }
}
