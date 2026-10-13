import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, ssrContextKey, type App } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import CollectionsView from './CollectionsView.vue'
import { IPC } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'

vi.mock('../utils/ipc', () => ({ invokeIpc: vi.fn(), ipcErrorMessage: String }))
vi.mock('../utils/logger', () => ({ logger: { error: vi.fn() } }))
vi.mock('../stores/viewStore', () => ({ useViewStore: () => ({}) }))
vi.mock('../stores/toastStore', () => ({ useToastStore: () => ({ addToast: vi.fn() }) }))
vi.mock('../stores/historyStore', () => ({ useHistoryStore: () => ({}) }))
vi.mock('../stores/exportStore', () => ({ useExportStore: () => ({}) }))
vi.mock('../composables/useConfirm', () => ({ useConfirm: () => ({ confirm: vi.fn() }) }))
vi.mock('../composables/useExportEntries', () => ({ useExportEntries: () => ({}) }))
vi.mock('../components/common/ContextMenu.vue', () => ({ default: { render: () => null } }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

// 保留页面回调及真实 collectionStore 的失败返回约定，所有数据库操作均由 IPC mock 代替。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

interface CollectionsState {
  creating: boolean
  newName: string
  startCreate: () => void
  submitCreate: () => Promise<void>
  cancelCreate: () => void
}

let app: App | null = null
const createCollection = vi.fn<(name: string) => Promise<number>>()
beforeEach(() => {
  setActivePinia(createPinia())
  createCollection.mockReset()
  vi.mocked(invokeIpc).mockReset()
  vi.mocked(invokeIpc).mockImplementation(async (command, args) => {
    if (command === IPC.CREATE_COLLECTION) return createCollection(args?.name as string)
    if (command === IPC.LIST_COLLECTIONS || command === IPC.LIST_DELETED_COLLECTIONS) return []
    throw new Error(`未模拟的 IPC: ${command}`)
  })
})
afterEach(() => { app?.unmount(); app = null })

function mountCollections(): CollectionsState {
  app = renderer.createApp({ ...CollectionsView, render: () => null })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  return (mounted.$ as unknown as { setupState: CollectionsState }).setupState
}

function renderState(state: CollectionsState) {
  return renderToString(createSSRApp({ ...CollectionsView, setup: () => state }))
}

describe('收藏夹新建失败恢复', () => {
  it('创建失败保留名称和编辑入口，可直接重试成功', async () => {
    createCollection.mockRejectedValueOnce(new Error('database busy')).mockResolvedValueOnce(7)
    const state = mountCollections()
    state.startCreate()
    state.newName = '旅行照片'
    await state.submitCreate()

    expect(state.creating).toBe(true)
    expect(state.newName).toBe('旅行照片')
    const failedHtml = await renderState(state)
    expect(failedHtml).toContain('common.retry')
    expect(failedHtml.match(/<input[^>]*>/)?.[0]).not.toContain('disabled')

    await state.submitCreate()
    expect(createCollection.mock.calls).toEqual([['旅行照片'], ['旅行照片']])
    expect(state.creating).toBe(false)
    expect(await renderState(state)).not.toContain('common.retry')
  })

  it('创建在途保留并禁用输入，Enter/blur 和取消不会造成重复提交或丢弃状态', async () => {
    let resolveCreate!: (id: number) => void
    createCollection.mockImplementationOnce(() => new Promise((resolve) => { resolveCreate = resolve }))
    const state = mountCollections()
    state.startCreate()
    state.newName = '旅行照片'
    const saving = state.submitCreate()
    await state.submitCreate()
    state.cancelCreate()
    const pendingHtml = await renderState(state)
    resolveCreate(7)
    await saving

    expect(pendingHtml.match(/<input[^>]*>/)?.[0] ?? '').toContain('disabled')
    expect(createCollection).toHaveBeenCalledTimes(1)
    expect(state.creating).toBe(false)
  })
})
