import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { EVENTS, IPC } from '../constants/ipc'
import { useExportStore, type ExportProgressPayload } from './exportStore'

const { invokeIpc, listen } = vi.hoisted(() => ({ invokeIpc: vi.fn(), listen: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  listen.mockReset()
})

describe('exportStore 进度恢复', () => {
  it('新任务开始后，迟到的旧任务快照不能覆盖当前任务或使取消指向旧任务', async () => {
    let receiveProgress!: (event: { payload: ExportProgressPayload }) => void
    listen.mockImplementation((event: string, callback: typeof receiveProgress) => {
      expect(event).toBe(EVENTS.EXPORT_PROGRESS)
      receiveProgress = callback
      return Promise.resolve(() => {})
    })
    let resolveSnapshot!: (progress: ExportProgressPayload) => void
    const snapshot = new Promise<ExportProgressPayload>((resolve) => { resolveSnapshot = resolve })
    invokeIpc.mockImplementation((command: string) => (
      command === IPC.EXPORT_STATUS ? snapshot : Promise.resolve()
    ))
    const store = useExportStore()
    const restore = store.restoreExportProgress()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(invokeIpc).toHaveBeenCalledWith(IPC.EXPORT_STATUS)

    receiveProgress({ payload: { jobId: 'new-job', status: 'running', processed: 2, total: 10 } })
    resolveSnapshot({ jobId: 'old-job', status: 'completed', succeeded: 5 })
    await restore

    expect(store.isRunning).toBe(true)
    expect(store.progress).toEqual({ jobId: 'new-job', status: 'running', processed: 2, total: 10 })
    await store.stopExport()
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.STOP_EXPORT, { jobId: 'new-job' })
  })
})
