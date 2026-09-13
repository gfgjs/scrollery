import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ref } from 'vue'
import { useContentViewerRouteNav } from './useContentViewerRouteNav'
import type { MediaDetail } from '../types/media'

describe('useContentViewerRouteNav', () => {
  const mockRouter = {
    replace: vi.fn(),
  }
  const mockRoute = {
    params: { id: '1' },
  }

  beforeEach(() => {
    mockRouter.replace.mockReset()
  })

  it('命中缓存时更新 detailItem 与路由，并正确同步 navContext.currentIndex', async () => {
    let detailItem: MediaDetail | null = { id: 10, mediaType: 'image' } as MediaDetail
    const navContext = {
      type: 'search' as const,
      itemIds: [10, 20, 30],
      currentIndex: 0,
    }
    const mockMedia = {
      get detailItem() {
        return detailItem
      },
      set detailItem(val: MediaDetail | null) {
        detailItem = val
      },
      get navContext() {
        return navContext
      },
      navigateDetail: vi.fn(),
      openDetail: vi.fn(),
    }
    const mockEditor = { status: ref('idle') }
    const cachedItem = { id: 20, mediaType: 'image' } as MediaDetail
    const getCachedAdjacent = vi.fn().mockReturnValue(cachedItem)
    const onNavigate = vi.fn()

    const { navigate } = useContentViewerRouteNav({
      media: mockMedia as unknown as Parameters<typeof useContentViewerRouteNav>[0]['media'],
      editor: mockEditor as unknown as Parameters<typeof useContentViewerRouteNav>[0]['editor'],
      router: mockRouter as unknown as Parameters<typeof useContentViewerRouteNav>[0]['router'],
      route: mockRoute as unknown as Parameters<typeof useContentViewerRouteNav>[0]['route'],
      onNavigate,
      getCachedAdjacent,
    })

    await navigate(1)

    expect(onNavigate).toHaveBeenCalledWith(1)
    expect(getCachedAdjacent).toHaveBeenCalledWith(10, 1)
    expect(mockMedia.detailItem).toBe(cachedItem)
    expect(mockMedia.navContext.currentIndex).toBe(1)
    expect(mockRouter.replace).toHaveBeenCalledWith('/view/20')
    expect(mockMedia.navigateDetail).not.toHaveBeenCalled()
  })

  it('未命中缓存时回退调用 media.navigateDetail', async () => {
    let detailItem: MediaDetail | null = { id: 10, mediaType: 'image' } as MediaDetail
    const mockMedia = {
      get detailItem() {
        return detailItem
      },
      set detailItem(val: MediaDetail | null) {
        detailItem = val
      },
      navContext: null,
      navigateDetail: vi.fn().mockImplementation(async () => {
        detailItem = { id: 20, mediaType: 'image' } as MediaDetail
      }),
      openDetail: vi.fn(),
    }
    const mockEditor = { status: ref('idle') }
    const getCachedAdjacent = vi.fn().mockReturnValue(undefined)

    const { navigate } = useContentViewerRouteNav({
      media: mockMedia as unknown as Parameters<typeof useContentViewerRouteNav>[0]['media'],
      editor: mockEditor as unknown as Parameters<typeof useContentViewerRouteNav>[0]['editor'],
      router: mockRouter as unknown as Parameters<typeof useContentViewerRouteNav>[0]['router'],
      route: mockRoute as unknown as Parameters<typeof useContentViewerRouteNav>[0]['route'],
      getCachedAdjacent,
    })

    await navigate(1)

    expect(mockMedia.navigateDetail).toHaveBeenCalledWith(1)
    expect(mockRouter.replace).toHaveBeenCalledWith('/view/20')
  })

  it('镜头上下文跳过预加载缓存，始终交给后端镜头顺序解析', async () => {
    let detailItem: MediaDetail | null = { id: 10, mediaType: 'image' } as MediaDetail
    const navContext = {
      type: 'lens' as const,
      layoutVersion: 42,
      totalCount: 100,
      currentIndex: null,
    }
    const mockMedia = {
      get detailItem() {
        return detailItem
      },
      set detailItem(val: MediaDetail | null) {
        detailItem = val
      },
      navContext,
      layoutVersion: 42,
      viewTotalItems: 100,
      setLensNavContext: vi.fn(),
      navigateDetail: vi.fn().mockImplementation(async () => {
        detailItem = { id: 20, mediaType: 'image' } as MediaDetail
      }),
      openDetail: vi.fn(),
    }
    const getCachedAdjacent = vi.fn().mockReturnValue({ id: 99 } as MediaDetail)

    const { navigate } = useContentViewerRouteNav({
      media: mockMedia as unknown as Parameters<typeof useContentViewerRouteNav>[0]['media'],
      editor: { status: ref('idle') } as unknown as Parameters<
        typeof useContentViewerRouteNav
      >[0]['editor'],
      router: mockRouter as unknown as Parameters<typeof useContentViewerRouteNav>[0]['router'],
      route: mockRoute as unknown as Parameters<typeof useContentViewerRouteNav>[0]['route'],
      getCachedAdjacent,
      isLensActive: () => true,
    })

    await navigate(1)

    expect(getCachedAdjacent).not.toHaveBeenCalled()
    expect(mockMedia.navigateDetail).toHaveBeenCalledWith(1)
    expect(detailItem?.id).toBe(20)
    expect(mockRouter.replace).toHaveBeenCalledWith('/view/20')
  })

  it('编辑器非 idle 状态时禁止翻页', async () => {
    const mockMedia = {
      detailItem: { id: 10 } as MediaDetail,
      navContext: null,
      navigateDetail: vi.fn(),
    }
    const mockEditor = { status: ref('editing') }
    const getCachedAdjacent = vi.fn()

    const { navigate } = useContentViewerRouteNav({
      media: mockMedia as unknown as Parameters<typeof useContentViewerRouteNav>[0]['media'],
      editor: mockEditor as unknown as Parameters<typeof useContentViewerRouteNav>[0]['editor'],
      router: mockRouter as unknown as Parameters<typeof useContentViewerRouteNav>[0]['router'],
      route: mockRoute as unknown as Parameters<typeof useContentViewerRouteNav>[0]['route'],
      getCachedAdjacent,
    })

    await navigate(1)

    expect(getCachedAdjacent).not.toHaveBeenCalled()
    expect(mockMedia.navigateDetail).not.toHaveBeenCalled()
  })
})
