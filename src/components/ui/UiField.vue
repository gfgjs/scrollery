<script setup lang="ts">
// UiField — 表单字段原语（S1 UI primitive）：label + 单个控件（input/select/textarea/range）的统一外壳。
//
// 目的：收敛全库 6 种碎裂的字段包裹范式（清点见 findings 会话续 8），从类型层保证 label 与控件的
// label 关联——修 A/D/E/F 四类「label 未关联（兄弟 label / span 伪 label）」与「无 label（仅 placeholder）」缺陷。
// 样式为本原语自持的轻量基座（各消费方原有 scoped 字段类迁移后删除）。
//
// 两种关联模式（用户裁决：双模并存）：
//   · nest（默认）：<label> 物理包裹「caption + 控件」→ 隐式关联，无需 id。适合控件可被 label 包裹的多数场景。
//   · for：<label for=id> 与控件[id] 分离 → 显式关联。适合控件不便被包裹、或 label 与控件跨容器的场景。
// 控件走默认插槽；插槽暴露 { id }：for 模式须把 id 绑到控件。
// hint/error 置于 <label> 之外（root 下、group 同级）。
import { computed, useId } from 'vue'

const props = withDefaults(
  defineProps<{
    label: string // 字段标签文本
    associate?: 'nest' | 'for' // 关联方式：nest=包裹 label（默认）/ for=显式 label[for]+控件[id]
    fieldId?: string // for 模式的控件 id；缺省用 useId() 自动生成（SSR 安全、跨渲染稳定）
    orientation?: 'stacked' | 'inline' // caption 在控件上方（默认）/ 在左
    hint?: string // 可选帮助文字（tertiary 色）
    error?: string // 可选错误文字（error 色）
  }>(),
  { associate: 'nest', orientation: 'stacked' },
)

// for 模式的控件 id：优先用传入 fieldId，否则 useId() 生成。hint/error 的 id 由它派生。
const autoId = useId()
const controlId = computed(() => props.fieldId ?? autoId)
const hintId = computed(() => `${controlId.value}-hint`)
const errorId = computed(() => `${controlId.value}-error`)
</script>

<template>
  <div class="ui-field" :class="{ 'ui-field--error': error }">
    <!-- group：caption + 控件的容器兼关联载体。nest 模式为 <label>（隐式包裹关联）；for 模式为 <div>。 -->
    <component
      :is="associate === 'nest' ? 'label' : 'div'"
      class="ui-field__group"
      :class="`ui-field__group--${orientation}`"
    >
      <!-- caption：nest 模式为 <span>（label 已在外层）；for 模式为 <label for=id>（显式关联）。 -->
      <component
        :is="associate === 'nest' ? 'span' : 'label'"
        class="ui-field__label"
        :for="associate === 'for' ? controlId : undefined"
        >{{ label }}</component
      >
      <!-- 控件插槽：for 模式须绑 :id="id"。 -->
      <slot :id="associate === 'for' ? controlId : undefined" />
    </component>
    <span v-if="hint" :id="hintId" class="ui-field__hint">{{ hint }}</span>
    <span v-if="error" :id="errorId" class="ui-field__error">{{ error }}</span>
  </div>
</template>

<style scoped>
.ui-field {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  min-width: 0;
}
.ui-field__group {
  display: flex;
  min-width: 0;
}
.ui-field__group--stacked {
  flex-direction: column;
  gap: var(--spacing-xs);
}
.ui-field__group--inline {
  flex-direction: row;
  align-items: center;
  gap: var(--spacing-sm);
}
.ui-field__label {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
/* 横排时 label 不随控件收缩，保持文案完整。 */
.ui-field__group--inline .ui-field__label {
  flex-shrink: 0;
}
.ui-field__hint {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  line-height: 1.4;
}
.ui-field__error {
  font-size: var(--font-size-sm);
  color: var(--color-error);
  line-height: var(--leading-normal);
}

/* 字段只约束常规表单控件；checkbox/radio/range 保留原生或专用交互尺寸。 */
.ui-field :deep(.input),
.ui-field :deep(.input-text),
.ui-field :deep(.input-number),
.ui-field :deep(input:not([type='checkbox']):not([type='radio']):not([type='range'])),
.ui-field :deep(select),
.ui-field :deep(textarea) {
  box-sizing: border-box;
  min-height: var(--control-size-default);
  padding: var(--spacing-xs) var(--control-padding-inline);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-base);
  transition:
    border-color var(--transition-fast),
    box-shadow var(--transition-fast),
    background-color var(--transition-fast);
}

.ui-field :deep(.input:focus),
.ui-field :deep(.input-text:focus),
.ui-field :deep(.input-number:focus),
.ui-field :deep(input:not([type='checkbox']):not([type='radio']):not([type='range']):focus),
.ui-field :deep(select:focus),
.ui-field :deep(textarea:focus) {
  border-color: var(--color-input-border-focus, var(--color-accent));
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.ui-field :deep(input:disabled),
.ui-field :deep(select:disabled),
.ui-field :deep(textarea:disabled) {
  cursor: not-allowed;
  opacity: var(--opacity-disabled);
}

.ui-field :deep(.select) {
  min-height: var(--control-size-default);
}
</style>
