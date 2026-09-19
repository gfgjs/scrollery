<template>
  <!-- Canvas 版时间轴 scrubber(原型,与 DOM 版 TimelineScrubber 同 props/emit 可直接替换)。
       canvas 只画「多节点」(月密度条/分隔符点),年份标签与浮层仍用 DOM(数量少且需左浮出轨道);
       hover 出放大镜(pointer-events:none 磁化预览,不抢指针)。§9 二期对比原型。 -->
  <div
    ref="trackRef"
    class="tlc-scrubber"
    :class="{ 'tlc-scrubber--dragging': dragging }"

    tabindex="0"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
    @pointerleave="onTrackLeave"
    @keydown="onKeydown"
    @focus="onFocus"
    @blur="onBlur"
  >
    <!-- 主轴:所有月密度条 / 分隔符点都画在这一张 canvas 上(替代数千 DOM 节点)。 -->
    <canvas ref="canvasRef" class="tlc-canvas"></canvas>

    <!-- 半透明视窗(VSCode minimap 式):矩形=当前可视区,按住拖动=连续即时滚动(不吸附)。
         所有坐标共用滚动比例几何(thumbGeometry):time 坐标也显示(2026-07-25 用户裁定)。取
         「滚动位置」语义而非「日历行」语义,与 MediaScrollbar 指针同式映射,恒对齐/同步贴顶贴底;
         线性式与日历行式只在端点重合,二者只能取一,日历定位改由点轨跳转/放大镜承担。 -->
    <!-- pointermove 不加 .stop:须冒泡到轨道层 processMove,拖动视窗时浮层/放大镜/聚光才跟随光标
         (轨道侧 dragging=false,只更新 hover 不重复 jump,对齐 DOM 版行为)。 -->
    <div
      v-if="viewportGeom"
      class="tlc-viewport"
      :style="{ top: `${viewportGeom.top}px`, height: `${viewportGeom.height}px` }"
      @pointerdown.stop="onViewportPointerdown"
      @pointermove="onViewportPointermove"
      @pointerup.stop="onViewportPointerup"
      @pointercancel.stop="onViewportPointerup"
    ></div>

    <!-- 年份标签(DOM 覆盖,数量少;canvas 无法左浮出 24px 轨道进画廊区)。time 坐标按日历行定位。 -->
    <template v-if="effectiveCoord === 'time'">
      <span
        v-for="yl in timeYearLabels"
        :key="yl.year"
        class="tlc-year"
        :style="{ top: `${yl.topPct}%` }"
        >{{ yl.year }}</span
      >
    </template>
    <template v-else-if="hasMonths">
      <span
        v-for="i in yearLabelIndices"
        :key="props.monthBuckets[i].groupId"
        class="tlc-year"
        :style="{ top: `${(props.monthBuckets[i].y / trackTotal) * 100}%` }"
        >{{ props.monthBuckets[i].year }}</span
      >
    </template>

    <!-- hover/拖拽浮层:年-月 · 张数(date) 或 分组名(folder)。 -->
    <div
      v-if="hasMonths && hoverIndex !== null"
      class="tlc-flyout"
      :style="{ top: `${monthFlyoutTopPct}%`, marginRight: flyoutShift }"
    >
      {{ props.monthBuckets[hoverIndex].year }}-{{
        String(props.monthBuckets[hoverIndex].month).padStart(2, '0')
      }}
      <span class="tlc-flyout__count">· {{ props.monthBuckets[hoverIndex].count }}</span>
    </div>
    <div
      v-else-if="!hasMonths && sepHoverIndex !== null"
      class="tlc-flyout"
      :style="{ top: `${(props.separators[sepHoverIndex].y / trackTotal) * 100}%`, marginRight: flyoutShift }"
    >
      {{ props.separators[sepHoverIndex].label }}
    </div>

    <!-- 放大镜(pointer-events:none):把光标附近一小段轨道逻辑区间纵向放大,让密集小节点看清。 -->
    <canvas
      v-show="loupeVisible"
      ref="loupeRef"
      class="tlc-loupe"
      :style="{ top: `${loupeTop}px` }"
    ></canvas>
  </div>
</template>

<script setup lang="ts">
// Canvas 版时间轴 scrubber 原型(§9 二期,与 DOM 版 TimelineScrubber 对比)。渲染改用单张 canvas,
// 交互/映射逻辑复用同一批 timelineScrubber.helpers 纯函数,保证与 DOM 版行为一致。
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import type { MonthBucket } from '../../types/layout'
import { parseColorToRgb } from '../../utils/color'
import type { ThemePalette } from '../../themes/types'
import { performanceRecorder } from '../../perf/performanceRecorder'
import { writeSettings } from '../../stores/settingsPersistence'
import { readSettingEnum } from '../../composables/settingsValues'
import { thumbGeometry, thumbTopToLogicalY } from './mediaScrollbar.helpers'
import {
  maxBucketCount,
  densityBarWidth,
  findActiveMonthIndex,
  isYearBoundary,
  nearestSeparatorIndex,
  nearestSeparatorWindow,
  buildRowIntensity,
  buildTimeBand,
  logicalYToTimeFrac,
  visibleYearLabelSet,
  stepScrubberIndex,
} from './timelineScrubber.helpers'

// 密度带视觉形态(方案 §2,可一键切换):bars=离散月条基线(当前行为),envelope=填充包络+轻热力,
// heat=纯热力色带,spectral=包络+冷暖光谱。存设置键 timeline_visual(设置集中保存,批次B)。
type VisualMode = 'bars' | 'envelope' | 'heat' | 'spectral'
const VISUAL_MODES: VisualMode[] = ['bars', 'envelope', 'heat', 'spectral']
const VISUAL_KEY = 'timeline_visual'

// 坐标系(方案 §1,可一键切换,与视觉正交):item=项累计空间(精确滚动 minimap,零后端),
// time=日历时间空间(纵向疏密,近 Apple/Google Photos;需 separator.epochDay)。存设置键 timeline_coord。
type CoordMode = 'item' | 'time'
const COORD_MODES: CoordMode[] = ['item', 'time']
const COORD_KEY = 'timeline_coord'

const props = withDefaults(
  defineProps<{
    monthBuckets: MonthBucket[]
    separators: { label: string; y: number; groupId?: string; count: number; epochDay: number | null }[]
    totalHeight: number
    currentY?: number
    /** 拖动视窗最小高(px):与滚动条 thumb 共享同一设置,二者同高(默认 48)。 */
    minThumb?: number
    /** 当前主题色板(themeStore.currentPalette):DOM 与 Canvas 消费同一份生成结果。 */
    themePalette: ThemePalette
  }>(),
  { currentY: 0, minThumb: 48 },
)

// 第二参 smooth:指针拖拽=false 即时落点(跟手),键盘/单击省略=平滑。
const emit = defineEmits<{
  jump: [y: number, smooth?: boolean]
  /** 轴上拖拽(轨道 scrub / 视窗拖动)起止:宿主中转给 MediaScrollbar 展开标尺线。 */
  scrubbing: [on: boolean]
}>()

const trackRef = ref<HTMLElement | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const loupeRef = ref<HTMLCanvasElement | null>(null)

const dragging = ref(false)
const hoverIndex = ref<number | null>(null)
const sepHoverIndex = ref<number | null>(null)
const loupeVisible = ref(false)
const loupeTop = ref(0) // 放大镜垂直中心(轨道内 px)
let loupeLogicalY = 0 // 放大镜中心对应逻辑 y
const cursorPx = ref<number | null>(null) // 光标轨道内 y(P2 聚光中心);null = 无指针悬停

const monthCount = computed(() => props.monthBuckets.length)
const hasMonths = computed(() => monthCount.value > 0)
const monthMaxCount = computed(() => maxBucketCount(props.monthBuckets))
const trackTotal = computed(() => Math.max(1, props.totalHeight))

// 轨道 CSS 尺寸(canvas 绘制基准);ResizeObserver 跟随。
const trackW = ref(0)
const trackH = ref(0)

// ── 半透明视窗(VSCode minimap 式拖动把手)────────────────────────────────────
// 复用 thumbGeometry:矩形=当前可视区比例;拖它=连续即时滚动(emit smooth=false),不吸附不跳。
// DOM 覆盖层(需接指针事件,非 canvas 绘制)。内容不足一屏(geom=null)时不显。视窗最小高由
// minThumb prop 决定,与滚动条 thumb 同值 → 二者同高;两端恒对齐(top=frac×(trackH−h),frac=0/1
// 处与 h 无关,恒贴顶/底)。
const viewportGeom = computed(() =>
  thumbGeometry(props.currentY, props.totalHeight, trackH.value, props.minThumb),
)
let vpDragging = false
let vpGrabOffset = 0
let vpMoveRaf: number | null = null
let vpPendingClientY: number | null = null
function onViewportPointerdown(e: PointerEvent) {
  if (e.button !== 0) return
  const g = viewportGeom.value
  const el = trackRef.value
  if (!g || !el) return
  vpDragging = true
  emit('scrubbing', true)
  vpGrabOffset = e.clientY - el.getBoundingClientRect().top - g.top
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  e.preventDefault()
}
function applyViewportMove(clientY: number) {
  if (!vpDragging) return
  const g = viewportGeom.value
  const el = trackRef.value
  if (!g || !el) return
  const thumbTop = clientY - el.getBoundingClientRect().top - vpGrabOffset
  emit('jump', thumbTopToLogicalY(thumbTop, props.totalHeight, trackH.value, g.height), false)
}
function onViewportPointermove(e: PointerEvent) {
  vpPendingClientY = e.clientY
  if (vpMoveRaf !== null) return
  vpMoveRaf = requestAnimationFrame(() => {
    vpMoveRaf = null
    const clientY = vpPendingClientY
    vpPendingClientY = null
    if (clientY !== null) applyViewportMove(clientY)
  })
}
function cancelViewportMove() {
  if (vpMoveRaf !== null) cancelAnimationFrame(vpMoveRaf)
  vpMoveRaf = null
  vpPendingClientY = null
}
function onViewportPointerup(e: PointerEvent) {
  // pointerup 前补交最后一个采样点，避免 rAF 尚未执行时末端位置丢失。
  if (vpPendingClientY !== null) applyViewportMove(vpPendingClientY)
  cancelViewportMove()
  if (vpDragging) emit('scrubbing', false)
  vpDragging = false
  ;(e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId)
}

// 视觉形态(可切换,存设置 timeline_visual)。默认 bars = 当前离散月条,切换前零行为变化。
// 用户改动经 cycleVisualMode 显式提交;watch(visualMode) 只负责重画,不再承担落盘。
const visualMode = computed(() => readSettingEnum<VisualMode>(VISUAL_KEY, VISUAL_MODES, 'bars'))
function cycleVisualMode() {
  const i = VISUAL_MODES.indexOf(visualMode.value)
  setVisualMode(VISUAL_MODES[(i + 1) % VISUAL_MODES.length])
}
/** 用户显式提交视觉形态;写盘失败由中央服务统一提示,此处 catch 只为收掉 promise。 */
function setVisualMode(mode: VisualMode) {
  writeSettings({ [VISUAL_KEY]: mode }).catch(() => {})
}
// ── 坐标系(item/time,可切换,持久化)──────────────────────────────────────────
// time 坐标需 separator 带 epochDay(仅 date 分组);且 bars 是 item 空间离散基线,time 坐标只对
// 密度带(envelope/heat/spectral)有意义 → coordApplies 双重门控:date 分组 + 非 bars 视觉。
const coordMode = computed(() => readSettingEnum<CoordMode>(COORD_KEY, COORD_MODES, 'item'))
/** 用户显式提交坐标系;写盘失败由中央服务统一提示,此处 catch 只为收掉 promise。 */
function setCoordMode(mode: CoordMode) {
  writeSettings({ [COORD_KEY]: mode }).catch(() => {})
}
const hasTime = computed(() => props.separators.some((s) => s.epochDay != null))
const coordApplies = computed(() => hasTime.value && visualMode.value !== 'bars')
// 实际生效坐标:仅当用户选 time 且当前上下文支持(date 分组 + 密度带视觉)才为 time,否则回落 item。
const effectiveCoord = computed<CoordMode>(() =>
  coordMode.value === 'time' && coordApplies.value ? 'time' : 'item',
)
function toggleCoordMode() {
  setCoordMode(coordMode.value === 'time' ? 'item' : 'time')
}
// 时间比例密度带(§3):仅 time 坐标生效时构建(含 intensity/rowJumpY/日历刻度);否则 null。
// computed 缓存:数据/尺寸/坐标变时才重算,非每帧。
const timeBand = computed(() =>
  effectiveCoord.value === 'time'
    ? buildTimeBand(props.separators, props.totalHeight, Math.floor(trackH.value))
    : null,
)

// 密度带强度数组(重采样到像素行,方案 §3);仅数据/尺寸/坐标变时重算(computed 缓存),非每帧。
// bars 模式不消费它,故仅非 bars 时构建;time 坐标取 timeBand.intensity(日历铺行),否则 item 累计铺行。
const rowIntensity = computed<Float32Array>(() => {
  if (visualMode.value === 'bars') return new Float32Array(0)
  const tb = timeBand.value
  if (tb) return tb.intensity
  return buildRowIntensity(props.separators, props.totalHeight, Math.floor(trackH.value))
})

// 年份标签(DOM,少量)——复用与 DOM 版同一去挤叠逻辑。
const yearLabelSet = computed(() =>
  visibleYearLabelSet(props.monthBuckets, props.totalHeight, trackH.value),
)
const yearLabelIndices = computed(() => [...yearLabelSet.value])

// time 坐标年标:item 空间的 monthBucket.y 在此坐标会错位,改按 yearTicks 的像素行定位。
// yearTicks 行号按年代序单调(朝向决定增减,故用 |Δpx|),逐个保留与上一已显标签距离足够者
// ——去挤叠与 item 坐标 visibleYearLabelSet 同 12px 准则,防超长跨度库(yearTicks 可达上百)标签堆叠。
const TIME_YEAR_MIN_GAP = 12
const timeYearLabels = computed(() => {
  const tb = timeBand.value
  if (!tb) return [] as { year: number; topPct: number }[]
  const rows = tb.intensity.length
  if (rows === 0) return []
  const pxPerRow = trackH.value > 0 ? trackH.value / rows : 1
  const out: { year: number; topPct: number }[] = []
  let lastPx = -Infinity
  for (const yt of tb.yearTicks) {
    const px = yt.row * pxPerRow
    if (Math.abs(px - lastPx) >= TIME_YEAR_MIN_GAP) {
      out.push({ year: yt.year, topPct: (yt.row / rows) * 100 })
      lastPx = px
    }
  }
  return out
})

// 月浮层纵向位置:time 坐标跟随光标(item 空间 bucket.y 会错位);键盘导航无光标(cursorPx=null)时经
// logicalYToTimeFrac 逆映射到时间行(评审 R3:否则浮层与同为时间行定位的指示线竖向分离)。item 坐标
// 按 bucket 逻辑 y 定位。
const monthFlyoutTopPct = computed(() => {
  if (hoverIndex.value === null) return 0
  if (effectiveCoord.value === 'time' && trackH.value > 0) {
    if (cursorPx.value !== null) return (cursorPx.value / trackH.value) * 100
    const tb = timeBand.value
    if (tb && tb.rowJumpY.length > 1) {
      return logicalYToTimeFrac(tb.rowJumpY, props.monthBuckets[hoverIndex.value].y) * 100
    }
  }
  return (props.monthBuckets[hoverIndex.value].y / trackTotal.value) * 100
})

// ── 调色板:颜色只从 props.themePalette 投影(与 DOM/网格 Canvas 同源),不读 DOM 取色、不留第二套
// 默认色。只有字体族与尺寸从 DOM 读一次(getComputedStyle 强制同步样式重算,不进每帧热路径)。
interface Palette {
  accent: string
  border: string
  text1: string
  text2: string
  text3: string
  hover: string
}
// 时间轴画在画廊底上,故文字取画廊底派生值:显式 gallery 可与窗口底色反极性,拿窗口
// textPrimary/secondary 会在「暗界面 + 浅色画廊」上读不清。第三档(最弱)复用辅助档,
// 靠字号区分层级。
function projectTimelinePalette(theme: ThemePalette): Palette {
  return {
    accent: theme.accent,
    border: theme.border,
    text1: theme.canvasText,
    text2: theme.canvasTextSecondary,
    text3: theme.canvasTextSecondary,
    // 中心行底色对齐 DOM 版 .is-center(选中浅底)。
    hover: theme.selection,
  }
}
// 挂载时赋值(onMounted 先于任何 draw);此处不留占位色,避免第二套默认值。
let palette!: Palette
// 密度带按强度在 border→accent 插值需 rgb 分量;生成色板恒为规范 hex,解析失败退回零值只为不崩。
type Rgb = { r: number; g: number; b: number }
function rgbOf(color: string): Rgb {
  return parseColorToRgb(color) ?? { r: 0, g: 0, b: 0 }
}
let accentRgb: Rgb = { r: 0, g: 0, b: 0 }
let borderRgb: Rgb = { r: 0, g: 0, b: 0 }
/** border→accent(或任意两色)线性插值为 rgba 串(t∈[0,1],alpha 独立)。 */
function rgbaLerp(c0: Rgb, c1: Rgb, t: number, alpha: number): string {
  const r = Math.round(c0.r + (c1.r - c0.r) * t)
  const g = Math.round(c0.g + (c1.g - c0.g) * t)
  const b = Math.round(c0.b + (c1.b - c0.b) * t)
  return `rgba(${r},${g},${b},${alpha})`
}
/** 冷→暖光谱色(蓝 210°→橙 20°),供 spectral 视觉;固定饱和/明度,alpha 独立。 */
function spectralColor(t: number, alpha: number): string {
  const hue = 210 - 190 * Math.min(1, Math.max(0, t))
  return `hsla(${hue},70%,55%,${alpha})`
}
let fontSans = 'sans-serif'
/** 拼 canvas font 串(缓存的应用字体族),bold 供中心行/悬停强调。 */
function fontFor(px: number, bold = false): string {
  return `${bold ? 'bold ' : ''}${px}px ${fontSans}`
}
// palette/accentRgb 等是非响应式模块变量(有意,避免深代理),此版本号在 applyPalette 后自增,
// 供依赖色值的 computed(rowFillStyles)感知主题切换。
const paletteVersion = ref(0)
function applyPalette() {
  const el = trackRef.value
  if (!el) return
  palette = projectTimelinePalette(props.themePalette)
  fontSans = getComputedStyle(el).getPropertyValue('--font-sans').trim() || 'sans-serif'
  accentRgb = rgbOf(palette.accent)
  borderRgb = rgbOf(palette.border)
  paletteVersion.value++
}

// DPR 适配:绘制缓冲按 devicePixelRatio 放大 + setTransform 缩回 CSS 像素,保证高分屏不糊(评审 M-canvas)。
// 幂等:仅当目标尺寸变化时才改 canvas.width/height(设置像素尺寸会清空 + 重分配后备存储,昂贵),
// 故放大镜每帧重画时尺寸不变则不重分配——只 setTransform(廉价)。
function fitCanvas(cv: HTMLCanvasElement, cssW: number, cssH: number): CanvasRenderingContext2D | null {
  const ctx = cv.getContext('2d')
  if (!ctx) return null
  const dpr = window.devicePixelRatio || 1
  const w = Math.max(1, Math.round(cssW * dpr))
  const h = Math.max(1, Math.round(cssH * dpr))
  if (cv.width !== w || cv.height !== h) {
    cv.width = w
    cv.height = h
    cv.style.width = `${cssW}px`
    cv.style.height = `${cssH}px`
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  return ctx
}

// ── 静态轴层缓存 ────────────────────────────────────────────────────────────
// 月条/文件夹点/密度带只随数据、尺寸、主题和视觉模式变化，不随 currentY/hover 变化，
// 缓存在同 DPR 离屏 canvas 后滚动帧只需一次位图合成 + O(1) 动态标记，避免 folder 分组较多时
// 每帧重放全部 arc/fill。
let staticAxisCanvas: HTMLCanvasElement | null = null
let staticAxisDirty = true

function invalidateStaticAxis() {
  staticAxisDirty = true
  scheduleDraw()
}

function drawStaticAxis(ctx: CanvasRenderingContext2D, cssW: number, cssH: number) {
  ctx.clearRect(0, 0, cssW, cssH)
  const span = Math.max(1, props.totalHeight)
  const yToPx = (y: number) => (y / span) * cssH
  if (visualMode.value === 'bars') drawBarsBase(ctx, cssW, yToPx)
  else drawBand(ctx, cssW, cssH)
  const tb = timeBand.value
  if (tb) drawMonthTicks(ctx, cssW, cssH, tb) // time 坐标 LOD 月网格线(§6)
}

function ensureStaticAxis(cssW: number, cssH: number): HTMLCanvasElement | null {
  if (!staticAxisCanvas) staticAxisCanvas = document.createElement('canvas')
  const dpr = window.devicePixelRatio || 1
  const targetW = Math.max(1, Math.round(cssW * dpr))
  const targetH = Math.max(1, Math.round(cssH * dpr))
  if (staticAxisCanvas.width !== targetW || staticAxisCanvas.height !== targetH) staticAxisDirty = true
  const layerCtx = fitCanvas(staticAxisCanvas, cssW, cssH)
  if (!layerCtx) return null
  if (staticAxisDirty) {
    const monitored = performanceRecorder.isActive()
    const startedAt = monitored ? performance.now() : 0
    drawStaticAxis(layerCtx, cssW, cssH)
    staticAxisDirty = false
    if (monitored) performanceRecorder.recordSpan('timeline.static', performance.now() - startedAt)
  }
  return staticAxisCanvas
}

/** 设备像素 1:1 合成静态层，避免再次缩放与逻辑坐标取整。 */
function blitStaticAxis(ctx: CanvasRenderingContext2D, cssW: number, cssH: number): boolean {
  const layer = ensureStaticAxis(cssW, cssH)
  const target = canvasRef.value
  if (!layer || !target) return false
  ctx.save()
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.clearRect(0, 0, target.width, target.height)
  ctx.drawImage(layer, 0, 0)
  ctx.restore()
  return true
}

// 主轴动态层：滚动/hover 帧只合成静态层并画高亮、聚光和当前位置线。
function drawAxis(ctx: CanvasRenderingContext2D, cssW: number, cssH: number) {
  if (!blitStaticAxis(ctx, cssW, cssH)) drawStaticAxis(ctx, cssW, cssH)
  const span = Math.max(1, props.totalHeight)
  const yToPx = (y: number) => (y / span) * cssH
  if (visualMode.value === 'bars') drawBarsHighlights(ctx, cssW, yToPx)
  drawSpotlight(ctx, cssW, cssH) // P2:光标渐变聚光(叠在标记上,在指示线之下)
  // 当前滚动位置指示线(所有视觉/坐标共用):currentY/可滚动行程(span−cssH)取比例,顶/底
  // 恰贴两端(分母取 span 时贴底留空档,用户回报),与 MediaScrollbar 标尺线同源映射。
  // time 坐标同走此线性式(2026-07-25 用户裁定:轴内指针与画廊指针全程对齐+同步贴顶/贴底
  // 优先)——线性式与 logicalYToTimeFrac 日历行式只在端点重合,指示线因此不落在密度带的
  // 日历行位;日历定位由点轨跳转/放大镜承担。
  const scrollFrac = Math.min(1, Math.max(0, props.currentY / Math.max(1, span - cssH)))
  const cy = scrollFrac * cssH
  if (cy >= 0 && cy <= cssH) {
    ctx.strokeStyle = palette.accent
    ctx.globalAlpha = 0.5
    ctx.beginPath()
    ctx.moveTo(0, cy)
    ctx.lineTo(cssW, cy)
    ctx.stroke()
    ctx.globalAlpha = 1
  }
}

// time 坐标 LOD 月网格线(§6):monthTicks 给出每个月初落在的像素行;仅当平均间距 >= 8px(不拥挤)
// 才画,否则概览尺度大库自动停显(避免把 ~700px 轨道塞几百条线糊成一片)。faint 边框色右侧短线。
const MONTH_TICK_MIN_GAP = 8
function drawMonthTicks(
  ctx: CanvasRenderingContext2D,
  cssW: number,
  cssH: number,
  tb: { monthTicks: number[]; intensity: Float32Array },
) {
  const ticks = tb.monthTicks
  const rows = tb.intensity.length
  if (ticks.length === 0 || rows === 0) return
  if (rows / ticks.length < MONTH_TICK_MIN_GAP) return // 太拥挤 → LOD 停显(细节交给放大镜)
  const rowH = cssH / rows
  ctx.strokeStyle = palette.border
  ctx.globalAlpha = 0.35
  ctx.lineWidth = 1
  ctx.beginPath()
  for (const r of ticks) {
    const y = Math.round(r * rowH) + 0.5 // +0.5 让 1px 线在物理像素上清晰
    ctx.moveTo(cssW * 0.55, y)
    ctx.lineTo(cssW, y)
  }
  ctx.stroke()
  ctx.globalAlpha = 1
}

/** 画单个月条；静态底层与动态 active/hover 共用同一几何。 */
function drawMonthBar(
  ctx: CanvasRenderingContext2D,
  cssW: number,
  yToPx: (y: number) => number,
  index: number,
  color: string,
  height: number,
) {
  const b = props.monthBuckets[index]
  if (!b) return
  const w = (densityBarWidth(b.count, monthMaxCount.value) / 100) * cssW
  const py = yToPx(b.y)
  ctx.fillStyle = color
  ctx.fillRect(cssW - w, py - height / 2, w, height)
}

// bars 静态底层：date=普通/年首月条，folder=全量分隔点；只在缓存失效时重建。
function drawBarsBase(ctx: CanvasRenderingContext2D, cssW: number, yToPx: (y: number) => number) {
  if (hasMonths.value) {
    for (let i = 0; i < props.monthBuckets.length; i++) {
      const isYear = isYearBoundary(props.monthBuckets, i)
      drawMonthBar(ctx, cssW, yToPx, i, isYear ? palette.text2 : palette.border, 3)
    }
  } else {
    // 全量点合并成一个 path + 一次 fill；该成本只在静态层重建时支付。
    const cx = cssW / 2
    ctx.fillStyle = palette.border
    ctx.beginPath()
    for (const separator of props.separators) {
      const py = yToPx(separator.y)
      ctx.moveTo(cx + 2, py)
      ctx.arc(cx, py, 2, 0, Math.PI * 2)
    }
    ctx.fill()
  }
}

// bars 动态高亮：每帧最多画 hover + active 两个条，或一个文件夹点。
function drawBarsHighlights(
  ctx: CanvasRenderingContext2D,
  cssW: number,
  yToPx: (y: number) => number,
) {
  if (hasMonths.value) {
    const activeIdx = findActiveMonthIndex(props.monthBuckets, props.currentY)
    const hoverIdx = hoverIndex.value
    if (hoverIdx !== null && hoverIdx !== activeIdx) {
      drawMonthBar(ctx, cssW, yToPx, hoverIdx, palette.text1, 4)
    }
    if (activeIdx >= 0) drawMonthBar(ctx, cssW, yToPx, activeIdx, palette.accent, 5)
    return
  }
  const hoverIdx = sepHoverIndex.value
  if (hoverIdx === null) return
  const separator = props.separators[hoverIdx]
  if (!separator) return
  ctx.fillStyle = palette.accent
  ctx.beginPath()
  ctx.arc(cssW / 2, yToPx(separator.y), 3.5, 0, Math.PI * 2)
  ctx.fill()
}

// 密度带视觉(方案 §5):rowIntensity 逐像素行绘制。heat=全宽色浓淡;envelope/spectral=横向长度(右
// 对齐,保底可见)+ 沿包络描 accent 山脊线。渲染成本 O(trackH)≈700 次 fillRect,与库规模解耦。
const BAND_FLOOR = 0.12 // envelope/spectral「有但少」的行保底可见宽度比例

// 逐行填充色缓存(评审 R7):避免 drawBand 内现拼 rgba/hsla 字符串造成 ~rows(≈700)次短命分配。
// 色串只依赖 强度×视觉模式×调色板,预先算成数组;数据/模式/主题(paletteVersion)变时才重建。
// bars 模式 rowIntensity 为空 → 空表。
const rowFillStyles = computed<string[]>(() => {
  void paletteVersion.value // 显式建立主题依赖(palette 本身非响应式)
  const inten = rowIntensity.value
  const mode = visualMode.value
  const out = new Array<string>(inten.length)
  for (let r = 0; r < inten.length; r++) {
    const v = inten[r]
    out[r] =
      mode === 'heat'
        ? rgbaLerp(borderRgb, accentRgb, v, 0.1 + 0.6 * v)
        : mode === 'spectral'
          ? spectralColor(v, 0.25 + 0.5 * v)
          : rgbaLerp(borderRgb, accentRgb, v, 0.18 + 0.42 * v)
  }
  return out
})

function drawBand(ctx: CanvasRenderingContext2D, cssW: number, cssH: number) {
  const inten = rowIntensity.value
  const rows = inten.length
  if (rows === 0) return
  const mode = visualMode.value
  const fills = rowFillStyles.value // 预算色表,热路径零字符串分配
  const rowH = cssH / rows // 像素行高(cssH 可能非整,均摊到每行避免留缝)
  for (let r = 0; r < rows; r++) {
    const v = inten[r] // 归一化 [0,1]
    const y = r * rowH
    if (mode === 'heat') {
      ctx.fillStyle = fills[r]
      ctx.fillRect(0, y, cssW, rowH + 0.5)
    } else {
      const len = v > 0 ? (BAND_FLOOR + v * (1 - BAND_FLOOR)) * cssW : 0
      if (len <= 0) continue
      ctx.fillStyle = fills[r]
      ctx.fillRect(cssW - len, y, len, rowH + 0.5)
    }
  }
  // 山脊线(envelope/spectral):沿每行包络右起点连成一条 accent 细线,强化轮廓。
  if (mode !== 'heat') {
    ctx.beginPath()
    for (let r = 0; r < rows; r++) {
      const v = inten[r]
      const len = v > 0 ? (BAND_FLOOR + v * (1 - BAND_FLOOR)) * cssW : 0
      const x = cssW - len
      const y = r * rowH + rowH / 2
      if (r === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.strokeStyle = palette.accent
    ctx.globalAlpha = 0.6
    ctx.lineWidth = 1
    ctx.stroke()
    ctx.globalAlpha = 1
  }
}

// P2 全轨渐变聚光:光标处以 accent 竖直渐隐(falloff R≈trackH*0.18)高亮已绘制的标记。用
// source-atop 只把渐变叠到已有像素上、不涂空白轨道 → 一次 gradient+fillRect,成本可忽略
// (DOM 做等效效果需 ~700 元素/逐帧 mask)。无指针悬停(cursorPx=null,如键盘导航)不画;色值用
// 缓存 accentRgb,不入 getComputedStyle 热路径。
function drawSpotlight(ctx: CanvasRenderingContext2D, cssW: number, cssH: number) {
  const cy = cursorPx.value
  if (cy === null) return
  const R = Math.max(50, cssH * 0.18)
  const a = accentRgb
  const g = ctx.createLinearGradient(0, cy - R, 0, cy + R)
  g.addColorStop(0, `rgba(${a.r},${a.g},${a.b},0)`)
  g.addColorStop(0.5, `rgba(${a.r},${a.g},${a.b},0.55)`)
  g.addColorStop(1, `rgba(${a.r},${a.g},${a.b},0)`)
  ctx.save()
  ctx.globalCompositeOperation = 'source-atop' // 只叠到已绘制标记上,不涂空轨
  ctx.fillStyle = g
  ctx.fillRect(0, 0, cssW, cssH)
  ctx.restore() // 复位 composite,后续指示线正常 source-over 叠上
}

let rafId: number | null = null
function scheduleDraw() {
  const monitored = performanceRecorder.isActive()
  if (monitored) {
    performanceRecorder.count('timeline.drawRequests')
    if (rafId !== null) performanceRecorder.count('timeline.drawCoalesced')
  }
  if (rafId !== null) return
  rafId = requestAnimationFrame(() => {
    rafId = null
    drawMain()
  })
}
function drawMain() {
  const cv = canvasRef.value
  if (!cv || trackW.value <= 0 || trackH.value <= 0) return
  const monitored = performanceRecorder.isActive()
  const startedAt = monitored ? performance.now() : 0
  // 注:调色板不在此重读——挂载 + 主题换代(watch props.themePalette)时已缓存,
  const ctx = fitCanvas(cv, trackW.value, trackH.value)
  if (ctx) {
    drawAxis(ctx, trackW.value, trackH.value)
    if (monitored) performanceRecorder.recordSpan('timeline.draw', performance.now() - startedAt)
  }
}

// ── 放大镜(方案乙:光标最近 K 项富标签列表,canvas 绘制)──────────────────────
// 取光标最近 K 个分隔符,逐行画密度底条+标签+数量(中心行高亮)。canvas 文本(fillText)每帧 K×2 次
// 绘制,无 DOM diff。date 分组分隔符=每日(label 日期串、count 该日项数),folder=每文件夹。
const LOUPE_W = 156
const LOUPE_ROW_H = 20
const LOUPE_K = 9
const LOUPE_PAD = 8 // 左右内边距(与 DOM 版 .tl-loupe-row padding 0 8px 对齐)
const LOUPE_COUNT_RESERVE = 38 // 右侧「数量 + 间隙」预留宽,标签超此宽度截断加「…」

/**
 * 单行标签超宽截断加「…」(对齐 DOM 版 text-overflow: ellipsis;canvas 无自动省略号需自算)。
 * 二分找放得下「前缀+…」的最长前缀。调用方须先 set ctx.font(measureText 依赖当前字体)。
 */
function fitLabel(ctx: CanvasRenderingContext2D, text: string, maxW: number): string {
  if (ctx.measureText(text).width <= maxW) return text
  const ell = '…'
  let lo = 0
  let hi = text.length
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1
    if (ctx.measureText(text.slice(0, mid) + ell).width <= maxW) lo = mid
    else hi = mid - 1
  }
  return text.slice(0, lo) + ell
}

function drawLoupe() {
  const cv = loupeRef.value
  const seps = props.separators
  if (!cv || seps.length === 0) return
  // 注:调色板/字体已缓存(挂载 + 主题换代),此处不重读——放大镜是飞掠热路径,每帧省一次强制样式重算。
  const win = nearestSeparatorWindow(seps, loupeLogicalY / trackTotal.value, props.totalHeight, LOUPE_K)
  const rows = win.end - win.start
  if (rows <= 0) return
  const H = rows * LOUPE_ROW_H
  const ctx = fitCanvas(cv, LOUPE_W, H)
  if (!ctx) return
  const monitored = performanceRecorder.isActive()
  const startedAt = monitored ? performance.now() : 0
  ctx.clearRect(0, 0, LOUPE_W, H)
  let maxC = 1
  for (let i = win.start; i < win.end; i++) maxC = Math.max(maxC, seps[i].count)
  const maxLabelW = LOUPE_W - LOUPE_PAD - LOUPE_COUNT_RESERVE
  ctx.textBaseline = 'middle'
  for (let r = 0; r < rows; r++) {
    const i = win.start + r
    const s = seps[i]
    const y = r * LOUPE_ROW_H
    const cy = y + LOUPE_ROW_H / 2
    const isCenter = i === win.center
    if (isCenter) {
      ctx.fillStyle = palette.hover
      ctx.fillRect(0, y, LOUPE_W, LOUPE_ROW_H)
    }
    // 密度底条(窗口内相对 count 从左充填)。
    ctx.fillStyle = palette.accent
    ctx.globalAlpha = 0.12
    ctx.fillRect(0, y, (s.count / maxC) * LOUPE_W, LOUPE_ROW_H)
    ctx.globalAlpha = 1
    // 数量(右对齐)。
    ctx.font = fontFor(10)
    ctx.textAlign = 'right'
    ctx.fillStyle = isCenter ? palette.text2 : palette.text3
    ctx.fillText(String(s.count), LOUPE_W - LOUPE_PAD, cy)
    // 标签(左对齐,超宽截断加「…」)。
    ctx.font = fontFor(10, isCenter)
    ctx.fillStyle = isCenter ? palette.text1 : palette.text2
    ctx.textAlign = 'left'
    ctx.fillText(fitLabel(ctx, s.label, maxLabelW), LOUPE_PAD, cy)
  }
  if (monitored) performanceRecorder.recordSpan('timeline.loupe', performance.now() - startedAt)
}

// ── 指针交互(与 DOM 版 jumpToPointer 同映射)────────────────────────────────
// 统一入口:轨道纵向比例 frac → logical y。time 坐标唯一改动点 —— 走 rowJumpY(日历行→真实日 y,
// 空日插值),否则线性 frac×totalHeight;下游 hover/loupe/jump 只此一处分叉。
function fracToLogicalY(frac: number): number {
  const tb = timeBand.value
  if (tb && tb.rowJumpY.length > 0) {
    const rows = tb.rowJumpY.length
    const row = Math.min(rows - 1, Math.max(0, Math.floor(frac * rows)))
    return tb.rowJumpY[row]
  }
  return frac * props.totalHeight
}
function updateHover(frac: number) {
  if (hasMonths.value) {
    hoverIndex.value = findActiveMonthIndex(props.monthBuckets, fracToLogicalY(frac))
  } else {
    const idx = nearestSeparatorIndex(props.separators, frac, props.totalHeight)
    sepHoverIndex.value = idx >= 0 ? idx : null
  }
}
// 轨道单击/拖拽跳转:平滑动画(指针「以前的样子」);folder 模式吸附最近分组边界。连续无吸附的
// 跟手拖动改由半透明视窗承担(见下),不再走此吸附路径。time 坐标走 rowJumpY(该日历行的真实日 y)。
function jumpToFrac(frac: number) {
  if (timeBand.value) {
    emit('jump', fracToLogicalY(frac))
  } else if (hasMonths.value) {
    emit('jump', frac * props.totalHeight)
  } else {
    const idx = nearestSeparatorIndex(props.separators, frac, props.totalHeight)
    emit('jump', idx >= 0 ? props.separators[idx].y : frac * props.totalHeight)
  }
}
function applyLoupe(frac: number, topPx: number) {
  loupeTop.value = topPx // 轨道内像素(CSS top + translateY(-50%) 居中于光标)
  loupeLogicalY = fracToLogicalY(frac) // time 坐标下 = 该日历行的真实日 y → 放大镜中心正确落在该时段
  loupeVisible.value = true
  cursorPx.value = topPx // 聚光中心 = 光标轨道内 y
  drawLoupe()
  scheduleDraw() // 轴重画让聚光跟随光标(scheduleDraw 去重到每帧一次)
}
// 放大镜可见时把浮层推到放大镜左侧,避免被 156px 宽的放大镜遮住文字(用户反馈)。
// 178 = 14(放大镜 margin-right)+ 156(LOUPE_W)+ 8(间隙);不可见时回落原 8px 贴轨。
const flyoutShift = computed(() => (loupeVisible.value ? '178px' : '8px'))

// pointermove rAF 节流(同 DOM 版):只暂存最新事件、每帧处理一个,rect 每帧只读一次;配合 fitCanvas
// 幂等,放大镜每帧只 clear+重画不重分配后备存储。
let moveRaf: number | null = null
let pendingMove: PointerEvent | null = null
function processMove(e: PointerEvent) {
  const el = trackRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const frac = Math.min(1, Math.max(0, (e.clientY - rect.top) / Math.max(1, rect.height)))
  // 拖拽中也更新 hover(对齐 DOM 版 applyJump,评审 R2):否则浮层文字/高亮全程钉在按下时
  // 那一月,与逐帧跟随的放大镜互相矛盾。
  updateHover(frac)
  if (dragging.value) jumpToFrac(frac)
  applyLoupe(frac, e.clientY - rect.top)
}
function cancelPendingMove() {
  if (moveRaf !== null) {
    cancelAnimationFrame(moveRaf)
    moveRaf = null
  }
  pendingMove = null
}

function onPointerDown(e: PointerEvent) {
  dragging.value = true
  emit('scrubbing', true)
  trackRef.value?.setPointerCapture(e.pointerId)
  const el = trackRef.value
  if (el) {
    const rect = el.getBoundingClientRect()
    const frac = Math.min(1, Math.max(0, (e.clientY - rect.top) / Math.max(1, rect.height)))
    updateHover(frac)
    jumpToFrac(frac)
    applyLoupe(frac, e.clientY - rect.top)
  }
}
function onPointerMove(e: PointerEvent) {
  pendingMove = e
  if (moveRaf !== null) return
  moveRaf = requestAnimationFrame(() => {
    moveRaf = null
    const ev = pendingMove
    pendingMove = null
    if (ev) processMove(ev)
  })
}
function onPointerUp(e: PointerEvent) {
  if (dragging.value) {
    dragging.value = false
    emit('scrubbing', false)
    trackRef.value?.releasePointerCapture(e.pointerId)
  }
}
function onTrackLeave() {
  if (!dragging.value) {
    cancelPendingMove()
    hoverIndex.value = null
    sepHoverIndex.value = null
    loupeVisible.value = false
    cursorPx.value = null // 关聚光;hover 置空的 watch 会触发轴重画
  }
}

// ── 键盘导航(与 DOM 版一致)──────────────────────────────────────────────
const stepCount = computed(() =>
  hasMonths.value ? props.monthBuckets.length : props.separators.length,
)
const currentIndex = computed(() => {
  if (hasMonths.value) return Math.max(0, findActiveMonthIndex(props.monthBuckets, props.currentY))
  if (props.separators.length === 0) return -1
  return nearestSeparatorIndex(props.separators, props.currentY / trackTotal.value, props.totalHeight)
})
const keyboardIndex = ref<number | null>(null)
const activeKbIndex = computed(() => keyboardIndex.value ?? currentIndex.value)
function stepTo(idx: number) {
  keyboardIndex.value = idx
  if (hasMonths.value) {
    hoverIndex.value = idx
    emit('jump', props.monthBuckets[idx].y)
  } else {
    sepHoverIndex.value = idx
    emit('jump', props.separators[idx].y)
  }
}
function onKeydown(e: KeyboardEvent) {
  const next = stepScrubberIndex(activeKbIndex.value, e.key, stepCount.value)
  if (next === null) return
  e.preventDefault()
  stepTo(next)
}
function onFocus() {
  if (keyboardIndex.value === null) keyboardIndex.value = Math.max(0, currentIndex.value)
}
function onBlur() {
  keyboardIndex.value = null
  if (!dragging.value) {
    hoverIndex.value = null
    sepHoverIndex.value = null
    cursorPx.value = null // 键盘导航不聚光(本就未设,防御性清)
  }
}

// ── 尺寸测量 + 重画触发 ─────────────────────────────────────────────────────
let trackRO: ResizeObserver | null = null
function measure() {
  const el = trackRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  trackW.value = rect.width
  trackH.value = rect.height
}
onMounted(() => {
  applyPalette() // 首帧前投影色值/字体(不再由 drawMain 每帧读)
  measure()
  scheduleDraw()
  if (typeof ResizeObserver !== 'undefined' && trackRef.value) {
    trackRO = new ResizeObserver(() => {
      measure()
      invalidateStaticAxis()
    })
    trackRO.observe(trackRef.value)
  }
})
// 主题/配色换代(store 每帧最多发布一次新色板引用):重投影色值 + 重画静态轴层与放大镜。
watch(
  () => props.themePalette,
  () => {
    applyPalette()
    invalidateStaticAxis()
    if (loupeVisible.value) drawLoupe()
  },
)
onBeforeUnmount(() => {
  trackRO?.disconnect()
  trackRO = null
  if (rafId !== null) cancelAnimationFrame(rafId)
  cancelViewportMove()
  cancelPendingMove() // 清 pointermove 节流的悬空 rAF
  // 拖拽中被卸载(切轴形态/关轴)不让宿主的 scrub 态卡在 true。
  if (dragging.value || vpDragging) emit('scrubbing', false)
  // 主动归零离屏 buffer，避免组件切回 DOM 后仍等待 GC 才释放 GPU/共享内存。
  if (staticAxisCanvas) {
    staticAxisCanvas.width = 0
    staticAxisCanvas.height = 0
    staticAxisCanvas = null
  }
})
// 数据变化(引用替换)→ 下一帧重画主轴;放大镜可见时一并重画(对齐 DOM 版 computed 的自动更新)。
watch(
  () => [props.monthBuckets, props.separators, props.totalHeight],
  () => nextTick(() => {
    invalidateStaticAxis()
    if (loupeVisible.value) drawLoupe()
  }),
)
watch(() => props.currentY, scheduleDraw)
// hover 项变化(光标所在月 / 最近分隔点)→ 重画主轴以更新悬停高亮(DOM 版靠 :hover,canvas 须主动重画)。
watch([hoverIndex, sepHoverIndex], scheduleDraw)
// 视觉形态变化(用户切换 / 恢复默认 / 外部改文件)→ 重画(rowIntensity 为 computed,数据/尺寸不变时
// 切模式只换绘制路径)。落盘由用户 setter 负责,此处只应用。
watch(visualMode, () => invalidateStaticAxis())
// 坐标变化 → 重画。effectiveCoord 变化(切坐标 / 切视觉致 coordApplies 变)→ band 数据源换,
// 一并重画放大镜(若可见)使其中心随新坐标重定位。同上:落盘由 setter 负责。
watch(effectiveCoord, () => {
  invalidateStaticAxis()
  if (loupeVisible.value) drawLoupe()
})

// 内嵌钮已迁底部状态栏簇(MediaGrid Teleport),父组件经 template ref 驱动这两个方法/读这两个 ref。
defineExpose({ cycleVisualMode, toggleCoordMode, visualMode, coordMode, coordApplies, effectiveCoord })
</script>

<style scoped>
.tlc-scrubber {
  position: absolute;
  left: 0;
  right: 0;
  /* 满高(非内缩):与 MediaScrollbar(top:0/bottom:0)同坐标系,视窗才能与滚动条 thumb 两端对齐。 */
  top: 0;
  bottom: 0;
  cursor: pointer;
  touch-action: none;
}
.tlc-scrubber:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
  border-radius: var(--radius-sm);
}
.tlc-canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none; /* 事件交给轨道 div */
}

/* ── 半透明视窗(VSCode minimap 式拖动把手)──────────────────────────────────
   不透明度可设:与 DOM 版 .tl-viewport 同款 --axis-viewport-opacity 乘缩放(见 uiScale.ts)。 */
.tlc-viewport {
  position: absolute;
  left: 0;
  right: 0;
  background: color-mix(
    in srgb,
    var(--color-accent) calc(15% * var(--axis-viewport-opacity, 1)),
    transparent
  );
  border: 1px solid
    color-mix(in srgb, var(--color-accent) calc(45% * var(--axis-viewport-opacity, 1)), transparent);
  border-radius: 2px;
  cursor: grab;
  pointer-events: auto;
  z-index: 3; /* 压在 canvas 主轴之上、放大镜(4)/浮层之下 */
  transition: background var(--transition-fast);
}
.tlc-scrubber:hover .tlc-viewport {
  background: color-mix(
    in srgb,
    var(--color-accent) calc(24% * var(--axis-viewport-opacity, 1)),
    transparent
  );
}
.tlc-viewport:active {
  cursor: grabbing;
  background: color-mix(
    in srgb,
    var(--color-accent) calc(32% * var(--axis-viewport-opacity, 1)),
    transparent
  );
}

/* 年份标签:与 DOM 版一致,左浮进画廊区,背景色光晕保任意底色可读。 */
.tlc-year {
  position: absolute;
  right: 100%;
  transform: translateY(-50%);
  margin-right: 4px;
  font-size: 9px;
  line-height: 1;
  color: var(--color-text-secondary);
  white-space: nowrap;
  pointer-events: none;
  opacity: 0.9;
  text-shadow:
    0 0 3px var(--color-bg-primary),
    0 0 2px var(--color-bg-primary),
    0 0 1px var(--color-bg-primary);
}

.tlc-flyout {
  position: absolute;
  right: 100%;
  margin-right: 8px;
  transform: translateY(-50%);
  padding: var(--spacing-2xs) var(--spacing-sm);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-xs);
  line-height: 1.4;
  color: var(--color-text-primary);
  white-space: nowrap;
  pointer-events: none;
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  z-index: 3;
}
.tlc-flyout__count {
  color: var(--color-text-secondary);
}

/* 放大镜:浮在轨道左侧、跟随光标垂直居中,不吃指针事件(磁化预览,不抢焦)。 */
.tlc-loupe {
  position: absolute;
  right: 100%;
  margin-right: 14px;
  transform: translateY(-50%);
  pointer-events: none;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  z-index: 4;
}
</style>
