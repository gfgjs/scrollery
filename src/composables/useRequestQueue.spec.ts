// useRequestQueue 槽位生命周期锁测试(R2-5):去重 single-flight、批容量 24、50ms 合批、
// cancel 语义分叉(queued 释放 vs inFlight 保留)、finally 兜底、30s 停滞看门狗、released
// 幂等、按批 slot 判定、targetSize 阶梯、scanStore 簿记(泄漏观测面);
// 有界退避重试(2026-08-16 阶段3):停滞/缺结果自动重排收敛、上限终拒、cancel 掐断等待期、
// 退避期新等待者共享重试 slot。
// node 环境,fake timers;invoke 返回测试持有的 deferred(生产代码链 .catch().finally());
// 结果经捕获的 Channel stub 手动 onmessage 注入。魔数出处:flush 防抖 50ms、STALL_MS=30000、
// THUMB_RETRY_MAX=3、THUMB_RETRY_BASE_MS=500——均未导出,此处按字面量对拍。
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { DEFAULTS } from '../constants/defaults'
import type { ThumbResult } from '../types/media'
import type { LayoutRowNormal } from '../types/layout'
import { useCanvasThumbPipeline } from './useCanvasThumbPipeline'

type InvokeCall = { cmd: string; args: { itemIds: number[]; targetSize: number; onResult: ChannelStub } }
type ChannelStub = { onmessage: (msg: ThumbResult) => void }

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
function result(itemId: number): ThumbResult {
  return { itemId, thumbStatus: 2, thumbPath: `p/${itemId}.webp`, thumbhash: null }
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

describe('去重 single-flight', () => {
  it('queued 阶段同 id 两次 request 共享 slot:一次 invoke、itemIds 不重复、两 Promise 同结果', async () => {
    const q = useRequestQueue()
    const p1 = q.request(7)
    const p2 = q.request(7)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(1)
    expect(call(0).cmd).toBe('batch_request_thumbnails')
    expect(call(0).args.itemIds).toEqual([7])
    const r = result(7)
    call(0).args.onResult.onmessage(r)
    await expect(p1).resolves.toBe(r)
    await expect(p2).resolves.toBe(r)
  })

  it('inFlight 阶段再 request 同 id:不发第二次 invoke,送达后一并 resolve', async () => {
    const q = useRequestQueue()
    const p1 = q.request(7)
    await vi.advanceTimersByTimeAsync(50)
    const p2 = q.request(7)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(1)
    call(0).args.onResult.onmessage(result(7))
    await expect(p1).resolves.toMatchObject({ itemId: 7 })
    await expect(p2).resolves.toMatchObject({ itemId: 7 })
  })

  it('结算后同 id 可重取(去重仅在 slot 存活期内)', async () => {
    const q = useRequestQueue()
    const p1 = q.request(7)
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage(result(7))
    await p1
    void q.request(7)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(2)
    expect(call(1).args.itemIds).toEqual([7])
  })
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

  it('25 个 id:首批恰好 24 个,第 25 个滞留队列(簿记 1/24)', async () => {
    const q = useRequestQueue()
    for (let i = 1; i <= 25; i++) void q.request(i).catch(() => {})
    await vi.advanceTimersByTimeAsync(50)
    expect(DEFAULTS.THUMB_BATCH_SIZE).toBe(24)
    expect(call(0).args.itemIds).toEqual(Array.from({ length: 24 }, (_, i) => i + 1))
    expect(scanMock.autoThumbQueueSize).toBe(1)
    expect(scanMock.autoThumbInFlight).toBe(24)
  })

  it('上一批在途时不发新批(isFlushing 串行门);drain 后续排余量', async () => {
    const q = useRequestQueue()
    for (let i = 1; i <= 25; i++) void q.request(i).catch(() => {})
    await vi.advanceTimersByTimeAsync(50)
    void q.request(26).catch(() => {})
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(1)
    // 送达全部 24 个 → pending 清空 → releaseBatch → 50ms 后续排下一批
    for (let i = 1; i <= 24; i++) call(0).args.onResult.onmessage(result(i))
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(2)
    expect(call(1).args.itemIds).toEqual([25, 26])
  })

  it('50ms 防抖窗口内合批为一个 invoke', async () => {
    const q = useRequestQueue()
    void q.request(1).catch(() => {})
    await vi.advanceTimersByTimeAsync(30)
    void q.request(2).catch(() => {})
    await vi.advanceTimersByTimeAsync(20)
    expect(mockState.calls.length).toBe(1)
    expect(call(0).args.itemIds).toEqual([1, 2])
  })
})

describe('cancel 语义分叉', () => {
  it('queued:出队 + reject cancelled + 槽位释放(可重建、不泄漏)', async () => {
    const q = useRequestQueue()
    const err = catchErr(q.request(9))
    q.cancel(9)
    expect((await err).message).toBe('cancelled')
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(0)
    expect(scanMock.autoThumbQueueSize).toBe(0)
    void q.request(9)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(1)
  })

  it('inFlight:仅 reject waiters,slot 保留 single-flight;重挂后送达仍 resolve', async () => {
    const q = useRequestQueue()
    const err = catchErr(q.request(9))
    await vi.advanceTimersByTimeAsync(50)
    q.cancel(9)
    expect((await err).message).toBe('cancelled')
    expect(scanMock.autoThumbInFlight).toBe(1) // slot 未 detach
    const p2 = q.request(9) // 挂回同一 in-flight slot
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(1) // 无第二次 invoke
    call(0).args.onResult.onmessage(result(9))
    await expect(p2).resolves.toMatchObject({ itemId: 9 })
    expect(scanMock.autoThumbInFlight).toBe(0)
  })
})

describe('结算兜底与看门狗', () => {
  it('finally 兜底:被后端跳过的 id 经 500ms 退避重试后收敛(重试批送达即 resolve)', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    const p2 = q.request(2)
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage(result(1))
    mockState.deferreds[0].resolve(undefined)
    await vi.advanceTimersByTimeAsync(0)
    await expect(p1).resolves.toMatchObject({ itemId: 1 })
    // id2 无结果 → 不再直接拒绝,500ms 退避后重排新批
    await vi.advanceTimersByTimeAsync(500)
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(2)
    expect(call(1).args.itemIds).toEqual([2])
    call(1).args.onResult.onmessage(result(2))
    await expect(p2).resolves.toMatchObject({ itemId: 2 })
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

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
    expect((await err).message).toBe('Batch finished without result')
    expect((await joined)?.message).toBe('Batch finished without result')
    expect(mockState.calls.length).toBe(4)
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

  it('STALL_MS=30000 无进展 → 停滞释放并退避重试;4 次停滞后终拒', async () => {
    const q = useRequestQueue()
    const err = catchErr(q.request(1))
    await vi.advanceTimersByTimeAsync(50)
    for (let n = 0; n < 3; n++) {
      await vi.advanceTimersByTimeAsync(30_000) // 本轮停滞释放,退避起表
      await vi.advanceTimersByTimeAsync(500 * 2 ** n)
      await vi.advanceTimersByTimeAsync(50) // 重试批 flush
      expect(scanMock.autoThumbInFlight).toBe(1)
    }
    await vi.advanceTimersByTimeAsync(30_000) // 第 4 次停滞:超重试上限 → 终拒
    expect((await err).message).toBe('thumb batch stalled')
    expect(mockState.calls.length).toBe(4)
    expect(scanMock.autoThumbInFlight).toBe(0)
  })

  it('每个送达结果重置看门狗:距上次进展满 30s 才停滞;停滞耗尽重试后终拒', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    const err2 = catchErr(q.request(2))
    await vi.advanceTimersByTimeAsync(50)
    await vi.advanceTimersByTimeAsync(29_999)
    call(0).args.onResult.onmessage(result(1))
    await expect(p1).resolves.toMatchObject({ itemId: 1 })
    await vi.advanceTimersByTimeAsync(29_999)
    expect(scanMock.autoThumbInFlight).toBe(1) // 尚未停滞(看门狗已被送达重置)
    await vi.advanceTimersByTimeAsync(1)
    for (let n = 0; n < 3; n++) {
      await vi.advanceTimersByTimeAsync(500 * 2 ** n)
      await vi.advanceTimersByTimeAsync(50)
      await vi.advanceTimersByTimeAsync(30_000)
    }
    expect((await err2).message).toBe('thumb batch stalled')
  })

  it('released 幂等:stall 释放后,旧批迟到的 finally 不得冲掉新批', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    await vi.advanceTimersByTimeAsync(30_000) // 批 A 停滞释放
    const p2 = q.request(1) // 新批 B
    await vi.advanceTimersByTimeAsync(550) // 同 id 加入既有退避,到期后合批
    expect(mockState.calls.length).toBe(2)
    mockState.deferreds[0].resolve(undefined) // 批 A 的卡死 invoke 迟到结算
    await vi.advanceTimersByTimeAsync(0)
    expect(scanMock.autoThumbInFlight).toBe(1) // 批 B 计数未被清
    call(1).args.onResult.onmessage(result(1))
    await expect(p1).resolves.toMatchObject({ itemId: 1 })
    await expect(p2).resolves.toMatchObject({ itemId: 1 })
  })
})

describe('有界退避重试(2026-08-16 阶段3)', () => {
  it('停滞释放后自动重试并收敛:第二批送达 → 原 Promise resolve,调用方无需重发', async () => {
    const q = useRequestQueue()
    const p = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    await vi.advanceTimersByTimeAsync(30_000) // 批 A 停滞
    await vi.advanceTimersByTimeAsync(500)
    await vi.advanceTimersByTimeAsync(50) // 重试批 B flush
    expect(mockState.calls.length).toBe(2)
    expect(call(1).args.itemIds).toEqual([1])
    call(1).args.onResult.onmessage(result(1))
    await expect(p).resolves.toMatchObject({ itemId: 1 })
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
    expect(mockState.calls.length).toBe(1) // 定时器已掐,无重试批
    expect(scanMock.autoThumbQueueSize).toBe(0)
  })

  it('退避等待期同 id 请求共享退避,到期后只有一次重试并一并完成', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    await vi.advanceTimersByTimeAsync(30_000) // p1 停滞,退避 500ms 起表
    const p2 = q.request(1) // 等待期新请求加入既有逻辑请求
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(1)
    await vi.advanceTimersByTimeAsync(450) // 退避到期再排队
    await vi.advanceTimersByTimeAsync(50)
    expect(mockState.calls.length).toBe(2)
    call(1).args.onResult.onmessage(result(1))
    await expect(p1).resolves.toMatchObject({ itemId: 1 })
    await expect(p2).resolves.toMatchObject({ itemId: 1 })
  })
})

describe('按批 slot 判定', () => {
  it('陌生 id 的送达被静默忽略', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    await vi.advanceTimersByTimeAsync(50)
    call(0).args.onResult.onmessage(result(999))
    expect(scanMock.autoThumbInFlight).toBe(1) // p1 仍在途
    call(0).args.onResult.onmessage(result(1))
    await expect(p1).resolves.toMatchObject({ itemId: 1 })
  })

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

describe('targetSize 阶梯(THUMB_SIZE_TIERS 首个 ≥ rowHeight,超界回退末档)', () => {
  it.each([
    [200, 256],
    [512, 512],
    [1200, 1024],
  ])('gridRowHeight=%i → targetSize=%i', async (rowHeight, expected) => {
    uiMock.gridRowHeight = rowHeight
    const q = useRequestQueue()
    void q.request(1).catch(() => {})
    await vi.advanceTimersByTimeAsync(50)
    expect(call(0).args.targetSize).toBe(expected)
  })
})

describe('簿记同步(泄漏观测面)', () => {
  it('queued→inFlight→drain 全程计数正确且终态归零', async () => {
    const q = useRequestQueue()
    const p1 = q.request(1)
    const p2 = q.request(2)
    expect(scanMock.autoThumbQueueSize).toBe(2)
    expect(scanMock.autoThumbInFlight).toBe(0)
    await vi.advanceTimersByTimeAsync(50)
    expect(scanMock.autoThumbQueueSize).toBe(0)
    expect(scanMock.autoThumbInFlight).toBe(2)
    call(0).args.onResult.onmessage(result(1))
    expect(scanMock.autoThumbInFlight).toBe(1)
    call(0).args.onResult.onmessage(result(2))
    expect(scanMock.autoThumbInFlight).toBe(0)
    await Promise.all([p1, p2])
  })
})
