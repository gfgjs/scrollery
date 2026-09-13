<script setup lang="ts">
// A/B 共用的轻量文件任务指示器:放 AppStatusBar 右区,展示唯一 export/backup job 的进度与取消。
// export 有确定 processed/total；backup 只有粗粒度 running → terminal，故显示不确定 spinner。
// 自动备份失败不弹 toast 打断用户，而是在这里保留可点击状态，入口直达设置页重试/换目录。

import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useBackupI18n } from '../../i18n/backupMessages'
import { AlertTriangle, X } from '@lucide/vue'
import { useRouter } from 'vue-router'
import { useExportStore } from '../../stores/exportStore'
import { useBackupStore } from '../../stores/backupStore'
import { useFileJobStore } from '../../stores/fileJobStore'
import { useToastStore } from '../../stores/toastStore'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { formatFileSize } from '../../utils/format'
import { parentDirectoryPath } from '../../utils/backupPresentation'

const { t } = useI18n()
const { t: bt } = useBackupI18n()
const router = useRouter()
const exportStore = useExportStore()
const backupStore = useBackupStore()
const fileJobs = useFileJobStore()
const toast = useToastStore()
const backupFailureDismissed = ref(false)

// 组件在状态栏异步挂载后立即接 app 事件 + 后端快照；WebView 重载时能续上正在运行的任务。
void fileJobs.restoreFileJobs()

const visible = computed(() => fileJobs.activeJob !== null)
const percent = computed(() => {
  if (fileJobs.activeJob?.type !== 'export') return 0
  const { processed, total } = fileJobs.activeJob
  if (!total) return 0
  return Math.min(100, Math.round((processed / total) * 100))
})
const showBackupFailure = computed(
  () =>
    backupStore.progress.status === 'failed' &&
    backupStore.progress.kind === 'auto' &&
    !backupFailureDismissed.value,
)

// 终态跳变(running → completed/failed/cancelled)弹一次性 toast;快照恢复(idle→completed 之类
// 非 running 起跳)不重复弹,避免刷新页面时把旧一轮的终态又提示一遍。
watch(
  () => exportStore.progress.status,
  (status, prev) => {
    if (prev !== 'running') return
    if (status === 'completed') {
      const finalDir = exportStore.progress.finalDir
      const actions = finalDir
        ? [
            {
              label: t('export.openFolder'),
              onClick: () => void invokeIpc(IPC.OPEN_DIRECTORY, { path: finalDir }),
            },
          ]
        : undefined
      // 审查 P3:后端专门传的 items/itemsTotal(单项跳过/失败明细及总数)此前零消费——
      // 执行期部分项 IO 失败(离线预检只覆盖开工前状态,不覆盖运行中失败)时,toast 只报
      // succeeded,用户看不出有项目未导出。itemsTotal > 0 时降级为 warning 并附失败数。
      const itemsTotal = exportStore.progress.itemsTotal ?? 0
      if (itemsTotal > 0) {
        toast.addToast(
          'warning',
          t('export.completedWithIssuesToast', {
            count: exportStore.progress.succeeded ?? 0,
            skipped: itemsTotal,
          }),
          6000,
          actions,
        )
      } else {
        toast.addToast(
          'success',
          t('export.completedToast', { count: exportStore.progress.succeeded ?? 0 }),
          6000,
          actions,
        )
      }
    } else if (status === 'failed') {
      toast.addToast('error', t('export.failedToast'))
    } else if (status === 'cancelled') {
      toast.addToast('info', t('export.cancelledToast'))
    }
  },
)

watch(
  () => backupStore.progress.status,
  (status, prev) => {
    if (status === 'running') backupFailureDismissed.value = false
    if (status === 'failed') backupFailureDismissed.value = false
    if (prev !== 'running') return

    const isAuto = backupStore.progress.kind === 'auto'
    if (status === 'completed' && !isAuto) {
      const path = backupStore.progress.path
      toast.addToast(
        'success',
        bt('backup.completedToast', {
          size: formatFileSize(backupStore.progress.bytes ?? 0),
        }),
        6000,
        path
          ? [
              {
                label: bt('backup.showPackage'),
                onClick: () => {
                  const directory = parentDirectoryPath(path)
                  if (directory) void invokeIpc(IPC.OPEN_DIRECTORY, { path: directory })
                },
              },
            ]
          : undefined,
      )
    } else if (status === 'failed' && !isAuto) {
      toast.addToast('error', bt('backup.failedToast'))
    } else if (status === 'cancelled' && !isAuto) {
      toast.addToast('info', bt('backup.cancelledToast'))
    }
  },
)

function cancel() {
  void fileJobs.cancelActive()
}

function openBackupSettings() {
  void router.push('/settings/storage')
}
</script>

<template>
  <span
    v-if="visible"
    class="statusbar__file-job"
    :title="
      fileJobs.activeJob?.type === 'export'
        ? t('export.progressTitle', { percent })
        : bt('backup.runningTitle')
    "
  >
    <span class="spinner" />
    <template v-if="fileJobs.activeJob?.type === 'export'">
      {{
        t('export.progressLabel', {
          processed: fileJobs.activeJob.processed,
          total: fileJobs.activeJob.total,
        })
      }}
    </template>
    <template v-else>
      {{
        bt(
          fileJobs.activeJob?.backupKind === 'auto'
            ? 'backup.autoRunningLabel'
            : 'backup.manualRunningLabel',
        )
      }}
    </template>
    <button
      type="button"
      class="statusbar__file-job-cancel"

      @click="cancel"
    >
      <X :size="12" />
    </button>
  </span>

  <span v-else-if="showBackupFailure" class="statusbar__file-job statusbar__file-job--failed">
    <button type="button" class="statusbar__file-job-link" @click="openBackupSettings">
      <AlertTriangle :size="13" />
      {{ bt('backup.backgroundFailedLabel') }}
    </button>
    <button
      type="button"
      class="statusbar__file-job-cancel"

      @click="backupFailureDismissed = true"
    >
      <X :size="12" />
    </button>
  </span>
</template>

<style scoped>
.statusbar__file-job {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  white-space: nowrap;
}

.statusbar__file-job-cancel {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
}
.statusbar__file-job-cancel:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.statusbar__file-job--failed {
  color: var(--color-warning);
}

.statusbar__file-job-link {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

.statusbar__file-job-link:hover {
  text-decoration: underline;
}
</style>
