// Canvas 版画廊网格的纯几何计算(命中检测 + cover 裁剪)。抽为纯函数以便单测,
// 与 MediaGridCanvas.vue 的绘制/IO 分离。承接项目「纯映射进 helpers + spec」惯例。
import type { LayoutRow, LayoutRowItem } from '../../types/layout'

/** 命中结果:项 + 所在行的逻辑 y(悬停卡定位需要格子的完整矩形,item 自身只带 x/w/h)。 */
export interface CellHit {
  item: LayoutRowItem
  rowY: number
}

/** 半开区间 `[start, end)`，表示与视口纵向范围相交的有序布局行。 */
export interface VisibleRowRange {
  start: number
  end: number
}

/**
 * Canvas 失活后，浏览器已排队的 rAF 与 ResizeObserver 回调仍可能到达。
 * 用单调代次拒绝旧回调，避免 KeepAlive 返回前按 0×0 几何重置 backing store。
 */
export class CanvasRenderLifecycle {
  private active = true
  private generation = 0

  get isActive(): boolean {
    return this.active
  }

  snapshot(): number {
    return this.generation
  }

  activate(): void {
    this.active = true
    this.generation++
  }

  deactivate(): void {
    this.active = false
    this.generation++
  }

  canDraw(generation: number): boolean {
    return this.active && this.generation === generation
  }
}

export type CanvasRafScheduleResult = 'skipped' | 'queued' | 'coalesced' | 'replaced'

export interface CanvasRafSchedulerOptions {
  snapshot: () => number
  canDraw: (generation: number) => boolean
  draw: () => void
  requestFrame: (callback: FrameRequestCallback) => number
  cancelFrame: (handle: number) => void
}

/**
 * 按 render generation 合并 Canvas 绘制帧。
 *
 * KeepAlive 首次挂载会先排入一帧再激活组件；激活会使 generation 递增。若只用一个
 * rafId，新代次的请求会被旧帧错误合并，旧帧随后又因代次失效而跳过绘制，直到滚动
 * 触发下一帧。这里把待执行帧的 generation 一并记录，保证每个当前代次至少有一帧。
 */
export class CanvasRafScheduler {
  private rafId: number | null = null
  private rafGeneration: number | null = null

  constructor(private readonly options: CanvasRafSchedulerOptions) {}

  schedule(): CanvasRafScheduleResult {
    const generation = this.options.snapshot()
    if (!this.options.canDraw(generation)) return 'skipped'

    let result: 'queued' | 'replaced' = 'queued'
    if (this.rafId !== null) {
      if (this.rafGeneration === generation) return 'coalesced'
      this.options.cancelFrame(this.rafId)
      this.rafId = null
      this.rafGeneration = null
      result = 'replaced'
    }

    let handle = -1
    handle = this.options.requestFrame(() => {
      if (this.rafId === handle) {
        this.rafId = null
        this.rafGeneration = null
      }
      if (!this.options.canDraw(generation)) return
      this.options.draw()
    })
    this.rafId = handle
    this.rafGeneration = generation
    return result
  }

  cancel(): void {
    if (this.rafId !== null) this.options.cancelFrame(this.rafId)
    this.rafId = null
    this.rafGeneration = null
  }
}

export interface CanvasPrefetchBudgets {
  /** 当前滚动方向前方的条目预算。 */
  ahead: number
  /** 当前滚动方向后方的条目预算。 */
  behind: number
}

/**
 * 按当前一屏真实格数计算 Canvas 预取预算。
 *
 * 旧实现固定为前方 96 / 后方 48，在 60px 极密网格下一屏可达数千格时只够约 2–4 行，
 * 稍快滚动就会越过热区。这里让条目预算与一屏容量同阶：默认前方覆盖 1.25 屏、后方保留
 * 0.5 屏；小屏/大卡片保持旧下限，异常超宽视口再由硬上限和组件侧 512MB 位图预算兜底。
 * 方向系数可调:闸门关闭(飞掠/拖拽)期组件用「仅前方 1 屏、后方 0」的收缩预算——
 * behindFactor ≤ 0 时后方预算严格为 0(不落旧下限 48),表示该方向整体停取。
 */
export function canvasPrefetchBudgets(
  visibleCells: number,
  aheadFactor = 1.25,
  behindFactor = 0.5,
): CanvasPrefetchBudgets {
  const cells = Math.max(0, Math.floor(visibleCells))
  return {
    ahead: Math.min(4096, Math.max(96, Math.ceil(cells * aheadFactor))),
    behind: behindFactor <= 0 ? 0 : Math.min(2048, Math.max(48, Math.ceil(cells * behindFactor))),
  }
}

/** runPrefetchWalk 的预算:新启动数/墙钟/全局在途三上限,任一到达即停。 */
export interface PrefetchWalkBudgets {
  maxStarts: number
  budgetMs: number
  maxInFlight: number
}

/** runPrefetchWalk 的 IO 面(组件注入;纯函数不识别缓存/闸门,只按回调观察)。 */
export interface PrefetchWalkIo {
  /** 计划游标:下一个候选项,耗尽返回 null。 */
  next: () => LayoutRowItem | null
  /** 触发备装(组件侧为 getImage;已缓存/在途/失败时为廉价 no-op)。 */
  ensure: (item: LayoutRowItem) => void
  isLoading: (id: number) => boolean
  loadingCount: () => number
  now: () => number
  /** idle deadline 余量(ms);非 idle 上下文(draw 尾同步分片)传 null。 */
  idleTimeRemaining: (() => number) | null
}

export interface PrefetchWalkResult {
  /** 本次真正新启动的加载数。 */
  started: number
  /** 走过的候选项数(含已缓存/在途的暖项)。 */
  walked: number
  /** true = 游标耗尽(计划完成);false = 因预算/在途上限提前停。 */
  exhausted: boolean
}

/**
 * 预取分片推进器:沿计划游标逐项 ensure,直至任一预算上限。**新启动数**与墙钟是两个
 * 独立预算——旧实现按「走过的项」计数,计划每帧随滚动重建后游标从暖头重扫,预算被
 * 已缓存/在途项白白耗尽,冷尾永远够不着(2026-07-17 阶段 6 根因之一);暖项走查是
 * 亚微秒级 Map/Set 探查,交给墙钟上限约束即可。
 */
export function runPrefetchWalk(io: PrefetchWalkIo, budgets: PrefetchWalkBudgets): PrefetchWalkResult {
  const startedAt = io.now()
  let started = 0
  let walked = 0
  while (started < budgets.maxStarts) {
    if (io.loadingCount() >= budgets.maxInFlight) break
    if (io.now() - startedAt >= budgets.budgetMs) break
    if (io.idleTimeRemaining && io.idleTimeRemaining() <= 1) break
    const item = io.next()
    if (!item) return { started, walked, exhausted: true }
    const wasLoading = io.isLoading(item.id)
    io.ensure(item)
    walked++
    if (!wasLoading && io.isLoading(item.id)) started++
  }
  return { started, walked, exhausted: false }
}

/**
 * 按行方向惰性枚举 Canvas 预取项。
 *
 * 只负责像素边界、方向和条目预算，不触发任何 IO。调用方可跨多个 idle slice 续取同一
 * iterator，避免每个分片都从预取窗口首行重扫，也避免一次同步启动数千个加载任务。
 */
export function* canvasPrefetchItems(
  rows: readonly LayoutRow[],
  start: number,
  step: 1 | -1,
  y0: number,
  viewH: number,
  marginPx: number,
  budget: number,
): Generator<LayoutRowItem> {
  let spent = 0
  const limit = Math.max(0, Math.floor(budget))
  for (let i = start; i >= 0 && i < rows.length && spent < limit; i += step) {
    const row = rows[i]
    if (step > 0 && row.y - y0 > viewH + marginPx) break
    if (step < 0 && row.y + row.height - y0 < -marginPx) break
    if (row.rowType !== 'normal') continue
    for (const item of row.items) {
      if (spent >= limit) return
      spent++
      yield item
    }
  }
}

/**
 * 在按 `row.y` 升序排列的布局行中二分找出与 `[startY, endY]` 相交的行区间。
 * 边界沿用旧绘制语义：行底恰贴视口顶、行顶恰贴视口底时仍包含，避免 1px 闪缝。
 */
export function visibleRowRange(
  rows: readonly LayoutRow[],
  startY: number,
  endY: number,
): VisibleRowRange {
  if (rows.length === 0 || endY < startY) return { start: 0, end: 0 }

  // 首个 row.bottom >= startY。
  let lo = 0
  let hi = rows.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    const row = rows[mid]
    if (row.y + row.height < startY) lo = mid + 1
    else hi = mid
  }
  const start = lo

  // 首个 row.y > endY；结果作为半开区间尾。
  hi = rows.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (rows[mid].y <= endY) lo = mid + 1
    else hi = mid
  }
  return { start, end: lo }
}

/** 二分定位包含逻辑 y 的行；落在行间空隙或范围外返回 -1。 */
function rowIndexAtY(rows: readonly LayoutRow[], y: number): number {
  let lo = 0
  let hi = rows.length
  // upper_bound(row.y, y) - 1。
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (rows[mid].y <= y) lo = mid + 1
    else hi = mid
  }
  const index = lo - 1
  if (index < 0) return -1
  const row = rows[index]
  return y < row.y + row.height ? index : -1
}

/**
 * 命中检测(带行坐标):内容区坐标 `x`(px,相对内容左缘)与逻辑 `y`(px,含滚动偏移)
 * 落在哪个媒体项上。未命中(落在格间空隙、分隔符行或越界)返回 null。布局行与行内项
 * 都按坐标升序排列，因此两级二分为 `O(log 行数 + log 行内项数)`，且不触碰逐格 DOM。
 */
export function hitTestCellWithRow(rows: LayoutRow[], x: number, y: number): CellHit | null {
  const rowIndex = rowIndexAtY(rows, y)
  if (rowIndex < 0) return null
  const row = rows[rowIndex]
  if (row.rowType !== 'normal') return null // 命中分隔符行 → 无可选项

  let lo = 0
  let hi = row.items.length
  // 首个 item.x > x 的前一项是唯一可能命中的格；额外检查右边界以识别格间空隙。
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (row.items[mid].x <= x) lo = mid + 1
    else hi = mid
  }
  const item = row.items[lo - 1]
  return item && x < item.x + item.w ? { item, rowY: row.y } : null
}

/** 命中检测(仅项):hitTestCellWithRow 的便捷面,点击/框选等只关心 id 的场景用。 */
export function hitTestCell(rows: LayoutRow[], x: number, y: number): LayoutRowItem | null {
  return hitTestCellWithRow(rows, x, y)?.item ?? null
}

// ── 悬停卡几何(T12 放大镜)────────────────────────────────────────────────────

export interface HoverRect {
  x: number
  y: number
  w: number
  h: number
  /** 弹出动画起始缩放(= 格原尺寸/目标尺寸,CSS 从 scale(scale0) 过渡到 1)。 */
  scale0: number
  /** 变换基点 = 原格中心在目标矩形内的坐标(px)。贴边钳位后目标矩形中心 ≠ 格中心,
   *  CSS 须以格中心为 transform-origin,scale(scale0) 的动画起点才与画格逐像素重合
   *  (默认居中 origin 会让贴边格的动画从偏移位开始,观感漂移)。 */
  originX: number
  originY: number
}

/**
 * 悬停卡目标矩形:以格中心等比放大 k = max(baseScale, min(minPx/min(w,h), maxScale))——
 * 常规格子放大 1.06 对齐 DOM hover;极小格向最小可用尺寸 minPx 放大(放大镜式,交互控件
 * 可点),但倍率**封顶 maxScale**:真机两轮反馈(先 1.5 仍嫌突兀 → 1.2)宁可略小于 minPx
 * 也保持温和。放大态再视口内推挤钳位:不越 [0,viewW]×[0,viewH],目标大于视口则居中
 * (贴边格子向内推,类 tooltip shift)。原位态(k=1)**不钳位**:钳位会把贴视口缘的格
 * (滚动停在中途时的首/尾行)平移出原格,卡与画格错位——DOM 模式里格子从不因 hover 挪位。
 * @param enableScale 是否启用用户设置的悬停放大；关闭时保持原格矩形(不钳位、不放大)。
 */
export function computeHoverRect(
  cell: { x: number; y: number; w: number; h: number },
  viewW: number,
  viewH: number,
  minPx = 120,
  baseScale = 1.06,
  maxScale = 1.2,
  enableScale = true,
): HoverRect {
  const k = enableScale
    ? Math.max(
        baseScale,
        Math.min(minPx / Math.max(1, Math.min(cell.w, cell.h)), maxScale),
      )
    : 1
  const w = cell.w * k
  const h = cell.h * k
  let x = cell.x + (cell.w - w) / 2
  let y = cell.y + (cell.h - h) / 2
  if (k > 1) {
    x = w >= viewW ? (viewW - w) / 2 : Math.min(Math.max(x, 0), viewW - w)
    y = h >= viewH ? (viewH - h) / 2 : Math.min(Math.max(y, 0), viewH - h)
  }
  return {
    x,
    y,
    w,
    h,
    scale0: 1 / k,
    originX: cell.x + cell.w / 2 - x,
    originY: cell.y + cell.h / 2 - y,
  }
}

/** object-fit: cover 的源裁剪矩形(居中裁剪)。 */
export interface CoverRect {
  sx: number
  sy: number
  sw: number
  sh: number
}

/**
 * 把 `nw×nh` 的源图以 cover 语义铺满 `w×h` 目标:取较大缩放比,居中裁掉溢出的一边。
 * 任一维 <=0 时退化为不裁剪(防除零 / NaN)。
 */
export function coverRect(nw: number, nh: number, w: number, h: number): CoverRect {
  if (nw <= 0 || nh <= 0 || w <= 0 || h <= 0) {
    return { sx: 0, sy: 0, sw: Math.max(0, nw), sh: Math.max(0, nh) }
  }
  const scale = Math.max(w / nw, h / nh)
  const sw = w / scale
  const sh = h / scale
  return { sx: (nw - sw) / 2, sy: (nh - sh) / 2, sw, sh }
}

// ── 解码期预缩放(createImageBitmap 管线)──────────────────────────────────────

/**
 * 位图目标高度阶梯(设备像素)。把连续的格高量化到离散桶,thumbSize 滑杆微调 / 容器 reflow
 * 不触发全量重解码——只有跨桶才换规格,期间旧位图按 stale 续画。顶桶 480 = 缩略图源上限,
 * 到顶后不再区分(等价于不缩)。
 */
export const BITMAP_HEIGHT_BUCKETS = [64, 96, 128, 192, 256, 384, 480] as const

/** 格高(CSS px)× DPR → 所属高度桶(设备像素):取 ≥ 目标的最小桶,超顶取顶桶。 */
export function bitmapBucketH(cellH: number, dpr: number): number {
  const target = cellH * dpr
  for (const b of BITMAP_HEIGHT_BUCKETS) {
    if (target <= b) return b
  }
  return BITMAP_HEIGHT_BUCKETS[BITMAP_HEIGHT_BUCKETS.length - 1]
}

/** `bitmapPrepParams` 的产物:整数化 cover 裁剪 + 可选缩小目标(null = 不缩,只裁)。 */
export interface BitmapPrep {
  sx: number
  sy: number
  sw: number
  sh: number
  /** 缩小目标宽/高(设备像素);null 表示源已 ≤ 桶高,裁剪后原样保留(只缩不放,保源清晰度)。 */
  outW: number | null
  outH: number | null
}

// ── 文本与角标几何(信息浮窗/分隔行/星级,纯函数单测)─────────────────────────────

/**
 * 按可用宽度截断文本并缀省略号(等价 CSS text-overflow: ellipsis 的单行语义)。
 * measure 由调用方注入(canvas 的 ctx.measureText 绑定当前字体),二分找最长可容前缀。
 */
export function truncateToWidth(
  text: string,
  maxW: number,
  measure: (s: string) => number,
): string {
  if (maxW <= 0) return ''
  if (measure(text) <= maxW) return text
  const ell = '…'
  let lo = 0
  let hi = text.length - 1
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2)
    if (measure(text.slice(0, mid) + ell) <= maxW) lo = mid
    else hi = mid - 1
  }
  return lo <= 0 ? ell : text.slice(0, lo) + ell
}

/**
 * 分隔行标签的「行内 sticky」钳位(等价 DOM position:sticky top:0 于行容器内):
 * 标签钉在视口顶(y=0),但不越出所在行的下缘;行高不足容纳标签时退回流内位置。
 * @param flowY 标签的流内 y(行顶 + margin,视口坐标)。
 * @param rowBottom 行下缘(视口坐标)。
 * @param labelH 标签高。
 */
export function clampStickyLabelY(flowY: number, rowBottom: number, labelH: number): number {
  const maxY = rowBottom - labelH
  if (maxY < flowY) return flowY
  return Math.min(Math.max(flowY, 0), maxY)
}

/**
 * 五角星顶点序列(外/内半径交替,起笔朝上),供 canvas 星级绘制连线填充。
 * innerRatio 0.5 近似 lucide Star 的填充观感。
 */
export function starPoints(
  cx: number,
  cy: number,
  outerR: number,
  innerRatio = 0.5,
  points = 5,
): Array<[number, number]> {
  const pts: Array<[number, number]> = []
  const rot = -Math.PI / 2
  for (let i = 0; i < points * 2; i++) {
    const r = i % 2 === 0 ? outerR : outerR * innerRatio
    const a = rot + (i * Math.PI) / points
    pts.push([cx + r * Math.cos(a), cy + r * Math.sin(a)])
  }
  return pts
}

// ── 选中态过渡动画(T4:canvas 无 CSS transition,自实现插值)────────────────────

/**
 * cubic-bezier 缓动求值器(语义等价 CSS transition-timing-function):给定控制点
 * (x1,y1)(x2,y2),返回 x∈[0,1]→y 的函数。x→t 用牛顿迭代 + 二分兜底(CSS 规范同款解法);
 * y 允许越界——DOM 选中动画的 (0.34, 1.18, 0.64, 1) 正是靠 y1>1 产生过冲的 spring 观感。
 */
export function cubicBezierEase(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): (x: number) => number {
  const cx = 3 * x1
  const bx = 3 * (x2 - x1) - cx
  const ax = 1 - cx - bx
  const cy = 3 * y1
  const by = 3 * (y2 - y1) - cy
  const ay = 1 - cy - by
  const sampleX = (t: number) => ((ax * t + bx) * t + cx) * t
  const sampleY = (t: number) => ((ay * t + by) * t + cy) * t
  const sampleDX = (t: number) => (3 * ax * t + 2 * bx) * t + cx
  return (x: number) => {
    if (x <= 0) return 0
    if (x >= 1) return 1
    let t = x
    for (let i = 0; i < 8; i++) {
      const err = sampleX(t) - x
      if (Math.abs(err) < 1e-5) return sampleY(t)
      const d = sampleDX(t)
      if (Math.abs(d) < 1e-6) break // 导数过平,转二分
      t -= err / d
    }
    let lo = 0
    let hi = 1
    t = x
    while (hi - lo > 1e-5) {
      if (sampleX(t) < x) lo = t
      else hi = t
      t = (lo + hi) / 2
    }
    return sampleY(t)
  }
}

/** DOM 选中动画同款曲线与时长(MediaThumb: transform .25s cubic-bezier(.34,1.18,.64,1))。 */
export const SELECT_EASE = cubicBezierEase(0.34, 1.18, 0.64, 1)
export const SELECT_ANIM_MS = 250

/**
 * 选中态过渡追踪器:记录「哪些格子正处于选中↔未选中的过渡中」,供 draw 逐格插值。
 * 快照纪律——只对 sync() 时**可见**的项做 diff 起动画;离屏时发生的选区变化(如全选后
 * 滚动)不回放动画,滚入即终态,避免「滚到哪、哪片成批跳动画」的失真。
 */
export class SelectionAnimTracker {
  private last = new Map<number, boolean>()
  private anims = new Map<number, { fromSelected: boolean; start: number }>()

  /** 选区版本变化时调用:diff 当前可见项起动画,快照重建为「仅可见集」。 */
  sync(visible: Iterable<{ id: number }>, isSelected: (id: number) => boolean, now: number): void {
    const next = new Map<number, boolean>()
    for (const { id } of visible) {
      const sel = isSelected(id)
      next.set(id, sel)
      const prev = this.last.get(id)
      if (prev !== undefined && prev !== sel) {
        this.anims.set(id, { fromSelected: prev, start: now })
      }
    }
    // 进行中的动画不随快照丢弃:250ms 内滚出又滚回不跳变,过期项由 hasActive 顺手清理。
    this.last = next
  }

  /**
   * 绘制时查询「选中程度」:0=未选中,1=完全选中;动画中返回 ease 后的插值
   * (配过冲曲线时可短暂 >1)。动画结束自动出队。
   */
  degree(
    id: number,
    selected: boolean,
    now: number,
    ease: (x: number) => number,
    durationMs: number,
  ): number {
    const anim = this.anims.get(id)
    if (!anim) return selected ? 1 : 0
    const t = (now - anim.start) / durationMs
    if (t >= 1) {
      this.anims.delete(id)
      return selected ? 1 : 0
    }
    const e = ease(Math.max(0, t))
    return anim.fromSelected ? 1 - e : e
  }

  /** 是否仍有进行中的动画(驱动 rAF 连续重绘);同时清理已到期项。 */
  hasActive(now: number, durationMs: number): boolean {
    let active = false
    for (const [id, a] of this.anims) {
      if (now - a.start >= durationMs) this.anims.delete(id)
      else active = true
    }
    return active
  }
}

/**
 * 由源图尺寸与格几何算「解码期预缩放」参数:先按 cover 语义裁到格纵横比(复用 coverRect,
 * 居中裁掉溢出边),再判断是否缩到高度桶——裁剪高超过 bucketH 才缩(只缩不放)。
 * 产出交给 `createImageBitmap(src, sx, sy, sw, sh, {resizeWidth, resizeHeight})`,
 * 故坐标全部整数化(spec 参数为 long)且钳位在源图界内。
 */
export function bitmapPrepParams(
  srcW: number,
  srcH: number,
  cellW: number,
  cellH: number,
  bucketH: number,
): BitmapPrep {
  const c = coverRect(srcW, srcH, cellW, cellH)
  const sx = Math.max(0, Math.floor(c.sx))
  const sy = Math.max(0, Math.floor(c.sy))
  const sw = Math.max(1, Math.min(Math.round(c.sw), srcW - sx))
  const sh = Math.max(1, Math.min(Math.round(c.sh), srcH - sy))
  if (sh <= bucketH || cellH <= 0) {
    return { sx, sy, sw, sh, outW: null, outH: null }
  }
  // 缩小目标按格纵横比取整,保证位图纵横比与格一致(draw 端 coverRect 退化为近全源)。
  const outH = bucketH
  const outW = Math.max(1, Math.round((bucketH * cellW) / cellH))
  return { sx, sy, sw, sh, outW, outH }
}
