// 目录移动半完成恢复清单的 store 契约（P0-1）。
//
// 关注点（都是用户能直接踩到的控制流）：
//   ① 读清单只读：不触发任何物理动作；
//   ② 并发/在途读取不把「刚登记的新条目」丢在旧快照外（迟到刷新）；
//   ③ 同一日志 id 重试运行中不重复发起；
//   ④ 重试结果三分支的语义：已收尾（成功+刷新树）/ 仍需收尾（保留条目+具体原因，不报成功）/
//      无需收尾（目标根已删、双侧皆无 → 移除但不谎称完成）；
//   ⑤ 读取与重试自身失败都不丢恢复入口。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))
vi.mock('../utils/logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))
// 真实中文文案：断言里出现的是用户实际看到的那句话（键写错会暴露成键名）。
vi.mock('../i18n', async () => {
  const { createI18n } = await import('vue-i18n')
  const zhCN = (await import('../i18n/locales/zh-CN')).default
  return {
    default: createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } }),
  }
})

import { IPC } from '../constants/ipc'
import type { DirectoryMoveRecovery } from '../types/ipc'
import { useToastStore } from './toastStore'
import {
  useDirectoryMoveRecoveryStore,
  dirMoveRecoveryDetailKey,
} from './directoryMoveRecoveryStore'

const dispatchEvent = vi.fn()

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

/** 取第 n 次 invoke 的命令名与参数（mock.calls 的元素是 any，这里收成窄类型再用）。 */
function callAt(n: number): [string, Record<string, unknown> | undefined] {
  return invoke.mock.calls[n] as unknown as [string, Record<string, unknown> | undefined]
}

function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void } {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

/** 队列最后一条 toast（项目 TS target 不含 es2022，故不用 Array.prototype.at）。 */
function lastToast() {
  const toasts = useToastStore().toasts
  return toasts[toasts.length - 1]
}

beforeEach(() => {
  setActivePinia(createPinia())
  invoke.mockReset()
  dispatchEvent.mockReset()
  vi.stubGlobal('window', { dispatchEvent })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

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
