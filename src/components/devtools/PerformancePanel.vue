<template>
  <Teleport to="body">
    <section
      v-if="panelOpen"
      class="perf-panel"
    >
      <header class="perf-panel__header">
        <div>
          <div class="perf-panel__title"><Gauge :size="17" />{{ t('performance.title') }}</div>
          <div class="perf-panel__shortcut">{{ t('performance.shortcut') }}</div>
        </div>
        <button class="perf-panel__icon" @click="closePanel">
          <X :size="17" />
        </button>
      </header>

      <div v-if="phase !== 'idle'" class="perf-panel__status" :class="`is-${phase}`">
        <span>{{
          phase === 'arming' ? t('performance.calibrating') : t('performance.recording')
        }}</span>
        <UiButton size="sm" variant="secondary" @click="stopRecording">
          <Square :size="13" />{{ t('performance.stop') }}
        </UiButton>
      </div>

      <div v-if="errorMessage" class="perf-panel__error">{{ errorMessage }}</div>

      <div class="perf-panel__controls">
        <label class="perf-panel__field">
          <span>{{ t('performance.duration') }}</span>
          <select v-model.number="durationSeconds" :disabled="phase !== 'idle'">
            <option :value="5">{{ t('performance.seconds', { count: 5 }) }}</option>
            <option :value="10">{{ t('performance.seconds', { count: 10 }) }}</option>
            <option :value="20">{{ t('performance.seconds', { count: 20 }) }}</option>
            <option :value="30">{{ t('performance.seconds', { count: 30 }) }}</option>
          </select>
        </label>
        <label class="perf-panel__toggle">
          <span>
            <strong>{{ t('performance.liveMode') }}</strong>
            <small>{{ t('performance.liveModeDesc') }}</small>
          </span>
          <UiToggle v-model="liveMode" :disabled="phase !== 'idle'" />
        </label>
        <div class="perf-panel__actions">
          <UiButton :disabled="phase !== 'idle'" @click="startManualRecording">
            <CircleDot :size="14" />{{ t('performance.manualRecord') }}
          </UiButton>
          <UiButton variant="secondary" :disabled="phase !== 'idle'" @click="runGalleryRoundTrip">
            <Repeat2 :size="14" />{{ t('performance.roundTrip') }}
          </UiButton>
        </div>
        <p class="perf-panel__hint">{{ t('performance.silentHint') }}</p>
      </div>

      <template v-if="currentSummary">
        <div class="perf-panel__section-title">
          <span>{{ t('performance.latestResult') }}</span>
          <button class="perf-panel__text-btn" @click="exportSession(currentSummary)">
            <Download :size="13" />{{ t('performance.exportJson') }}
          </button>
        </div>

        <div class="perf-panel__metrics">
          <article>
            <strong>{{ currentSummary.frame.avgFps }}</strong>
            <span>{{ t('performance.avgFps') }}</span>
          </article>
          <article>
            <strong>{{ formatMs(currentSummary.frame.frameTime.p95) }}</strong>
            <span>{{ t('performance.p95') }}</span>
          </article>
          <article>
            <strong>{{ formatMs(currentSummary.frame.frameTime.p99) }}</strong>
            <span>{{ t('performance.p99') }}</span>
          </article>
          <article>
            <strong>{{ currentSummary.frame.missedVsync }}</strong>
            <span>{{ t('performance.missedVsync') }}</span>
          </article>
          <article>
            <strong>{{ formatPercent(currentSummary.frame.jankRatio) }}</strong>
            <span>{{ t('performance.jankRatio') }}</span>
          </article>
        </div>

        <div class="perf-panel__meta">
          <span>{{
            t('performance.refreshSummary', {
              rate: currentSummary.frame.refreshRate,
              budget: currentSummary.frame.budgetMs,
            })
          }}</span>
          <span>{{ formatSeconds(currentSummary.durationMs) }}</span>
          <span v-for="(value, key) in currentSummary.context" :key="key"
            >{{ key }}={{ value }}</span
          >
        </div>

        <div v-if="spanRows.length" class="perf-panel__table-wrap">
          <table class="perf-panel__table">
            <thead>
              <tr>
                <th>{{ t('performance.metric') }}</th>
                <th>{{ t('performance.samples') }}</th>
                <th>{{ t('performance.p50') }}</th>
                <th>{{ t('performance.p95') }}</th>
                <th>{{ t('performance.max') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="row in spanRows" :key="row.name">
                <td>{{ spanLabel(row.name) }}</td>
                <td>{{ row.summary.samples }}</td>
                <td>{{ formatMs(row.summary.p50) }}</td>
                <td>{{ formatMs(row.summary.p95) }}</td>
                <td>{{ formatMs(row.summary.max) }}</td>
              </tr>
            </tbody>
          </table>
        </div>

        <div class="perf-panel__details">
          <span>
            {{ t('performance.longFrames') }}: {{ currentSummary.longFrames.samples }}
            <template v-if="currentSummary.capabilities.longFrameSource">
              {{
                t('performance.capabilitySource', {
                  source: currentSummary.capabilities.longFrameSource,
                })
              }}
            </template>
          </span>
          <span>
            {{ t('performance.inputEvents') }}: {{ currentSummary.inputDelay.samples }}
            <template v-if="currentSummary.inputDelay.samples">
              {{ t('performance.inputP95', { value: formatMs(currentSummary.inputDelay.p95) }) }}
            </template>
          </span>
          <span v-if="currentSummary.gauges['gallery.cacheBytes'] != null">
            {{ t('performance.canvasCache') }}:
            {{ formatBytes(currentSummary.gauges['gallery.cacheBytes']) }} /
            {{ currentSummary.gauges['gallery.cacheItems'] ?? 0 }}
          </span>
          <span v-if="currentSummary.heapDeltaBytes != null">
            {{
              t('performance.jsHeapDelta', { value: signedBytes(currentSummary.heapDeltaBytes) })
            }}
          </span>
        </div>
        <p class="perf-panel__caveat">{{ t('performance.gpuCaveat') }}</p>
      </template>

      <template v-if="sessions.length">
        <div class="perf-panel__section-title">
          <span>{{ t('performance.history') }}</span>
          <button class="perf-panel__text-btn" @click="clearHistory">
            <Trash2 :size="13" />{{ t('performance.clearHistory') }}
          </button>
        </div>
        <div class="perf-panel__history">
          <button
            v-for="session in sessions"
            :key="session.id"
            type="button"
            :class="{ active: currentSummary?.id === session.id }"
            @click="liveSummary = session"
          >
            <span>{{
              t('performance.historyContext', {
                scenario: session.context.scenario,
                mode: session.context.galleryMode ?? t('performance.notAvailable'),
              })
            }}</span>
            <strong>{{
              t('performance.historySummary', {
                fps: session.frame.avgFps,
                p95: formatMs(session.frame.frameTime.p95),
              })
            }}</strong>
          </button>
        </div>
      </template>
    </section>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { CircleDot, Download, Gauge, Repeat2, Square, Trash2, X } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../ui/UiButton.vue'
import UiToggle from '../ui/UiToggle.vue'
import { usePerformanceMonitor } from '../../composables/usePerformanceMonitor'
import type {
  PerformanceSessionSummary,
  PerformanceSpanName,
  SampleSummary,
} from '../../perf/performanceRecorder'

const { t } = useI18n()
const {
  panelOpen,
  phase,
  durationSeconds,
  liveMode,
  errorMessage,
  sessions,
  liveSummary,
  closePanel,
  togglePanel,
  startManualRecording,
  runGalleryRoundTrip,
  stopRecording,
  clearHistory,
} = usePerformanceMonitor()

const currentSummary = computed(() => liveSummary.value ?? sessions.value[0] ?? null)
const spanRows = computed<Array<{ name: PerformanceSpanName; summary: SampleSummary }>>(() => {
  const summary = currentSummary.value
  if (!summary) return []
  return (Object.entries(summary.spans) as Array<[PerformanceSpanName, SampleSummary]>)
    .filter(([, value]) => value.samples > 0)
    .map(([name, value]) => ({ name, summary: value }))
})

function spanLabel(name: PerformanceSpanName): string {
  const keys: Record<PerformanceSpanName, string> = {
    'gallery.draw': 'performance.spanGalleryDraw',
    'gallery.bitmapLoad': 'performance.spanBitmapLoad',
    'gallery.imageFallback': 'performance.spanImageFallback',
    // Canvas 缩略图分段(密集缩略图方案 §7.3):候选等槽 → 字节 → 源解码 → 裁剪缩放 → 入屏首绘。
    'gallery.thumbWait': 'performance.spanThumbWait',
    'gallery.thumbFetch': 'performance.spanThumbFetch',
    'gallery.thumbSourceDecode': 'performance.spanThumbSourceDecode',
    'gallery.thumbPrep': 'performance.spanThumbPrep',
    'gallery.thumbFirstPaint': 'performance.spanThumbFirstPaint',
    'timeline.draw': 'performance.spanTimelineDraw',
    'timeline.static': 'performance.spanTimelineStatic',
    'timeline.loupe': 'performance.spanTimelineLoupe',
  }
  return t(keys[name])
}

function formatMs(value: number): string {
  return `${value.toFixed(value >= 10 ? 1 : 2)}ms`
}

function formatSeconds(value: number): string {
  return `${(value / 1000).toFixed(1)}s`
}

function formatPercent(value: number): string {
  return `${(value * 100).toFixed(1)}%`
}

function formatBytes(value: number): string {
  if (Math.abs(value) < 1024) return `${Math.round(value)}B`
  if (Math.abs(value) < 1024 * 1024) return `${(value / 1024).toFixed(1)}KB`
  return `${(value / 1024 / 1024).toFixed(1)}MB`
}

function signedBytes(value: number): string {
  return `${value >= 0 ? '+' : ''}${formatBytes(value)}`
}

function exportSession(session: PerformanceSessionSummary): void {
  const blob = new Blob([JSON.stringify(session, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `scrollery-performance-${session.id}.json`
  anchor.click()
  URL.revokeObjectURL(url)
}

function onGlobalKeydown(event: KeyboardEvent): void {
  if (!(event.ctrlKey && event.shiftKey && !event.altKey && event.code === 'KeyP')) return
  event.preventDefault()
  event.stopPropagation()
  togglePanel()
}

onMounted(() => window.addEventListener('keydown', onGlobalKeydown, true))
onBeforeUnmount(() => window.removeEventListener('keydown', onGlobalKeydown, true))
</script>

<style scoped>
.perf-panel {
  position: fixed;
  top: 52px;
  right: 16px;
  z-index: calc(var(--z-toast, 300) + 10);
  width: min(540px, calc(100vw - 32px));
  max-height: calc(100vh - 84px);
  overflow: auto;
  padding: var(--spacing-md);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-xl);
  background: var(--color-bg-elevated);
  color: var(--color-text-primary);
  box-shadow: var(--shadow-lg);
  backdrop-filter: none;
  -webkit-backdrop-filter: none;
  contain: layout paint style;
  scrollbar-width: thin;
}

.perf-panel__header,
.perf-panel__section-title,
.perf-panel__status,
.perf-panel__toggle,
.perf-panel__actions,
.perf-panel__details {
  display: flex;
  align-items: center;
}

.perf-panel__header,
.perf-panel__section-title,
.perf-panel__status,
.perf-panel__toggle {
  justify-content: space-between;
}

.perf-panel__title {
  display: flex;
  gap: var(--spacing-sm);
  align-items: center;
  font-size: var(--font-size-md);
  font-weight: 650;
}

.perf-panel__shortcut,
.perf-panel__hint,
.perf-panel__caveat,
.perf-panel__toggle small {
  color: var(--color-text-tertiary);
  font-size: var(--font-size-2xs);
}

.perf-panel__icon,
.perf-panel__text-btn {
  display: inline-flex;
  gap: var(--spacing-xs);
  align-items: center;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-xs);
  border: 0;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
}

.perf-panel__status,
.perf-panel__error {
  margin-top: var(--spacing-md);
  padding: var(--spacing-sm) var(--spacing-md);
  border-radius: var(--radius-md);
  font-size: var(--font-size-xs);
}

.perf-panel__status {
  background: var(--color-accent-subtle);
}

/* 原 var(--color-danger) 为幽灵 token 且无 fallback:color 回落继承色、color-mix 整条声明
   失效,录制态/错误态一直没红出来。按语义落 error(与 S5 三处同族修法一致)(S7 修)。 */
.perf-panel__status.is-recording {
  color: var(--color-error);
  background: var(--color-error-subtle);
}

.perf-panel__error {
  color: var(--color-error);
  background: var(--color-error-subtle);
}

.perf-panel__controls {
  display: grid;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-md);
}

.perf-panel__field {
  display: flex;
  gap: var(--spacing-sm);
  align-items: center;
  font-size: var(--font-size-xs);
}

.perf-panel__field select {
  min-width: 76px;
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
}
.perf-panel__field select:focus-visible {
  outline: none;
  border-color: var(--color-input-border-focus);
  box-shadow: var(--control-focus-ring);
}

.perf-panel__toggle span {
  display: grid;
  gap: var(--spacing-2xs);
}

.perf-panel__toggle strong {
  font-size: var(--font-size-xs);
  font-weight: 550;
}

.perf-panel__actions {
  gap: var(--spacing-sm);
  flex-wrap: wrap;
}

.perf-panel__hint,
.perf-panel__caveat {
  margin: 0;
  line-height: 1.5;
}

.perf-panel__section-title {
  margin-top: var(--spacing-lg);
  padding-top: var(--spacing-md);
  border-top: 1px solid var(--color-divider);
  font-size: var(--font-size-xs);
  font-weight: 600;
}

.perf-panel__metrics {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: var(--spacing-xs);
  margin-top: var(--spacing-sm);
}

.perf-panel__metrics article {
  display: grid;
  gap: 2px;
  min-width: 0;
  padding: var(--spacing-sm) var(--spacing-xs);
  border-radius: var(--radius-sm);
  background: var(--color-bg-surface);
  text-align: center;
}

.perf-panel__metrics strong {
  font-size: var(--font-size-md);
}

.perf-panel__metrics span,
.perf-panel__meta,
.perf-panel__details {
  color: var(--color-text-secondary);
  font-size: var(--font-size-2xs);
}

.perf-panel__meta {
  display: flex;
  gap: var(--spacing-xs);
  flex-wrap: wrap;
  margin-top: var(--spacing-sm);
}

.perf-panel__meta span {
  padding: var(--spacing-2xs) var(--spacing-xs);
  border-radius: var(--radius-xs);
  background: var(--color-bg-hover);
}

.perf-panel__table-wrap {
  margin-top: var(--spacing-sm);
  overflow-x: auto;
}

.perf-panel__table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--font-size-2xs);
}

.perf-panel__table th,
.perf-panel__table td {
  padding: var(--spacing-xs);
  border-bottom: 1px solid var(--color-border);
  text-align: right;
}

.perf-panel__table th:first-child,
.perf-panel__table td:first-child {
  text-align: left;
}

.perf-panel__details {
  gap: var(--spacing-xs) var(--spacing-md);
  flex-wrap: wrap;
  margin-top: var(--spacing-sm);
}

.perf-panel__history {
  display: grid;
  gap: var(--spacing-xs);
  margin-top: var(--spacing-sm);
}

.perf-panel__history button {
  display: flex;
  justify-content: space-between;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: var(--color-bg-surface);
  color: var(--color-text-secondary);
  font-size: var(--font-size-2xs);
  text-align: left;
  cursor: pointer;
}

.perf-panel__history button.active {
  border-color: var(--color-accent);
}

.perf-panel__history strong {
  color: var(--color-text-primary);
  font-weight: 550;
}

@media (max-width: 640px) {
  .perf-panel {
    top: 8px;
    right: 8px;
    width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
  }

  .perf-panel__metrics {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .perf-panel__history button {
    display: grid;
    gap: var(--spacing-2xs);
  }
}
</style>
