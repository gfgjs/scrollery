<template>
  <div class="bm-panel">
    <div class="bm-panel__head">
      <span class="bm-panel__title">{{ t('doc.bookmarks') }}</span>
      <div class="bm-panel__actions">
        <button
          class="bm-panel__btn"
          @click="emit('add')"
          :title="t('doc.addBookmark')"

        >
          <BookmarkPlus :size="16" />
        </button>
        <button
          class="bm-panel__btn"
          @click="emit('close')"
          :title="t('common.close')"

        >
          <X :size="16" />
        </button>
      </div>
    </div>

    <div class="bm-panel__list">
      <div v-if="!bookmarks.length" class="bm-panel__empty">{{ t('doc.bookmarksEmpty') }}</div>
      <div v-for="bm in bookmarks" :key="bm.id" class="bm-item">
        <button class="bm-item__go" :title="bm.label" @click="emit('navigate', bm.locator)">
          <span class="bm-item__label">{{ bm.label || t('doc.bookmarkUntitled') }}</span>
          <span class="bm-item__pct">{{ Math.round(bm.fraction * 100) }}%</span>
        </button>
        <button
          class="bm-item__del"
          @click="emit('delete', bm.id)"
          :title="t('doc.removeBookmark')"

        >
          <Trash2 :size="14" />
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// 书签面板（阅读器方案 R4，§6.2）。纯展示：列出某书书签（章名 + 全书百分比，按进度升序，
// 后端已排序）。头部「+」添加当前位置（emit 'add'），点击条目 emit 'navigate'(locator) 跳转，
// 垃圾桶 emit 'delete'(id) 删除。数据加载/持久化在 DocumentViewer。
import { X, BookmarkPlus, Trash2 } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import type { ReaderBookmark } from '../../types/reader'

defineProps<{
  /** 书签列表（后端按 fraction 升序）。 */
  bookmarks: ReaderBookmark[]
}>()

const emit = defineEmits<{
  (e: 'add'): void
  (e: 'navigate', locator: string): void
  (e: 'delete', id: number): void
  (e: 'close'): void
}>()

const { t } = useI18n()
</script>

<style scoped>
.bm-panel {
  display: flex;
  flex-direction: column;
  width: 300px;
  height: 100%;
  background: var(--color-bg-surface);
  border-left: 1px solid var(--color-divider);
}
.bm-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
}
.bm-panel__title {
  font-weight: 600;
}
.bm-panel__actions {
  display: inline-flex;
  gap: var(--spacing-xs);
}
.bm-panel__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.bm-panel__btn:hover {
  color: var(--color-text-primary);
}
.bm-panel__list {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-xs) 0;
}
.bm-panel__empty {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  padding: var(--spacing-xl) var(--spacing-md);
  text-align: center;
}
.bm-item {
  display: flex;
  align-items: center;
}
.bm-item__go {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: var(--spacing-sm);
  text-align: left;
  background: transparent;
  border: none;
  color: var(--color-text-primary);
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-xs) 0 var(--spacing-md);
  cursor: pointer;
  font-size: var(--font-size-sm);
}
.bm-item__go:hover {
  background: var(--color-bg-hover);
}
.bm-item__label {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.bm-item__pct {
  flex: 0 0 auto;
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.bm-item__del {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-compact);
  height: var(--control-size-compact);
  margin-right: var(--spacing-xs);
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
  opacity: 0;
  transition: opacity var(--transition-fast);
}
/* 悬停条目才显删除键，避免误删 + 视觉整洁。 */
.bm-item:hover .bm-item__del {
  opacity: 1;
}
.bm-item__del:hover {
  /* 原 var(--color-danger, #d9534f) 引用不存在的幽灵 token,一直走 fallback 的固定红、
     不随主题走(与 S5 三处同族修法一致)(S7 修)。 */
  color: var(--color-error);
}
</style>
