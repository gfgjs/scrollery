import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { EVENTS, IPC } from '../constants/ipc'
import type { EnhanceJob } from '../types/enhance'
import { useEnhanceStore } from './enhanceStore'

const { invokeIpc, listenAppEvent, addToast, fetchQueue } = vi.hoisted(() => ({
  invokeIpc: vi.fn(),
  listenAppEvent: vi.fn<(event: string, callback: () => void) => Promise<() => void>>(),
  addToast: vi.fn(),
  fetchQueue: vi.fn<() => Promise<EnhanceJob[]>>(),
}))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../utils/appEvents', () => ({ listenAppEvent }))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn() } }))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('./toastStore', () => ({ useToastStore: () => ({ addToast }) }))

function job(status: EnhanceJob['status']): EnhanceJob {
  return {
    id: 7, status, total: 2, done: status === 'done' ? 2 : 0,
    tileDone: status === 'done' ? 20 : 4, tilesTotal: 20, errorCode: null,
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.resetAllMocks()
  listenAppEvent.mockResolvedValue(() => {})
  invokeIpc.mockImplementation((command: string) => {
    if (command === IPC.GET_ENHANCE_QUEUE) return fetchQueue()
    if (command === IPC.ENHANCE_CANCEL) return Promise.resolve()
    throw new Error(`Unexpected IPC: ${command}`)
  })
})

describe('enhanceStore 队列快照顺序', () => {
  it('取消已确认后，先前查询的运行中快照不得恢复活跃状态和取消入口', async () => {
    const store = useEnhanceStore()
    fetchQueue.mockResolvedValueOnce([job('running')])
    await store.subscribeQueue()
    expect(store.hasActiveJob).toBe(true)

    let finishOld!: (value: EnhanceJob[]) => void
    fetchQueue.mockReturnValueOnce(new Promise((resolve) => { finishOld = resolve }))
    const pendingRefresh = store.fetchQueue()
    fetchQueue.mockResolvedValueOnce([job('cancelled')])
    await store.cancel(7)
    expect(invokeIpc).toHaveBeenCalledWith(IPC.ENHANCE_CANCEL, { jobId: 7 })
    expect(store.queue).toEqual([job('cancelled')])

    finishOld([job('running')])
    await pendingRefresh
    expect(store.queue).toEqual([job('cancelled')])
    expect(store.hasActiveJob).toBe(false)
    expect(addToast).not.toHaveBeenCalled()
    store.unsubscribeQueue()
  })

  it('完成事件的新快照优先于迟到的进度快照，后续刷新不重复通知完成', async () => {
    const store = useEnhanceStore()
    fetchQueue.mockResolvedValueOnce([job('running')])
    await store.subscribeQueue()
    expect(listenAppEvent).toHaveBeenCalledWith(EVENTS.ENHANCE_QUEUE_CHANGED, expect.any(Function))
    const queueChanged = listenAppEvent.mock.calls[0]![1]

    let finishOld!: (value: EnhanceJob[]) => void
    fetchQueue.mockReturnValueOnce(new Promise((resolve) => { finishOld = resolve }))
    const pendingRefresh = store.fetchQueue()
    fetchQueue.mockResolvedValue([job('done')])
    queueChanged()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(store.queue).toEqual([job('done')])
    expect(addToast).toHaveBeenCalledExactlyOnceWith('success', 'enhance.toastJobDone')

    finishOld([job('running')])
    await pendingRefresh
    expect(store.queue).toEqual([job('done')])
    expect(store.hasActiveJob).toBe(false)
    await store.fetchQueue()
    expect(addToast).toHaveBeenCalledTimes(1)
    store.unsubscribeQueue()
  })
})
