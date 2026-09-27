// useCanvasThumbPipeline characterization(超长文件拆分复核补测):锁三面既有行为——
// getImage 的 renderSig 失效判定、64 槽全局在途上限背压、cancelAbortableThumbLoads 的
// abortable/非 abortable 判定。项目无 DOM 测试环境(全 node,不引 jsdom):window/fetch/Image
// 用最小桩,写法对齐 useThumbLoader.spec.ts(先 vi.stubGlobal 再动态 import,避开模块顶层
// window.location.search 读取先于桩生效的坑)。同时覆盖视口生成需求的上限、取消与异步归属；
// 只消费公开 API，不触碰 thumbLoads / thumbRequests 等私有闭包状态。
import { afterEach, describe, it, expect, vi } from 'vitest'
import type { LayoutRow, LayoutRowItem } from '../types/layout'
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



/** 缺图但可加载(有 status/path 即有可用源)的可见项。 */
function coldItem(id: number): LayoutRowItem {
  return makeItem({ id, thumbStatus: 1, thumbPath: `cold${id}.jpg` })
}

function normalRows(...groups: Array<{ y: number; items: LayoutRowItem[] }>): LayoutRow[] {
  return groups.map((g) => ({ rowType: 'normal', y: g.y, height: 100, items: g.items }))
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
})
