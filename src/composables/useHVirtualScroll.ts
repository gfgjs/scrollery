// src/composables/useHVirtualScroll.ts
// H-Lab 横向虚拟滚动(x 轴可视窗口;docs/designs/2026-07-02-horizontal-gallery-lab.md §2-3)。
//
// 与生产 gallery 虚拟滚动的关系:同款「rAF 节流 + 取数边界框去重 + fetchId 竞态守卫」模式
// (那套已被实战验证),但**独立实现且刻意不移植坐标压缩**(SAFE_MAX 平移模式)——实验库
// 规模用不到;总宽超过实验上限时经 `overCap` 暴露给宿主横幅告警。某模式毕业转正时,
// 再做「生产滚动器轴泛化 + 平移模式移植」的统一(届时坐标数学测试参数化跑两轴)。
//
// 滚轮转译:横向画廊必须让普通竖滚轮直接驱动横向滚动(桌面横向滚动的第一可用性障碍),
// 触摸板横扫(|deltaX| > |deltaY|)保持原生。

import { ref, onMounted, onBeforeUnmount } from 'vue'
import type { HBlock } from '../types/hgallery'
import { logger } from '../utils/logger'

const LOG = '[HVirtualScroll]'

/// 离屏缓冲(逻辑 px):按视口宽的比例取,钳制在 [MIN, MAX]——窗口越宽预取越多,
/// 但极宽屏不至于一次拉数千项。
const MIN_BUFFER_PX = 600
const MAX_BUFFER_PX = 1600

/// 实验滚动上限:低于 Chromium/WebView2 ~1677 万 px 的元素尺寸钳制,留安全余量。
/// 超过即 overCap(宿主显示告警横幅;滚动条在超出段不可达——实验期已知限制)。
export const H_SCROLL_CAP_PX = 10_000_000

interface UseHVirtualScrollOptions {
  totalWidth: () => number
  fetchBlocksByX: (leftX: number, rightX: number) => Promise<HBlock[]>
  containerRef: () => HTMLElement | null
}

// 滚轮动画器:定时长线性重定标(五轮手测收敛的终态模型)。
//
// 为何自研(六轮演进,全史见 plan §2-3①):原生 scrollBy/scrollTo smooth 是 ease-in-out 曲线、
// 每次重定标都从零速起步,且无缓动控制 API——快速启动的缓入迟滞在原生路线上无解。四条体验
// 约束:不阶跃、慢滚不脉动、连滚不丢量、启动即跟手,线性段是唯一同时满足的曲线——
// 输入即满速(跟手),又不带其速度尖峰(慢滚不脉动)。
//
// 实现=帧积分而非锚点插值(六轮「极快启动顿滞」教训:锚点插值每次输入把时间锚拉回当下,帧 dt
// 塌缩为输入间隔,速度随输入率反常下降,极快连滚起手形同冻结):输入只更新「累积目标 + 截止线
// (末次输入 + durMs)」,位移在帧循环里按 `Δx = 剩余距离 × 帧dt / 剩余时间` 积分,无输入时
// 逐帧衔接即恒速线性,输入频率高于帧率时速度不塌缩。
//
// 方向安全三道构造性约束(三轮方向乱跳教训——根因实为未钳制 t 的实现 bug,当时却误判为
// 模型缺陷整体撤销,后已复活模型并修复实现):
// ① 帧 dt 钳 0:rAF 时间戳可早于输入侧 now() 时钟,负 dt 退化为原地而非反向外插;
// ② 目标仅在滚动方向前方时才累积,否则从当前位置重算——每帧写值恒在 [当前位置, 目标] 区间内,
//    方向单调由构造保证,反向输入/外源位移自动重基;
// ③ 定时长:末次输入后 durMs 内必然终止,不与滚动条拖拽等外源滚动持续对抗。
//
// 独立于 Vue 生命周期并注入 raf/时钟，便于确定性地复现输入时序。

/// 单段动画时长 ms:输入间隔 ≤ 此值时相邻段无缝衔接成连续运动(约两倍于快速
/// 滚轮的格间隔);再大则慢滚每格拖尾过长,再小则退化向阶跃。
const WHEEL_ANIM_MS = 160

/// 动画器只读写这三个字段——收窄类型以便测试用普通对象伪造容器。
export interface WheelScrollEl {
  scrollLeft: number
  scrollWidth: number
  clientWidth: number
}

interface WheelAnimatorDeps {
  el: () => WheelScrollEl | null
  durMs?: number
  raf?: (cb: FrameRequestCallback) => number
  caf?: (id: number) => void
  now?: () => number
}

export function createWheelAnimator(deps: WheelAnimatorDeps) {
  const dur = deps.durMs ?? WHEEL_ANIM_MS
  const raf = deps.raf ?? ((cb: FrameRequestCallback) => requestAnimationFrame(cb))
  const caf = deps.caf ?? ((id: number) => cancelAnimationFrame(id))
  const now = deps.now ?? (() => performance.now())

  let targetX: number | null = null // null = 无进行中的滚轮动画
  let deadline = 0 // 到达目标的截止时刻 = 末次输入 + durMs(恒速隐含其中)
  let lastFrame = 0 // 帧积分锚:上一帧时间戳;仅在动画启动时由输入时钟播种
  let rafId: number | null = null

  function step(frameNow: number) {
    const el = deps.el()
    if (!el || targetX === null) {
      rafId = null
      return
    }
    // 帧积分:Δx = 剩余距离 × 帧dt / 剩余时间。dt 钳 0 防时间戳乱序反向外插;
    // dt 吃满剩余时间即到点,快照目标并终止。
    const dt = Math.max(0, frameNow - lastFrame)
    const timeLeft = deadline - lastFrame
    lastFrame = Math.max(lastFrame, frameNow)
    if (timeLeft <= 0 || dt >= timeLeft) {
      el.scrollLeft = targetX
      rafId = null
      targetX = null
      return
    }
    el.scrollLeft += (targetX - el.scrollLeft) * (dt / timeLeft)
    rafId = raf(step)
  }

  /// 送入一次已归一的滚动增量(逻辑 px,正 = 向右):目标累积,并把截止线
  /// 重置为「现在 + durMs」——即「末次输入后 durMs 恒速到达累积目标」。
  function push(dy: number) {
    const el = deps.el()
    if (!el) return
    const cur = el.scrollLeft
    // 目标在滚动方向前方 → 累积(承接上段剩余距离);否则从当前位置重算(含反向)。
    const base = targetX !== null && (targetX - cur) * dy > 0 ? targetX : cur
    const maxLeft = Math.max(0, el.scrollWidth - el.clientWidth)
    targetX = Math.min(maxLeft, Math.max(0, base + dy))
    deadline = now() + dur
    // 六轮「极快启动顿滞」修复核心:输入不重置帧积分锚(lastFrame),只在动画
    // 启动时播种——时间积分只属于帧循环,输入只改目标与截止线。
    if (rafId === null) {
      lastFrame = now()
      rafId = raf(step)
    }
  }

  /// 取消动画并作废目标(非滚轮导航/卸载时调用)。
  function stop() {
    if (rafId !== null) {
      caf(rafId)
      rafId = null
    }
    targetX = null
  }

  return { push, stop }
}

export function useHVirtualScroll(opts: UseHVirtualScrollOptions) {
  // 深层 ref(非 shallowRef):可视块量级小(数十块 × 数项),而缩略图结果需要
  // 就地 patch 子项字段并触发响应式更新(同 MediaGrid 对 visibleRows 的用法)。
  const visibleBlocks = ref<HBlock[]>([])
  const containerWidth = ref(0)
  const containerHeight = ref(0)
  const isFetching = ref(false)
  const overCap = ref(false)

  let resizeObserver: ResizeObserver | null = null
  let wheelTarget: HTMLElement | null = null

  let currentFetchId = 0
  let lastFetchedLeft = -1
  let lastFetchedRight = -1
  let ticking = false
  let pendingUpdate = false

  // 可视窗口计算(生产 updateVisible 的 x 轴版,无坐标平移分支)。
  async function updateVisible(force = false) {
    if (force) {
      lastFetchedLeft = -1
    }
    const container = opts.containerRef()
    if (!container) return

    const total = opts.totalWidth()
    overCap.value = total > H_SCROLL_CAP_PX
    if (total <= 0) {
      visibleBlocks.value = []
      return
    }

    const viewW = containerWidth.value > 0 ? containerWidth.value : container.clientWidth
    if (viewW === 0) return

    const scrollX = container.scrollLeft
    const buffer = Math.min(MAX_BUFFER_PX, Math.max(MIN_BUFFER_PX, viewW * 0.75))
    const leftX = Math.max(0, scrollX - buffer)
    const rightX = Math.min(total, scrollX + viewW + buffer)

    // 未移出上次取数边界框 → 跳过(同生产去重守卫)。
    if (lastFetchedLeft !== -1 && leftX >= lastFetchedLeft && rightX <= lastFetchedRight) {
      return
    }

    const requestLeft = Math.max(0, scrollX - buffer * 1.2)
    const requestRight = Math.min(total, scrollX + viewW + buffer * 1.2)
    lastFetchedLeft = requestLeft
    lastFetchedRight = requestRight

    const myFetchId = ++currentFetchId
    isFetching.value = true
    try {
      const blocks = await opts.fetchBlocksByX(requestLeft, requestRight)
      // 等待期间有更新的取数发起 → 丢弃本次(竞态守卫)。
      if (myFetchId !== currentFetchId) return
      visibleBlocks.value = blocks
    } catch (err) {
      logger.error(`${LOG} fetchBlocksByX FAILED`, { error: err })
    } finally {
      if (myFetchId === currentFetchId) {
        isFetching.value = false
      }
    }
  }

  function scheduleUpdate(force = false) {
    if (force) {
      lastFetchedLeft = -1
    }
    if (isFetching.value) {
      pendingUpdate = true
      return
    }
    if (!ticking) {
      ticking = true
      requestAnimationFrame(async () => {
        await updateVisible(false)
        ticking = false
        if (pendingUpdate) {
          pendingUpdate = false
          scheduleUpdate()
        }
      })
    }
  }

  // 滚动中标志(2026-07-02 掉帧反馈修复)。
  /// 滚动进行中(160ms 空闲判定)。宿主用它在滚动期抑制 hover(pointer-events),
  /// 避免快速横扫时 hover 样式重算/悬停预览抖动——对齐生产网格 isScrolling 纪律。
  const isScrolling = ref(false)
  let scrollIdleTimer: ReturnType<typeof setTimeout> | null = null

  function markScrolling() {
    if (!isScrolling.value) isScrolling.value = true
    if (scrollIdleTimer) clearTimeout(scrollIdleTimer)
    scrollIdleTimer = setTimeout(() => {
      isScrolling.value = false
    }, 160)
  }

  function onScroll() {
    markScrolling()
    scheduleUpdate()
  }

  // 滚轮转译:竖滚 → 横滚(deltaMode 归一同生产 wheel 补偿)。
  /// 动画模型与方向安全约束见文件头部 createWheelAnimator;此处仅做事件归一与接线。
  const wheelAnim = createWheelAnimator({ el: () => opts.containerRef() })

  function onWheel(e: WheelEvent) {
    if (!opts.containerRef()) return
    // 触摸板横扫交给原生横向滚动,不拦(原生自带惯性)。
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return
    let dy = e.deltaY
    if (e.deltaMode === 1) dy *= 16
    else if (e.deltaMode === 2) dy *= containerWidth.value || 800
    if (dy === 0) return
    e.preventDefault()
    wheelAnim.push(dy)
    // 动画帧内写 scrollLeft → scroll 事件 → onScroll → 取数调度,无需重复调度。
  }

  // 键盘导航(宿主在容器 keydown 中调用)。
  /// 平移一个视口宽的 ratio 倍(翻屏用 smooth,方向键小步用 instant 以支持连按)。
  function scrollByViewport(ratio: number, smooth = true) {
    const container = opts.containerRef()
    if (!container) return
    wheelAnim.stop() // 非滚轮导航取消滚轮动画,避免双动画源竞写
    container.scrollBy({
      left: containerWidth.value * ratio,
      behavior: smooth ? 'smooth' : 'auto',
    })
  }

  function scrollToStart() {
    wheelAnim.stop()
    opts.containerRef()?.scrollTo({ left: 0 })
  }

  function scrollToEnd() {
    const el = opts.containerRef()
    if (!el) return
    wheelAnim.stop()
    el.scrollTo({ left: el.scrollWidth })
  }

  onMounted(() => {
    const el = opts.containerRef()
    if (!el) {
      logger.warn(`${LOG} onMounted: containerRef is null`)
      return
    }
    containerWidth.value = el.clientWidth
    containerHeight.value = el.clientHeight

    // 滚轮转译需 preventDefault → 非被动监听(仅挂实验容器,不影响其它视图)。
    el.addEventListener('wheel', onWheel, { passive: false })
    wheelTarget = el

    resizeObserver = new ResizeObserver((entries) => {
      const rect = entries[0].contentRect
      // 宽变化 → 重取可视窗口;高变化经 containerHeight 暴露,宿主 watch 后重算布局
      // (视口高是横向布局的输入,不是取数参数)。
      if (rect.width > 0 && Math.abs(rect.width - containerWidth.value) > 1) {
        containerWidth.value = rect.width
        scheduleUpdate(true)
      }
      if (rect.height > 0 && Math.abs(rect.height - containerHeight.value) > 1) {
        containerHeight.value = rect.height
      }
    })
    resizeObserver.observe(el)
  })

  onBeforeUnmount(() => {
    resizeObserver?.disconnect()
    wheelAnim.stop()
    if (scrollIdleTimer) clearTimeout(scrollIdleTimer)
    if (wheelTarget) {
      wheelTarget.removeEventListener('wheel', onWheel)
      wheelTarget = null
    }
  })

  return {
    visibleBlocks,
    containerWidth,
    containerHeight,
    isFetching,
    isScrolling,
    overCap,
    onScroll,
    updateVisible,
    scheduleUpdate,
    scrollByViewport,
    scrollToStart,
    scrollToEnd,
  }
}
