<template>
  <!-- T16 B3.2 自研逻辑滚动条:拇指 = 当前逻辑位 / 逻辑总高 的纯百分比渲染,与画廊
       逐帧同步;不参与物理 scrollTop 账务(映射态停稳偿债只动原生 scrollTop,本条
       零感知——这正是它替代原生条的理由,见 mediaScrollbar.helpers.ts 头注)。
       轨道常驻为透明命中区,空闲时拇指淡出;拖拽经 pointer capture,轨道点击直达定位
       并可无缝转拖。注:覆盖层锚定 .media-grid-wrapper 全高,人物返回栏可见时轨道顶
       与其重叠 ~34px(几何误差 <4%,特例场景可接受)。 -->
  <div
    ref="trackRef"
    class="media-scrollbar"
    :class="{
      'is-active': lingering || dragging || hovering || scrubbing,
      'is-dragging': dragging || scrubbing,
    }"
    :style="{ '--line-full': lineFullWidth + 'px' }"
    @pointerdown="onTrackPointerdown"
    @pointermove="onPointermove"
    @pointerup="endDrag"
    @pointercancel="endDrag"
    @pointerenter="hovering = true"
    @pointerleave="hovering = false"
  >
    <!-- 标尺线(独立于拇指):纵位 = currentY/可滚动行程 比例 × 轨道高(顶/底恰贴轨道
         两端),与时间轴 tl-indicator / minimap 指示线同一映射——拇指是固定高度(fixedThumb),
         其中心与轴内指示线存在可见错位(用户回报),故不再挂拇指中心。未拖动 = 短刻度;
         拖动经 .is-dragging 切 width 展开到画廊全宽(--line-full,px 实测)。宽度
         动画只在拖起/松手各跑一次,不与逐帧 translateY 干扰(transition 不含
         transform);class + CSS 驱动可在 devtools 直接核对状态。 -->
    <div
      v-if="geom"
      class="media-scrollbar__line"
      :style="{ transform: `translateY(${lineY}px)` }"
    />
    <div
      v-if="geom"
      class="media-scrollbar__thumb"
      :style="{ transform: `translateY(${geom.top}px)`, height: geom.height + 'px' }"
      @pointerdown.stop="onThumbPointerdown"
    >
      <!-- 视觉层与位移层拆分:外层只做逐帧 translateY(无 transition,防撕裂/掉帧),
           缩放/配色等观感过渡全部下沉到内层,互不干扰。 -->
      <div
        class="media-scrollbar__thumb-pill"
        :style="{ '--thumb-scale': axisVisible ? 1 : 1.2 }"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { thumbGeometry, thumbTopToLogicalY } from './mediaScrollbar.helpers'

const props = withDefaults(
  defineProps<{
    /** 逻辑总高(px)——与引擎 totalHeight 同源,非物理 spacer。 */
    totalHeight: number
    /** 当前逻辑滚动位(px)——双引擎统一的 currentLogicalY。 */
    currentY: number
    /** 宿主滚动进行中(isScrolling)→ 拇指显形。 */
    active: boolean
    /** thumb 最小高(px):与时间轴拖动视窗共享同一设置,二者同高(默认 48)。 */
    minThumb?: number
    /** 时间轴/minimap 是否可见:不可见时指针放大 1.2× 补偿缺失的位置线索。 */
    axisVisible?: boolean
    /** 轴上拖拽(时间轴 scrub / minimap 视窗)进行中——宿主中转:标尺线同步展开,
        轴侧拖动也获得贯穿画廊的全宽位置线。 */
    scrubbing?: boolean
  }>(),
  { minThumb: 48, axisVisible: true, scrubbing: false },
)

const emit = defineEmits<{ (e: 'jump', y: number): void }>()

const trackRef = ref<HTMLElement | null>(null)
const trackH = ref(0)
/// 画廊(track 父元素 .media-grid-wrapper)宽度:横线拖动展开的目标全宽。
/// 宿主无 overflow:hidden 可借,超宽会溢进左侧面板,必须实测精确 px。
const wrapperW = ref(0)
const hovering = ref(false)
const dragging = ref(false)

/// 线右端锚在轨道右缘(right:0,用户定样式):全宽 = 画廊宽,恰好延伸到画廊左缘。
const lineFullWidth = computed(() => Math.max(0, wrapperW.value))

/// 显形滞回(B3.2.1 真机回报修复):active(宿主 isScrolling)在滚动收尾会抖动——
/// 惯性尾梢的稀疏滚动事件(间隔超过宿主 150ms 复位阈值)与停稳偿债的内部 scrollTop
/// 写各翻转一轮 false→true→false,拇指随 opacity/颜色过渡明暗频闪。显形立即、
/// 隐没延迟合并:短暂重激活并入同一次可见期,任凭信号抖动也只有一次淡出。
const LINGER_MS = 700
const lingering = ref(false)
let lingerTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.active,
  (on) => {
    if (lingerTimer !== null) {
      clearTimeout(lingerTimer)
      lingerTimer = null
    }
    if (on) {
      lingering.value = true
      return
    }
    lingerTimer = setTimeout(() => {
      lingerTimer = null
      lingering.value = false
    }, LINGER_MS)
  },
  { immediate: true },
)
let resizeObserver: ResizeObserver | null = null
/// 抓点 = 指针在拇指内的相对位置(px),拖动全程保持,拇指不会跳到指针下。
let grabOffset = 0
let rafId: number | null = null
let pendingY: number | null = null

const maxY = computed(() => Math.max(0, props.totalHeight - trackH.value))
const clampedY = computed(() => Math.min(maxY.value, Math.max(0, props.currentY)))
const geom = computed(() =>
  // fixedThumb = minThumb:指针改为固定尺寸的左向箭头,高度不再随库长比例伸缩。
  thumbGeometry(clampedY.value, props.totalHeight, trackH.value, props.minThumb, props.minThumb),
)
/// 标尺线纵位(px):currentY/可滚动行程(maxY = totalHeight−trackH)比例 × 轨道高,与轴内
/// 指示线同源映射。分母不可取 totalHeight:currentY 贴底最大只到 maxY,那样贴底会留
/// trackH²/totalHeight 的空档(用户回报)。固定高拇指中心走 thumbGeometry 插值,纵位与
/// 本线不重合,不能挂拇指中心。
const lineY = computed(() => (clampedY.value / Math.max(1, maxY.value)) * trackH.value)

/// 拖拽跳转按 rAF 节流:pointermove 可达 120-240Hz,每帧只发最后一个目标位——
/// 引擎侧远跳/近跳都是 O(1) 算术,但没必要一帧多次。
function emitJumpThrottled(y: number) {
  pendingY = y
  if (rafId !== null) return
  rafId = requestAnimationFrame(() => {
    rafId = null
    if (pendingY !== null) emit('jump', pendingY)
    pendingY = null
  })
}

function trackTopScreen(): number {
  return trackRef.value?.getBoundingClientRect().top ?? 0
}

function beginDrag(e: PointerEvent, offset: number) {
  dragging.value = true
  grabOffset = offset
  // capture 设在轨道上:后续 move/up 无论指针滑到哪都回到本组件。
  trackRef.value?.setPointerCapture(e.pointerId)
}

function moveTo(e: PointerEvent) {
  const g = geom.value
  if (!g) return
  const thumbTop = e.clientY - trackTopScreen() - grabOffset
  emitJumpThrottled(thumbTopToLogicalY(thumbTop, props.totalHeight, trackH.value, g.height))
}

function onThumbPointerdown(e: PointerEvent) {
  if (e.button !== 0 || !geom.value) return
  beginDrag(e, e.clientY - trackTopScreen() - geom.value.top)
  e.preventDefault()
}

function onTrackPointerdown(e: PointerEvent) {
  if (e.button !== 0 || !geom.value) return
  // 轨道点击 = 直达定位(拇指中心对齐点击点),随即可无缝继续拖动。
  beginDrag(e, geom.value.height / 2)
  moveTo(e)
  e.preventDefault()
}

function onPointermove(e: PointerEvent) {
  if (!dragging.value) return
  moveTo(e)
}

function endDrag() {
  dragging.value = false
}

onMounted(() => {
  const el = trackRef.value
  if (!el || typeof ResizeObserver === 'undefined') return
  resizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      if (entry.target === el) trackH.value = entry.contentRect.height
      else wrapperW.value = entry.contentRect.width
    }
  })
  resizeObserver.observe(el)
  // 同一 observer 兼职量画廊宽:轨道自身只有 16px 宽,横线全宽须取父容器。
  if (el.parentElement) resizeObserver.observe(el.parentElement)
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
  if (rafId !== null) cancelAnimationFrame(rafId)
  if (lingerTimer !== null) clearTimeout(lingerTimer)
})
</script>

<style scoped>
.media-scrollbar {
  position: absolute;
  top: 0;
  bottom: 0;
  right: 0;
  width: 16px; /* 命中区容纳药丸(6px)+ 右呼吸边距 + 抓取余量 */
  z-index: 5;
  user-select: none;
  touch-action: none; /* 触屏上拖本条 = 拖拇指,不触发页面滚动 */
}

.media-scrollbar__thumb {
  /* 外层只承载逐帧位移(translateY)与几何尺寸,严禁任何 transition/scale——
     拖拽/RAF 节流靠它逐帧写 style,过渡效果会与高频写入打架、造成视觉拖影。 */
  position: absolute;
  top: 0;
  right: 0;
  width: 100%;
  will-change: transform;
}

.media-scrollbar__thumb-pill {
  /* 内层承载全部视觉:圆角药丸,右缘留 4px 呼吸位。缩放(轴隐藏时 1.2×)与配色/
     加宽过渡全部落在这层,与外层的逐帧位移互不干扰。 */
  position: absolute;
  top: 0;
  right: 0;
  width: var(--scrollbar-width);
  height: 100%;
  border-radius: var(--radius-full);
  background: var(--color-scrollbar-thumb);
  opacity: 0.35;
  transform: scale(var(--thumb-scale, 1));
  transform-origin: right center;
  transition:
    opacity var(--transition-fast),
    background var(--transition-fast),
    width 160ms ease,
    transform 160ms ease;
  pointer-events: none; /* 事件留给外层 .media-scrollbar__thumb,内层纯视觉 */
}

.media-scrollbar.is-active .media-scrollbar__thumb-pill,
.media-scrollbar:hover .media-scrollbar__thumb-pill {
  opacity: 1;
  background: var(--color-scrollbar-thumb-hover);
}

/* 拖动/轴 scrub 中药丸染主题强调色并微加宽,与横线连成一体的鲜艳 T 形。 */
.media-scrollbar.is-dragging .media-scrollbar__thumb-pill {
  width: 8px;
  background: var(--color-accent);
  opacity: 1;
}

.media-scrollbar__line {
  /* 标尺线:top:-1px 让 2px 高的线居中于 translateY 映射点(与轴内 1px 指示线同点);
     纵向逐帧 translateY(内联),transition 刻意不含 transform。展开走 width 过渡而非
     transform:元素仅 2px 高、且只在拖起/松手各触发一次,reflow 可忽略,换来 devtools
     可直读的确定状态(scaleX 内联写法排障吃过亏)。 */
  position: absolute;
  top: -1px;
  right: 0;
  width: 24px;
  height: 2px;
  will-change: transform;
  border-radius: 1px;
  background: var(--color-accent);
  box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 55%, transparent);
  opacity: 0.35;
  transition:
    width 320ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity var(--transition-fast);
  pointer-events: none; /* 与药丸同理,命中区全归外层 */
}

.media-scrollbar.is-active .media-scrollbar__line,
.media-scrollbar:hover .media-scrollbar__line {
  opacity: 1;
}

/* 拖动展开贯穿画廊;max() 兜底:ResizeObserver 首回调前实测宽未就绪(0)时保持短刻度,不闪没。 */
.media-scrollbar.is-dragging .media-scrollbar__line {
  width: max(24px, var(--line-full, 24px));
}
</style>
