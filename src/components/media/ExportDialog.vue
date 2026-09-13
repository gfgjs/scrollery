<script setup lang="ts">
// src/components/media/ExportDialog.vue
// 导出整理成果对话框(方案 A §4):目的父目录、命名档、冲突策略、manifest 开关、预检摘要与
// 库内/旋转警告。全局单例(挂载一次,状态经 exportStore.dialogOpen 驱动),任意入口(选区工具条/
// 右键菜单/相册视图工具栏)只需 exportStore.openExportDialog(selection, source, rebuild) 即可唤出——
// 第三参 rebuild 是 ViewStale 重试时该入口自己的描述符重建回调(见 start() 内 resolveViewStaleRetry
// 调用处注释),不同入口各自持有,不共用同一份「借用当前选区」逻辑。

import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-dialog'
import UiDialog from '../ui/UiDialog.vue'
import UiField from '../ui/UiField.vue'
import UiSelect from '../ui/UiSelect.vue'
import UiCheckbox from '../ui/UiCheckbox.vue'
import UiButton from '../ui/UiButton.vue'
import {
  useExportStore,
  type ExportNamingScheme,
  type ExportConflictPolicy,
  type ExportPreflight,
  type ExportRequest,
} from '../../stores/exportStore'
import { useToastStore } from '../../stores/toastStore'
import { IpcError, ipcErrorMessage } from '../../utils/ipc'
import { formatFileSize } from '../../utils/format'

const { t } = useI18n()
const store = useExportStore()
const toast = useToastStore()

// manifest 默认关、记忆选择(方案 A-1 裁决)。
const MANIFEST_PREF_KEY = 'scrollery-export-include-manifest'

const targetParent = ref('')
const naming = ref<ExportNamingScheme>('sequence') // A-2:默认 sequence,保留手排/视图顺序
const conflict = ref<ExportConflictPolicy>('rename')
const includeManifest = ref(localStorage.getItem(MANIFEST_PREF_KEY) === 'true')
const allowInsideLibrary = ref(false)
const preflight = ref<ExportPreflight | null>(null)
const starting = ref(false)
const errorMessage = ref('')

watch(includeManifest, (v) => localStorage.setItem(MANIFEST_PREF_KEY, String(v)))

function buildRequest(): ExportRequest | null {
  if (!store.dialogSelection) return null
  return {
    selection: store.dialogSelection,
    targetParent: targetParent.value,
    naming: naming.value,
    conflict: conflict.value,
    includeManifest: includeManifest.value,
    source: store.dialogSource,
    allowInsideLibrary: allowInsideLibrary.value,
  }
}

// 并发守卫(审查 P2):打开对话框即发 preflight#1(大 selectAll 下 resolve+meta 查询可达秒级),
// 用户随即选目录触发 preflight#2 ——只接受最新一次请求的结果,防旧请求晚到覆盖新结果。
let preflightSeq = 0
async function runPreflight() {
  const req = buildRequest()
  if (!req) return
  const seq = ++preflightSeq
  errorMessage.value = ''
  try {
    const result = await store.preflightExport(req)
    if (seq !== preflightSeq) return
    preflight.value = result
  } catch (e) {
    if (seq !== preflightSeq) return
    errorMessage.value = ipcErrorMessage(e)
  }
}

// 每次打开都是新一轮:重置表单,先跑一次预检(targetParent 为空时后端 targetWritable 恒 false,
// 忽略即可——此时只为拿选区规模/估算体积/离线缺失/非零旋转四项计数)。
watch(
  () => store.dialogOpen,
  (isOpen) => {
    if (!isOpen) return
    targetParent.value = ''
    naming.value = 'sequence'
    conflict.value = 'rename'
    allowInsideLibrary.value = false
    preflight.value = null
    errorMessage.value = ''
    void runPreflight()
  },
)

async function pickTargetFolder() {
  const selected = await open({ directory: true, multiple: false, title: t('export.pickFolderTitle') })
  if (typeof selected !== 'string') return
  targetParent.value = selected
  allowInsideLibrary.value = false // 换目标须重新走库内确认
  await runPreflight()
}

const summaryLine = computed(() => {
  if (!preflight.value) return t('export.summaryCount', { count: 0 })
  return t('export.summaryWithSize', {
    count: preflight.value.count,
    size: formatFileSize(preflight.value.estimatedBytes),
  })
})

const canStart = computed(() => {
  if (!targetParent.value || !preflight.value) return false
  if (!preflight.value.targetWritable) return false
  if (preflight.value.insideLibraryWarning && !allowInsideLibrary.value) return false
  return true
})

// ViewStale(审查 P2/task_plan 43 行承诺项):selectAll 描述符携带 layout_version,对话框开启期间
// 布局重算(扫描/写操作 bump 版本)会使其过期,start_export 拒绝执行。不静默重试(选区本身可能已
// 随布局变化而不同,自动重试会在用户不知情下导出一份「已变过」的集合)——重取描述符、重跑预检,
// 提示用户核实后再次点击开始。
function isViewStale(e: unknown): boolean {
  return e instanceof IpcError && e.code === 'ViewStale'
}

async function start() {
  const req = buildRequest()
  if (!req || !canStart.value) return
  starting.value = true
  errorMessage.value = ''
  try {
    await store.startExport(req)
    store.closeExportDialog()
    toast.addToast('success', t('export.started'))
  } catch (e) {
    // 重建走开对话框那一刻由入口(选区/相册/视图)各自注册的回调,解析逻辑抽到 exportStore.
    // resolveViewStaleRetry(可独立单测的种子)——此前这里无条件用当前选区重建,会把相册/视图导出
    // 静默换成当前选区(外部审查【严重】)。回调缺失或返回 null 一律视为重试失败,走既有错误显示,
    // 严禁回退借用当前选区。
    const fresh = isViewStale(e) ? store.resolveViewStaleRetry() : null
    if (fresh) {
      store.dialogSelection = fresh
      errorMessage.value = t('export.viewStaleRetry')
      await runPreflight()
    } else {
      errorMessage.value = ipcErrorMessage(e)
    }
  } finally {
    starting.value = false
  }
}

function close() {
  // 审查 P3:starting 期间(start_export 请求在途)不可关框——否则启动失败只写进已不可见的
  // errorMessage,用户看不到失败提示、误以为已开始导出。UiDialog 的 close-on-esc/close-on-overlay
  // 已按 starting 门控,Cancel 按钮也已 disabled;这里再兜底一层,防任何遗漏路径调用 close()。
  if (starting.value) return
  store.closeExportDialog()
}
</script>

<template>
  <UiDialog
    :open="store.dialogOpen"
    :title="t('export.dialogTitle')"
    :close-label="t('common.close')"
    max-width="480px"
    :close-on-esc="!starting"
    :close-on-overlay="!starting"
    @close="close"
  >
    <p class="export-summary">{{ summaryLine }}</p>
    <p v-if="preflight && preflight.offlineOrMissingCount > 0" class="export-warning">
      {{ t('export.offlineWarning', { count: preflight.offlineOrMissingCount }) }}
    </p>
    <p v-if="preflight && preflight.nonZeroRotationCount > 0" class="export-hint">
      {{ t('export.rotationHint', { count: preflight.nonZeroRotationCount }) }}
    </p>

    <UiField :label="t('export.targetLabel')">
      <div class="export-target-row">
        <span class="export-target-path">{{ targetParent || t('export.targetPlaceholder') }}</span>
        <UiButton variant="secondary" @click="pickTargetFolder">{{ t('export.pickFolder') }}</UiButton>
      </div>
    </UiField>
    <p v-if="targetParent && preflight && !preflight.targetWritable" class="export-error">
      {{ t('export.targetNotWritable') }}
    </p>
    <div v-if="preflight?.insideLibraryWarning" class="export-warning-block">
      <p class="export-warning">{{ t('export.insideLibraryWarning') }}</p>
      <UiCheckbox v-model="allowInsideLibrary" :label="t('export.insideLibraryConfirm')" />
    </div>

    <UiField :label="t('export.namingLabel')">
      <UiSelect
        :model-value="naming"
        @update:model-value="(v) => (naming = v as ExportNamingScheme)"
      >
        <option value="sequence">{{ t('export.namingSequence') }}</option>
        <option value="original">{{ t('export.namingOriginal') }}</option>
        <option value="date">{{ t('export.namingDate') }}</option>
      </UiSelect>
    </UiField>

    <UiField :label="t('export.conflictLabel')">
      <UiSelect
        :model-value="conflict"
        @update:model-value="(v) => (conflict = v as ExportConflictPolicy)"
      >
        <option value="rename">{{ t('export.conflictRename') }}</option>
        <option value="skip">{{ t('export.conflictSkip') }}</option>
      </UiSelect>
    </UiField>

    <UiCheckbox v-model="includeManifest" :label="t('export.includeManifest')" />
    <p class="export-hint">{{ t('export.manifestHint') }}</p>

    <p v-if="errorMessage" class="export-error">{{ errorMessage }}</p>

    <template #footer>
      <UiButton variant="secondary" :disabled="starting" @click="close">
        {{ t('common.cancel') }}
      </UiButton>
      <UiButton variant="primary" :loading="starting" :disabled="!canStart || starting" @click="start">
        {{ t('export.start') }}
      </UiButton>
    </template>
  </UiDialog>
</template>

<style scoped>
.export-summary {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}
.export-warning {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--color-warning);
}
.export-hint {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}
.export-error {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--color-error);
}
.export-warning-block {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  padding: var(--spacing-sm);
  border: 1px solid var(--color-warning);
  border-radius: var(--radius-sm);
}
.export-target-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-width: 0;
}
.export-target-path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
}
</style>
