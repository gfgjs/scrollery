import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { watch } from 'vue'
import type { MediaMeta } from '../types/layout'
import { IPC } from '../constants/ipc'

const invokeIpc = vi.hoisted(() => vi.fn())
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../utils/logger', () => ({ logger: { warn: vi.fn(), error: vi.fn() } }))
import { useMediaStore } from './mediaStore'

function meta(id: number, fileName = String(id)): MediaMeta { return { id, fileName } as MediaMeta }
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}
beforeEach(() => {
  setActivePinia(createPinia())
  vi.useFakeTimers()
  invokeIpc.mockReset()
  vi.stubGlobal('window', { devicePixelRatio: 1 })
})
afterEach(() => {
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

it('密集窗口串行分批，同 ID 在途不重发，连续浏览后缓存仍有界', async () => {
  const store = useMediaStore()
  const first = deferred<MediaMeta[]>()
  const calls: number[][] = []
  invokeIpc.mockImplementation((_cmd: string, args: { ids: number[] }) => {
    calls.push(args.ids)
    return calls.length === 1 ? first.promise : Promise.resolve(args.ids.map((id) => meta(id)))
  })
  let peak = 0
  const stop = watch(() => store.viewportMeta, (cache) => { peak = Math.max(peak, cache.size) }, { flush: 'sync' })
  const initial = Array.from({ length: 600 }, (_, i) => i)
  store.ensureMeta(initial)
  await vi.advanceTimersByTimeAsync(120)
  expect(calls[0].length).toBeLessThanOrEqual(256)
  store.ensureMeta(initial)
  await vi.advanceTimersByTimeAsync(120)
  expect(calls).toHaveLength(1)
  first.resolve(calls[0].map((id) => meta(id)))
  await vi.advanceTimersByTimeAsync(0)
  expect(store.viewportMeta.size).toBe(600)
  // 单次持续浏览轨迹，观察每次 Map 发布的峰值；不是多个独立用例的参数循环。
  for (let page = 1; page <= 40; page++) {
    store.ensureMeta(initial.map((id) => id + page * 600))
    await vi.advanceTimersByTimeAsync(120)
  }
  expect(peak).toBeLessThanOrEqual(600 + 256)
  expect(calls.every((ids) => ids.length <= 256)).toBe(true)
  expect(store.viewportMeta.has(24_000)).toBe(true)
  expect(store.viewportMeta.has(0)).toBe(false)
  const fetched = calls.flat()
  expect(new Set(fetched).size).toBe(fetched.length)
  const back = initial.map((id) => id + 39 * 600)
  store.ensureMeta(back)
  await vi.advanceTimersByTimeAsync(120)
  expect(back.every((id) => store.viewportMeta.has(id))).toBe(true)
  expect(peak).toBeLessThanOrEqual(856)
  stop()
})

it('切作用域与关闭信息浮层撤销旧响应，当前窗口随后正常取数', async () => {
  const store = useMediaStore()
  const old = deferred<MediaMeta[]>()
  const next = deferred<MediaMeta[]>()
  let metaCalls = 0
  invokeIpc.mockImplementation((cmd: string) => {
    if (cmd === IPC.GET_META_FOR_VIEWPORT) return ++metaCalls === 1 ? old.promise : next.promise
    return Promise.resolve({ totalRows: 1, totalHeight: 100, layoutVersion: 1,
      orderVersion: 1, totalItems: 1, separators: [], monthBuckets: [] })
  })
  store.ensureMeta([1])
  await vi.advanceTimersByTimeAsync(120)
  await store.computeLayout({ directoryId: 2, containerWidth: 800 })
  store.ensureMeta([1])
  await vi.advanceTimersByTimeAsync(120)
  expect(metaCalls).toBe(1)
  old.resolve([meta(1, 'old')])
  await vi.advanceTimersByTimeAsync(0)
  expect(store.viewportMeta.has(1)).toBe(false)
  expect(metaCalls).toBe(2)
  store.ensureMeta([])
  next.resolve([meta(1, 'closed')])
  await vi.advanceTimersByTimeAsync(0)
  expect(store.viewportMeta.size).toBe(0)
  invokeIpc.mockResolvedValue([meta(2, 'current')])
  store.ensureMeta([2])
  await vi.advanceTimersByTimeAsync(120)
  expect(store.viewportMeta.get(2)?.fileName).toBe('current')
})

it('请求失败后可由有效窗口重试，销毁时在途重试不能回填', async () => {
  const store = useMediaStore()
  invokeIpc.mockRejectedValueOnce(new Error('query failed'))
  store.ensureMeta([7])
  await vi.advanceTimersByTimeAsync(120)
  expect(invokeIpc).toHaveBeenCalledTimes(1)
  expect(store.viewportMeta.size).toBe(0)
  const retry = deferred<MediaMeta[]>()
  invokeIpc.mockReturnValueOnce(retry.promise)
  store.ensureMeta([7])
  await vi.advanceTimersByTimeAsync(120)
  expect(invokeIpc).toHaveBeenCalledTimes(2)
  store.$dispose()
  retry.resolve([meta(7)])
  await vi.advanceTimersByTimeAsync(0)
  expect(store.viewportMeta.size).toBe(0)
})
