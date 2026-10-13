import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, h, nextTick, ssrContextKey } from 'vue'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { parse, NodeTypes, type ElementNode, type TemplateChildNode } from '@vue/compiler-dom'
import { IPC } from '../../constants/ipc'
import VditorDocumentModule from './VditorDocumentModule.vue'
import { getVditorResourceBaseUrl } from './vditorResourceBase'

interface TestLute {
  SetLinkBase: (base: string) => void
  SetVditorIR: (enabled: boolean) => void
  SetVditorWYSIWYG: (enabled: boolean) => void
  SetVditorSV: (enabled: boolean) => void
  Md2VditorIRDOM: (markdown: string) => string
  VditorIRDOM2Md: (html: string) => string
  Md2VditorDOM: (markdown: string) => string
  VditorDOM2Md: (html: string) => string
}

interface EditorOptions {
  value?: string
  mode?: string
  theme?: string
  preview?: { theme?: { current?: string; path?: string }; hljs?: { style?: string }; markdown?: { linkBase?: string } }
  link?: { click?: (element: HTMLElement) => void }
  cache?: { id?: string; enable?: boolean }
  after?: () => void
  input?: (source: string) => void
}
const mocks = vi.hoisted(() => ({
  invoke: vi.fn(), toast: vi.fn(),
  outlineToggle: vi.fn(), outlineRender: vi.fn(), appendOutlineButton: vi.fn(),
  outlineStyle: { display: 'none' },
  findMatches: vi.fn(),
  scrollBy: vi.fn(),
  construct: vi.fn(), setTheme: vi.fn(), setValue: vi.fn(), destroy: vi.fn(),
  options: null as EditorOptions | null,
  source: '',
  mode: 'ir' as 'ir' | 'wysiwyg',
  html: '',
  lute: null as TestLute | null,
  serializationDocument: null as Document | null,
  leave: null as (() => Promise<boolean>) | null,
  update: null as (() => Promise<boolean>) | null,
}))
vi.mock('../../utils/ipc', () => ({
  invokeIpc: mocks.invoke, ipcErrorMessage: () => 'save failed',
}))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast: mocks.toast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('vditor/dist/js/icons/ant.js', () => ({}))
vi.mock('./vditorSearch', () => ({ findVditorMatches: mocks.findMatches }))
const paste = vi.hoisted(() => ({
  created: [] as Array<{
    onChange: (source: string) => void
    onRetry: () => void
    handle: ReturnType<typeof vi.fn>
    waitForPendingInputs: ReturnType<typeof vi.fn>
    dispose: ReturnType<typeof vi.fn>
  }>,
}))
vi.mock('./vditorImagePaste', () => ({
  createVditorImagePasteController: (_editor: unknown, callbacks: { onChange: (source: string) => void; onRetry: () => void }) => {
    const controller = {
      ...callbacks,
      handle: vi.fn(() => true),
      waitForPendingInputs: vi.fn(async () => 'idle'),
      dispose: vi.fn(),
    }
    paste.created.push(controller)
    return controller
  },
}))
vi.mock('vue-router', () => ({
  onBeforeRouteLeave: (guard: () => Promise<boolean>) => { mocks.leave = guard },
  onBeforeRouteUpdate: (guard: () => Promise<boolean>) => { mocks.update = guard },
}))
vi.mock('vditor', () => ({
  default: class {
    vditor = {
      get currentMode() { return mocks.mode },
      get lute() { return mocks.lute },
      ir: { element: {
        clientHeight: 200, scrollHeight: 1200, scrollTop: 0, getBoundingClientRect: () => ({ top: 10 }), scrollBy: mocks.scrollBy,
        querySelectorAll: () => [],
        // 实际长文由 contenteditable 本身滚动，外层固定容器不会移动正文。
        parentElement: { clientHeight: 200, getBoundingClientRect: () => ({ top: 10 }), scrollBy: vi.fn() },
      } },
      wysiwyg: { element: {
        clientHeight: 200, getBoundingClientRect: () => ({ top: 10 }), scrollBy: mocks.scrollBy,
        querySelectorAll: () => [],
        parentElement: { clientHeight: 200, getBoundingClientRect: () => ({ top: 10 }), scrollBy: vi.fn() },
      } },
      options: { outline: { enable: false, position: 'left' } },
      outline: {
        element: {
          style: mocks.outlineStyle,
          ownerDocument: { createElement: () => ({}) },
          firstElementChild: { appendChild: mocks.appendOutlineButton },
        },
        toggle: mocks.outlineToggle,
        render: mocks.outlineRender,
      },
    }
    constructor(_host: unknown, options: EditorOptions) {
      mocks.construct()
      mocks.options = options
      // 与第三方初始化相同：缓存默认优先，禁用后才使用磁盘文本。
      mocks.source = (options.cache?.enable !== false && localStorage.getItem(options.cache?.id ?? '')) || options.value || ''
      if (mocks.lute) {
        mocks.lute.SetLinkBase(options.preview?.markdown?.linkBase ?? '')
        mocks.lute.SetVditorIR(true)
        mocks.lute.SetVditorWYSIWYG(false)
        mocks.lute.SetVditorSV(false)
        mocks.html = mocks.lute.Md2VditorIRDOM(mocks.source)
      }
    }
    getValue() {
      if (!mocks.lute) return mocks.source
      return mocks.mode === 'ir' ? mocks.lute.VditorIRDOM2Md(mocks.html) : mocks.lute.VditorDOM2Md(mocks.html)
    }
    setValue(source: string) { mocks.setValue(source); mocks.source = source }
    setTheme(...appearance: string[]) { mocks.setTheme(...appearance) }
    destroy() { mocks.destroy() }
  },
}))

const renderer = createRenderer<object, object>({
  createElement: () => ({ ownerDocument: mocks.serializationDocument }), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})
const cacheKey = 'scrollery-vditor-41'
const defaultTypography = { fontSizePx: 19, lineHeight: 1.75, fontFamily: 'serif' as const, maxInlineSizePx: 720 }
let cache: Map<string, string>

beforeEach(() => {
  vi.useFakeTimers()
  mocks.invoke.mockReset()
  mocks.toast.mockReset()
  mocks.outlineStyle.display = 'none'
  mocks.outlineToggle.mockReset()
  // Vditor 在窄窗中拒绝显示目录，即使 show=true。
  mocks.outlineToggle.mockImplementation(() => { mocks.outlineStyle.display = 'none' })
  mocks.outlineRender.mockReset()
  mocks.appendOutlineButton.mockReset()
  mocks.findMatches.mockReset()
  mocks.scrollBy.mockReset()
  mocks.construct.mockReset()
  mocks.setTheme.mockReset()
  mocks.setValue.mockReset()
  mocks.destroy.mockReset()
  mocks.mode = 'ir'
  mocks.lute = null
  mocks.html = ''
  mocks.serializationDocument = null
  mocks.leave = mocks.update = null
  paste.created.length = 0
  cache = new Map()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => cache.get(key) ?? null,
    removeItem: (key: string) => cache.delete(key),
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

function mountEditor(overrides: { documentUrl?: string; value?: string; initial?: string | null } = {}) {
  const change = vi.fn()
  const panelOpen = vi.fn()
  const progress = vi.fn()
  const app = renderer.createApp({ ...VditorDocumentModule, render: () => h('div', { ref: 'host' }) }, {
    itemId: 41, value: overrides.value ?? '# 原文', documentUrl: overrides.documentUrl ?? 'http://asset.localhost/C%3A%5Cnotes%5Cbook.md',
    isDark: false, initial: overrides.initial, typography: defaultTypography, onChange: change, onPanelOpen: panelOpen, onProgress: progress,
  })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  mocks.options?.after?.()
  const state = (mounted.$ as unknown as {
    setupState: {
      restoreCachedDraft: () => void; saveState: string
      searchOpen: boolean; searchQuery: string; searchMatches: Range[]; searchIndex: number
      handleKeyDown: (event: KeyboardEvent) => void; moveSearch: (direction: number) => void
      editor: { vditor: { ir: { element: { scrollTop: number } } } }
    }
  }).setupState
  return { app, state, change, panelOpen, progress, exposed: mounted as unknown as {
    toggleToc?: () => void; toggleSearch?: () => void; closePanels?: () => boolean
    getCurrentPosition?: () => string | null
  } }
}

it('旧缓存仅在明确恢复后进入自动保存，成功前保留缓存且初次打开不覆盖源文', async () => {
  cache.set(cacheKey, '# 旧草稿')
  mocks.invoke.mockResolvedValue(undefined)
  const { app, state, change } = mountEditor()
  try {
    expect(mocks.source).toBe('# 原文')
    expect(mocks.options?.mode).toBe('ir')
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.invoke).not.toHaveBeenCalled()
    expect(cache.get(cacheKey)).toBe('# 旧草稿')

    state.restoreCachedDraft()
    expect(mocks.source).toBe('# 旧草稿')
    expect(change).toHaveBeenLastCalledWith('# 旧草稿')
    expect(cache.get(cacheKey)).toBe('# 旧草稿')
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith(IPC.SAVE_VERSION, {
      itemId: 41, content: '# 旧草稿', label: null, parentId: null,
      target: 'overwrite', source: 'user',
    })
    expect(cache.has(cacheKey)).toBe(false)
  } finally {
    app.unmount()
  }
})

it('相对图片使用文档目录且在跨模式与跨平台往返保存后保留相对路径', async () => {
  const runtime = { require: createRequire(import.meta.url), setTimeout, clearTimeout, console }
  runInNewContext(readFileSync(new URL('../../../public/vditor/dist/js/lute/lute.min.js', import.meta.url), 'utf8'), runtime)
  const factory = (runtime as unknown as { Lute: { New: () => TestLute } }).Lute
  const baseline = factory.New()
  const source = '![目录内的图片 alt](<images/示例 image.svg> "图片 title")\n\n![space](<../shared/a b.png>)\n\n![percent](./images/a%252Fb.png)\n\n[相对文档](<../shared/notes with space.md>)\n'
  const normalized = '![目录内的图片 alt](images/示例%20image.svg "图片 title")\n\n![space](../shared/a%20b.png)\n\n![percent](./images/a%252Fb.png)\n\n[相对文档](../shared/notes%20with%20space.md)\n'
  mocks.lute = factory.New()
  mocks.serializationDocument = createImageSerializationDocument()
  mocks.invoke.mockResolvedValue(undefined)
  const openLink = vi.fn()
  vi.stubGlobal('window', { open: openLink })
  const { app, state } = mountEditor({ documentUrl: 'http://asset.localhost/C%3A%5CLibrary%20space%5Cnotes%5Cbook.md', value: source })
  try {
    expect(mocks.options?.preview?.markdown?.linkBase).toBe('http://asset.localhost/C%3A/Library%20space/notes/')
    const clickLink = (destination: string, attribute = false) => mocks.options?.link?.click?.({
      getAttribute: () => attribute ? destination : null, textContent: destination,
    } as unknown as HTMLElement)
    clickLink('../shared/notes with space.md')
    expect(openLink).toHaveBeenLastCalledWith('http://asset.localhost/C%3A/Library%20space/shared/notes%20with%20space.md')
    clickLink('#section')
    expect(openLink).toHaveBeenLastCalledWith('http://asset.localhost/C%3A/Library%20space/notes/book.md#section')
    clickLink('http://asset.localhost/C%3A/Library%20space/notes/#section', true)
    expect(openLink).toHaveBeenLastCalledWith('http://asset.localhost/C%3A/Library%20space/notes/book.md#section')
    clickLink('https://example.invalid/path?q=1#part', true)
    expect(openLink).toHaveBeenLastCalledWith('https://example.invalid/path?q=1#part')
    clickLink('asset://localhost/home/manual.md', true)
    expect(openLink).toHaveBeenLastCalledWith('asset://localhost/home/manual.md')
    clickLink('mailto:reader@example.invalid', true)
    expect(openLink).toHaveBeenLastCalledWith('mailto:reader@example.invalid')
    clickLink('tel:+123456789', true)
    expect(openLink).toHaveBeenLastCalledWith('tel:+123456789')
    openLink.mockClear()
    clickLink('javascript:alert(1)')
    clickLink('data:text/html,hello')
    clickLink('file:///C:/outside.md')
    expect(openLink).not.toHaveBeenCalled()
    const root = Object.assign(state.editor.vditor.ir.element, { querySelectorAll: () => [
      { id: 'ir-ir-anchor_38_5', offsetTop: 100 },
      { id: 'ir-anchor_38', offsetTop: 900 },
      { id: 'ir-远处章节_40', offsetTop: 1200 },
      { id: 'ir-远处章节-_42', offsetTop: 1500 },
    ] })
    clickLink('#anchor')
    expect(root.scrollTop).toBe(900)
    clickLink('#ir-anchor_38')
    expect(root.scrollTop).toBe(900) // 完整 ID 优先于去前后缀后的同名。
    clickLink('#%E8%BF%9C%E5%A4%84%E7%AB%A0%E8%8A%82')
    expect(root.scrollTop).toBe(1200)
    clickLink('#远处章节-')
    expect(root.scrollTop).toBe(1500)
    expect(openLink).not.toHaveBeenCalled()
    clickLink('other.md#anchor')
    expect(openLink).toHaveBeenLastCalledWith('http://asset.localhost/C%3A/Library%20space/notes/other.md#anchor')
    clickLink('#missing')
    expect(openLink).toHaveBeenLastCalledWith('http://asset.localhost/C%3A/Library%20space/notes/book.md#missing')
    clickLink('#%E0%A4%A')
    expect(openLink).toHaveBeenLastCalledWith('http://asset.localhost/C%3A/Library%20space/notes/book.md#%E0%A4%A')
    expect(root.scrollTop).toBe(1500)
    const imageUrls = [...mocks.html.matchAll(/<img src="([^"]+)"/g)].map((match) => new URL(match[1]!).href)
    expect(imageUrls).toEqual([
      'http://asset.localhost/C%3A/Library%20space/notes/images/%E7%A4%BA%E4%BE%8B%20image.svg',
      'http://asset.localhost/C%3A/Library%20space/shared/a%20b.png',
      'http://asset.localhost/C%3A/Library%20space/notes/images/a%252Fb.png',
    ])
    expect(mocks.lute.VditorIRDOM2Md(mocks.html)).toBe(normalized)

    // 真实 getValue 的 IR / WYSIWYG 各自调用对应 DOM2Md；切模式再编辑后经真实保存队列落盘。
    mocks.mode = 'wysiwyg'
    mocks.lute.SetVditorIR(false)
    mocks.lute.SetVditorWYSIWYG(true)
    mocks.html = mocks.lute.Md2VditorDOM(`${source}\n新增段落\n`)
    baseline.SetVditorWYSIWYG(true)
    const expected = baseline.VditorDOM2Md(baseline.Md2VditorDOM(`${normalized}\n新增段落\n`))
    expect(await mocks.leave?.()).toBe(true)
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith(IPC.SAVE_VERSION, {
      itemId: 41, content: expected, label: null, parentId: null, target: 'overwrite', source: 'user',
    })

    // 同一份已保存文档在 Unix 资源 URL 下重新打开，编码过的文件名不能被二次解码。
    const unixBase = getVditorResourceBaseUrl('asset://localhost/%2Fhome%2Fbook%252Fnotes%2Fbook.md')
    expect(unixBase).toBe('asset://localhost//home/book%252Fnotes/')
    const reopened = factory.New()
    reopened.SetLinkBase(unixBase)
    reopened.SetVditorIR(true)
    const reopenedHtml = reopened.Md2VditorIRDOM(expected)
    expect(reopenedHtml).toContain('src="asset://localhost//home/book%252Fnotes/../shared/a%20b.png"')
    expect(reopened.VditorIRDOM2Md(reopenedHtml)).toBe(expected)
    expect([...reopenedHtml.matchAll(/<img /g)]).toHaveLength(3)
  } finally {
    app.unmount()
  }
})

// Node 无 DOM：用项目已有 HTML 解析器建节点关系，仅适配本模块使用的 template/资源/marker API。
// 选择器按真实父子和 class/data-type 定位，正文和属性修改以各自源区间回写，避免硬编码图片输出。
function createImageSerializationDocument(): Document {
  return {
    createElement: () => {
      let html = ''
      let edits: Array<{ start: number; end: number; value: string }> = []
      interface Entry { node: ElementNode; parent?: Entry }
      let elements: Entry[] = []
      const attribute = (entry: Entry, name: string) => entry.node.props.find((prop) => prop.type === NodeTypes.ATTRIBUTE && prop.name === name)
      const readAttribute = (entry: Entry, name: string) => {
        const prop = attribute(entry, name)
        return prop?.type === NodeTypes.ATTRIBUTE ? prop.value?.content ?? '' : null
      }
      const hasClass = (entry: Entry, name: string) => readAttribute(entry, 'class')?.split(' ').includes(name)
      const wrap = (entry: Entry): Element => ({
        tagName: entry.node.tag.toUpperCase(),
        getAttribute: (name: string) => readAttribute(entry, name),
        setAttribute: (name: string, value: string) => {
          const prop = attribute(entry, name)
          if (prop?.type !== NodeTypes.ATTRIBUTE || !prop.value) throw new Error('fixture attribute missing')
          edits.push({ start: prop.value.loc.start.offset, end: prop.value.loc.end.offset, value: JSON.stringify(value) })
        },
        closest: (selector: string) => {
          if (selector !== '.vditor-ir__node[data-type="img"]') throw new Error('unsupported fixture ancestor selector')
          let parent: Entry | undefined = entry
          while (parent) {
            if (hasClass(parent, 'vditor-ir__node') && readAttribute(parent, 'data-type') === 'img') return wrap(parent)
            parent = parent.parent
          }
          return null
        },
        querySelector: (selector: string) => {
          if (selector !== '.vditor-ir__marker--link') throw new Error('unsupported fixture destination selector')
          const marker = elements.find((child) => child.parent === entry && hasClass(child, 'vditor-ir__marker--link'))
          return marker ? wrap(marker) : null
        },
        get textContent() {
          return entry.node.children.map((child) => child.type === NodeTypes.TEXT ? child.content : '').join('')
        },
        set textContent(value: string) {
          const source = entry.node.loc.source
          edits.push({ start: entry.node.loc.start.offset + source.indexOf('>') + 1, end: entry.node.loc.start.offset + source.lastIndexOf('</'), value })
        },
      } as unknown as Element)
      return {
        set innerHTML(value: string) {
          html = value
          edits = []
          elements = []
          const visit = (children: TemplateChildNode[], parent?: Entry) => {
            for (const node of children) {
              if (node.type !== NodeTypes.ELEMENT) continue
              const entry: Entry = { node, parent }
              elements.push(entry)
              visit(node.children, entry)
            }
          }
          visit(parse(value).children)
        },
        get innerHTML() {
          return edits.sort((a, b) => b.start - a.start).reduce((result, edit) => result.slice(0, edit.start) + edit.value + result.slice(edit.end), html)
        },
        content: { querySelectorAll: (selector: string) => {
          if (selector === 'img, a') return elements.filter((entry) => entry.node.tag === 'img' || entry.node.tag === 'a').map(wrap)
          if (selector === '.vditor-ir__marker--link') return elements.filter((entry) => hasClass(entry, 'vditor-ir__marker--link')).map(wrap)
          throw new Error('unsupported fixture resource selector')
        } },
      }
    },
  } as unknown as Document
}

it('切引擎前捕获尚未触发input回调的最新文本，保存失败保留稿并允许重试', async () => {
  let failWrite!: (error: unknown) => void
  mocks.invoke.mockReturnValueOnce(new Promise<void>((_, reject) => { failWrite = reject }))
  mocks.invoke.mockResolvedValueOnce(undefined)
  const { app, state, change, progress, exposed } = mountEditor()
  try {
    state.editor.vditor.ir.element.scrollTop = 550
    expect(exposed.getCurrentPosition?.()).toBe('vditor:0.55')
    // Vditor 的 input 回调有 undoDelay，路由守卫须直接读取编辑器当前内容。
    mocks.source = '# 最后一次输入'
    const leaving = mocks.update?.()
    expect(progress).toHaveBeenLastCalledWith('vditor:0.55', 41)
    // 无条件屏障先让出一个微任务，之后才读取编辑器正文并写盘。
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.invoke).toHaveBeenCalledTimes(1)
    expect(change).toHaveBeenLastCalledWith('# 最后一次输入')
    expect(state.saveState).toBe('saving')
    failWrite({ code: 'permission_denied' })
    expect(await leaving).toBe(false)
    await nextTick()
    expect(state.saveState).toBe('error')
    expect(mocks.source).toBe('# 最后一次输入')

    expect(await mocks.leave?.()).toBe(true)
    expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.SAVE_VERSION, {
      itemId: 41, content: '# 最后一次输入', label: null, parentId: null,
      target: 'overwrite', source: 'user',
    })
    expect(state.saveState).toBe('saved')
  } finally {
    app.unmount()
  }
})

it('离开等待保存期间继续输入时，末次防抖文本落盘前不放行', async () => {
  let finishFirst!: () => void
  let finishLatest!: () => void
  mocks.invoke.mockReturnValueOnce(new Promise<void>((resolve) => { finishFirst = resolve }))
  mocks.invoke.mockReturnValueOnce(new Promise<void>((resolve) => { finishLatest = resolve }))
  const { app, change } = mountEditor()
  try {
    mocks.source = '# 第一稿'
    let released = false
    const leaving = mocks.leave?.().then((result) => { released = true; return result })
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.invoke).toHaveBeenCalledTimes(1)

    // 保存期间仍可编辑；第三方 input 回调尚未发出，编辑器 DOM 已是更新稿。
    mocks.source = '# 等待期间的末次输入'
    finishFirst()
    await vi.advanceTimersByTimeAsync(0)
    expect(released).toBe(false)
    expect(change).toHaveBeenLastCalledWith('# 等待期间的末次输入')
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, IPC.SAVE_VERSION, {
      itemId: 41, content: '# 等待期间的末次输入', label: null, parentId: null,
      target: 'overwrite', source: 'user',
    })
    finishLatest()
    expect(await leaving).toBe(true)
  } finally {
    app.unmount()
  }
})

it('图片粘贴尚在读取时，保存与离开先等待输入落定，图片进入正文后才落盘', async () => {
  mocks.invoke.mockResolvedValue(undefined)
  const { app, change } = mountEditor()
  try {
    const controller = paste.created[paste.created.length - 1]!
    let settleInput!: (result: 'idle' | 'settled' | 'failed') => void
    controller.waitForPendingInputs.mockReturnValueOnce(new Promise<'idle' | 'settled' | 'failed'>((resolve) => { settleInput = resolve }))

    const leaving = mocks.leave!()
    await vi.advanceTimersByTimeAsync(0)
    // 读取未完成期间不能以旧正文保存并放行导航。
    expect(mocks.invoke).not.toHaveBeenCalled()
    expect(controller.waitForPendingInputs).toHaveBeenCalledTimes(1)

    mocks.source = '# 原文\n\n![图片](data:image/png;base64,QUJD)'
    settleInput('settled')
    expect(await leaving).toBe(true)
    expect(change).toHaveBeenLastCalledWith('# 原文\n\n![图片](data:image/png;base64,QUJD)')
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith(IPC.SAVE_VERSION, {
      itemId: 41, content: '# 原文\n\n![图片](data:image/png;base64,QUJD)', label: null, parentId: null,
      target: 'overwrite', source: 'user',
    })
  } finally {
    app.unmount()
  }
})

it('图片读取失败时本轮保存与离开被阻止，提示重试后再次保存可放行', async () => {
  mocks.invoke.mockResolvedValue(undefined)
  const { app } = mountEditor()
  try {
    const controller = paste.created[paste.created.length - 1]!
    controller.waitForPendingInputs.mockResolvedValueOnce('failed')

    expect(await mocks.leave!()).toBe(false)
    expect(mocks.invoke).not.toHaveBeenCalled()

    // 控制器在读取失败或内容冲突时通过重试回调给出明确反馈。
    controller.onRetry()
    expect(mocks.toast).toHaveBeenCalledWith('warning', 'doc.editorImagePasteRetry')

    // 本轮之后已无在途输入，允许重新保存与离开，且不写入变化前的旧稿。
    expect(await mocks.leave!()).toBe(true)
    expect(mocks.invoke).not.toHaveBeenCalled()
  } finally {
    app.unmount()
  }
})

it('写盘期间图片读取失败时本轮导航仍被阻止', async () => {
  mocks.invoke.mockResolvedValue(undefined)
  const { app } = mountEditor()
  try {
    const controller = paste.created[paste.created.length - 1]!
    mocks.source = '# 已写盘的正文'
    // 顶部屏障无在途输入；写盘期间新接收的图片快速失败，由放行前的复核报告。
    controller.waitForPendingInputs.mockResolvedValueOnce('idle').mockResolvedValueOnce('failed')

    expect(await mocks.leave!()).toBe(false)
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith(IPC.SAVE_VERSION, {
      itemId: 41, content: '# 已写盘的正文', label: null, parentId: null,
      target: 'overwrite', source: 'user',
    })

    // 失败结果消费后不再无限阻止；已保存且无新输入时可放行。
    expect(await mocks.leave!()).toBe(true)
  } finally {
    app.unmount()
  }
})

it('文内查找快捷键打开面板并循环定位，关闭时清理高亮且不写入文档', async () => {
  const highlights = new Map<string, unknown>()
  vi.stubGlobal('CSS', { highlights })
  vi.stubGlobal('Highlight', class extends Set<Range> {
    constructor(...ranges: Range[]) { super(ranges) }
  })
  const matches = [
    { getBoundingClientRect: () => ({ top: 100 }), cloneRange() { return { ...this } } },
    { getBoundingClientRect: () => ({ top: 300 }), cloneRange() { return { ...this } } },
  ] as unknown as Range[]
  mocks.findMatches.mockReturnValue(matches)
  const { app, state, exposed, panelOpen } = mountEditor()
  try {
    expect(exposed.toggleSearch).toBeTypeOf('function')
    const preventDefault = vi.fn()
    state.handleKeyDown({ key: 'f', ctrlKey: true, preventDefault, stopImmediatePropagation: vi.fn() } as unknown as KeyboardEvent)
    expect(preventDefault).toHaveBeenCalledTimes(1)
    expect(state.searchOpen).toBe(true)
    expect(panelOpen).toHaveBeenCalledTimes(1)
    state.searchQuery = '原文'
    await nextTick()
    await vi.advanceTimersByTimeAsync(150)
    expect(state.searchMatches).toHaveLength(2)
    expect(state.searchIndex).toBe(0)
    state.moveSearch(-1)
    expect(state.searchIndex).toBe(1)
    expect(mocks.scrollBy).toHaveBeenNthCalledWith(1, { top: -10 })
    expect(mocks.scrollBy).toHaveBeenNthCalledWith(2, { top: 190 })
    expect(mocks.findMatches).toHaveBeenCalledTimes(1)
    expect(highlights.size).toBe(2)
    expect(exposed.closePanels?.()).toBe(true)
    expect(state.searchOpen).toBe(false)
    expect(highlights.size).toBe(0)
    expect(exposed.closePanels?.()).toBe(false)
    expect(panelOpen).toHaveBeenCalledTimes(1)
    expect(mocks.source).toBe('# 原文')
    expect(mocks.invoke).not.toHaveBeenCalled()
  } finally {
    app.unmount()
  }
})
