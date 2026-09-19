<template>
  <!-- Semantic search results overlay panel -->
  <!-- 语义搜索结果覆盖面板 -->
  <Transition name="semantic-panel">
    <div
      v-if="ai.isSemanticMode"
      class="semantic-panel"
      :class="{
        'semantic-panel--has-results':
          ai.semanticQuery && !ai.isSearching && (media.layoutSummary?.totalItems || 0) > 0,
      }"
    >
      <!-- Header -->
      <div class="semantic-panel__header">
        <!-- 身份与引擎 -->
        <div class="semantic-panel__identity">
          <span class="semantic-panel__icon-shell">
            <Sparkles :size="18" class="semantic-panel__icon" />
          </span>
          <div class="semantic-panel__title-copy">
            <div class="semantic-panel__title">
              <span>{{ $t('semantic.title') }}</span>
              <span class="semantic-panel__title-badge">{{ $t('semantic.aiBadge') }}</span>
            </div>
            <div class="semantic-panel__subtitle">
              <span>{{ $t('semantic.engineLabel') }}</span>
              <span class="semantic-panel__provider" v-if="ai.status.provider">
                {{ ai.providerLabel }}
              </span>
            </div>
          </div>
        </div>

        <!-- 分析进度（后台索引期间显示） -->
        <!-- 分析进度徽章（后台索引期间显示） -->
        <div
          v-if="ai.status.totalItems > 0"
          class="semantic-panel__analysis"
          :class="{
            'semantic-panel__analysis--active': ai.status.isAnalyzing,
            'semantic-panel__analysis--complete': isAnalysisComplete,
          }"
          :title="
            $t('semantic.analyzedTitle', {
              analyzed: ai.status.analyzedItems,
              total: ai.status.totalItems,
            })
          "

        >
          <div class="semantic-panel__analysis-head">
            <span class="semantic-panel__analysis-label">
              <span class="semantic-panel__status-dot" />
              <span v-if="ai.status.isAnalyzing">{{ $t('semantic.analyzingStatus') }}</span>
              <span v-else-if="isAnalysisComplete">{{ $t('semantic.readyStatus') }}</span>
              <span v-else>
                {{ $t('semantic.pendingStatus', { count: ai.status.pendingItems }) }}
              </span>
            </span>
            <strong class="semantic-panel__progress-text">{{ ai.analyzeProgress }}%</strong>
          </div>
          <div
            class="semantic-panel__progress"
          >
            <div
              class="semantic-panel__progress-bar"
              :style="{ width: ai.analyzeProgress + '%' }"
            />
          </div>
          <span class="semantic-panel__analysis-detail">
            {{
              $t('semantic.analyzedTitle', {
                analyzed: ai.status.analyzedItems.toLocaleString(),
                total: ai.status.totalItems.toLocaleString(),
              })
            }}
          </span>
        </div>

        <!-- 操作 -->
        <div class="semantic-panel__controls">
          <!-- Start analysis if not yet running -->
          <!-- 如果尚未运行则启动分析 -->
          <button
            v-if="!ai.status.isAnalyzing && (!ai.status.clipLoaded || ai.status.pendingItems > 0)"
            type="button"
            class="semantic-panel__btn semantic-panel__btn--primary"
            @click="initAndStart"
            :disabled="isInitialising"
          >
            <Zap v-if="!isInitialising" :size="13" />
            <span v-if="isInitialising" class="btn-spinner" />
            {{ isInitialising ? $t('semantic.initializing') : $t('semantic.startAnalysis') }}
          </button>
          <!-- Stop analysis -->
          <!-- 停止分析 -->
          <button
            v-else-if="ai.status.isAnalyzing"
            type="button"
            class="semantic-panel__btn semantic-panel__btn--danger"
            @click="ai.stopAnalysis()"
          >
            <Square :size="13" color="currentColor" fill="currentColor" />
            {{ $t('common.stop') }}
          </button>
          <!-- Rebuild -->
          <!-- 重建 -->
          <button
            type="button"
            class="semantic-panel__btn semantic-panel__btn--ghost"
            @click="confirmRebuild()"
            :title="$t('semantic.rebuildAll')"

          >
            <RefreshCw :size="15" />
          </button>
        </div>
      </div>

      <!-- Error state -->
      <!-- 错误状态 -->
      <div v-if="ai.searchError" class="semantic-panel__error">
        <AlertCircle :size="14" />
        {{ ai.searchError }}
      </div>

      <!-- Empty state: no query yet -->
      <!-- 空状态：尚无查询 -->
      <div v-else-if="!ai.semanticQuery && !ai.isSearching" class="semantic-panel__empty">
        <Sparkles :size="40" class="semantic-panel__empty-icon" />
        <p>{{ $t('semantic.emptyPrompt') }}</p>
        <p class="semantic-panel__hint">{{ $t('semantic.emptyExamples') }}</p>
      </div>

      <!-- Searching spinner -->
      <!-- 搜索中旋转器 -->
      <div v-else-if="ai.isSearching" class="semantic-panel__loading">
        <div class="semantic-panel__spinner" />
        <span>{{ $t('semantic.searching') }}</span>
      </div>

      <!-- Results state (query exists, not searching) -->
      <!-- 结果状态（存在查询，非搜索中） -->
      <template v-else-if="ai.semanticQuery">
        <!-- Results meta (ALWAYS shown if we have a query, so slider is available) -->
        <div class="semantic-panel__results-meta">
          <span class="semantic-panel__results-count">{{
            $t('semantic.resultsCount', { count: media.layoutSummary?.totalItems || 0 })
          }}</span>
          <UiField
            :label="$t('semantic.thresholdLabel', { percent: (localThreshold * 100).toFixed(0) })"
            orientation="inline"
          >
            <input
              type="range"
              min="0.1"
              max="0.5"
              step="0.01"
              :value="localThreshold"
              @input="onSliderInput"
              @change="onSliderChange"
            />
          </UiField>
        </div>

        <!-- Empty state (shown if 0 items) -->
        <div v-if="(media.layoutSummary?.totalItems || 0) === 0" class="semantic-panel__empty">
          <Search :size="32" class="semantic-panel__empty-icon" />
          <p>{{ $t('semantic.noResults') }}</p>
          <p class="semantic-panel__hint">{{ $t('semantic.noResultsHint') }}</p>
        </div>
      </template>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Sparkles, Zap, Square, RefreshCw, AlertCircle, Search } from '@lucide/vue'
import UiField from '../ui/UiField.vue'
import { useAiStore } from '../../stores/aiStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useConfirm } from '../../composables/useConfirm'

const ai = useAiStore()
const media = useMediaStore()
const isInitialising = ref(false)
const { t } = useI18n()
const { confirm } = useConfirm()

const isAnalysisComplete = computed(
  () =>
    ai.status.totalItems > 0 &&
    ai.status.pendingItems === 0 &&
    !ai.status.isAnalyzing &&
    ai.status.analyzedItems >= ai.status.totalItems,
)

// 2026-07-10 审查 U3:rebuild 一键清空全库向量并打断运行中分析,与侧栏「重新开始」同级
// 破坏性——必须过同款确认闸(此前 ghost 小图标误点一次 = 数小时 GPU 重算)。
async function confirmRebuild() {
  const { confirmed } = await confirm({
    title: t('semantic.rebuildConfirmTitle'),
    message: t('semantic.rebuildConfirmMsg'),
    confirmText: t('semantic.rebuildConfirm'),
    cancelText: t('common.cancel'),
  })
  if (!confirmed) return
  await ai.rebuildEmbeddings()
}

// Debounce logic for the slider
const localThreshold = ref(ai.similarityThreshold)
let sliderTimeout: ReturnType<typeof setTimeout> | null = null

watch(
  () => ai.similarityThreshold,
  (val) => {
    localThreshold.value = val
  },
)

function onSliderInput(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  localThreshold.value = val

  if (sliderTimeout) clearTimeout(sliderTimeout)
  sliderTimeout = setTimeout(() => {
    ai.similarityThreshold = val
  }, 300) // 300ms debounce
}

function onSliderChange(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value)
  localThreshold.value = val
  if (sliderTimeout) clearTimeout(sliderTimeout)
  ai.similarityThreshold = val
}

async function initAndStart() {
  isInitialising.value = true
  try {
    await ai.initEngine()
    await ai.startAnalysis()
  } finally {
    isInitialising.value = false
  }
}
</script>

<style scoped>
/* ── Panel container ─────────────────────────────────────────────────────── */
.semantic-panel {
  position: absolute;
  inset: 0;
  z-index: 10;
  background: var(--color-bg-primary);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  pointer-events: auto;
}

.semantic-panel--has-results {
  position: relative;
  inset: auto;
  flex: 0 0 auto;
}

/* Transition */
.semantic-panel-enter-active,
.semantic-panel-leave-active {
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}
.semantic-panel-enter-from,
.semantic-panel-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

/* ── Header ─────────────────────────────────────────────────────────────── */
.semantic-panel__header {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  min-height: 56px;
  padding: var(--spacing-sm) clamp(var(--spacing-md), 2vw, var(--spacing-lg));
  border-bottom: 1px solid var(--color-divider);
  background: var(--color-bg-secondary);
  flex-shrink: 0;
  pointer-events: auto;
}

.semantic-panel__identity {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-width: 190px;
  flex-shrink: 0;
}

.semantic-panel__icon-shell {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--control-size-default);
  height: var(--control-size-default);
  flex-shrink: 0;
  color: var(--color-accent);
  background: var(--color-accent-subtle);
  border: 1px solid var(--color-accent);
  border-radius: var(--radius-md);
}

.semantic-panel__title-copy {
  min-width: 0;
}

.semantic-panel__title {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
}

.semantic-panel__title-badge {
  padding: var(--spacing-2xs) var(--spacing-xs);
  color: var(--color-accent);
  font-size: var(--font-size-2xs);
  font-weight: 500;
  line-height: 1;
  letter-spacing: 0.5px;
  background: var(--color-accent-subtle);
  border: 1px solid transparent;
  border-radius: var(--radius-xs);
}

.semantic-panel__subtitle {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  margin-top: 3px;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-2xs);
  line-height: 1.2;
  white-space: nowrap;
}

.semantic-panel__provider {
  display: inline-flex;
  align-items: center;
  padding: var(--spacing-2xs) var(--spacing-xs);
  color: var(--color-text-secondary);
  font-weight: 600;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-full);
}

.semantic-panel__analysis {
  display: flex;
  flex: 1 1 300px;
  flex-direction: column;
  gap: 5px;
  min-width: 230px;
  max-width: 420px;
  padding: 8px 11px;
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-md);
  transition:
    border-color var(--transition-fast),
    background-color var(--transition-fast),
    box-shadow var(--transition-fast);
}

.semantic-panel__analysis--active {
  border-color: var(--color-accent);
  background: var(--color-accent-subtle);
  box-shadow: none;
}

.semantic-panel__analysis--complete {
  border-color: var(--color-success);
  background: var(--color-success-subtle);
}

.semantic-panel__analysis-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
  min-width: 0;
}

.semantic-panel__analysis-label {
  display: inline-flex;
  align-items: center;
  min-width: 0;
  gap: var(--spacing-xs);
  overflow: hidden;
  color: var(--color-text-secondary);
  font-size: var(--font-size-2xs);
  font-weight: 600;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.semantic-panel__status-dot {
  width: 6px;
  height: 6px;
  flex-shrink: 0;
  background: var(--color-text-tertiary);
  border-radius: var(--radius-full);
}

.semantic-panel__analysis--active .semantic-panel__status-dot {
  background: var(--color-accent);
  animation: semantic-pulse 1.6s ease-in-out infinite;
}

.semantic-panel__analysis--complete .semantic-panel__status-dot {
  background: var(--color-success);
}

.semantic-panel__progress-text {
  flex-shrink: 0;
  color: var(--color-text-primary);
  font-size: var(--font-size-xs);
  font-variant-numeric: tabular-nums;
}

.semantic-panel__progress {
  position: relative;
  height: 6px;
  overflow: hidden;
  background: var(--color-bg-inset);
  border-radius: var(--radius-full);
}

.semantic-panel__analysis-detail {
  overflow: hidden;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-2xs);
  line-height: 1.2;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.semantic-panel__progress-bar {
  position: absolute;
  inset: 0 auto 0 0;
  min-width: 0;
  background: var(--color-accent);
  border-radius: inherit;
  transition: width var(--duration-slow) var(--ease-out);
}

.semantic-panel__analysis--complete .semantic-panel__progress-bar {
  background: var(--color-success);
}

@keyframes semantic-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.45;
  }
}

/* ── Controls ───────────────────────────────────────────────────────────── */
.semantic-panel__controls {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  gap: var(--spacing-xs);
  margin-left: auto;
}

.semantic-panel__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: var(--control-size-default);
  gap: var(--spacing-xs);
  padding: 0 var(--spacing-sm);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  font-weight: 600;
  background: transparent;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition:
    background-color var(--transition-fast),
    border-color var(--transition-fast),
    color var(--transition-fast),
    box-shadow var(--transition-fast);
}

.semantic-panel__btn:hover {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
  border-color: var(--color-accent);
}

.semantic-panel__btn:focus-visible {
  outline: none;
  box-shadow: var(--control-focus-ring);
}

.semantic-panel__btn--primary {
  color: var(--color-text-on-accent);
  background: var(--color-accent);
  border-color: var(--color-accent);
  box-shadow: none;
}

.semantic-panel__btn--primary:hover {
  color: var(--color-text-on-accent);
  background: var(--color-accent-hover);
  border-color: var(--color-accent-hover);
}

.semantic-panel__btn--danger {
  color: var(--color-error);
  border-color: color-mix(in srgb, var(--color-error) 38%, var(--color-border));
}

.semantic-panel__btn--danger:hover {
  color: var(--color-text-on-error);
  background: var(--color-error);
  border-color: var(--color-error);
}

.semantic-panel__btn--ghost {
  width: var(--control-size-default);
  padding: 0;
  color: var(--color-text-tertiary);
  background: transparent;
  border-color: transparent;
}

.semantic-panel__btn--ghost:hover {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
  border-color: transparent;
}

.semantic-panel__btn:disabled {
  opacity: var(--opacity-disabled);
  cursor: not-allowed;
  box-shadow: none;
}

.semantic-panel__btn:disabled:hover {
  color: var(--color-text-tertiary);
  background: var(--color-bg-hover);
  border-color: var(--color-border);
}

.btn-spinner {
  display: inline-block;
  width: 12px;
  height: 12px;
  border: 2px solid color-mix(in srgb, var(--color-text-on-accent) 30%, transparent);
  border-top-color: var(--color-text-on-accent);
  border-radius: 50%;
  animation: spin var(--duration-spin) linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* ── Body states ─────────────────────────────────────────────────────────── */
.semantic-panel__error {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
  color: var(--color-error);
  font-size: var(--font-size-sm);
  background: var(--color-error-subtle);
  border-bottom: 1px solid var(--color-error);
  pointer-events: auto;
}

.semantic-panel__empty,
.semantic-panel__loading {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
  text-align: center;
  padding: var(--spacing-xl);
  pointer-events: auto;
  background: var(--color-bg-primary);
}
.semantic-panel__empty-icon {
  opacity: 0.25;
  margin-bottom: var(--spacing-sm);
  color: var(--color-accent);
}
.semantic-panel__hint {
  font-size: var(--font-size-xs);
  opacity: 0.6;
}

.semantic-panel__spinner {
  width: 32px;
  height: 32px;
  border: 3px solid color-mix(in srgb, var(--color-accent) 20%, transparent);
  border-top-color: var(--color-accent);
  border-radius: 50%;
  animation: spin var(--duration-spin) linear infinite;
}

/* ── Results meta ────────────────────────────────────────────────────────── */
.semantic-panel__results-meta {
  padding: 10px var(--spacing-md);
  background: var(--color-bg-primary);
  border-bottom: 1px solid var(--color-border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  pointer-events: auto;
}
.semantic-panel__results-count {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  font-weight: 500;
}
/* 阈值滑块字段(label+range)迁 UiField(orientation=inline);原 .threshold-slider + label 已删。 */

@media (max-width: 760px) {
  .semantic-panel__header {
    flex-wrap: wrap;
    gap: var(--spacing-sm);
    min-height: 0;
  }

  .semantic-panel__identity {
    flex: 1 1 auto;
    min-width: 0;
  }

  .semantic-panel__analysis {
    order: 3;
    flex-basis: calc(100% - var(--spacing-sm));
    width: calc(100% - var(--spacing-sm));
    max-width: calc(100% - var(--spacing-sm));
  }

  .semantic-panel__controls {
    margin-left: 0;
  }
}

</style>
