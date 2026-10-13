import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref, type EffectScope } from 'vue'
import { IPC } from '../../constants/ipc'
import { useSeekbarPreview } from './useSeekbarPreview'

const { invokeIpc } = vi.hoisted(() => ({
  invokeIpc: vi.fn<(...args: unknown[]) => Promise<string | null>>(),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc }))
vi.mock('../../utils/assetUrl', () => ({ resolveAssetUrl: (path: string) => `asset:${path}` }))
vi.mock('../../stores/uiStore', () => ({ useUiStore: () => ({ videoKeyframeCount: 10 }) }))

class PreviewImage {
  static instances: PreviewImage[] = []
  src = ''
  onload: (() => void) | null = null
  onerror: (() => void) | null = null

  constructor() {
    PreviewImage.instances.push(this)
  }
}

let scope: EffectScope
let now: number

function setup() {
  const itemId = ref<number | null>(1)
  scope = effectScope()
  const preview = scope.run(() => useSeekbarPreview(itemId))!
  return { itemId, preview }
}

async function flush() {
  await nextTick()
  await new Promise<void>((resolve) => setImmediate(resolve))
}

beforeEach(() => {
  vi.resetAllMocks()
  now = 1_000
  vi.spyOn(Date, 'now').mockImplementation(() => now)
  PreviewImage.instances = []
  vi.stubGlobal('Image', PreviewImage)
})

afterEach(() => {
  scope.stop()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('进度条预览负缓存恢复', () => {
  it('同一视频的关键帧生成后，悬停或拖动在负缓存过期时重新加载', async () => {
    invokeIpc.mockResolvedValueOnce(null).mockResolvedValueOnce('/sprites/1.jpg')
    const { preview } = setup()
    await flush()
    expect(preview.hasSprite.value).toBe(false)

    now += 29_999
    expect(preview.styleForRatio(0.2)).toBeNull()
    expect(invokeIpc).toHaveBeenCalledTimes(1)

    now += 1
    expect(preview.styleForRatio(0.2)).toBeNull()
    expect(preview.styleForRatio(0.8)).toBeNull()
    await flush()
    expect(invokeIpc).toHaveBeenCalledTimes(2)
    expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_KEYFRAME_SPRITE, { itemId: 1 })
    expect(PreviewImage.instances).toHaveLength(1)
    PreviewImage.instances[0]!.onload?.()
    await flush()

    expect(preview.hasSprite.value).toBe(true)
    expect(preview.styleForRatio(1)).toMatchObject({
      backgroundImage: 'url("asset:/sprites/1.jpg")',
      backgroundSize: '1000% 100%',
      backgroundPosition: '100% 0%',
    })
    expect(invokeIpc).toHaveBeenCalledTimes(2)
  })

  it('失效图片重试期间切项，迟到的旧图片不会覆盖当前视频', async () => {
    invokeIpc.mockResolvedValueOnce('/sprites/1.jpg')
      .mockResolvedValueOnce('/sprites/1-rebuilt.jpg')
      .mockResolvedValueOnce('/sprites/2.jpg')
    const { itemId, preview } = setup()
    await flush()
    PreviewImage.instances[0]!.onerror?.()
    await flush()
    expect(preview.styleForRatio(0.5)).toBeNull()

    now += 30_000
    preview.styleForRatio(0.5)
    await flush()
    expect(PreviewImage.instances).toHaveLength(2)
    const oldImage = PreviewImage.instances[1]!

    itemId.value = 2
    await flush()
    expect(preview.styleForRatio(0.5)).toBeNull()
    PreviewImage.instances[2]!.onload?.()
    await flush()
    oldImage.onload?.()
    await flush()

    expect(preview.styleForRatio(0)).toMatchObject({
      backgroundImage: 'url("asset:/sprites/2.jpg")',
      backgroundPosition: '0% 0%',
    })
    expect(invokeIpc).toHaveBeenCalledTimes(3)
  })
})
