<template>
  <CollapsibleCard id="backup" :title="bt('backup.sectionTitle')" @toggle="emit('toggle', $event)">
    <div class="backup-intro">
      <DatabaseBackup :size="18" />
      <p>{{ bt('backup.privacyNotice') }}</p>
    </div>

    <div class="backup-row backup-row--destination">
      <div class="backup-row__info">
        <strong>{{ bt('backup.destination') }}</strong>
        <button
          v-if="destination"
          type="button"
          class="backup-path"
          :title="$t('settings.openInExplorer')"
          @click="openDestination"
        >
          {{ destination }}
        </button>
        <span v-else class="backup-muted">{{ bt('backup.destinationUnset') }}</span>
      </div>
      <UiButton @click="chooseDestination">{{ bt('backup.chooseDestination') }}</UiButton>
    </div>

    <div v-if="preflight?.sameVolumeWarning" class="backup-alert backup-alert--warning">
      <AlertTriangle :size="16" />
      <span>{{ bt('backup.sameVolumeWarning') }}</span>
    </div>
    <div v-if="preflight && !preflight.writable" class="backup-alert backup-alert--error">
      <AlertTriangle :size="16" />
      <span>{{ bt('backup.errorNotWritable') }}</span>
    </div>
    <div
      v-if="preflight && !preflight.documentsConsistent"
      class="backup-alert backup-alert--error"
    >
      <AlertTriangle :size="16" />
      <span>{{ bt('backup.errorDocumentInconsistent') }}</span>
    </div>

    <div class="backup-row">
      <div class="backup-row__info">
        <strong>{{ bt('backup.automaticTitle') }}</strong>
        <span class="backup-muted">{{ bt('backup.automaticDesc') }}</span>
      </div>
      <UiToggle
        :model-value="autoEnabled"
        :disabled="!canEnableAuto"
        :label="bt('backup.automaticTitle')"
        @update:model-value="setAutoEnabled"
      />
    </div>

    <div class="backup-row">
      <div class="backup-row__info">
        <strong>{{ bt('backup.retention') }}</strong>
        <span class="backup-muted">{{ bt('backup.retentionDesc') }}</span>
      </div>
      <label class="backup-retention">
        <input
          v-model.number="retention"
          type="number"
          min="1"
          max="100"
          :disabled="!autoEnabled"
          @change="saveRetention"
        />
        <span>{{ bt('backup.copies') }}</span>
      </label>
    </div>

    <div class="backup-row backup-row--status">
      <div class="backup-row__info">
        <strong>{{ bt('backup.latestStatus') }}</strong>
        <span :class="latestStatusClass">{{ latestStatusLabel }}</span>
        <span v-if="lastAutoSuccess" class="backup-muted">
          {{ bt('backup.lastAutoSuccess', { time: lastAutoSuccess }) }}
        </span>
      </div>
      <div class="backup-actions">
        <UiButton :disabled="!destination || backup.isRunning" @click="refreshAll">
          {{ bt('backup.refresh') }}
        </UiButton>
        <UiButton v-if="backup.isRunning" variant="danger" @click="backup.stopBackup()">
          {{ $t('common.stop') }}
        </UiButton>
        <UiButton
          v-else
          variant="primary"
          :loading="starting"
          :disabled="!canStart"
          @click="startManualBackup"
        >
          {{ bt('backup.backupNow') }}
        </UiButton>
      </div>
    </div>

    <div v-if="preflight" class="backup-estimate">
      {{ bt('backup.estimatedSize', { size: formatFileSize(preflight.estimatedBytes) }) }}
    </div>

    <section class="backup-list-section">
      <header class="backup-list-header">
        <div>
          <strong>{{ bt('backup.listTitle') }}</strong>
          <span class="backup-muted">{{ bt('backup.listDesc') }}</span>
        </div>
        <UiButton variant="secondary" @click="chooseRestoreFile">
          {{ bt('backup.restoreFromFile') }}
        </UiButton>
      </header>

      <div v-if="listLoading" class="backup-empty">{{ $t('common.loading') }}</div>
      <div v-else-if="!backups.length" class="backup-empty">{{ bt('backup.listEmpty') }}</div>
      <div v-else class="backup-list">
        <article v-for="entry in backups" :key="entry.backupId" class="backup-item">
          <FileArchive :size="17" />
          <div class="backup-item__info">
            <strong :title="entry.fileName">{{ entry.fileName }}</strong>
            <span>
              {{ formatDate(entry.createdAtUtc) }} · {{ formatFileSize(entry.bytes) }} ·
              {{ bt(entry.kind === 'auto' ? 'backup.kindAuto' : 'backup.kindManual') }}
            </span>
          </div>
          <div class="backup-item__actions">
            <button
              type="button"
              class="backup-icon-btn"
              :title="bt('backup.showPackage')"

              @click="showPackage(entry.path)"
            >
              <FolderOpen :size="15" />
            </button>
            <UiButton @click="openRestoreWizard(entry.path)">{{ bt('backup.restore') }}</UiButton>
          </div>
        </article>
      </div>
    </section>
  </CollapsibleCard>

  <RestoreWizard
    :open="restoreOpen"
    :package-path="restorePackagePath"
    @close="restoreOpen = false"
  />
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useBackupI18n } from '../../i18n/backupMessages'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, DatabaseBackup, FileArchive, FolderOpen } from '@lucide/vue'
import CollapsibleCard from './CollapsibleCard.vue'
import RestoreWizard from './RestoreWizard.vue'
import UiButton from '../ui/UiButton.vue'
import UiToggle from '../ui/UiToggle.vue'
import {
  useBackupStore,
  type BackupListEntry,
  type BackupPreflight,
} from '../../stores/backupStore'
import { useConfigStore } from '../../stores/configStore'
import { useToastStore } from '../../stores/toastStore'
import { useConfirm } from '../../composables/useConfirm'
import { IPC } from '../../constants/ipc'
import { invokeIpc, IpcError } from '../../utils/ipc'
import { backupErrorKey, parentDirectoryPath } from '../../utils/backupPresentation'
import { formatFileSize } from '../../utils/format'

const backup = useBackupStore()
const config = useConfigStore()
const toast = useToastStore()
const { confirm } = useConfirm()
const { locale } = useI18n()
const { t: bt } = useBackupI18n()
const emit = defineEmits<{
  toggle: [open: boolean]
}>()

const destination = ref('')
const autoEnabled = ref(false)
const retention = ref(5)
const lastSuccessEpoch = ref<number | null>(null)
const preflight = ref<BackupPreflight | null>(null)
const backups = ref<BackupListEntry[]>([])
const listLoading = ref(false)
const starting = ref(false)
const restoreOpen = ref(false)
const restorePackagePath = ref('')

const canEnableAuto = computed(
  () =>
    !!destination.value &&
    preflight.value?.writable === true &&
    preflight.value.documentsConsistent,
)
const canStart = computed(() => canEnableAuto.value && !backup.isRunning)
const lastAutoSuccess = computed(() =>
  lastSuccessEpoch.value
    ? new Date(lastSuccessEpoch.value * 1000).toLocaleString(locale.value)
    : '',
)
const latestStatusClass = computed(() => ({
  'backup-status': true,
  'backup-status--success': backup.progress.status === 'completed',
  'backup-status--error': backup.progress.status === 'failed',
}))
const latestStatusLabel = computed(() => {
  const progress = backup.progress
  if (progress.status === 'running') {
    return bt(progress.kind === 'auto' ? 'backup.autoRunningLabel' : 'backup.manualRunningLabel')
  }
  if (progress.status === 'completed') {
    return bt('backup.latestCompleted', { size: formatFileSize(progress.bytes ?? 0) })
  }
  if (progress.status === 'failed') return bt(backupErrorKey(progress.code))
  if (progress.status === 'cancelled') return bt('backup.cancelledToast')
  return bt('backup.latestIdle')
})

onMounted(async () => {
  const [dir, enabled, keep, last] = await Promise.all([
    loadConfig('backup_dir'),
    loadConfig('backup_auto_enabled'),
    loadConfig('backup_retention'),
    loadConfig('backup_last_success_at'),
  ])
  destination.value = dir ?? ''
  autoEnabled.value = enabled === 'true' && !!destination.value
  retention.value = clampRetention(Number.parseInt(keep ?? '5', 10))
  const parsedLast = Number.parseInt(last ?? '', 10)
  lastSuccessEpoch.value = Number.isFinite(parsedLast) ? parsedLast : null
  if (destination.value) await refreshAll()
})

watch(
  () => backup.progress.status,
  (status, previous) => {
    if (previous !== 'running' || status !== 'completed') return
    void refreshBackups()
    if (backup.progress.kind === 'auto') void refreshLastAutoSuccess()
  },
)

async function loadConfig(key: string): Promise<string | null> {
  return invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key }).catch(() => null)
}

async function refreshLastAutoSuccess() {
  const value = await loadConfig('backup_last_success_at')
  const parsed = Number.parseInt(value ?? '', 10)
  lastSuccessEpoch.value = Number.isFinite(parsed) ? parsed : null
}

function clampRetention(value: number): number {
  return Number.isFinite(value) ? Math.min(100, Math.max(1, Math.round(value))) : 5
}

async function chooseDestination() {
  const hadDestination = !!destination.value
  const selected = await openDialog({
    directory: true,
    multiple: false,
    title: bt('backup.chooseDestinationTitle'),
  })
  if (!selected || typeof selected !== 'string') return

  destination.value = selected
  await config.saveConfig('backup_dir', selected)
  await refreshAll()

  // 改选既有目录时保留用户当前自动策略；只在首次设置目的地时给出 B-1 显式建议。
  if (hadDestination) return
  // 不可写或文档不一致时后端必拒绝；此时不建议开启一个注定失败的定时任务。
  if (!canEnableAuto.value) return

  // B-1:选目录不等于已默认保护；明确建议「每日 + 保留 5」，让用户显式接受或保留手动模式。
  const { confirmed } = await confirm({
    title: bt('backup.autoSuggestionTitle'),
    message: bt('backup.autoSuggestionMessage'),
    confirmText: bt('backup.enableDaily'),
    cancelText: bt('backup.manualOnly'),
  })
  retention.value = 5
  await config.saveConfig('backup_retention', '5')
  await setAutoEnabled(confirmed)
}

async function setAutoEnabled(enabled: boolean) {
  if (enabled && !canEnableAuto.value) return
  autoEnabled.value = enabled
  await config.saveConfig('backup_auto_enabled', String(enabled))
}

async function saveRetention() {
  retention.value = clampRetention(retention.value)
  await config.saveConfig('backup_retention', String(retention.value))
}

async function refreshAll() {
  if (!destination.value) return
  await Promise.all([refreshPreflight(), refreshBackups()])
}

async function refreshPreflight() {
  try {
    preflight.value = await backup.preflightBackup(destination.value)
  } catch (error) {
    preflight.value = null
    const code = error instanceof IpcError ? error.code : undefined
    toast.addToast('error', bt(backupErrorKey(code)))
  }
}

async function refreshBackups() {
  if (!destination.value || listLoading.value) return
  listLoading.value = true
  try {
    backups.value = await backup.listBackups(destination.value)
  } catch (error) {
    const code = error instanceof IpcError ? error.code : undefined
    toast.addToast('error', bt(backupErrorKey(code)))
  } finally {
    listLoading.value = false
  }
}

async function startManualBackup() {
  if (starting.value) return
  starting.value = true
  try {
    await refreshPreflight()
    if (!canStart.value) return
    await backup.startBackup(destination.value)
    toast.addToast('info', bt('backup.startedToast'))
  } catch (error) {
    const code = error instanceof IpcError ? error.code : undefined
    toast.addToast('error', bt(backupErrorKey(code)))
  } finally {
    starting.value = false
  }
}

async function chooseRestoreFile() {
  const selected = await openDialog({
    directory: false,
    multiple: false,
    title: bt('backup.chooseRestoreTitle'),
    filters: [{ name: bt('backup.packageFilter'), extensions: ['scrollerybackup'] }],
  })
  if (selected && typeof selected === 'string') openRestoreWizard(selected)
}

function openRestoreWizard(path: string) {
  restorePackagePath.value = path
  restoreOpen.value = true
}

function formatDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale.value)
}

function openDestination() {
  if (destination.value) void invokeIpc(IPC.OPEN_DIRECTORY, { path: destination.value })
}

function showPackage(path: string) {
  const directory = parentDirectoryPath(path)
  if (directory) void invokeIpc(IPC.OPEN_DIRECTORY, { path: directory })
}
</script>

<style scoped>
.backup-intro,
.backup-row,
.backup-list-header,
.backup-item {
  display: flex;
  align-items: center;
  gap: 12px;
}
.backup-intro {
  padding: 12px 16px;
  color: var(--color-text-secondary);
  background: color-mix(in srgb, var(--color-accent) 5%, transparent);
}
.backup-intro svg {
  flex: 0 0 auto;
  color: var(--color-accent);
}
.backup-intro p {
  margin: 0;
  font-size: var(--font-size-xs);
  line-height: 1.6;
}
.backup-row {
  justify-content: space-between;
  padding: 12px 16px;
  border-top: 1px solid var(--color-border);
}
.backup-row__info,
.backup-list-header > div {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.backup-row__info strong,
.backup-list-header strong {
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
}
.backup-muted,
.backup-estimate,
.backup-list-header span {
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}
.backup-path {
  max-width: 100%;
  padding: 0;
  overflow: hidden;
  border: 0;
  background: transparent;
  color: var(--color-text-secondary);
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.backup-path:hover {
  color: var(--color-accent);
  text-decoration: underline;
}
.backup-alert {
  margin: 0 16px 10px;
  padding: 9px 11px;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  border-radius: var(--radius-md);
  font-size: var(--font-size-xs);
  line-height: 1.5;
}
.backup-alert svg {
  flex: 0 0 auto;
}
.backup-alert--warning {
  color: var(--color-warning);
  background: color-mix(in srgb, var(--color-warning) 10%, transparent);
}
.backup-alert--error {
  color: var(--color-error);
  background: color-mix(in srgb, var(--color-error) 9%, transparent);
}
.backup-retention {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
.backup-retention input {
  width: 70px;
  padding: 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
}
.backup-actions,
.backup-item__actions {
  display: flex;
  align-items: center;
  gap: 7px;
  flex-shrink: 0;
}
.backup-status {
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}
.backup-status--success {
  color: var(--color-success);
}
.backup-status--error {
  color: var(--color-error);
}
.backup-estimate {
  padding: 0 16px 12px;
  text-align: right;
}
.backup-list-section {
  border-top: 1px solid var(--color-border);
}
.backup-list-header {
  justify-content: space-between;
  padding: 12px 16px;
}
.backup-list,
.backup-empty {
  margin: 0 16px 12px;
}
.backup-empty {
  padding: 14px;
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text-secondary);
  text-align: center;
  font-size: var(--font-size-sm);
}
.backup-item {
  padding: 10px 0;
  border-top: 1px solid var(--color-border);
}
.backup-item > svg {
  flex: 0 0 auto;
  color: var(--color-text-secondary);
}
.backup-item__info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.backup-item__info strong {
  overflow: hidden;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.backup-item__info span {
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}
.backup-icon-btn {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
}
.backup-icon-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
@media (max-width: 700px) {
  .backup-row,
  .backup-list-header,
  .backup-item {
    align-items: stretch;
    flex-direction: column;
  }
  .backup-actions,
  .backup-item__actions {
    justify-content: flex-end;
  }
}
</style>
