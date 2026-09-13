<script setup lang="ts">
// 时间线直方图 + 级别分布(日志能力重构 S5 P2,方案 §5「时间线直方图+级别分布」)。
// 手写内联 SVG 堆叠柱状图——不引图表库(§9.1 护栏 #1「此外一律不引」精神下 S5 同样不新增前端依赖),
// 数据来自后端 compute_log_histogram(rusqlite 临时表 GROUP BY strftime 分桶),本组件只管渲染。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

interface HistogramBucket {
  bucket: string
  level: string
  count: number
}

const props = defineProps<{ buckets: HistogramBucket[]; bucket: 'hour' | 'day' }>()
const { t } = useI18n()

// 固定级别顺序(低频→高频视觉权重从下往上堆叠,error 置顶最显眼)。
const LEVEL_ORDER = ['trace', 'debug', 'info', 'warn', 'error'] as const
const LEVEL_COLOR: Record<string, string> = {
  trace: 'var(--color-text-tertiary)',
  debug: 'var(--color-text-tertiary)',
  info: 'var(--color-accent)',
  warn: 'var(--color-warning)',
  error: 'var(--color-error)',
}

interface StackedBucket {
  label: string
  total: number
  segments: Array<{ level: string; count: number; offset: number }>
}

const stacked = computed<StackedBucket[]>(() => {
  const byBucket = new Map<string, Map<string, number>>()
  for (const b of props.buckets) {
    const level = b.level.toLowerCase()
    if (!byBucket.has(b.bucket)) byBucket.set(b.bucket, new Map())
    const levels = byBucket.get(b.bucket)
    if (!levels) continue
    levels.set(level, (levels.get(level) ?? 0) + b.count)
  }
  return Array.from(byBucket.entries())
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([bucket, levels]) => {
      let offset = 0
      const segments = LEVEL_ORDER.filter((lv) => levels.has(lv)).map((lv) => {
        const count = levels.get(lv) ?? 0
        const seg = { level: lv, count, offset }
        offset += count
        return seg
      })
      return { label: bucket, total: offset, segments }
    })
})

const levelTotals = computed<Array<{ level: string; count: number }>>(() => {
  const totals = new Map<string, number>()
  for (const b of props.buckets) {
    const level = b.level.toLowerCase()
    totals.set(level, (totals.get(level) ?? 0) + b.count)
  }
  return LEVEL_ORDER.filter((lv) => totals.has(lv)).map((level) => ({
    level,
    count: totals.get(level) ?? 0,
  }))
})

const maxTotal = computed(() => Math.max(1, ...stacked.value.map((b) => b.total)))

const CHART_HEIGHT = 140
const BAR_WIDTH = 18
const BAR_GAP = 4
const chartWidth = computed(() => Math.max(1, stacked.value.length) * (BAR_WIDTH + BAR_GAP))

// 桶太多时坐标轴文字会挤成一坨,只每隔 N 个标一次(目标:大致 <=12 条可见标签)。
const labelStride = computed(() => Math.max(1, Math.ceil(stacked.value.length / 12)))

function barX(index: number): number {
  return index * (BAR_WIDTH + BAR_GAP)
}
function segY(offset: number, count: number): number {
  return CHART_HEIGHT - (offset + count) * (CHART_HEIGHT / maxTotal.value)
}
function segHeight(count: number): number {
  return count * (CHART_HEIGHT / maxTotal.value)
}
function shortLabel(bucketLabel: string): string {
  // 粒度由父组件按"生成这批数据时实际请求的粒度"传入(props.bucket),不能靠字符串形状猜——
  // 小时粒度里恰好落在 00 时的桶(如 "...T00:00:00")与天粒度桶同形,猜测法会把它误判成天粒度
  // (reviewer 深审 2026-07-20 修复)。
  const m = /^\d{4}-(\d{2})-(\d{2})T(\d{2})/.exec(bucketLabel)
  if (!m) return bucketLabel
  return props.bucket === 'day' ? `${m[1]}-${m[2]}` : `${m[2]}日${m[3]}时`
}
</script>

<template>
  <div class="log-histogram">
    <div v-if="stacked.length === 0" class="log-histogram__empty">
      {{ t('logWindow.histogramEmpty') }}
    </div>
    <template v-else>
      <div class="log-histogram__scroll">
        <svg
          :width="chartWidth"
          :height="CHART_HEIGHT + 24"
          :viewBox="`0 0 ${chartWidth} ${CHART_HEIGHT + 24}`"
        >
          <g v-for="(b, i) in stacked" :key="b.label">
            <rect
              v-for="seg in b.segments"
              :key="seg.level"
              :x="barX(i)"
              :y="segY(seg.offset, seg.count)"
              :width="BAR_WIDTH"
              :height="Math.max(0.5, segHeight(seg.count))"
              :fill="LEVEL_COLOR[seg.level]"
            >
              <title>{{ b.label }} · {{ seg.level.toUpperCase() }} · {{ seg.count }}</title>
            </rect>
            <text
              v-if="i % labelStride === 0"
              :x="barX(i) + BAR_WIDTH / 2"
              :y="CHART_HEIGHT + 16"
              class="log-histogram__axis-label"
              text-anchor="middle"
            >
              {{ shortLabel(b.label) }}
            </text>
          </g>
        </svg>
      </div>
      <div class="log-histogram__legend">
        <span
          v-for="row in levelTotals"
          :key="row.level"
          class="log-histogram__legend-item"
          :style="{ color: LEVEL_COLOR[row.level] }"
        >
          {{ row.level.toUpperCase() }}: {{ row.count }}
        </span>
      </div>
    </template>
  </div>
</template>

<style scoped>
.log-histogram {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}
.log-histogram__scroll {
  overflow-x: auto;
  overflow-y: hidden;
}
.log-histogram__empty {
  padding: var(--spacing-xl);
  text-align: center;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-xs);
}
.log-histogram__axis-label {
  font-size: var(--font-size-2xs);
  fill: var(--color-text-tertiary);
}
.log-histogram__legend {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-md);
  font-size: var(--font-size-xs);
  font-family: var(--font-mono);
}
.log-histogram__legend-item {
  font-weight: 600;
}
</style>
