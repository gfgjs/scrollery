import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, disposePinia, setActivePinia } from 'pinia'
import { useIccProfileManager } from './useIccProfileManager'
import { useConfigStore } from '../stores/configStore'
import {
  flushSettings,
  initializeSettings,
  resetSettingsModuleStateForTests,
} from '../stores/settingsPersistence'
import { IPC } from '../constants/ipc'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), toast: vi.fn(), open: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc: mocks.invoke, ipcErrorMessage: String }))
vi.mock('../utils/appEvents', () => ({ listenAppEvent: async () => () => {} }))
vi.mock('../utils/logger', () => ({
  logger: { warn: vi.fn(), error: vi.fn() }, setLoggerEnabled: vi.fn(),
}))
vi.mock('../stores/toastStore', () => ({ useToastStore: () => ({ addToast: mocks.toast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))

const profile = { id: '0123456789abcdef', name: '测试显示器', fileSizeBytes: 512 }
let pinia: ReturnType<typeof createPinia>
let saved: Record<string, string>
let revision: number

function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((res) => { resolve = res })
  return { promise, resolve }
}

function commit(patch: Record<string, string>) {
  saved = { ...saved, ...patch }
  return {
    snapshot: { values: { ...saved }, revision: ++revision, generation: 0 },
    keys: Object.keys(patch), restart_required: [], apply_failed: [],
  }
}

beforeEach(async () => {
  resetSettingsModuleStateForTests()
  pinia = createPinia()
  setActivePinia(pinia)
  mocks.invoke.mockReset()
  mocks.toast.mockReset()
  mocks.open.mockResolvedValue('C:/test-fixture/monitor.icc')
  saved = { viewer_color_target: 'srgb', viewer_color_custom_id: '' }
  revision = 1
  mocks.invoke.mockResolvedValueOnce({
    settings: { values: { ...saved }, revision, generation: 0 },
    state: { firstLaunch: null, guideSeen: null },
  })
  await initializeSettings()
})

afterEach(() => {
  disposePinia(pinia)
  resetSettingsModuleStateForTests()
})

describe('ICC 应用与后续色域选择的提交顺序', () => {
  it.each(['select', 'import'] as const)('%s 的设置保存尚未返回时改选 Display P3，不被旧 ICC 续体覆盖', async (action) => {
    const requested = deferred()
    const response = deferred()
    const patches: Record<string, string>[] = []
    mocks.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === IPC.IMPORT_ICC_PROFILE) return profile
      if (command === IPC.LIST_ICC_PROFILES) return [profile]
      if (command !== IPC.SET_APP_SETTINGS) throw new Error(`Unexpected command: ${command}`)
      const patch = args?.patch as Record<string, string>
      patches.push(patch)
      if (patches.length === 1) {
        requested.resolve()
        await response.promise
      }
      return commit(patch)
    })
    // 使用真实配置 store 和中央串行保存服务，只替代对话框、文件与设置 IPC。
    const manager = useIccProfileManager()
    const config = useConfigStore()
    const applying = action === 'select'
      ? manager.selectIccProfile(profile.id)
      : manager.importIccProfile()
    await requested.promise
    const newerChoice = config.setViewerColorTarget('display-p3')
    response.resolve()
    await Promise.all([applying, newerChoice])
    await flushSettings()

    expect(config.viewerColorTarget).toBe('display-p3')
    expect(saved.viewer_color_target).toBe('display-p3')
    expect(config.viewerColorCustomId).toBe(profile.id)
    expect(patches).toEqual([
      { viewer_color_custom_id: profile.id, viewer_color_target: 'custom' },
      { viewer_color_target: 'display-p3' },
    ])
  })

  it('导入成功但应用保存失败时保留列表、保持原设置并报告应用失败', async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === IPC.IMPORT_ICC_PROFILE) return profile
      if (command === IPC.LIST_ICC_PROFILES) return [profile]
      if (command === IPC.SET_APP_SETTINGS) throw new Error('配置写入失败')
      throw new Error(`Unexpected command: ${command}`)
    })
    const manager = useIccProfileManager()
    const config = useConfigStore()
    await manager.importIccProfile()

    expect(manager.iccProfiles.value).toEqual([profile])
    expect(config.viewerColorCustomId).toBe('')
    expect(config.viewerColorTarget).toBe('srgb')
    expect(mocks.toast).toHaveBeenCalledExactlyOnceWith('error', 'settings.viewerIccSetActiveFailed')
  })
})
