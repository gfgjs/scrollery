<script setup lang="ts">
// UiIconButton — 全局 `.btn-icon` 体系的类型化包裹（S1 UI primitive）。
// `label` 提为**必填 prop**，作为图标按钮的 title 提示源；active 视觉统一映射 `.btn-icon.active`。
// 样式仍复用已过 contrast 门禁的全局 `.btn-icon` / `.btn-icon.active`。
// @click / data-* / tabindex 等经 fallthrough 落到根 button；父作用域 scoped 后代选择器亦可命中根元素。
import { ref } from 'vue'

withDefaults(
  defineProps<{
    /** 图标按钮的提示文案（必填）：缺省 title 取该值。 */
    label: string
    /** 激活态:映射 `.btn-icon.active` 视觉(开关型的「按下」外观)。 */
    active?: boolean
    disabled?: boolean
    /** tooltip;缺省取 label。需要与 label 不同的提示(如附快捷键)时显式传入。 */
    title?: string
    /** 原生 type。默认 button,避免在 form 中意外提交。 */
    type?: 'button' | 'submit' | 'reset'
  }>(),
  {
    active: false,
    disabled: false,
    type: 'button',
  },
)

// 暴露根 <button> 供需 DOM 锚定的消费方使用(如 AppToolbar 弹层用 getBoundingClientRect 定位):
// ref 落在组件上取到的是组件实例,须经 defineExpose 显式暴露元素,消费方经 `ref.value?.el` 取用。
const el = ref<HTMLButtonElement | null>(null)
defineExpose({ el })
</script>

<template>
  <button
    ref="el"
    :type="type"
    class="btn-icon"
    :class="{ active }"
    :disabled="disabled"
    :title="title ?? label"
  >
    <slot />
  </button>
</template>

<style scoped>
/* 28px quiet icon button：透明常态只在 hover/active 时给出最小面层反馈。 */
.btn-icon {
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  min-width: var(--control-size-compact);
  min-height: var(--control-size-compact);
  padding: var(--spacing-xs);
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  line-height: 1;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast),
    box-shadow var(--transition-fast);
}

.btn-icon:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.btn-icon.active,
.btn-icon:active {
  background: var(--color-accent-subtle);
  color: var(--color-accent);
}

.btn-icon:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.btn-icon:disabled {
  cursor: not-allowed;
  opacity: var(--opacity-disabled);
}
</style>
