// useCanvasThumbPipeline characterization(超长文件拆分复核补测):锁三面既有行为——
// getImage 的 renderSig 失效判定、64 槽全局在途上限背压、cancelAbortableThumbLoads 的
// abortable/非 abortable 判定。项目无 DOM 测试环境(全 node,不引 jsdom):window/fetch/Image
// 用最小桩,写法对齐 useThumbLoader.spec.ts(先 vi.stubGlobal 再动态 import,避开模块顶层
// window.location.search 读取先于桩生效的坑)。同时覆盖视口生成需求的上限、取消与异步归属；
// 只消费公开 API，不触碰 thumbLoads / thumbRequests 等私有闭包状态。
import { afterEach, describe, it, expect, vi } from 'vitest'
import type { LayoutRowItem } from '../types/layout'
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
    const current = deferred<void>()
    const { pipeline, onRequestThumb, onCancelThumb, scheduleDraw } = makePipeline()
    onRequestThumb
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(current.promise)
      .mockResolvedValue(undefined)
    const item = makeItem({ id: 1 })
    const rows = [{ rowType: 'normal' as const, y: 0, height: 100, items: [item] }]
    pipeline.getImage(item)
    pipeline.prioritizeVisibleThumbLoads([], 0, 0)
    expect(onCancelThumb).toHaveBeenCalledWith(1)
    pipeline.getImage(item)
    old.resolve(undefined)
    await old.promise
    await Promise.resolve()
    expect(scheduleDraw).not.toHaveBeenCalled()
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(2)

    // 父组件捕获队列重试耗尽后的异常，Promise 正常落定，项目仍保持 0/null 占位。
    current.resolve(undefined)
    await current.promise
    await Promise.resolve()
    pipeline.prioritizeVisibleThumbLoads(rows, 0, 1)
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(2) // 同屏不绕过队列的重试上限

    pipeline.prioritizeVisibleThumbLoads([], 0, 0)
    pipeline.prioritizeVisibleThumbLoads(rows, 0, 1)
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(3) // 离屏后重入获得新的请求机会
    await Promise.resolve()
    await Promise.resolve()
    pipeline.pause()
    pipeline.getImage(item)
    expect(onRequestThumb).toHaveBeenCalledTimes(4) // KeepAlive 失活也清理已落定需求
    pipeline.dispose()
  })
})

describe('Canvas pipeline async ownership', () => {
  it('pause 后源解码才完成时立即释放整图，不再启动裁剪', async () => {
    const source = deferred<{ width: number; height: number; close: () => void }>()
    const crop = deferred<{ width: number; height: number; close: () => void }>()
    const closeFull = vi.fn()
    const closeOut = vi.fn()
    const decode = vi.fn().mockReturnValueOnce(source.promise).mockReturnValueOnce(crop.promise)
    vi.stubGlobal('createImageBitmap', decode)
    const { pipeline, scheduleDraw } = makePipeline(() =>
      Promise.resolve({ ok: true, blob: () => Promise.resolve(new Blob(['x'])) }),
    )
    const item = makeItem({ id: 8, thumbStatus: 3, thumbPath: '/fixture/source.jpg' })
    pipeline.getImage(item)
    await Promise.resolve()
    await Promise.resolve()
    expect(decode).toHaveBeenCalledTimes(1)

    pipeline.pause()
    source.resolve({ width: 4000, height: 3000, close: closeFull })
    await Promise.resolve()

    // 同一异步边界比较：旧实现启动第二次解码并持有整图，直到裁剪 Promise 落定。
    expect.soft(decode).toHaveBeenCalledTimes(1)
    expect.soft(closeFull).toHaveBeenCalledTimes(1)
    crop.resolve({ width: 100, height: 100, close: closeOut })
    await Promise.resolve()
    await Promise.resolve()
    expect(pipeline.thumbState.get(item.id)).toBeUndefined()
    expect(scheduleDraw).not.toHaveBeenCalled()
    pipeline.dispose()
  })

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

    first.resolve({ width: 100, height: 100, close: closeFull })
    await Promise.resolve()
    // 裁剪已经开始才失活，继续覆盖不可取消产物迟到后的释放。
    pipeline.pause()
    second.resolve({ width: 50, height: 50, close: closeOut })
    await Promise.resolve()
    await Promise.resolve()

    expect(pipeline.thumbState.get(item.id)).toBeUndefined()
    expect(scheduleDraw).not.toHaveBeenCalled()
    expect(closeFull).toHaveBeenCalledTimes(1)
    expect(closeOut).toHaveBeenCalledTimes(1)
  })
})

afterEach(() => {
  setDeferThumbLoad(false)
  fakeImages.length = 0
})
