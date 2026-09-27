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
