// 覆盖 Canvas 连续滚动下的批容量与串行门、取消后释放名额及旧结果隔离、
// invoke 失败的有界重试、pending 结果重排，以及停滞退避期的共享取消。
// 通过业务流程断言队列/在途计数，避免仅靠测试标题声明覆盖。
// node 环境,fake timers;invoke 返回测试持有的 deferred(成功只确认受理);
// 结果经捕获的 Channel stub 手动 onmessage 注入。魔数出处:flush 防抖 50ms、STALL_MS=30000、
// THUMB_RETRY_MAX=3、THUMB_RETRY_BASE_MS=500——均未导出,此处按字面量对拍。
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { ThumbResult } from '../types/media'
import type { LayoutRowNormal } from '../types/layout'
import { useCanvasThumbPipeline } from './useCanvasThumbPipeline'

type InvokeCall = { cmd: string; args: { itemIds: number[]; targetSize: number; requestId: string; onResult: ChannelStub } }
type ChannelStub = { onmessage: (msg: ThumbResult & { pending: boolean }) => void }

const mockState = vi.hoisted(() => {
  const state = {
    calls: [] as Array<{ cmd: string; args: unknown }>,
    deferreds: [] as Array<{ resolve: (v: unknown) => void; reject: (e: unknown) => void }>,
    reset() {
      state.calls.length = 0
      state.deferreds.length = 0
    },
  }
  return state
})

vi.mock('@tauri-apps/api/core', () => {
  // Channel 最小 stub:生产代码只 new + 赋 onmessage + 作为 args 传递。
  class Channel {
    onmessage: (msg: unknown) => void = () => {}
  }
  return {
    Channel,
    // 普通函数而非 vi.fn:tinyspy 会把已被下游 catch 的 mock 拒绝误报为 unhandled(见
    // usePluginEntitlement.spec.ts 的同款记载)。
    invoke: (cmd: string, args: unknown) => {
      mockState.calls.push({ cmd, args })
      return new Promise((resolve, reject) => {
        mockState.deferreds.push({ resolve, reject })
      })
    },
  }
})

const scanMock = vi.hoisted(() => ({ autoThumbQueueSize: 0, autoThumbInFlight: 0 }))
vi.mock('../stores/scanStore', () => ({ useScanStore: () => scanMock }))

const uiMock = vi.hoisted(() => ({ gridRowHeight: 200 }))
vi.mock('../stores/uiStore', () => ({ useUiStore: () => uiMock }))

import { useRequestQueue } from './useRequestQueue'

function call(i: number): InvokeCall {
  return mockState.calls[i] as unknown as InvokeCall
}
/** 预挂 catch 防 unhandled rejection,返回可等待的拒因。 */
function catchErr(p: Promise<unknown>): Promise<Error> {
  return p.then(
    () => {
      throw new Error('expected rejection')
    },
    (e: Error) => e,
  )
}
function result(itemId: number): ThumbResult & { pending: boolean } {
  return { itemId, thumbStatus: 2, thumbPath: `p/${itemId}.webp`, thumbhash: null, pending: false }
}

beforeEach(() => {
  vi.useFakeTimers()
  mockState.reset()
  scanMock.autoThumbQueueSize = 0
  scanMock.autoThumbInFlight = 0
  uiMock.gridRowHeight = 200
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
  vi.spyOn(console, 'debug').mockImplementation(() => {})
})

afterEach(() => {
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('批容量与串行门', () => {
  it('Canvas 连续滚过 4000 项：旧排队撤销，当前后端批次完成后直接服务最终视口', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 })
    const q = useRequestQueue()
    const pipeline = useCanvasThumbPipeline({
      cacheDir: () => '/cache',
      onRequestThumb: (id) => q.request(id).then(() => {}),
      onCancelThumb: q.cancel,
      onRegenerateThumb: () => {},
      scheduleDraw: () => {},
      viewportW: () => 1000,
      viewportH: () => 100,
    })
    const rows: LayoutRowNormal[] = Array.from({ length: 40 }, (_, row) => ({
      rowType: 'normal',
      y: row * 100,
      height: 100,
      items: Array.from({ length: 100 }, (_, col) => ({
        id: row * 100 + col,
        x: col * 10,
        w: 10,
        h: 100,
        fileSize: 0,
        fileFormat: 'jpg',
        mediaType: 'image',
        isLivePhoto: false,
        durationMs: null,
        thumbStatus: 0,
        thumbPath: null,
        placeholderColor: null,
        isFavorited: false,
        rating: 0,
        colorLabel: 0,
        availability: 'online',
        originalWidth: 10,
        originalHeight: 100,
        sortDatetime: 0,
      })),
    }))
    pipeline.prioritizeVisibleThumbLoads(rows, 0, 1)
    for (const item of rows[0].items) pipeline.getImage(item)
    await vi.advanceTimersByTimeAsync(50)
    expect(scanMock.autoThumbInFlight).toBe(24)
    let peakQueued = scanMock.autoThumbQueueSize
    for (let row = 1; row < rows.length; row++) {
      pipeline.prioritizeVisibleThumbLoads(rows, row, row + 1)
      for (const item of rows[row].items) pipeline.getImage(item)
      peakQueued = Math.max(peakQueued, scanMock.autoThumbQueueSize)
    }
    expect(peakQueued).toBe(48)
    expect(mockState.calls).toHaveLength(1)
    for (const id of call(0).args.itemIds) call(0).args.onResult.onmessage(result(id))
    await vi.advanceTimersByTimeAsync(50)
    expect(call(1).args.itemIds).toEqual(Array.from({ length: 24 }, (_, i) => 3900 + i))
    pipeline.dispose()
    expect(scanMock.autoThumbQueueSize).toBe(0)
    for (const id of call(1).args.itemIds) call(1).args.onResult.onmessage(result(id))
    await vi.advanceTimersByTimeAsync(1000)
    expect(scanMock.autoThumbInFlight).toBe(0)
    expect(mockState.calls).toHaveLength(2)
  })
})

describe('cancel 语义分叉', () => {

  it('inFlight:撤订阅立即释放名额;新请求独立入批且旧结果不能结算它', async () => {
    const q = useRequestQueue()
    const err = catchErr(q.request(9))
    const err2 = catchErr(q.request(10))
    await vi.advanceTimersByTimeAsync(50)
    q.cancel(9)
    q.cancel(10)
    expect((await err).message).toBe('cancelled')
    expect((await err2).message).toBe('cancelled')
    expect(scanMock.autoThumbInFlight).toBe(0)
    mockState.deferreds[0].resolve(undefined)
    await vi.advanceTimersByTimeAsync(0)
    expect(mockState.calls[1]).toMatchObject({
      cmd: 'cancel_viewport_thumbnail_request',
      args: { requestId: call(0).args.requestId, itemIds: [9, 10] },
    })
    const p2 = q.request(9)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(3)
    call(0).args.onResult.onmessage(result(9))
    expect(scanMock.autoThumbInFlight).toBe(1)
    call(2).args.onResult.onmessage(result(9))
    await expect(p2).resolves.toMatchObject({ itemId: 9 })
    expect(scanMock.autoThumbInFlight).toBe(0)
  })
})

describe('结算兜底与看门狗', () => {

  it('invoke 整批失败:3 轮退避重试耗尽后终拒(消息不变),无未捕获异常', async () => {
    const q = useRequestQueue()
    const err = catchErr(q.request(1))
    let joined: Promise<Error> | undefined
    await vi.advanceTimersByTimeAsync(50)
    mockState.deferreds[0].reject(new Error('backend down'))
    await vi.advanceTimersByTimeAsync(0)
    // 退避 500/1000/2000ms(THUMB_RETRY_MAX=3,未导出,按字面量对拍)+ 每轮 50ms 合批
    for (let n = 0; n < 3; n++) {
      if (n === 2) joined = catchErr(q.request(1)) // 后加入者不能重置逻辑请求预算
      await vi.advanceTimersByTimeAsync(500 * 2 ** n)
      await vi.advanceTimersByTimeAsync(50)
      mockState.deferreds[n + 1].reject(new Error('backend down'))
      await vi.advanceTimersByTimeAsync(0)
    }
    expect((await err).message).toBe('Batch invoke failed')
    expect((await joined)?.message).toBe('Batch invoke failed')
    expect(mockState.calls.length).toBe(4)
    expect(scanMock.autoThumbInFlight).toBe(0)
  })
})

describe('有界退避重试(2026-08-16 阶段3)', () => {
  it('pending 不结算占位结果，释放名额并重试；旧批迟到结果不能结算新批', async () => {
    const q = useRequestQueue()
    const pending = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage({
      itemId: 1,
      thumbStatus: 0,
      thumbPath: null,
      thumbhash: null,
      pending: true,
    })
    expect(scanMock.autoThumbInFlight).toBe(0)
    await vi.advanceTimersByTimeAsync(550)
    expect(call(1).args.itemIds).toEqual([1])
    call(0).args.onResult.onmessage(result(1))
    expect(scanMock.autoThumbInFlight).toBe(1)
    call(1).args.onResult.onmessage(result(1))
    await expect(pending).resolves.toMatchObject({ itemId: 1, thumbStatus: 2 })
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

  it('cancel 掐断共享退避:全部等待者拒绝且迟到旧结果不再触发重试', async () => {
    const q = useRequestQueue()
    const first = vi.fn()
    const second = vi.fn()
    void catchErr(q.request(1)).then(first)
    void catchErr(q.request(1)).then(second)
    await vi.advanceTimersByTimeAsync(50)
    await vi.advanceTimersByTimeAsync(30_000) // 停滞 → 500ms 退避起表
    const joined = vi.fn()
    void catchErr(q.request(1)).then(joined) // 退避期新等待者共享同一取消
    q.cancel(1)
    await vi.advanceTimersByTimeAsync(0)
    expect(first).toHaveBeenCalledWith(expect.objectContaining({ message: 'cancelled' }))
    expect(second).toHaveBeenCalledWith(expect.objectContaining({ message: 'cancelled' }))
    expect(joined).toHaveBeenCalledWith(expect.objectContaining({ message: 'cancelled' }))
    call(0).args.onResult.onmessage(result(1))
    mockState.deferreds[0].resolve(undefined)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(mockState.calls.length).toBe(2) // 旧批仅撤订阅,无重试批
    expect(mockState.calls[1].cmd).toBe('cancel_viewport_thumbnail_request')
    expect(scanMock.autoThumbQueueSize).toBe(0)
  })
})
