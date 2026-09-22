<script setup lang="ts">
// UiSelect — 全局 `.select` 下拉的类型化包裹（S1 UI primitive）。
// 结构 = .select-wrap（含 ::after 下拉箭头）> select.select > <slot> 选项。
// 选项经默认插槽由消费方渲染（保持 i18n / optgroup 灵活性,原语不依赖 i18n keys）。
// v-model 经 writable computed 走 Vue 规范的 vModelSelect 指令,选项选中/change 读值均由其处理。
import { computed } from 'vue'

const props = defineProps<{
  modelValue: string
  disabled?: boolean
  /** 原生选择框的可访问名称。 */
  label?: string
}>()
const emit = defineEmits<{ 'update:modelValue': [string] }>()

// 桥接 prop→v-model:读回 modelValue,写时向父 emit(不直接改 prop)。
const selected = computed<string>({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
})
</script>

<template>
  <div class="select-wrap">
    <select v-model="selected" class="select" :disabled="disabled" :aria-label="label">
      <slot />
    </select>
  </div>
</template>

<style scoped>
.select-wrap {
  position: relative;
  display: inline-flex;
  min-width: 0;
  max-width: 100%;
}

.select {
  box-sizing: border-box;
  height: var(--select-height, var(--control-size-default));
  min-height: var(--control-size-default);
  padding: 0 32px 0 var(--control-padding-inline);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  border-color: var(--color-input-border);
  transition:
    border-color var(--transition-fast),
    background-color var(--transition-fast),
    box-shadow var(--transition-fast);
}

.select:focus-visible {
  border-color: var(--color-input-border-focus, var(--color-accent));
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.select:disabled {
  cursor: not-allowed;
  opacity: var(--opacity-disabled);
}

/* DynamicSettingControl 已有 compact-select-wrap fallthrough；统一到 28px 档。 */
.select-wrap.compact-select-wrap .select {
  height: var(--control-size-compact);
  min-height: var(--control-size-compact);
  padding-inline-start: var(--spacing-sm);
  font-size: var(--font-size-xs);
}

.select-wrap::after {
  right: var(--spacing-sm);
}
</style>
