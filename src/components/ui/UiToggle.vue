<script setup lang="ts">
// UiToggle — 全局 `.toggle` 开关组件的类型化包裹（S1 UI primitive）。
// 收敛此前各处手写的裸三元 DOM 结构,复用已过 contrast 门禁的全局 `.toggle` / `.toggle__thumb` 样式;
// 选中/滑动/焦点态全走纯 CSS(`:has(input:checked)`、`input:checked ~ .toggle__thumb`、`:focus-within`),
// 无 JS 态,构造即视觉等价。
// compact 等作用域变体经 class fallthrough 合并到根 <label>:单根组件默认 inheritAttrs,父作用域 scoped
// 选择器借 child-root 携带的父 data-v 仍命中(与 UiIconButton 迁移同理)。
defineProps<{
  /** 当前开关值(v-model)。 */
  modelValue: boolean
  /** 禁用:落到原生 input 的 disabled,阻断交互。 */
  disabled?: boolean
  /** 原生开关的可访问名称。 */
  label?: string
}>()

const emit = defineEmits<{ 'update:modelValue': [boolean] }>()

// v-model on native checkbox 语义:读 checked、监听 change 写回布尔(与原 `v-model` 行为一致)。
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
