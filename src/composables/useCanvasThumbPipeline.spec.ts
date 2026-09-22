// useCanvasThumbPipeline characterization(超长文件拆分复核补测):锁三面既有行为——
// getImage 的 renderSig 失效判定、64 槽全局在途上限背压、cancelAbortableThumbLoads 的
// abortable/非 abortable 判定。项目无 DOM 测试环境(全 node,不引 jsdom):window/fetch/Image
// 用最小桩,写法对齐 useThumbLoader.spec.ts(先 vi.stubGlobal 再动态 import,避开模块顶层
// window.location.search 读取先于桩生效的坑)。同时覆盖视口生成需求的上限、取消与异步归属；
// 只消费公开 API，不触碰 thumbLoads / thumbRequests 等私有闭包状态。
import { afterEach, describe, it, expect, vi } from 'vitest'
import type { LayoutRow, LayoutRowItem } from '../types/layout'
import type { CanvasThumbPipeline, ThumbSrc } from './useCanvasThumbPipeline'
import { setDeferThumbLoad } from './useThumbLoadGate'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}))

// useThumbLoader 模块顶层读 window.location.search(?clear= 强刷串)→ node 下须先桩
// window 再动态 import(静态 import 会被提升到桩之前而炸)。
vi.stubGlobal('window', { location: { search: '' }, devicePixelRatio: 1 })

/** 安全 Image 桩:承接属性赋值,并保留实例供测试手动派发 load/error(cancelAbortableThumbLoads
 *  测的非 abortable 分支要走到 loadViaImage,需要 `new Image()` 不炸;批次 A 的 Image 回退
 *  所有权回归还需要真实触达 onload/onerror——不桩则回退永不落定,占槽缺口测不出来)。 */
const fakeImages: FakeImage[] = []
class FakeImage {
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  src = ''
  constructor() {
    fakeImages.push(this)
  }
}
vi.stubGlobal('Image', FakeImage)
// canvasThumbState 的字节预算 byteCost 用 `instanceof ImageBitmap` 分流(pipeline 侧构造函数);
// node 无该全局,给个空壳类满足 instanceof 判定即可,测试从不真正构造/close 它。
class FakeImageBitmap {}
vi.stubGlobal('ImageBitmap', FakeImageBitmap)

const { useCanvasThumbPipeline } = await import('./useCanvasThumbPipeline')
const { bitmapBucketH } = await import('../components/media/mediaGridCanvas.helpers')

function makeItem(overrides: Partial<LayoutRowItem> & { id: number }): LayoutRowItem {
  return {
    x: 0,
    w: 100,
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
    originalWidth: 100,
    originalHeight: 100,
    sortDatetime: 0,
    ...overrides,
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function makePipeline(fetchImpl?: (url: string) => Promise<unknown>) {
  vi.stubGlobal(
    'fetch',
    vi.fn((url: string) => {
      if (fetchImpl) return fetchImpl(url)
      throw new Error(`unexpected fetch: ${url}`)
    }),
  )
  const onRequestThumb = vi.fn<(id: number) => Promise<void>>(() => new Promise(() => {}))
  const onCancelThumb = vi.fn()
  const onRegenerateThumb = vi.fn()
  const scheduleDraw = vi.fn()
  const pipeline = useCanvasThumbPipeline({
    cacheDir: () => '/cache',
    onRequestThumb,
    onCancelThumb,
    onRegenerateThumb,
    scheduleDraw,
    viewportW: () => 800,
    viewportH: () => 600,
  })
  return { pipeline, onRequestThumb, onCancelThumb, onRegenerateThumb, scheduleDraw }
}

describe('视口生成请求生命周期', () => {
  it('同签名连续绘制只请求一次，生成状态变化后加载返回的源', () => {
    const { pipeline, onRequestThumb } = makePipeline(() => new Promise(() => {}))
    const item = makeItem({ id: 1 })
    pipeline.getImage(item)
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(1)
    pipeline.getImage({ ...item, thumbStatus: 1, thumbPath: 'ready.webp' })
    expect(globalThis.fetch).toHaveBeenCalledTimes(1)
  })

  it('一屏数千待生成项只提交两批需求，落定后重绘推进下一项', async () => {
    const done = deferred<void>()
    const { pipeline, onRequestThumb, scheduleDraw } = makePipeline()
    onRequestThumb.mockReturnValueOnce(done.promise)
    const items = Array.from({ length: 3000 }, (_, id) => makeItem({ id }))
    for (const item of items) pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(48)
    done.resolve(undefined)
    await done.promise
    await Promise.resolve()
    expect(scheduleDraw).toHaveBeenCalledTimes(1)
    pipeline.getImage(items[48])
    expect(onRequestThumb).toHaveBeenLastCalledWith(48)
  })

  it('离屏取消、重入可重发；旧请求迟到落定不释放新请求', async () => {
    const old = deferred<void>()
    const { pipeline, onRequestThumb, onCancelThumb, scheduleDraw } = makePipeline()
    onRequestThumb.mockReturnValueOnce(old.promise)
    const item = makeItem({ id: 1 })
    pipeline.getImage(item)
    pipeline.prioritizeVisibleThumbLoads([], 0, 0)
    expect(onCancelThumb).toHaveBeenCalledWith(1)
    pipeline.getImage(item)
    old.resolve(undefined)
    await old.promise
    await Promise.resolve()
    expect(scheduleDraw).not.toHaveBeenCalled()
    pipeline.prioritizeVisibleThumbLoads([], 0, 0)
    expect(onCancelThumb).toHaveBeenCalledTimes(2)
    expect(onRequestThumb).toHaveBeenCalledTimes(2)
  })

  it('已在当前视口的生成请求保留，预取不新增生成或路径解析需求', () => {
    const { pipeline, onRequestThumb, onCancelThumb } = makePipeline()
    const item = makeItem({ id: 1 })
    pipeline.getImage(item)
    pipeline.prioritizeVisibleThumbLoads(normalRows({ y: 0, items: [item] }), 0, 1)
    expect(onCancelThumb).not.toHaveBeenCalled()
    pipeline.getImage(makeItem({ id: 2 }), 'prefetch')
    pipeline.getImage(makeItem({ id: 3, thumbStatus: 3 }), 'prefetch')
    expect(onRequestThumb).toHaveBeenCalledTimes(1)
  })

  it('请求终拒释放额度并补绘，同签名不无限重试', async () => {
    const { pipeline, onRequestThumb, scheduleDraw } = makePipeline()
    onRequestThumb.mockRejectedValueOnce(new Error('failed'))
    const item = makeItem({ id: 1 })
    pipeline.getImage(item)
    await Promise.resolve()
    await Promise.resolve()
    expect(scheduleDraw).toHaveBeenCalledTimes(1)
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(1)
  })

  it.each(['pause', 'dispose'] as const)('%s 撤销生成需求，迟到落定不重绘，恢复后可请求', async (method) => {
    const done = deferred<void>()
    const { pipeline, onRequestThumb, onCancelThumb, scheduleDraw } = makePipeline()
    onRequestThumb.mockReturnValueOnce(done.promise)
    const item = makeItem({ id: 1 })
    pipeline.getImage(item)
    pipeline[method]()
    expect(onCancelThumb).toHaveBeenCalledWith(1)
    done.resolve(undefined)
    await done.promise
    await Promise.resolve()
    expect(scheduleDraw).not.toHaveBeenCalled()
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(2)
  })
})

describe('getImage renderSig 失效', () => {
  it('规格现行(renderSig 精确匹配)是纯命中:不发起刷新请求;规格过期(w/h 变化)则回退旧位图续画并发起一次刷新', () => {
    const { pipeline, onRequestThumb } = makePipeline()
    // status=0/path=null:sig 恒为 "0|",数据签名不随本用例变化——只有 renderSig(w/h 派生)变。
    const item = makeItem({ id: 1, thumbStatus: 0, thumbPath: null, w: 100, h: 100 })
    const bucketH = bitmapBucketH(item.h, 1)
    const freshRenderSig = `0|#${bucketH}x${Math.round((item.w / item.h) * 8)}`
    const fakeSrc = { tag: 'bitmap-v1' } as unknown as ThumbSrc
    pipeline.thumbState.syncSig(item.id, '0|')
    pipeline.thumbState.commitLoad(item.id, '0|', { src: fakeSrc, renderSig: freshRenderSig })

    // 规格精确匹配:纯缓存命中,不触碰 requestThumbOnce/onRequestThumb。
    const hit = pipeline.getImage(item)
    expect(hit?.src).toBe(fakeSrc)
    expect(onRequestThumb).not.toHaveBeenCalled()

    // w 改变(纵横比派生的 renderSig 随之变)→ 规格过期:仍回退旧位图续画(不留空白),
    // 但落入刷新分支,对 status=0 上抛一次生成请求(sig 未变、旧图未被凭空清空/复用为“新鲜”)。
    const resized = makeItem({ id: 1, thumbStatus: 0, thumbPath: null, w: 50, h: 100 })
    const stale = pipeline.getImage(resized)
    expect(stale?.src).toBe(fakeSrc) // 旧位图原样续画
    expect(stale?.renderSig).toBe(freshRenderSig) // 缓存条目未被就地篡改/清空
    expect(onRequestThumb).toHaveBeenCalledTimes(1)
    expect(onRequestThumb).toHaveBeenCalledWith(1)
  })
})

describe('64 槽全局在途上限背压', () => {
  it('loadingCount 达 MAX_IN_FLIGHT_THUMBS(64) 时新格直接回退占位,不发起 fetch/不占新槽', () => {
    const { pipeline } = makePipeline()
    for (let i = 1; i <= 64; i++) {
      pipeline.thumbState.syncSig(i, 's')
      pipeline.thumbState.markLoading(i)
    }
    expect(pipeline.thumbState.loadingCount()).toBe(64)

    const item = makeItem({ id: 999, thumbStatus: 1, thumbPath: 'p.jpg' })
    const result = pipeline.getImage(item)
    expect(result).toBeNull() // 无缓存、被背压拦下,只能占位
    expect(pipeline.thumbState.isLoading(999)).toBe(false) // 未被 markLoading 占新槽
    expect(globalThis.fetch).not.toHaveBeenCalled() // 背压检查先于 buildThumbUrl/fetch
  })
})

describe('cancelAbortableThumbLoads 判定', () => {
  it('dispose 只中止仍处 abortable 阶段的在途加载,已进入解码(abortable=false)的不被误伤', () => {
    // itemA:fetch 挂起不落定(仍在 fetch 阶段,controller 真能中止)——abortable 应保持 true。
    // itemB:fetch 同步抛(模拟网络层异常)→ 内部落 abortable=false 再回退 loadViaImage,
    // 对齐 loadBitmapImpl catch 分支「经典路径重试」语义,借此制造一个非 abortable 的在途项。
    const { pipeline } = makePipeline((url: string) => {
      if (url.includes('a.jpg')) return new Promise(() => {}) // 永不落定
      if (url.includes('b.jpg')) throw new Error('network layer error')
      throw new Error(`unexpected url: ${url}`)
    })
    const itemA = makeItem({ id: 1, thumbStatus: 1, thumbPath: 'a.jpg' })
    const itemB = makeItem({ id: 2, thumbStatus: 1, thumbPath: 'b.jpg' })
    pipeline.getImage(itemA)
    pipeline.getImage(itemB)
    expect(pipeline.thumbState.isLoading(1)).toBe(true)
    expect(pipeline.thumbState.isLoading(2)).toBe(true)

    const abortSpy = vi.spyOn(AbortController.prototype, 'abort')
    const cancelSpy = vi.spyOn(pipeline.thumbState, 'cancelLoad')
    pipeline.dispose() // 内部走 cancelAbortableThumbLoads() 再 thumbState.clear()

    // 只有 itemA(仍 abortable)被真正中止;itemB(已进入不可逆的经典路径回退)被跳过。
    expect(abortSpy).toHaveBeenCalledTimes(1)
    expect(cancelSpy).toHaveBeenCalledTimes(1)
    expect(cancelSpy).toHaveBeenCalledWith(1)
    expect(cancelSpy).not.toHaveBeenCalledWith(2)
  })
})

describe('Canvas pipeline async ownership', () => {
  it('pause 后不可取消的 decode 回包不得 commit 或 scheduleDraw', async () => {
    const first = deferred<{ width: number; height: number; close: () => void }>()
    const second = deferred<{ width: number; height: number; close: () => void }>()
    const closeFull = vi.fn()
    const closeOut = vi.fn()
    vi.stubGlobal(
      'createImageBitmap',
      vi
        .fn()
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise),
    )
    const { pipeline, scheduleDraw } = makePipeline(() =>
      Promise.resolve({ ok: true, blob: () => Promise.resolve(new Blob(['x'])) }),
    )
    const item = makeItem({ id: 7, thumbStatus: 1, thumbPath: 'late.jpg' })
    pipeline.getImage(item)
    await Promise.resolve()
    await Promise.resolve()

    pipeline.pause()
    first.resolve({ width: 100, height: 100, close: closeFull })
    await Promise.resolve()
    second.resolve({ width: 50, height: 50, close: closeOut })
    await Promise.resolve()
    await Promise.resolve()

    expect(pipeline.thumbState.get(item.id)).toBeUndefined()
    expect(scheduleDraw).not.toHaveBeenCalled()
    expect(closeFull).toHaveBeenCalledTimes(1)
    expect(closeOut).toHaveBeenCalledTimes(1)
  })
})

// ── 批次 A:可见需求优先调度 + Image 回退所有权(2026-09-12 密集缩略图方案 §4.2 / §9.2)────
// 用例先于实现落库,锁定当时两处缺口:①槽满时按「可见格数」而非「真实缺图数」腾槽,可见全
// 热也会取消仍有效的前向预取,且开闸态完全没有可见优先保证;②Image 回退在 fetch 异常分支
// 同步返回,任务身份被 finally 立即删除,迟到 onload/onerror 被 isCurrentLoad 一律拒绝 →
// 既不提交位图也不释放槽位(该格长期停在占位并占住全局在途额度)。

const DPR = 1

/** 已提交现行规格位图的可见项(命中:peek 有值且 renderSig 精确匹配)。 */
function cachedItem(pipeline: CanvasThumbPipeline, id: number): LayoutRowItem {
  const item = makeItem({ id, thumbStatus: 1, thumbPath: `hot${id}.jpg` })
  const sig = `1|hot${id}.jpg`
  pipeline.thumbState.syncSig(id, sig)
  pipeline.thumbState.commitLoad(id, sig, {
    src: { tag: `cached-${id}` } as unknown as ThumbSrc,
    renderSig: `${sig}#${bitmapBucketH(item.h, DPR)}x${Math.round((item.w / item.h) * 8)}`,
  })
  return item
}

/** 缺图但可加载(有 status/path 即有可用源)的可见项。 */
function coldItem(id: number): LayoutRowItem {
  return makeItem({ id, thumbStatus: 1, thumbPath: `cold${id}.jpg` })
}

function normalRows(...groups: Array<{ y: number; items: LayoutRowItem[] }>): LayoutRow[] {
  return groups.map((g) => ({ rowType: 'normal', y: g.y, height: 100, items: g.items }))
}

/** 灌满 64 个永不落定(仍在 fetch、仍可中止)的在途加载,模拟全局槽满。 */
function fillInFlightSlots(pipeline: CanvasThumbPipeline, firstId: number): void {
  for (let i = 0; i < 64; i++) {
    pipeline.getImage(makeItem({ id: firstId + i, thumbStatus: 1, thumbPath: `prefetch${i}.jpg` }))
  }
}

afterEach(() => {
  setDeferThumbLoad(false)
  fakeImages.length = 0
})

describe('Image 回退所有权(§9.2)', () => {
  it('fetch 异常转 Image 回退:任务身份保留到回退真正完成,onload 提交位图并释放在途槽位', async () => {
    const { pipeline, scheduleDraw } = makePipeline(() => {
      throw new Error('network layer error')
    })
    pipeline.getImage(makeItem({ id: 21, thumbStatus: 1, thumbPath: 'fallback.jpg' }))
    await Promise.resolve()
    await Promise.resolve()
    const img = fakeImages[fakeImages.length - 1]
    expect(img).toBeDefined()
    // 回退尚未完成:身份与槽位都必须仍在,否则迟到回包无处落定。
    expect(pipeline.thumbState.isLoading(21)).toBe(true)

    img.onload?.()
    expect(pipeline.thumbState.get(21)?.src).toBe(img)
    expect(pipeline.thumbState.isLoading(21)).toBe(false)
    expect(scheduleDraw).toHaveBeenCalled()
  })

  it('fetch 异常转 Image 回退:onerror 归入失败收口,释放槽位并按 status=1 上抛一次自愈', async () => {
    const { pipeline, onRegenerateThumb } = makePipeline(() => {
      throw new Error('network layer error')
    })
    pipeline.getImage(makeItem({ id: 22, thumbStatus: 1, thumbPath: 'bad.jpg' }))
    await Promise.resolve()
    await Promise.resolve()

    fakeImages[fakeImages.length - 1].onerror?.()
    expect(pipeline.thumbState.isFailed(22)).toBe(true)
    expect(pipeline.thumbState.isLoading(22)).toBe(false)
    expect(onRegenerateThumb).toHaveBeenCalledWith(22)
  })

  it('fetch 异常转 Image 回退:失活后的迟到 onload 不提交、不重绘,槽位由失活路径释放', async () => {
    const { pipeline, scheduleDraw } = makePipeline(() => {
      throw new Error('network layer error')
    })
    pipeline.getImage(makeItem({ id: 23, thumbStatus: 1, thumbPath: 'late.jpg' }))
    await Promise.resolve()
    await Promise.resolve()

    pipeline.pause()
    fakeImages[fakeImages.length - 1].onload?.()
    expect(pipeline.thumbState.get(23)).toBeUndefined()
    expect(pipeline.thumbState.isLoading(23)).toBe(false)
    expect(scheduleDraw).not.toHaveBeenCalled()
  })
})

describe('可见需求优先与预取保护(§4.2 S1/S2)', () => {
  it('槽满但可见全热:不因「槽满」取消仍有效的在途前向预取(S1)', () => {
    const { pipeline } = makePipeline(() => new Promise(() => {}))
    fillInFlightSlots(pipeline, 1000)
    expect(pipeline.thumbState.loadingCount()).toBe(64)
    const rows = normalRows(
      { y: 0, items: [cachedItem(pipeline, 11), cachedItem(pipeline, 12)] },
      { y: 100, items: [cachedItem(pipeline, 13), cachedItem(pipeline, 14)] },
      { y: 200, items: [cachedItem(pipeline, 15)] },
    )

    pipeline.prioritizeVisibleThumbLoads(rows, 0, rows.length)
    expect(pipeline.thumbState.loadingCount()).toBe(64)
  })

  it('关闸(飞掠)态同样是「可见全热 → 不取消」:腾槽规则不依赖闸门', () => {
    setDeferThumbLoad(true)
    const { pipeline } = makePipeline(() => new Promise(() => {}))
    fillInFlightSlots(pipeline, 1100)
    const rows = normalRows({ y: 0, items: [cachedItem(pipeline, 21), cachedItem(pipeline, 22)] })

    pipeline.prioritizeVisibleThumbLoads(rows, 0, rows.length)
    expect(pipeline.thumbState.loadingCount()).toBe(64)
  })

  it('开闸态可见有真实缺图且槽满:只为需求腾必要槽,近前向保护带(0.5 屏)内的在途预取不受影响(S2)', () => {
    const { pipeline } = makePipeline(() => new Promise(() => {}))
    const nearIds: number[] = []
    const farIds: number[] = []
    for (let i = 0; i < 10; i++) {
      const id = 200 + i
      nearIds.push(id)
      pipeline.getImage(makeItem({ id, thumbStatus: 1, thumbPath: `near${i}.jpg` }))
    }
    for (let i = 0; i < 54; i++) {
      const id = 300 + i
      farIds.push(id)
      pipeline.getImage(makeItem({ id, thumbStatus: 1, thumbPath: `far${i}.jpg` }))
    }
    expect(pipeline.thumbState.loadingCount()).toBe(64)

    // 可见块 [0,300):6 个真实缺图 → 只需 6 个槽;下一行 y=400 落在可见块下缘 +0.5 屏(300px)
    // 的保护带内,再下一行 y=800 已在带外(可取消)。
    const rows = normalRows(
      { y: 0, items: [coldItem(41), coldItem(42)] },
      { y: 100, items: [coldItem(43), coldItem(44)] },
      { y: 200, items: [coldItem(45), coldItem(46)] },
      { y: 400, items: nearIds.map((id) => makeItem({ id, thumbStatus: 1, thumbPath: 'near.jpg' })) },
      { y: 800, items: farIds.map((id) => makeItem({ id, thumbStatus: 1, thumbPath: 'far.jpg' })) },
    )

    pipeline.prioritizeVisibleThumbLoads(rows, 0, 3)
    expect(pipeline.thumbState.loadingCount()).toBe(58)
    for (const id of nearIds) expect(pipeline.thumbState.isLoading(id)).toBe(true)
    expect(farIds.filter((id) => !pipeline.thumbState.isLoading(id))).toEqual(farIds.slice(0, 6))
  })

  it('刚离屏的后向近带 fetch 保留,两侧远处任务仍可为可见冷格腾槽', () => {
    const { pipeline } = makePipeline(() => new Promise(() => {}))
    const nearAbove = coldItem(1200)
    const nearBelow = coldItem(1201)
    pipeline.getImage(nearAbove)
    pipeline.getImage(nearBelow)
    const farItems = Array.from({ length: 62 }, (_, i) => coldItem(1300 + i))
    for (const item of farItems) pipeline.getImage(item)
    const rows = normalRows(
      { y: 0, items: farItems.slice(0, 31) },
      { y: 400, items: [nearAbove] },
      { y: 600, items: [coldItem(1400), coldItem(1401)] },
      { y: 800, items: [nearBelow] },
      { y: 1200, items: farItems.slice(31) },
    )

    for (const direction of [1, -1] as const) {
      pipeline.setScrollDirection(direction)
      pipeline.prioritizeVisibleThumbLoads(rows, 2, 3)

      expect(pipeline.thumbState.isLoading(nearAbove.id)).toBe(true)
      expect(pipeline.thumbState.isLoading(nearBelow.id)).toBe(true)
      expect(pipeline.thumbState.loadingCount()).toBe(62)
      const cancelled = farItems.filter((item) => !pipeline.thumbState.isLoading(item.id))
      expect(cancelled).toHaveLength(2)
      for (const item of cancelled) pipeline.getImage(item)
    }
  })

  it('腾槽只针对仍可中止的 fetch:已进入解码(abortable=false)的任务保留在途账本', async () => {
    const decodePending = deferred<{ width: number; height: number; close: () => void }>()
    const { pipeline } = makePipeline((url: string) =>
      url.includes('decoding.jpg')
        ? Promise.resolve({ ok: true, blob: () => Promise.resolve(new Blob(['x'])) })
        : new Promise(() => {}),
    )
    vi.stubGlobal('createImageBitmap', vi.fn().mockReturnValue(decodePending.promise))
    const decodingId = 700
    pipeline.getImage(makeItem({ id: decodingId, thumbStatus: 1, thumbPath: 'decoding.jpg' }))
    for (let i = 0; i < 4; i++) await Promise.resolve()
    expect(pipeline.thumbState.isLoading(decodingId)).toBe(true)
    for (let i = 0; i < 63; i++) {
      pipeline.getImage(makeItem({ id: 800 + i, thumbStatus: 1, thumbPath: `far${i}.jpg` }))
    }
    expect(pipeline.thumbState.loadingCount()).toBe(64)

    const rows = normalRows({ y: 0, items: [coldItem(900)] })
    pipeline.prioritizeVisibleThumbLoads(rows, 0, 1)
    // 需求 1 → 只腾 1 槽;不可取消的解码任务不在其列,实际在途数不为账面释放所动。
    expect(pipeline.thumbState.loadingCount()).toBe(63)
    expect(pipeline.thumbState.isLoading(decodingId)).toBe(true)
  })

  it('idle/预取为可见真实需求保留空槽:预取分片填不满最后 16 槽,可见冷格下一帧即可起载', () => {
    const { pipeline } = makePipeline(() => new Promise(() => {}))
    for (let i = 0; i < 56; i++) {
      pipeline.getImage(makeItem({ id: 400 + i, thumbStatus: 1, thumbPath: `busy${i}.jpg` }))
    }
    const visible = [coldItem(51), coldItem(52), coldItem(53), coldItem(54), coldItem(55)]
    const rows = normalRows(
      { y: 0, items: visible },
      { y: 700, items: Array.from({ length: 20 }, (_, i) => coldItem(600 + i)) },
    )
    // 需求 5 → 预取额度 64−5=59:只能推进到 59,余下 5 槽留给可见格。
    pipeline.prioritizeVisibleThumbLoads(rows, 0, 1)
    pipeline.ensurePrefetchPlan(rows, 0, 0, 0, 5, false)
    pipeline.runDrawPrefetchSlice()
    expect(pipeline.thumbState.loadingCount()).toBe(59)

    for (const item of visible) pipeline.getImage(item)
    expect(pipeline.thumbState.loadingCount()).toBe(64)
    for (const item of visible) expect(pipeline.thumbState.isLoading(item.id)).toBe(true)
    pipeline.cancelPrefetchPlan()
  })

  it('分类前按当前签名对账:换源(status/path 变)的真冷项不被旧缓存判热,槽位照样腾出', () => {
    const { pipeline } = makePipeline(() => new Promise(() => {}))
    // 旧签名留着一张现行渲染规格的位图,但行数据已指向新路径 → 必须按真冷项处理。
    const stale = cachedItem(pipeline, 61)
    fillInFlightSlots(pipeline, 900)
    const rows = normalRows({ y: 0, items: [{ ...stale, thumbPath: 'new.jpg' }] })

    pipeline.prioritizeVisibleThumbLoads(rows, 0, 1)
    expect(pipeline.thumbState.get(61)).toBeUndefined() // 旧签名缓存已作废
    expect(pipeline.thumbState.loadingCount()).toBe(63) // 腾出 1 槽给真冷项
  })
})

describe('录制会话分界(指标不跨会话)', () => {
  function stubFrameClock() {
    vi.stubGlobal(
      'requestAnimationFrame',
      vi.fn(() => 1),
    )
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
  }

  it('上一会话因槽满留下的等待起点不被新会话的起载结算为 thumbWait', async () => {
    stubFrameClock()
    const { performanceRecorder } = await import('../perf/performanceRecorder')
    const pending = deferred<unknown>()
    const { pipeline } = makePipeline(() => pending.promise)
    const blocked = coldItem(41)

    performanceRecorder.start({ scenario: 'test-a' }, 1000 / 60)
    fillInFlightSlots(pipeline, 300) // 64 槽灌满
    pipeline.getImage(blocked) // 槽满 → 记下等待起点
    expect(pipeline.thumbState.isLoading(blocked.id)).toBe(false)
    const first = performanceRecorder.stop()
    expect(first?.spans['gallery.thumbWait'].samples).toBe(0)

    performanceRecorder.start({ scenario: 'test-b' }, 1000 / 60)
    // 释放一个槽后可见格起载:等待起点属于上一会话,必须被代次对账丢弃。
    pipeline.thumbState.cancelLoad(300)
    pipeline.getImage(blocked)
    const second = performanceRecorder.stop()
    expect(second?.spans['gallery.thumbWait'].samples).toBe(0)
    expect(second?.counters['gallery.coldCellMs']).toBeUndefined()
  })
})
