// A/B 共用文件任务投影。后端 file-job gate 保证 export / backup 同时至多一个运行；本 store
// 只统一状态栏所需的名称、进度与取消动作，不吞并两个域各自的请求/终态细节。

import { computed } from 'vue'
import { defineStore } from 'pinia'
import { useBackupStore } from './backupStore'
import { useExportStore } from './exportStore'

export type ActiveFileJob =
  | {
      type: 'export'
      processed: number
      total: number
    }
  | {
      type: 'backup'
      backupKind: 'backup' | 'auto'
    }

export const useFileJobStore = defineStore('fileJob', () => {
  const exportStore = useExportStore()
  const backupStore = useBackupStore()

  const activeJob = computed<ActiveFileJob | null>(() => {
    if (exportStore.progress.status === 'running') {
      return {
        type: 'export',
        processed: exportStore.progress.processed ?? 0,
        total: exportStore.progress.total ?? 0,
      }
    }
    if (backupStore.progress.status === 'running') {
      return {
        type: 'backup',
        backupKind: backupStore.progress.kind ?? 'backup',
      }
    }
    return null
  })

  async function restoreFileJobs() {
    // 一路监听失败不应阻断另一路恢复；各 store 会在下次发起任务时重新尝试注册。
    await Promise.allSettled([
      exportStore.restoreExportProgress(),
      backupStore.restoreBackupProgress(),
    ])
  }

  async function cancelActive() {
    if (activeJob.value?.type === 'export') await exportStore.stopExport()
    else if (activeJob.value?.type === 'backup') await backupStore.stopBackup()
  }

  return { activeJob, restoreFileJobs, cancelActive }
})
