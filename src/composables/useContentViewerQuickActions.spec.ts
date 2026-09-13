import { ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'

const { invokeIpc } = vi.hoisted(() => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../utils/assetUrl', () => ({ resolveAssetUrl: (path: string) => `asset:${path}` }))

import { useContentViewerQuickActions } from './useContentViewerQuickActions'
import type { useMediaDetail } from './useMediaDetail'
import type { useMediaStore } from '../stores/mediaStore'
import type { useToastStore } from '../stores/toastStore'
import type { MediaDetail } from '../types/media'

function mediaDetail(id: number): MediaDetail {
  return {
    id,
    rating: 0,
    colorLabel: 0,
    isFavorited: false,
  } as MediaDetail
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function setup(initial: MediaDetail) {
  let current: MediaDetail | null = initial
  const media = {
    toggleFavorite: vi.fn(),
    setRating: vi.fn(),
    setColorLabel: vi.fn(),
  } as unknown as ReturnType<typeof useMediaStore>
  const toast = { addToast: vi.fn() } as unknown as ReturnType<typeof useToastStore>
  const state = {
    isPlayingLive: ref(false),
    liveVideoSrc: ref<string | null>(null),
  } as unknown as ReturnType<typeof useMediaDetail>
  const api = useContentViewerQuickActions({
    detail: () => current,
    media,
    toast,
    t: (key) => key,
    state,
  })
  return { api, media, toast, state, setCurrent: (value: MediaDetail | null) => (current = value) }
}

describe('useContentViewerQuickActions 异步回写隔离', () => {
  it('收藏、评分与色标的旧项结果不会写入已切换的新项', async () => {
    const first = mediaDetail(1)
    const second = mediaDetail(2)
    const { api, media, setCurrent } = setup(first)
    const favorite = deferred<boolean>()
    const rating = deferred<void>()
    const color = deferred<void>()
    vi.mocked(media.toggleFavorite).mockReturnValueOnce(favorite.promise)
    vi.mocked(media.setRating).mockReturnValueOnce(rating.promise)
    vi.mocked(media.setColorLabel).mockReturnValueOnce(color.promise)

    const pending = [api.toggleFav(), api.setRating(4), api.setColorLabel(3)]
    setCurrent(second)
    favorite.resolve(true)
    rating.resolve()
    color.resolve()
    await Promise.all(pending)

    expect(second.isFavorited).toBe(false)
    expect(second.rating).toBe(0)
    expect(second.colorLabel).toBe(0)
  })

  it('旧项 Live Photo 请求完成后不启动新项播放，也不弹旧错误', async () => {
    const first = mediaDetail(1)
    const second = mediaDetail(2)
    const { api, toast, state, setCurrent } = setup(first)
    const companion = deferred<string>()
    invokeIpc.mockReturnValueOnce(companion.promise)

    const pending = api.toggleLive()
    setCurrent(second)
    companion.resolve('old.mov')
    await pending

    expect(state.isPlayingLive.value).toBe(false)
    expect(state.liveVideoSrc.value).toBeNull()
    expect(toast.addToast).not.toHaveBeenCalled()
  })
})
