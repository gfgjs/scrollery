<script setup lang="ts">
// 空状态的图标和动作由插槽提供，标题与说明由 props 提供；视觉使用全局 .empty-state 样式。
defineProps<{
  /** 标题(必填,总是渲染)。 */
  title: string
  /** 可选说明文字(缺省不渲染说明行)。 */
  description?: string
  /** 普通空态可显示主题静态装饰；加载、错误与重复分析保持关闭。 */
  decorative?: boolean
}>()
</script>

<template>
  <div class="empty-state" :class="{ 'empty-state--decorative': decorative }">
    <div v-if="decorative" class="empty-state__decorative" aria-hidden="true" />
    <div v-if="$slots.icon" class="empty-state__icon"><slot name="icon" /></div>
    <div class="empty-state__title">{{ title }}</div>
    <div v-if="description" class="empty-state__desc">{{ description }}</div>
    <!-- 动作区可并排放置主次动作。 -->
    <div v-if="$slots.actions" class="empty-state__actions"><slot name="actions" /></div>
  </div>
</template>

<style scoped>
/* 空状态是内容区基座，不套卡片；只收敛密度与文字层级到共享 token。 */
.empty-state {
  position: relative;
  gap: var(--spacing-md);
  padding: var(--spacing-xl);
  color: var(--color-text-secondary);
}

.empty-state__decorative {
  position: absolute;
  top: -48px;
  left: 50%;
  width: 120px;
  height: 60px;
  transform: translateX(-50%);
  pointer-events: none;
}

.empty-state__icon {
  color: var(--color-text-tertiary);
  opacity: 0.55;
}

.empty-state__title {
  font-size: var(--font-size-md);
  color: var(--color-text-primary);
}

.empty-state__desc {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  line-height: var(--leading-normal);
}

.empty-state__actions {
  gap: var(--spacing-sm);
  margin-top: var(--spacing-xs);
  flex-wrap: wrap;
}
</style>
