import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { IPC } from '../constants/ipc'
import type { DedupStatusSnapshot } from '../types/ipc'

const invokeIpc = vi.hoisted(() => vi.fn())
// DEDUP_PROGRESS 监听句柄:completedAt 的边沿记录依赖事件回调,测试据此注入快照。
const listenMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({
  listen: listenMock,
}))
vi.mock('../utils/ipc', () => ({ invokeIpc }))

import { useDedupStore } from './dedupStore'

type ProgressHandler = (event: { payload: DedupStatusSnapshot }) => void
let progressHandler: ProgressHandler | null = null

function snapshot(overrides: Partial<DedupStatusSnapshot> = {}): DedupStatusSnapshot {
  return {
    runId: 'run-1',
    status: 'idle',
    phase: 'idle',
    itemsDone: 0,
    itemsTotal: 0,
    bytesDone: 0,
    bytesTotal: 0,
    groupsFound: 0,
    potentialLogicalBytes: 0,
    errors: [],
    waitingOn: [],
    ...overrides,
  }
}

function emitProgress(overrides: Partial<DedupStatusSnapshot>): void {
  if (!progressHandler) throw new Error('DEDUP_PROGRESS listener not registered')
  progressHandler({ payload: snapshot(overrides) })
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  listenMock.mockReset()
  listenMock.mockImplementation(async (_event: string, handler: ProgressHandler) => {
    progressHandler = handler
    return () => {}
  })
  progressHandler = null
})

describe('dedupStore completedAt 记录(主画廊重复镜头状态条)', () => {
  it('DEDUP_PROGRESS 进入 completed 边沿记录本地完成时刻,离开后再次完成刷新', async () => {
    invokeIpc.mockResolvedValue(snapshot())
    const store = useDedupStore()
    await store.restoreStatusOnce()
    expect(store.completedAt).toBeNull()

    emitProgress({ status: 'running', itemsDone: 5, itemsTotal: 10 })
    expect(store.completedAt).toBeNull()

    vi.spyOn(Date, 'now').mockReturnValueOnce(1111)
    emitProgress({ status: 'completed', itemsDone: 10, itemsTotal: 10 })
    expect(store.completedAt).toBe(1111)

    // running(旧结果重跑)→ 再次 completed:新完成时刻覆盖旧值。
    emitProgress({ status: 'running' })
    expect(store.completedAt).toBe(1111)
    vi.spyOn(Date, 'now').mockReturnValueOnce(2222)
    emitProgress({ status: 'completed' })
    expect(store.completedAt).toBe(2222)

    vi.restoreAllMocks()
  })

  it('首次 restoreStatus 清空 completedAt(重启丢失,MVP 接受),同会话再次恢复保留', async () => {
    invokeIpc.mockResolvedValue(snapshot({ status: 'completed' }))
    const store = useDedupStore()
    // 模拟本会话内分析已完成:事件流先记录完成时刻。
    await store.restoreStatusOnce()
    emitProgress({ status: 'running' })
    emitProgress({ status: 'completed' })
    const recorded = store.completedAt
    expect(recorded).not.toBeNull()

    // 再次恢复(如重复镜头状态条重复挂载)不得把已完成时刻清掉。
    await store.restoreStatusOnce()
    expect(store.completedAt).toBe(recorded)

    // 首次恢复路径:新会话(store 尚未 restore 过)恢复即清空。
    setActivePinia(createPinia())
    const fresh = useDedupStore()
    await fresh.restoreStatus()
    expect(fresh.completedAt).toBeNull()
  })

  it('start 清空旧完成时刻(新一轮开始即作废「最近完成于」)', async () => {
    invokeIpc.mockImplementation((command: string) => {
      if (command === IPC.DEDUP_STATUS) return Promise.resolve(snapshot())
      return Promise.resolve(snapshot({ status: 'running', itemsDone: 1, itemsTotal: 9 }))
    })
    const store = useDedupStore()
    await store.restoreStatusOnce()
    emitProgress({ status: 'completed' })
    expect(store.completedAt).not.toBeNull()

    await store.start(true)
    expect(store.completedAt).toBeNull()
    expect(store.status.status).toBe('running')
  })

  it('restoreStatus 请求在途收到进度事件时不以旧快照回退状态', async () => {
    let resolveStatus!: (value: DedupStatusSnapshot) => void
    invokeIpc.mockReturnValue(
      new Promise<DedupStatusSnapshot>((resolve) => {
        resolveStatus = resolve
      }),
    )
    const store = useDedupStore()
    const restoring = store.restoreStatus()
    await vi.waitFor(() => expect(invokeIpc).toHaveBeenCalledWith(IPC.DEDUP_STATUS))

    emitProgress({ status: 'completed', itemsDone: 10, itemsTotal: 10 })
    resolveStatus(snapshot({ status: 'running', itemsDone: 2, itemsTotal: 10 }))
    await restoring

    expect(store.status.status).toBe('completed')
    expect(store.status.itemsDone).toBe(10)
  })
})
