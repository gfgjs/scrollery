// 画廊滚动闸门 + 程序化滚动守卫(自 MediaGrid.vue 结构拆分抽出,阈值与滞回逐字保留)。
//
// 拆分方案 §3.3 取「方案 A」:onGridScroll 主体仍留根组件(多域汇合点),此处只承载纯判定的
// 状态机——速度采样/滞回、程序化落点豁免、滚动条拖拽链关闸、释放去抖。
// 阈值常量(GATE_RELEASE_MS / INTERNAL_HOP_CHAIN_MS / SCROLLBAR_DRAG_CHAIN_MS)不得改动。
import {
  scrollVelocity,
  shouldDeferThumbLoad,
  thumbGateThresholds,
} from '../components/media/mediaGrid.helpers'
import { setDeferThumbLoad, isThumbLoadDeferred } from './useThumbLoadGate'

export interface GalleryScrollGateDeps {
  /** 当前网格行高:双阈值随行高缩放(thumbGateThresholds)。 */
  rowHeight: () => number
}

export function useGalleryScrollGate(deps: GalleryScrollGateDeps) {
  // B(快滚甩滚低保真):速度闸门采样基准。相邻 scroll 事件的物理位移/间隔 → 瞬时速度,
  // 高于阈值判为飞掠 → 抑制缩略图加载启动(见 setDeferThumbLoad)。
  let lastScrollTop = 0
  let lastScrollSampleTs = 0
  // B-甲(削停稳后延迟):闸门专用短释放去抖。无极滚轮急停时最后一帧速度高、闸门关,若只靠
  // 150ms settle 兜底放行则出图偏慢。滚动停顿达 GATE_RELEASE_MS(~4 帧)即放行,比 settle 早
  // ~86ms 出图;取值须大于飞掠中的帧间隔(~16ms)以免动量途中误放。
  const GATE_RELEASE_MS = 64
  let gateReleaseTimer: ReturnType<typeof setTimeout> | null = null
  // F1(程序化落点豁免):bucket 引擎偿债/远跳自触发的 scroll 事件不是用户手势,单帧巨位移
  // 会被速度采样误判为飞掠,给落点凭空加 ~64ms 出图延迟。孤立跳(scrubber/侧栏点击、恢复位)
  // 落点即终点 → 立即放行;相邻 < INTERNAL_HOP_CHAIN_MS 的连续程序化跳(自研滚动条快拖的
  // 逐事件远跳)语义上就是飞掠 → 关闸,由释放去抖兜底放行(取值镜像引擎 SCROLL_CHAIN_MS)。
  const INTERNAL_HOP_CHAIN_MS = 100
  let lastInternalHopTs = -Infinity

  // 拖拽链关闸(2026-07-10 起源,2026-07-17 Canvas 裁决修订):相邻 jump < 链阈值判为
  // 拖拽中 → 强制关闭 DOM 逐卡加载与 Canvas 视口外预取，防逐帧预取洪流；Canvas **可见区**
  // 例外，仍在 64 全局槽与大跨度旧 fetch 取消的约束下持续加载真实缩略图。旧的“快速拖动只显
  // 占位、停下出图”体验已被用户推翻。松手后事件停 → GATE_RELEASE(64ms)恢复预取；孤立跳
  // (轨道点击/scrubber)不成链，落点加载不受影响。
  const SCROLLBAR_DRAG_CHAIN_MS = 150
  let lastScrollbarJumpTs = -Infinity
  let scrollbarDragUntil = -Infinity

  // ── 程序化滚动守卫（侧栏点击飞滚） ───────────────────────────────────────────
  // 为 true 时 onGridScroll 抑制 画廊→侧栏 的文件夹联动，使树不追逐点击触发的平滑滚动
  // 飞过的每个文件夹。滚动停稳后清除；并设安全计时器，以防完全没有滚动事件触发
  // （例如目标位置等于当前位置）。
  let programmaticScroll = false
  let programmaticScrollSafety: ReturnType<typeof setTimeout> | null = null

  function beginProgrammaticScroll() {
    programmaticScroll = true
    if (programmaticScrollSafety !== null) clearTimeout(programmaticScrollSafety)
    programmaticScrollSafety = setTimeout(() => {
      programmaticScroll = false
    }, 1500)
  }

  function endProgrammaticScroll() {
    programmaticScroll = false
    if (programmaticScrollSafety !== null) {
      clearTimeout(programmaticScrollSafety)
      programmaticScrollSafety = null
    }
  }

  function isProgrammaticScroll(): boolean {
    return programmaticScroll
  }

  /**
   * 轴侧(自研滚动条 / minimap)跳转的拖拽链关闸链头。相邻 jump < SCROLLBAR_DRAG_CHAIN_MS
   * 判为拖拽中 → 强制关闸;释放路径在 sampleScrollGate 的 scrollbarDragUntil 判定,与引擎无关。
   */
  function noteAxisJump() {
    const now = performance.now()
    if (now - lastScrollbarJumpTs < SCROLLBAR_DRAG_CHAIN_MS) {
      scrollbarDragUntil = now + SCROLLBAR_DRAG_CHAIN_MS
      setDeferThumbLoad(true)
    }
    lastScrollbarJumpTs = now
  }

  /**
   * onGridScroll 的闸门段:按物理滚动速度置加载闸门 + 重排释放去抖。
   * @param st 当前物理 scrollTop
   * @param now performance.now() 单调时钟
   * @param internalHop 本次 scroll 事件是否由 bucket 引擎程序化跳转自触发
   */
  function sampleScrollGate(st: number, now: number, internalHop: boolean) {
    if (internalHop) {
      // F1:程序化落点不计速度(见 INTERNAL_HOP_CHAIN_MS 注),只重置采样基准——否则下一
      // 真实手势事件会以落点前位置算位移,同样误判飞掠。
      setDeferThumbLoad(now - lastInternalHopTs < INTERNAL_HOP_CHAIN_MS)
      lastInternalHopTs = now
    } else if (now < scrollbarDragUntil) {
      // 滚动条拖拽链中:强制维持关闸,不让拖动产生的低速采样把闸门重新打开(见 noteAxisJump)。
      setDeferThumbLoad(true)
    } else {
      const v = scrollVelocity(st - lastScrollTop, now - lastScrollSampleTs)
      // F3:传入当前闸门态做滞回,防减速穿越反复翻转。双阈值随行高缩放(thumbGateThresholds,
      // 2026-07-10 真机回归):60px 极密网格维持原 3/1.5 妥协;大格同速度下入视口格数少,
      // 阈值等比抬升——滚轮式「慢速浏览」(峰值 2-4 px/ms)不再误关闸出占位墙。
      const th = thumbGateThresholds(deps.rowHeight())
      setDeferThumbLoad(shouldDeferThumbLoad(v, isThumbLoadDeferred(), th.engage, th.release))
    }
    lastScrollTop = st
    lastScrollSampleTs = now
    // B-甲:一旦滚动停顿即快速放行闸门(急停不必苦等 150ms settle 兜底)。慢滚时闸门本已
    // false,此路径为幂等 no-op;飞掠中每帧 clear+重排,只有真停顿(>64ms 无事件)才触发。
    if (gateReleaseTimer !== null) clearTimeout(gateReleaseTimer)
    gateReleaseTimer = setTimeout(() => {
      gateReleaseTimer = null
      setDeferThumbLoad(false)
    }, GATE_RELEASE_MS)
  }

  /** 卸载/失活时掐断释放去抖定时器(闸门复位由调用方按 F4 契约显式做)。 */
  function clearGateTimer() {
    if (gateReleaseTimer !== null) {
      clearTimeout(gateReleaseTimer)
      gateReleaseTimer = null
    }
  }

  return {
    beginProgrammaticScroll,
    endProgrammaticScroll,
    isProgrammaticScroll,
    noteAxisJump,
    sampleScrollGate,
    clearGateTimer,
  }
}
