// 数据备份与恢复(方案 B):备份任务采用 app 级事件 + 后端快照恢复；恢复阶段由设置页向导
// 显式驱动。备份进度是粗粒度 running → terminal，不能伪装成 export 的分块百分比。

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { listen } from '@tauri-apps/api/event'
import { IPC, EVENTS } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'

export type BackupJobStatus = 'idle' | 'running' | 'completed' | 'failed' | 'cancelled'
export type BackupKind = 'backup' | 'auto'

export interface BackupProgressPayload {
  status: BackupJobStatus
  kind?: BackupKind
  path?: string
  bytes?: number
  code?: string
}

export interface BackupPreflight {
  destDir: string
  writable: boolean
  sameVolumeWarning?: boolean
  documentsConsistent: boolean
  estimatedBytes: number
}

export interface BackupListEntry {
  path: string
  fileName: string
  kind: BackupKind
  createdAtUtc: string
  backupId: string
  bytes: number
}

export interface BackupCounts {
  items: number
  albums: number
  tags: number
  namedPersons: number
}

export interface BackupRootEntry {
  id: number
  alias?: string
  hidden: boolean
}

export interface RestoreStageResult {
  backupId: string
  stagingDir: string
  schemaVersion: number
  needsMigration: boolean
  kind: BackupKind
  createdAtUtc: string
  counts: BackupCounts
  roots: BackupRootEntry[]
  externalDocumentVersions: number
  appdataDocumentCount: number
}

const IDLE_PROGRESS: BackupProgressPayload = { status: 'idle' }
const TERMINAL_STATUSES: BackupJobStatus[] = ['completed', 'failed', 'cancelled']

export const useBackupStore = defineStore('backup', () => {
  const progress = ref<BackupProgressPayload>(IDLE_PROGRESS)
  const isRunning = computed(() => progress.value.status === 'running')

  /**
   * 快照恢复是「先订阅、后查询」的两步异步操作。后端 payload 没有 jobId，因此只对迟到的
   * snapshot 禁止 terminal → running 回退；真正的新一轮 running event 必须允许覆盖旧终态。
   */
  function applyProgress(next: BackupProgressPayload, source: 'event' | 'snapshot') {
    if (
      source === 'snapshot' &&
      TERMINAL_STATUSES.includes(progress.value.status) &&
      next.status === 'running'
    ) {
      return
    }
    progress.value = next
  }

  let listenerPromise: Promise<unknown> | null = null
  function ensureListener(): Promise<unknown> {
    listenerPromise ??= listen<BackupProgressPayload>(EVENTS.BACKUP_PROGRESS, (event) =>
      applyProgress(event.payload, 'event'),
    ).catch((error) => {
      // listen 的失败 Promise 不可永久缓存，否则本次瞬时失败会让备份 UI 锁死到重启。
      listenerPromise = null
      throw error
    })
    return listenerPromise
  }

  async function restoreBackupProgress() {
    await ensureListener()
    const snapshot = await invokeIpc<BackupProgressPayload>(IPC.BACKUP_STATUS).catch(() => null)
    if (snapshot && snapshot.status !== 'idle') applyProgress(snapshot, 'snapshot')
  }

  async function preflightBackup(dest?: string): Promise<BackupPreflight> {
    return invokeIpc<BackupPreflight>(IPC.PREFLIGHT_BACKUP, { dest: dest || null })
  }

  async function startBackup(dest?: string) {
    await ensureListener()
    await invokeIpc(IPC.START_BACKUP, { dest: dest || null })
  }

  async function stopBackup() {
    if (!isRunning.value) return
    await invokeIpc(IPC.STOP_BACKUP)
  }

  async function listBackups(dest?: string): Promise<BackupListEntry[]> {
    return invokeIpc<BackupListEntry[]>(IPC.LIST_BACKUPS, { dest: dest || null })
  }

  async function restoreStage(packagePath: string): Promise<RestoreStageResult> {
    return invokeIpc<RestoreStageResult>(IPC.RESTORE_STAGE, { packagePath })
  }

  async function restoreArm(result: RestoreStageResult) {
    await invokeIpc(IPC.RESTORE_ARM, {
      backupId: result.backupId,
      stagingDir: result.stagingDir,
    })
  }

  async function relaunchApp() {
    await invokeIpc(IPC.RELAUNCH_APP)
  }

  return {
    progress,
    isRunning,
    restoreBackupProgress,
    preflightBackup,
    startBackup,
    stopBackup,
    listBackups,
    restoreStage,
    restoreArm,
    relaunchApp,
  }
})
