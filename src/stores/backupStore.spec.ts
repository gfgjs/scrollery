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
  it('先订阅事件再查询快照，running 能恢复并由事件推进终态', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.BACKUP_STATUS ? { status: 'running', kind: 'auto' } : undefined),
    )
    const store = useBackupStore()
    await store.restoreBackupProgress()

    expect(store.isRunning).toBe(true)
    expect(store.progress.kind).toBe('auto')
    listeners.get(EVENTS.BACKUP_PROGRESS)?.({
      payload: { status: 'completed', kind: 'auto', bytes: 1024, path: 'D:/b.scrollerybackup' },
    })
    expect(store.progress.status).toBe('completed')
    expect(store.progress.bytes).toBe(1024)
  })

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

  it('旧终态之后的新一轮 running 事件仍可进入运行态', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(cmd === IPC.BACKUP_STATUS ? { status: 'idle' } : undefined),
    )
    const store = useBackupStore()
    await store.restoreBackupProgress()
    listeners.get(EVENTS.BACKUP_PROGRESS)?.({ payload: { status: 'failed', code: 'backup_io' } })
    listeners.get(EVENTS.BACKUP_PROGRESS)?.({ payload: { status: 'running', kind: 'backup' } })
    expect(store.progress.status).toBe('running')
  })

  it('监听注册单次失败后允许重试', async () => {
    listenMock.mockImplementationOnce(() => Promise.reject(new Error('listen failed')))
    const store = useBackupStore()
    await expect(store.restoreBackupProgress()).rejects.toThrow('listen failed')
    invokeIpc.mockResolvedValue({ status: 'idle' })
    await expect(store.restoreBackupProgress()).resolves.toBeUndefined()
    expect(listenMock).toHaveBeenCalledTimes(2)
  })
})

describe('backupStore IPC 契约', () => {
  it('恢复 arm 只传后端契约需要的 backupId 与 stagingDir', async () => {
    const store = useBackupStore()
    await store.restoreArm({
      backupId: 'abc',
      stagingDir: 'C:/app/restore-staging/abc',
      schemaVersion: 21,
      needsMigration: false,
      kind: 'backup',
      createdAtUtc: '2026-07-19T00:00:00Z',
      counts: { items: 1, albums: 2, tags: 3, namedPersons: 4 },
      roots: [],
      externalDocumentVersions: 0,
      appdataDocumentCount: 5,
    })
    expect(invokeIpc).toHaveBeenCalledWith(IPC.RESTORE_ARM, {
      backupId: 'abc',
      stagingDir: 'C:/app/restore-staging/abc',
    })
  })

  it('空闲态 stopBackup 不发 IPC，运行态才发送 stop_backup', async () => {
    const store = useBackupStore()
    await store.stopBackup()
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.STOP_BACKUP)

    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.BACKUP_STATUS ? { status: 'running', kind: 'backup' } : undefined,
      ),
    )
    await store.restoreBackupProgress()
    await store.stopBackup()
    expect(invokeIpc).toHaveBeenCalledWith(IPC.STOP_BACKUP)
  })
})
