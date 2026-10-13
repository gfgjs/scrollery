import { afterEach, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import { setDeferThumbLoad } from './useThumbLoadGate'

const hooks = vi.hoisted(() => ({
  mounted: [] as Array<() => unknown>,
  unmount: [] as Array<() => unknown>,
}))
vi.mock('vue', async (load) => ({
  ...await load<typeof import('vue')>(),
  onMounted: (callback: () => unknown) => hooks.mounted.push(callback),
  onBeforeUnmount: (callback: () => unknown) => hooks.unmount.push(callback),
}))
vi.mock('../utils/assetUrl', () => ({
  resolveAssetUrl: (path: string) => `asset://${path}`,
}))

import { useThumbLoader } from './useThumbLoader'

class DeferredImage {
  src = ''
  onerror: (() => void) | null = null
  finish: () => void = () => {}
  private decoded = new Promise<void>((resolve) => { this.finish = resolve })

  decode() {
    return this.decoded
  }
}

afterEach(() => {
  hooks.mounted.length = 0
  hooks.unmount.length = 0
  setDeferThumbLoad(false)
  vi.unstubAllGlobals()
})

it.each([1, 3])('状态 %s：快滚推迟新图源时，旧解码不得阻止停稳后的补载', async (status) => {
  const images: DeferredImage[] = []
  vi.stubGlobal('Image', class extends DeferredImage {
    constructor() {
      super()
      images.push(this)
    }
  })
  const path = ref('old.jpg')
  const scope = effectScope()
  const loader = scope.run(() => useThumbLoader({
    id: () => 7,
    thumbStatus: () => status,
    thumbPath: () => path.value,
    cacheDir: () => '/cache',
  }, {
    requestThumb: vi.fn(),
    cancelThumb: vi.fn(),
    regenerateThumb: vi.fn(),
  }))!

  try {
    for (const mount of hooks.mounted) void mount()
    expect(images).toHaveLength(1)
    setDeferThumbLoad(true)
    path.value = 'current.jpg'
    await nextTick()
    expect(images).toHaveLength(1)

    // 即使旧解码的完成回调已经排入队列，也不能让它抢占新图源的已加载状态。
    images[0].finish()
    await nextTick()
    expect(loader.isLoaded.value).toBe(false)
    expect(loader.displaySrc.value).toBe('')
    expect(images[0].src).toBe('')

    setDeferThumbLoad(false)
    await nextTick()
    expect(images).toHaveLength(2)
    images[1].finish()
    await nextTick()
    expect(loader.isLoaded.value).toBe(true)
    expect(loader.displaySrc.value).toBe(status === 1
      ? 'asset:///cache/thumbnails/current.jpg'
      : 'asset://current.jpg')
  } finally {
    for (const unmount of hooks.unmount) unmount()
    scope.stop()
  }
})
