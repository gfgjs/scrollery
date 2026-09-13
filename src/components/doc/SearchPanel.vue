<template>
  <div class="search-panel">
    <div class="search-panel__head">
      <span class="search-panel__title">{{ t('doc.search') }}</span>
      <button
        class="search-panel__x"
        @click="emit('close')"
        :title="t('common.close')"

      >
        <X :size="16" />
      </button>
    </div>

    <!-- 查询输入：回车触发（全书搜索开销大，不做逐键 debounce，交用户显式提交）。 -->
    <div class="search-panel__query">
      <Search :size="15" class="search-panel__query-icon" />
      <input
        ref="inputEl"
        v-model="q"
        class="search-panel__input"
        type="search"
        :placeholder="t('doc.searchPlaceholder')"
        @keydown.enter="submit"
      />
    </div>

    <!-- 状态行：搜索中显示进度百分比；完成且无命中显示无结果；有命中显示计数。 -->
    <div class="search-panel__status">
      <template v-if="searching">{{ t('doc.searching') }} {{ Math.round(progress * 100) }}%</template>
      <template v-else-if="submitted && totalHits > 0">{{ t('doc.searchHits', { n: totalHits }) }}</template>
      <template v-else-if="submitted">{{ t('doc.searchNoResults') }}</template>
    </div>

    <div class="search-panel__list">
      <div v-for="(sec, si) in results" :key="si" class="search-sec">
        <div class="search-sec__label" :title="sec.label">{{ sec.label || '—' }}</div>
        <button
          v-for="(m, mi) in sec.matches"
          :key="mi"
          class="search-hit"
          @click="emit('navigate', m.cfi)"
        >
          <span class="search-hit__ctx">{{ m.excerpt.pre }}</span
          ><mark class="search-hit__mark">{{ m.excerpt.match }}</mark
          ><span class="search-hit__ctx">{{ m.excerpt.post }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// 书内搜索面板（阅读器方案 R4）。纯展示：查询输入 + 按章分组的命中列表（命中片段 <mark> 高亮）。
// 搜索的驱动（调用 foliate 原生 search 生成器、流式收集）在 DocumentViewer；本面板只负责
// 采集查询（回车提交，emit 'search'）、渲染 results、点击命中 emit 'navigate'(cfi)。
import { ref, computed, onMounted } from 'vue'
import { X, Search } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import type { FoliateSearchMatch } from '../../vendor/foliate-js/view.js'

/** 一章的命中组（label=章名，来自 foliate search 的 tocProgress）。 */
export interface SearchSection {
  label: string
  matches: FoliateSearchMatch[]
}

const props = defineProps<{
  /** 命中结果，按章分组（DocumentViewer 流式追加）。 */
  results: SearchSection[]
  /** 是否搜索进行中（显示进度）。 */
  searching: boolean
  /** 扫描进度 0..1。 */
  progress: number
}>()

const emit = defineEmits<{
  (e: 'search', query: string): void
  (e: 'navigate', cfi: string): void
  (e: 'close'): void
}>()

const { t } = useI18n()

const q = ref('')
// 是否已提交过一次查询：控制「无结果」文案只在搜索后出现，而非初始空面板。
const submitted = ref(false)
const inputEl = ref<HTMLInputElement | null>(null)

const totalHits = computed(() => props.results.reduce((n, s) => n + s.matches.length, 0))

function submit() {
  submitted.value = q.value.trim().length > 0
  emit('search', q.value)
}

// 打开面板即聚焦输入，省一次点击。
onMounted(() => inputEl.value?.focus())
</script>

<style scoped>
.search-panel {
  display: flex;
  flex-direction: column;
  width: 300px;
  height: 100%;
  background: var(--color-bg-surface);
  border-left: 1px solid var(--color-divider);
}
.search-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
}
.search-panel__title {
  font-weight: 600;
}
.search-panel__x {
  background: transparent;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.search-panel__query {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-sm) var(--spacing-md) var(--spacing-xs);
}
.search-panel__query-icon {
  flex: 0 0 auto;
  color: var(--color-text-secondary);
}
.search-panel__input {
  flex: 1;
  min-width: 0;
  background: var(--color-input-bg);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  font-size: var(--font-size-sm);
}
.search-panel__status {
  min-height: 18px;
  padding: 0 var(--spacing-md) var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.search-panel__list {
  flex: 1;
  overflow-y: auto;
  padding: 0 0 6px;
}
.search-sec__label {
  position: sticky;
  top: 0;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-md);
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--color-text-secondary);
  background: var(--color-bg-surface);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.search-hit {
  width: 100%;
  text-align: left;
  background: transparent;
  border: none;
  color: var(--color-text-primary);
  padding: var(--spacing-xs) var(--spacing-md);
  cursor: pointer;
  font-size: var(--font-size-sm);
  line-height: 1.5;
  /* 命中片段可能较长：限两行，超出省略，避免撑爆面板。 */
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.search-hit:hover {
  background: var(--color-bg-hover);
}
.search-hit__ctx {
  color: var(--color-text-secondary);
}
.search-hit__mark {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  border-radius: var(--radius-xs);
  padding: 0 var(--spacing-2xs);
}
</style>
