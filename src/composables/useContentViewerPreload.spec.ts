import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { ref, effectScope, nextTick } from 'vue'
import { useContentViewerPreload } from './useContentViewerPreload'
import type { MediaDetail } from '../types/media'
import { IPC } from '../constants/ipc'

const mockInvokeIpc = vi.fn()
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => mockInvokeIpc(...args),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

describe('useContentViewerPreload', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockInvokeIpc.mockReset()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('连续快速翻页（<180ms）进入 burst 模式，停顿后自动 settle', async () => {
    const scope = effectScope()
    const detailRef = ref<MediaDetail | null>({
      id: 1,
      mediaType: 'image',
      availability: 'online',
      absPath: '/path/1.jpg',
      thumbStatus: 1,
      thumbPath: '1.webp',
      fileFormat: 'jpg',
    } as unknown as MediaDetail)
    const isHighResReadyRef = ref(true)
    const mockMedia = { navContext: null } as unknown as Parameters<
      typeof useContentViewerPreload
    >[0]['media']

    const preload = scope.run(() =>
      useContentViewerPreload({
        detail: () => detailRef.value,
        media: mockMedia,
        thumbCacheDir: () => '/cache',
        isHighResReady: () => isHighResReadyRef.value,
      }),
    )!

    expect(preload.isBursting.value).toBe(false)

    // 第一次翻页
    preload.markNavigation(1)
    expect(preload.isBursting.value).toBe(false)

    // 50ms 内第二次快速翻页 -> 进入 burst 模式
    vi.advanceTimersByTime(50)
    preload.markNavigation(1)
    expect(preload.isBursting.value).toBe(true)

    // 50ms 内第三次快速翻页 -> 保持 burst 模式
    vi.advanceTimersByTime(50)
    preload.markNavigation(1)
    expect(preload.isBursting.value).toBe(true)

    // 停止翻页 180ms -> 退出 burst 模式
    vi.advanceTimersByTime(180)
    expect(preload.isBursting.value).toBe(false)

    scope.stop()
  })

  it('空闲时预拉取前后相邻项并存入元数据缓存', async () => {
    const scope = effectScope()
    const nextItem = {
      id: 2,
      mediaType: 'image',
      availability: 'online',
      absPath: '/path/2.jpg',
      thumbStatus: 1,
      thumbPath: '2.webp',
      fileFormat: 'jpg',
    } as unknown as MediaDetail

    mockInvokeIpc.mockResolvedValue(nextItem)

    const detailRef = ref<MediaDetail | null>({
      id: 1,
      mediaType: 'image',
      availability: 'online',
      absPath: '/path/1.jpg',
      thumbStatus: 1,
      thumbPath: '1.webp',
      fileFormat: 'jpg',
    } as unknown as MediaDetail)
    const isHighResReadyRef = ref(true)
    const mockMedia = { navContext: null } as unknown as Parameters<
      typeof useContentViewerPreload
    >[0]['media']

    const preload = scope.run(() =>
      useContentViewerPreload({
        detail: () => detailRef.value,
        media: mockMedia,
        thumbCacheDir: () => '/cache',
        isHighResReady: () => isHighResReadyRef.value,
      }),
    )!

    preload.scheduleIdlePreload()
    vi.advanceTimersByTime(350)
    await nextTick()

    // 缓存应已建立
    expect(preload.getCachedAdjacent(1, 1)).toBeDefined()
    expect(preload.getCachedAdjacent(1, 1)?.id).toBe(2)

    scope.stop()
  })

  it('镜头预加载只走带 layoutVersion 的后端顺序，不调用普通邻接 IPC', async () => {
    const scope = effectScope()
    const nextItem = { id: 2, mediaType: 'image' } as MediaDetail
    mockInvokeIpc.mockResolvedValue({ detail: nextItem, index: 1, totalCount: 3 })
    const detailRef = ref<MediaDetail | null>({ id: 1, mediaType: 'image' } as MediaDetail)
    const isHighResReadyRef = ref(true)
    const mockMedia = {
      navContext: {
        type: 'lens' as const,
        layoutVersion: 42,
        totalCount: 3,
        currentIndex: 0,
      },
      layoutVersion: 42,
    } as unknown as Parameters<typeof useContentViewerPreload>[0]['media']

    const preload = scope.run(() =>
      useContentViewerPreload({
        detail: () => detailRef.value,
        media: mockMedia,
        thumbCacheDir: () => '',
        isHighResReady: () => isHighResReadyRef.value,
      }),
    )!

    preload.scheduleIdlePreload()
    vi.advanceTimersByTime(350)
    await nextTick()

    expect(mockInvokeIpc).toHaveBeenCalledWith(IPC.GET_LENS_ADJACENT_MEDIA, {
      currentId: 1,
      offset: 1,
      layoutVersion: 42,
    })
    expect(mockInvokeIpc).not.toHaveBeenCalledWith(IPC.GET_ADJACENT_MEDIA, expect.anything())
    scope.stop()
  })

  it('镜头上下文或 layoutVersion 切换会清空缓存，且丢弃旧请求迟到结果', async () => {
    const scope = effectScope()
    const first = deferred<{ detail: MediaDetail; index: number; totalCount: number }>()
    let navContext = {
      type: 'lens' as const,
      layoutVersion: 42,
      totalCount: 3,
      currentIndex: 0,
    }
    const mockMedia = {
      get navContext() {
        return navContext
      },
      layoutVersion: 42,
    } as unknown as Parameters<typeof useContentViewerPreload>[0]['media']
    const detailRef = ref<MediaDetail | null>({ id: 1, mediaType: 'image' } as MediaDetail)
    mockInvokeIpc.mockReturnValueOnce(first.promise).mockResolvedValue({
      detail: { id: 2, mediaType: 'image' },
      index: 1,
      totalCount: 3,
    })

    const preload = scope.run(() =>
      useContentViewerPreload({
        detail: () => detailRef.value,
        media: mockMedia,
        thumbCacheDir: () => '',
        isHighResReady: () => true,
      }),
    )!

    preload.scheduleIdlePreload()
    vi.advanceTimersByTime(350)
    await nextTick()
    navContext = { ...navContext, layoutVersion: 43 }
    first.resolve({ detail: { id: 2, mediaType: 'image' } as MediaDetail, index: 1, totalCount: 3 })
    await nextTick()

    expect(preload.getCachedAdjacent(1, 1)).toBeUndefined()
    scope.stop()
  })

  it('元数据缓存容量限制在 50 条以内', async () => {
    const scope = effectScope()
    const detailRef = ref<MediaDetail | null>({
      id: 1,
      mediaType: 'image',
      availability: 'online',
      absPath: '/path/1.jpg',
      thumbStatus: 1,
      thumbPath: '1.webp',
      fileFormat: 'jpg',
    } as unknown as MediaDetail)
    const isHighResReadyRef = ref(true)
    const mockMedia = { navContext: null } as unknown as Parameters<
      typeof useContentViewerPreload
    >[0]['media']

    const preload = scope.run(() =>
      useContentViewerPreload({
        detail: () => detailRef.value,
        media: mockMedia,
        thumbCacheDir: () => '/cache',
        isHighResReady: () => isHighResReadyRef.value,
      }),
    )!

    // 连续写入 60 个不同条目的邻近缓存
    for (let i = 1; i <= 60; i++) {
      detailRef.value = { id: i, mediaType: 'image', availability: 'online' } as MediaDetail
      mockInvokeIpc.mockResolvedValueOnce({ id: i + 1, mediaType: 'image', availability: 'online' })
      preload.scheduleIdlePreload()
      vi.advanceTimersByTime(350)
      await nextTick()
    }

    // 最早存入的第 1 项已被淘汰
    expect(preload.getCachedAdjacent(1, 1)).toBeUndefined()
    // 最新存入的第 60 项仍存在
    expect(preload.getCachedAdjacent(60, 1)?.id).toBe(61)

    scope.stop()
  })
})
