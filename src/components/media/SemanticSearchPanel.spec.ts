import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, reactive } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import SemanticSearchPanel from './SemanticSearchPanel.vue'
import { useAiStore } from '../../stores/aiStore'
import { IPC } from '../../constants/ipc'

const invokeIpc = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>()
const ui = reactive({
  groupBy: 'date',
  sortWithinGroup: 'datetime',
  searchQuery: '',
  setGroupBy(value: string) { this.groupBy = value },
  setSortWithinGroup(value: string) { this.sortWithinGroup = value },
})
// 实际 mediaStore 的失效操作保留旧布局，失败查询也不会发起新布局计算。
const media = reactive({
  layoutSummary: { totalItems: 4 },
  invalidateViewOrder: vi.fn(),
  invalidateLayout: vi.fn(),
})
vi.mock('../../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../../stores/mediaStore', () => ({ useMediaStore: () => media }))
vi.mock('../../utils/ipc', () => ({
  invokeIpc: (command: string, args?: Record<string, unknown>) => invokeIpc(command, args),
  ipcErrorMessage: (error: Error) => error.message,
  parseAppError: vi.fn(),
}))
vi.mock('../../utils/logger', () => ({ logger: { error: vi.fn() } }))
vi.mock('../../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('../../composables/useConfirm', () => ({ useConfirm: () => ({ confirm: vi.fn() }) }))
vi.mock('../../composables/useAnalysisController', () => ({ useAnalysisController: () => ({}) }))

beforeEach(() => {
  setActivePinia(createPinia())
  Object.assign(ui, { groupBy: 'date', sortWithinGroup: 'datetime', searchQuery: '' })
  invokeIpc.mockReset()
  invokeIpc.mockImplementation(async (command) => {
    if (command === IPC.CLEAR_SEMANTIC_SEARCH) return undefined
    if (command === IPC.SEMANTIC_SEARCH_CMD) return 4
    throw new Error(`Unexpected command: ${command}`)
  })
})

async function renderPanel() {
  const app = createSSRApp(SemanticSearchPanel)
  app.config.globalProperties.$t = (key: string) => key
  return renderToString(app)
}

describe('语义搜索失败时的结果遮罩', () => {
  it.each(['semantic', 'mixed'] as const)(
    '%s 模式的后续查询失败时遮住旧图库，重试成功后才重新展示结果',
    async (mode) => {
      const ai = useAiStore()
      ai.setSearchMode(mode)
      await ai.runSemanticSearch('海边')
      expect(await renderPanel()).toContain('semantic-panel--has-results')

      let rejectSearch!: (error: Error) => void
      invokeIpc.mockReturnValueOnce(new Promise<unknown>((_, reject) => { rejectSearch = reject }))
      const pending = ai.runSemanticSearch('雪山')
      const loadingHtml = await renderPanel()
      expect(loadingHtml).toContain('semantic-panel__loading')
      expect(loadingHtml).not.toContain('semantic-panel--has-results')

      rejectSearch(new Error('搜索执行失败'))
      await pending
      expect(ai.semanticQuery).toBe('雪山')
      expect(ai.isSearching).toBe(false)
      expect(media.layoutSummary.totalItems).toBe(4)
      const failedHtml = await renderPanel()
      expect(failedHtml).toContain('搜索执行失败')
      expect(failedHtml).not.toContain('semantic-panel--has-results')
      expect(failedHtml).not.toContain('semantic-panel__results-count')

      await ai.runSemanticSearch('雪山')
      const recoveredHtml = await renderPanel()
      expect(recoveredHtml).not.toContain('搜索执行失败')
      expect(recoveredHtml).toContain('semantic-panel--has-results')
    },
  )
})
