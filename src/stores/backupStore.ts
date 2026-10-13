// 数据备份与恢复：备份任务采用 app 级事件 + 后端快照恢复；恢复阶段由设置页向导
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
  /** 本次预检的所有权标识，同备份重新预检后旧标识失效。 */
  stagingToken: string
  stagingDir: string
  schemaVersion: number
  kind: BackupKind
  createdAtUtc: string
  counts: BackupCounts
  roots: BackupRootEntry[]
  externalDocumentVersions: number
  appdataDocumentCount: number
}

const IDLE_PROGRESS: BackupProgressPayload = { status: 'idle' }

export const useBackupStore = defineStore('backup', () => {
  const progress = ref<BackupProgressPayload>(IDLE_PROGRESS)
  const isRunning = computed(() => progress.value.status === 'running')

  // 快照恢复期间若收到新事件，以事件为准，避免旧任务快照覆盖新任务状态。
  let progressEventGeneration = 0

  let listenerPromise: Promise<unknown> | null = null
  function ensureListener(): Promise<unknown> {
    listenerPromise ??= listen<BackupProgressPayload>(EVENTS.BACKUP_PROGRESS, (event) => {
      progressEventGeneration++
      progress.value = event.payload
    }).catch((error) => {
      // listen 的失败 Promise 不可永久缓存，否则本次瞬时失败会让备份 UI 锁死到重启。
      listenerPromise = null
      throw error
    })
    return listenerPromise
  }

  async function restoreBackupProgress() {
    const generation = progressEventGeneration
    await ensureListener()
    const snapshot = await invokeIpc<BackupProgressPayload>(IPC.BACKUP_STATUS).catch(() => null)
    if (snapshot && snapshot.status !== 'idle' && generation === progressEventGeneration) {
      progress.value = snapshot
    }
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

  /** 释放尚未安排重启恢复的暂存数据，路径仅由后端根据备份标识派生。 */
  async function restoreDiscard(result: RestoreStageResult): Promise<void> {
    await invokeIpc<void>(IPC.RESTORE_DISCARD, { backupId: result.backupId, stagingToken: result.stagingToken })
  }

  async function restoreArm(result: RestoreStageResult) {
    await invokeIpc(IPC.RESTORE_ARM, {
      backupId: result.backupId,
      stagingToken: result.stagingToken,
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
    restoreDiscard,
    restoreArm,
    relaunchApp,
  }
})
