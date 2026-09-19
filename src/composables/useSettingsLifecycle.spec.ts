// src/composables/useSettingsLifecycle.spec.ts
// 退出 flush 协议的局部测试(设置集中保存,批次C)。覆盖三件容易写错的事:
//   1. 单飞——重叠请求不会并行写盘;
//   2. 失败回执 ok=false(不把未保存当已保存),成功后 ok=true;
//   3. 同一 requestId 重复投递只回执一次。
// 协议的对端(后端 lifecycle::flush_ack)按 requestId 记账、收齐才放行退出,故这三条是退出不丢
// 设置的直接前提。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPendingSettings, installSettingsLifecycle, disposeSettingsLifecycle } from './useSettingsLifecycle'

/** 中央保存集合的落盘入口(被测模块的依赖,整体替身)。 */
const flushSettings = vi.fn<() => Promise<void>>()
const invokeIpc = vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>()
/** 事件监听:捕获注册进来的处理器,测试里手动投递事件。 */
let handlers: Array<(event: { payload: { requestId: string } }) => void> = []
const unlistenSpy = vi.fn()

vi.mock('../stores/settingsPersistence', () => ({
  flushSettings: () => flushSettings(),
}))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (cmd: string, args?: Record<string, unknown>) => invokeIpc(cmd, args),
}))
vi.mock('../utils/appEvents', () => ({
  listenAppEvent: async (
    _event: string,
    handler: (event: { payload: { requestId: string } }) => void,
  ) => {
    handlers.push(handler)
    return unlistenSpy
  },
}))
vi.mock('../utils/logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

/** 造一个可手动结算的 Promise,用于观察「在途期间」的行为。 */
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

/** 等待微任务清空——被测代码用 void async 收尾,需要让这些微任务跑完。 */
function flushMicrotasks(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

beforeEach(() => {
  handlers = []
  vi.clearAllMocks()
  flushSettings.mockResolvedValue(undefined)
  invokeIpc.mockResolvedValue(undefined)
  disposeSettingsLifecycle()
})

afterEach(() => {
  disposeSettingsLifecycle()
})

describe('useSettingsLifecycle', () => {
  it('重叠请求复用同一次 flush(不并行写盘)', async () => {
    const pending = deferred<void>()
    flushSettings.mockReturnValueOnce(pending.promise)

    const first = flushPendingSettings()
    const second = flushPendingSettings()
    expect(second).toBe(first)
    expect(flushSettings).toHaveBeenCalledTimes(1)

    pending.resolve()
    await first
    // 结算后再调会开新的一轮(单飞闸已释放)。
    await flushPendingSettings()
    expect(flushSettings).toHaveBeenCalledTimes(2)
  })

  it('flush 成功后回执 ok=true', async () => {
    await installSettingsLifecycle()
    handlers[0]({ payload: { requestId: 'flush-1' } })
    await flushMicrotasks()

    expect(flushSettings).toHaveBeenCalledTimes(1)
    expect(invokeIpc).toHaveBeenCalledWith('settings_flush_done', {
      requestId: 'flush-1',
      ok: true,
    })
  })

  it('flush 失败回执 ok=false,不把未保存当已保存', async () => {
    flushSettings.mockRejectedValueOnce(new Error('disk full'))
    await installSettingsLifecycle()
    handlers[0]({ payload: { requestId: 'flush-2' } })
    await flushMicrotasks()

    expect(invokeIpc).toHaveBeenCalledWith('settings_flush_done', {
      requestId: 'flush-2',
      ok: false,
    })
  })

  it('同一 requestId 失败时也只回执一次 ok=false(与成功路径同规则)', async () => {
    flushSettings.mockRejectedValue(new Error('disk full'))
    await installSettingsLifecycle()
    handlers[0]({ payload: { requestId: 'flush-fail-dup' } })
    await flushMicrotasks()
    handlers[0]({ payload: { requestId: 'flush-fail-dup' } })
    await flushMicrotasks()

    expect(flushSettings).toHaveBeenCalledTimes(2)
    const acks = invokeIpc.mock.calls.filter((c) => c[0] === 'settings_flush_done')
    expect(acks).toHaveLength(1)
    expect((acks[0][1] as { ok: boolean }).ok).toBe(false)
  })

  it('同一 requestId 重复投递只重跑 flush、不重复回执', async () => {
    await installSettingsLifecycle()
    handlers[0]({ payload: { requestId: 'flush-3' } })
    await flushMicrotasks()
    handlers[0]({ payload: { requestId: 'flush-3' } })
    await flushMicrotasks()

    expect(flushSettings).toHaveBeenCalledTimes(2)
    const acks = invokeIpc.mock.calls.filter((c) => c[0] === 'settings_flush_done')
    expect(acks).toHaveLength(1)
  })

  it('不同 requestId 各自回执', async () => {
    await installSettingsLifecycle()
    handlers[0]({ payload: { requestId: 'flush-4' } })
    await flushMicrotasks()
    handlers[0]({ payload: { requestId: 'flush-5' } })
    await flushMicrotasks()

    const ids = invokeIpc.mock.calls
      .filter((c) => c[0] === 'settings_flush_done')
      .map((c) => (c[1] as { requestId: string }).requestId)
    expect(ids).toEqual(['flush-4', 'flush-5'])
  })

  it('缺 requestId 的请求被忽略(不落盘、不回执)', async () => {
    await installSettingsLifecycle()
    handlers[0]({ payload: undefined as unknown as { requestId: string } })
    await flushMicrotasks()

    expect(flushSettings).not.toHaveBeenCalled()
    expect(invokeIpc).not.toHaveBeenCalled()
  })

  it('重复装配不重复注册监听', async () => {
    const first = await installSettingsLifecycle()
    const second = await installSettingsLifecycle()
    expect(second).toBe(first)
    expect(handlers).toHaveLength(1)

    // 卸载后再次装配会重新注册(幂等只针对同一份装配)。
    disposeSettingsLifecycle()
    await installSettingsLifecycle()
    expect(handlers).toHaveLength(2)
    expect(unlistenSpy).toHaveBeenCalledTimes(1)
  })

  it('回执自身失败不抛出(后端有超时兜底)', async () => {
    invokeIpc.mockRejectedValueOnce(new Error('ipc down'))
    await installSettingsLifecycle()
    handlers[0]({ payload: { requestId: 'flush-6' } })
    await expect(flushMicrotasks()).resolves.toBeUndefined()
  })
})
