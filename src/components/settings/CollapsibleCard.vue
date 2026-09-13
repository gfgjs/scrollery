<!-- 可折叠的设置分组：复用全局设置行契约，标题行点击折叠/展开，状态按 id 持久化。 -->
<template>
  <div class="settings-card" :class="{ 'settings-card--collapsed': !open }">
    <div
      class="settings-card__header settings-card__header--toggle"

      tabindex="0"

      @click="toggle"
      @keydown.enter.prevent="toggle"
      @keydown.space.prevent="toggle"
    >
      <ChevronRight :size="14" class="settings-card__chevron" :class="{ expanded: open }" />
      <span class="settings-card__header-title"
        ><slot name="title">{{ title }}</slot></span
      >
      <!-- 可选右侧操作区；此处点击不触发折叠。 -->
      <span v-if="$slots.actions" class="settings-card__header-actions" @click.stop
        ><slot name="actions"
      /></span>
    </div>

    <!-- 折叠动画：grid 行高 1fr↔0fr，内层裁剪溢出；DOM 常驻不卸载。 -->
    <!-- Collapse via grid 1fr↔0fr; inner clips overflow; DOM stays mounted. -->
    <div class="settings-card__body">
      <div class="settings-card__body-inner">
        <slot />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { useSettingsCards } from '../../composables/useSettingsCards'

const props = defineProps<{
  /** 持久化展开状态的稳定键 | stable key for persisting expand-state */
  id: string
  /** 标题文本（也可用 #title 插槽覆盖）| header text (or override via #title slot) */
  title?: string
  /** 首次（无持久化值时）是否展开，默认展开 | default open when no stored value */
  defaultOpen?: boolean
}>()
const emit = defineEmits<{
  toggle: [open: boolean]
}>()

// 展开状态交由全局协调器管理，使「一键全部折叠/展开」能跨组件作用。
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
/* 标题行改为可点击的开合控件（基础样式来自全局 .settings-card__header）。 */
.settings-card__header--toggle {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  cursor: pointer;
  user-select: none;
  /* 展开时显示分隔线（折叠时透明，避免遗留 1px 线）。 */
  border-bottom: 1px solid transparent;
  transition:
    background var(--transition-fast),
    color var(--transition-fast),
    border-color var(--transition-fast);
}
.settings-card:not(.settings-card--collapsed) .settings-card__header--toggle {
  border-bottom-color: var(--color-divider);
}
.settings-card__header--toggle:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-secondary);
}
.settings-card__header--toggle:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -2px;
}
.settings-card__chevron {
  flex-shrink: 0;
  transition: transform 0.2s;
}
.settings-card__chevron.expanded {
  transform: rotate(90deg);
}
.settings-card__header-title {
  flex: 1;
  min-width: 0;
}
.settings-card__header-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  cursor: default;
}

.settings-card__body {
  display: grid;
  grid-template-rows: 1fr;
  transition: grid-template-rows var(--duration-moderate) var(--ease-in-out);
}
.settings-card--collapsed .settings-card__body {
  grid-template-rows: 0fr;
}
.settings-card__body-inner {
  overflow: hidden;
  min-width: 0;
}
</style>
