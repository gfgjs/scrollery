<script setup lang="ts">
// 复用全局 `.toggle` 样式；选中、滑动和焦点态由 CSS 处理。
// compact 等变体通过 class fallthrough 合并到根 <label>，父组件的 scoped 样式仍可命中。
defineProps<{
  /** 当前开关值(v-model)。 */
  modelValue: boolean
  /** 禁用:落到原生 input 的 disabled,阻断交互。 */
  disabled?: boolean
  /** 原生开关的可访问名称。 */
  label?: string
}>()

const emit = defineEmits<{ 'update:modelValue': [boolean] }>()

// 按原生 checkbox 的 change 事件回写布尔值。
function onChange(e: Event) {
  emit('update:modelValue', (e.target as HTMLInputElement).checked)
}
</script>

<template>
  <label class="toggle">
    <input
      type="checkbox"
      :aria-label="label"
      :checked="modelValue"
      :disabled="disabled"
      @change="onChange"
    />
    <span class="toggle__thumb" />
  </label>
</template>

<style scoped>
.toggle {
  width: var(--toggle-width);
  height: var(--toggle-height);
  border-radius: var(--radius-full);
}

.toggle:has(input:disabled) {
  cursor: not-allowed;
  opacity: var(--opacity-disabled);
}
</style>
