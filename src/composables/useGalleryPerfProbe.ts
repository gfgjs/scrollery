import { watch, onMounted, onBeforeUnmount } from 'vue'
import { summarizeFrames } from './galleryPerfProbe.helpers'

/**
 * 画廊性能探针(dev 门控)。仅当 localStorage['scrollery.debug.perfProbe'] 为真时激活;
 * 关闭时**立即 return、不注册任何 watch/监听** —— 生产零开销(镜像 useVirtualScroll 的
 * scrollery.debug.safeMax dev-flag 惯例)。
 *
 * 采三项基线(对应 docs/designs/2026-07-09-画廊极密网格渲染方案.md §1.1 三根因):
 *  1. DOM 节点普查 `window.__scrolleryPerf.census()` —— 根因 1(单屏节点数)
 *  2. 滚动 FPS(isScrolling 期间 rAF 采样,停稳打印 min/avg/dropped)—— 根因 1/2(快滚 churn)
 *  3. 进入选择态耗时 selectEnterMs + 新增节点数 —— 根因 3(响应式/样式失效爆炸)
 *
 * 观察者效应控制:关闭即死代码;以 getter 注入,不在宿主热路径 render 里加响应式依赖。
 */

const DEBUG_KEY = 'scrollery.debug.perfProbe'

interface GalleryPerfProbeOptions {
  /** 画廊滚动容器(.media-grid)。getter 注入,避免与 ref/readonly-ref 类型耦合。 */
  gridEl: () => HTMLElement | null
  /** 是否处于滚动中(驱动 FPS 采样窗口)。 */
  isScrolling: () => boolean
  /** 是否处于选择态(驱动进入选择态耗时测量)。 */
  isSelectionMode: () => boolean
}

interface CensusResult {
  /** 网格内 .media-card 总数(含缓冲)。 */
  cards: number
  /** 网格子树全部元素节点数(复刻 devtools childNodes 取证)。 */
  totalNodes: number
  /** 外接矩形与容器视口相交的卡片数(真·可见格)。 */
  visibleCards: number
}

interface PerfProbeBridge {
  census: () => CensusResult | null
}

declare global {
  interface Window {
    __scrolleryPerf?: PerfProbeBridge
  }
}

function isEnabled(): boolean {
  try {
    return !!localStorage.getItem(DEBUG_KEY)
  } catch {
    return false // localStorage 不可用(隐私模式等)→ 视为关闭
  }
}

export function useGalleryPerfProbe(opts: GalleryPerfProbeOptions): void {
  // 关闭时零成本:不注册任何 watch/hook/监听。门控值会话内稳定,故条件式注册对每个组件实例确定。
  if (!isEnabled()) return

  // ── ① DOM 节点普查(手动调,复刻 devtools temp1.childElementCount 取证)──
  function census(): CensusResult | null {
    const grid = opts.gridEl()
    if (!grid) return null
    const cards = grid.querySelectorAll('.media-card').length
    const totalNodes = grid.querySelectorAll('*').length
    // 可见格:卡片外接矩形与容器视口纵向相交。O(cards) getBoundingClientRect,仅手动调用故可接受。
    const box = grid.getBoundingClientRect()
    let visibleCards = 0
    grid.querySelectorAll('.media-card').forEach((el) => {
      const r = (el as HTMLElement).getBoundingClientRect()
      if (r.bottom > box.top && r.top < box.bottom) visibleCards++
    })
    return { cards, totalNodes, visibleCards }
  }

  // ── ② 滚动 FPS 采样(用 rAF 回调自带的时间戳,不额外 performance.now)──
  let rafId: number | null = null
  let frameTimes: number[] = []
  function tick(t: number): void {
    frameTimes.push(t)
    rafId = requestAnimationFrame(tick)
  }
  function startSampling(): void {
    frameTimes = []
    rafId = requestAnimationFrame(tick)
  }
  function stopSampling(): void {
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    if (frameTimes.length > 2) {
      console.info('[perfProbe] scroll', summarizeFrames(frameTimes))
    }
    frameTimes = []
  }
  watch(
    () => opts.isScrolling(),
    (scrolling) => {
      if (scrolling) startSampling()
      else stopSampling()
    },
  )

  // ── ③ 进入选择态耗时 + 新增节点数 ──
  watch(
    () => opts.isSelectionMode(),
    (on) => {
      if (!on) return
      const grid = opts.gridEl()
      const before = grid ? grid.querySelectorAll('*').length : 0
      const t0 = performance.now()
      // double-rAF:等 Vue flush + 浏览器布局/首帧后再量,近似「同步重渲染 + 布局 + 绘制」总耗时。
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const after = grid ? grid.querySelectorAll('*').length : 0
          console.info('[perfProbe] selectEnter', {
            ms: Math.round((performance.now() - t0) * 10) / 10,
            addedNodes: after - before,
          })
        }),
      )
    },
  )

  onMounted(() => {
    window.__scrolleryPerf = { census }
    console.info('[perfProbe] enabled — call window.__scrolleryPerf.census()')
  })
  onBeforeUnmount(() => {
    stopSampling()
    if (window.__scrolleryPerf) delete window.__scrolleryPerf
  })
}
