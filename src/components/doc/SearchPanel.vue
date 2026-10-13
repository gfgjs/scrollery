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
        @keydown.tab="!$event.shiftKey && onResultTab($event, 0)"
      />
    </div>

    <!-- 状态行：失败明确标为未完成；仅正常完成后展示命中计数或无结果。 -->
    <div class="search-panel__status">
      <template v-if="searching">{{ t('doc.searching') }} {{ Math.round(progress * 100) }}%</template>
      <span v-else-if="failed" role="alert">{{ t('doc.searchFailed') }}</span>
      <template v-else-if="submitted && totalHits > 0">{{ t('doc.searchHits', { n: totalHits }) }}</template>
      <template v-else-if="submitted">{{ t('doc.searchNoResults') }}</template>
    </div>

    <div ref="scrollEl" class="search-panel__list">
      <div class="search-panel__spacer" :style="{ height: `${totalSize}px` }">
        <div
          v-for="row in virtualRows"
          :key="row.key as number"
          :ref="measureRow"
          :data-index="row.index"
          class="search-panel__row"
          :class="{ 'search-panel__row--sticky': row.index === activeHeader }"
          :style="row.index === activeHeader ? undefined : { transform: `translateY(${row.start}px)` }"
        >
          <div v-if="row.entry.kind === 'section'" class="search-sec__label" :title="row.entry.label">
            {{ row.entry.label || '—' }}
          </div>
          <button
            v-else
            class="search-hit"
            @click="emit('navigate', row.entry.match.cfi)"
            @focus="focusedIndex = row.index"
            @blur="focusedIndex === row.index && (focusedIndex = -1)"
            @keydown="onResultTab($event, row.index)"
          >
            <span class="search-hit__ctx">{{ row.entry.match.excerpt.pre }}</span
            ><mark class="search-hit__mark">{{ row.entry.match.excerpt.match }}</mark
            ><span class="search-hit__ctx">{{ row.entry.match.excerpt.post }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// 书内搜索面板展示查询输入和按章分组的命中列表，并高亮命中片段。
// 搜索的驱动（调用 foliate 原生 search 生成器、流式收集）在 DocumentViewer；本面板只负责
// 采集查询（回车提交，emit 'search'）、渲染 results、点击命中 emit 'navigate'(cfi)。
import { ref, computed, onMounted, nextTick, watch } from 'vue'
import { defaultRangeExtractor, useVirtualizer } from '@tanstack/vue-virtual'
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
  /** 搜索未完整执行；保留已有命中并允许用户再次回车重试。 */
  failed: boolean
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
const scrollEl = ref<HTMLElement | null>(null)
const focusedIndex = ref(-1)

const totalHits = computed(() => props.results.reduce((n, s) => n + s.matches.length, 0))

type SearchRow = { kind: 'section'; label: string } | { kind: 'match'; match: FoliateSearchMatch }
const flattened = computed(() => {
  const rows: SearchRow[] = []
  const headers: number[] = []
  for (const section of props.results) {
    headers.push(rows.length)
    rows.push({ kind: 'section', label: section.label })
    for (const match of section.matches) rows.push({ kind: 'match', match })
  }
  return { rows, headers }
})

function headerAt(headers: number[], index: number): number {
  for (let i = headers.length - 1; i >= 0; i--) {
    if (headers[i] <= index) return headers[i]
  }
  return -1
}

const virtualizer = useVirtualizer(computed(() => {
  const { rows, headers } = flattened.value
  const focused = focusedIndex.value
  return {
    count: rows.length,
    getScrollElement: () => scrollEl.value,
    estimateSize: (index: number) => rows[index].kind === 'section' ? 28 : 50,
    overscan: 6,
    scrollPaddingStart: 28,
    rangeExtractor: (range: Parameters<typeof defaultRangeExtractor>[0]) => {
      const indices = defaultRangeExtractor(range)
      const header = headerAt(headers, range.startIndex)
      // 保留当前章节标题与焦点行，滚动回收窗口时不丢章节辨识和键盘焦点。
      if (header >= 0) indices.push(header)
      if (focused >= 0 && focused < rows.length) indices.push(focused)
      return [...new Set(indices)].sort((a, b) => a - b)
    },
  }
}))
const virtualRows = computed(() => virtualizer.value.getVirtualItems().map(row => ({ ...row, entry: flattened.value.rows[row.index] })))
const totalSize = computed(() => virtualizer.value.getTotalSize())
const activeHeader = computed(() => headerAt(flattened.value.headers, virtualizer.value.range?.startIndex ?? 0))

function measureRow(el: Element | { $el?: Element } | null) {
  const node = el instanceof Element ? el : el?.$el
  if (node instanceof Element) virtualizer.value.measureElement(node)
}

async function onResultTab(event: KeyboardEvent, index: number) {
  if (event.key !== 'Tab' || event.altKey || event.ctrlKey || event.metaKey) return
  const step = event.shiftKey ? -1 : 1
  let target = index + step
  while (flattened.value.rows[target]?.kind === 'section') target += step
  if (!flattened.value.rows[target]) return
  event.preventDefault()
  // 目标先进入窗口；旧行卸载的 blur 只能清除自己的焦点，不能覆盖这个目标。
  focusedIndex.value = target
  virtualizer.value.scrollToIndex(target, { align: 'auto' })
  await nextTick()
  scrollEl.value?.querySelector<HTMLButtonElement>(`[data-index="${target}"] .search-hit`)?.focus({ preventScroll: true })
}

watch(() => props.results[0], () => {
  // 流式追加沿用首组；首组替换意味着新查询，旧滚动位置与行高缓存不应带入。
  focusedIndex.value = -1
  virtualizer.value.scrollToOffset(0)
  virtualizer.value.measure()
})

watch(q, (value) => {
  if (value.trim() || !submitted.value) return
  // 清除按钮和删空输入都应立即作废旧查询；非空输入仍只在回车时搜索。
  submitted.value = false
  emit('search', '')
})

function submit(event: KeyboardEvent) {
  // 输入法确认候选也会发 Enter，此时 v-model 尚未同步最终文本。
  if (event.isComposing) return
  submitted.value = q.value.trim().length > 0
  emit('search', q.value)
}

/** 查找快捷键重复触发时选中原查询，方便直接改搜。 */
function focusQuery() {
  inputEl.value?.focus()
  inputEl.value?.select()
}

onMounted(focusQuery)
defineExpose({ focusQuery })
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
  min-height: 0;
  overflow-y: auto;
  padding: 0 0 6px;
}
.search-panel__spacer {
  position: relative;
}
.search-panel__row {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
}
.search-panel__row--sticky {
  position: sticky;
  z-index: 1;
  top: 0;
}
.search-sec__label {
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
