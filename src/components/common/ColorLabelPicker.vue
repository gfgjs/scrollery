<template>
  <!-- 颜色标签选择器（Part5 T16）：一行色块，点选设色、点当前色清零（toggle-off）。
       与 StarRating 同为「纯展示+交互、不内嵌 IPC」的可复用控件——副作用经 v-model / change 上抛。
       色块离散无序（不同于星级的「填充到 N」），故无 hover 预览填充逻辑。 -->
  <div class="color-picker" :class="{ 'color-picker--readonly': readonly }">
    <button
      v-for="c in COLOR_LABELS"
      :key="c.value"
      type="button"
      class="color-picker__swatch"
      :class="{ active: c.value === modelValue }"
      :style="{ '--swatch': c.hex, width: `${size}px`, height: `${size}px` }"
      :disabled="readonly"
      :title="t(c.name)"

      @click="onClick(c.value)"
    ></button>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { COLOR_LABELS } from '../../constants/colorLabels'

// COLOR_LABELS.name 存的是 i18n 键名（colorLabels.red…），展示时经 t() 解析。
const { t } = useI18n()

const props = withDefaults(
  defineProps<{
    /** 当前档位（0=未标 / 不筛选）。 */
    modelValue: number
    /** 色块边长（px）。 */
    size?: number
    /** 只读：仅展示，不可交互。 */
    readonly?: boolean
    /** 允许「点当前色清零」（清空标签 / 取消按色筛选）。默认开。 */
    allowClear?: boolean
  }>(),
  { size: 14, readonly: false, allowClear: true },
)

const emit = defineEmits<{
  'update:modelValue': [value: number]
  /** 用户显式改动（与 v-model 同值，便于父层做副作用如批量设色）。 */
  change: [value: number]
}>()

function onClick(v: number) {
  if (props.readonly) return
  // 点当前色 → 清零（toggle-off）；否则设为 v。
  const next = props.allowClear && v === props.modelValue ? 0 : v
  emit('update:modelValue', next)
  emit('change', next)
}
</script>

<style scoped>
.color-picker {
  display: inline-flex;
  gap: 3px;
}
.color-picker__swatch {
  padding: 0;
  border: 1.5px solid transparent;
  border-radius: 50%;
  background: var(--swatch);
  cursor: pointer;
  /* 未选中时稍暗、缩小，选中/hover 时实色放大 —— 凸显当前档位。 */
  opacity: 0.55;
  transition:
    opacity var(--transition-fast),
    transform var(--transition-fast),
    border-color var(--transition-fast);
}
.color-picker:not(.color-picker--readonly) .color-picker__swatch:hover {
  opacity: 1;
  transform: scale(1.15);
}
.color-picker__swatch.active {
  opacity: 1;
  /* 选中描白边（在任意底色上都可辨），并轻微放大。 */
  border-color: #fff;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.35);
  transform: scale(1.1);
}
.color-picker--readonly .color-picker__swatch {
  cursor: default;
}
</style>
