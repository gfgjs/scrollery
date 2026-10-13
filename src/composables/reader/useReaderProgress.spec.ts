import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { useReaderProgress } from './useReaderProgress'

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args: { itemId: number; position: string }) => Promise<void>>(),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc: invoke }))

beforeEach(() => {
  vi.useFakeTimers()
  invoke.mockReset()
})
afterEach(() => vi.useRealTimers())

it('退出后重开同一本书，旧页面的迟到写入不能覆盖新阅读位置', async () => {
  let releaseOld!: () => void
  const oldWrite = new Promise<void>(resolve => { releaseOld = resolve })
  let diskPosition = 'page:1'
  invoke.mockImplementation(async (_command, { position }) => {
    // 模拟旧保存尚未取得数据库写锁，新页面已经能发起保存。
    if (position === 'page:10') await oldWrite
    diskPosition = position
  })
  const oldPage = useReaderProgress(ref(41))
  const reopenedPage = useReaderProgress(ref(41))
  try {
    oldPage.onProgress('page:10')
    oldPage.flushProgress()
    reopenedPage.onProgress('page:20')
    reopenedPage.flushProgress()
    await vi.advanceTimersByTimeAsync(0)

    releaseOld()
    await vi.advanceTimersByTimeAsync(0)
    expect(diskPosition).toBe('page:20')
    expect(invoke).toHaveBeenCalledTimes(2)
    expect(invoke).toHaveBeenLastCalledWith(IPC.SET_READING_PROGRESS, { itemId: 41, position: 'page:20' })
    expect(vi.getTimerCount()).toBe(0)
  } finally {
    releaseOld()
    await vi.advanceTimersByTimeAsync(0)
  }
})

it('另一文档的保存不等待旧文档写入，防抖位置仍绑定收到事件时的文档', async () => {
  let releaseOld!: () => void
  const oldWrite = new Promise<void>(resolve => { releaseOld = resolve })
  const disk = new Map<number, string>()
  invoke.mockImplementation(async (_command, { itemId, position }) => {
    if (itemId === 41) await oldWrite
    disk.set(itemId, position)
  })
  const id = ref(41)
  const progress = useReaderProgress(id)
  try {
    progress.onProgress('page:10')
    id.value = 42
    // 与 load() 一致：路由已变时，先冲刷旧文档位置，再接收新文档事件。
    progress.flushProgress()
    progress.reset()
    progress.onProgress('page:2')
    await vi.advanceTimersByTimeAsync(1200)
    expect(disk.get(42)).toBe('page:2')
    expect(disk.has(41)).toBe(false)
    expect(invoke).toHaveBeenCalledTimes(2)

    releaseOld()
    await vi.advanceTimersByTimeAsync(0)
    expect(disk.get(41)).toBe('page:10')
    expect(disk.get(42)).toBe('page:2')
  } finally {
    releaseOld()
    await vi.advanceTimersByTimeAsync(0)
  }
})
