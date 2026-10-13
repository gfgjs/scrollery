import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, type App } from 'vue'
import { IPC } from '../constants/ipc'

const { invokeIpc } = vi.hoisted(() => ({
  invokeIpc: vi.fn<(...args: unknown[]) => Promise<unknown>>(),
}))
vi.mock('../utils/ipc', () => ({ invokeIpc }))
vi.mock('../utils/assetUrl', () => ({ resolveAssetUrl: (path: string) => `asset:${path}` }))
vi.mock('../stores/uiStore', () => ({
  useUiStore: () => ({
    hoverAutoplay: true, hoverDelayMs: 200, heavyVideoMaxPixels: 3840 * 2160,
    heavyVideoMaxBytes: 10 * 1024 ** 3, videoKeyframeCount: 10,
  }),
}))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})
const apps: App[] = []
const testWindow = new EventTarget()
const testDocument = Object.assign(new EventTarget(), { visibilityState: 'visible' })
let useHoverPreview: typeof import('./useHoverPreview').useHoverPreview
let nextId = 1

beforeAll(async () => {
  // 在模块注册窗口监听前提供事件目标，测试不访问真实窗口或媒体文件。
  vi.stubGlobal('window', testWindow)
  vi.stubGlobal('document', testDocument)
  ;({ useHoverPreview } = await import('./useHoverPreview'))
})
beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  invokeIpc.mockReset()
  invokeIpc.mockResolvedValue({ absPath: '/mock/video.mp4' })
  testDocument.visibilityState = 'visible'
})
afterEach(() => {
  apps.splice(0).forEach((app) => app.unmount())
  vi.useRealTimers()
})
afterAll(() => vi.unstubAllGlobals())

function mountPreview() {
  const id = nextId++
  let preview!: ReturnType<typeof useHoverPreview>
  const app = renderer.createApp({
    setup() {
      preview = useHoverPreview({
        id: () => id, mediaType: () => 'video', isLivePhoto: () => false,
        fileSize: () => 100, videoWidth: () => 1280, videoHeight: () => 720,
        compact: () => false, isSelectionMode: () => false,
      })
      return () => null
    },
  })
  app.mount({})
  apps.push(app)
  return { id, preview }
}

async function settle() {
  await new Promise<void>((resolve) => setImmediate(resolve))
}

describe('图库悬停预览退出清理', () => {
  it('延迟起播前窗口失焦，取消计时且不解析媒体路径', async () => {
    const { preview } = mountPreview()
    preview.onEnter()
    vi.advanceTimersByTime(100)
    testWindow.dispatchEvent(new Event('blur'))
    vi.advanceTimersByTime(200)
    await settle()

    expect(invokeIpc).not.toHaveBeenCalled()
    expect(preview.isPreviewing.value).toBe(false)
    expect(preview.previewSrc.value).toBe('')
  })

  it('媒体路径解析期间页签隐藏，迟到响应不能在后台启动预览', async () => {
    let finish!: (detail: { absPath: string }) => void
    invokeIpc.mockReturnValueOnce(new Promise((resolve) => { finish = resolve }))
    const { id, preview } = mountPreview()
    preview.onEnter()
    vi.advanceTimersByTime(200)
    expect(invokeIpc).toHaveBeenCalledExactlyOnceWith(IPC.GET_MEDIA_DETAIL, { id })

    testDocument.visibilityState = 'hidden'
    testDocument.dispatchEvent(new Event('visibilitychange'))
    finish({ absPath: '/mock/late.mp4' })
    await settle()

    expect(preview.isPreviewing.value).toBe(false)
    expect(preview.previewSrc.value).toBe('')
    expect(preview.isScrubbing.value).toBe(false)
  })

  it('切到相邻格子后旧格清理不影响新格，失焦仍能结束新格预览', async () => {
    const first = mountPreview().preview
    const second = mountPreview().preview
    first.onEnter()
    vi.advanceTimersByTime(200)
    await settle()
    expect(first.isPreviewing.value).toBe(true)
    first.onLeave()
    second.onEnter()
    vi.advanceTimersByTime(200)
    await settle()
    first.onLeave()

    expect(first.isPreviewing.value).toBe(false)
    expect(second.isPreviewing.value).toBe(true)
    testWindow.dispatchEvent(new Event('blur'))
    expect(second.isPreviewing.value).toBe(false)
    expect(second.previewSrc.value).toBe('')
  })
})
