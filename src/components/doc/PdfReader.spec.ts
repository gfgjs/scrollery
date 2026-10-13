import { afterEach, expect, it, vi } from 'vitest'
import { createRenderer, nextTick, ssrContextKey } from 'vue'
import PdfReader from './PdfReader.vue'

const { getDocument } = vi.hoisted(() => ({ getDocument: vi.fn() }))

vi.mock('../../utils/pdfjs', () => ({
  getPdfjs: async () => ({ getDocument }),
}))
vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

// 挂载真实 setup 与卸载钩子；加载尚未完成时无需 DOM 或 canvas。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

afterEach(() => vi.unstubAllGlobals())

it('滚动到底时将较短末页作为当前页并保存其进度', async () => {
  // 首页面高 1200，末页面高 400；末页完全可见时页顶仍在视口 30% 探针之下。
  const pages = [24, 1240].map(offsetTop => ({
    offsetTop, style: {}, dataset: {}, querySelector: () => null,
  }))
  const onInfo = vi.fn()
  const onProgress = vi.fn()
  let onScroll: (() => void) | undefined
  let frame: FrameRequestCallback | undefined
  const scroll = {
    scrollTop: 0, clientHeight: 1000, scrollHeight: 1664,
    scrollTo({ top }: ScrollToOptions) {
      this.scrollTop = Math.max(0, Math.min(top ?? 0, this.scrollHeight - this.clientHeight))
      onScroll?.()
    },
    addEventListener: (_: string, listener: () => void) => { onScroll = listener },
    removeEventListener: vi.fn(),
  }
  let created = 0
  vi.stubGlobal('document', { createElement: () => pages[created++] })
  vi.stubGlobal('IntersectionObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frame = callback; return 1 })
  vi.stubGlobal('cancelAnimationFrame', vi.fn())
  getDocument.mockReturnValue({
    promise: Promise.resolve({
      numPages: 2,
      getPage: async () => ({ getViewport: () => ({ width: 800, height: 1200 }) }),
    }),
    destroy: vi.fn(async () => {}),
  })
  const app = renderer.createApp({ ...PdfReader, render: () => null }, {
    url: 'asset:/mixed-page-sizes.pdf', initial: 'page:1', onInfo, onProgress,
  })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  const state = (mounted.$ as unknown as {
    setupState: { scrollEl: typeof scroll; pagesEl: { appendChild(): void } }
  }).setupState
  state.scrollEl = scroll
  state.pagesEl = { appendChild() {} }
  try {
    await vi.waitFor(() => expect(onInfo).toHaveBeenLastCalledWith({ page: 1, pages: 2 }))
    const reader = mounted as unknown as { next(): void; prev(): void }
    reader.next()
    frame?.(0)
    expect(scroll.scrollTop).toBe(664)
    expect(onInfo).toHaveBeenLastCalledWith({ page: 2, pages: 2 })
    expect(onProgress).toHaveBeenLastCalledWith('page:2')

    reader.prev()
    frame?.(0)
    expect(scroll.scrollTop).toBe(24)
    expect(onInfo).toHaveBeenLastCalledWith({ page: 1, pages: 2 })
    expect(onProgress).toHaveBeenLastCalledWith('page:1')
  } finally {
    app.unmount()
  }
})

it('关闭仍在打开的 PDF 会终止加载任务且不报告就绪', async () => {
  let rejectLoad!: (reason: Error) => void
  const promise = new Promise<never>((_, reject) => { rejectLoad = reject })
  const destroy = vi.fn(async () => { rejectLoad(new Error('Loading aborted')) })
  getDocument.mockReturnValue({ promise, destroy })
  const ready = vi.fn()
  const app = renderer.createApp({ ...PdfReader, render: () => null }, {
    url: 'asset:/pending.pdf', initial: null, onReady: ready,
  })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  const state = (mounted.$ as unknown as { setupState: { error: string } }).setupState
  await nextTick()
  expect(getDocument).toHaveBeenCalledWith({ url: 'asset:/pending.pdf', isEvalSupported: false })

  app.unmount()
  try {
    expect(destroy).toHaveBeenCalledTimes(1)
    await nextTick()
    expect(state.error).toBe('')
    expect(ready).not.toHaveBeenCalled()
  } finally {
    rejectLoad(new Error('Test completed'))
    await nextTick()
  }
})
