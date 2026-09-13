<template>
  <div class="edit-overlay" @pointerdown.stop @wheel.stop @click.stop @contextmenu.stop.prevent>
    <div class="edit-overlay__stage" ref="stageRef">
      <div v-if="!supported" class="edit-overlay__unsupported">
        <p>{{ t('edit.unsupportedFormat') }}</p>
      </div>
      <div v-else-if="preview.loading.value" class="edit-overlay__unsupported">
        <p>{{ t('edit.previewLoading') }}</p>
      </div>
      <div v-else-if="preview.error.value" class="edit-overlay__unsupported">
        <p>{{ t('edit.previewFailed') }}</p>
      </div>
      <template v-else-if="preview.url.value">
        <div class="edit-overlay__fine-viewport" :style="displayBoxStyle">
          <img
            v-show="!adjustCanvasVisible"
            ref="imgRef"
            :src="preview.url.value"
            class="edit-overlay__img"
            :style="imgStyle"
            draggable="false"
            @load="onImgLoad"
          />
          <!-- E3 调色预览:同一坐标/变换,像素为双端同源公式结果;半透明区域靠 v-show 互斥防双重合成 -->
          <canvas
            v-show="adjustCanvasVisible"
            ref="adjustCanvasRef"
            class="edit-overlay__img"
            :style="imgStyle"

          ></canvas>
        </div>
        <div
          v-if="straightenActive"
          class="edit-overlay__straighten-grid"
          :style="displayBoxStyle"

        >
          <span class="is-v" style="left: 33.333%"></span>
          <span class="is-v" style="left: 66.667%"></span>
          <span class="is-h" style="top: 33.333%"></span>
          <span class="is-h" style="top: 66.667%"></span>
        </div>
        <CropperSelectionOverlay
          v-if="editor.crop.value"
          :rect="editor.crop.value"
          :box="displayBox"
          :aspect-ratio="cropAspectRatio"
          @change="editor.setCropRect"
        />
      </template>
    </div>

    <div v-if="supported" class="edit-overlay__toolbar">
      <UiIconButton :label="t('edit.rotateCcw')" @click="editor.rotateCcw()">
        <RotateCcw :size="18" />
      </UiIconButton>
      <UiIconButton :label="t('edit.rotateCw')" @click="editor.rotateCw()">
        <RotateCw :size="18" />
      </UiIconButton>
      <UiIconButton
        :label="t('edit.flipHorizontal')"
        :active="editor.flipH.value"
        @click="editor.toggleFlipH()"
      >
        <FlipHorizontal :size="18" />
      </UiIconButton>
      <UiIconButton
        :label="t('edit.flipVertical')"
        :active="editor.flipV.value"
        @click="editor.toggleFlipV()"
      >
        <FlipVertical :size="18" />
      </UiIconButton>
      <span class="edit-overlay__sep"></span>
      <UiIconButton :label="t('edit.crop')" :active="cropToolActive" @click="toggleCropTool">
        <CropIcon :size="18" />
      </UiIconButton>
      <select
        v-if="cropToolActive"
        class="edit-overlay__ratio-select"
        :value="editor.cropRatio.value"
        @change="onRatioChange"
      >
        <option value="free">{{ t('edit.cropFree') }}</option>
        <option value="original">{{ t('edit.cropOriginal') }}</option>
        <option value="1:1">1:1</option>
        <option value="4:3">4:3</option>
        <option value="3:4">3:4</option>
        <option value="16:9">16:9</option>
        <option value="9:16">9:16</option>
      </select>
      <span class="edit-overlay__sep"></span>
      <label class="edit-overlay__straighten">
        <span>{{ t('edit.straighten') }}</span>
        <input
          type="range"
          min="-45"
          max="45"
          step="0.1"
          :value="editor.rotateFine.value"

          @input="onStraightenInput"
          @pointerdown="straightenActive = true"
          @pointerup="straightenActive = false"
          @pointercancel="straightenActive = false"
          @change="straightenActive = false"
          @blur="straightenActive = false"
        />
        <output>{{ editor.rotateFine.value.toFixed(1) }}{{ t('edit.degreesUnit') }}</output>
      </label>
      <span class="edit-overlay__sep"></span>
      <label class="edit-overlay__adjust">
        <span>{{ t('edit.brightness') }}</span>
        <input
          type="range"
          min="-100"
          max="100"
          step="1"
          :value="editor.brightness.value"

          @input="editor.setBrightness(Number(($event.target as HTMLInputElement).value))"
        />
        <output>{{ editor.brightness.value }}</output>
      </label>
      <label class="edit-overlay__adjust">
        <span>{{ t('edit.contrast') }}</span>
        <input
          type="range"
          min="-100"
          max="100"
          step="1"
          :value="editor.contrast.value"

          @input="editor.setContrast(Number(($event.target as HTMLInputElement).value))"
        />
        <output>{{ editor.contrast.value }}</output>
      </label>
      <label class="edit-overlay__adjust">
        <span>{{ t('edit.saturation') }}</span>
        <input
          type="range"
          min="-100"
          max="100"
          step="1"
          :value="editor.saturation.value"

          @input="editor.setSaturation(Number(($event.target as HTMLInputElement).value))"
        />
        <output>{{ editor.saturation.value }}</output>
      </label>
      <span class="edit-overlay__sep"></span>
      <UiIconButton :label="t('edit.reset')" @click="onReset">
        <Undo2 :size="18" />
      </UiIconButton>
    </div>

    <div class="edit-overlay__footer">
      <div v-if="supported" class="edit-overlay__output">
        <label class="edit-overlay__field">
          <span>{{ t('edit.format') }}</span>
          <select v-model="editor.format.value">
            <option value="jpeg">JPEG</option>
            <option value="png">PNG</option>
          </select>
        </label>
        <label v-if="editor.format.value === 'jpeg'" class="edit-overlay__field">
          <span>{{ t('edit.quality') }}</span>
          <input
            type="range"
            min="1"
            max="100"
            v-model.number="editor.quality.value"
            class="edit-overlay__quality"
          />
          <span class="edit-overlay__quality-value">{{ editor.quality.value }}</span>
        </label>
      </div>

      <p
        v-if="editor.status.value === 'error' && editor.errorMessage.value"
        class="edit-overlay__message is-error"
      >
        {{ editor.errorMessage.value }}
      </p>
      <div
        v-if="editor.status.value === 'partial' && editor.savedNeedsIndex.value"
        class="edit-overlay__message is-warning"
      >
        <p>
          {{ t('edit.savedNeedsIndexTitle') }} —
          {{
            t('edit.savedNeedsIndexMessage', { fileName: editor.savedNeedsIndex.value.pathHint })
          }}
        </p>
        <button class="btn-secondary" @click="onRescanNow">{{ t('edit.rescanNow') }}</button>
        <button class="btn-secondary" @click="handleClose">{{ t('common.ok') }}</button>
      </div>

      <div class="edit-overlay__actions">
        <button class="btn-secondary" :disabled="isSaving" @click="handleClose">
          {{ t('edit.cancel') }}
        </button>
        <button v-if="supported" class="btn-primary" :disabled="isSaving" @click="handleSave">
          {{ isSaving ? t('edit.saving') : t('edit.save') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// src/components/media/EditOverlay.vue
// 图片简单编辑呈现层(方案 C §7):几何/状态机在 useImageEditor.ts,本组件只管测量与手柄拖拽。
//
// 预览变换顺序(与后端 editing::geometry 严格对应,见 useImageEditor.ts 文件头注释):
// `<img>` 只消费 Rust 生成的长边 2048 sRGB blob，orientation 已在后端烤入；元素 CSS 尺寸显式
// 投影为全尺寸源宽高，CSS transform 从内到外依次为 90° rotate → flip → fine rotate →
// 整体 fitScale → 居中位移，与后端 D-107 顺序一致。外层 viewport 裁掉 fine rotate 的边角，
// 尺寸由 Rust/TS 共享的保持宽高比最大内接公式给出；裁剪框也在该 viewport 坐标系内。
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  RotateCcw,
  RotateCw,
  FlipHorizontal,
  FlipVertical,
  Crop as CropIcon,
  Undo2,
} from '@lucide/vue'
import UiIconButton from '../ui/UiIconButton.vue'
import CropperSelectionOverlay from './CropperSelectionOverlay.vue'
import { useToastStore } from '../../stores/toastStore'
import { useScanStore } from '../../stores/scanStore'
import type { MediaDetail } from '../../types/media'
import type { ImageEditor, CropRatioId } from '../../composables/useImageEditor'
import { requestDiscardEdits } from '../../composables/useImageEditor'
import { useEditPreview } from '../../composables/useEditPreview'
import { useAdjustPreview } from '../../composables/useAdjustPreview'
import { isNeutralAdjust, type AdjustParams } from '../../composables/adjustFormula'

const props = defineProps<{
  detail: MediaDetail
  editor: ImageEditor
}>()

const emit = defineEmits<{
  close: []
  saved: [newItemId: number]
}>()

const { t } = useI18n()
const toast = useToastStore()
const scan = useScanStore()
const editor = props.editor
const preview = useEditPreview()

/** v1 已准入的可编辑输入格式(须与后端 `edit_commands::SUPPORTED_INPUT_EXTS` 保持一致——
 * 纯前端提示用,真正的准入判定仍在后端;此处只为避免打开一个注定报错的编辑会话)。 */
const SUPPORTED_INPUT_EXTS = new Set(['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tiff', 'tif'])
const supported = computed(() => SUPPORTED_INPUT_EXTS.has(props.detail.fileFormat.toLowerCase()))
const isSaving = computed(() => editor.status.value === 'saving')

const stageRef = ref<HTMLDivElement | null>(null)
const imgRef = ref<HTMLImageElement | null>(null)
const stageWidth = ref(0)
const stageHeight = ref(0)
const straightenActive = ref(false)

// ── E3 调色预览(D-104:canvas 双端同源公式,rAF 节流,禁 CSS filter)─────────────
const adjustPreview = useAdjustPreview()
const adjustCanvasRef = adjustPreview.canvasRef
const adjustParams = computed<AdjustParams>(() => ({
  brightness: editor.brightness.value,
  contrast: editor.contrast.value,
  saturation: editor.saturation.value,
}))
// 三滑杆全零时展示原 <img>(零 canvas 常驻成本);任一非零且基准像素就绪时切 canvas。
const adjustCanvasVisible = computed(
  () => !isNeutralAdjust(adjustParams.value) && adjustPreview.ready.value,
)
watch(adjustParams, (params) => adjustPreview.schedule(params))

function onImgLoad(): void {
  if (!imgRef.value) return
  editor.setNaturalSize(preview.sourceWidth.value, preview.sourceHeight.value)
  // 预览位图就绪即抓基准像素;失败(极端环境)时 ready 保持 false,维持纯 <img> 展示。
  if (adjustPreview.setSource(imgRef.value)) adjustPreview.schedule(adjustParams.value)
}

let ro: ResizeObserver | null = null
onMounted(() => {
  if (supported.value) void preview.load(props.detail.id)
  if (typeof ResizeObserver !== 'undefined' && stageRef.value) {
    ro = new ResizeObserver(() => {
      stageWidth.value = stageRef.value?.clientWidth ?? 0
      stageHeight.value = stageRef.value?.clientHeight ?? 0
    })
    ro.observe(stageRef.value)
    stageWidth.value = stageRef.value.clientWidth
    stageHeight.value = stageRef.value.clientHeight
  }
  // 图已在浏览器缓存中命中(不会再触发 @load)时补一次尺寸回填。
  if (imgRef.value?.complete && imgRef.value.naturalWidth) onImgLoad()
})
onBeforeUnmount(() => {
  adjustPreview.dispose()
  preview.dispose()
  ro?.disconnect()
})

const fitScale = computed(() => {
  const pw = editor.postWidth.value
  const ph = editor.postHeight.value
  if (!pw || !ph || !stageWidth.value || !stageHeight.value) return 1
  return Math.min(stageWidth.value / pw, stageHeight.value / ph, 1)
})

// 顺序(外→内):居中位移 → 整体适配缩放 → fine rotate → 翻转 → 90° rotate。
const imgTransform = computed(() => {
  const fx = editor.flipH.value ? -1 : 1
  const fy = editor.flipV.value ? -1 : 1
  return `translate(-50%, -50%) scale(${fitScale.value}) rotate(${editor.rotateFine.value}deg) scaleX(${fx}) scaleY(${fy}) rotate(${editor.combinedRotation.value}deg)`
})

const imgStyle = computed(() => ({
  transform: imgTransform.value,
  width: `${editor.naturalWidth.value}px`,
  height: `${editor.naturalHeight.value}px`,
}))

const displayBox = computed(() => {
  const dw = editor.postWidth.value * fitScale.value
  const dh = editor.postHeight.value * fitScale.value
  return {
    left: (stageWidth.value - dw) / 2,
    top: (stageHeight.value - dh) / 2,
    width: dw,
    height: dh,
  }
})

const displayBoxStyle = computed(() => ({
  left: `${displayBox.value.left}px`,
  top: `${displayBox.value.top}px`,
  width: `${displayBox.value.width}px`,
  height: `${displayBox.value.height}px`,
}))

// ── 裁剪工具 ─────────────────────────────────────────────────────────────
const cropToolActive = ref(false)
function toggleCropTool(): void {
  cropToolActive.value = !cropToolActive.value
  if (cropToolActive.value && !editor.crop.value) {
    editor.setCropRect({ x: 0.1, y: 0.1, width: 0.8, height: 0.8 })
  }
}

function onRatioChange(e: Event): void {
  editor.setCropRatio((e.target as HTMLSelectElement).value as CropRatioId)
}

function onReset(): void {
  editor.resetOps()
  cropToolActive.value = false
}

function onStraightenInput(event: Event): void {
  editor.setRotateFine(Number((event.target as HTMLInputElement).value))
}

const cropAspectRatio = computed<number | null>(() => {
  const r = editor.cropRatio.value
  if (r === 'free') return null
  if (r === 'original') return editor.postWidth.value / Math.max(editor.postHeight.value, 1)
  const parts = r.split(':').map(Number)
  return parts[0] / parts[1]
})

// ── 保存 / 取消 ───────────────────────────────────────────────────────────
async function handleSave(): Promise<void> {
  const result = await editor.save(props.detail.id)
  if (!result) return // 失败:editor.status 已转 'error',错误消息在页内展示,停留编辑态重试
  if (result.status === 'saved') {
    emit('saved', result.newItemId)
  }
  // 'savedNeedsIndex':停在页内展示 partial 消息,等用户点「立即扫描」或「知道了」
}

async function onRescanNow(): Promise<void> {
  const rootId = editor.savedNeedsIndex.value?.rootId
  if (rootId == null) return
  try {
    await scan.startScan(rootId)
    toast.addToast('success', t('edit.rescanStarted'))
  } catch (e) {
    toast.addToast('error', e instanceof Error ? e.message : String(e))
  }
  handleClose()
}

async function handleClose(): Promise<void> {
  if (editor.status.value === 'saving') return
  if (editor.status.value === 'partial' || editor.status.value === 'done') {
    emit('close')
    return
  }
  const ok = await requestDiscardEdits(editor)
  if (ok) emit('close')
}

defineExpose({ requestClose: handleClose })
</script>

<style scoped>
.edit-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: #000;
  z-index: 5;
}
.edit-overlay__stage {
  position: relative;
  flex: 1;
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
}
.edit-overlay__img {
  position: absolute;
  left: 50%;
  top: 50%;
  max-width: none;
  max-height: none;
  user-select: none;
}
.edit-overlay__fine-viewport {
  position: absolute;
  overflow: hidden;
}
.edit-overlay__straighten-grid {
  position: absolute;
  pointer-events: none;
  border: 1px solid rgba(255, 255, 255, 0.8);
}
.edit-overlay__straighten-grid span {
  position: absolute;
  display: block;
  background: rgba(255, 255, 255, 0.55);
}
.edit-overlay__straighten-grid .is-v {
  top: 0;
  bottom: 0;
  width: 1px;
}
.edit-overlay__straighten-grid .is-h {
  left: 0;
  right: 0;
  height: 1px;
}
.edit-overlay__unsupported {
  color: rgba(255, 255, 255, 0.85);
  font-size: var(--font-size-md, 14px);
  text-align: center;
  padding: var(--spacing-xl);
}
.edit-overlay__toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap; /* E3 三滑杆加入后窄窗允许折行,不截断控件 */
  gap: var(--spacing-xs, 4px);
  padding: var(--spacing-sm, 8px) var(--spacing-md, 12px);
  background: rgba(0, 0, 0, 0.75);
  color: #fff;
}
.edit-overlay__sep {
  width: 1px;
  height: 20px;
  background: rgba(255, 255, 255, 0.25);
  margin: 0 var(--spacing-xs, 4px);
}
.edit-overlay__ratio-select {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
  border: 1px solid rgba(255, 255, 255, 0.3);
  border-radius: 4px;
  padding: 2px 6px;
}
.edit-overlay__straighten {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs, 4px);
  font-size: var(--font-size-sm, 13px);
  white-space: nowrap;
}
.edit-overlay__straighten input {
  width: 140px;
}
.edit-overlay__straighten output {
  min-width: 3.8em;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.edit-overlay__adjust {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs, 4px);
  font-size: var(--font-size-sm, 13px);
  white-space: nowrap;
}
.edit-overlay__adjust input {
  width: 90px;
}
.edit-overlay__adjust output {
  min-width: 2.6em;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.edit-overlay__footer {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm, 8px);
  padding: var(--spacing-sm, 8px) var(--spacing-md, 12px);
  background: rgba(0, 0, 0, 0.85);
  color: #fff;
}
.edit-overlay__output {
  display: flex;
  align-items: center;
  gap: var(--spacing-md, 12px);
}
.edit-overlay__field {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs, 4px);
  font-size: var(--font-size-sm, 13px);
}
.edit-overlay__quality {
  width: 120px;
}
.edit-overlay__quality-value {
  min-width: 2.5em;
  text-align: right;
}
.edit-overlay__message {
  font-size: var(--font-size-sm, 13px);
  display: flex;
  align-items: center;
  gap: var(--spacing-sm, 8px);
  flex-wrap: wrap;
}
.edit-overlay__message.is-error {
  color: var(--color-error);
}
.edit-overlay__message.is-warning {
  color: var(--color-warning);
}
.edit-overlay__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm, 8px);
}
</style>
