import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, ssrContextKey, type App } from 'vue'
import { createPinia, disposePinia, setActivePinia } from 'pinia'
import ViewerColorMenu from './ViewerColorMenu.vue'
import { useConfigStore } from '../../stores/configStore'
import {
  flushSettings,
  initializeSettings,
  resetSettingsModuleStateForTests,
} from '../../stores/settingsPersistence'
import { IPC } from '../../constants/ipc'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../../utils/ipc', () => ({ invokeIpc: mocks.invoke, ipcErrorMessage: String }))
vi.mock('../../utils/appEvents', () => ({ listenAppEvent: async () => () => {} }))
vi.mock('../../utils/logger', () => ({
  logger: { warn: vi.fn(), error: vi.fn() }, setLoggerEnabled: vi.fn(),
}))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('../ui/UiIconButton.vue', () => ({ default: { render: () => null } }))
vi.mock('../ui/UiPopover.vue', () => ({ default: { render: () => null } }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

interface MenuState {
  open: boolean
  selectCustom: (id: string) => Promise<void>
  selectFixed: (target: 'srgb' | 'display-p3' | 'dci-p3') => Promise<void>
}

const profileId = '0123456789abcdef'
let pinia: ReturnType<typeof createPinia>
let app: App | null = null
let menu: MenuState
let saved: Record<string, string>
let revision: number

beforeEach(async () => {
  resetSettingsModuleStateForTests()
  pinia = createPinia()
  setActivePinia(pinia)
  mocks.invoke.mockReset()
  saved = { viewer_color_target: 'srgb', viewer_color_custom_id: '' }
  revision = 1
  mocks.invoke.mockResolvedValueOnce({
    settings: { values: { ...saved }, revision, generation: 0 },
    state: { firstLaunch: null, guideSeen: null },
  })
  await initializeSettings()
  // 保留菜单真实点击回调和配置保存链，node 渲染器仅替代 DOM。
  app = renderer.createApp({ ...ViewerColorMenu, render: () => null })
  app.use(pinia)
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  menu = (mounted.$ as unknown as { setupState: MenuState }).setupState
  menu.open = true
})

afterEach(() => {
  app?.unmount()
  app = null
  disposePinia(pinia)
  resetSettingsModuleStateForTests()
})

describe('查看器色域菜单的保存顺序', () => {
  it('自定义 ICC 保存期间点击 Display P3，最终保留后一次选择', async () => {
    let finishFirst!: () => void
    const firstResponse = new Promise<void>((resolve) => { finishFirst = resolve })
    const patches: Record<string, string>[] = []
    mocks.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command !== IPC.SET_APP_SETTINGS) throw new Error(`Unexpected command: ${command}`)
      const patch = args?.patch as Record<string, string>
      patches.push(patch)
      if (patches.length === 1) await firstResponse
      saved = { ...saved, ...patch }
      return {
        snapshot: { values: { ...saved }, revision: ++revision, generation: 0 },
        keys: Object.keys(patch), restart_required: [], apply_failed: [],
      }
    })

    const selectingCustom = menu.selectCustom(profileId)
    expect(patches).toHaveLength(1)
    expect(menu.open).toBe(true)
    const selectingFixed = menu.selectFixed('display-p3')
    finishFirst()
    await Promise.all([selectingCustom, selectingFixed])
    await flushSettings()

    expect(useConfigStore().viewerColorTarget).toBe('display-p3')
    expect(saved.viewer_color_target).toBe('display-p3')
    expect(patches).toEqual([
      { viewer_color_custom_id: profileId, viewer_color_target: 'custom' },
      { viewer_color_target: 'display-p3' },
    ])
    expect(menu.open).toBe(false)
  })

  it('应用失败时保持原配置和打开的菜单，允许再次选择', async () => {
    mocks.invoke.mockRejectedValueOnce(new Error('配置写入失败'))
    await expect(menu.selectCustom(profileId)).rejects.toThrow('配置写入失败')

    expect(useConfigStore().viewerColorTarget).toBe('srgb')
    expect(useConfigStore().viewerColorCustomId).toBe('')
    expect(menu.open).toBe(true)
  })
})
