<template>
  <div class="toc-panel">
    <div class="toc-panel__head">
      <span class="toc-panel__title">{{ t('doc.toc') }}</span>
      <button
        class="toc-panel__x"
        @click="emit('close')"
        :title="t('common.close')"

      >
        <X :size="16" />
      </button>
    </div>

    <div class="toc-panel__list">
      <div v-if="!flatItems.length" class="toc-panel__empty">{{ t('doc.tocEmpty') }}</div>
      <button
        v-for="(item, i) in flatItems"
        :key="i"
        class="toc-item"
        :class="{ active: item.href === activeHref }"
        :style="{ paddingLeft: 12 + item.depth * 14 + 'px' }"
        :title="item.label"
        @click="emit('navigate', item.href)"
      >
        {{ item.label }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
// 目录（TOC）面板（阅读器方案 R2-3）。接收 foliate book.toc 树，扁平化为带深度的列表渲染
// （避免递归组件），点击 emit 'navigate'(href) 由 DocumentViewer 调 BookReader.goToHref 跳转。
import { computed } from 'vue'
import { X } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import type { FoliateTocItem } from '../../vendor/foliate-js/view.js'

const props = defineProps<{
  toc: FoliateTocItem[]
  /** 当前所在章的 href（高亮用，可选）。 */
  activeHref?: string
}>()

const emit = defineEmits<{
  (e: 'navigate', href: string): void
  (e: 'close'): void
}>()

const { t } = useI18n()

interface FlatTocItem {
  label: string
  href: string
  depth: number
}

// 深度优先扁平化：每项带 depth 供缩进；空 label 用占位符避免出现不可点的空行。
function flatten(items: FoliateTocItem[], depth: number, out: FlatTocItem[]): void {
  for (const it of items) {
    out.push({ label: it.label || '—', href: it.href ?? '', depth })
    if (it.subitems?.length) flatten(it.subitems, depth + 1, out)
  }
}

const flatItems = computed(() => {
  const out: FlatTocItem[] = []
  flatten(props.toc ?? [], 0, out)
  return out
})
</script>

<style scoped>
.toc-panel {
  display: flex;
  flex-direction: column;
  width: 300px;
  height: 100%;
  background: var(--color-bg-surface);
  border-left: 1px solid var(--color-border);
}
.toc-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid var(--color-border);
}
.toc-panel__title {
  font-weight: 600;
}
.toc-panel__x {
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.toc-panel__list {
  flex: 1;
  overflow-y: auto;
  padding: 6px 0;
}
.toc-panel__empty {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  padding: 16px 12px;
  text-align: center;
}
.toc-item {
  display: block;
  width: 100%;
  text-align: left;
  background: transparent;
  border: none;
  color: var(--color-text-primary);
  padding: 6px 12px;
  cursor: pointer;
  font-size: var(--font-size-sm);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.toc-item:hover {
  background: var(--color-bg-elevated);
}
.toc-item.active {
  color: var(--color-accent);
  font-weight: 600;
}
</style>
