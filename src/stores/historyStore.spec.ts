// historyStore 的目录移动/复制语义（P0-1）。
//
// 关注点：
//   ① 正常移动照旧进撤销栈；
//   ② 半完成移动（IPC 报 move_db_pending，或结果带 sourceLeftover/recoveryId）**不进撤销栈**——
//      反向移动会把「旧路径还有一份」的残留搬来搬去，得到两份内容；改为可重试的收尾任务；
//   ③ 复制成功但入库重扫失败：不把复制报成失败，明确提示「已复制、尚未完成入库」并给重扫动作。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))
vi.mock('../utils/logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))
const startScan = vi.fn(() => Promise.resolve())
vi.mock('./scanStore', () => ({ useScanStore: () => ({ startScan }) }))
vi.mock('../i18n', async () => {
  const { createI18n } = await import('vue-i18n')
  const zhCN = (await import('../i18n/locales/zh-CN')).default
  return {
    default: createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } }),
  }
})

import { IPC } from '../constants/ipc'
import type { CopyDirResult, MoveDirResult } from '../types/ipc'
import { useHistoryStore } from './historyStore'
import { useDirectoryMoveRecoveryStore } from './directoryMoveRecoveryStore'
import { useToastStore } from './toastStore'

const dispatchEvent = vi.fn()

function moveResult(over: Partial<MoveDirResult> = {}): MoveDirResult {
  return {
    dirId: 1,
    rootId: 5,
    newRelPath: '归档/旅行',
    affectedDirs: 2,
    affectedMedia: 3,
    targetAbsPath: 'D:/archive/旅行',
    sourceLeftover: null,
    recoveryId: null,
    ...over,
  }
}

function copyResult(over: Partial<CopyDirResult> = {}): CopyDirResult {
  return {
    createdRootId: 5,
    createdRelPath: '归档/旅行',
    createdAbsPath: 'D:/archive/旅行',
    copiedFiles: 12,
    needsRescan: true,
    ...over,
  }
}

function callAt(n: number): [string, Record<string, unknown> | undefined] {
  return invoke.mock.calls[n] as unknown as [string, Record<string, unknown> | undefined]
}

/** 队列里最后一条 toast（项目 TS target 不含 es2022，故不用 Array.prototype.at）。 */
function lastToast() {
  const toasts = useToastStore().toasts
  return toasts[toasts.length - 1]
}

beforeEach(() => {
  setActivePinia(createPinia())
  invoke.mockReset()
  dispatchEvent.mockReset()
  startScan.mockClear()
  startScan.mockImplementation(() => Promise.resolve())
  vi.stubGlobal('window', { dispatchEvent })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('historyStore：目录移动', () => {
  it('正常移动进撤销栈，不产生收尾任务', async () => {
    invoke.mockResolvedValue(moveResult())
    const history = useHistoryStore()

    await expect(history.move(1, '旅行', 9, 4)).resolves.toBe('complete')

    expect(history.undoStack).toHaveLength(1)
    expect(history.undoStack[0]).toMatchObject({ type: 'move', dirId: 1, toParentId: 4 })
    expect(useDirectoryMoveRecoveryStore().items).toHaveLength(0)
    expect(useToastStore().toasts).toHaveLength(0)
  })

  it('结果带源残留/recoveryId：不进撤销栈，登记可重试的收尾任务并给出真实落点', async () => {
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.MOVE_DIRECTORY
        ? Promise.resolve(moveResult({ sourceLeftover: 'C:/photos/旅行', recoveryId: 31 }))
        : Promise.resolve([]),
    )
    const history = useHistoryStore()

    await expect(history.move(1, '旅行', 9, 4)).resolves.toBe('pending')

    // 反向移动会把残留继续搬：这一条不能作为普通成功进入撤销历史。
    expect(history.undoStack).toHaveLength(0)
    const toast = lastToast()
    expect(toast?.type).toBe('warning')
    expect(toast?.message).toContain('D:/archive/旅行')
    expect(toast?.actions?.[0].label).toBe('重试收尾')

    await history.undo() // 栈空：不该发生任何 IPC
    expect(invoke.mock.calls.filter((c) => c[0] === IPC.MOVE_DIRECTORY)).toHaveLength(1)
  })

  it('IPC 报 move_db_pending：判定为半完成（不抛、不进撤销栈），重试动作按日志 id 发起', async () => {
    invoke.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.MOVE_DIRECTORY) {
        return Promise.reject({
          code: 'move_db_pending',
          message: '文件已移动到新位置，索引尚未更新',
          recoveryId: 42,
          targetAbsPath: 'D:/archive/旅行',
        })
      }
      if (cmd === IPC.LIST_PENDING_DIRECTORY_MOVES) {
        return Promise.resolve([
          {
            recoveryId: 42,
            stage: 'published',
            sourceName: '旅行',
            sourceAbsPath: 'C:/photos/旅行',
            targetAbsPath: 'D:/archive/旅行',
            targetRelPath: '归档/旅行',
            targetRootId: 5,
            needsRetry: true,
            detail: 'source_leftover',
          },
        ])
      }
      return Promise.reject(new Error(`unexpected ${String(cmd)}`))
    })
    const history = useHistoryStore()

    await expect(history.move(1, '旅行', 9, 4)).resolves.toBe('pending')

    expect(history.undoStack).toHaveLength(0)
    const toast = lastToast()
    expect(toast?.type).toBe('warning')
    expect(toast?.message).toContain('D:/archive/旅行')

    await toast?.actions?.[0].onClick()
    const retry = invoke.mock.calls.find((c) => c[0] === IPC.RETRY_DIRECTORY_MOVE)
    expect(retry?.[1]).toEqual({ recoveryId: 42 })
  })

  it('真失败（同名冲突）仍抛出：调用方保留原有错误分流', async () => {
    invoke.mockRejectedValue({ code: 'DirectoryExists', message: '旅行' })
    const history = useHistoryStore()

    await expect(history.move(1, '旅行', 9, 4)).rejects.toMatchObject({ code: 'DirectoryExists' })
    expect(history.undoStack).toHaveLength(0)
    expect(useDirectoryMoveRecoveryStore().items).toHaveLength(0)
  })
})

describe('historyStore：撤销 / 重做目录移动（与初次移动共用完整/半完成判定）', () => {
  /** 先做一次正常移动，让撤销栈里有一条 move 记录。 */
  async function withMovedFolder() {
    invoke.mockResolvedValue(moveResult())
    const history = useHistoryStore()
    await history.move(1, '旅行', 9, 4)
    useToastStore().toasts.splice(0)
    return history
  }

  it('完整撤销：移回原位、进重做栈并报成功', async () => {
    const history = await withMovedFolder()

    await history.undo()

    const moves = invoke.mock.calls.filter((c) => c[0] === IPC.MOVE_DIRECTORY)
    expect(moves[1]?.[1]).toEqual({ sourceDirId: 1, targetDirId: 9 }) // 回到原父目录
    expect(history.undoStack).toHaveLength(0)
    expect(history.redoStack).toHaveLength(1)
    expect(lastToast()?.type).toBe('success')
    expect(lastToast()?.message).toContain('已撤销移动')
  })

  it('撤销遇到 move_db_pending：登记恢复任务、丢弃该历史项、不进重做栈、不报成功', async () => {
    const history = await withMovedFolder()
    invoke.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.MOVE_DIRECTORY) {
        return Promise.reject({
          code: 'move_db_pending',
          message: '文件已移动到新位置，索引尚未更新',
          recoveryId: 77,
          targetAbsPath: 'C:/photos/旅行',
        })
      }
      return Promise.resolve([])
    })

    await history.undo()

    // 半完成：文件已回原位置但没走完，反向再执行一次同样不安全 → 这条历史必须丢掉。
    expect(history.undoStack).toHaveLength(0)
    expect(history.redoStack).toHaveLength(0)
    const toast = lastToast()
    expect(toast?.type).toBe('warning') // 不是 undoFailed（普通失败），也不是 success
    expect(toast?.message).toContain('C:/photos/旅行')
    expect(toast?.message).toContain('收尾尚未完成')
    expect(toast?.actions?.[0].label).toBe('重试收尾')
    expect(useToastStore().toasts.some((to) => to.message.includes('已撤销移动'))).toBe(false)

    await toast?.actions?.[0].onClick()
    expect(invoke.mock.calls.find((c) => c[0] === IPC.RETRY_DIRECTORY_MOVE)?.[1]).toEqual({
      recoveryId: 77,
    })
  })

  it('撤销结果带源残留/recoveryId：同样丢弃历史项并登记恢复任务', async () => {
    const history = await withMovedFolder()
    invoke.mockResolvedValue(moveResult({ sourceLeftover: 'D:/archive/旅行', recoveryId: 12 }))

    await history.undo()

    expect(history.undoStack).toHaveLength(0)
    expect(history.redoStack).toHaveLength(0)
    expect(lastToast()?.type).toBe('warning')
    expect(useToastStore().toasts.some((to) => to.message.includes('已撤销移动'))).toBe(false)
  })

  it('重做遇到半完成：同样丢弃该历史项、不报成功', async () => {
    const history = await withMovedFolder()
    await history.undo() // 完整撤销后 move 记录进重做栈
    expect(history.redoStack).toHaveLength(1)
    useToastStore().toasts.splice(0)
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.MOVE_DIRECTORY
        ? Promise.reject({
            code: 'move_db_pending',
            message: '文件已移动到新位置，索引尚未更新',
            recoveryId: 88,
            targetAbsPath: 'D:/archive/旅行',
          })
        : Promise.resolve([]),
    )

    await history.redo()

    expect(history.redoStack).toHaveLength(0)
    expect(history.undoStack).toHaveLength(0) // 不能回流成可撤销项
    expect(lastToast()?.type).toBe('warning')
    expect(useToastStore().toasts.some((to) => to.message.includes('已重做移动'))).toBe(false)
  })
})

describe('historyStore：目录复制与入库', () => {
  it('入库重扫成功：进撤销栈且不出现「待入库」提示', async () => {
    invoke.mockResolvedValue(copyResult())
    const history = useHistoryStore()

    await history.copy(1, '旅行', 4)

    expect(startScan).toHaveBeenCalledTimes(1)
    expect(history.undoStack[0]).toMatchObject({
      type: 'copy',
      createdRootId: 5,
      createdAbsPath: 'D:/archive/旅行',
    })
    expect(useToastStore().toasts).toHaveLength(0)
  })

  it('入库重扫失败：复制不算失败，明确提示已复制待入库并给重扫动作', async () => {
    invoke.mockResolvedValue(copyResult())
    startScan.mockRejectedValueOnce(new Error('扫描根不可用'))
    const history = useHistoryStore()

    await expect(history.copy(1, '旅行', 4)).resolves.toBeUndefined()

    // 撤销记录仍要留下：物理副本确实存在，撤销必须能删掉它。
    expect(history.undoStack[0]).toMatchObject({ type: 'copy', createdAbsPath: 'D:/archive/旅行' })
    const toast = lastToast()
    expect(toast?.type).toBe('warning')
    expect(toast?.message).toContain('已复制')
    expect(toast?.message).toContain('尚未完成入库')
    expect(toast?.message).toContain('D:/archive/旅行')
    expect(toast?.message).toContain('12') // 落盘文件数来自 CopyDirResult.copiedFiles
    expect(toast?.actions?.[0].label).toBe('重新扫描')

    // 动作走的是既有的重扫入口（同一个 startScan），不另开一条文件操作通道。
    await toast?.actions?.[0].onClick()
    expect(startScan).toHaveBeenCalledTimes(2)
    expect(callAt(0)[0]).toBe(IPC.COPY_DIRECTORY)
  })
})
