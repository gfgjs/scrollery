// latestWrite 单测(审查 F-09):锁三件事——
//  1. 串行:每 key 至多一个在途写,后端永远看不到并发(乱序落库的根源被结构性消灭)。
//  2. latest-write-wins:在途期间连点,中间值跳过,终值 = 最后一次 push。
//  3. 失败:onError 拿到 key+最新目标,链终止不重试;下一次 push 重开新链。
import { describe, it, expect, vi } from 'vitest'
import { createLatestWriteQueue } from './latestWrite'

/** 手动控制 resolve/reject 的 write 桩:每次调用记录参数并挂起,由测试逐个放行。 */
function makeHarness() {
  const calls: Array<{ key: number; value: number }> = []
  const resolvers: Array<{ resolve: () => void; reject: (e: unknown) => void }> = []
  const write = vi.fn((key: number, value: number) => {
    calls.push({ key, value })
    return new Promise<void>((resolve, reject) => {
      resolvers.push({ resolve, reject })
    })
  })
  const errors: Array<{ key: number; value: number; err: unknown }> = []
  const queue = createLatestWriteQueue<number, number>(write, (key, value, err) =>
    errors.push({ key, value, err }),
  )
  /** 放行第 n 个在途写并让微任务队列排空。 */
  const settle = async (n: number, mode: 'resolve' | 'reject' = 'resolve') => {
    if (mode === 'resolve') resolvers[n].resolve()
    else resolvers[n].reject(new Error('write failed'))
    await Promise.resolve()
    await Promise.resolve()
  }
  return { queue, calls, errors, settle }
}

describe('createLatestWriteQueue', () => {
  it('单次 push:写一次,值正确,完成后不再在途', async () => {
    const { queue, calls, settle } = makeHarness()
    queue.push(1, 90)
    expect(calls).toEqual([{ key: 1, value: 90 }])
    expect(queue.isPending(1)).toBe(true)
    await settle(0)
    expect(queue.isPending(1)).toBe(false)
    expect(calls.length).toBe(1)
  })

  it('在途期间连点:中间值跳过,补写终值(latest-write-wins)', async () => {
    const { queue, calls, settle } = makeHarness()
    queue.push(1, 90)
    queue.push(1, 180) // 在途:只记目标
    queue.push(1, 270) // 覆盖目标
    expect(calls.length).toBe(1) // 串行:第二笔必须等第一笔完成
    await settle(0)
    expect(calls).toEqual([
      { key: 1, value: 90 },
      { key: 1, value: 270 }, // 180 被跳过
    ])
    await settle(1)
    expect(queue.isPending(1)).toBe(false)
    expect(calls.length).toBe(2)
  })

  it('不同 key 互不阻塞', async () => {
    const { queue, calls, settle } = makeHarness()
    queue.push(1, 90)
    queue.push(2, 180)
    expect(calls).toEqual([
      { key: 1, value: 90 },
      { key: 2, value: 180 },
    ])
    await settle(0)
    await settle(1)
    expect(queue.isPending(1)).toBe(false)
    expect(queue.isPending(2)).toBe(false)
  })

  it('写失败:onError 拿到最新目标,在途累积值不再补写,链终止', async () => {
    const { queue, calls, errors, settle } = makeHarness()
    queue.push(1, 90)
    queue.push(1, 180) // 在途累积
    await settle(0, 'reject')
    expect(errors).toEqual([{ key: 1, value: 180, err: new Error('write failed') }])
    expect(calls.length).toBe(1) // 不重试、不补写
    expect(queue.isPending(1)).toBe(false)
  })

  it('失败后再 push 重开新链', async () => {
    const { queue, calls, errors, settle } = makeHarness()
    queue.push(1, 90)
    await settle(0, 'reject')
    expect(errors.length).toBe(1)
    queue.push(1, 270)
    expect(calls[1]).toEqual({ key: 1, value: 270 })
    await settle(1)
    expect(queue.isPending(1)).toBe(false)
  })
})
