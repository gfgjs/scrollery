<!-- 前后对比分割拖杆（降噪/超分子系统 P0 批 5）：左=原图 before、右=增强 after，中缝可拖动。
     数据源容错：beforeSrc/afterSrc 任一缺失（P0 后端 enhance_preview 返 enhance_not_implemented）
     → 显示「预览即将可用」占位态，组件仍可挂载，不阻塞主流程（4.5 批实现后数据自然接上）。 -->
<template>
  <div class="ba">
    <div v-if="!ready" class="ba__placeholder">
      <span>{{ placeholder ?? '' }}</span>
    </div>
    <div
      v-else
      ref="frameEl"
      class="ba__frame"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
    >
      <!-- after 铺满作底；before 用 clip 从左裁到 divider。 -->
      <img class="ba__img" :src="afterSrc ?? ''" draggable="false" />
      <img
        class="ba__img ba__img--before"
        :src="beforeSrc ?? ''"

        draggable="false"
        :style="{ clipPath: `inset(0 ${100 - pos}% 0 0)` }"
      />
      <div class="ba__divider" :style="{ left: pos + '%' }">
        <span class="ba__handle"></span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'

const props = defineProps<{
  beforeSrc: string | null
  afterSrc: string | null
  /** 数据源未就绪时的占位文案。 */
  placeholder?: string
}>()

const ready = computed(() => !!props.beforeSrc && !!props.afterSrc)

const frameEl = ref<HTMLElement | null>(null)
/** 分割位置百分比（0=全 after，100=全 before）。 */
const pos = ref(50)
let dragging = false

function setFromClientX(clientX: number) {
  const el = frameEl.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  if (rect.width === 0) return
  const ratio = ((clientX - rect.left) / rect.width) * 100
  pos.value = Math.min(100, Math.max(0, ratio))
}

function onPointerDown(e: PointerEvent) {
  dragging = true
  ;(e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId)
  setFromClientX(e.clientX)
}
function onPointerMove(e: PointerEvent) {
  if (!dragging) return
  setFromClientX(e.clientX)
}
function onPointerUp(e: PointerEvent) {
  dragging = false
  ;(e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId)
}
</script>

<style scoped>
.ba {
  width: 100%;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--color-bg-primary);
}
.ba__placeholder {
  display: grid;
  place-items: center;
  min-height: 160px;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
  border: 1px dashed var(--color-border);
  border-radius: var(--radius-md);
}
.ba__frame {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 10;
  touch-action: none;
  cursor: ew-resize;
  user-select: none;
}
.ba__img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.ba__img--before {
  /* clipPath 由内联 style 驱动。 */
}
.ba__divider {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  transform: translateX(-1px);
  background: var(--color-accent);
  pointer-events: none;
}
.ba__handle {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 22px;
  height: 22px;
  transform: translate(-50%, -50%);
  border-radius: 50%;
  background: var(--color-accent);
  border: 2px solid var(--color-text-inverse);
}
</style>
