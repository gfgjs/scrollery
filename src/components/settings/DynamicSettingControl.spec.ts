import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, nextTick, ssrContextKey, type App } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import DynamicSettingControl from './DynamicSettingControl.vue'
import { useConfigStore } from '../../stores/configStore'
import { settingsConfirmedValues, writeSettings } from '../../stores/settingsPersistence'

// 保留真实控件、设置范围和 configStore 的数值钳制，只替换持久化与无关 UI 服务。
vi.mock('../../stores/settingsPersistence', async () => {
  const { ref } = await import('vue')
  const values = ref<Record<string, string>>({})
  return {
    settingsConfirmedValues: values,
    settingsReady: ref(true),
    readSetting: (key: string) => values.value[key],
    writeSettings: vi.fn(async (patch: Record<string, string>) => {
      values.value = { ...values.value, ...patch }
    }),
    initializeSettings: vi.fn(),
    resetSettings: vi.fn(),
  }
})
vi.mock('../../stores/uiStore', () => ({ useUiStore: () => ({}) }))
vi.mock('../../stores/themeStore', () => ({ useThemeStore: () => ({}) }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast: vi.fn() }) }))
vi.mock('../../stores/mediaStore', () => ({ useMediaStore: () => ({}) }))
vi.mock('../../stores/scanStore', () => ({ useScanStore: () => ({}) }))
vi.mock('../../stores/aiStore', () => ({ useAiStore: () => ({ status: {} }) }))
vi.mock('../../composables/useConfirm', () => ({ useConfirm: () => ({ confirm: vi.fn() }) }))
vi.mock('../../composables/useRenderMode', () => ({ useRenderMode: () => ({}) }))
vi.mock('../../composables/useTitlebarMode', () => ({ useTitlebarMode: () => ({}) }))
vi.mock('../../composables/useSelectionBarMode', () => ({ useSelectionBarMode: () => ({}) }))
vi.mock('../../composables/useToolbarAlign', () => ({ useToolbarAlign: () => ({}) }))
vi.mock('../../composables/usePerformanceMonitor', () => ({
  usePerformanceMonitor: () => ({ openPanel: vi.fn() }),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc: vi.fn() }))
vi.mock('../../utils/logger', () => ({ logger: { error: vi.fn() }, setLoggerEnabled: vi.fn() }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

interface ControlState {
  numberLocal: number | string
  commitNumber: () => void
}

let app: App | null = null
beforeEach(() => {
  setActivePinia(createPinia())
  settingsConfirmedValues.value = { thumb_webp_quality: '100' }
  vi.mocked(writeSettings).mockClear()
})
afterEach(() => { app?.unmount(); app = null })

function mountQualityControl(): ControlState {
  app = renderer.createApp({ ...DynamicSettingControl, render: () => null }, {
    settingKey: 'thumbWebpQuality',
  })
  app.provide(ssrContextKey, {})
  const mounted = app.mount({})
  return (mounted.$ as unknown as { setupState: ControlState }).setupState
}

describe('数字设置提交后的显示值', () => {
  it('已处于上限时提交超限输入，显示值仍与实际保存值一致', async () => {
    const state = mountQualityControl()
    state.numberLocal = 150
    state.commitNumber()
    await nextTick()

    expect(writeSettings).toHaveBeenCalledWith({ thumb_webp_quality: '100' })
    expect(useConfigStore().thumbWebpQuality).toBe(100)
    expect(state.numberLocal).toBe(100)
  })

  it('清空数字输入后提交恢复当前值，不写入空字符串或 NaN', async () => {
    const state = mountQualityControl()
    state.numberLocal = ''
    state.commitNumber()
    await nextTick()

    expect(writeSettings).not.toHaveBeenCalled()
    expect(state.numberLocal).toBe(100)
    expect(useConfigStore().thumbWebpQuality).toBe(100)
  })
})
