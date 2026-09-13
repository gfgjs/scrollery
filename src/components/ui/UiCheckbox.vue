<script setup lang="ts">
// UiCheckbox — 表单复选框原语（S1 UI primitive）。
// 蒸馏 ConfirmDialog/CloseConfirmDialog 两处重复定义的 `.remember-checkbox` 视觉
// (native input 16×16 + `accent-color:var(--color-accent)` + flex 行 + gap 8px),零自绘 SVG,
// 选中/焦点态全由原生 checkbox + accent-color 驱动,无 JS 态。
// `<label>` 包裹隐式关联 label 文本与控件。
// 样式作用域私有(作用域类名 `.checkbox-field`),且类名刻意避开 `.checkbox`:全局 `.checkbox` 已被
// MediaThumb 网格选择控件(20px 圆 + 白勾)占用,若定义同名全局类会层叠泄漏污染那个控件。
defineProps<{
  /** 当前勾选值(v-model)。 */
  modelValue: boolean
  /** 文字标签;更复杂内容用默认插槽(插槽优先)。 */
  label?: string
  /** 禁用:落到原生 input,并加 `--disabled` 视觉。 */
  disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [boolean] }>()

// v-model on native checkbox 语义:读 checked、监听 change 写回布尔。
function onChange(e: Event) {
  emit('update:modelValue', (e.target as HTMLInputElement).checked)
}
</script>

<template>
  <label class="checkbox-field" :class="{ 'checkbox-field--disabled': disabled }">
    <input type="checkbox" :checked="modelValue" :disabled="disabled" @change="onChange" />
    <span class="checkbox-field__label"
      ><slot>{{ label }}</slot></span
    >
  </label>
</template>

<style scoped>
/* 蒸馏自 ConfirmDialog/CloseConfirmDialog 的 `.remember-checkbox`——字节复刻使二者迁移同构。 */
.checkbox-field {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  font-size: var(--font-size-sm);
  line-height: var(--leading-normal);
  color: var(--color-text-secondary);
  cursor: pointer;
  user-select: none;
}
.checkbox-field input[type='checkbox'] {
  width: var(--spacing-lg);
  height: var(--spacing-lg);
  cursor: pointer;
  accent-color: var(--color-accent);
  /* flex-shrink:0=防御性加固(长标签下 16px 方框不被挤扁);当前短标签消费者无挤压压力,视觉零变化。 */
  flex-shrink: 0;
}
.checkbox-field input[type='checkbox']:focus-visible {
  outline: var(--focus-ring-width) solid var(--color-focus-ring);
  outline-offset: var(--focus-ring-offset);
}
.checkbox-field--disabled {
  cursor: not-allowed;
  opacity: var(--opacity-disabled);
}
.checkbox-field--disabled input[type='checkbox'] {
  cursor: not-allowed;
}
</style>
