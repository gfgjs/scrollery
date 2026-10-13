import { afterEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { IPC } from '../constants/ipc'
import { useUiStore } from './uiStore'
import { useViewerStore } from './viewerStore'
import { initializeSettings, resetSettingsModuleStateForTests } from './settingsPersistence'
import { useMarkdownAutoSave } from '../composables/reader/useMarkdownAutoSave'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc: invoke }))
vi.mock('../utils/appEvents', () => ({ listenAppEvent: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('../utils/logger', () => ({
  logger: { warn: vi.fn(), error: vi.fn(), info: vi.fn() }, setLoggerEnabled: vi.fn(),
}))
vi.mock('../i18n', () => ({ default: createI18n({ legacy: false, locale: 'zh-CN', messages: {} }) }))

afterEach(() => {
  resetSettingsModuleStateForTests()
  invoke.mockReset()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

it('直接退出等待文档最新稿，写入失败保留窗口且重试成功后才退出', async () => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  vi.stubGlobal('document', { documentElement: { setAttribute: vi.fn(), style: { setProperty: vi.fn() } } })
  invoke.mockResolvedValueOnce({
    settings: { values: { close_behavior: 'exit' }, revision: 1, generation: 0 },
    state: { firstLaunch: null, guideSeen: null },
  })
  await initializeSettings()
  const ui = useUiStore()
  const editor = useMarkdownAutoSave({ itemId: 41, initialSource: '# 原文' })
  useViewerStore().populate({
    kind: 'markdown', mediaType: 'document', fileFormat: 'md', id: 41,
    path: null, title: '样例', immersive: false, fileInfo: null,
    api: { prepareClose: editor.saveNow },
  })
  invoke.mockClear()
  let failSave!: (error: unknown) => void
  invoke.mockImplementationOnce(() => new Promise<void>((_, reject) => { failSave = reject }))
  try {
    editor.updateSource('# 未保存稿')
    const closing = ui.requestAppClose()
    expect(invoke).toHaveBeenCalledWith(IPC.SAVE_VERSION, expect.objectContaining({ itemId: 41, content: '# 未保存稿' }))
    expect(invoke.mock.calls.some(([command]) => command === IPC.EXIT_APP)).toBe(false)
    failSave({ code: 'Io', message: 'disk unavailable' })
    await closing
    expect(editor.saveState.value).toBe('error')
    expect(invoke.mock.calls.some(([command]) => command === IPC.EXIT_APP)).toBe(false)

    invoke.mockResolvedValue(undefined)
    await ui.requestAppClose()
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([IPC.SAVE_VERSION, IPC.SAVE_VERSION, IPC.EXIT_APP])
    expect(editor.saveState.value).toBe('saved')
  } finally {
    editor.dispose()
    ui.$dispose()
  }
})
