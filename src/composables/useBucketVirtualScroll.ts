// src/composables/useBucketVirtualScroll.ts
// T16 方案 B(B1.5):bucket 分段虚拟滚动——等高算术分段 + 有界最新优先取数管线。
//
// 机制:废弃「单 spacer + 行级坐标压缩」——容器总高 = 真实逻辑总高,每段(bucket)一个
// 绝对定位 div(top=seg.start、height=段真实高),段内行以 (row.y - seg.start) 定位。
// 滚动/惯性/滚动条全走浏览器原生,**零坐标平移、零每帧补偿**(方案 A 平移模式不顺滑的
// 根因即「原生滚动 + JS 反向补偿」的双步差帧,本引擎从结构上消除它,见 T16 评估文档 §2)。
//
// B1.5 重构(2026-07-04,真机四根因修复;原 B1 语义边界 + IntersectionObserver 方案已废):
// - **等高算术分段**:段边界 = 0, S, 2S, …(与日期/目录语义无关)。get_bucket_rows 的
//   半开区间归属保证任意边界下每行恰属一段 → 三种分组(date/folder/none)统一覆盖,
//   段大小恒定 → 「超大单桶」(Immich #28861)从构造上消失。
// - **可见段 = 纯算术**(scrollTop/S 两次除法)→ 不再需要 IntersectionObserver,也不再
//   渲染全量占位 div(原方案数百段 div × 内联函数 ref × 每滚动帧重渲染 = 每帧数千次
//   observe churn,真机根因 C);只渲染愿望窗口内的 2-3 个段。
// - **愿望清单 + 最新视口优先取数**:常态仍单飞;若唯一在途段已因远跳离开愿望集,
//   允许 1 个最新目标段旁路(总在途硬上限 2),不让陈旧 IPC 阻塞当前视口。出队按「距
//   视口中心最近」重选,应答落地前复核「该段仍被需要且仍是同一对象」——飞掠应答
//   仍会丢弃,也不会形成无界并发或幽灵挂载。
//
// B3 段级坐标映射(2026-07-04):总高 > 物理 spacer 上限(16M,WebView2 2^24 钳制留余量)
// 时进入「映射态」——spacer 封顶,段以 (seg.start − anchorDelta) 物理定位。滚动语义按
// **输入源分类**(B3.1,2026-07-04 真机回报修复,见 onScroll/onWheel):
//  - **滚轮/触摸/滚动键**(有 1:1 印记):局部 1:1——1 物理 px = 1 逻辑 px,零补偿零重锚;
//    物理钉边后原生不再产生 scroll 事件,由 onWheel 推锚差续滚到**真正的逻辑边缘**;
//  - **滚动条拖动/轨道点击/Home/End**(无印记的滚动链,或单事件巨跳):**逐事件**全局
//    线性重锚——拇指位置 ≈ 库内比例(滚动条的用户心智模型),拖到边 = 逻辑边,构造上
//    无钉住(B3 初版只认单事件巨跳,慢拖被误判 1:1 → 拖到底逻辑远未到底);
//  - **压缩债**(局部 1:1 令拇指渐失真):仅在滚动**停稳**时原子偿还——同帧改
//    delta+scrollTop,内容零位移、仅拇指悄然归位。手势进行中绝不写 scrollTop(B3 初版
//    「钉边立即偿债」与拖拽/惯性互搏,真机表现为到边后一跳一跳还能继续滚,已废)。
// 与方案 A 行级每帧补偿的本质区别:1:1 路径零干预,重锚只发生在拖动/远跳/停稳这些
// 低频或本就非连续的事件上。
//
// 与方案 A 的关系:P22 收敛后本引擎是画廊唯一虚拟滚动实现(方案 A 线性平移引擎及其
// enabled 开关已删)——其 SAFE_MAX 平移态由本引擎的 B3 段级映射态覆盖,且本引擎额外覆盖
// 滚轮/键盘/触摸的输入源分类。

import {
  shallowRef,
  reactive,
  ref,
  computed,
  watch,
  nextTick,
  onMounted,
  onBeforeUnmount,
} from 'vue'
import type { LayoutRow } from '../types/layout'
import { logger } from '../utils/logger'
import { canvasPrefetchCoveragePx } from '../utils/galleryPrefetchWindow'

const LOG = '[BucketScroll]'

/// 物理 spacer 上限(px):低于 WebView2 单元素高度钳制(2^24 = 16,777,216)的保守值。
/// 总高 ≤ 此值 → 纯原生(零映射,B1.5 形态);总高 > 此值 → B3 映射态(spacer 封顶,段级重锚)。
export const BUCKET_NATIVE_MAX = 16_000_000

/// dev-only:localStorage['scrollery.debug.bucketSpacer'] 覆盖 spacer 上限(镜像方案 A 的
/// debug.safeMax 先例)——调小(如 2_000_000)即可用中小库在真机触发 B3 映射态验收,
/// 免造百万项库。模块加载时读一次(零每帧开销),改值刷新生效、清除恢复默认。
export function resolveBucketSpacerCap(): number {
  try {
    if (import.meta.env.DEV) {
      const raw = localStorage.getItem('scrollery.debug.bucketSpacer')
      const o = raw == null ? NaN : Number(raw)
      if (Number.isFinite(o) && o > 0) {
        logger.info(`${LOG} spacer cap overridden → ${o} (debug, B3)`)
        return o
      }
    }
  } catch {
    /* localStorage 不可用(隐私模式等)→ 默认 */
  }
  return BUCKET_NATIVE_MAX
}

const SPACER_CAP = resolveBucketSpacerCap()

/// 滚动停稳判定(ms):停稳后偿还映射态压缩债(repayDebt)。
const SCROLL_SETTLE_MS = 200

/// 手势链间隔(ms):相邻 scroll 事件间隔小于此值视为**同一手势**,沿用手势起点的输入源
/// 分类——触摸板惯性/平滑滚动动画的后续事件不会因印记过期被误判为滚动条拖动。
export const SCROLL_CHAIN_MS = 100

/// 1:1 输入印记时效(ms):wheel/touchmove/滚动键之后,此窗口内**新起**的滚动手势按局部
/// 1:1 分类;窗口外新起的滚动链只能来自滚动条拖动/轨道点击 → 按全局比例逐事件重锚。
export const ONE_TO_ONE_STICKY_MS = 250

/// 1:1 印记适格的滚动键。Home/End **有意不含**——其语义是文档边界,走巨跳兜底按全局
/// 比例重锚,恰好精确落到逻辑边界。
const ONE_TO_ONE_KEYS = new Set(['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', ' '])

/// 段高上限/默认(px):既是懒加载取数单元(一次 IPC),也是挂载粒度。**现为上限**——实际段高
/// 由 clampSegmentPx 按行高自适应(见下)。默认行高 200px × ROWS_PER_SEGMENT(20) = 4000px,
/// 与历史固定值一致(≥200px 行高维持 4000px,零回归)。注意这与「元素高度上限」是两个量级的
/// 问题:>16.7M 总高所需的粗粒度段级映射是 B3 在本层之上的正交一层。
export const SEGMENT_PX = 4_000

/// 每段目标行数:× 行高 = 段高。200px × 20 = 4000px = SEGMENT_PX(默认行为锚点)。
export const ROWS_PER_SEGMENT = 20

/// 自适应段高下限(px):防极小行高下段过碎(段 div / IPC 过多);取 ≈ PRELOAD_MARGIN_PX 量级。
const MIN_SEGMENT_PX = 1_000

/**
 * 自适应段高(纯函数,单测锁定):clamp(行高 × ROWS_PER_SEGMENT, MIN_SEGMENT_PX, SEGMENT_PX)。
 *
 * 固定 4000px 段在小行高下每段项数 ∝ 段高/行高² 爆炸(60px 满屏 ~1860 项/段),跨段的挂载
 * (DOM)/反序列化落地成为单帧 ~100ms 卡顿——探针实测 60px 快滚 dropped 5-11/burst、120px 仅
 * 0-1(症状①的主因)。按行高线性缩小段高把每段项数拉回可控量级(60px → 1200px,~1/3 项),
 * 把「大爆发」切成「更频繁但更小、能塞进一帧」的块。≥200px 行高封顶 4000px = 历史值,零回归。
 */
export function clampSegmentPx(rowHeight: number): number {
  const raw = Math.round(Math.max(1, rowHeight) * ROWS_PER_SEGMENT)
  return Math.max(MIN_SEGMENT_PX, Math.min(SEGMENT_PX, raw))
}

/// 预取边距基线(px):愿望窗口 = 视口 ± 此值(静止/慢滚时的边距)。须大于最大行高(跨界行
/// 归「行首 y 所在段」,由边距保证其所属段在该行可见前已被挂载),其余部分是纯预取余量。
/// 甩滚更快时实际生效边距由 [`adaptiveMarginPx`] 按速度放大——canvas 模式对未就绪段没有
/// 占位兜底(mountedRows 只吐 ready 段),取数跟不上视口会在该 y 段出现内容空洞（画廊滚动
/// 卡顿分析·canvas 段空洞）,故甩滚快时提前把更远的段纳入愿望集,抢在视口抵达前完成取数。
export const PRELOAD_MARGIN_PX = 1_000

/// 速度→预取边距放大系数:每 1px/ms 的平滑滚动速度追加这么多像素边距。
const VELOCITY_MARGIN_FACTOR = 300
/// 预取边距放大上限(px):防极端速度(远跳/惯性峰值)把愿望窗口撑得过大,徒增段数与
/// IPC 排队深度——常态并发仍受 pumpFetch 单飞节流,边距只决定「多早把段纳入愿望集」。
const MAX_PRELOAD_MARGIN_PX = 6_000

/// 渲染段 = 几何 + 懒加载状态。仅愿望窗口内的段存在(离窗即整体丢弃,无占位)。
export interface RenderSegment {
  /// 段序号(start = index × SEGMENT_PX)。模板 key。
  index: number
  start: number
  /// 段结束逻辑 y(不含)= min((index+1)×SEGMENT_PX, totalHeight)。
  end: number
  rows: LayoutRow[] | null
  state: 'idle' | 'loading' | 'ready' | 'error'
}

/**
 * 愿望窗口的段序号闭区间(纯函数,单测锁定):视口 ± margin 所触及的段。
 * 返回 null 表示无内容或视口不可用。
 */
export function desiredSegmentRange(
  scrollTop: number,
  viewportHeight: number,
  totalHeight: number,
  segmentPx: number = SEGMENT_PX,
  marginPx: number = PRELOAD_MARGIN_PX,
): [number, number] | null {
  if (!(totalHeight > 0) || !(viewportHeight > 0)) return null
  const lastIndex = Math.max(0, Math.ceil(totalHeight / segmentPx) - 1)
  const top = Math.max(0, scrollTop - marginPx)
  const bottom = Math.min(totalHeight, scrollTop + viewportHeight + marginPx)
  const first = Math.min(lastIndex, Math.floor(top / segmentPx))
  // bottom 恰落段边界时该段不需要(半开区间)→ 取 bottom-1 所在段。
  const last = Math.min(lastIndex, Math.floor(Math.max(0, bottom - 1) / segmentPx))
  return [first, Math.max(first, last)]
}

interface UseBucketVirtualScrollOptions {
  totalHeight: () => number
  /// 布局版本:变化即换代重建段表,在途应答按代/按对象丢弃。
  layoutVersion: () => number
  fetchBucketRows: (startY: number, endY: number) => Promise<LayoutRow[]>
  containerRef: () => HTMLElement | null
  /// 当前行高(px):驱动自适应段高(clampSegmentPx)。行高变→relayout→layoutVersion 变→rebuild
  /// 重算段高,故段高与布局版本同源、一代内恒定。
  rowHeight: () => number
  /// Canvas 渲染模式开关(可选,T16 §4.3 S3):true 时愿望窗口除速度边距外,至少覆盖位图
  /// 预取的几何范围(前 1.25 屏 + 行级余量,两侧同宽以兜住方向反转)。缺省/恒 false = 既有
  /// 语义,零变化——DOM 路径不得因本契约扩大挂载量。
  canvasMode?: () => boolean
}

export function useBucketVirtualScroll(opts: UseBucketVirtualScrollOptions) {
  /// 当前愿望窗口内的段(按 index 升序)。数组本身 shallowRef(仅成员变化时整体替换);
  /// 段对象为 reactive——rows/state 变更及宿主对行 item 的就地 patch(缩略图/收藏/评分
  /// 回写)直接触发渲染,面积有界(1-3 段,与方案 A 可见行同量级)。
  const segments = shallowRef<RenderSegment[]>([])
  /// 当前逻辑滚动位。bucket 模式零映射 → 恒等于容器 scrollTop;供 scrubber 高亮与
  /// 画廊→侧栏分隔符联动(与方案 A 的 logicalScrollTop 对偶)。
  const logicalScrollTop = ref(0)

  /// 愿望集(index → 段对象,与 segments 数组共享同一批 reactive 对象)。
  const desired = new Map<number, RenderSegment>()
  /// 换代重建时把旧段行作为暂存首帧种子；新 IPC 落地前继续显示旧行，避免整段闪成空白。
  let rebuildSeed: Map<number, RenderSegment> | null = null
  /// 换代计数:布局版本变化/开关翻转即 +1;在途应答须同时满足「同代 + 段对象仍在愿望集
  /// 且是同一对象」才落地——飞掠丢弃与幽灵挂载的双保险。
  let generation = 0
  /// 常态单飞；仅当所有在途请求都已离开当前愿望集时，允许最新视口额外旁路一个。
  /// 由 Set 同时承担身份复核与总在途硬上限，避免滚动条横扫演变为 IPC 风暴。
  const activeFetches = new Set<RenderSegment>()
  /// 愿望窗口快速路径 key(代数+区间):滚动帧内区间未变则整个 sync 为 no-op。
  let lastRangeKey = ''
  /// 当前自适应段高:在 rebuild(行高/版本变)时快照,保证「一代内所有段边界用同一段高」——
  /// makeSegment(index*segPx)与 desiredSegmentRange(…, segPx)读同一值,不会因中途行高抖动串位。
  let segPx = clampSegmentPx(opts.rowHeight())
  let resizeObserver: ResizeObserver | null = null

  // ── B3 段级坐标映射状态 ────────────────────────────────────────────────────
  /// 「逻辑 − 物理」的当前窗口锚差(≥0)。非映射态恒 0;映射态下段物理位 =
  /// seg.start − anchorDelta(模板绑定,重锚这一低频事件才触发段 div 重定位)。
  const anchorDelta = ref(0)
  /// 物理 spacer 高(模板绑定):min(totalHeight, SPACER_CAP)。
  const spacerHeight = computed(() => Math.min(opts.totalHeight(), SPACER_CAP))
  /// 上次 scrollTop(单事件位移 = 跳变检测输入)。
  let lastP = 0
  /// 内部 scrollTop 写(偿债/远跳落点)标志:下一个 scroll 事件跳过处理(状态已就绪)。
  let internalScroll = false
  let settleTimer: ReturnType<typeof setTimeout> | null = null
  // ── B3.1 输入源分类状态(仅映射态消费)──────────────────────────────────────
  /// 上一 scroll 事件时刻(手势链判定);-Infinity = 链已断(程序化落点/引擎重开)。
  let lastScrollTs = -Infinity
  /// 1:1 印记过期时刻(wheel/touchmove/滚动键/程序化局部滚动盖印)。
  let oneToOneUntil = -Infinity
  /// 当前手势是否局部 1:1(手势起点定类,链内沿用)。
  let gestureOneToOne = false

  // ── 甩滚速度追踪(预取边距自适应)──────────────────────────────────────────
  /// 物理滚动速度 EMA(px/ms)。与上面 B3.1 的 lastScrollTs 分开追踪——常见库规模恒不进
  /// 映射分支,而 lastScrollTs 只在 mapped 分支更新;此处不区分映射态,onScroll 每次都算。
  let scrollVelocityPxMs = 0
  /// 上次速度采样时刻(ms);0 = 尚未采样(下一次 onScroll 只播种时间戳,不计速度,避免
  /// 首次调用因缺基准把巨跳误判成瞬时高速)。
  let lastVelocityTs = 0

  /// 速度→预取边距:基线 [`PRELOAD_MARGIN_PX`] 按当前平滑速度线性放大,封顶
  /// [`MAX_PRELOAD_MARGIN_PX`]。
  function adaptiveMarginPx(): number {
    return Math.min(
      MAX_PRELOAD_MARGIN_PX,
      PRELOAD_MARGIN_PX + scrollVelocityPxMs * VELOCITY_MARGIN_FACTOR,
    )
  }

  /// 实际生效的取数边距:速度边距是下限,canvas 模式再抬到「位图预取几何」之上。
  /// 两侧取同一跨度(前向值):反向滚动时原后方立刻成为新的前方,对称覆盖保证反转后仍有
  /// 完整的 1.25 屏。余量(一行行高)已含在纯函数内。
  function effectiveMarginPx(viewH: number): number {
    const speedMargin = adaptiveMarginPx()
    if (!(opts.canvasMode?.() ?? false) || !(viewH > 0)) return speedMargin
    return Math.max(speedMargin, canvasPrefetchCoveragePx(viewH, opts.rowHeight()).aheadPx)
  }

  function geometry() {
    const el = opts.containerRef()
    const viewH = el?.clientHeight ?? 0
    const total = opts.totalHeight()
    return {
      viewH,
      total,
      physMax: Math.max(0, Math.min(total, SPACER_CAP) - viewH),
      logMax: Math.max(0, total - viewH),
      mapped: total > SPACER_CAP,
    }
  }

  /// 全局线性:物理 p → 逻辑 L(滚动条拇指比例语义;仅大位移重锚/偿债时使用)。
  function globalLogical(p: number): number {
    const { physMax, logMax } = geometry()
    if (physMax <= 0) return 0
    return (Math.min(Math.max(p, 0), physMax) / physMax) * logMax
  }

  /// 全局线性:逻辑 L → 物理 p。
  function globalPhysical(l: number): number {
    const { physMax, logMax } = geometry()
    if (logMax <= 0) return 0
    return (Math.min(Math.max(l, 0), logMax) / logMax) * physMax
  }

  function makeSegment(index: number): RenderSegment {
    const total = opts.totalHeight()
    const previous = rebuildSeed?.get(index)
    return reactive({
      index,
      start: index * segPx,
      end: Math.min((index + 1) * segPx, total),
      // rows 可以是上一代布局；state 仍从 idle 开始，让新代立即发起取数并在完成后原子替换。
      rows: previous?.rows ?? null,
      state: 'idle',
    }) as RenderSegment
  }

  function publish() {
    segments.value = Array.from(desired.values()).sort((a, b) => a.index - b.index)
  }

  /// 按当前逻辑位同步愿望集:窗外段整体丢弃(含 loading 中的——其在途应答将因
  /// 「对象已不在愿望集」被丢弃),窗内缺失段以 idle 补齐,然后踢一脚取数泵。
  /// B3:逻辑位 = scrollTop + anchorDelta;远跳时 scrollTop 尚未落位,经 logicalOverride
  /// 显式传入目标逻辑位。
  function syncDesired(force = false, logicalOverride?: number) {
    const el = opts.containerRef()
    if (!el) return
    const logicalTop = logicalOverride ?? el.scrollTop + anchorDelta.value
    const range = desiredSegmentRange(
      logicalTop,
      el.clientHeight,
      opts.totalHeight(),
      segPx,
      effectiveMarginPx(el.clientHeight),
    )
    if (!range) {
      if (desired.size > 0) {
        desired.clear()
        publish()
      }
      return
    }
    const [first, last] = range
    const key = `${generation}:${first}:${last}`
    if (!force && key === lastRangeKey) return
    lastRangeKey = key

    let changed = false
    for (const i of Array.from(desired.keys())) {
      if (i < first || i > last) {
        desired.delete(i)
        changed = true
      }
    }
    for (let i = first; i <= last; i++) {
      if (!desired.has(i)) {
        desired.set(i, makeSegment(i))
        changed = true
      }
    }
    if (changed) publish()
    void pumpFetch()
    flushSettled()
  }

  /// 挑下一个要取的段:idle 中距视口中心(逻辑坐标)最近者——远跳后终点段永远最先取。
  function pickNextIdle(): RenderSegment | null {
    const el = opts.containerRef()
    const center = el ? el.scrollTop + anchorDelta.value + el.clientHeight / 2 : 0
    let best: RenderSegment | null = null
    let bestDist = Infinity
    for (const seg of desired.values()) {
      if (seg.state !== 'idle') continue
      const d = Math.abs((seg.start + seg.end) / 2 - center)
      if (d < bestDist) {
        bestDist = d
        best = seg
      }
    }
    return best
  }

  function hasActiveDesiredFetch(): boolean {
    for (const seg of activeFetches) {
      if (desired.get(seg.index) === seg) return true
    }
    return false
  }

  /**
   * 单段取数与落地。陈旧段仍完成 IPC，但因代次/对象复核不会落地；finally 释放槽位后
   * 重新踢泵，让期间最新的愿望窗口获得下一优先槽。
   */
  async function fetchSegment(seg: RenderSegment) {
    const myGeneration = generation
    activeFetches.add(seg)
    seg.state = 'loading'
    try {
      const rows = await opts.fetchBucketRows(seg.start, seg.end)
      // 落地三重复核:同代 + 该 index 的愿望对象仍是本对象(飞掠段已被 syncDesired
      // 丢弃 → get 返回 undefined 或新建对象 → 丢弃应答,杜绝幽灵挂载)。
      if (myGeneration === generation && desired.get(seg.index) === seg) {
        seg.rows = rows
        seg.state = 'ready'
      }
    } catch (err) {
      if (myGeneration === generation && desired.get(seg.index) === seg) {
        // error 粘滞至该段离窗重进或布局换代——LayoutNotReady 多为换代竞态,
        // 换代 watch 马上会整表重建。
        seg.state = 'error'
        logger.error(`${LOG} fetchBucketRows(${seg.start}, ${seg.end}) FAILED`, { error: err })
      }
    } finally {
      activeFetches.delete(seg)
      flushSettled()
      pumpFetch()
    }
  }

  /**
   * 常态只维持 1 个 IPC；若现有在途均已被远跳淘汰，则允许最新目标段占第 2 槽。
   * 连续横扫最多积压 2 个请求，其中任一落定后都会重新挑当前最近段，不追历史队列。
   */
  function pumpFetch() {
    while (true) {
      const limit = activeFetches.size === 0 || hasActiveDesiredFetch() ? 1 : 2
      if (activeFetches.size >= limit) return
      const seg = pickNextIdle()
      if (!seg) {
        flushSettled()
        return
      }
      void fetchSegment(seg)
      // fetchSegment 在首次 await 前同步把 seg 记为 loading 并入 activeFetches，下一轮可安全
      // 重新计算 limit；常态因此停在 1，只有陈旧请求占槽时才会进入第 2 槽。
      if (activeFetches.size >= 2) return
      if (hasActiveDesiredFetch()) return
    }
  }

  // ── 段稳定屏障(B2):FLIP 重排动画的 Last 快照必须等段行落地 ─────────────────
  // bucket 模式下 compute 换版本 → 段表重建 → 行数据**异步**回填,mutate()+nextTick 时
  // DOM 尚空 → FLIP 读不到新位置。whenSettled() 在「愿望集内无 idle/loading 段」时兑现,
  // 供删除重排等一次性时序消费;error 段视为已稳定(不无限等待)。
  const settleWaiters: Array<() => void> = []

  function isSettled(): boolean {
    for (const seg of desired.values()) {
      if (seg.state === 'idle' || seg.state === 'loading') return false
    }
    return true
  }

  function whenSettled(): Promise<void> {
    if (isSettled()) return Promise.resolve()
    return new Promise((resolve) => settleWaiters.push(resolve))
  }

  function flushSettled() {
    if (!isSettled()) return
    while (settleWaiters.length) settleWaiters.shift()!()
  }

  function rebuild() {
    const previous = new Map(desired)
    generation++
    desired.clear()
    lastRangeKey = ''
    // 行高变(→relayout→layoutVersion→本函数)时重算段高:段高与布局版本同源、一代内恒定。
    segPx = clampSegmentPx(opts.rowHeight())
    // 布局换代后钳制锚差(总高可能缩水;非映射态自然归 0)。滚动位恢复由宿主的
    // layoutVersion watcher 经 scrollToLogicalY 完成,此处只保证几何不越界。
    anchorDelta.value = Math.min(anchorDelta.value, Math.max(0, opts.totalHeight() - SPACER_CAP))
    rebuildSeed = previous
    try {
      // syncDesired 会先建立带旧 rows 的新代段，再 publish；期间不发布空段表。
      syncDesired(true)
    } finally {
      rebuildSeed = null
    }
  }

  /// 宿主 @scroll 转发入口。非映射态:纯记录 + 算术同步(零映射零补偿——顺滑来源)。
  /// 映射态(B3.1 输入源分类):有 1:1 印记的手势 → 局部 1:1(滚轮/惯性/键盘原生手感,
  /// 零干预);无印记的滚动链 = 滚动条拖动/轨道点击 → **逐事件**全局线性重锚(拇指比例
  /// 语义,拖到边 = 逻辑边);单事件巨跳(Home/End/拇指跳转)无论分类一律比例兜底。
  /// 手势进行中绝不写 scrollTop——偿债只在停稳后(scheduleRepay)。
  /// 返回 true = 本事件为程序化落点(偿债/远跳)的自触发事件,已被引擎消费;宿主的用户
  /// 手势侧采样(加载闸门速度采样)应豁免该事件——单帧巨位移会被误判为飞掠(审查 F1)。
  function onScroll(): boolean {
    const el = opts.containerRef()
    if (!el) return false
    const p = el.scrollTop
    if (internalScroll) {
      // 偿债/远跳落点的自触发事件:逻辑位与愿望窗口已就绪,仅更新跳变基准并断开手势链
      // (程序化落点不是用户手势,下一事件重新定类)。速度基准同步清零——程序化落点不是
      // 真实甩滚,不该把它的巨位移算进速度 EMA 抬高预取边距。
      internalScroll = false
      lastP = p
      lastScrollTs = -Infinity
      scrollVelocityPxMs = 0
      lastVelocityTs = 0
      return true
    }
    // 甩滚速度 EMA(px/ms,物理坐标):独立于映射态分类,驱动 adaptiveMarginPx。
    // lastVelocityTs=0 时(首次采样/刚重置)只播种基准,不计入本次——避免把首个事件的
    // 巨位移(如远跳后紧跟的下一帧)误判成瞬时高速。
    const nowMs = Date.now()
    if (lastVelocityTs > 0) {
      const dt = nowMs - lastVelocityTs
      if (dt > 0) {
        const inst = Math.abs(p - lastP) / dt
        scrollVelocityPxMs = scrollVelocityPxMs * 0.6 + inst * 0.4
      }
    }
    lastVelocityTs = nowMs
    const g = geometry()
    if (g.mapped) {
      const now = Date.now()
      if (now - lastScrollTs > SCROLL_CHAIN_MS) gestureOneToOne = now < oneToOneUntil
      lastScrollTs = now
      // 巨跳兜底:单个 scroll 事件位移超 3 屏,滚轮/触摸物理上给不出 → 必是拇指跳转。
      if (Math.abs(p - lastP) > Math.max(3 * g.viewH, 6000)) gestureOneToOne = false
      if (!gestureOneToOne) anchorDelta.value = globalLogical(p) - p
    }
    lastP = p
    logicalScrollTop.value = p + anchorDelta.value
    syncDesired()
    if (g.mapped) scheduleRepay()
    return false
  }

  /// 偿还压缩债(映射态):scrollTop 归位到当前逻辑位的全局线性位置。原子重锚——
  /// 先改 delta(段 top 绑定随 Vue 渲染更新),nextTick 后**同一事件循环任务内**写
  /// scrollTop:两写落同一渲染帧,内容零位移、仅滚动条拇指悄然归位。
  async function repayDebt() {
    const el = opts.containerRef()
    if (!el || !geometry().mapped) return
    const logical = el.scrollTop + anchorDelta.value
    const pStar = Math.round(globalPhysical(logical))
    if (Math.abs(pStar - el.scrollTop) < 2) return
    anchorDelta.value = logical - pStar
    await nextTick()
    internalScroll = true
    el.scrollTop = pStar
    lastP = pStar
  }

  function scheduleRepay() {
    if (settleTimer !== null) clearTimeout(settleTimer)
    const id = setTimeout(() => {
      if (settleTimer === id) settleTimer = null
      // 竞态守卫(B3.2):本回调可能在 clearTimeout 生效前已入任务队列——期间若有
      // 新滚动(lastScrollTs 更新),放弃本次偿债(新滚动已重新武装定时器),避免在
      // 手势恢复瞬间写 scrollTop 与其互搏。
      if (Date.now() - lastScrollTs < SCROLL_SETTLE_MS) return
      void repayDebt()
    }, SCROLL_SETTLE_MS)
    settleTimer = id
  }

  /// 宿主 @wheel.passive 转发入口(B3.1)。双职责:①盖 1:1 印记——新起手势据此与滚动条
  /// 拖动区分;②**边缘续滚**——物理钉边后原生不再产生 scroll 事件,改为直接推锚差,内容
  /// 以 1:1 继续滚到真正的逻辑边缘(修复真机「到边一跳一跳还能继续滚」:旧的钉边立即偿债
  /// 在手势中改 scrollTop,与拖拽/惯性互搏)。永不 preventDefault,对原生滚动零干预。
  function onWheel(e: WheelEvent) {
    oneToOneUntil = Date.now() + ONE_TO_ONE_STICKY_MS
    const g = geometry()
    if (!g.mapped) return
    const el = opts.containerRef()
    if (!el) return
    // deltaMode:0=像素(WebView2 常见)、1=行、2=页——归一到像素(镜像方案 A wheel 补偿)。
    let dy = e.deltaY
    if (e.deltaMode === 1) dy *= 16
    else if (e.deltaMode === 2) dy *= g.viewH || 800
    const p = el.scrollTop
    const logical = p + anchorDelta.value
    // 钉边判定留 1px 容差(缩放下 scrollTop 可为分数);推进后锚差可有 ±1px 瞬时越界,
    // 停稳偿债即归一。到达逻辑边缘后条件不再成立 → 硬停,不再「还能继续滚」。
    const pinnedBottom = dy > 0 && p >= g.physMax - 1 && logical < g.logMax
    const pinnedTop = dy < 0 && p <= 1 && logical > 0
    if (!pinnedBottom && !pinnedTop) return
    const next = Math.min(g.logMax, Math.max(0, logical + dy))
    anchorDelta.value = next - p
    logicalScrollTop.value = next
    // 边缘续滚在逻辑上就是一步滚动:记入 lastScrollTs,让停稳判定/偿债竞态守卫与
    // 手势链分类把它当作滚动事件对待。
    lastScrollTs = Date.now()
    syncDesired()
    scheduleRepay()
  }

  /// 宿主 @keydown 转发入口(B3.1):滚动键盖 1:1 印记(与滚轮同权)。
  function onKeydown(e: KeyboardEvent) {
    if (ONE_TO_ONE_KEYS.has(e.key)) oneToOneUntil = Date.now() + ONE_TO_ONE_STICKY_MS
  }

  /// 宿主 @touchmove.passive 转发入口(B3.1):触屏平移盖 1:1 印记(平移中持续刷新,
  /// 抬指后的惯性滚动由手势链续接分类)。
  function onTouchmove() {
    oneToOneUntil = Date.now() + ONE_TO_ONE_STICKY_MS
  }

  /// 程序化跳转到逻辑 y——scrubber/侧栏文件夹/锚点与缓存恢复/引擎切换的统一入口。
  /// 非映射态 = 直滚;映射态:近距(≤3 屏)且物理可达 → 局部滚动(可平滑,不重锚);
  /// 远跳 → 全局重锚 + 立即落点(跨千万 px 的平滑无意义,落点即出骨架/内容)。
  async function scrollToLogicalY(y: number, o?: { smooth?: boolean }) {
    const el = opts.containerRef()
    if (!el) return
    const g = geometry()
    const target = Math.min(Math.max(0, y), g.logMax)
    if (!g.mapped) {
      el.scrollTo({ top: target, behavior: o?.smooth ? 'smooth' : 'auto' })
      return
    }
    const pLocal = target - anchorDelta.value
    if (
      Math.abs(target - (el.scrollTop + anchorDelta.value)) <= 3 * g.viewH &&
      pLocal >= 0 &&
      pLocal <= g.physMax
    ) {
      // 程序化局部滚动(尤其 smooth 动画)产生的 scroll 事件序列必须按 1:1 分类,否则
      // 会被当作滚动条拖动逐事件重锚、破坏落点;600ms 覆盖动画启动,后续由手势链续接。
      oneToOneUntil = Date.now() + 600
      el.scrollTo({ top: pLocal, behavior: o?.smooth ? 'smooth' : 'auto' })
      return
    }
    const pStar = Math.round(globalPhysical(target))
    anchorDelta.value = target - pStar
    logicalScrollTop.value = target
    syncDesired(true, target)
    await nextTick()
    internalScroll = true
    el.scrollTop = pStar
    lastP = pStar
  }

  /// 当前已挂载各段的行(平铺)。供宿主对可视项就地 patch(缩略图/收藏/评分/色标回写、
  /// 上下文菜单查找)——与方案 A 的 visibleRows 消费面对齐。
  function mountedRows(): LayoutRow[] {
    const out: LayoutRow[] = []
    for (const seg of segments.value) {
      if (seg.rows) out.push(...seg.rows)
    }
    return out
  }

  // 布局换代 → 重建段表。immediate:挂载时即按当前视口建段(生产唯一形态)。
  watch(() => opts.layoutVersion(), rebuild, { immediate: true })

  // Canvas 模式翻转(一键切换 DOM↔Canvas)按新边距**即时**重算愿望窗口:否则可能出现
  // 「位图想预取 1.25 屏、行数据只到 1000px」的旧窗残留,要等下一次滚动才补齐(S3 的
  // 及时重取要求)。停用时 syncDesired 自会早退,行为与既有休眠语义一致。
  if (opts.canvasMode) {
    const isCanvasMode = opts.canvasMode
    watch(
      () => isCanvasMode(),
      () => syncDesired(true),
    )
  }

  // 视口尺寸变化 → 愿望窗口变化(宽度变化走 relayout→版本重建,此处兜住纯高度变化)。
  onMounted(() => {
    const el = opts.containerRef()
    if (!el || typeof ResizeObserver === 'undefined') return
    resizeObserver = new ResizeObserver(() => syncDesired(true))
    resizeObserver.observe(el)
  })

  onBeforeUnmount(() => {
    resizeObserver?.disconnect()
    resizeObserver = null
    if (settleTimer !== null) {
      clearTimeout(settleTimer)
      settleTimer = null
    }
  })

  return {
    segments,
    logicalScrollTop,
    anchorDelta,
    spacerHeight,
    onScroll,
    onWheel,
    onKeydown,
    onTouchmove,
    scrollToLogicalY,
    mountedRows,
    whenSettled,
  }
}
