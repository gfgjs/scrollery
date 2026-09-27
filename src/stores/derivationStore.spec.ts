import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { reactive } from 'vue'
import { IPC } from '../constants/ipc'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), toast: vi.fn() }))
const config = reactive({ enableVideoKeyframes: true })
vi.mock('./configStore', () => ({ useConfigStore: () => config }))
vi.mock('./toastStore', () => ({ useToastStore: () => ({ addToast: mocks.toast }) }))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => mocks.invoke(...args),
  generateOperationId: () => 'operation',
  ipcErrorMessage: () => 'failed',
}))

import { useDerivationStore } from './derivationStore'

type Status = ReturnType<typeof useDerivationStore>['videoStatus']
const status = (isRunning: boolean, done = 0): Status => ({
  pending: isRunning ? 1 : 0,
  processing: 0,
  done,
  error: 0,
  active: isRunning,
  isRunning,
})
const queries: Array<{ resolve: (s: Status) => void; reject: (e: Error) => void }> = []
let store: ReturnType<typeof useDerivationStore>

beforeEach(() => {
  vi.useFakeTimers()
  mocks.invoke.mockReset()
  mocks.toast.mockReset()
  config.enableVideoKeyframes = true
  queries.length = 0
  mocks.invoke.mockImplementation((cmd: string) => {
    if (cmd !== IPC.DERIVATION_STATUS) return Promise.resolve()
    return new Promise<Status>((resolve, reject) => queries.push({ resolve, reject }))
  })
  setActivePinia(createPinia())
  store = useDerivationStore()
})

afterEach(() => {
  store.$dispose()
  vi.clearAllTimers()
  vi.useRealTimers()
})

describe('派生状态查询生命周期', () => {

  it('停止并重启后，旧轮询不能覆盖新状态或清掉新查询的在途标记', async () => {
    const initial = store.fetchVideoStatus()
    queries[0].resolve(status(true))
    await initial
    await vi.advanceTimersByTimeAsync(1000)
    const stopping = store.stopVideoExtraction()
    await vi.advanceTimersByTimeAsync(0)
    queries[2].resolve(status(false, 1))
    await stopping
    const starting = store.startVideoIncremental()
    await vi.advanceTimersByTimeAsync(0)
    queries[3].resolve(status(true, 2))
    await starting
    await vi.advanceTimersByTimeAsync(1000)
    queries[1].resolve(status(true, 999))
    await vi.advanceTimersByTimeAsync(2000)
    expect(store.videoFinished).toBe(2)
    expect(queries).toHaveLength(5)
    queries[4].resolve(status(false, 3))
    await vi.advanceTimersByTimeAsync(0)
    expect(store.videoFinished).toBe(3)
    expect(vi.getTimerCount()).toBe(0)
  })
})
