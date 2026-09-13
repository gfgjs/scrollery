import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, reactive, ref, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createI18n } from 'vue-i18n'
import zhCN from '../../i18n/locales/zh-CN'

const lens = reactive({ isLensActive: false, mode: null as 'groups' | 'folders' | null })
const ui = reactive({
  gallerySidebarVisible: true,
  groupBy: 'date',
  searchScope: 'filename',
  searchQuery: 'sunset',
  searchDebounceMs: 200,
  toggleGallerySidebar: () => {},
})
const ai = reactive({
  searchMode: 'normal',
  isSemanticMode: false,
  isSearching: false,
  semanticQuery: '',
  toggleSearchMode: () => {},
})
const search = reactive({
  draftMixedQuery: '',
  commitMixed: () => {},
  commitSemantic: () => {},
  commitNormal: (query: string) => {
    ui.searchQuery = query
  },
})

vi.mock('vue-router', () => ({ useRouter: () => ({ push: () => Promise.resolve() }) }))
vi.mock('../../composables/useToolbarOverflow', () => ({
  useToolbarOverflow: () => ({
    visibleCount: ref(99),
    hasOverflow: ref(false),
    isMeasuring: ref(false),
    isSettling: ref(false),
  }),
}))
vi.mock('../../composables/useToolbarAlign', () => ({
  useToolbarAlign: () => ({ align: ref('center') }),
}))
vi.mock('./filterChips.descriptors', () => ({
  chipStateOf: () => ({}),
  chipCountOf: () => 0,
  chipWidthKey: () => '',
}))
vi.mock('../../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../../stores/viewStore', () => ({
  useViewStore: () => ({ activeSmartAlbum: 'all' }),
}))
vi.mock('../../stores/filterStore', () => ({ useFilterStore: () => ({}) }))
vi.mock('../../stores/duplicateLensStore', () => ({ useDuplicateLensStore: () => lens }))
vi.mock('../../stores/mediaStore', () => ({
  useMediaStore: () => ({ stats: null, totalItems: 0, viewTotalItems: 0 }),
}))
vi.mock('../../stores/aiStore', () => ({ useAiStore: () => ai }))
vi.mock('../../stores/searchStore', () => ({ useSearchStore: () => search }))
vi.mock('../../commands/keybinding', () => ({ dispatchKeybinding: () => false }))
vi.mock('../../commands/context', () => ({ buildCommandContext: () => ({}) }))

vi.mock('./GalleryViewControls.vue', async () => {
  const { defineComponent: component, h: node } = await import('vue')
  return { default: component({ render: () => node('div') }) }
})
vi.mock('./GalleryFilterChips.vue', async () => {
  const { defineComponent: component, h: node } = await import('vue')
  return { default: component({ render: () => node('div') }) }
})
vi.mock('../ui/UiIconButton.vue', async () => {
  const { defineComponent: component, h: node } = await import('vue')
  return { default: component({ render: () => node('div') }) }
})
vi.mock('../ui/UiPopover.vue', async () => {
  const { defineComponent: component, h: node } = await import('vue')
  return { default: component({ render: () => node('div') }) }
})

const i18n = createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } })

async function render(active: boolean): Promise<string> {
  lens.isLensActive = active
  lens.mode = active ? 'groups' : null
  const { default: AppToolbar } = await import('./AppToolbar.vue')
  const app = createSSRApp({ render: () => h(AppToolbar as Component) })
  app.use(i18n)
  return renderToString(app)
}

describe('AppToolbar: 重复镜头搜索控件', () => {
  it('镜头内隐藏搜索簇，退出后恢复原搜索状态', async () => {
    vi.stubGlobal('localStorage', { getItem: () => null })

    const normal = await render(false)
    expect(normal).toContain('toolbar__search-wrap')
    expect(normal).toContain('value="sunset"')

    expect(await render(true)).not.toContain('toolbar__search-wrap')

    const restored = await render(false)
    expect(restored).toContain('toolbar__search-wrap')
    expect(restored).toContain('value="sunset"')
  })
})
