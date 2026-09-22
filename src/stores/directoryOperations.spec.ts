// 目录操作：集中移动、复制、撤销重做和半完成恢复；共用 IPC 与用户提示 fixture。

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
import type { CopyDirResult, MoveDirResult, DirectoryMoveRecovery } from '../types/ipc'
import { useHistoryStore } from './historyStore'
import { useDirectoryMoveRecoveryStore, dirMoveRecoveryDetailKey } from './directoryMoveRecoveryStore'
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

describe('historyStore：幂等回调失败', () => {
  it.each(['undo', 'redo'] as const)('%s 失败保留原栈位置与重试入口，不报告成功', async (direction) => {
    const undo = vi.fn(() => Promise.resolve())
    const redo = vi.fn(() => Promise.resolve())
    const history = useHistoryStore()
    history.pushUndoable({ undo, redo, undoMessage: 'undo done', redoMessage: 'redo done' })
    if (direction === 'redo') await history.undo()
    const source = direction === 'undo' ? history.undoStack : history.redoStack
    const target = direction === 'undo' ? history.redoStack : history.undoStack
    const action = direction === 'undo' ? undo : redo
    useToastStore().toasts.splice(0)
    action.mockRejectedValueOnce(new Error('temporarily unavailable'))

    await history[direction]()
    expect(source).toHaveLength(1)
    expect(target).toHaveLength(0)
    expect(lastToast()?.type).toBe('error')
    expect(history.busy).toBe(false)

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

function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void } {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

describe('目录移动恢复清单：读取', () => {
  it('读清单只读：只发 list_pending_directory_moves，条目按后端报告填充', async () => {
    invoke.mockResolvedValue([report(), report({ recoveryId: 2, detail: 'staging_busy' })])
    const store = useDirectoryMoveRecoveryStore()

    await store.load()

    expect(invoke).toHaveBeenCalledTimes(1)
    expect(callAt(0)[0]).toBe(IPC.LIST_PENDING_DIRECTORY_MOVES)
    expect(store.items.map((i) => i.recoveryId)).toEqual([1, 2])
    expect(store.pendingCount).toBe(2)
    expect(dispatchEvent).not.toHaveBeenCalled() // 读清单不动目录树/画廊
  })

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

  it('读清单失败保留现有条目：一次读取失败不该让恢复入口消失', async () => {
    invoke.mockResolvedValueOnce([report()])
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    invoke.mockRejectedValueOnce(new Error('db busy'))
    await expect(store.load()).resolves.toBeUndefined()

    expect(store.items).toHaveLength(1)
    expect(store.loading).toBe(false)
  })

  it('契约外响应（UI harness 的兜底 null）按空清单处理，不抛错', async () => {
    invoke.mockResolvedValue(null)
    const store = useDirectoryMoveRecoveryStore()

    await store.load()

    expect(store.items).toEqual([])
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

  it('旧读过期不影响后续读取：重读回传的就是权威清单', async () => {
    const slowRead = deferred<DirectoryMoveRecovery[]>()
    let listCalls = 0
    invoke.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.LIST_PENDING_DIRECTORY_MOVES) {
        listCalls += 1
        return listCalls === 1 ? slowRead.promise : Promise.resolve([report({ recoveryId: 5 })])
      }
      return Promise.resolve(report({ detail: 'completed', needsRetry: false }))
    })
    const store = useDirectoryMoveRecoveryStore()
    store.items = [report()]

    const loading = store.load()
    await store.retry(1)
    slowRead.resolve([report()])
    await loading

    expect(store.items.map((i) => i.recoveryId)).toEqual([5])
  })
})

describe('目录移动恢复清单：单条重试', () => {
  it('收尾完成：移除条目 + 刷新目录树/画廊 + 成功提示', async () => {
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.RETRY_DIRECTORY_MOVE
        ? Promise.resolve(report({ detail: 'completed', needsRetry: false }))
        : Promise.resolve([report()]),
    )
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    const done = await store.retry(1)

    expect(done).toBe(true)
    expect(store.items).toHaveLength(0)
    expect(callAt(1)[0]).toBe(IPC.RETRY_DIRECTORY_MOVE)
    expect(callAt(1)[1]).toEqual({ recoveryId: 1 })
    expect(dispatchEvent).toHaveBeenCalledTimes(1)
    expect(dispatchEvent.mock.calls[0][0]).toMatchObject({ type: 'folder-stats-changed' })
    expect(lastToast()?.type).toBe('success')
    expect(lastToast()?.message).toContain('已收尾')
  })

  it('后端已无该日志行（null）：视作已收尾，移除条目而不是留在清单里', async () => {
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.RETRY_DIRECTORY_MOVE ? Promise.resolve(null) : Promise.resolve([report()]),
    )
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    expect(await store.retry(1)).toBe(true)
    expect(store.items).toHaveLength(0)
    expect(callAt(1)[1]).toEqual({ recoveryId: 1 })
  })

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

  it('无需收尾（目标扫描根已删）：移除条目并说明，不谎称「已收尾」', async () => {
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.RETRY_DIRECTORY_MOVE
        ? Promise.resolve(report({ detail: 'target_root_missing', needsRetry: false }))
        : Promise.resolve([report()]),
    )
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    expect(await store.retry(1)).toBe(true)
    expect(store.items).toHaveLength(0)
    const last = lastToast()
    expect(last?.type).toBe('info')
    expect(last?.message).toContain('无需收尾')
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

  it('重试自身失败：条目留在清单里可再试，并给错误提示', async () => {
    invoke.mockImplementation((cmd: unknown) =>
      cmd === IPC.LIST_PENDING_DIRECTORY_MOVES
        ? Promise.resolve([report()])
        : Promise.reject({ code: 'Io', message: '目标盘不可写' }),
    )
    const store = useDirectoryMoveRecoveryStore()
    await store.load()

    expect(await store.retry(1)).toBe(false)

    expect(store.items).toHaveLength(1)
    expect(store.isRetrying(1)).toBe(false)
    const last = lastToast()
    expect(last?.type).toBe('error')
    expect(last?.message).toContain('目标盘不可写')
  })
})

describe('目录移动恢复清单：登记半完成移动', () => {
  it('提示里带真实落点与可点的重试动作，并读一次清单让管理区显示', async () => {
    invoke.mockResolvedValue([report({ recoveryId: 42 })])
    const store = useDirectoryMoveRecoveryStore()

    store.notePending({ recoveryId: 42, targetAbsPath: 'D:/archive/旅行', name: '旅行' })
    const toast = lastToast()
    expect(toast?.type).toBe('warning')
    expect(toast?.message).toContain('D:/archive/旅行')
    expect(toast?.actions?.[0].label).toBe('重试收尾')

    await store.load() // 等登记触发的这次清单读取落地
    expect(callAt(0)[0]).toBe(IPC.LIST_PENDING_DIRECTORY_MOVES)
    expect(store.items.map((i) => i.recoveryId)).toEqual([42])
  })
})

describe('detail 文案映射', () => {
  it('白名单标签映射到自己的文案键，陌生标签回落 unknown', () => {
    expect(dirMoveRecoveryDetailKey('source_leftover')).toBe(
      'sidebar.dirRecovery.detail.source_leftover',
    )
    expect(dirMoveRecoveryDetailKey('target_conflict')).toBe(
      'sidebar.dirRecovery.detail.target_conflict',
    )
    expect(dirMoveRecoveryDetailKey('brand_new_code')).toBe('sidebar.dirRecovery.detail.unknown')
    expect(dirMoveRecoveryDetailKey('')).toBe('sidebar.dirRecovery.detail.unknown')
  })
})
