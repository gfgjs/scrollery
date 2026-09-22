<template>
  <div class="settings-card" :class="{ 'settings-card--collapsed': !open }" :data-settings-card="id">
    <div class="settings-card__header">
      <button type="button" class="settings-card__header-toggle" :aria-expanded="open"
        :aria-controls="`settings-card-body-${id}`" @click="toggle">
        <span class="settings-card__heading-text">
          <span class="settings-card__header-title"><slot name="title">{{ title }}</slot></span>
          <span v-if="!open && summary" class="settings-card__summary">{{ summary }}</span>
        </span>
        <ChevronRight :size="16" class="settings-card__chevron" :class="{ expanded: open }" />
      </button>
      <div v-if="$slots.actions" class="settings-card__header-actions"><slot name="actions" /></div>
    </div>
    <!-- 常驻 DOM 保留编辑与任务状态；收起时隐藏，键盘不会进入不可见控件，搜索可立即定位。 -->
    <div v-show="open" :id="`settings-card-body-${id}`" class="settings-card__body"><slot /></div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { useSettingsCards } from '../../composables/useSettingsCards'
const props = withDefaults(defineProps<{
  /** 分组展开状态的持久化键，与配置 schema 一致。 */
  id: string
  title?: string
  /** 收起时仍可查看当前参数。 */
  summary?: string
  /** 权威快照到达前的默认展开状态。 */
  defaultOpen?: boolean
}>(), { defaultOpen: true })
const emit = defineEmits<{ toggle: [open: boolean] }>()
const cards = useSettingsCards()
onMounted(() => cards.register(props.id, props.defaultOpen ?? true))
onUnmounted(() => cards.unregister(props.id))
const open = computed(() => cards.isOpen(props.id))
function toggle() {
  cards.toggle(props.id)
  emit('toggle', open.value)
}
</script>

<style scoped>
.settings-card__header {
  display: flex;
  align-items: center;
  padding: 0;
  text-transform: none;
  letter-spacing: normal;
}
.settings-card__header-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-md);
  flex: 1;
  min-width: 0;
  padding: var(--spacing-md) var(--spacing-lg);
  border: 0;
  background: transparent;
  color: var(--color-text-primary);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.settings-card__header-toggle:hover { background: var(--color-bg-hover); }
.settings-card__header-toggle:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -3px;
}
.settings-card__heading-text { display: grid; gap: var(--spacing-xs); min-width: 0; }
.settings-card__header-title { font-weight: 500; }
.settings-card__summary {
  font-size: var(--font-size-xs);
  font-weight: 400;
  color: var(--color-text-secondary);
  overflow-wrap: anywhere;
}
.settings-card__chevron {
  flex-shrink: 0;
  color: var(--color-text-tertiary);
  transition: transform var(--transition-fast);
}
.settings-card__chevron.expanded { transform: rotate(90deg); }
.settings-card__header-actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding-right: var(--spacing-lg);
}
.settings-card__body { border-top: 1px solid var(--color-divider); }
</style>
