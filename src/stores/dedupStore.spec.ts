import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useDedupStore } from './dedupStore'
import { EVENTS, IPC } from '../constants/ipc'
import type { DedupStatusSnapshot } from '../types/ipc'
import { resolveDuplicateLensStatusView } from '../components/media/duplicateLensStatus'

const invokeIpc = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>()
const listeners = new Map<string, (event: { payload: DedupStatusSnapshot }) => void>()
vi.mock('../utils/ipc', () => ({
  invokeIpc: (command: string, args?: Record<string, unknown>) => invokeIpc(command, args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, callback: (event: { payload: DedupStatusSnapshot }) => void) => {
    listeners.set(name, callback)
    return () => listeners.delete(name)
  },
}))

function snapshot(status: DedupStatusSnapshot['status'], runId = '1'): DedupStatusSnapshot {
  return {
    runId, status, phase: 'quick', itemsDone: 0, itemsTotal: 2,
    bytesDone: 0, bytesTotal: 20, groupsFound: 0, potentialLogicalBytes: 0,
    errors: [], waitingOn: [],
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => { resolve = res })
  return { promise, resolve }
}

function emitProgress(payload: DedupStatusSnapshot) {
  const callback = listeners.get(EVENTS.DEDUP_PROGRESS)
  expect(callback).toBeDefined()
  callback!({ payload })
}

function primaryAction(store: ReturnType<typeof useDedupStore>) {
  return resolveDuplicateLensStatusView({
    lensMode: 'groups', runStatus: store.status.status, hasLayout: true,
    separatorCount: 1, positionCount: 2, percent: 100,
    completedAt: store.completedAt, safeErrorCode: null,
  }).primaryAction
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  listeners.clear()
})

describe('去重启停响应与进度事件的先后归属', () => {
  it('完成事件先到时，迟到的启动响应不能恢复运行态和停止按钮', async () => {
    const response = deferred<DedupStatusSnapshot>()
    const requested = deferred<void>()
    invokeIpc.mockImplementation(() => {
      requested.resolve(undefined)
      return response.promise
    })
    const store = useDedupStore()
    const starting = store.start(true)
    await requested.promise
    expect(invokeIpc).toHaveBeenCalledWith(IPC.START_DEDUP_ANALYSIS, { reset: true })

    emitProgress(snapshot('running'))
    const completed = { ...snapshot('completed'), itemsDone: 2, groupsFound: 1 }
    emitProgress(completed)
    const completedAt = store.completedAt
    expect(completedAt).not.toBeNull()
    response.resolve(snapshot('running'))
    await starting

    expect(store.status).toEqual(completed)
    expect(store.isRunning).toBe(false)
    expect(store.completedAt).toBe(completedAt)
    expect(primaryAction(store)).toBe('restart')
  })

  it('收到停止事件后重新开始，旧停止响应不能盖掉新轮次及其停止入口', async () => {
    const store = useDedupStore()
    invokeIpc.mockResolvedValueOnce(snapshot('running'))
    await store.start()

    const oldStop = deferred<DedupStatusSnapshot>()
    invokeIpc.mockReturnValueOnce(oldStop.promise)
    const stopping = store.stop()
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.STOP_DEDUP_ANALYSIS, undefined)
    emitProgress(snapshot('cancelled'))
    expect(primaryAction(store)).toBe('restart')

    // 状态条收到 cancelled 后已允许重新分析，旧 IPC 响应此时仍可在途。
    const nextRun = snapshot('running', '2')
    invokeIpc.mockImplementationOnce(async () => {
      emitProgress(nextRun)
      return nextRun
    })
    await store.start(true)
    oldStop.resolve(snapshot('cancelled'))
    await stopping

    expect(store.status).toEqual(nextRun)
    expect(store.isRunning).toBe(true)
    expect(primaryAction(store)).toBe('stop')
  })

  it('请求期间没有进度事件时，启停响应仍正常更新状态', async () => {
    const store = useDedupStore()
    invokeIpc.mockResolvedValueOnce(snapshot('running'))
    await store.start()
    expect(store.isRunning).toBe(true)
    expect(primaryAction(store)).toBe('stop')

    invokeIpc.mockResolvedValueOnce(snapshot('cancelled'))
    await store.stop()
    expect(store.status.status).toBe('cancelled')
    expect(store.isRunning).toBe(false)
    expect(primaryAction(store)).toBe('restart')
  })
})
