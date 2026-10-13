import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { EVENTS, IPC } from '../constants/ipc'

const invokeIpc = vi.fn((..._args: unknown[]) => Promise.resolve<unknown>(undefined))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
}))

const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }))
const listeners = new Map<string, (event: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

import { useBackupStore } from './backupStore'

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  invokeIpc.mockImplementation(() => Promise.resolve(undefined))
  listeners.clear()
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb)
    return Promise.resolve(() => {})
  })
})

describe('backupStore 进度恢复', () => {

  it('completed 事件先到时，迟到的 running 快照不得覆盖终态', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.BACKUP_STATUS ? { status: 'running', kind: 'backup' } : undefined,
      ),
    )
    const store = useBackupStore()
    const restore = store.restoreBackupProgress()
    listeners.get(EVENTS.BACKUP_PROGRESS)?.({
      payload: { status: 'completed', kind: 'backup', path: 'D:/done.scrollerybackup' },
    })
    await restore
    expect(store.progress.status).toBe('completed')
    expect(store.progress.path).toContain('done.scrollerybackup')
  })

  it('新备份 running 事件先到时迟到的旧 completed 快照不能隐藏停止入口', async () => {
    let resolveSnapshot!: (value: unknown) => void
    const snapshot = new Promise<unknown>((resolve) => { resolveSnapshot = resolve })
    let notifyRequested!: () => void
    const requested = new Promise<void>((resolve) => { notifyRequested = resolve })
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.BACKUP_STATUS) {
        notifyRequested()
        return snapshot
      }
      return Promise.resolve(undefined)
    })
    const store = useBackupStore()
    const restore = store.restoreBackupProgress()
    await requested
    expect(invokeIpc).toHaveBeenCalledWith(IPC.BACKUP_STATUS)

    listeners.get(EVENTS.BACKUP_PROGRESS)?.({ payload: { status: 'running', kind: 'auto' } })
    resolveSnapshot({ status: 'completed', kind: 'backup', path: 'D:/old.scrollerybackup' })
    await restore

    expect(store.isRunning).toBe(true)
    expect(store.progress).toEqual({ status: 'running', kind: 'auto' })
    await store.stopBackup()
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.STOP_BACKUP)
  })
})
