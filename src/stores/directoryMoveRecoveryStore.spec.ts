import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { IPC } from '../constants/ipc'
import type { DirectoryMoveRecovery } from '../types/ipc'

const { invokeIpc, addToast } = vi.hoisted(() => ({
  invokeIpc: vi.fn(),
  addToast: vi.fn(),
}))
vi.mock('../utils/ipc', () => ({ invokeIpc, ipcErrorMessage: String }))
vi.mock('./toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))

import {
  dirMoveRecoveryDetailKey,
  useDirectoryMoveRecoveryStore,
} from './directoryMoveRecoveryStore'

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  addToast.mockReset()
  vi.stubGlobal('window', { dispatchEvent: vi.fn() })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('目录移动恢复清单：索引已提交的部分完成', () => {
  it.each(['source_leftover', 'source_changed', 'source_cleanup_required'])('%s 保留恢复入口并刷新已迁移的目录与图库', async (detail) => {
    // 新标签必须命中自身文案，不能落到 unknown（未知标签会把稳定原因吞成「状态未知」）。
    expect(dirMoveRecoveryDetailKey(detail)).toBe(`sidebar.dirRecovery.detail.${detail}`)
    const pending: DirectoryMoveRecovery = {
      recoveryId: 7,
      stage: 'published',
      sourceName: '旅行',
      sourceAbsPath: 'C:/photos/旅行',
      targetAbsPath: 'D:/archive/旅行',
      targetRelPath: '旅行',
      targetRootId: 2,
      needsRetry: true,
      detail: 'published',
    }
    const indexed = { ...pending, stage: 'source_leftover', detail }
    let completeRetry: (report: DirectoryMoveRecovery) => void = () => {}
    const retryResult = new Promise<DirectoryMoveRecovery>((resolve) => { completeRetry = resolve })
    invokeIpc.mockImplementation((command: string) =>
      command === IPC.LIST_PENDING_DIRECTORY_MOVES ? Promise.resolve([pending]) : retryResult,
    )
    const store = useDirectoryMoveRecoveryStore()
    await store.load()
    expect(window.dispatchEvent).not.toHaveBeenCalled()

    const retry = store.retry(pending.recoveryId)
    expect(store.isRetrying(pending.recoveryId)).toBe(true)
    // finish_entry 先完成 apply_move_db，再发现源无法删除或包含新内容，返回仍需收尾。
    completeRetry(indexed)
    expect(await retry).toBe(false)

    expect(store.items).toEqual([indexed])
    expect(store.pendingCount).toBe(1)
    expect(store.isRetrying(pending.recoveryId)).toBe(false)
    expect(window.dispatchEvent).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ type: 'folder-stats-changed' }),
    )
    expect(addToast).toHaveBeenCalledExactlyOnceWith(
      'warning', 'sidebar.dirRecovery.stillPending', 8000,
    )
    expect(invokeIpc.mock.calls).toEqual([
      [IPC.LIST_PENDING_DIRECTORY_MOVES],
      [IPC.RETRY_DIRECTORY_MOVE, { recoveryId: pending.recoveryId }],
    ])
  })
})
