<template>
  <div ref="hostRef" class="cropper-selection-overlay" :style="hostStyle">
    <!-- Cropper.js 要求首参为 img/canvas；自定义 template 只保留选择层，不生成结果 canvas。 -->
    <canvas ref="sourceRef" width="1" height="1"></canvas>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import Cropper, { type CropperSelection } from 'cropperjs'

import type { CropRectNorm } from '../../composables/useImageEditor'
import { normalizedToSelection, selectionToNormalized } from './cropperCoordinates'

interface Box {
  left: number
  top: number
  width: number
  height: number
}

const props = defineProps<{
  rect: CropRectNorm
  box: Box
  aspectRatio: number | null
}>()

const emit = defineEmits<{ change: [rect: CropRectNorm] }>()

const TEMPLATE =
  '<cropper-canvas>' +
  '<cropper-selection movable resizable keyboard outlined precise>' +
  '<cropper-grid bordered covered></cropper-grid>' +
  '<cropper-crosshair centered></cropper-crosshair>' +
  '<cropper-handle action="move" theme-color="rgba(255,255,255,.28)"></cropper-handle>' +
  '<cropper-handle action="n-resize"></cropper-handle>' +
  '<cropper-handle action="e-resize"></cropper-handle>' +
  '<cropper-handle action="s-resize"></cropper-handle>' +
  '<cropper-handle action="w-resize"></cropper-handle>' +
  '<cropper-handle action="ne-resize"></cropper-handle>' +
  '<cropper-handle action="nw-resize"></cropper-handle>' +
  '<cropper-handle action="se-resize"></cropper-handle>' +
  '<cropper-handle action="sw-resize"></cropper-handle>' +
  '</cropper-selection>' +
  '</cropper-canvas>'

const hostRef = ref<HTMLDivElement | null>(null)
const sourceRef = ref<HTMLCanvasElement | null>(null)
let cropper: Cropper | null = null
let selection: CropperSelection | null = null
let syncingFromProps = false

const hostStyle = computed(() => ({
  left: `${props.box.left}px`,
  top: `${props.box.top}px`,
  width: `${props.box.width}px`,
  height: `${props.box.height}px`,
}))

function syncSelection(): void {
  if (!selection || !props.box.width || !props.box.height) return
  const rect = normalizedToSelection(props.rect, props.box.width, props.box.height)
  syncingFromProps = true
  selection.aspectRatio = props.aspectRatio ?? Number.NaN
  selection.$change(rect.x, rect.y, rect.width, rect.height, props.aspectRatio ?? Number.NaN, true)
  syncingFromProps = false
}

function onSelectionChange(event: Event): void {
  if (syncingFromProps || !props.box.width || !props.box.height) return
  const detail = (event as CustomEvent<{ x: number; y: number; width: number; height: number }>)
    .detail
  emit('change', selectionToNormalized(detail, props.box.width, props.box.height))
}

onMounted(async () => {
  await nextTick()
  if (!sourceRef.value || !hostRef.value) return
  cropper = new Cropper(sourceRef.value, { container: hostRef.value, template: TEMPLATE })
  selection = cropper.getCropperSelection()
  selection?.addEventListener('change', onSelectionChange)
  syncSelection()
})

watch(
  () => [props.rect.x, props.rect.y, props.rect.width, props.rect.height, props.aspectRatio],
  syncSelection,
)
watch(() => [props.box.width, props.box.height], syncSelection)

onBeforeUnmount(() => {
  selection?.removeEventListener('change', onSelectionChange)
  cropper?.destroy()
  selection = null
  cropper = null
})
</script>

<style scoped>
.cropper-selection-overlay {
  position: absolute;
  z-index: 2;
  overflow: hidden;
  touch-action: none;
}

.cropper-selection-overlay canvas {
  display: none;
}

.cropper-selection-overlay :deep(cropper-canvas) {
  width: 100%;
  height: 100%;
  background: transparent;
}
</style>
