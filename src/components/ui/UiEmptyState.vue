<script setup lang="ts">
// UiEmptyState — 空状态原语（S1 UI primitive,设计 §9）：icon + title + description + 可选动作。
// 收敛各空场景的统一外壳,落实设计 §7.1「所有 empty state 都包含下一步」;同构包裹全局
// .empty-state* 类,视觉全交由全局 index.css,原语只管结构 + API。
// icon / actions 走具名插槽(消费方传定尺寸图标与 UiButton);title / description 走 props——
// 替代此前把「标题\n说明」两行塞进一个 i18n 串再按 '\n' 切分的脆弱约定(翻译须保留换行、单行键静默丢说明)。
defineProps<{
  /** 标题(必填,总是渲染)。 */
  title: string
  /** 可选说明文字(缺省不渲染说明行)。 */
  description?: string
}>()
</script>

<template>
  <div class="empty-state">
    <div v-if="$slots.icon" class="empty-state__icon"><slot name="icon" /></div>
    <div class="empty-state__title">{{ title }}</div>
    <div v-if="description" class="empty-state__desc">{{ description }}</div>
    <!-- 动作区(§9 primary+secondary):横向排布,可容 primary + secondary 两动作;容器样式见全局 index.css。 -->
    <div v-if="$slots.actions" class="empty-state__actions"><slot name="actions" /></div>
  </div>
</template>

<style scoped>
/* 空状态是内容区基座，不套卡片；只收敛密度与文字层级到共享 token。 */
.empty-state {
  gap: var(--spacing-md);
  padding: var(--spacing-xl);
  color: var(--color-text-secondary);
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
