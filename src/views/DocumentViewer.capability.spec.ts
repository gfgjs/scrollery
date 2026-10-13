import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, nextTick, reactive, ref, ssrContextKey, type App } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { IPC } from '../constants/ipc'
import type { MarkdownEngine } from '../types/reader'
import DocumentViewer from './DocumentViewer.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), replace: vi.fn(), toast: vi.fn() }))
const route = reactive({ params: { id: '41' }, query: {} as Record<string, string>, fullPath: '/doc/41' })
vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ replace: mocks.replace }),
  onBeforeRouteLeave: vi.fn(), onBeforeRouteUpdate: vi.fn(),
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: ref('zh-CN') }) }))
vi.mock('../utils/ipc', () => ({ invokeIpc: mocks.invoke, ipcErrorMessage: String }))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => `asset://localhost/${path}` }))
vi.mock('../stores/toastStore', () => ({ useToastStore: () => ({ addToast: mocks.toast }) }))
vi.mock('../stores/uiStore', () => ({ useUiStore: () => ({}) }))
vi.mock('../stores/themeStore', () => ({ useThemeStore: () => ({ isDark: false }) }))
vi.mock('../stores/viewerStore', () => ({ useViewerStore: () => ({ setImmersive: vi.fn() }) }))
vi.mock('../composables/reader/useDocActiveViewerSync', () => ({ useDocActiveViewerSync: vi.fn() }))
vi.mock('../composables/useConfirm', () => ({ useConfirm: () => ({ confirm: vi.fn() }) }))
vi.mock('../stores/settingsPersistence', () => ({ readSetting: () => undefined, writeSettings: vi.fn() }))
vi.mock('../composables/useToolbarOverflow', () => ({ useToolbarOverflow: () => ({
  visibleCount: ref(0), hasOverflow: ref(true), isMeasuring: ref(false), isSettling: ref(false),
}) }))
vi.mock('../components/ui/UiPopover.vue', () => ({ __esModule: true, default: { template: '<div><slot /></div>' } }))
vi.mock('../components/doc/BookReader.vue', () => ({ __esModule: true, default: { template: '<article>readable-document</article>' } }))
vi.mock('../components/doc/MarkdownEditorDocumentModule.vue', () => ({ __esModule: true, default: { template: '<article>experimental-editor</article>' } }))
vi.mock('../components/doc/VditorDocumentModule.vue', () => ({ __esModule: true, default: { template: '<article>vditor-editor</article>' } }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText: () => {}, setElementText: () => {}, patchProp: () => {}, insert: () => {},
  remove: () => {}, parentNode: () => null, nextSibling: () => null,
})
interface ViewerState {
  selectedMarkdownEngine: MarkdownEngine
  bookMarkdownEngine: MarkdownEngine
  isNativeReading: boolean
  setMarkdownEngine: (engine: MarkdownEngine) => Promise<void>
  onBookPrefsChange: (prefs: { encoding: string; reflow: boolean; zhConvert: ''; theme: string }) => Promise<void>
}
let app: App | undefined
async function mountViewer() {
  app = renderer.createApp({ ...DocumentViewer, render: () => null })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  await new Promise(resolve => setImmediate(resolve))
  const state = (mounted.$ as unknown as { setupState: ViewerState }).setupState
  // 使用真实模板和已加载的响应式状态，异步正文组件只替换渲染边界。
  const html = () => renderToString(createSSRApp({ ...DocumentViewer, setup: () => state }))
  return { state, html }
}
function preferenceWrites() {
  return mocks.invoke.mock.calls.filter(([command]) => command === IPC.SET_READER_BOOK_PREFS)
}
beforeEach(() => {
  vi.resetAllMocks()
  route.query = {}
  vi.stubGlobal('EditContext', undefined)
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() })
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === IPC.GET_MEDIA_DETAIL) return { id: 41, fileFormat: 'md', fileName: 'book.md', absPath: 'C:/book.md' }
    if (command === IPC.GET_DOCUMENT_TEXT) return '# 正文'
    if (command === IPC.GET_EFFECTIVE_REPLACEMENTS) return []
    if (command === IPC.GET_READER_BOOK_PREFS) return JSON.stringify({ v: 1, markdownEngine: 'markdown-editor' })
    return null
  })
})
afterEach(() => {
  app?.unmount()
  app = undefined
  vi.unstubAllGlobals()
})

it('T1-01 缺少能力时旧偏好仍可阅读，两个菜单禁用实验项，修改主题保留原引擎偏好', async () => {
  const { state, html } = await mountViewer()
  expect(state.selectedMarkdownEngine).toBe('native')
  expect(state.isNativeReading).toBe(true)
  expect(state.bookMarkdownEngine).toBe('markdown-editor')
  expect(preferenceWrites()).toHaveLength(0)
  const rendered = await html()
  expect(rendered).toContain('readable-document')
  const options = rendered.match(/<option[^>]*value="markdown-editor"[^>]*>/g)
  expect(options).toHaveLength(2)
  expect(options?.every(option => /\bdisabled\b/.test(option) && option.includes('doc.markdownEditorUnavailable'))).toBe(true)
  await state.onBookPrefsChange({ encoding: '', reflow: false, zhConvert: '', theme: 'paper' })
  expect(JSON.parse(preferenceWrites()[0]![1].prefs)).toMatchObject({ markdownEngine: 'markdown-editor', theme: 'paper' })
})

it('T1-02 无能力深链临时回退，拒绝不可用选择且不写路由或偏好', async () => {
  route.query = { engine: 'markdown-editor' }
  const { state } = await mountViewer()
  expect(state.selectedMarkdownEngine).toBe('native')
  await state.setMarkdownEngine('markdown-editor')
  expect(mocks.replace).not.toHaveBeenCalled()
  expect(preferenceWrites()).toHaveLength(0)
  expect(route.query.engine).toBe('markdown-editor')
})

it('T1-03 有能力时恢复原引擎且两个菜单保持可选', async () => {
  vi.stubGlobal('EditContext', class {})
  const { state, html } = await mountViewer()
  expect(state.selectedMarkdownEngine).toBe('markdown-editor')
  const rendered = await html()
  expect(rendered).toContain('experimental-editor')
  const options = rendered.match(/<option[^>]*value="markdown-editor"[^>]*>/g)
  expect(options).toHaveLength(2)
  expect(options?.every(option => !/\bdisabled\b/.test(option))).toBe(true)
  expect(preferenceWrites()).toHaveLength(0)
})

it('T1-04 切换引擎等待路由保存屏障，取消不写偏好，放行后才保存选择', async () => {
  vi.stubGlobal('EditContext', class {})
  const { state } = await mountViewer()
  let finish!: (result: object | undefined) => void
  mocks.replace.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const blocked = state.setMarkdownEngine('vditor')
  expect(state.selectedMarkdownEngine).toBe('markdown-editor')
  expect(preferenceWrites()).toHaveLength(0)
  finish({ type: 'aborted' })
  await blocked
  expect(state.bookMarkdownEngine).toBe('markdown-editor')
  expect(preferenceWrites()).toHaveLength(0)
  mocks.replace.mockImplementationOnce(async ({ query }: { query: Record<string, string> }) => {
    route.query = query
    await nextTick()
  })
  await state.setMarkdownEngine('vditor')
  expect(state.selectedMarkdownEngine).toBe('vditor')
  expect(JSON.parse(preferenceWrites()[0]![1].prefs).markdownEngine).toBe('vditor')
})
