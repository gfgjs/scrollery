// 大图查看器可见源提交状态机回归：
// 可见 <img> 永远只绑定已完成离屏加载/解码的 URL；跨条目候选开始即撤旧帧，同条目派生
// 换源才保留当前图。原图失败时由加载态直接进入错误态，不把失败 URL 绑到可见 DOM。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref, type EffectScope } from 'vue'

class FakeImage {
  static instances: FakeImage[] = []

  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  decoding = 'auto'
  src = ''
  private decodePromise: Promise<void> = Promise.resolve()
  private resolveDecode: (() => void) | null = null
  private rejectDecode: ((reason?: unknown) => void) | null = null

  constructor() {
    FakeImage.instances.push(this)
  }

  decode(): Promise<void> {
    return this.decodePromise
  }

  holdDecode(): void {
    this.decodePromise = new Promise((resolve, reject) => {
      this.resolveDecode = resolve
      this.rejectDecode = reject
    })
  }

  finishDecode(): void {
    this.resolveDecode?.()
    this.resolveDecode = null
    this.rejectDecode = null
  }

  failDecode(): void {
    this.rejectDecode?.(new DOMException('图片解码失败', 'EncodingError'))
    this.resolveDecode = null
    this.rejectDecode = null
  }

  succeed(): void {
    this.onload?.()
  }

  fail(): void {
    this.onerror?.()
  }
}

vi.stubGlobal('Image', FakeImage)

import {
  useViewerImageSource,
  type ViewerImageSourceItem,
  type ViewerImageSource,
} from './useViewerImageSource'

const scopes: EffectScope[] = []

interface Host {
  item: ReturnType<typeof ref<ViewerImageSourceItem | null>>
  source: ReturnType<typeof ref<string>>
  api: ViewerImageSource
}

function makeHost(
  initialItem: ViewerImageSourceItem | null = {
    id: 1,
    mediaType: 'image',
    availability: 'online',
  },
  initialSource = 'asset://one.jpg',
  handleCandidateError?: (url: string, itemId: number) => boolean,
): Host {
  const item = ref<ViewerImageSourceItem | null>(initialItem)
  const source = ref(initialSource)
  const scope = effectScope()
  let api!: ViewerImageSource
  scope.run(() => {
    api = useViewerImageSource({
      item: () => item.value,
      source: () => source.value,
      handleCandidateError,
    })
  })
  scopes.push(scope)
  return { item, source, api }
}

async function flush(): Promise<void> {
  await nextTick()
  await Promise.resolve()
  await nextTick()
}

beforeEach(() => {
  FakeImage.instances = []
})

afterEach(() => {
  while (scopes.length) scopes.pop()!.stop()
})

describe('useViewerImageSource', () => {
  it('候选图加载并解码成功后才提交为可见源', async () => {
    const host = makeHost()
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.loading.value).toBe(true)

    const candidate = FakeImage.instances[0]
    candidate.holdDecode()
    candidate.succeed()
    await flush()
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.loading.value).toBe(true)

    candidate.finishDecode()
    await flush()

    expect(host.api.displaySrc.value).toBe('asset://one.jpg')
    expect(host.api.displayItemId.value).toBe(1)
    expect(host.api.loading.value).toBe(false)
    expect(host.api.loadError.value).toBe(false)
  })

  it('切换正常新图时立即撤下旧帧，新图解码成功后再原子提交', async () => {
    const host = makeHost()
    FakeImage.instances[0].succeed()
    await flush()

    host.item.value = { id: 2, mediaType: 'image', availability: 'online' }
    host.source.value = 'asset://two.jpg'
    await nextTick()

    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.displayItemId.value).toBeNull()
    expect(host.api.loading.value).toBe(true)

    FakeImage.instances[1].succeed()
    await flush()
    expect(host.api.displaySrc.value).toBe('asset://two.jpg')
    expect(host.api.displayItemId.value).toBe(2)
  })

  it('下一张原图损坏时不把失败 URL 绑定可见层，直接清旧帧并提交错误态', async () => {
    const host = makeHost()
    FakeImage.instances[0].succeed()
    await flush()

    host.item.value = { id: 2, mediaType: 'image', availability: 'online' }
    host.source.value = 'asset://broken.jpg'
    await nextTick()
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.displayItemId.value).toBeNull()

    FakeImage.instances[1].fail()
    await flush()
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.displayItemId.value).toBeNull()
    expect(host.api.loadError.value).toBe(true)
  })

  it('候选已触发 load 但 decode 失败时仍按损坏图片处理', async () => {
    const host = makeHost()
    FakeImage.instances[0].succeed()
    await flush()

    host.item.value = { id: 2, mediaType: 'image', availability: 'online' }
    host.source.value = 'asset://decode-broken.jpg'
    await nextTick()

    const broken = FakeImage.instances[1]
    broken.holdDecode()
    broken.succeed()
    await flush()
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.displayItemId.value).toBeNull()

    broken.failDecode()
    await flush()
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.displayItemId.value).toBeNull()
    expect(host.api.loadError.value).toBe(true)
  })

  it('下一张已知 missing 时立即撤下旧帧且不启动图片请求', async () => {
    const host = makeHost()
    FakeImage.instances[0].succeed()
    await flush()

    host.item.value = { id: 2, mediaType: 'image', availability: 'missing' }
    host.source.value = 'asset://missing.jpg'
    await flush()

    expect(FakeImage.instances).toHaveLength(1)
    expect(host.api.displaySrc.value).toBe('')
    expect(host.api.displayItemId.value).toBeNull()
    expect(host.api.loadError.value).toBe(false)
  })

  it('快速连翻时丢弃迟到的旧候选，只允许当前条目提交', async () => {
    const host = makeHost()
    const first = FakeImage.instances[0]

    host.item.value = { id: 2, mediaType: 'image', availability: 'online' }
    host.source.value = 'asset://two.jpg'
    await nextTick()
    first.succeed()
    await flush()
    expect(host.api.displaySrc.value).toBe('')

    FakeImage.instances[1].succeed()
    await flush()
    expect(host.api.displaySrc.value).toBe('asset://two.jpg')
    expect(host.api.displayItemId.value).toBe(2)
  })

  it('同条目 ICC 派生源加载期间保留原图，派生失败回退时原图不闪退', async () => {
    const source = ref('asset://original.jpg')
    const host = makeHost(undefined, source.value, (url) => {
      if (url !== 'asset://derived.jpg') return false
      source.value = 'asset://original.jpg'
      host.source.value = source.value
      return true
    })
    FakeImage.instances[0].succeed()
    await flush()

    host.source.value = 'asset://derived.jpg'
    await nextTick()
    expect(host.api.displaySrc.value).toBe('asset://original.jpg')
    FakeImage.instances[1].fail()
    await flush()

    expect(host.api.displaySrc.value).toBe('asset://original.jpg')
    expect(host.api.displayItemId.value).toBe(1)
    expect(host.api.loadError.value).toBe(false)
  })

  it('同条目 ICC 派生源解码成功前保留原图，成功后才提交派生图', async () => {
    const host = makeHost()
    FakeImage.instances[0].succeed()
    await flush()

    host.source.value = 'asset://derived.jpg'
    await nextTick()
    const derived = FakeImage.instances[1]
    derived.holdDecode()
    derived.succeed()
    await flush()
    expect(host.api.displaySrc.value).toBe('asset://one.jpg')

    derived.finishDecode()
    await flush()
    expect(host.api.displaySrc.value).toBe('asset://derived.jpg')
    expect(host.api.displayItemId.value).toBe(1)
  })

  it('可见资源在预解码后仍加载失败时，按当前 URL 判定回退或错误态', async () => {
    const host = makeHost()
    FakeImage.instances[0].succeed()
    await flush()

    host.api.handleDisplayError('asset://stale.jpg')
    expect(host.api.loadError.value).toBe(false)
    expect(host.api.displaySrc.value).toBe('asset://one.jpg')

    host.api.handleDisplayError('asset://one.jpg')
    expect(host.api.loadError.value).toBe(true)
    expect(host.api.displaySrc.value).toBe('')
  })
})
