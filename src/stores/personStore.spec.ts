import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { IPC } from '../constants/ipc'
import type { LikelyMatchGroup } from '../types/person'
import { usePersonStore } from './personStore'

const { invokeIpc } = vi.hoisted(() => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../utils/thumbCacheDir', () => ({
  getThumbCacheDir: vi.fn().mockResolvedValue('/test-cache'),
}))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn() } }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

function group(faceIds: number[]): LikelyMatchGroup {
  return {
    personId: 1,
    personName: null,
    confidence: 0.9,
    candidateFaces: faceIds.map((faceId) => ({
      faceId,
      itemId: faceId,
      thumbPath: null,
      thumbStatus: null,
      bbox: [0, 0, 1, 1],
      similarity: 0.9,
    })),
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
})

describe('personStore 审批候选加载', () => {
  it('人物页旧请求晚于面板加载和审批完成时，不恢复已处理的候选', async () => {
    const pageResponse = deferred<LikelyMatchGroup[]>()
    const panelResponse = deferred<LikelyMatchGroup[]>()
    invokeIpc
      .mockReturnValueOnce(pageResponse.promise)
      .mockReturnValueOnce(panelResponse.promise)
      .mockResolvedValueOnce(undefined)

    const store = usePersonStore()
    await store.ensureCacheDir()
    // 重进人物页时保留上次候选，审批入口在页面请求尚未完成时仍可打开。
    store.likelyMatches = [group([11, 12])]
    const pageLoad = store.loadLikelyMatches(undefined, 50)
    await Promise.resolve()
    const panelLoad = store.loadLikelyMatches(undefined, 50)
    await Promise.resolve()
    expect(invokeIpc).toHaveBeenNthCalledWith(2, IPC.LIST_LIKELY_FACE_MATCHES, {
      personId: null,
      limit: 50,
    })

    panelResponse.resolve([group([11, 12])])
    await panelLoad
    await store.confirmFaces([11])
    expect(invokeIpc).toHaveBeenNthCalledWith(3, IPC.CONFIRM_FACES, { faceIds: [11] })
    expect(store.likelyMatches).toEqual([group([12])])

    pageResponse.resolve([group([11, 12])])
    await pageLoad
    expect(store.likelyMatches).toEqual([group([12])])

    // 后续主动加载仍应能正常发布新候选。
    invokeIpc.mockResolvedValueOnce([group([12, 13])])
    await store.loadLikelyMatches(undefined, 50)
    expect(store.likelyMatches).toEqual([group([12, 13])])
  })
})
