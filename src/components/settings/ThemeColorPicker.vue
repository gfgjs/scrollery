<template>
  <div class="color-field">
    <span class="color-field__label">{{ label }}</span>
    <button
      ref="triggerEl"
      type="button"
      class="color-field__swatch"
      :style="{ background: modelValue }"
      :aria-label="label"
      :aria-expanded="open"
      @click="open = !open"
    />
    <input
      v-model="hexDraft"
      class="color-field__hex"
      type="text"
      spellcheck="false"
      inputmode="text"
      maxlength="7"
      :aria-label="t('settings.themeHexInput', { label })"
      @input="onHexInput"
      @blur="onHexBlur"
      @keydown.enter.prevent="onHexBlur"
    />

    <UiPopover :open="open" :anchor="triggerEl" @update:open="open = $event">
      <div class="color-panel">
        <div
          class="color-panel__area"
          :style="{ background: hueColor(hsv.h) }"
          role="slider"
          tabindex="0"
          :aria-label="t('settings.themeSaturationValue')"
          :aria-valuetext="modelValue"
          @pointerdown="onAreaPointerDown"
          @keydown="onAreaKeydown"
        >
          <span
            class="color-panel__thumb"
            :style="{ left: hsv.s * 100 + '%', top: (1 - hsv.v) * 100 + '%' }"
          />
        </div>

        <input
          class="color-panel__hue"
          type="range"
          min="0"
          max="359"
          step="1"
          :value="Math.round(hsv.h)"
          :aria-label="t('settings.themeHue')"
          @input="onHueInput"
        />

        <div class="color-panel__swatches">
          <button
            v-for="color in COMMON_COLORS"
            :key="color"
            type="button"
            class="color-panel__swatch"
            :class="{ active: color === modelValue }"
            :style="{ background: color }"
            :aria-label="color"
            @click="pickColor(color)"
          />
        </div>
      </div>
    </UiPopover>
  </div>
</template>

<script setup lang="ts">
// 选色器(方案 §2):二维饱和度/明度面板 + 色相条 + HEX 输入 + 常用色。
// 拖动即时上抛预览;HEX 未完成不发布,失焦恢复最后有效值。弹层收起只收起控件,主题草稿不受影响。
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import UiPopover from '../ui/UiPopover.vue'
import { normalizeHex } from '../../themes/colors'
import { hexToHsv, hueColor, hsvToHex, type Hsv } from './colorPickerMath'

const props = defineProps<{
  modelValue: string
  label: string
}>()

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const { t } = useI18n()

/** 常用色:中性两端 + 常用强调色,作为快速起点而非预设主题的一部分。 */
const COMMON_COLORS: readonly string[] = [
  '#ffffff',
  '#f4f4f5',
  '#d4d4d8',
  '#71717a',
  '#27272a',
  '#181818',
  '#000000',
  '#087f5b',
  '#34d399',
  '#1d4ed8',
  '#60a5fa',
  '#e0a458',
  '#b21f26',
  '#8e24aa',
]

const open = ref(false)
const triggerEl = ref<HTMLElement | null>(null)
const hsv = ref<Hsv>(hexToHsv(props.modelValue))
const hexDraft = ref(props.modelValue)
/** 本组件刚上抛的值:回流时不再重解析,避免灰阶(饱和度/明度为 0)把色相归零。 */
let lastEmitted: string | null = null

watch(
  () => props.modelValue,
  (value) => {
    if (value !== lastEmitted) hsv.value = hexToHsv(value)
    if (value !== normalizeHex(hexDraft.value)) hexDraft.value = value
  },
  { immediate: true },
)

function publish(next: Hsv): void {
  hsv.value = next
  lastEmitted = hsvToHex(next.h, next.s, next.v)
  emit('update:modelValue', lastEmitted)
}

/** 面板坐标系 → 饱和度(横)与明度(纵,上端为满明度)。 */
function fromPointer(event: PointerEvent): Hsv {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
  return {
    h: hsv.value.h,
    s: (event.clientX - rect.left) / rect.width,
    v: 1 - (event.clientY - rect.top) / rect.height,
  }
}

function onAreaPointerDown(event: PointerEvent): void {
  const area = event.currentTarget as HTMLElement
  area.setPointerCapture(event.pointerId)
  publish(fromPointer(event))
  const move = (moveEvent: PointerEvent) => publish(fromPointer(moveEvent))
  const stop = () => {
    area.removeEventListener('pointermove', move)
    area.removeEventListener('pointerup', stop)
    area.removeEventListener('pointercancel', stop)
  }
  area.addEventListener('pointermove', move)
  area.addEventListener('pointerup', stop)
  area.addEventListener('pointercancel', stop)
}

/** 方向键调饱和度/明度,Shift 加大步长(键盘与指针同能力)。 */
function onAreaKeydown(event: KeyboardEvent): void {
  const step = event.shiftKey ? 0.1 : 0.02
  const current = hsv.value
  if (event.key === 'ArrowLeft') publish({ ...current, s: current.s - step })
  else if (event.key === 'ArrowRight') publish({ ...current, s: current.s + step })
  else if (event.key === 'ArrowUp') publish({ ...current, v: current.v + step })
  else if (event.key === 'ArrowDown') publish({ ...current, v: current.v - step })
  else return
  event.preventDefault()
}

function onHueInput(event: Event): void {
  const hue = Number((event.target as HTMLInputElement).value)
  publish({ ...hsv.value, h: hue, s: hsv.value.s || 1, v: hsv.value.v || 1 })
}

function pickColor(color: string): void {
  lastEmitted = color
  hsv.value = hexToHsv(color)
  hexDraft.value = color
  emit('update:modelValue', color)
}

/** 输入过程中已完成的 HEX 立即预览;未完成不发布。 */
function onHexInput(): void {
  const parsed = normalizeHex(hexDraft.value)
  if (!parsed) return
  lastEmitted = parsed
  hsv.value = hexToHsv(parsed)
  emit('update:modelValue', parsed)
}

function onHexBlur(): void {
  hexDraft.value = props.modelValue
}
</script>

<style scoped>
.color-field {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.color-field__label {
  min-width: 4.5em;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}

.color-field__swatch {
  width: 24px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.color-field__swatch:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.color-field__hex {
  width: 84px;
  padding: 2px 6px;
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
}

.color-panel {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  width: 232px;
  padding: var(--spacing-sm);
}

/* 二维面板:底色为纯色相,横向叠加白→透明(饱和度)、纵向叠加透明→黑(明度)。 */
.color-panel__area {
  position: relative;
  height: 132px;
  border-radius: var(--radius-sm);
  background-image:
    linear-gradient(to top, #000000, rgba(0, 0, 0, 0)),
    linear-gradient(to right, #ffffff, rgba(255, 255, 255, 0));
  cursor: crosshair;
  touch-action: none;
}

.color-panel__area:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.color-panel__thumb {
  position: absolute;
  width: 12px;
  height: 12px;
  margin: -6px 0 0 -6px;
  border: 2px solid #ffffff;
  border-radius: 50%;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5);
  pointer-events: none;
}

.color-panel__hue {
  width: 100%;
  height: 12px;
  appearance: none;
  border-radius: var(--radius-xs);
  background: linear-gradient(
    to right,
    #ff0000,
    #ffff00,
    #00ff00,
    #00ffff,
    #0000ff,
    #ff00ff,
    #ff0000
  );
  cursor: pointer;
}

.color-panel__hue::-webkit-slider-thumb {
  appearance: none;
  width: 12px;
  height: 12px;
  border: 2px solid #ffffff;
  border-radius: 50%;
  background: transparent;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5);
}

.color-panel__hue::-moz-range-thumb {
  width: 12px;
  height: 12px;
  border: 2px solid #ffffff;
  border-radius: 50%;
  background: transparent;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5);
}

.color-panel__swatches {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: var(--spacing-2xs);
}

.color-panel__swatch {
  height: 20px;
  padding: 0;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-xs);
  cursor: pointer;
}

.color-panel__swatch.active {
  box-shadow: 0 0 0 2px var(--color-accent);
}
</style>
