<script setup lang="ts">
// 独立日志窗口内容(日志能力重构 S4/S5,方案 §5/§9.4)。main.ts 按 window label==='logs'
// 直接把本组件挂为应用根(跳过 App.vue 的 AppShell/路由整套,方案 §5「主体是独立日志窗口」)。
// S5 新增「分析」第三标签(P1 错误聚合/preset/诊断可见 + P2 直方图/诊断包导出)+ 双窗格上下文。
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLogWindowStore, RENDER_CAP_MIN, RENDER_CAP_MAX } from '../stores/logWindowStore'
import { useToastStore } from '../stores/toastStore'
import {
  initializeSettings,
  setSettingsApplyFailedFormatter,
  setSettingsErrorReporter,
  setSettingsWriteFailedFormatter,
  settingsReady,
} from '../stores/settingsPersistence'
import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { LOG_LEVELS, type LogLevelFilter } from '../types/logEntry'
import { aggregateErrorEntries, aggregateSpanDurations } from '../utils/logAggregation'
import LogVirtualList from '../components/logwindow/LogVirtualList.vue'
import LogHistogramChart from '../components/logwindow/LogHistogramChart.vue'
import LogRow from '../components/logwindow/LogRow.vue'
import UiCheckbox from '../components/ui/UiCheckbox.vue'
import UiButton from '../components/ui/UiButton.vue'
import ToastContainer from '../components/common/ToastContainer.vue'

const { t } = useI18n()
const store = useLogWindowStore()
const toast = useToastStore()

// 中央设置的失败提示:本窗口是独立 Vue app(main.ts 按 label 直接挂根),不走 App.vue。若不在此
// 接一次口,preset 写盘失败与「已保存但部分项未应用」在本窗口无人提示 —— logWindowStore 的
// persistPresets 假定「中央服务已提示」,静默吞掉就会让用户以为 preset 已保存。文案复用
// settings.* 既有键,不新增文案与 UI 框架。
setSettingsErrorReporter((message) => toast.addToast('error', message, 5000))
setSettingsWriteFailedFormatter(() => t('settings.saveFailedNotice'))
setSettingsApplyFailedFormatter((keys) => t('settings.applyFailedNotice', { keys: keys.join(', ') }))

/** 设置快照读取失败:preset 读写不可用。给出可见提示与重试,不静默当作已就绪。 */
const settingsInitFailed = ref(false)
const settingsInitRetrying = ref(false)

/**
 * 确保权威设置已加载。失败保持可见状态(settingsReady 未置位 → 保存入口禁用),
 * 重试走同一入口(initializeSettings 读取失败会清缓存 Promise,可再次发起)。
 */
async function ensureSettingsLoaded(): Promise<void> {
  if (settingsReady.value) {
    settingsInitFailed.value = false
    return
  }
  try {
    await initializeSettings()
    settingsInitFailed.value = false
  } catch {
    settingsInitFailed.value = true
  }
}

async function retrySettingsInit(): Promise<void> {
  settingsInitRetrying.value = true
  try {
    await ensureSettingsLoaded()
  } finally {
    settingsInitRetrying.value = false
  }
}

type Tab = 'live' | 'history' | 'analysis'
const activeTab = ref<Tab>('live')
const logDir = ref('')

onMounted(async () => {
  // 不 await:设置是否就绪由 settingsReady 驱动界面门控,不阻塞日志窗口的其余初始化。
  void ensureSettingsLoaded()
  try {
    logDir.value = await invokeIpc<string>(IPC.GET_LOG_DIR)
  } catch {
    // 非致命:仅「打开日志目录」按钮不可用,不阻塞其余功能。
  }
  await store.loadHistoryFiles().catch(() => {})
})

function onSwitchTab(tab: Tab) {
  activeTab.value = tab
  if (tab === 'history' && !store.selectedHistoryFile && store.historyFiles.length > 0) {
    void store.openHistoryFile(store.historyFiles[0].name)
  }
  if (tab === 'analysis') {
    void store.refreshDiagnostics().catch(() => {})
  }
}

async function onSelectHistoryFile(e: Event) {
  const name = (e.target as HTMLSelectElement).value
  if (name) await store.openHistoryFile(name)
}

const visibleEntries = computed(() =>
  activeTab.value === 'history' ? store.filteredHistory : store.filteredLive,
)

// 分析面板的错误聚合:基于当前已加载(实时+历史)的全部条目,不受级别/target/文本过滤影响——
// 用户即便把级别过滤只留 info,仍希望看到"这段时间内到底出了多少错"(方案 §5 P1)。
const errorAggregation = computed(() =>
  aggregateErrorEntries(store.liveEntries.concat(store.historyEntries)),
)

// span 耗时 Top N(span 埋点线 W2,方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入):
// 同 errorAggregation 姿态——基于当前已加载的全部(实时+历史)条目,不受级别/target/文本过滤影响。
const spanAggregation = computed(() =>
  aggregateSpanDurations(store.liveEntries.concat(store.historyEntries)),
)

async function copyVisible() {
  const text = visibleEntries.value
    .map((e) => `${e.ts} ${e.level.toUpperCase()} ${e.target} ${e.msg}`)
    .join('\n')
  try {
    await navigator.clipboard.writeText(text)
    toast.addToast('success', t('logWindow.copySuccess', { n: visibleEntries.value.length }))
  } catch (err) {
    toast.addToast('error', t('logWindow.copyFailed', { error: err }))
  }
}

async function openLogDir() {
  if (!logDir.value) return
  try {
    await invokeIpc(IPC.OPEN_DIRECTORY, { path: logDir.value })
  } catch (err) {
    toast.addToast('error', t('settings.openDirFailed', { error: err }))
  }
}

function onRenderCapChange(e: Event) {
  const n = Number((e.target as HTMLInputElement).value)
  if (Number.isFinite(n)) store.setRenderCap(n)
}

// ── 过滤 preset(方案 §5 P1)────────────────────────────────────────────
function onSavePreset() {
  // 未拿到权威设置前禁止保存:占位值不得被当成用户修改写回(preset 区已同条件禁用按钮)。
  if (!settingsReady.value) return
  let name: string | null = null
  try {
    name = window.prompt(t('logWindow.presetNamePrompt'))
  } catch {
    name = null
  }
  name = name?.trim() ?? ''
  if (!name) return
  store.saveCurrentAsPreset(name)
}

function onApplyPresetChange(e: Event) {
  const name = (e.target as HTMLSelectElement).value
  if (name) store.applyPreset(name)
  ;(e.target as HTMLSelectElement).value = ''
}

function onDeletePreset(name: string) {
  if (!settingsReady.value) return
  store.deletePreset(name)
}

// ── 直方图(方案 §5 P2)────────────────────────────────────────────────
const histogramFile = ref('')
const histogramBucket = ref<'hour' | 'day'>('hour')

function onHistogramFileChange(e: Event) {
  histogramFile.value = (e.target as HTMLSelectElement).value
}

async function generateHistogram() {
  if (!histogramFile.value) return
  try {
    await store.loadHistogram(histogramFile.value, histogramBucket.value)
  } catch (err) {
    toast.addToast('error', t('logWindow.histogramFailed', { error: err }))
  }
}

// ── 诊断包导出(方案 §5 P2)────────────────────────────────────────────
const exporting = ref(false)
const lastExportDir = ref('')

async function onExportDiagnostics() {
  exporting.value = true
  try {
    const result = await store.exportDiagnosticsPackage()
    lastExportDir.value = result.dir
    toast.addToast('success', t('logWindow.exportSuccess', { n: result.redactedMatches }))
  } catch (err) {
    toast.addToast('error', t('logWindow.exportFailed', { error: err }))
  } finally {
    exporting.value = false
  }
}

async function onOpenExportDir() {
  if (!lastExportDir.value) return
  try {
    await invokeIpc(IPC.OPEN_DIRECTORY, { path: lastExportDir.value })
  } catch (err) {
    toast.addToast('error', t('settings.openDirFailed', { error: err }))
  }
}

// ── 双窗格上下文(方案 §5 P2)──────────────────────────────────────────
const contextEntries = computed(() => store.contextView?.entries ?? [])
const contextSelectedIndex = computed(() => store.contextView?.selectedIndex ?? -1)
</script>

<template>
  <div class="log-window">
    <header class="log-window__toolbar">
      <div class="log-window__tabs">
        <button
          type="button"
          class="log-window__tab"
          :class="{ 'log-window__tab--active': activeTab === 'live' }"
          @click="onSwitchTab('live')"
        >
          {{ t('logWindow.tabLive') }}
        </button>
        <button
          type="button"
          class="log-window__tab"
          :class="{ 'log-window__tab--active': activeTab === 'history' }"
          @click="onSwitchTab('history')"
        >
          {{ t('logWindow.tabHistory') }}
        </button>
        <button
          type="button"
          class="log-window__tab"
          :class="{ 'log-window__tab--active': activeTab === 'analysis' }"
          @click="onSwitchTab('analysis')"
        >
          {{ t('logWindow.tabAnalysis') }}
        </button>
      </div>

      <div class="log-window__filters">
        <UiCheckbox
          v-for="level in LOG_LEVELS"
          :key="level"
          :model-value="store.levelFilter.has(level as LogLevelFilter)"
          :label="level.toUpperCase()"
          @update:model-value="store.toggleLevel(level as LogLevelFilter)"
        />
        <input
          v-model="store.targetFilter"
          class="log-window__input"
          type="text"
          :placeholder="t('logWindow.targetPlaceholder')"
        />
        <div class="log-window__text-filter">
          <input
            v-model="store.textFilter"
            class="log-window__input log-window__input--grow"
            :class="{ 'log-window__input--invalid': store.textFilterInvalid }"
            type="text"
            :placeholder="
              store.textFilterMode === 'regex' ? t('logWindow.textRegexPlaceholder') : t('logWindow.textPlaceholder')
            "
          />
          <button
            type="button"
            class="log-window__regex-toggle"
            :class="{ 'log-window__regex-toggle--active': store.textFilterMode === 'regex' }"
            :title="t('logWindow.regexToggle')"
            @click="store.toggleTextFilterMode()"
          >
            .*
          </button>
        </div>
        <select class="log-window__input" :disabled="!settingsReady" @change="onApplyPresetChange">
          <option value="">{{ t('logWindow.presetSelectPlaceholder') }}</option>
          <option v-for="p in store.presets" :key="p.name" :value="p.name">{{ p.name }}</option>
        </select>
        <UiButton variant="secondary" :disabled="!settingsReady" @click="onSavePreset">
          {{ t('logWindow.presetSave') }}
        </UiButton>
        <!-- 未就绪/读取失败时的最小提示:标题点明状态(悬停给出原因),重试按钮紧邻被禁用的保存入口,
             让「preset 现在存不了」这件事可见,而不是静默失败。 -->
        <template v-if="settingsInitFailed">
          <span class="log-window__settings-error" :title="t('settings.loadFailedHint')">
            {{ t('settings.loadFailedTitle') }}
          </span>
          <UiButton variant="secondary" :loading="settingsInitRetrying" @click="retrySettingsInit">
            {{ t('settings.loadFailedRetry') }}
          </UiButton>
        </template>
      </div>

      <div class="log-window__actions">
        <template v-if="activeTab === 'live'">
          <UiButton variant="secondary" @click="store.setPaused(!store.paused)">
            {{ store.paused ? t('logWindow.resume', { n: store.pendingCount }) : t('logWindow.pause') }}
          </UiButton>
          <UiButton variant="secondary" @click="store.clearView()">
            {{ t('logWindow.clearView') }}
          </UiButton>
        </template>
        <UiButton v-if="activeTab !== 'analysis'" variant="secondary" @click="copyVisible">
          {{ t('logWindow.copyVisible') }}
        </UiButton>
        <UiButton variant="secondary" :disabled="!logDir" @click="openLogDir">
          {{ t('logWindow.openLogDir') }}
        </UiButton>
      </div>
    </header>

    <div v-if="activeTab === 'live'" class="log-window__subbar">
      <label class="log-window__cap">
        {{ t('logWindow.renderCap') }}
        <input
          type="number"
          :min="RENDER_CAP_MIN"
          :max="RENDER_CAP_MAX"
          step="1000"
          :value="store.renderCap"
          class="log-window__input log-window__input--number"
          @change="onRenderCapChange"
        />
      </label>
      <span class="log-window__count">
        {{ t('logWindow.entryCount', { n: store.filteredLive.length, total: store.liveEntries.length }) }}
      </span>
    </div>
    <div v-else-if="activeTab === 'history'" class="log-window__subbar">
      <select class="log-window__input" @change="onSelectHistoryFile">
        <option v-if="store.historyFiles.length === 0" value="">{{ t('logWindow.noHistory') }}</option>
        <option
          v-for="f in store.historyFiles"
          :key="f.name"
          :value="f.name"
          :selected="f.name === store.selectedHistoryFile"
        >
          {{ f.name }}
        </option>
      </select>
      <UiButton
        v-if="store.historyHasMoreOlder"
        variant="secondary"
        :loading="store.historyLoading"
        @click="store.loadOlderHistory()"
      >
        {{ t('logWindow.loadOlder') }}
      </UiButton>
    </div>
    <div v-else class="log-window__subbar">
      <span class="log-window__count">{{ t('logWindow.analysisHint') }}</span>
    </div>

    <div v-if="activeTab === 'analysis'" class="log-analysis">
      <section class="log-analysis__section">
        <h3 class="log-analysis__heading">{{ t('logWindow.diagnosticsTitle') }}</h3>
        <UiButton variant="secondary" @click="store.refreshDiagnostics()">
          {{ t('logWindow.diagnosticsRefresh') }}
        </UiButton>
        <div v-if="store.diagnostics" class="log-analysis__diagnostics">
          <p class="log-analysis__stat">
            {{ t('logWindow.droppedLines', { n: store.diagnostics.droppedLines }) }}
          </p>
          <table v-if="store.diagnostics.dedupActive.length > 0" class="log-analysis__table">
            <thead>
              <tr>
                <th>{{ t('logWindow.columnTarget') }}</th>
                <th>{{ t('logWindow.columnCode') }}</th>
                <th>{{ t('logWindow.dedupSwallowed') }}</th>
                <th>{{ t('logWindow.dedupWindowAge') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="d in store.diagnostics.dedupActive" :key="`${d.target}-${d.code}`">
                <td>{{ d.target }}</td>
                <td>{{ d.code }}</td>
                <td>{{ d.swallowedInWindow }}</td>
                <td>{{ d.windowAgeSecs }}s</td>
              </tr>
            </tbody>
          </table>
          <p v-else class="log-analysis__empty">{{ t('logWindow.dedupEmpty') }}</p>
        </div>
      </section>

      <section class="log-analysis__section">
        <h3 class="log-analysis__heading">{{ t('logWindow.errorAggTitle') }}</h3>
        <table v-if="errorAggregation.length > 0" class="log-analysis__table">
          <thead>
            <tr>
              <th>{{ t('logWindow.columnCode') }}</th>
              <th>{{ t('logWindow.errorAggCount') }}</th>
              <th>{{ t('logWindow.errorAggFirst') }}</th>
              <th>{{ t('logWindow.errorAggLast') }}</th>
              <th>{{ t('logWindow.errorAggSample') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in errorAggregation" :key="row.code">
              <td>{{ row.code }}</td>
              <td>{{ row.count }}</td>
              <td>{{ row.firstTs }}</td>
              <td>{{ row.lastTs }}</td>
              <td class="log-analysis__sample">{{ row.sampleMsg }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="log-analysis__empty">{{ t('logWindow.errorAggEmpty') }}</p>
      </section>

      <section class="log-analysis__section">
        <h3 class="log-analysis__heading">{{ t('logWindow.spanAggTitle') }}</h3>
        <table v-if="spanAggregation.length > 0" class="log-analysis__table">
          <thead>
            <tr>
              <th>{{ t('logWindow.columnSpanName') }}</th>
              <th>{{ t('logWindow.spanAggCount') }}</th>
              <th>{{ t('logWindow.spanAggTotal') }}</th>
              <th>{{ t('logWindow.spanAggAvg') }}</th>
              <th>{{ t('logWindow.spanAggMax') }}</th>
              <th>{{ t('logWindow.spanAggLast') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in spanAggregation" :key="row.name">
              <td>{{ row.name }}</td>
              <td>{{ row.count }}</td>
              <td>{{ row.totalMs.toFixed(0) }}</td>
              <td>{{ row.avgMs.toFixed(1) }}</td>
              <td>{{ row.maxMs.toFixed(0) }}</td>
              <td>{{ row.lastTs }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="log-analysis__empty">{{ t('logWindow.spanAggEmpty') }}</p>
      </section>

      <section class="log-analysis__section">
        <h3 class="log-analysis__heading">{{ t('logWindow.histogramTitle') }}</h3>
        <div class="log-analysis__histogram-controls">
          <select class="log-window__input" @change="onHistogramFileChange">
            <option value="">{{ t('logWindow.histogramFilePlaceholder') }}</option>
            <option v-for="f in store.historyFiles" :key="f.name" :value="f.name">{{ f.name }}</option>
          </select>
          <select v-model="histogramBucket" class="log-window__input">
            <option value="hour">{{ t('logWindow.bucketHour') }}</option>
            <option value="day">{{ t('logWindow.bucketDay') }}</option>
          </select>
          <UiButton
            variant="secondary"
            :disabled="!histogramFile"
            :loading="store.histogramLoading"
            @click="generateHistogram"
          >
            {{ t('logWindow.histogramGenerate') }}
          </UiButton>
        </div>
        <p v-if="store.histogram" class="log-analysis__stat">
          {{ t('logWindow.histogramCoverage', { parsed: store.histogram.parsedLines, total: store.histogram.totalLines }) }}
        </p>
        <LogHistogramChart
          v-if="store.histogram"
          :buckets="store.histogram.buckets"
          :bucket="store.histogramBucketUsed"
        />
      </section>

      <section class="log-analysis__section">
        <h3 class="log-analysis__heading">{{ t('logWindow.diagnosticsPackageTitle') }}</h3>
        <p class="log-analysis__stat">{{ t('logWindow.exportHint') }}</p>
        <div class="log-analysis__histogram-controls">
          <UiButton variant="secondary" :loading="exporting" @click="onExportDiagnostics">
            {{ t('logWindow.exportButton') }}
          </UiButton>
          <UiButton v-if="lastExportDir" variant="secondary" @click="onOpenExportDir">
            {{ t('logWindow.openLogDir') }}
          </UiButton>
        </div>
      </section>

      <section v-if="store.presets.length > 0" class="log-analysis__section">
        <h3 class="log-analysis__heading">{{ t('logWindow.presetManageTitle') }}</h3>
        <ul class="log-analysis__preset-list">
          <li v-for="p in store.presets" :key="p.name" class="log-analysis__preset-item">
            <span>{{ p.name }}</span>
            <button type="button" class="log-analysis__preset-delete" @click="onDeletePreset(p.name)">
              {{ t('logWindow.presetDelete') }}
            </button>
          </li>
        </ul>
      </section>
    </div>
    <template v-else>
      <LogVirtualList
        :key="activeTab"
        :entries="visibleEntries"
        :follow-bottom="activeTab === 'live'"
        :selected-seq="store.selectedSeq"
        @select="store.selectEntry"
      />
      <div v-if="store.contextView" class="log-window__context">
        <div class="log-window__context-header">
          <span>{{ t('logWindow.contextTitle') }}</span>
          <button type="button" class="log-window__context-close" @click="store.selectEntry(null)">
            {{ t('logWindow.contextClose') }}
          </button>
        </div>
        <div class="log-window__context-body">
          <LogRow
            v-for="(e, i) in contextEntries"
            :key="e._seq"
            :entry="e"
            :selected="i === contextSelectedIndex"
            @select="store.selectEntry"
          />
        </div>
      </div>
    </template>
    <ToastContainer />
  </div>
</template>

<style scoped>
.log-window {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
}

.log-window__toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: var(--toolbar-height);
  padding: 0 var(--spacing-md);
  border-bottom: 1px solid var(--color-divider);
  background: var(--color-bg-primary);
}

.log-window__tabs {
  display: flex;
  gap: var(--spacing-xs);
}
.log-window__tab {
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: var(--font-size-xs);
}
.log-window__tab--active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  border-color: transparent;
}

.log-window__filters {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  flex: 1 1 auto;
  min-width: 200px;
}

.log-window__actions {
  display: flex;
  gap: var(--spacing-sm);
}

.log-window__input {
  height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-input-border);
  background: var(--color-input-bg);
  color: var(--color-text-primary);
  font-size: var(--font-size-xs);
  min-width: 8em;
}
.log-window__input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.log-window__settings-error {
  font-size: var(--font-size-xs);
  color: var(--color-error);
  white-space: nowrap;
}
.log-window__input:focus {
  border-color: var(--color-input-border-focus);
  outline: none;
  box-shadow: var(--control-focus-ring);
}
.log-window__input--grow {
  flex: 1 1 auto;
  min-width: 12em;
}
.log-window__input--number {
  width: 6em;
  min-width: unset;
}
.log-window__input--invalid {
  border-color: var(--color-error);
}

.log-window__text-filter {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  flex: 1 1 auto;
  min-width: 12em;
}
.log-window__regex-toggle {
  flex-shrink: 0;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
}
.log-window__regex-toggle--active {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  border-color: transparent;
}

.log-window__subbar {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: 24px;
  padding: 0 var(--spacing-md);
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  border-bottom: 1px solid var(--color-divider);
}

.log-window__cap {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.log-window__context {
  flex: 0 0 auto;
  max-height: 40vh;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--color-divider);
  background: var(--color-bg-surface);
}
.log-window__context-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-md);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  border-bottom: 1px solid var(--color-divider);
}
.log-window__context-close {
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  font-size: var(--font-size-xs);
}
.log-window__context-body {
  overflow-y: auto;
}

.log-analysis {
  flex: 1 1 auto;
  overflow-y: auto;
  padding: var(--spacing-xl);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xl);
}
.log-analysis__section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}
.log-analysis__heading {
  margin: 0;
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
}
.log-analysis__stat {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.log-analysis__empty {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.log-analysis__table {
  border-collapse: collapse;
  font-size: var(--font-size-xs);
  font-family: var(--font-mono);
}
.log-analysis__table th,
.log-analysis__table td {
  padding: var(--spacing-xs) var(--spacing-sm);
  text-align: left;
  border-bottom: 1px solid var(--color-border);
}
.log-analysis__sample {
  max-width: 40em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.log-analysis__histogram-controls {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.log-analysis__preset-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}
.log-analysis__preset-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
  font-size: var(--font-size-xs);
  padding: var(--spacing-xs) 0;
}
.log-analysis__preset-delete {
  border: none;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  font-size: var(--font-size-xs);
}
.log-analysis__preset-delete:hover {
  color: var(--color-error);
}
</style>
