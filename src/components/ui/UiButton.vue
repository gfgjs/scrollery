<script setup lang="ts">
// UiButton — 全局 `.btn` 体系的类型化包裹（S1 UI primitive）。
// 消费方用 `variant` 语义而非裸 class 字符串,杜绝拼写漂移与 scoped 遮蔽
// (此前 SettingsView 的 scoped `.btn[data-v]` 会盖住全局变体,index.css:115-117 记)。
// 样式复用已过 contrast 门禁的全局 `.btn` / `.btn-{variant}`,本组件只加类型化 API + loading 契约。
// @click 等原生属性经 fallthrough 落到根 button;外部 class 由 Vue 合并到全局 class 上。

withDefaults(
  defineProps<{
    /** 视觉变体，对应全局 `.btn-{variant}`。 */
    variant?: 'primary' | 'secondary' | 'danger' | 'ghost'
    /** 加载态：显示 spinner 并禁用（防重复提交）。 */
    loading?: boolean
    disabled?: boolean
    /** 原生 type。默认 button，避免在 form 中意外提交。 */
    type?: 'button' | 'submit' | 'reset'
  }>(),
  {
    variant: 'secondary',
    loading: false,
    disabled: false,
    type: 'button',
  },
)
</script>

<template>
  <button
    :type="type"
    class="btn"
    :class="`btn-${variant}`"
    :disabled="disabled || loading"
  >
    <span v-if="loading" class="ui-btn__spinner" />
    <slot />
  </button>
</template>

<style scoped>
/* 控件基座：默认 32px 表单档；工具栏可用既有 `size="sm"`/`.btn-sm` fallthrough 走 28px 档。
   只改几何与交互反馈，变体颜色继续由全局 `.btn-{variant}` API 提供。 */
.btn {
  min-height: var(--control-size-default);
  height: var(--control-size-default);
  padding: 0 var(--control-padding-inline);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-sm);
  line-height: 1;
  transition:
    background-color var(--transition-fast),
    border-color var(--transition-fast),
    color var(--transition-fast),
    box-shadow var(--transition-fast);
}

/* 保留消费者已有的 compact 入口，不扩充组件 props/API。 */
.btn.btn-sm,
.btn[size='sm'] {
  min-height: var(--control-size-compact);
  height: var(--control-size-compact);
  padding-inline: var(--spacing-sm);
  font-size: var(--font-size-xs);
}

/* 暗房配方使用稳定的按压反馈，不缩放文字与图标，避免布局抖动。 */
.btn:active {
  transform: none;
}

.btn-primary {
  --theme-button-edge: var(--color-accent-hover);
}

.btn-secondary {
  --theme-button-edge: var(--color-border-strong);
}

.btn-primary,
.btn-secondary {
  box-shadow: 0 var(--theme-button-depth) 0 var(--theme-button-edge);
}

.btn-primary:active,
.btn-secondary:active {
  box-shadow: 0 calc(var(--theme-button-depth) * 0.5) 0 var(--theme-button-edge);
}

.btn:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.btn-primary:focus-visible,
.btn-secondary:focus-visible {
  box-shadow: var(--control-focus-ring), 0 var(--theme-button-depth) 0 var(--theme-button-edge);
}

.btn:disabled {
  cursor: not-allowed;
  opacity: var(--opacity-disabled);
}

.btn-primary:disabled,
.btn-secondary:disabled {
  box-shadow: none;
}

/* 加载指示器：currentColor 描边环,随按钮文字色自适应各变体/主题。 */
.ui-btn__spinner {
  width: 14px;
  height: 14px;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: ui-btn-spin var(--duration-spin) linear infinite;
  flex-shrink: 0;
}
@keyframes ui-btn-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
