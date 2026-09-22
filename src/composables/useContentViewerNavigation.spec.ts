import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope, nextTick, reactive, ref, type EffectScope } from 'vue'
import type { Router, RouteLocationNormalizedLoaded } from 'vue-router'
import type { MediaDetail } from '../types/media'
import { IPC } from '../constants/ipc'

const invokeIpc = vi.fn()
vi.mock('../utils/ipc', () => ({ invokeIpc: (...args: unknown[]) => invokeIpc(...args) }))
vi.mock('../utils/assetUrl', () => ({ resolveAssetUrl: vi.fn() }))
vi.mock('./useThumbLoader', () => ({ buildThumbUrl: vi.fn() }))
vi.mock('../stores/uiStore', () => ({ useUiStore: () => ({}) }))
import { useMediaStore } from '../stores/mediaStore'
import { useViewIds } from './useViewIds'
import { useContentViewerPreload } from './useContentViewerPreload'
import { useContentViewerRouteNav } from './useContentViewerRouteNav'
import type { useImageEditor } from './useImageEditor'

function detail(id: number) { return { id } as MediaDetail }
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}
function publish(media: ReturnType<typeof useMediaStore>, layoutVersion: number, orderVersion: number, key = 'normal:dall:f{}') {
  media.layoutSummary = { layoutVersion, orderVersion, totalHeight: 1000, totalRows: 5, totalItems: 3, separators: [], monthBuckets: [] }
  media.layoutSemanticKey = key
  useViewIds().setExpectedVersion(key.startsWith('normal:') ? orderVersion : null)
}
let scope: EffectScope
beforeEach(() => {
  setActivePinia(createPinia())
  scope = effectScope()
  invokeIpc.mockReset()
  vi.useFakeTimers()
})
afterEach(() => { scope.stop(); vi.useRealTimers() })

it('同一当前 ID 换普通顺序/搜索/镜头会清缓存并隔离迟到预取,同顺序几何重排保持命中', async () => {
  const media = useMediaStore()
  media.detailItem = detail(10)
  publish(media, 1, 1)
  media.setLayoutNavContext(10)
  const old = deferred<MediaDetail>()
  invokeIpc.mockReturnValueOnce(old.promise)
  const preload = scope.run(() => useContentViewerPreload({
    media, detail: () => media.detailItem, thumbCacheDir: () => '', isHighResReady: () => true,
  }))!
  await vi.advanceTimersByTimeAsync(60)
  preload.scheduleIdlePreload()
  await vi.advanceTimersByTimeAsync(60)
  expect(invokeIpc).toHaveBeenCalledTimes(1)
  invokeIpc.mockResolvedValueOnce(detail(10)).mockResolvedValueOnce(detail(99))
  await media.openDetailFromSearch(10, [10, 99])
  await nextTick()
  await vi.advanceTimersByTimeAsync(60)
  expect(preload.getCachedAdjacent(10, 1)?.detail.id).toBe(99)
  old.resolve(detail(11))
  await nextTick()
  expect(preload.getCachedAdjacent(10, 1)?.detail.id).toBe(99)
  publish(media, 2, 2)
  media.setLayoutNavContext(10)
  expect(preload.getCachedAdjacent(10, 1)).toBeUndefined()
  invokeIpc.mockResolvedValueOnce(detail(77)).mockResolvedValueOnce(null)
  await nextTick()
  await vi.advanceTimersByTimeAsync(60)
  expect(preload.getCachedAdjacent(10, 1)?.detail.id).toBe(77)
  expect(invokeIpc).toHaveBeenCalledWith(IPC.GET_ADJACENT_MEDIA, { currentId: 10, offset: 1, orderVersion: 2 })
  publish(media, 3, 2)
  expect(preload.getCachedAdjacent(10, 1)?.detail.id).toBe(77)
  publish(media, 4, 4, 'lens:groups')
  media.setLensNavContext(4, 3)
  expect(preload.getCachedAdjacent(10, 1)).toBeUndefined()
  invokeIpc.mockResolvedValueOnce({ detail: detail(55), index: 1, totalCount: 3 }).mockResolvedValueOnce(null)
  await nextTick()
  await vi.advanceTimersByTimeAsync(60)
  expect(preload.getCachedAdjacent(10, 1)?.detail.id).toBe(55)
  publish(media, 5, 5, 'lens:folders:u1')
  media.setLensNavContext(5, 3)
  expect(preload.getCachedAdjacent(10, 1)).toBeUndefined()
  const late = deferred<{ detail: MediaDetail; index: number; totalCount: number }>()
  invokeIpc.mockReturnValueOnce(late.promise)
  await nextTick()
  await vi.advanceTimersByTimeAsync(60)
  expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_LENS_ADJACENT_MEDIA, { currentId: 10, offset: 1, layoutVersion: 5 })
  scope.stop()
  late.resolve({ detail: detail(66), index: 1, totalCount: 3 })
  await nextTick()
  expect(preload.getCachedAdjacent(10, 1)).toBeUndefined()
})

it('缓存翻页与在途翻页共用提交守卫,较早响应不能覆盖缓存已提交的项和路由', async () => {
  const media = useMediaStore()
  invokeIpc.mockResolvedValueOnce(detail(10))
  await media.openDetailFromSearch(10, [10, 11, 12])
  invokeIpc.mockResolvedValueOnce(detail(11))
  const preload = scope.run(() => useContentViewerPreload({ media, detail: () => media.detailItem, thumbCacheDir: () => '', isHighResReady: () => true }))!
  await vi.advanceTimersByTimeAsync(60)
  const first = deferred<MediaDetail>()
  const second = deferred<MediaDetail>()
  invokeIpc.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
  const navFirst = media.navigateDetail(1)
  const navSecond = media.navigateDetail(1)
  expect(media.navContext?.currentIndex).toBe(0)
  const replace = vi.fn()
  const route = reactive({ params: { id: '10' } }) as unknown as RouteLocationNormalizedLoaded
  const nav = scope.run(() => useContentViewerRouteNav({
    media, route, router: { replace } as unknown as Router,
    editor: { status: ref('idle') } as unknown as ReturnType<typeof useImageEditor>,
    getCachedAdjacent: preload.getCachedAdjacent,
  }))!
  const requestsBeforeCacheHit = invokeIpc.mock.calls.length
  await nav.navigate(1)
  expect(invokeIpc).toHaveBeenCalledTimes(requestsBeforeCacheHit)
  expect(media.detailItem?.id).toBe(11)
  expect(media.navContext?.currentIndex).toBe(1)
  expect(replace).toHaveBeenCalledWith('/view/11')
  second.resolve(detail(12))
  await navSecond
  first.resolve(detail(12))
  await navFirst
  expect(media.detailItem?.id).toBe(11)
  expect(media.navContext?.currentIndex).toBe(1)
  expect(replace).toHaveBeenCalledTimes(1)
  const pending = deferred<MediaDetail>()
  invokeIpc.mockReturnValueOnce(pending.promise)
  const navigating = nav.navigate(1)
  scope.stop()
  pending.resolve(detail(12))
  await navigating
  expect(replace).toHaveBeenCalledTimes(1)
})
