import { afterEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, reactive, ref, ssrContextKey, type App } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import AppToolbar from './AppToolbar.vue'
import { useAiStore } from '../../stores/aiStore'
import { IPC } from '../../constants/ipc'

const invokeIpc = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>()
const ui = reactive({
  groupBy: 'date', sortWithinGroup: 'datetime', searchQuery: '', searchScope: 'filename', searchDebounceMs: 300,
  setGroupBy(value: string) { this.groupBy = value },
  setSortWithinGroup(value: string) { this.sortWithinGroup = value },
})
vi.mock('../../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../../stores/viewStore', () => ({ useViewStore: () => ({ activeSmartAlbum: 'all' }) }))
vi.mock('../../stores/filterStore', () => ({ useFilterStore: () => ({}) }))
vi.mock('../../stores/duplicateLensStore', () => ({ useDuplicateLensStore: () => ({ isLensActive: false }) }))
vi.mock('../../stores/mediaStore', () => ({ useMediaStore: () => ({
  invalidateViewOrder: vi.fn(), invalidateLayout: vi.fn(),
}) }))
vi.mock('../../utils/ipc', () => ({
  invokeIpc: (command: string, args?: Record<string, unknown>) => invokeIpc(command, args),
  ipcErrorMessage: String, parseAppError: vi.fn(),
}))
vi.mock('../../utils/logger', () => ({ logger: { error: vi.fn() } }))
vi.mock('../../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: ref('zh-CN') }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('../../commands/keybinding', () => ({ dispatchKeybinding: vi.fn() }))
vi.mock('../../commands/context', () => ({ buildCommandContext: vi.fn() }))
vi.mock('../../composables/useAnalysisController', () => ({ useAnalysisController: () => ({}) }))
vi.mock('../../composables/useToolbarOverflow', () => ({ useToolbarOverflow: () => ({
  visibleCount: ref(10), hasOverflow: ref(false), isMeasuring: ref(false), isSettling: ref(false),
}) }))
vi.mock('../../composables/useDemoPrivacy', () => ({ useDemoPrivacy: () => ({
  demoPrivacyEnabled: ref(false), demoPrivacySupported: ref(false), toggleDemoPrivacy: vi.fn(),
}) }))
vi.mock('../../composables/useToolbarAlign', () => ({ useToolbarAlign: () => ({ align: ref('center') }) }))
vi.mock('../ui/UiIconButton.vue', () => ({ default: { render: () => null } }))
vi.mock('../ui/UiPopover.vue', () => ({ default: { render: () => null } }))
vi.mock('./GalleryViewControls.vue', () => ({ default: { render: () => null } }))
vi.mock('./GalleryFilterChips.vue', () => ({ default: { render: () => null } }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})
interface ToolbarState {
  currentSearchQuery: string
  toggleSearchMode: () => void
  isMixedDropdownOpen: boolean
  mixedDropdownIndex: number
  onKeydownDown: (event: KeyboardEvent) => void
  onKeydownUp: (event: KeyboardEvent) => void
  onKeydownEnter: (event: KeyboardEvent) => void
  onEscape: (event: KeyboardEvent) => void
}
let app: App | null = null
afterEach(() => {
  app?.unmount()
  app = null
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('工具栏搜索防抖的模式归属', () => {
  it('输入后立即切换模式撤销旧延迟提交，新模式输入仍正常搜索', async () => {
    vi.useFakeTimers()
    vi.stubGlobal('document', new EventTarget())
    vi.stubGlobal('localStorage', { getItem: () => null })
    const pinia = createPinia()
    setActivePinia(pinia)
    invokeIpc.mockImplementation(async (command) => {
      if (command === IPC.SEMANTIC_SEARCH_CMD) return 3
      if (command === IPC.CLEAR_SEMANTIC_SEARCH) return undefined
      throw new Error(`Unexpected command: ${command}`)
    })
    // 保留真实工具栏输入 setter、切换回调及搜索门面/store，仅替代布局与外部 IPC。
    app = renderer.createApp({ ...AppToolbar, render: () => null })
    app.use(pinia)
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const toolbar = (mounted.$ as unknown as { setupState: ToolbarState }).setupState
    const ai = useAiStore()

    toolbar.currentSearchQuery = '旧查询'
    toolbar.toggleSearchMode()
    expect(ai.searchMode).toBe('semantic')
    expect(toolbar.currentSearchQuery).toBe('')
    await vi.advanceTimersByTimeAsync(ui.searchDebounceMs)
    expect(invokeIpc.mock.calls.filter(([command]) => command === IPC.SEMANTIC_SEARCH_CMD)).toEqual([])
    expect(toolbar.currentSearchQuery).toBe('')

    toolbar.currentSearchQuery = '新查询'
    await vi.advanceTimersByTimeAsync(ui.searchDebounceMs)
    expect(invokeIpc.mock.calls.filter(([command]) => command === IPC.SEMANTIC_SEARCH_CMD)).toEqual([
      [IPC.SEMANTIC_SEARCH_CMD, { query: '新查询', limit: 1000 }],
    ])
    expect(ai.semanticQuery).toBe('新查询')
    expect(ai.matchCount).toBe(3)
  })

})
