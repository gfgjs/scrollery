// 视口请求生命周期：24 项总在途上限、逐项归还、受理后合批退订、迟到回包隔离。
// invoke 成功只表示受理；让路结束请求，明确的过期结果与通道停滞才有限退避。
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { ThumbResult } from '../types/media'
import type { LayoutRowNormal } from '../types/layout'
import { useCanvasThumbPipeline } from './useCanvasThumbPipeline'

type InvokeCall = { cmd: string; args: { itemIds: number[]; targetSize: number; requestId: string; onResult: ChannelStub } }
type ChannelStub = { onmessage: (msg: ThumbResult & { outcome: 'complete' | 'deferred' | 'retry' | 'failed'; error?: { code: string; message: string } }) => void }

const mockState = vi.hoisted(() => {
  const state = {
    calls: [] as Array<{ cmd: string; args: unknown }>,
    cancellations: [] as Array<{ requestId: string; itemIds: number[] }>,
    deferreds: [] as Array<{ resolve: (v: unknown) => void; reject: (e: unknown) => void }>,
    reset() {
      state.calls.length = 0
      state.cancellations.length = 0
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
      if (cmd === 'cancel_viewport_thumbnail_request') {
        mockState.cancellations.push(args as { requestId: string; itemIds: number[] })
        return Promise.resolve(undefined)
      }
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
function result(itemId: number): ThumbResult & { outcome: 'complete' } {
  return { itemId, thumbStatus: 2, thumbPath: `p/${itemId}.webp`, thumbhash: null, outcome: 'complete' }
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

describe('批容量与逐项名额', () => {
  it('Canvas 连续滚过 4000 项：离屏订阅撤销，无需等待旧批次即可服务最终视口', async () => {
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
    expect(scanMock.autoThumbInFlight).toBe(0)
    mockState.deferreds[0].resolve(undefined)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.cancellations).toEqual([{ requestId: call(0).args.requestId, itemIds: call(0).args.itemIds }])
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

  it('inFlight:取消立即归还，受理后合批退订，旧批回包不影响重订阅', async () => {
    const q = useRequestQueue()
    const err = catchErr(q.request(9))
    const other = catchErr(q.request(10))
    await vi.advanceTimersByTimeAsync(50)
    q.cancel(9)
    q.cancel(10)
    expect((await err).message).toBe('cancelled')
    expect((await other).message).toBe('cancelled')
    expect(scanMock.autoThumbInFlight).toBe(0)
    const p2 = q.request(9)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(2)
    expect(call(1).args.requestId).not.toBe(call(0).args.requestId)
    expect(mockState.cancellations).toHaveLength(0)
    mockState.deferreds[0].resolve(undefined)
    await vi.advanceTimersByTimeAsync(1)
    expect(mockState.cancellations).toEqual([{ requestId: call(0).args.requestId, itemIds: [9, 10] }])
    call(0).args.onResult.onmessage(result(9))
    expect(scanMock.autoThumbInFlight).toBe(1)
    call(1).args.onResult.onmessage(result(9))
    await expect(p2).resolves.toMatchObject({ itemId: 9 })
    expect(scanMock.autoThumbInFlight).toBe(0)
  })
})

describe('按批 slot 判定', () => {

  it('重复迟到消息按批内 pending 忽略,不影响同 id 的新 slot', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage(result(1))
    await p1
    const p2 = q.request(1) // 新 slot,新批
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage(result(1)) // 旧批 channel 的重复消息
    expect(scanMock.autoThumbInFlight).toBe(1) // 新 slot 不受影响
    call(1).args.onResult.onmessage(result(1))
    await expect(p2).resolves.toMatchObject({ itemId: 1 })
  })
})

describe('结果分类', () => {
  it('Exotic 让路释放名额且不执行短周期重试', async () => {
    const q = useRequestQueue()
    const promise = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage({ ...result(1), thumbStatus: 0, thumbPath: null, outcome: 'deferred' })
    await expect(promise).resolves.toMatchObject({ thumbStatus: 0 })
    await vi.advanceTimersByTimeAsync(4000)
    expect(mockState.calls).toHaveLength(1)
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

  it.each(['invoke', 'channel'])('%s 真实错误保留分类且不重试', async (source) => {
    const q = useRequestQueue()
    const error = catchErr(q.request(1))
    await vi.advanceTimersByTimeAsync(50)
    const failure = { code: 'Db', message: 'Database error' }
    if (source === 'invoke') mockState.deferreds[0].reject(failure)
    else call(0).args.onResult.onmessage({ ...result(1), outcome: 'failed', error: failure })
    // 旧实现会重试或把错误回包误报成功，先推进退避时间以观察最终结果。
    await vi.advanceTimersByTimeAsync(4000)
    expect(mockState.calls).toHaveLength(1)
    expect(await error).toMatchObject(failure)
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

  it('明确的过期结果有界重试，重复等待者共享预算', async () => {
    const q = useRequestQueue()
    const error = catchErr(q.request(1))
    const sameError = catchErr(q.request(1))
    await vi.advanceTimersByTimeAsync(50)
    for (let attempt = 0; attempt < 4; attempt++) {
      call(attempt).args.onResult.onmessage({ ...result(1), thumbStatus: 0, outcome: 'retry' })
      if (attempt < 3) await vi.advanceTimersByTimeAsync(500 * 2 ** attempt + 50)
    }
    expect(await error).toMatchObject({ reason: 'incomplete' })
    expect(await sameError).toBe(await error)
    expect(mockState.calls).toHaveLength(4)
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

  it('通道失联后退避重订阅，旧批迟到结果不影响恢复', async () => {
    const q = useRequestQueue()
    const promise = q.request(1)
    await vi.advanceTimersByTimeAsync(50 + 30000 + 500 + 50)
    expect(mockState.calls).toHaveLength(2)
    call(0).args.onResult.onmessage({ ...result(1), thumbPath: 'stale' })
    expect(scanMock.autoThumbInFlight).toBe(1)
    call(1).args.onResult.onmessage(result(1))
    await expect(promise).resolves.toMatchObject({ thumbPath: 'p/1.webp' })
    expect(scanMock.autoThumbInFlight).toBe(0)
  })
})
