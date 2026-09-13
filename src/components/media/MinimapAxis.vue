<template>
  <!-- 无缝 minimap 轴(VSCode 式缩略预览):canvas 画内容微缩(placeholderColor 色块即时
       + 微缩略图异步回填),DOM 视口框覆盖层负责命中/拖拽(镜像 MediaScrollbar 的
       pointer capture 交互)。无 scrubber 数据时自动接管右轴；有数据时可与时间轴
       手动切换。几何全部来自 minimapAxis.helpers 纯函数(单测锁定)。 -->
  <div
    ref="trackRef"
    class="minimap-axis"

    tabindex="0"
    @pointerdown="onTrackPointerdown"
    @pointermove="onPointermove"
    @pointerup="endDrag"
    @pointercancel="endDrag"
    @wheel.prevent="onWheel"
    @keydown="onKeydown"
  >
    <canvas ref="canvasRef" class="minimap-axis__canvas" />
    <!-- 当前滚动位置指示线(与时间轴 tl-indicator 同款同映射):currentY/可滚动行程
         比例(顶/底恰贴两端),与画廊标尺线/滚动条同源对齐;刻意不用 minimap 内容窗
         坐标——深库窗口化后窗内坐标与画廊标尺线无法对齐,该比例语义才可跨轴一致。 -->
    <div v-if="showIndicator" class="minimap-axis__indicator" :style="{ top: `${indicatorPct}%` }" />
    <div
      v-if="slider"
      class="minimap-axis__viewport"
      :class="{ 'is-active': active || dragging }"
      :style="{ transform: `translateY(${slider.top}px)`, height: slider.height + 'px' }"
      @pointerdown.stop="onSliderPointerdown"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useMediaStore } from '../../stores/mediaStore'
import type { MinimapRenderMode } from '../../stores/uiStore'
import type { LayoutRow, LayoutRowItem } from '../../types/layout'
import { buildThumbUrl } from '../../composables/useThumbLoader'
import { useThumbLoadGate } from '../../composables/useThumbLoadGate'
import { createCanvasThumbState } from './canvasThumbState'
import {
  clickToLogicalY,
  keyboardToLogicalY,
  minimapScale,
  minimapSlider,
  minimapThumbRetryDelay,
  minimapThumbSig,
  minimapWindow,
  sliderTopToLogicalY,
  wheelToLogicalY,
  type MinimapWindow,
} from './minimapAxis.helpers'

const props = defineProps<{
  /** 逻辑总高(px)——与引擎 totalHeight 同源,非物理 spacer。 */
  totalHeight: number
  /** 当前逻辑滚动位(px)——双引擎统一的 currentLogicalY。 */
  currentY: number
  /** 画廊视口高(px)——gridRef 内容区高,视口框高度与点击居中的依据。 */
  viewportHeight: number
  /** 布局内容宽(px)——与 compute_layout 的 containerWidth 同源,缩放系数分母。 */
  containerWidth: number
  /** 宿主滚动进行中(isScrolling)→ 视口框高亮。 */
  active: boolean
  /** 缩略图缓存目录(buildThumbUrl 入参)。 */
  cacheDir: string
  /** 内容渲染模式:纯色块零图片 IO;缩略图异步回填。 */
  renderMode: MinimapRenderMode
}>()

const emit = defineEmits<{
  (e: 'jump', y: number): void
  /** 视口框/轨道拖动起止:宿主中转给 MediaScrollbar 展开标尺线。 */
  (e: 'scrubbing', on: boolean): void
}>()

const media = useMediaStore()
const loadGate = useThumbLoadGate()

const trackRef = ref<HTMLElement | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const trackW = ref(0)
const trackH = ref(0)
const dragging = ref(false)

const scale = computed(() => minimapScale(trackW.value, props.containerWidth))
const miniWin = computed<MinimapWindow | null>(() =>
  minimapWindow(props.currentY, props.totalHeight, props.viewportHeight, trackH.value, scale.value),
)
const slider = computed(() =>
  minimapSlider(props.currentY, props.totalHeight, props.viewportHeight, trackH.value, scale.value),
)
const maxScrollY = computed(() => Math.max(0, props.totalHeight - props.viewportHeight))
const clampedY = computed(() => Math.min(maxScrollY.value, Math.max(0, props.currentY)))
// 当前滚动位置指示线(与时间轴 tl-indicator 同式):currentY/可滚动行程 映射到轨道比例,
// 顶/底恰贴两端——分母取 totalHeight 时贴底会留一屏比例的空档(用户回报);不足一屏不显。
const indicatorPct = computed(() => (clampedY.value / Math.max(1, maxScrollY.value)) * 100)
const showIndicator = computed(() => maxScrollY.value > 0)

// ── 行数据分块缓存 ───────────────────────────────────────────────────────────
// 按固定逻辑高分块经 fetchRowsByY 拉取(视口相交语义,块界行会在相邻块重复出现,
// 绘制时按 row.y 去重)。layoutVersion 翻转整体作废;远离窗口的块随滚动修剪,
// 深库长途漫游不累积。
const CHUNK_PX = 8192
const MAX_CHUNKS = 48
// loading 用逐请求 Symbol 而非共享字符串:远块被修剪后迟到结果不得重新塞回缓存,
// 同块重请求后旧请求的 catch 也不得误删新 owner。
const chunkCache = new Map<number, LayoutRow[] | symbol>()

function ensureChunks(win: MinimapWindow): void {
  const i0 = Math.max(0, Math.floor(win.topY / CHUNK_PX))
  const i1 = Math.max(0, Math.floor(win.bottomY / CHUNK_PX))
  for (let i = i0; i <= i1; i++) {
    if (chunkCache.has(i)) continue
    const loadingToken = Symbol(`minimap-chunk-${i}`)
    chunkCache.set(i, loadingToken)
    const version = media.layoutVersion
    media
      .fetchRowsByY(i * CHUNK_PX, (i + 1) * CHUNK_PX)
      .then((rows) => {
        if (media.layoutVersion !== version) return // 布局已换代,结果作废
        if (chunkCache.get(i) !== loadingToken) return // 已修剪或由更新请求接管
        chunkCache.set(i, rows)
        schedulePaint()
      })
      .catch(() => {
        if (chunkCache.get(i) === loadingToken) chunkCache.delete(i)
      })
  }
  if (chunkCache.size > MAX_CHUNKS) {
    for (const k of chunkCache.keys()) {
      if (chunkCache.size <= MAX_CHUNKS) break
      if (k < i0 - 2 || k > i1 + 2) chunkCache.delete(k)
    }
  }
}

// ── 微缩略图缓存 ─────────────────────────────────────────────────────────────
// 复用 canvasThumbState 状态机(sig 对账/去重/LRU),但预算远小于主画廊:微图单条
// 约几 KB,8MB ≈ 数个整窗驻留。解码期直接缩到绘制尺寸(resizeWidth/Height),
// 内存与绘制成本同时封顶;scale 变化(拖窗宽)不重解码,drawImage 拉伸的轻微
// 模糊在微缩尺度不可辨。瞬时失败做两次有限退避；预算耗尽后仅标记不上抛自愈——
// 404/驱逐的修复责任仍在主画廊(useThumbLoader 懒自愈),故 failLoad 恒传 status=0
// 绕开 heal 判定，避免 minimap 触发批量重生成。
const MINIMAP_CACHE_ENTRIES = 2048
const MINIMAP_CACHE_BYTES = 8 * 1024 * 1024
const MAX_INFLIGHT = 8
// 模式切换代次进入请求 sig:缩略图→色块→缩略图时,切换前迟到的同路径请求也必须作废,
// 否则清缓存后旧请求会误命中新代次并与新请求重复落地。
let thumbEpoch = 0
let thumbAbortController = new AbortController()
const thumbState = createCanvasThumbState<ImageBitmap>(MINIMAP_CACHE_ENTRIES, (b) => b.close(), {
  maxBytes: MINIMAP_CACHE_BYTES,
  byteCost: (b) => b.width * b.height * 4,
})

interface ThumbRetry {
  sig: string
  failureCount: number
  retryAt: number
}

const MAX_RETRY_RECORDS = MINIMAP_CACHE_ENTRIES
const thumbRetries = new Map<number, ThumbRetry>()
const thumbLoadTokens = new Map<number, symbol>()
let retryTimer: ReturnType<typeof setTimeout> | null = null
let retryTimerAt = Number.POSITIVE_INFINITY

function scheduleRetryPaint(retryAt: number): void {
  if (retryTimer !== null && retryTimerAt <= retryAt) return
  if (retryTimer !== null) clearTimeout(retryTimer)
  retryTimerAt = retryAt
  retryTimer = setTimeout(
    () => {
      retryTimer = null
      retryTimerAt = Number.POSITIVE_INFINITY
      schedulePaint()
    },
    Math.max(0, retryAt - Date.now()),
  )
}

function clearThumbRetries(): void {
  if (retryTimer !== null) clearTimeout(retryTimer)
  retryTimer = null
  retryTimerAt = Number.POSITIVE_INFINITY
  thumbRetries.clear()
}

function startThumbLoad(item: LayoutRowItem, sig: string, drawW: number, drawH: number): void {
  if (thumbState.isLoading(item.id) || thumbState.isFailed(item.id)) return
  if (thumbState.loadingCount() >= MAX_INFLIGHT) return
  const retry = thumbRetries.get(item.id)
  if (retry && retry.sig !== sig) thumbRetries.delete(item.id)
  else if (retry && retry.retryAt > Date.now()) {
    scheduleRetryPaint(retry.retryAt)
    return
  }
  const url = buildThumbUrl(item.thumbStatus, item.thumbPath, props.cacheDir)
  if (!url) return // status 0/2:留色块,生成推进由主画廊负责
  thumbState.markLoading(item.id)
  const loadToken = Symbol(`minimap-thumb-${item.id}`)
  thumbLoadTokens.set(item.id, loadToken)
  const controller = thumbAbortController
  void (async () => {
    try {
      const resp = await fetch(url, { signal: controller.signal })
      if (!resp.ok) throw new Error(`http ${resp.status}`)
      const blob = await resp.blob()
      const bmp = await createImageBitmap(blob, {
        resizeWidth: Math.max(1, Math.round(drawW)),
        resizeHeight: Math.max(1, Math.round(drawH)),
        resizeQuality: 'low',
      })
      // 同 id 新代次请求已接管时，旧 decode 结果只能释放，不能清理新请求的 loading 账。
      if (thumbLoadTokens.get(item.id) !== loadToken) {
        bmp.close()
        return
      }
      thumbLoadTokens.delete(item.id)
      thumbRetries.delete(item.id)
      if (thumbState.commitLoad(item.id, sig, { src: bmp, renderSig: sig })) schedulePaint()
    } catch {
      if (thumbLoadTokens.get(item.id) !== loadToken) return
      thumbLoadTokens.delete(item.id)
      // 模式/目录切换路径已整体 clear；旧 abort 不得误清新代次 loading。
      if (controller.signal.aborted) return
      thumbState.cancelLoad(item.id)
      const failureCount = (thumbRetries.get(item.id)?.failureCount ?? 0) + 1
      const delay = minimapThumbRetryDelay(failureCount)
      if (delay === null) {
        thumbRetries.delete(item.id)
        thumbState.failLoad(item.id, 0, sig)
        return
      }
      const retryAt = Date.now() + delay
      thumbRetries.set(item.id, { sig, failureCount, retryAt })
      // 快速滚过大量坏图时限制退避账本；被驱逐项未来重新进入视窗才会重新尝试。
      if (thumbRetries.size > MAX_RETRY_RECORDS) {
        const oldestId = thumbRetries.keys().next().value
        if (oldestId !== undefined) thumbRetries.delete(oldestId)
      }
      scheduleRetryPaint(retryAt)
    }
  })()
}

// ── 绘制 ─────────────────────────────────────────────────────────────────────
let paintRafId: number | null = null
function schedulePaint(): void {
  if (paintRafId !== null) return
  paintRafId = requestAnimationFrame(() => {
    paintRafId = null
    paint()
  })
}

function paint(): void {
  const canvas = canvasRef.value
  const track = trackRef.value
  const ctx = canvas?.getContext('2d')
  if (!canvas || !track || !ctx) return
  const w = trackW.value
  const h = trackH.value
  const dpr = window.devicePixelRatio || 1
  // DPR 适配(镜像 TimelineScrubberCanvas):缓冲按物理像素,坐标系缩回 CSS 像素。
  const bw = Math.max(1, Math.round(w * dpr))
  const bh = Math.max(1, Math.round(h * dpr))
  if (canvas.width !== bw || canvas.height !== bh) {
    canvas.width = bw
    canvas.height = bh
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, w, h)

  const s = scale.value
  const win = miniWin.value
  if (!(s > 0) || !win) return
  ensureChunks(win)

  const fallback =
    getComputedStyle(track).getPropertyValue('--color-bg-hover').trim() || 'rgba(127,127,127,0.28)'
  const gateOn = loadGate.value
  const i0 = Math.max(0, Math.floor(win.topY / CHUNK_PX))
  const i1 = Math.max(0, Math.floor(win.bottomY / CHUNK_PX))
  const seenRowY = new Set<number>()
  for (let i = i0; i <= i1; i++) {
    const rows = chunkCache.get(i)
    if (!rows || !Array.isArray(rows)) continue
    for (const row of rows) {
      if (row.rowType !== 'normal') continue // 分组视图切换到 minimap 时不绘制分隔标题行
      if (row.y + row.height < win.topY || row.y > win.bottomY) continue
      if (seenRowY.has(row.y)) continue // 块界重复行去重(视口相交语义)
      seenRowY.add(row.y)
      const ry = (row.y - win.topY) * s
      for (const item of row.items) {
        const rx = item.x * s
        const rw = item.w * s
        const rh = item.h * s
        // 两种模式都先画色块:纯色块模式到此为止;缩略图模式则让透明图片也有稳定底色。
        ctx.fillStyle = item.placeholderColor || fallback
        ctx.fillRect(rx, ry, rw, rh)
        if (props.renderMode === 'colors') continue

        // cacheDir 也是资源身份的一部分:设置页迁移缓存目录后,同一 thumbPath 必须硬失效。
        const sig = minimapThumbSig(
          thumbEpoch,
          props.cacheDir,
          item.thumbStatus,
          item.thumbPath,
        )
        thumbState.syncSig(item.id, sig)
        const cached = thumbState.get(item.id)
        if (cached) ctx.drawImage(cached.src, rx, ry, rw, rh)
        else if (!gateOn) startThumbLoad(item, sig, rw * dpr, rh * dpr)
      }
    }
  }
}

// ── 交互(镜像 MediaScrollbar:pointer capture + rAF 节流 jump)────────────────
let grabOffset = 0
let jumpRafId: number | null = null
let pendingY: number | null = null

function emitJumpThrottled(y: number): void {
  pendingY = y
  if (jumpRafId !== null) return
  jumpRafId = requestAnimationFrame(() => {
    jumpRafId = null
    if (pendingY !== null) emit('jump', pendingY)
    pendingY = null
  })
}

function trackTopScreen(): number {
  return trackRef.value?.getBoundingClientRect().top ?? 0
}

function beginDrag(e: PointerEvent, offset: number): void {
  dragging.value = true
  emit('scrubbing', true)
  grabOffset = offset
  trackRef.value?.setPointerCapture(e.pointerId)
}

function moveTo(e: PointerEvent): void {
  const top = e.clientY - trackTopScreen() - grabOffset
  emitJumpThrottled(
    sliderTopToLogicalY(top, props.totalHeight, props.viewportHeight, trackH.value, scale.value),
  )
}

function onSliderPointerdown(e: PointerEvent): void {
  if (e.button !== 0 || !slider.value) return
  beginDrag(e, e.clientY - trackTopScreen() - slider.value.top)
  e.preventDefault()
}

function onTrackPointerdown(e: PointerEvent): void {
  if (e.button !== 0) return
  const win = miniWin.value
  const g = slider.value
  if (!win || !g) return
  // 点击 = 点击处内容居中直达(VSCode 语义),随即可无缝转拖(框中心对指针)。
  emitJumpThrottled(
    clickToLogicalY(
      e.clientY - trackTopScreen(),
      win,
      scale.value,
      props.viewportHeight,
      props.totalHeight,
    ),
  )
  beginDrag(e, g.height / 2)
  e.preventDefault()
}

function onPointermove(e: PointerEvent): void {
  if (dragging.value) moveTo(e)
}

function endDrag(): void {
  if (dragging.value) emit('scrubbing', false)
  dragging.value = false
}

function onWheel(e: WheelEvent): void {
  // 滚轮转发:minimap 上滚动 = 滚画廊(1:1 delta,与直接滚内容手感一致)。
  // 同一帧可能涌入多次 wheel;父层 currentY 尚未来得及回填时须在 pendingY 上累加,否则吞量。
  const baseY = pendingY ?? props.currentY
  emitJumpThrottled(
    wheelToLogicalY(baseY, e.deltaY, e.deltaMode, props.totalHeight, props.viewportHeight),
  )
}

function onKeydown(e: KeyboardEvent): void {
  const next = keyboardToLogicalY(
    e.key,
    pendingY ?? props.currentY,
    props.totalHeight,
    props.viewportHeight,
  )
  if (next === null) return
  e.preventDefault()
  emitJumpThrottled(next)
}

// ── 生命周期与重绘触发 ───────────────────────────────────────────────────────
let resizeObserver: ResizeObserver | null = null

watch(
  () => [
    props.currentY,
    props.totalHeight,
    props.viewportHeight,
    props.containerWidth,
    props.cacheDir,
    trackW.value,
    trackH.value,
  ],
  () => schedulePaint(),
)

// 布局换代:行几何/条目集已变,块缓存整体作废(微图缓存靠 sig 对账自净,保留可复用解码)。
watch(
  () => media.layoutVersion,
  () => {
    chunkCache.clear()
    schedulePaint()
  },
)

// 飞掠闸门放行 → 补起被抑制的微图加载。
watch(loadGate, (deferred) => {
  if (!deferred) schedulePaint()
})

// 色块模式必须释放已有 bitmap 并中止图片 IO;缓存目录变化同样要取消旧目录请求。
// 代次隔离继续负责 createImageBitmap 已开始、无法 Abort 的极窄迟到窗口。
watch(
  () => [props.renderMode, props.cacheDir],
  () => {
    thumbAbortController.abort()
    thumbAbortController = new AbortController()
    thumbEpoch++
    clearThumbRetries()
    thumbLoadTokens.clear()
    thumbState.clear()
    schedulePaint()
  },
)

onMounted(() => {
  const el = trackRef.value
  if (el && typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver((entries) => {
      trackW.value = entries[0].contentRect.width
      trackH.value = entries[0].contentRect.height
    })
    resizeObserver.observe(el)
  }
  schedulePaint()
})

onBeforeUnmount(() => {
  // 拖拽中被卸载(切轴形态/关轴)不让宿主的 scrub 态卡在 true。
  if (dragging.value) emit('scrubbing', false)
  resizeObserver?.disconnect()
  resizeObserver = null
  if (paintRafId !== null) cancelAnimationFrame(paintRafId)
  if (jumpRafId !== null) cancelAnimationFrame(jumpRafId)
  chunkCache.clear()
  thumbAbortController.abort()
  clearThumbRetries()
  thumbLoadTokens.clear()
  thumbState.clear() // ImageBitmap 须显式释放,不能只清 Map
})
</script>

<style scoped>
.minimap-axis {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  user-select: none;
  touch-action: none; /* 触屏上拖本轴 = 导航,不触发页面滚动 */
}

.minimap-axis:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -2px;
}

.minimap-axis__canvas {
  display: block;
  width: 100%;
  height: 100%;
}

/* 当前位置指示线(镜像 TimelineScrubber .tl-indicator):1px accent 半透明,浮在
   canvas 之上、视口框之下(DOM 顺序),不吃指针事件。 */
.minimap-axis__indicator {
  position: absolute;
  left: 0;
  right: 0;
  height: 0;
  border-top: 1px solid var(--color-accent);
  opacity: 0.5;
  transform: translateY(-50%);
  pointer-events: none;
}

/* 视口框不透明度可设(设置页「轴视窗不透明度」):静息 0.3/悬停 0.55 各乘
   --axis-viewport-opacity(默认 1),min() 封顶 1 防缩放 >1 时越界。 */
.minimap-axis__viewport {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  border: 1px solid var(--color-scrollbar-thumb-hover);
  border-radius: 2px;
  background: var(--color-scrollbar-thumb);
  opacity: min(1, calc(0.3 * var(--axis-viewport-opacity, 1)));
  will-change: transform;
  transition: opacity var(--transition-fast);
}

.minimap-axis:hover .minimap-axis__viewport,
.minimap-axis__viewport.is-active {
  opacity: min(1, calc(0.55 * var(--axis-viewport-opacity, 1)));
}
</style>
