import { expect, it, vi } from 'vitest'

const invokeIpc = vi.hoisted(() => vi.fn())
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../utils/logger', () => ({ logger: { warn: vi.fn() } }))
import { useViewIds } from './useViewIds'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

it('切换顺序立即撤销旧全集，迟到拒绝不清新数据，当前失败可重试', async () => {
  const ids = useViewIds()
  const old = deferred<number[]>()
  const intermediate = deferred<number[]>()
  const latest = deferred<number[]>()
  invokeIpc.mockReset().mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(intermediate.promise).mockReturnValueOnce(latest.promise)
  ids.setExpectedVersion(201)
  const first = ids.ensureFresh(201)
  ids.setExpectedVersion(null)
  expect(ids.isReady()).toBe(false)
  expect(ids.rangeBetween(8, 9)).toEqual([])
  ids.setExpectedVersion(202)
  const second = ids.ensureFresh(202)
  ids.setExpectedVersion(203)
  const third = ids.ensureFresh(203)
  latest.resolve([9, 8, 3])
  await third
  old.resolve([1, 2])
  intermediate.reject(new Error('ViewStale'))
  await Promise.all([first, second])
  expect(ids.allIds()).toEqual([9, 8, 3])

  ids.setExpectedVersion(204)
  invokeIpc.mockRejectedValueOnce(new Error('LayoutNotReady'))
  await ids.ensureFresh(204)
  expect(ids.totalCount()).toBe(0)
  invokeIpc.mockResolvedValueOnce([3, 8])
  await ids.ensureFresh(204)
  expect(ids.isFresh(204)).toBe(true)
  expect(ids.allIds()).toEqual([3, 8])
})
