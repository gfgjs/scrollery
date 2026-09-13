<script setup lang="ts">
// VideoSeekBar — 进度条:played(已播)+ buffered(已缓冲)双层,点击/拖拽 seek(拖拽中只动预览、
// 松手才提交,边界 4),hover 显时间 tooltip。sprite 缩略帧预览接口以具名 slot `preview` 预留
// (props/位置已备),实现归 GE 批;GC 仅落时间 tooltip。duration 非有限(NaN/Infinity 直播流)时
// 禁 seek(边界 2)。
import { computed, onScopeDispose, ref, watch } from 'vue'
import { formatPlayerTime } from '../../../utils/format'

const props = defineProps<{
  currentTime: number
  /** 时长(秒);非有限时禁 seek。 */
  duration: number
  /** 已缓冲末端(秒)。 */
  bufferedEnd: number
}>()

const emit = defineEmits<{
  /** 松手提交:跳到该秒。 */
  (e: 'seek', seconds: number): void
  /** 拖拽/hover 预览位置(秒);null=离开,供上层(GE sprite)消费。 */
  (e: 'preview', seconds: number | null): void
}>()

const barRef = ref<HTMLElement | null>(null)
const hovering = ref(false)
const dragging = ref(false)
/** hover 光标处比例 [0,1]。 */
const hoverRatio = ref(0)
/** 拖拽中比例 [0,1]。 */
const dragRatio = ref(0)
/** 松手后待确认比例:currentTime 尚未回读到位前顶替显示,消「跳到点击处-弹回原处-再跳回」的
 * 闪烁(el.currentTime 写入是同步的,但 Vue 侧 currentTime 镜像要等 timeupdate/seeked 事件才回读,
 * 中间这一帧若直接跟 props.currentTime 计算比例就会先回退到旧播放头)。 */
const pendingRatio = ref<number | null>(null)
let pendingClearTimer: ReturnType<typeof setTimeout> | null = null

function clearPending(): void {
  pendingRatio.value = null
  if (pendingClearTimer !== null) {
    clearTimeout(pendingClearTimer)
    pendingClearTimer = null
  }
}

/** duration 可 seek(有限且 >0)。 */
const seekable = computed(() => Number.isFinite(props.duration) && props.duration > 0)

/** 已播比例:拖拽中跟随光标;松手后跟随待确认目标;否则跟随实际播放头。 */
const playedRatio = computed(() => {
  if (dragging.value) return dragRatio.value
  if (pendingRatio.value !== null) return pendingRatio.value
  if (!seekable.value) return 0
  return Math.min(1, Math.max(0, props.currentTime / props.duration))
})

// currentTime 回读到位(与待确认目标足够接近)即可交还渲染权;1000ms 兜底超时防
// seeked 事件异常场景(如切源/报错)下进度条永久卡在待确认位置。
watch(
  () => props.currentTime,
  (t) => {
    if (pendingRatio.value === null || !seekable.value) return
    const actualRatio = t / props.duration
    if (Math.abs(actualRatio - pendingRatio.value) < 0.005) clearPending()
  },
)
// 切源（duration 变化）立即清 pendingRatio，不等 1000ms 兜底——避免残留旧源的待确认比例
// 短暂错误映射到新源的进度条上。
watch(() => props.duration, clearPending)
const bufferedRatio = computed(() => {
  if (!seekable.value) return 0
  return Math.min(1, Math.max(0, props.bufferedEnd / props.duration))
})

/** 预览比例:拖拽优先于 hover。 */
const previewRatio = computed(() => (dragging.value ? dragRatio.value : hoverRatio.value))
const previewVisible = computed(() => seekable.value && (hovering.value || dragging.value))
const previewTime = computed(() => previewRatio.value * props.duration)

function ratioFromEvent(clientX: number): number {
  const el = barRef.value
  if (!el) return 0
  const rect = el.getBoundingClientRect()
  if (rect.width <= 0) return 0
  return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
}

function onPointerDown(e: PointerEvent): void {
  if (!seekable.value) return
  clearPending()
  dragging.value = true
  dragRatio.value = ratioFromEvent(e.clientX)
  emit('preview', dragRatio.value * props.duration)
  // 指针捕获:拖出条外仍持续接收 move/up。
  ;(e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId)
  e.preventDefault()
}
function onPointerMove(e: PointerEvent): void {
  if (!seekable.value) return
  const ratio = ratioFromEvent(e.clientX)
  if (dragging.value) {
    dragRatio.value = ratio
    emit('preview', ratio * props.duration)
  } else if (hovering.value) {
    hoverRatio.value = ratio
    emit('preview', ratio * props.duration)
  }
}
function onPointerUp(e: PointerEvent): void {
  if (!dragging.value) return
  const ratio = ratioFromEvent(e.clientX)
  pendingRatio.value = ratio
  pendingClearTimer = setTimeout(clearPending, 1000)
  emit('seek', ratio * props.duration)
  dragging.value = false
  ;(e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId)
  if (!hovering.value) emit('preview', null)
}
/** 指针被系统中断(如触屏手势被识别为滚动/多指、窗口失焦等):与 pointerup 的差异——
 * 不提交 seek,只复位拖拽态并清预览,避免半途中断却把播放头跳到中断点。 */
function onPointerCancel(e: PointerEvent): void {
  if (!dragging.value) return
  dragging.value = false
  ;(e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId)
  if (!hovering.value) emit('preview', null)
}
function onPointerEnter(): void {
  hovering.value = true
}
function onPointerLeave(): void {
  hovering.value = false
  if (!dragging.value) emit('preview', null)
}

onScopeDispose(() => {
  if (pendingClearTimer !== null) clearTimeout(pendingClearTimer)
})
</script>

<template>
  <div
    ref="barRef"
    class="video-seekbar"
    :class="{ 'is-disabled': !seekable, 'is-active': dragging || hovering }"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerCancel"
    @pointerenter="onPointerEnter"
    @pointerleave="onPointerLeave"
  >
    <div class="video-seekbar__track">
      <div class="video-seekbar__buffered" :style="{ width: `${bufferedRatio * 100}%` }" />
      <div class="video-seekbar__played" :style="{ width: `${playedRatio * 100}%` }">
        <span class="video-seekbar__thumb" />
      </div>
    </div>

    <!-- 预览浮层:上层 sprite 缩略帧(GE 具名 slot)+ 时间 tooltip(GC)。 -->
    <div
      v-if="previewVisible"
      class="video-seekbar__preview"
      :style="{ left: `${previewRatio * 100}%` }"
    >
      <slot name="preview" :ratio="previewRatio" :time="previewTime" />
      <span class="video-seekbar__preview-time">{{ formatPlayerTime(previewTime) }}</span>
    </div>
  </div>
</template>

<style scoped>
/* 进度条:承 VideoControlBar 深色浮层语义(硬编码白系,不随主题——同 ContentViewer 看图台豁免)。
   悬停/拖拽时轨道加粗、拇指显现。上下留 padding 扩大命中区(细轨道也易点)。 */
.video-seekbar {
  position: relative;
  height: 16px;
  padding: 6px 0;
  cursor: pointer;
  touch-action: none;
}
.video-seekbar.is-disabled {
  cursor: default;
  opacity: 0.5;
}
.video-seekbar__track {
  position: relative;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.22);
  overflow: visible;
  transition: height var(--transition-fast);
}
.video-seekbar.is-active .video-seekbar__track {
  height: 6px;
}
.video-seekbar__buffered {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.4);
}
.video-seekbar__played {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  border-radius: 2px;
  background: var(--color-accent);
}
.video-seekbar__thumb {
  position: absolute;
  right: 0;
  top: 50%;
  width: 12px;
  height: 12px;
  transform: translate(50%, -50%) scale(0);
  border-radius: 50%;
  background: var(--color-accent);
  box-shadow: 0 0 0 2px rgba(0, 0, 0, 0.35);
  transition: transform var(--transition-fast);
}
.video-seekbar.is-active .video-seekbar__thumb {
  transform: translate(50%, -50%) scale(1);
}
/* 预览浮层:居中于光标 x,位于轨道上方。 */
.video-seekbar__preview {
  position: absolute;
  bottom: 100%;
  margin-bottom: 8px;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  pointer-events: none;
}
.video-seekbar__preview-time {
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  background: rgba(0, 0, 0, 0.82);
  color: #fff;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
</style>
