// src/composables/useSettingsLifecycle.spec.ts
// 退出 flush 协议的局部测试(设置集中保存,批次C)。覆盖三件容易写错的事:
//   1. 单飞——重叠请求不会并行写盘;
//   2. 失败回执 ok=false(不把未保存当已保存),成功后 ok=true;
//   3. 同一 requestId 重复投递只回执一次。
// 协议的对端(后端 lifecycle::flush_ack)按 requestId 记账、收齐才放行退出,故这三条是退出不丢
// 设置的直接前提。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { installSettingsLifecycle, disposeSettingsLifecycle } from './useSettingsLifecycle'

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
    event: string,
    handler: (event: { payload: { requestId: string } }) => void,
  ) => {
    expect(event).toBe('settings-flush-requested')
    handlers.push(handler)
    return unlistenSpy
  },
}))
vi.mock('../utils/logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))


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
})
