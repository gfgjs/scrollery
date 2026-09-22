<template>
  <!-- Canvas 网格(§9 T4「转正 v2」:零逐格 DOM + 单元素算术命中)。结构(均在 .media-grid
       滚动容器内):wrap(高占位=逻辑总高,撑原生滚动条) └─ canvas(sticky 满视口,唯一元素;
       滚动时按 currentY 重绘)。交互全在 canvas 一个元素:点击/右键/pointerdown → hitTestCell
       两级二分命中(O(log n));框选经注入 setPointerIdResolver 用同一算术命中,脱离逐格 DOM。
       ⚠️ v1 的「DOM 镜像层」已被推翻(每格一个 div 重新引入数千节点,真机实测更卡);
       逐格键盘焦点降级为 DOM 模式专属。 -->
  <div ref="wrapRef" class="mgc-wrap" :style="{ height: spacerHeight + 'px' }">
    <!-- 悬停卡层(T12):与 canvas 同 sticky 钉视口、高 0 不占流,内部绝对定位与 canvas 绘制
         共用同一视口坐标系。⚠️ 这是**单例** DOM(同一时刻至多一张卡),不是被推翻的 v1 逐格
         镜像层——O(1) 节点不随库规模增长。复用 MediaThumb 整卡,收藏/星级/checkbox/悬停
         预览/信息浮窗与 DOM 模式逐字节同源;极小格按放大镜放大到最小可用尺寸(用户裁决)。 -->
    <div ref="hoverLayerRef" class="mgc-hover-layer">
      <div
        v-if="hoverCard"
        :key="hoverCard.item.id"
        class="mgc-hover-card"
        :class="{
          'mgc-hover-card--static': isSelectionMode,
          'mgc-hover-card--bare': hoverCardBare,
          'mgc-hover-card--zoom': hoverCard.rect.scale0 !== 1,
          'mgc-hover-card--mirror': hoverCardMirror,
        }"
        :style="{
          left: hoverCard.rect.x + 'px',
          top: hoverCard.rect.y + 'px',
          width: hoverCard.rect.w + 'px',
          height: hoverCard.rect.h + 'px',
          '--mgc-scale0': hoverCard.rect.scale0,
          '--mgc-origin-x': hoverCard.rect.originX + 'px',
          '--mgc-origin-y': hoverCard.rect.originY + 'px',
        }"
        @pointermove="onHoverCardPointerMove"
        @pointerleave="onHoverCardPointerLeave"
        @click="onHoverCardClick"
        @contextmenu.prevent="onHoverCardContextMenu"
        @pointerdown="onHoverCardPointerDown"
      >
        <!-- 悬停卡的生成由 Canvas 可见需求统一管理；卡片卸载不能取消仍可见的同项请求。
             §8.1 browse-only 透传 MediaThumb,镜头态隐藏收藏/评分快捷动作与拖拽手柄。 -->
        <MediaThumb
          :id="hoverCard.item.id"
          :item="hoverCard.item"
          :w="hoverCard.rect.w"
          :h="hoverCard.rect.h"
          :media-type="hoverCard.item.mediaType"
          :is-live-photo="hoverCard.item.isLivePhoto"
          :duration-ms="hoverCard.item.durationMs"
          :thumb-status="hoverCard.item.thumbStatus"
          :thumb-path="hoverCard.item.thumbPath"
          :placeholder-color="hoverCard.item.placeholderColor"
          :file-format="hoverCard.item.fileFormat"
          :file-size="hoverCard.item.fileSize"
          :similarity="hoverCard.item.similarity"
          :is-favorited="hoverCard.item.isFavorited"
          :rating="hoverCard.item.rating"
          :color-label="hoverCard.item.colorLabel"
          :is-selected="hoverSelected"
          :is-selection-mode="isSelectionMode"
          :browse-only="lensActive"
          :force-full="!isSelectionMode"
          :cache-dir="cacheDir"
          @request-thumb="scheduleDraw"
          @regenerate-thumb="(id: number) => emit('regenerate-thumb', id)"
          @favorite="(id: number) => emit('cell-favorite', id)"
          @rate="(id: number, value: number) => emit('cell-rate', id, value)"
          @select="(id: number) => emit('cell-select', id)"
        />
      </div>
    </div>
    <canvas
      :key="glassBackground ? 'glass' : 'opaque'"
      ref="canvasRef"
      class="mgc-canvas"

      tabindex="0"

      @click="onClick"
      @contextmenu.prevent="onContextMenu"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerleave="onCanvasPointerLeave"
    ></canvas>
  </div>
</template>

<script setup lang="ts">
import {
  computed,
  toRaw,
  ref,
  watch,
  nextTick,
  onMounted,
  onBeforeUnmount,
  onActivated,
  onDeactivated,
} from 'vue'
import MediaThumb from './MediaThumb.vue'
import { setPointerIdResolver } from '../../composables/useSelection'
import { isThumbLoadDeferred, useThumbLoadGate } from '../../composables/useThumbLoadGate'
import { useCanvasThumbPipeline } from '../../composables/useCanvasThumbPipeline'
import { useCanvasHitTest } from '../../composables/useCanvasHitTest'
import { useCanvasHoverCard } from '../../composables/useCanvasHoverCard'
import {
  visibleRowRange,
  CanvasRenderLifecycle,
  CanvasRafScheduler,
  SelectionAnimTracker,
  SELECT_ANIM_MS,
} from './mediaGridCanvas.helpers'
import { PALETTE_METRICS_FALLBACK, projectPalette, readPaletteMetrics, type Palette } from './mediaGridCanvas.palette'
import {
  drawSeparator,
  type LensFolderHeaderLines,
} from './mediaGridCanvas.painters'
import { createInfoOverlayCache } from './mediaGridCanvas.infoOverlay'
import { createCellRenderer } from './mediaGridCanvas.cellRenderer'
import type { LayoutRow, LayoutRowItem, LayoutRowSeparator, MediaMeta } from '../../types/layout'
import type { ThemePalette } from '../../themes/types'
import type { DemoInfoFormatter } from '../../utils/demoAlias'
import { performanceRecorder } from '../../perf/performanceRecorder'
import { createCanvasThumbCellMetrics } from '../../perf/canvasThumbCellMetrics'

const props = defineProps<{
  /** 当前挂载窗口的布局行(双引擎同源,由宿主 activeRows 提供)。 */
  rows: LayoutRow[]
  /** 逻辑滚动位(视口顶部对应的逻辑 y)。 */
  currentY: number
  /** 逻辑总高(撑起 wrap 高度 → 原生滚动条)。 */
  spacerHeight: number
  /** 缩略图缓存目录(已规整为正斜杠)。 */
  cacheDir: string
  /** 返回生成/解析请求的完成信号，供 Canvas 释放有界请求额度。 */
  requestThumb: (id: number) => Promise<void>
  /** 是否极密(<100px):与 DOM 一致按此 gate 非必要 badges(播放/时长)。 */
  compactCells: boolean
  /** 选中谓词(单格粒度)。 */
  isSelected: (id: number) => boolean
  /** 暂存删除谓词(置灰 + 角标)。 */
  isPendingDelete: (id: number) => boolean
  /** 暂存删除角标文案。 */
  pendingDeleteLabel: string
  /** 可用态角标文案(missing/offline),i18n 由宿主注入(canvas 无 $t)。 */
  availMissingLabel: string
  availOfflineLabel: string
  /** 选区版本号:任何选区变化时递增,驱动重绘(canvas 无逐格响应式)。 */
  selectionVersion: number
  /** 是否处于选择模式:所有格常显 checkbox(对齐 DOM 的 --selection-mode),悬停卡抑制放大。 */
  isSelectionMode: boolean
  /** 宿主滚动中标志:抑制悬停跟踪(对齐 DOM 的 .is-scrolling pointer-events:none)。 */
  scrolling: boolean
  /** 缩略图悬停放大开关(config.enableHoverScale):Canvas 不经过 DOM hover CSS,需显式下发。 */
  enableHoverScale: boolean
  /** 信息浮窗开关(ui.showThumbInfo 经宿主注入,canvas 不进 store)。 */
  showThumbInfo: boolean
  /** 演示打码开关(2026-09-16):开启时缩略图位图强模糊、信息浮窗换示例文案、悬停卡完全停用。 */
  demoPrivacy: boolean
  /** 信息浮窗演示文案组装(宿主注入;关闭时为 null → 走真实文案)。 */
  demoInfoText?: DemoInfoFormatter | null
  /** 路径类分隔行(画廊分组头)的演示别名:返回 null 的行(日期/组头)原样绘制。 */
  demoSeparatorLabel?: (row: LayoutRowSeparator) => string | null
  /** 选择态拖拽手柄开关(#5,ui.showDragHandle 经宿主注入):关闭时不绘且不可命中。 */
  showDragHandle: boolean
  /** 就地 patch 信号(#15):宿主乐观更新(收藏/评分/色标/缩略图回写)后递增。canvas 据此
   *  重绘,替代原 rows 深 watch——深 watch 在 bucket 段边界帧对整个挂载窗口做依赖遍历+
   *  响应式代理物化(密行高下数千 item),正是「按住方向键每隔一段距离卡一下」的主因。 */
  patchTick: number
  /** 信息浮窗勾选元素(ui.thumbInfoElements)。 */
  thumbInfoElements: readonly string[]
  /** 可视区懒加载重型元数据(fileName/EXIF…):每批到达整体换 Map 引用,watch 驱动重绘。 */
  viewportMeta: Map<number, MediaMeta>
  /** 分组方式(date/folder/none):决定分隔行图标与 folder 分组的行内 sticky 钳位。 */
  groupBy: string
  /** LayoutSummary 提供的分组计数;Canvas 行载荷保持精简,按 groupId/label 查读。 */
  separatorCounts?: ReadonlyMap<string, number>
  /** 重复镜头激活(duplicateLensStore.mode 非空):组内位次角标 M/N 绘制开关(§6.2) +
   *  browse-only 快捷动作隐藏(§8.1,经悬停卡 MediaThumb 的 browse-only 透传)。 */
  lensActive?: boolean
  /** 镜头卡片徽标文本(§6.2/§7.3):宿主经 lensSeparator.resolveLensCardBadge + t 组装;
   *  groups= M/N、folders=「组 N」/问号兜底文本;null = 无徽标。与 DOM 徽标同源同信息位。 */
  lensCardBadgeText?: (item: LayoutRowItem) => string | null
  /** 重复组头文案(§6.1):宿主经结构化数值 + i18n 组装。 */
  lensGroupLabel?: (row: LayoutRowSeparator) => string
  /** 文件夹头行文字组(§7.2):宿主经 lensSeparator.getLensFolderStats/formatLensFolderStats
   *  组装(cluster 仅簇首);仅 duplicateFolder 分隔行被调用,其余行返回 null。 */
  lensFolderHeaderLines?: (row: LayoutRowSeparator) => LensFolderHeaderLines | null
  /** 当前主题色板(themeStore.currentPalette):DOM 与 Canvas 消费同一份生成结果,
   *  换代时只重投影调色板并重绘——几何、预取计划、缩略图请求都不动。 */
  themePalette: ThemePalette

  /** Windows 原生玻璃开启时，画布底面透明以透出 DWM Mica/Acrylic；其余平台保持不透明快路径。 */
  glassBackground: boolean
}>()

const emit = defineEmits<{
  'cell-click': [item: LayoutRowItem, event: MouseEvent]
  'cell-contextmenu': [item: LayoutRowItem, event: MouseEvent]
  // onHandle:指针是否落在已选中格的左上拖拽手柄上(Canvas 几何命中);宿主 onCardPointerDown
  // 据此分流「拖到文件夹 vs 反转扫选」——DOM 网格不传此参、走 e.target 兜底判定。
  'cell-pointerdown': [id: number, event: PointerEvent, onHandle: boolean]
  'cancel-thumb': [id: number]
  'regenerate-thumb': [id: number]
  /** 悬停卡交互(T13):收藏/评分/checkbox 上抛宿主(handleFavorite/handleRate/toggleSelect)。 */
  'cell-favorite': [id: number]
  'cell-rate': [id: number, value: number]
  'cell-select': [id: number]
}>()

const wrapRef = ref<HTMLElement | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const renderLifecycle = new CanvasRenderLifecycle()
const drawScheduler = new CanvasRafScheduler({
  snapshot: () => renderLifecycle.snapshot(),
  canDraw: (generation) => renderLifecycle.canDraw(generation),
  requestFrame: (callback) => requestAnimationFrame(callback),
  cancelFrame: (handle) => cancelAnimationFrame(handle),
  draw,
})

// ── 调色板(挂载/换尺寸/主题换代时投影一次并缓存;canvas 需具体色值,不能用 CSS 变量)──
// 颜色只从 props.themePalette 投影(与 DOM 同源):这里不读 DOM 取色,也不留第二套默认色;
// 只有尺寸/字体度量从 DOM 读一次。之后每帧引用同一对象引用。paletteOf() 只兜住「挂载测量前的
// 理论调用点」——onMounted 已先 applyPalette(),正常首帧即已就绪。
let palette: Palette | null = null
function paletteOf(): Palette {
  palette ??= projectPalette(props.themePalette, PALETTE_METRICS_FALLBACK)
  return palette
}
function applyPalette() {
  const el = wrapRef.value
  if (!el) return
  palette = projectPalette(props.themePalette, readPaletteMetrics(el))
}

// ── 尺寸(视口)与 DPR 适配 ─────────────────────────────────────────────────
let viewW = 0
let viewH = 0
// 画布设备像素比(fitCanvas 每次写入):演示打码的模糊半径按设备像素给定,高 DPR 下观感一致。
let canvasDpr = 1
const RESIZE_SETTLE_MS = 120
let resizeSettleTimer: ReturnType<typeof setTimeout> | null = null
let resizeSettling = false
function measure() {
  const wrap = wrapRef.value
  if (!wrap) return
  const scroller = wrap.parentElement // = .media-grid(滚动容器)
  viewW = wrap.clientWidth // 内容区宽(wrap 为内容宽块级子,与 item.x 同坐标系)
  viewH = scroller ? scroller.clientHeight : 0
}
function applyCanvasCssSize() {
  const cv = canvasRef.value
  if (!cv || viewW <= 0 || viewH <= 0) return
  // resize 拖动过程中只改 CSS 尺寸，让浏览器缩放最后一帧；backing store 在停稳后一次性重建。
  cv.style.width = `${viewW}px`
  cv.style.height = `${viewH}px`
}
function fitCanvas(): CanvasRenderingContext2D | null {
  const cv = canvasRef.value
  if (!cv) return null
  // KeepAlive 失活时视口可能暂时变成 0×0；写入 canvas.width/height 会清空 backing store，
  // 返回画廊前应保留最后一帧，而不是强行写成 1×1。
  if (viewW <= 0 || viewH <= 0) return null
  // 普通模式显式不透明，维持原有 compositor 快路径；玻璃模式才启用 alpha，令照片间隙
  // 透出窗口的 DWM Mica/Acrylic。alpha 是 context 创建参数，模板 key 会在模式切换时重建 canvas。
  const ctx = cv.getContext('2d', { alpha: props.glassBackground })
  if (!ctx) return null
  const dpr = window.devicePixelRatio || 1
  canvasDpr = dpr
  const w = Math.max(1, Math.round(viewW * dpr))
  const h = Math.max(1, Math.round(viewH * dpr))
  applyCanvasCssSize()
  if (cv.width !== w || cv.height !== h) {
    cv.width = w
    cv.height = h
  }
  // 清屏在设备像素空间做:dpr 非整数时 round(viewW×dpr) 可比逻辑 viewW 多出一列设备像素,
  // 逻辑坐标系 fillRect 会漏出黑边,故先复位 transform 铺满整个 buffer。玻璃路径 clear 后保留透明。
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  if (props.glassBackground) {
    ctx.clearRect(0, 0, w, h)
  } else {
    ctx.fillStyle = paletteOf().canvasGap
    ctx.fillRect(0, 0, w, h)
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.imageSmoothingEnabled = true
  ctx.imageSmoothingQuality = 'high' // 缩略图缩放贴近 <img> 观感(默认 low 会偏糊)
  return ctx
}

// ── 绘制 ───────────────────────────────────────────────────────────────────
function scheduleDraw() {
  if (resizeSettling) return
  const result = drawScheduler.schedule()
  if (result === 'skipped') return
  const monitored = performanceRecorder.isActive()
  if (monitored) {
    performanceRecorder.count('gallery.drawRequests')
    if (result === 'coalesced') performanceRecorder.count('gallery.drawCoalesced')
  }
}

/** 已二分收窄的可见项遍历：供选中动画快照 diff 用，避免再次扫描整个挂载段。 */
function* visibleItems(
  rows: readonly LayoutRow[],
  start: number,
  end: number,
): Generator<LayoutRowItem> {
  for (let i = start; i < end; i++) {
    const row = rows[i]
    if (row.rowType !== 'normal') continue
    yield* row.items
  }
}

// 选中态过渡追踪(T4):每帧 draw 开头 sync 快照——选区未变时 diff 为空仅刷新基线,
// 变化帧(selectionVersion watch 已先 scheduleDraw)对可见项起动画;动画中连续排帧。
const selAnim = new SelectionAnimTracker()

// ── 工厂调用(§3.1 红线:均在 setup 期一次性构建,热路径只调用产物,不得入 rAF 或逐格循环)──
// 图像缓存/加载/失败收口 + 视口外预取(idle+draw 尾双通道,原逻辑域 B/E 预取部分)。
const pipeline = useCanvasThumbPipeline({
  cacheDir: () => props.cacheDir,
  onRequestThumb: (id) => props.requestThumb(id),
  onCancelThumb: (id) => emit('cancel-thumb', id),
  onRegenerateThumb: (id) => emit('regenerate-thumb', id),
  scheduleDraw,
  viewportW: () => viewW,
  viewportH: () => viewH,
})

// 有限录制期指标(§7.3):首次入屏命中/未命中与「入屏→首次绘出位图」代理时延。非录制态
// beginFrame 返回 false、noteCell/endFrame 全 no-op,零维护成本与零分配。
const cellMetrics = createCanvasThumbCellMetrics()

// 算术命中(点击/右键/拖拽手柄,原逻辑域 I)。
const hitTest = useCanvasHitTest({
  canvasRef,
  rows: () => props.rows,
  currentY: () => props.currentY,
  isSelected: props.isSelected,
  showDragHandle: () => props.showDragHandle,
})
const { idAtClient, pick, hitHandleAt } = hitTest

// 悬停跟踪 + 单例悬停卡状态机(原逻辑域 J)。返回值名与 template 现有绑定逐一对应。
const {
  hoverLayerRef,
  hoverCard,
  hoverSelected,
  hoverCardBare,
  hoverCardMirror,
  mirrorCellId,
  clearHover,
  cancelHoverPrep,
  onPointerMove,
  onHoverCardPointerMove,
  onCanvasPointerLeave,
  onHoverCardPointerLeave,
  onHoverCardClick,
  onHoverCardContextMenu,
  onHoverCardPointerDown,
} = useCanvasHoverCard({
  canvasRef,
  scrolling: () => props.scrolling,
  isSelectionMode: () => props.isSelectionMode,
  isPendingDelete: props.isPendingDelete,
  isSelected: props.isSelected,
  enableHoverScale: () => props.enableHoverScale,
  demoPrivacy: () => props.demoPrivacy,
  cacheDir: () => props.cacheDir,
  currentY: () => props.currentY,
  selectionVersion: () => props.selectionVersion,
  viewport: () => ({ w: viewW, h: viewH }),
  hitTest: { pickWithRow: hitTest.pickWithRow, hitHandleAt: hitTest.hitHandleAt },
  requestRedraw: scheduleDraw,
  emit,
})

// 信息浮窗组装 + 逐帧缓存(原逻辑域 G)。
const infoOverlay = createInfoOverlayCache()

// drawCell 复合入口(原逻辑域 F 中 drawCell 本体)。
const drawCell = createCellRenderer({
  getPalette: paletteOf,
  selAnim,
  getImage: pipeline.getImage,
  isPendingDelete: props.isPendingDelete,
  isSelected: props.isSelected,
  compactCells: () => props.compactCells,
  showThumbInfo: () => props.showThumbInfo,
  thumbInfoElements: () => props.thumbInfoElements,
  demoPrivacy: () => props.demoPrivacy,
  devicePixelRatio: () => canvasDpr,
  demoInfo: () => (props.demoPrivacy ? props.demoInfoText ?? null : null),
  showDragHandle: () => props.showDragHandle,
  isSelectionMode: () => props.isSelectionMode,
  pendingDeleteLabel: () => props.pendingDeleteLabel,
  availMissingLabel: () => props.availMissingLabel,
  availOfflineLabel: () => props.availOfflineLabel,
  lensActive: () => props.lensActive ?? false,
  lensCardBadgeText: (item) => props.lensCardBadgeText?.(item) ?? null,
  viewportMeta: () => props.viewportMeta,
  mirrorCellId,
  drawInfoOverlay: infoOverlay.drawInfoOverlay,
})

// ── canvas 元素交互(点击/右键/pointerdown,原逻辑域 I 的入口函数)────────────────────
function onClick(e: MouseEvent) {
  const item = pick(e)
  if (item) emit('cell-click', item, e)
}
function onContextMenu(e: MouseEvent) {
  const item = pick(e)
  if (item) emit('cell-contextmenu', item, e)
}
function onPointerDown(e: PointerEvent) {
  // pointerdown → 命中项 → 交宿主 onCardPointerDown(分流框选/拖到文件夹)。框选的后续 pointermove
  // 经 setPointerIdResolver 注入的 idAtClient 解析当前悬停项(无逐格 DOM);拖拽的文件夹落点用侧栏
  // 真实 DOM 的 elementFromPoint(不受影响)。
  const item = pick(e)
  if (item) emit('cell-pointerdown', item.id, e, hitHandleAt(e))
}

// 绘制读取原对象以减少代理开销；就地更新仍由 patchTick 通知重绘。
// 命中与悬停继续读取 props.rows 的代理，保留悬停卡的响应式更新。
const drawRows = computed(() => props.rows.map((row) => toRaw(row)))

function draw() {
  if (!renderLifecycle.isActive || resizeSettling) return
  const monitored = performanceRecorder.isActive()
  const startedAt = monitored ? performance.now() : 0
  const ctx = fitCanvas()
  if (!ctx || viewW <= 0 || viewH <= 0) return
  // 清屏已由 fitCanvas 完成：普通模式铺 canvasGap，玻璃模式清为透明以露出原生背板。
  const now = performance.now()
  const y0 = props.currentY
  const rows = drawRows.value
  const range = visibleRowRange(rows, y0, y0 + viewH)
  pipeline.prioritizeVisibleThumbLoads(rows, range.start, range.end)
  const metricsOn = cellMetrics.beginFrame(now)
  selAnim.sync(visibleItems(rows, range.start, range.end), props.isSelected, now)
  const firstVisible = range.start < range.end ? range.start : -1
  const lastVisible = range.start < range.end ? range.end - 1 : -1
  let visibleCells = 0
  let coldCells = 0
  // 绘制顺序恒为布局顺序，避免把“由上到下替换”机械改成“随滚动方向替换”却不改善吞吐。
  for (let i = range.start; i < range.end; i++) {
    const row = rows[i]
    const sy = row.y - y0
    if (row.rowType === 'separator') {
      const separatorKey = row.groupId ?? row.separatorLabel
      drawSeparator(
        ctx,
        row,
        sy,
        paletteOf(),
        props.groupBy,
        props.separatorCounts?.get(separatorKey),
        props.lensFolderHeaderLines?.(row) ?? null,
        props.lensGroupLabel?.(row) ?? null,
        props.demoSeparatorLabel?.(row) ?? null,
      )
      continue
    }
    visibleCells += row.items.length
    for (const item of row.items) {
      const drewImage = drawCell(ctx, item, sy, now)
      // 冷格 = 本帧未画出任何位图(现行/stale 均算已出图)且未判失败的媒体格——即用户
      // 看到的「占位等待真图」;逐帧累计为 coldCellFrames(格·帧积分),量化换图波强度。
      if (monitored) {
        // DB已判失败的格保持固定占位,不属于等待出图;管线失败表只覆盖本次加载失败。
        const failed = !drewImage && (item.thumbStatus === 2 || pipeline.thumbState.isFailed(item.id))
        if (!drewImage && !failed) coldCells++
        if (metricsOn) cellMetrics.noteCell(item.id, drewImage, failed)
      }
    }
  }
  if (metricsOn) cellMetrics.endFrame()
  // 视口外预取:开闸态全窗(前 1.25 屏+后 0.5 屏),关闸态收缩为仅前向(稍快速滚动
  // 的换图波根治,2026-07-17 阶段 6)。计划登记后先跑 draw 尾同步小分片保底推进
  // (idle 空隙真机不可靠),深填仍交 idle 分片;可见格已在上方绘制循环中先占加载槽。
  if (firstVisible !== -1) {
    pipeline.ensurePrefetchPlan(rows, y0, firstVisible, lastVisible, visibleCells, isThumbLoadDeferred())
    pipeline.runDrawPrefetchSlice()
  } else pipeline.cancelPrefetchPlan()
  if (selAnim.hasActive(now, SELECT_ANIM_MS)) scheduleDraw()
  if (monitored) {
    performanceRecorder.setGauge('gallery.visibleCells', visibleCells)
    performanceRecorder.setGauge('gallery.visibleColdCells', coldCells)
    performanceRecorder.count('gallery.coldCellFrames', coldCells)
    performanceRecorder.setGauge('gallery.cacheItems', pipeline.thumbState.size())
    performanceRecorder.setGauge('gallery.cacheBytes', pipeline.thumbState.bytes())
    performanceRecorder.setGauge('gallery.inFlightThumbs', pipeline.thumbState.loadingCount())
    performanceRecorder.setGauge('gallery.prefetchedCells', pipeline.getPrefetchProgress())
    performanceRecorder.recordSpan('gallery.draw', performance.now() - startedAt)
  }
}

// ── 生命周期 + 重绘触发源 ───────────────────────────────────────────────────
let ro: ResizeObserver | null = null
function scheduleResizeSettle() {
  resizeSettling = true
  drawScheduler.cancel()
  measure()
  applyCanvasCssSize()
  pipeline.cancelPrefetchPlan()
  if (resizeSettleTimer !== null) clearTimeout(resizeSettleTimer)
  resizeSettleTimer = setTimeout(() => {
    resizeSettleTimer = null
    resizeSettling = false
    if (!renderLifecycle.isActive) return
    measure()
    scheduleDraw()
  }, RESIZE_SETTLE_MS)
}
function installResizeObserver(): void {
  const scroller = wrapRef.value?.parentElement
  if (typeof ResizeObserver === 'undefined' || !scroller || !renderLifecycle.isActive) return
  ro?.disconnect()
  const generation = renderLifecycle.snapshot()
  ro = new ResizeObserver(() => {
    if (!renderLifecycle.canDraw(generation)) return
    scheduleResizeSettle()
  })
  ro.observe(scroller)
}

function stopCanvasWork(): void {
  renderLifecycle.deactivate()
  resizeSettling = false
  ro?.disconnect()
  ro = null
  if (resizeSettleTimer !== null) {
    clearTimeout(resizeSettleTimer)
    resizeSettleTimer = null
  }
  drawScheduler.cancel()
  pipeline.pause()
  // 失活(KeepAlive/卸载)后不再有 draw:把冷格/可见格 gauge 归零并重置入屏追踪,避免后台
  // 仍按最后一帧的冷格数继续积分,也避免返回画廊时把屏外停留算进「入屏→首绘」。
  performanceRecorder.setGauge('gallery.visibleColdCells', 0)
  performanceRecorder.setGauge('gallery.visibleCells', 0)
  cellMetrics.reset()
  cancelHoverPrep()
  clearHover()
  setPointerIdResolver(null)
}

onMounted(() => {
  applyPalette()
  measure()
  scheduleDraw()
  setPointerIdResolver(idAtClient) // 框选脱离逐格 DOM(见 useSelection)
  installResizeObserver()
})
onActivated(() => {
  renderLifecycle.activate()
  applyPalette()
  measure()
  setPointerIdResolver(idAtClient)
  installResizeObserver()
  scheduleDraw()
})
onDeactivated(() => {
  stopCanvasWork()
})
onBeforeUnmount(() => {
  stopCanvasWork()
  pipeline.dispose() // 复刻原 cancelPrefetchPlan → cancelAbortableThumbLoads → thumbState.clear 顺序(§3.4)
})

// 滚动(currentY)、行集换代、选区变化 → 重绘;spacerHeight 变化经模板 style 绑定 + 重绘。
watch(
  () => props.currentY,
  (nextY, previousY) => {
    if (nextY > previousY) pipeline.setScrollDirection(1)
    else if (nextY < previousY) pipeline.setScrollDirection(-1)
    clearHover() // 内容从指针下滚走,旧命中已失效;停稳后下一次 pointermove 重建
    pipeline.cancelPrefetchPlan() // 旧视口 iterator 立即作废，未启动的 IO 不再追赶过期位置
    scheduleDraw()
  },
)
// 行集换代(段挂载/离窗/updateVisible,rows 引用变):预取计划作废 + 重绘。
// #15:原为 {deep:true} 以等价 DOM 深响应式承接就地 patch——但 bucket 段落地帧,深 watch
// 对整个挂载窗口(密行高下数百行/数千 item)做依赖遍历并物化响应式代理,同步开销集中在
// 边界那一帧 = 周期性卡顿主因。改浅 watch;就地 patch 由宿主 bump patchTick 显式通知
// (下一个 watch),两条通路合并覆盖原深 watch 的全部触发面。
watch(
  () => props.rows,
  () => {
    pipeline.cancelPrefetchPlan()
    scheduleDraw()
  },
)
// 宿主乐观 patch(收藏/评分/色标/缩略图回写):几何不变,仅重绘、不作废预取计划。
watch(() => props.patchTick, scheduleDraw)
watch(() => props.selectionVersion, scheduleDraw)
// 进出选择模式:重绘(checkbox 常显切换)并清悬停卡——卡的几何按旧模式算(放大↔原位),
// 留着会以错误尺寸挡在格上,清掉等下一次 pointermove 按新模式重建。
watch(
  () => props.isSelectionMode,
  () => {
    clearHover()
    scheduleDraw()
  },
)
// 悬停放大开关由设置页实时切换:清掉按旧倍率创建的单例悬停卡,避免旧卡继续覆盖画面。
watch(() => props.enableHoverScale, clearHover)
// 信息浮窗数据/设置换代:缓存整体作废(gen 递增)并重绘——viewportMeta 每批到达换 Map 引用,
// thumbInfoElements/showThumbInfo 由设置页改动。
watch(
  // 演示文案组装器随语言换代(别名前缀词是译文):换引用即作废缓存里的旧语言文本。
  [
    () => props.viewportMeta,
    () => props.thumbInfoElements,
    () => props.showThumbInfo,
    () => props.demoInfoText,
  ],
  () => {
    infoOverlay.invalidate()
    scheduleDraw()
  },
)
// 语言换代无需单独信号:demoInfoText 的新闭包会走过下面的 watch,那里已作废文案缓存并排帧;
// draw() 本身也会重画分隔头(其别名词同样是译文),一套信号覆盖两处。
watch(() => props.groupBy, scheduleDraw)
// 重复镜头进出(§6.2):组内位次角标显隐切换,重绘一次即可(几何不变)。
watch(() => props.lensActive, scheduleDraw)
// 手柄开关(#5)由设置页改动:无缓存可作废,重绘即生效。
watch(() => props.showDragHandle, scheduleDraw)
// 演示打码开关(2026-09-16):翻转即整块作废信息浮窗文案缓存(防残留真实文件名/路径),并清掉
// 悬停卡——卡内是真实缩略图与真实文案,打码期间必须立即消失;随后的重绘让位图模糊/还原同帧生效。
watch(
  () => props.demoPrivacy,
  () => {
    infoOverlay.invalidate()
    clearHover()
    scheduleDraw()
  },
)
watch(
  () => props.scrolling,
  (v) => {
    if (v) clearHover()
  },
)
watch(() => props.compactCells, scheduleDraw)
// Canvas 的 alpha 创建后不可改变；模板 key 已替换元素，等 DOM 更新后重测并重绘新 context。
watch(
  () => props.glassBackground,
  async () => {
    clearHover()
    await nextTick()
    measure()
    applyPalette()
    scheduleDraw()
  },
)
watch(
  () => props.spacerHeight,
  () => {
    measure()
    pipeline.cancelPrefetchPlan()
    scheduleDraw()
  },
)
// 主题/配色换代(store 每帧最多发布一次新色板引用):重新投影调色板并重绘。只重绘——
// 几何与预取计划不变,不触发缩略图重载(与旧 themeToken/tintToken/textToken 同一触发面)。
watch(
  () => props.themePalette,
  () => {
    applyPalette()
    scheduleDraw()
  },
)
// B:加载闸门翻转 → 重绘。draw 会按新闸门态重建预取计划(开闸=全窗恢复被推迟的加载;
// 关闸=收缩为仅前向 1 屏,不再整体停取——稍快速滚动也要持续出真图,阶段 6 裁决)。
watch(useThumbLoadGate(), () => scheduleDraw())
</script>

<style scoped src="./MediaGridCanvas.styles.css"></style>
