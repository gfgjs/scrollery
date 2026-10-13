import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref, type EffectScope } from 'vue'
import { useContentViewerPreload, type ViewerPreloadOptions } from './useContentViewerPreload'
import type { AdjacentDetail, NavigationContext } from '../stores/mediaStore'
import type { MediaDetail } from '../types/media'

vi.mock('../utils/assetUrl', () => ({ resolveAssetUrl: (path: string) => `asset:${path}` }))
vi.mock('./useThumbLoader', () => ({ buildThumbUrl: vi.fn() }))

const decodedUrls: string[] = []
class TestImage {
  src = ''
  decoding = ''
  onload = null
  onerror = null
  decode() {
    decodedUrls.push(this.src)
    return Promise.resolve()
  }
}

let scope: EffectScope | null = null
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(1000)
  vi.stubGlobal('Image', TestImage)
  decodedUrls.length = 0
})
afterEach(() => {
  scope?.stop()
  scope = null
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function createPreloader(ready: boolean, mediaType: MediaDetail['mediaType'] = 'image') {
  const highResReady = ref(ready)
  const detail = ref({ id: 2, mediaType } as MediaDetail)
  const context: NavigationContext = {
    type: 'search', itemIds: [1, 2, 3], currentIndex: 1,
  }
  const next: AdjacentDetail = {
    detail: {
      id: 3, mediaType: 'image', availability: 'online', absPath: '/next.jpg', thumbStatus: 0,
    } as MediaDetail,
    index: 2,
    totalCount: 3,
  }
  const fetchAdjacentDetail = vi.fn<
    (context: NavigationContext, id: number, offset: number) => Promise<AdjacentDetail | null>
  >().mockImplementation(async (_context, _id, offset) => offset === 1 ? next : null)
  const media = {
    navContext: context,
    isNavContextCurrent: (candidate: NavigationContext) => candidate === context,
    fetchAdjacentDetail,
  } as unknown as ViewerPreloadOptions['media']
  scope = effectScope()
  const preload = scope.run(() => useContentViewerPreload({
    detail: () => detail.value,
    media,
    thumbCacheDir: () => '',
    isHighResReady: () => highResReady.value,
  }))!
  return { preload, detail, highResReady, fetchAdjacentDetail, next }
}

describe('当前图片优先于邻图预加载', () => {
  it('翻页停稳时当前图尚未加载，不启动邻图查询或解码，就绪后自动恢复', async () => {
    const { preload, highResReady, fetchAdjacentDetail } = createPreloader(false)
    preload.markNavigation(1)
    await vi.advanceTimersByTimeAsync(240)

    expect(fetchAdjacentDetail).not.toHaveBeenCalled()
    expect(decodedUrls).toEqual([])

    highResReady.value = true
    await nextTick()
    await vi.advanceTimersByTimeAsync(60)
    expect(fetchAdjacentDetail).toHaveBeenCalledTimes(2)
    expect(decodedUrls).toEqual(['asset:/next.jpg'])
  })

  it('邻图查询期间当前图换源重新加载，迟到结果不触发解码，就绪后重新预取', async () => {
    const { highResReady, fetchAdjacentDetail, next } = createPreloader(true)
    let resolveAdjacent!: (result: AdjacentDetail) => void
    fetchAdjacentDetail.mockImplementationOnce(() => new Promise((resolve) => {
      resolveAdjacent = resolve
    }))
    await vi.advanceTimersByTimeAsync(60)
    expect(fetchAdjacentDetail).toHaveBeenCalledTimes(1)

    highResReady.value = false
    await nextTick()
    resolveAdjacent(next)
    await vi.advanceTimersByTimeAsync(0)
    expect(decodedUrls).toEqual([])
    expect(fetchAdjacentDetail).toHaveBeenCalledTimes(1)

    highResReady.value = true
    await nextTick()
    await vi.advanceTimersByTimeAsync(60)
    expect(decodedUrls).toEqual(['asset:/next.jpg'])
    expect(fetchAdjacentDetail).toHaveBeenCalledTimes(3)
  })
})

describe('视频邻项预加载', () => {
  it('首次打开视频时不等待图片就绪，预取邻项并解码前方图片', async () => {
    const { preload, highResReady, fetchAdjacentDetail, next } = createPreloader(false, 'video')

    await vi.advanceTimersByTimeAsync(60)

    expect(highResReady.value).toBe(false)
    expect(fetchAdjacentDetail).toHaveBeenCalledTimes(2)
    expect(preload.getCachedAdjacent(2, 1)).toEqual(next)
    expect(decodedUrls).toEqual(['asset:/next.jpg'])
  })

  it('快速翻到视频时暂停预加载，停稳后恢复邻项查询和前方图片解码', async () => {
    const { preload, detail, highResReady, fetchAdjacentDetail, next } = createPreloader(false)
    preload.markNavigation(1)
    await vi.advanceTimersByTimeAsync(20)
    preload.markNavigation(1)
    detail.value = { ...detail.value, mediaType: 'video' }
    await nextTick()

    await vi.advanceTimersByTimeAsync(179)
    expect(preload.isBursting.value).toBe(true)
    expect(fetchAdjacentDetail).not.toHaveBeenCalled()
    expect(decodedUrls).toEqual([])

    await vi.advanceTimersByTimeAsync(61)
    expect(highResReady.value).toBe(false)
    expect(preload.isBursting.value).toBe(false)
    expect(fetchAdjacentDetail).toHaveBeenCalledTimes(2)
    expect(preload.getCachedAdjacent(2, 1)).toEqual(next)
    expect(decodedUrls).toEqual(['asset:/next.jpg'])
  })
})
