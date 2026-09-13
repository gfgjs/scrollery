import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('../utils/ipc', () => ({ invokeIpc: vi.fn(() => Promise.resolve(undefined)) }))

import { useBackupStore } from './backupStore'
import { useExportStore } from './exportStore'
import { useFileJobStore } from './fileJobStore'

beforeEach(() => setActivePinia(createPinia()))

describe('fileJobStore 共用投影', () => {
  it('投影 export 的确定进度与 backup 的不确定进度', () => {
    const files = useFileJobStore()
    const exports = useExportStore()
    const backups = useBackupStore()

    exports.progress = { jobId: 'e1', status: 'running', processed: 3, total: 8 }
    expect(files.activeJob).toEqual({ type: 'export', processed: 3, total: 8 })

    exports.progress = { jobId: 'e1', status: 'completed' }
    backups.progress = { status: 'running', kind: 'auto' }
    expect(files.activeJob).toEqual({ type: 'backup', backupKind: 'auto' })
  })
})
