import { setImmediate as flushAsync } from 'node:timers/promises'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, ssrContextKey, type App } from 'vue'
import PluginStoreView from './PluginStoreView.vue'
import { IPC } from '../constants/ipc'
import type { RegistrySummary } from '../types/exotic'
import type { useExoticStore } from '../composables/useExoticStore'

const invokeIpc = vi.fn<(command: string) => Promise<unknown>>()
const addToast = vi.fn()

vi.mock('../utils/ipc', () => ({
  invokeIpc: (command: string) => invokeIpc(command),
}))
vi.mock('../composables/useTauriListen', () => ({ useTauriListen: vi.fn() }))
vi.mock('../composables/useOcr', () => ({ resetOcrStatusCache: vi.fn() }))
vi.mock('../composables/useConfirm', () => ({ useConfirm: () => ({ confirm: vi.fn() }) }))
vi.mock('../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('../components/exotic/OfficialFeaturesPanel.vue', () => ({
  default: { render: () => null },
}))
vi.mock('../components/exotic/FeatureActivationDialog.vue', () => ({
  default: { render: () => null },
}))
vi.mock('@lucide/vue', () => {
  const icon = { render: () => null }
  return {
    RefreshCw: icon, Puzzle: icon, Download: icon, ArrowUpCircle: icon, Wrench: icon,
    KeyRound: icon, Trash2: icon, PackageOpen: icon, Play: icon, Pause: icon, Square: icon,
    CheckCircle2: icon, ChevronDown: icon, ChevronUp: icon, X: icon,
  }
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

// 运行真实挂载流程、商店数据层和按钮回调；所有 IPC 均替换，不访问网络或插件目录。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

interface StoreViewState {
  store: ReturnType<typeof useExoticStore>
  refreshing: boolean
  onRefresh: () => Promise<void>
}

let app: App | null = null
const summary: RegistrySummary = { pluginCount: 0, sequence: 1, expired: false }

function mountStore(): StoreViewState {
  app = renderer.createApp({ ...PluginStoreView, render: () => null })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  return (mounted.$ as unknown as { setupState: StoreViewState }).setupState
}

function cachedResponse(command: string): Promise<unknown> {
  switch (command) {
    case IPC.LIST_EXOTIC_REGISTRY:
    case IPC.LIST_INSTALLED_EXOTIC_PLUGINS:
      return Promise.resolve([])
    case IPC.GET_EXOTIC_PROCESSING_STATUS:
      return Promise.resolve({
        pending: 0, processing: 0, done: 0, error: 0,
        blockedByAvailability: 0, running: false, paused: false,
      })
    case IPC.FETCH_EXOTIC_REGISTRY:
      return Promise.resolve(summary)
    default:
      throw new Error(`未模拟的 IPC: ${command}`)
  }
}

beforeEach(() => { invokeIpc.mockReset(); addToast.mockReset() })
afterEach(() => { app?.unmount(); app = null })

describe('插件商店目录刷新互斥', () => {
  it('自动刷新期间阻止手动重复请求，自动失败后允许手动重试并清除错误', async () => {
    const automatic = deferred<RegistrySummary>()
    let fetchCount = 0
    invokeIpc.mockImplementation((command) => {
      if (command === IPC.FETCH_EXOTIC_REGISTRY && ++fetchCount === 1) return automatic.promise
      return cachedResponse(command)
    })
    const state = mountStore()
    await flushAsync()
    const busyDuringAutomatic = state.refreshing
    await state.onRefresh()
    const requestsDuringAutomatic = fetchCount
    const failure = { code: 'http', message: '目录暂时不可用' }
    automatic.reject(failure)
    await flushAsync()

    expect(requestsDuringAutomatic).toBe(1)
    expect(busyDuringAutomatic).toBe(true)
    expect(state.refreshing).toBe(false)
    expect(state.store.error.value).toEqual(failure)
    expect(addToast).not.toHaveBeenCalled()

    await state.onRefresh()
    expect(fetchCount).toBe(2)
    expect(state.store.error.value).toBeNull()
    expect(state.refreshing).toBe(false)
    expect(addToast).toHaveBeenCalledWith('success', 'exotic.storeRefreshed')
  })

  it('首载未结束时已开始手动刷新，首载返回空目录后不再并发自动刷新', async () => {
    const initialRegistry = deferred<unknown[]>()
    const manual = deferred<RegistrySummary>()
    let registryReads = 0
    let fetchCount = 0
    invokeIpc.mockImplementation((command) => {
      if (command === IPC.LIST_EXOTIC_REGISTRY && ++registryReads === 1) return initialRegistry.promise
      if (command === IPC.FETCH_EXOTIC_REGISTRY && ++fetchCount === 1) return manual.promise
      return cachedResponse(command)
    })
    const state = mountStore()
    const refreshing = state.onRefresh()
    initialRegistry.resolve([])
    await flushAsync()
    const requestsDuringManual = fetchCount
    const busyDuringManual = state.refreshing
    manual.resolve(summary)
    await refreshing

    expect(requestsDuringManual).toBe(1)
    expect(busyDuringManual).toBe(true)
    expect(state.refreshing).toBe(false)
    expect(state.store.error.value).toBeNull()
  })
})
