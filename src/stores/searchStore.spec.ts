import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, reactive } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { useSearchStore } from './searchStore'
import { useAiStore } from './aiStore'
import { useJustifiedLayout } from '../composables/useJustifiedLayout'
import { decodeSearch, encodeSearch } from '../utils/searchUrl'
import { IPC } from '../constants/ipc'

const ui = reactive({
  searchQuery: '',
  searchScope: 'filename',
  groupBy: 'date',
  sortWithinGroup: 'datetime',
  sortOrder: 'desc',
  layoutMode: 'justified',
  gridRowHeight: 200,
  seamlessGroups: false,
  setGroupBy(value: string) { this.groupBy = value },
  setSortWithinGroup(value: string) { this.sortWithinGroup = value },
})
const computeLayout = vi.fn(async (_params: { filters?: Record<string, unknown> }) => 'committed')
vi.mock('./uiStore', () => ({ useUiStore: () => ui }))
vi.mock('./mediaStore', () => ({ useMediaStore: () => ({
  computeLayout,
  layoutDirtyRevision: 0,
  consumeLayoutDirty: vi.fn(),
  invalidateViewOrder: vi.fn(),
  invalidateLayout: vi.fn(),
}) }))
vi.mock('./viewStore', () => ({ useViewStore: () => ({
  galleryQueryReady: true,
  activeSmartAlbum: 'all',
  activeDirectoryId: null,
  activeCollection: null,
  activePersonId: null,
}) }))
vi.mock('./scanStore', () => ({ useScanStore: () => ({ isAnyScanRunning: false }) }))
vi.mock('./duplicateLensStore', () => ({ useDuplicateLensStore: () => ({ mode: null }) }))
vi.mock('../utils/ipc', () => ({
  invokeIpc: async (command: string) => {
    if (command === IPC.CLEAR_SEMANTIC_SEARCH) return undefined
    throw new Error(`Unexpected command: ${command}`)
  },
  ipcErrorMessage: String,
  parseAppError: vi.fn(),
}))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn() } }))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('../composables/useAnalysisController', () => ({ useAnalysisController: () => ({}) }))

beforeEach(() => {
  setActivePinia(createPinia())
  Object.assign(ui, {
    searchQuery: '', searchScope: 'filename', groupBy: 'date', sortWithinGroup: 'datetime',
  })
  computeLayout.mockClear()
})

async function layoutFilters() {
  // 使用真实布局条件组装，仅替代最终读取，避免在测试里复制搜索范围投影。
  const scope = effectScope()
  const layout = scope.run(() => useJustifiedLayout(() => 800))!
  try {
    await layout.compute()
    return computeLayout.mock.calls[computeLayout.mock.calls.length - 1]?.[0].filters
  } finally {
    scope.stop()
  }
}

describe('混合模式文件名候选的搜索范围', () => {
  it('普通模式按文件夹搜索后切到混合模式，文件名候选不继承隐藏的文件夹范围', async () => {
    const search = useSearchStore()
    const ai = useAiStore()
    search.setMode('normal')
    search.setScope('folder')
    search.commitNormal('旅行')
    expect(await layoutFilters()).toMatchObject({ searchQuery: '旅行', searchScope: 'folder' })

    // 工具栏直接经 aiStore 切换模式，再由门面提交用户选中的文件名候选。
    ai.toggleSearchMode()
    expect(search.mode).toBe('mixed')
    search.commitMixed(1, 'IMG_001')
    expect(await layoutFilters()).toMatchObject({ searchQuery: 'IMG_001', searchScope: 'filename' })
    expect(ai.activeMixedQueryType).toBe('normal')
    expect(search.committedQuery).toBe('IMG_001')
  })

  it('恢复带旧范围的混合文件名 URL 后，实际查询与重新序列化的范围均为文件名', async () => {
    const search = useSearchStore()
    const saved = decodeSearch({ mode: 'mixed', scope: 'location', mixed: 'normal', q: 'IMG_002' })
    // 与 useGalleryQuerySync 相同的恢复顺序：模式、范围、查询。
    search.setMode(saved.mode!)
    search.setScope(saved.scope!)
    search.apply(saved.query!, { mixedIndex: saved.mixedIndex })

    expect(await layoutFilters()).toMatchObject({ searchQuery: 'IMG_002', searchScope: 'filename' })
    expect(encodeSearch({
      mode: search.mode, scope: search.scope, query: search.committedQuery, mixedIndex: 1,
    })).toEqual({ q: 'IMG_002', mixed: 'normal' })
  })
})
