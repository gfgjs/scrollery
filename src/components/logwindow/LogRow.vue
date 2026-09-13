<script setup lang="ts">
// 单条日志行(日志能力重构 S4,方案 §5):级别徽标(颜色 + 文字双编码,WCAG 1.4.1)+ 时间戳 +
// target + msg + 非空 attributes 紧凑展开。纯展示组件,状态(是否选中)由父级传入,点击只 emit
// 事件不自持选中态(S5 P2「过滤命中↔原文上下文」双窗格,选中态由 logWindowStore.selectedSeq 单源)。
import { computed } from 'vue'
import type { LogEntry } from '../../types/logEntry'

const props = defineProps<{ entry: LogEntry; selected?: boolean }>()
const emit = defineEmits<{ select: [seq: number] }>()

const levelClass = computed(() => `log-row__level--${props.entry.level.toLowerCase()}`)

/** attributes 紧凑一行展开(`key=value` 空格分隔);空对象不渲染。 */
const attributesText = computed(() => {
  const entries = Object.entries(props.entry.attributes ?? {})
  if (entries.length === 0) return ''
  return entries
    .map(([k, v]) => `${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`)
    .join(' ')
})

const shortTime = computed(() => {
  // ts 形如 2026-07-20T21:03:11.284+08:00;日志窗口只要时分秒,日期在文件名/history 分组里已知。
  const m = /T(\d{2}:\d{2}:\d{2})/.exec(props.entry.ts)
  return m ? m[1] : props.entry.ts
})
</script>

<template>
  <div
    class="log-row"
    :class="{ 'log-row--selected': selected }"

    tabindex="0"
    @click="emit('select', entry._seq)"
    @keydown.enter="emit('select', entry._seq)"
  >
    <span class="log-row__level" :class="levelClass">{{ entry.level.toUpperCase() }}</span>
    <span class="log-row__ts">{{ shortTime }}</span>
    <span v-if="entry.target" class="log-row__target">{{ entry.target }}</span>
    <span class="log-row__msg">{{ entry.msg }}</span>
    <span v-if="attributesText" class="log-row__attrs">{{ attributesText }}</span>
  </div>
</template>

<style scoped>
.log-row {
  display: flex;
  align-items: baseline;
  gap: var(--spacing-sm);
  padding: var(--spacing-xs) var(--spacing-sm);
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  border-bottom: 1px solid var(--color-divider);
  cursor: pointer;
}
.log-row--selected {
  background: var(--color-accent-subtle);
  box-shadow: inset 2px 0 0 var(--color-accent);
}

.log-row__level {
  flex-shrink: 0;
  min-width: 3.6em;
  font-weight: 700;
  text-align: center;
  border-radius: var(--radius-xs);
}
/* 颜色 + 文字双编码(WCAG 1.4.1):不单靠色相区分级别,DEBUG/TRACE 用低饱和灰视觉降权(方案 §5)。 */
.log-row__level--trace {
  color: var(--color-text-tertiary);
  opacity: 0.75;
}
.log-row__level--debug {
  color: var(--color-text-tertiary);
}
.log-row__level--info {
  color: var(--color-accent);
}
.log-row__level--warn {
  color: var(--color-warning);
}
.log-row__level--error {
  color: var(--color-error);
}

.log-row__ts {
  flex-shrink: 0;
  color: var(--color-text-tertiary);
}

.log-row__target {
  flex-shrink: 0;
  color: var(--color-text-secondary);
  max-width: 22em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.log-row__msg {
  color: var(--color-text-primary);
  flex: 1 1 auto;
}

.log-row__attrs {
  color: var(--color-text-tertiary);
  flex-basis: 100%;
  padding-left: 3.6em;
}
</style>
