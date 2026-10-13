import { beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, ssrContextKey } from 'vue'
import VersionPanel from './VersionPanel.vue'

const { invoke, addToast } = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  addToast: vi.fn(),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc: invoke, ipcErrorMessage: String }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

// 挂载真实 setup、props 监听及卸载钩子，不依赖浏览器布局。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

function version(id: number, isCurrent: boolean) {
  return {
    id, itemId: 41, parentId: null, label: `版本 ${id}`, storage: 'snapshot',
    absPath: '', source: 'user', note: null, contentHash: null, isCurrent, createdAt: 1,
  }
}

function mountPanel() {
  const changed = vi.fn()
  const app = renderer.createApp(Object.assign({}, VersionPanel, { render: () => null }), {
    itemId: 41, onChanged: changed,
  })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  const state = (mounted.$ as unknown as {
    setupState: {
      versions: ReturnType<typeof version>[]
      currentId: number | null
      setCurrent(versionId: number | null): Promise<void>
      remove(versionId: number): Promise<void>
      reload(): Promise<boolean>
    }
  }).setupState
  return { app, state, changed }
}

beforeEach(() => vi.resetAllMocks())

it.each(['setCurrent', 'remove'] as const)('%s 成功后列表读取失败仍保留已确认版本状态，提示失败且正文继续刷新', async (method) => {
  const initial = [version(1, true), version(2, false)]
  const expected = method === 'setCurrent' ? [version(1, false), version(2, true)] : [version(2, false)]
  invoke.mockResolvedValueOnce(initial).mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('list read failed'))
  const { app, state, changed } = mountPanel()
  try {
    await vi.waitFor(() => expect(state.versions).toEqual(initial))
    await state[method](method === 'setCurrent' ? 2 : 1)
    expect(state.versions).toEqual(expected)
    expect(state.currentId).toBe(method === 'setCurrent' ? 2 : null)
    expect(addToast).toHaveBeenCalledExactlyOnceWith('error', 'doc.versionOpFailed')
    // 写操作已经成功；父层的 refreshText 不应被列表读取失败阻断。
    expect(changed).toHaveBeenCalledTimes(1)

    invoke.mockResolvedValueOnce(expected)
    expect(await state.reload()).toBe(true)
    expect(state.versions).toEqual(expected)
  } finally {
    app.unmount()
  }
})

it('较旧列表请求迟到失败不会覆盖新列表或弹出过期错误', async () => {
  invoke.mockResolvedValueOnce([version(1, true)])
  const { app, state } = mountPanel()
  let rejectOld!: (error: Error) => void
  const oldRead = new Promise<never>((_, reject) => { rejectOld = reject })
  try {
    await vi.waitFor(() => expect(state.versions).toHaveLength(1))
    invoke.mockReturnValueOnce(oldRead).mockResolvedValueOnce([version(2, true)])
    const oldReload = state.reload()
    expect(await state.reload()).toBe(true)
    rejectOld(new Error('old read failed'))
    expect(await oldReload).toBe(false)
    expect(state.versions).toEqual([version(2, true)])
    expect(addToast).not.toHaveBeenCalled()
  } finally {
    app.unmount()
  }
})
