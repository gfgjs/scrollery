// 目录操作：集中移动、复制、撤销重做和半完成恢复；共用 IPC 与用户提示 fixture。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  Channel: class { onmessage = (_message: unknown) => {} },
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
import type { CopyDirResult, MoveDirResult, DirectoryMoveRecovery, DirectoryCopyProgress } from '../types/ipc'
import { useHistoryStore } from './historyStore'
import { useCollectionStore } from './collectionStore'
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

describe('historyStore：媒体批量操作失败后的界面刷新', () => {
  it('移动部分失败仍刷新已生效结果，不创建完整成功的撤销记录', async () => {
    const history = useHistoryStore()
    invoke.mockRejectedValue({ code: 'MoveFile', message: 'second item failed' })
    await expect(history.moveMedia([1, 2], 9, 'photos')).rejects.toThrow('second item failed')
    expect(dispatchEvent).toHaveBeenCalledOnce()
    expect(dispatchEvent.mock.calls[0][0].type).toBe('folder-stats-changed')
    expect(history.undoStack).toHaveLength(0)
    expect(history.busy).toBe(false)
  })

  it('复制部分失败仍刷新已落库副本，不创建未知副本的撤销记录', async () => {
    const history = useHistoryStore()
    invoke.mockRejectedValue({ code: 'CopyFile', message: 'second item failed' })
    await expect(history.copyMedia([1, 2], 9, 'photos')).rejects.toThrow('second item failed')
    expect(dispatchEvent).toHaveBeenCalledOnce()
    expect(dispatchEvent.mock.calls[0][0].type).toBe('folder-stats-changed')
    expect(history.undoStack).toHaveLength(0)
    expect(history.busy).toBe(false)
  })

  it('撤销复制部分失败仍刷新已移除的副本且不提示成功', async () => {
    const history = useHistoryStore()
    invoke.mockResolvedValueOnce({
      completed: [{ srcId: 1, newId: 11 }, { srcId: 2, newId: 12 }],
      failure: null,
    })
    await history.copyMedia([1, 2], 9, 'photos')
    dispatchEvent.mockClear()
    invoke.mockRejectedValueOnce({ code: 'Os', message: 'second removal failed' })
    await history.undo()
    expect(dispatchEvent).toHaveBeenCalledOnce()
    expect(dispatchEvent.mock.calls[0][0].type).toBe('folder-stats-changed')
    expect(history.redoStack).toHaveLength(0)
    expect(lastToast().type).toBe('error')
  })
})

describe('historyStore：幂等回调失败', () => {
  it.each(['undo'] as const)('%s 失败保留原栈位置与重试入口，不报告成功', async (direction) => {
    const collections = useCollectionStore()
    const undo = () => collections.restore(7)
    const redo = () => collections.remove(7)
    const history = useHistoryStore()
    history.pushUndoable({ undo, redo, undoMessage: 'undo done', redoMessage: 'redo done' })
    const source = direction === 'undo' ? history.undoStack : history.redoStack
    const target = direction === 'undo' ? history.redoStack : history.undoStack
    useToastStore().toasts.splice(0)
    // 经收藏夹真实写入链制造失败，避免 store 吞错却被纯回调替身漏过。
    invoke.mockResolvedValue([]).mockRejectedValueOnce({ code: 'Db', message: 'temporarily unavailable' })

    await history[direction]()
    expect(source).toHaveLength(1)
    expect(target).toHaveLength(0)
    expect(lastToast()?.type).toBe('error')
    expect(history.busy).toBe(false)
    expect(invoke).toHaveBeenCalledWith(IPC.RESTORE_COLLECTION, { albumId: 7 })

    await history[direction]()
    expect(source).toHaveLength(0)
    expect(target).toHaveLength(1)
    expect(lastToast()?.type).toBe('success')
  })
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
})

describe('historyStore：目录复制与入库', () => {
  it('累计复制进度按代次取消，取消完成后不入库也不登记撤销', async () => {
    const pending = deferred<CopyDirResult>()
    invoke.mockReturnValueOnce(pending.promise).mockResolvedValue(undefined)
    const history = useHistoryStore()
    const copy = history.copy(1, '旅行', 4)
    expect(history.copyProgress).toMatchObject({ name: '旅行', generation: null })
    await history.cancelDirectoryCopy()
    expect(invoke).toHaveBeenCalledTimes(1)
    copyChannel().onmessage({ generation: 7, copiedFiles: 2, copiedBytes: 65536, publishing: false })
    expect(history.copyProgress).toMatchObject({ copiedFiles: 2, copiedBytes: 65536 })
    await history.cancelDirectoryCopy()
    await history.cancelDirectoryCopy()
    expect(callAt(1)).toEqual([IPC.CANCEL_DIRECTORY_COPY, { generation: 7 }])
    expect(invoke).toHaveBeenCalledTimes(2)
    expect(history.copyProgress?.cancelling).toBe(true)
    pending.reject({ code: 'Cancelled', message: 'cancelled' })
    await expect(copy).rejects.toMatchObject({ code: 'Cancelled' })
    expect(history.copyProgress).toBeNull()
    expect(history.busy).toBe(false)
    expect(history.undoStack).toHaveLength(0)
    expect(startScan).not.toHaveBeenCalled()
  })

  it('取消到达发布边界后仍以成功结果入库并保留撤销', async () => {
    const pending = deferred<CopyDirResult>()
    invoke.mockReturnValueOnce(pending.promise).mockResolvedValue(undefined)
    const history = useHistoryStore()
    const copy = history.copy(1, '旅行', 4)
    const channel = copyChannel()
    channel.onmessage({ generation: 8, copiedFiles: 1, copiedBytes: 42, publishing: false })
    await history.cancelDirectoryCopy()
    channel.onmessage({ generation: 8, copiedFiles: 1, copiedBytes: 42, publishing: true })
    expect(history.copyProgress).toMatchObject({ publishing: true, cancelling: false })
    await history.cancelDirectoryCopy()
    expect(invoke).toHaveBeenCalledTimes(2)
    pending.resolve(copyResult())
    await copy
    expect(history.undoStack).toHaveLength(1)
    expect(startScan).toHaveBeenCalledOnce()
    expect(history.copyProgress).toBeNull()
  })

  it('取消重做保留原记录，重试成功后才移入撤销栈', async () => {
    const history = useHistoryStore()
    invoke.mockResolvedValueOnce(copyResult({ needsRescan: false })).mockResolvedValueOnce(undefined)
    await history.copy(1, '旅行', 4)
    await history.undo()
    const record = history.redoStack[0]
    invoke.mockRejectedValueOnce({ code: 'Cancelled', message: 'cancelled' })
    await history.redo()
    expect(history.redoStack[0]).toBe(record)
    expect(history.undoStack).toHaveLength(0)
    expect(lastToast()).toMatchObject({ type: 'info', message: '已取消复制' })
    invoke.mockResolvedValueOnce(copyResult({ needsRescan: false }))
    await history.redo()
    expect(history.redoStack).toHaveLength(0)
    expect(history.undoStack).toHaveLength(1)
    expect(lastToast().type).toBe('success')
  })

  it('T19-FE-01:新编辑使在途复制重做分支失效后，取消仅提示且不复活旧记录', async () => {
    const history = useHistoryStore()
    invoke.mockResolvedValueOnce(copyResult({ needsRescan: false })).mockResolvedValueOnce(undefined)
    await history.copy(1, '旅行', 4)
    await history.undo()
    const oldBranch = history.redoStack
    const pending = deferred<CopyDirResult>()
    invoke.mockReturnValueOnce(pending.promise)
    const redoing = history.redo()

    // 复制重做等待后端期间，新的软删除可以登记历史并作废旧重做分支。
    const undoNew = vi.fn(async () => {})
    const redoNew = vi.fn(async () => {})
    const id = history.pushUndoable({
      undo: undoNew, redo: redoNew, undoMessage: '恢复新编辑', redoMessage: '重做新编辑',
    })
    expect(history.redoStack).not.toBe(oldBranch)
    pending.reject({ code: 'Cancelled', message: 'cancelled' })
    await redoing

    expect(history.undoStack).toMatchObject([{ type: 'callback', id }])
    expect(history.redoStack).toEqual([])
    expect(lastToast()).toMatchObject({ type: 'info', message: '已取消复制' })
    expect(history.copyProgress).toBeNull()
    expect(history.busy).toBe(false)
    expect(startScan).not.toHaveBeenCalled()
    await history.undoIfTop(id)
    await history.redo()
    expect(undoNew).toHaveBeenCalledOnce()
    expect(redoNew).toHaveBeenCalledOnce()
    expect(history.undoStack).toMatchObject([{ type: 'callback', id }])
    expect(history.redoStack).toEqual([])
    expect(invoke).toHaveBeenCalledTimes(3)
  })

  it('旧通道迟到和并发新请求不覆盖正在运行的复制', async () => {
    const history = useHistoryStore()
    invoke.mockResolvedValueOnce(copyResult({ needsRescan: false }))
    await history.copy(1, '旅行', 4)
    const old = copyChannel()
    old.onmessage({ generation: 1, copiedFiles: 99, copiedBytes: 9999, publishing: true })
    expect(history.copyProgress).toBeNull()
    const pending = deferred<CopyDirResult>()
    invoke.mockReturnValueOnce(pending.promise)
    const current = history.copy(2, '新目录', 4)
    copyChannel(1).onmessage({ generation: 2, copiedFiles: 0, copiedBytes: 42, publishing: false })
    old.onmessage({ generation: 1, copiedFiles: 99, copiedBytes: 9999, publishing: true })
    await expect(history.copy(3, '第三个目录', 4)).rejects.toMatchObject({ code: 'FileJobBusy' })
    expect(history.copyProgress).toMatchObject({ name: '新目录', generation: 2, copiedBytes: 42, publishing: false })
    expect(history.busy).toBe(true)
    expect(invoke).toHaveBeenCalledTimes(2)
    pending.resolve(copyResult({ needsRescan: false }))
    await current
  })

  it('取消命令失败保留复制进度并恢复取消入口', async () => {
    const pending = deferred<CopyDirResult>()
    invoke.mockReturnValueOnce(pending.promise).mockRejectedValueOnce({ code: 'Internal', message: 'unavailable' })
    const history = useHistoryStore()
    const copy = history.copy(1, '旅行', 4)
    copyChannel().onmessage({ generation: 3, copiedFiles: 0, copiedBytes: 42, publishing: false })
    await history.cancelDirectoryCopy()
    expect(history.copyProgress).toMatchObject({ generation: 3, cancelling: false })
    expect(lastToast()).toMatchObject({ type: 'error', message: '取消复制失败：unavailable' })
    pending.resolve(copyResult({ needsRescan: false }))
    await copy
    expect(history.undoStack).toHaveLength(1)
  })

  it('复制期间其他操作失败不提前开放撤销重做', async () => {
    const history = useHistoryStore()
    invoke.mockResolvedValueOnce(copyResult({ needsRescan: false })).mockResolvedValueOnce(undefined)
    await history.copy(1, '旅行', 4)
    await history.undo()
    const pending = deferred<CopyDirResult>()
    invoke.mockReturnValueOnce(pending.promise).mockRejectedValueOnce({ code: 'InvalidMove', message: 'file job busy' })
    const copy = history.copy(2, '新目录', 4)
    await expect(history.move(3, '另一个目录', 5, 4)).rejects.toMatchObject({ code: 'InvalidMove' })
    expect(history.canRedo).toBe(false)
    await history.redo()
    expect(invoke).toHaveBeenCalledTimes(4)
    expect(history.copyProgress?.name).toBe('新目录')
    pending.resolve(copyResult({ needsRescan: false }))
    await copy
    expect(history.canUndo).toBe(true)
    expect(history.canRedo).toBe(false)
  })

  it('复制目标离线时撤销保留记录，恢复访问后可重试同一目标', async () => {
    const history = useHistoryStore()
    invoke.mockResolvedValueOnce(copyResult({ needsRescan: false }))
    await history.copy(1, '旅行', 4)
    const record = history.undoStack[0]
    dispatchEvent.mockClear()
    invoke.mockRejectedValueOnce({ code: 'PathResolution', message: 'scan root missing' })

    await history.undo()

    expect(history.undoStack[0]).toBe(record)
    expect(history.canUndo).toBe(true)
    expect(history.canRedo).toBe(false)
    expect(lastToast().type).toBe('error')
    expect(dispatchEvent).not.toHaveBeenCalled()

    invoke.mockResolvedValueOnce(undefined)
    await history.undo()
    expect(callAt(2)).toEqual(callAt(1))
    expect(callAt(2)).toEqual([IPC.DELETE_DIRECTORY_TO_TRASH, {
      absPath: 'D:/archive/旅行', rootId: 5, relPath: '归档/旅行',
    }])
    expect(history.canUndo).toBe(false)
    expect(history.canRedo).toBe(true)
    expect(lastToast().type).toBe('success')
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

function report(over: Partial<DirectoryMoveRecovery> = {}): DirectoryMoveRecovery {
  return {
    recoveryId: 1,
    stage: 'published',
    sourceName: '旅行',
    sourceAbsPath: 'C:/photos/旅行',
    targetAbsPath: 'D:/archive/旅行',
    targetRelPath: '旅行',
    targetRootId: 2,
    needsRetry: true,
    detail: 'source_leftover',
    ...over,
  }
}

function copyChannel(callIndex = 0): { onmessage: (progress: DirectoryCopyProgress) => void } {
  return callAt(callIndex)[1]?.onProgress as { onmessage: (progress: DirectoryCopyProgress) => void }
}

function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void; reject: (reason: unknown) => void } {
  let resolve!: (v: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('目录移动恢复清单：读取', () => {

  it('在途读取期间的登记会补读一次，新条目不落在旧快照外', async () => {
    const first = deferred<DirectoryMoveRecovery[]>()
    const second = deferred<DirectoryMoveRecovery[]>()
    invoke.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise)
    const store = useDirectoryMoveRecoveryStore()

    const loading = store.load()
    // 读取在途时后端刚写下一条半完成记录（登记路径内部会再读一次清单）。
    store.notePending({ recoveryId: 7, targetAbsPath: 'D:/archive/旅行', name: '旅行' })
    first.resolve([])
    second.resolve([report({ recoveryId: 7 })])
    await loading

    expect(invoke).toHaveBeenCalledTimes(2)
    expect(store.items.map((i) => i.recoveryId)).toEqual([7])
  })

  it('旧读在重试收尾之后才回传：该快照失效并重读，不把已收尾的条目复活', async () => {
    const slowRead = deferred<DirectoryMoveRecovery[]>()
    let listCalls = 0
    invoke.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.LIST_PENDING_DIRECTORY_MOVES) {
        listCalls += 1
        return listCalls === 1 ? slowRead.promise : Promise.resolve([])
      }
      return Promise.resolve(report({ detail: 'completed', needsRetry: false }))
    })
    const store = useDirectoryMoveRecoveryStore()
    store.items = [report()]

    const loading = store.load() // 在途读（慢）
    expect(await store.retry(1)).toBe(true) // 收尾完成 → 条目已 drop
    expect(store.items).toHaveLength(0)

    slowRead.resolve([report()]) // 旧读此刻才回传，快照里还带着刚收尾掉的条目
    await loading

    expect(listCalls).toBe(2) // 过期快照被丢弃并重读一次
    expect(store.items).toEqual([]) // 已完成任务不复活
  })
})

describe('目录移动恢复清单：单条重试', () => {

  it('目标离线/冲突等仍待收尾：保留条目并说明原因，不报成功', async () => {
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.RETRY_DIRECTORY_MOVE
        ? Promise.resolve(report({ detail: 'target_conflict', stage: 'intent' }))
        : Promise.resolve([report()]),
    )
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    expect(await store.retry(1)).toBe(false)

    expect(store.items).toHaveLength(1)
    expect(store.items[0].detail).toBe('target_conflict')
    const last = lastToast()
    expect(last?.type).toBe('warning')
    expect(last?.message).toContain('源与目标同时存在') // detail 经 UI 文案而非生码
    expect(dispatchEvent).not.toHaveBeenCalled() // 没改库、没动磁盘就不刷新树
  })

  it('同一日志 id 运行中不重复发起；跑完后标志复位', async () => {
    const retryCall = deferred<DirectoryMoveRecovery>()
    let retryCalls = 0
    invoke.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.RETRY_DIRECTORY_MOVE) {
        retryCalls += 1
        return retryCall.promise
      }
      return Promise.resolve([report()])
    })
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    const running = store.retry(1)
    expect(store.isRetrying(1)).toBe(true)
    // 第二次点击（或快捷键触发）在途时被挡下，不产生第二个 IPC。
    expect(await store.retry(1)).toBe(false)
    expect(retryCalls).toBe(1)

    retryCall.resolve(report({ detail: 'completed', needsRetry: false }))
    expect(await running).toBe(true)
    expect(store.isRetrying(1)).toBe(false)
  })
})
