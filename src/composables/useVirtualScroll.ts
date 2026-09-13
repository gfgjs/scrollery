// src/composables/useVirtualScroll.ts
// 行级虚拟滚动 + 坐标平移 (§10.3 + B1)
//
// 为什么需要坐标平移：
//   Chromium/WebView2 会把单个元素高度钳制在约 1677 万 px。百万张图库约 4000 万 px 高，
//   会击穿原生滚动条（到不了底、滚动事件无限触发）。因此当逻辑布局高度超过安全上限时，
//   我们把物理滚动占位高度封顶在 SAFE_MAX，并在两套坐标系间做线性映射。
//
// 模型：
//   physicalScrollTop ∈ [0, physMax]      ← native container.scrollTop
//   logicalScrollTop  = physicalScrollTop / physMax * logMax
//   行渲染在一个"渲染层"内。逻辑 y 的行被放在层内 (row.y - renderAnchor) 处（数值小、精度安全），
//   层整体平移 contentOffset = renderAnchor + (physicalScrollTop - logicalScrollTop) 以把可视窗口
//   钉到视口。普通模式（高度 ≤ SAFE_MAX）下 δ = 0，层偏移在两次取数之间恒定，原生滚动与此前一致。
//
// 层 transform 以命令式（直接写 style）应用，避免平移模式下快速滚动每帧都触发行列表的 Vue 重渲染。

import { ref, watch, onMounted, onBeforeUnmount } from 'vue'
import type { LayoutRow } from '../types/layout'
import { DEFAULTS } from '../constants/defaults'
import { logger } from '../utils/logger'
import { canvasPrefetchCoveragePx } from '../utils/galleryPrefetchWindow'

const LOG = '[VirtualScroll]'

/// 离屏渲染缓冲边界（逻辑像素）。实际缓冲 = `rowHeight * SCROLL_BUFFER_ROWS` 钳制到
/// [MIN, MAX]，使缓冲“行数”大致恒定，而非在极小行高时膨胀（固定像素缓冲在 60px 时
/// 渲染约 16 行，240px 时仅约 4 行）。
const MIN_BUFFER_PX = 400
const MAX_BUFFER_PX = 1200

/// 物理滚动占位上限，安全地低于约 1677 万 px 的元素高度钳制阈值。
/// 平移模式（逻辑高度 > 此值）的核心修复 `overflow:hidden`（MediaGrid `.media-grid__content`）
/// 已落地——`scrollHeight==spacerHeight` 不变量维持，原「滚动错位」根因已治。约 25 万项
/// （行高 40px）以上的库进入平移模式；以下走普通模式、原生滚动零回归。
export const SAFE_MAX_DEFAULT = 10_000_000

/// 解析生效的 SAFE_MAX。**仅 dev** 期可经 `localStorage['scrollery.debug.safeMax']` 覆盖（T6 §3.6.1
/// 验证用）：设一个**小于测试库总逻辑高度**的值（如 `9_000_000`）即可强制进入平移模式、迫使
/// `ratio≈2–4×`，免重编译验证滚动条稳定性 / 可达真底 / 无跳底错位。读取一次（模块加载时），
/// 故零每帧开销；改值后刷新页面生效，清除即恢复默认。
// export 仅为单测(R2-5):模块加载时已快照进 SAFE_MAX,导出不改变运行时行为。
export function resolveSafeMax(): number {
  try {
    if (import.meta.env.DEV) {
      const raw = localStorage.getItem('scrollery.debug.safeMax')
      const o = raw == null ? NaN : Number(raw)
      if (Number.isFinite(o) && o > 0) {
        logger.info(`${LOG} SAFE_MAX overridden → ${o} (debug, T6 §3.6.1)`)
        return o
      }
    }
  } catch {
    /* localStorage 不可用（隐私模式等）→ 回退默认 */
  }
  return SAFE_MAX_DEFAULT
}

const SAFE_MAX = resolveSafeMax()

interface UseVirtualScrollOptions {
  totalHeight: () => number
  totalRows: () => number
  fetchRowsByY: (topY: number, bottomY: number) => Promise<LayoutRow[]>
  containerRef: () => HTMLElement | null
  /// 渲染层元素，其 transform 把可视窗口钉到视口。
  layerRef: () => HTMLElement | null
  /// 当前网格目标行高（px）—— 驱动自适应滚动缓冲。
  rowHeight: () => number
  /// 双引擎互斥开关(T16 方案B):false = 本引擎休眠——不取数、不写 transform、不挂
  /// wheel 补偿(强制退出平移态)。缺省恒 true,单引擎用法行为零变化。
  enabled?: () => boolean
  /// Canvas 渲染模式开关(可选,T16 §4.3 S3):true 时取行窗至少覆盖位图预取几何范围
  /// (前 1.25 屏 + 行级余量,两侧同宽兜方向反转)。此时宿主渲染的是 canvas,行集只作数据源,
  /// 不产生 DOM 行;DOM 分支运行时本开关恒 false(两者互斥),故既有缓冲语义零变化。
  canvasMode?: () => boolean
}

export function useVirtualScroll(opts: UseVirtualScrollOptions) {
  const containerHeight = ref(0)
  const visibleRows = ref<LayoutRow[]>([])
  const startIndex = ref(0)
  const paddingTop = ref(0) // 保留以兼容旧 API(行本身已用绝对定位)
  const paddingBottom = ref(0)
  const isFetching = ref(false)

  // ── 坐标平移状态 ───────────────────────────────────────
  /// 滚动占位的物理高度(封顶 SAFE_MAX)。响应式——绑定占位 div 的高度；仅在布局变化时改变。
  const spacerHeight = ref(0)
  /// 当前窗口内行偏移的度量起点(逻辑 y)。响应式——绑定各行的 transform；仅在取数时改变。
  const renderAnchor = ref(0)
  /// 当前逻辑滚动位置(消费方用它代替 scrollTop)。仅 JS 内读取(非热渲染绑定)，故逐帧更新不触发重渲染。
  const logicalScrollTop = ref(0)
  /// 布局高度超过 SAFE_MAX、平移生效时为 true。
  const isTranslated = ref(false)

  /// 上次施加到层的 transform —— 变更守卫，跳过冗余的 style 写入。
  let lastAppliedOffset = Number.NaN

  let rafId: number | null = null
  let resizeObserver: ResizeObserver | null = null

  let currentFetchId = 0
  let lastFetchedTop = -1
  let lastFetchedBottom = -1
  let ticking = false
  let pendingUpdate = false

  const isEnabled = () => opts.enabled?.() ?? true

  // ── 几何辅助 ───────────────────────────────────────────────────

  /// 计算当前容器 + 布局的物理→逻辑映射。
  function geometry() {
    const container = opts.containerRef()
    const viewH = containerHeight.value > 0 ? containerHeight.value : (container?.clientHeight ?? 0)
    const logicalTotal = opts.totalHeight()
    const physicalTotal = Math.min(logicalTotal, SAFE_MAX)
    const physMax = Math.max(0, physicalTotal - viewH)
    const logMax = Math.max(0, logicalTotal - viewH)
    return { viewH, logicalTotal, physicalTotal, physMax, logMax }
  }

  function physicalToLogical(physicalTop: number): number {
    const { physMax, logMax } = geometry()
    if (physMax <= 0) return 0
    return (physicalTop / physMax) * logMax
  }

  /// 将逻辑 y 转换为能把它带到视口顶部的物理 scrollTop。
  function logicalToPhysical(logicalY: number): number {
    const { physMax, logMax } = geometry()
    if (logMax <= 0) return 0
    return (Math.max(0, logicalY) / logMax) * physMax
  }

  /// 命令式地把渲染层钉到当前滚动位置对应的视口；每帧运行，开销小且跳过冗余写入。
  function syncTransform() {
    const container = opts.containerRef()
    if (!container) return
    const physicalTop = container.scrollTop
    const logical = physicalToLogical(physicalTop)
    logicalScrollTop.value = logical
    const offset = renderAnchor.value + (physicalTop - logical)
    if (offset !== lastAppliedOffset) {
      const layer = opts.layerRef()
      if (layer) layer.style.transform = `translate3d(0, ${offset}px, 0)`
      lastAppliedOffset = offset
    }
  }

  // ── 滚动处理程序（由宿主 @scroll 调用） ────────────────────────────

  function onScroll() {
    if (!isEnabled()) return
    // 每帧把渲染层钉在视口上（平移模式下原生滚动的逻辑速率不对，必须每帧修正）。
    syncTransform()
    scheduleUpdate()
  }

  // ── 滚轮惯性补偿(T6 §3.6.2，仅平移模式) ──────────────────────────────────────
  /// 平移模式下物理滚动条被压缩（spacerHeight=SAFE_MAX ≪ 逻辑高度），故 1 物理 px = `ratio`
  /// 逻辑 px（`ratio=logMax/physMax`，可达 2–4×）。原生 wheel 按物理 px 滚 → 内容以 `ratio` 倍速
  /// 漂移（触摸板惯性尤甚，一甩滑过大量行）。补偿：拦 wheel，把物理步进缩成 `dy/ratio`，使「逻辑
  /// 滚动速度」与无压缩时一致。**仅平移模式介入**——普通模式不挂此（非被动）监听，原生滚动零回归。
  let wheelTarget: HTMLElement | null = null

  function onWheel(e: WheelEvent) {
    const container = opts.containerRef()
    if (!container) return
    const { physMax, logMax } = geometry()
    if (physMax <= 0 || logMax <= 0) return
    const ratio = logMax / physMax
    if (ratio <= 1) return // 未压缩 → 原生即正确，不介入
    // deltaMode：0=像素（WebView2/常见）、1=行、2=页。归一到像素。
    let dy = e.deltaY
    if (e.deltaMode === 1) dy *= 16
    else if (e.deltaMode === 2) dy *= containerHeight.value || 800
    e.preventDefault()
    // 物理步进 = 期望逻辑步进 / ratio，消除 ratio 倍漂移；随后立即重钉层（免 1 帧滞后）。
    container.scrollTop += dy / ratio
    syncTransform()
    scheduleUpdate()
  }

  /// 进入平移模式时挂载 wheel 补偿；退出时卸载——保证普通模式无任何非被动 wheel 监听（零回归）。
  function setWheelCompensation(enabled: boolean) {
    const el = opts.containerRef()
    if (enabled) {
      if (el && wheelTarget !== el) {
        if (wheelTarget) wheelTarget.removeEventListener('wheel', onWheel)
        el.addEventListener('wheel', onWheel, { passive: false })
        wheelTarget = el
      }
    } else if (wheelTarget) {
      wheelTarget.removeEventListener('wheel', onWheel)
      wheelTarget = null
    }
  }

  // 随 isTranslated 翻转开关 wheel 补偿（updateVisible 中设置 isTranslated）。
  watch(isTranslated, (on) => setWheelCompensation(on))

  // 引擎互斥(T16 方案B):被停用时强制退出平移态——否则 bucket 引擎接管后,残留的
  // wheel 补偿监听仍会按 ratio 篡改滚轮增量、直接破坏原生滚动手感;重新启用时强制
  // 重取当前视口(spacer 高度/可见行在休眠期间已失真)。
  if (opts.enabled) {
    watch(opts.enabled, (on) => {
      if (on) {
        scheduleUpdate(true)
      } else {
        isTranslated.value = false
      }
    })
  }

  // Canvas 模式翻转(DOM↔Canvas 一键切换)按新窗即时重取:否则残留的窄取行窗会把画面
  // 卡在「位图够远、行数据不够」的错配态,直到下一次滚动才自愈(§4.3 S3)。
  if (opts.canvasMode) {
    watch(opts.canvasMode, () => scheduleUpdate(true))
  }

  function scheduleUpdate(force = false) {
    if (force) {
      lastFetchedTop = -1
    }

    // 如果获取操作已经在进行中，则标记我们需要在它完成后进行另一次更新
    if (isFetching.value) {
      pendingUpdate = true
      return
    }

    if (!ticking) {
      ticking = true
      // rAF id 存账(2026-07-06 审查 F2):原先从未写入 rafId,卸载时 cancelAnimationFrame 恒 no-op。
      rafId = requestAnimationFrame(async () => {
        // 等待获取，这样我们就不会开始重叠的请求
        await updateVisible(false)
        ticking = false

        // 如果用户在获取时保持滚动，请再次运行它以赶上
        if (pendingUpdate) {
          pendingUpdate = false
          scheduleUpdate()
        }
      })
    }
  }

  // ── 计算可见窗口 ─────────────────────────────────────────────

  async function updateVisible(force: boolean = false) {
    if (!isEnabled()) return
    if (force) {
      lastFetchedTop = -1
    }
    rafId = null

    const container = opts.containerRef()
    if (!container) {
      logger.warn(`${LOG} updateVisible: containerRef is null, skipping`)
      return
    }

    const { viewH, logicalTotal, physicalTotal, logMax } = geometry()
    const totalR = opts.totalRows()

    // 让物理占位高度 + 平移标志与布局保持同步。
    spacerHeight.value = physicalTotal
    isTranslated.value = logicalTotal > SAFE_MAX

    if (logicalTotal === 0 || totalR === 0) {
      visibleRows.value = []
      paddingTop.value = 0
      paddingBottom.value = 0
      renderAnchor.value = 0
      logicalScrollTop.value = 0
      lastAppliedOffset = Number.NaN
      syncTransform()
      return
    }

    if (viewH === 0) {
      logger.warn(`${LOG} updateVisible: containerHeight is 0, skipping`)
      return
    }

    // 取数完全在逻辑坐标系中进行。
    const physicalTop = container.scrollTop
    const scrollY = physicalToLogical(physicalTop)
    logicalScrollTop.value = scrollY

    // 自适应缓冲：保持缓冲行数大致恒定，避免极小行高时过度渲染（见上方 MIN/MAX 说明）。
    const rh = Math.max(40, opts.rowHeight())
    // 极密(compact,<100px)收窗(方案 §7.2):缓冲行数 8→4 且下限 400→240px,单屏外缓冲格数减半——
    // 60px 下 480→240px/侧,直击「快滚 churn」的挂载/卸载量。快滚瞬白由 thumbhash 占位兜底(§8)。
    // 仅方案 A 生效;bucket 引擎(默认)走固定段 margin,不经此路径。
    const compactBuf = rh < 100
    const bufferRows = compactBuf ? 4 : DEFAULTS.SCROLL_BUFFER_ROWS
    const bufferFloor = compactBuf ? 240 : MIN_BUFFER_PX
    const bufferH = Math.min(MAX_BUFFER_PX, Math.max(bufferFloor, rh * bufferRows))
    // 取行窗:canvas 模式下至少覆盖位图预取的几何范围(两侧同宽,兜方向反转)。该行集由
    // canvas 消费、DOM 分支不渲染,故不额外挂节点(DOM 运行时 canvasMode 恒 false,走 bufferH)。
    const fetchBufferH = opts.canvasMode?.()
      ? Math.max(bufferH, canvasPrefetchCoveragePx(viewH, rh).aheadPx)
      : bufferH
    const topY = Math.max(0, scrollY - fetchBufferH)
    const bottomY = Math.min(logMax + viewH, scrollY + viewH + fetchBufferH)

    // 如果可见范围实际上没有移出我们上次获取的边界框，则跳过
    if (
      lastFetchedTop !== -1 &&
      topY >= lastFetchedTop &&
      bottomY <= lastFetchedBottom &&
      totalR > 0
    ) {
      return
    }

    // 我们需要一个新的超集。获取一个稍大的逻辑框。
    const requestTop = Math.max(0, scrollY - fetchBufferH * 1.2)
    const requestBottom = scrollY + viewH + fetchBufferH * 1.2

    lastFetchedTop = requestTop
    lastFetchedBottom = requestBottom

    const myFetchId = ++currentFetchId
    isFetching.value = true

    try {
      const rows = await opts.fetchRowsByY(requestTop, requestBottom)

      // 如果在我们等待时开始了更新的获取，则丢弃这个
      if (myFetchId !== currentFetchId) return

      visibleRows.value = rows

      // 把行偏移锚定到窗口顶部，使逐行 transform 保持很小（在 4000 万 px 逻辑尺度下
      // 仍精度安全），随后重新钉住渲染层。
      renderAnchor.value = Math.floor(requestTop)
      syncTransform()

      if (rows.length > 0) {
        const firstRow = rows[0]
        const lastRow = rows[rows.length - 1]
        const firstY = typeof firstRow.y === 'number' ? firstRow.y : 0
        const lastY = typeof lastRow.y === 'number' ? lastRow.y : 0
        const lastH = typeof lastRow.height === 'number' ? lastRow.height : 0
        paddingTop.value = Math.max(0, firstY)
        paddingBottom.value = Math.max(0, logicalTotal - (lastY + lastH))
      } else {
        paddingTop.value = requestTop
        paddingBottom.value = Math.max(0, logicalTotal - paddingTop.value)
        logger.warn(`${LOG}   0 rows returned`)
      }
    } catch (err) {
      logger.error(`${LOG} fetchRowsByY FAILED`, { error: err })
      // 失败不毒化跳过框(2026-07-06 审查 F1):原先失败前已写入 lastFetchedTop/Bottom 且
      // catch 不回滚 → 同窗滚动永不重试,空白区持续到滚出边界框。方案 A 是官方回退引擎,
      // 不应带病上路。仅当前批失败才回滚(迟到的旧批失败不得作废新批的框)。
      if (myFetchId === currentFetchId) {
        lastFetchedTop = -1
      }
    } finally {
      if (myFetchId === currentFetchId) {
        isFetching.value = false
        // 消费在途期间积压的更新(2026-07-06 审查 P1-12):pendingUpdate 原先只在 scheduleUpdate
        // 自己的 rAF 回调尾部消费;宿主直调 updateVisible(布局 watcher/compute 后)在途时,
        // onScroll → scheduleUpdate 置起的标志无人消费,视口停在旧位置的行窗口直到下一次滚动
        // 事件才自愈。统一在收尾消费,直调与调度路径共用。注意不能经 scheduleUpdate 重派——
        // 调度路径走到这里时 ticking 仍为 true,会被其守卫挡掉而白白清掉标志。
        if (pendingUpdate) {
          pendingUpdate = false
          rafId = requestAnimationFrame(() => {
            void updateVisible(false)
          })
        }
      }
    }
  }

  // ── 生命周期 ──────────────────────────────────────────────────────────

  onMounted(() => {
    const el = opts.containerRef()
    if (!el) {
      logger.warn(`${LOG} onMounted: containerRef is null`)
      return
    }

    containerHeight.value = el.clientHeight

    resizeObserver = new ResizeObserver((entries) => {
      const h = entries[0].contentRect.height

      if (h > 0 && Math.abs(h - containerHeight.value) > 1) {
        containerHeight.value = h
        // 容器已调整大小 — 重新获取新视口的可见行
        scheduleUpdate(true)
      }
    })
    resizeObserver.observe(el)
  })

  onBeforeUnmount(() => {
    resizeObserver?.disconnect()
    if (rafId !== null) cancelAnimationFrame(rafId)
    setWheelCompensation(false) // 卸载 wheel 补偿监听，防泄漏
  })

  function scrollToTop() {
    opts.containerRef()?.scrollTo({ top: 0 })
  }

  function scrollToBottom() {
    const el = opts.containerRef()
    if (el) el.scrollTo({ top: el.scrollHeight })
  }

  return {
    visibleRows,
    paddingTop,
    paddingBottom,
    startIndex,
    isFetching,
    containerHeight,
    // 坐标平移接口（在宿主模板中绑定）：
    spacerHeight,
    renderAnchor,
    logicalScrollTop,
    isTranslated,
    logicalToPhysical,
    onScroll,
    updateVisible,
    scrollToTop,
    scrollToBottom,
  }
}
