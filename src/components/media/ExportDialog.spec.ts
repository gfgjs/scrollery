import { afterEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, nextTick, ssrContextKey, type App } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { open } from '@tauri-apps/plugin-dialog'
import ExportDialog from './ExportDialog.vue'
import { useExportStore, type ExportPreflight } from '../../stores/exportStore'
import { IpcError } from '../../utils/ipc'
import type { BackendSelectionDescriptor } from '../../composables/useSelection'

vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('../../utils/ipc', () => ({
  invokeIpc: vi.fn(),
  IpcError: class extends Error {
    constructor(readonly code: string, message: string) { super(message) }
  },
  ipcErrorMessage: (error: Error) => error.message,
}))
vi.mock('../../stores/settingsPersistence', () => ({ writeSettings: vi.fn() }))
vi.mock('../../composables/settingsValues', () => ({ readSettingBool: () => false }))
vi.mock('../../utils/format', () => ({ formatFileSize: (size: number) => String(size) }))
vi.mock('../ui/UiDialog.vue', () => ({ default: {} }))
vi.mock('../ui/UiField.vue', () => ({ default: {} }))
vi.mock('../ui/UiSelect.vue', () => ({ default: {} }))
vi.mock('../ui/UiCheckbox.vue', () => ({ default: {} }))
vi.mock('../ui/UiButton.vue', () => ({ default: {} }))

// 保留真实组件 setup、watch 和 Pinia，node 环境只替代模板挂载。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

interface DialogState {
  targetParent: string
  errorMessage: string
  starting: boolean
  canStart: boolean
  preflight: ExportPreflight | null
  pickTargetFolder: () => Promise<void>
  start: () => Promise<void>
}

function selection(layoutVersion: number): BackendSelectionDescriptor {
  return {
    kind: 'selectAll', excludedIds: [],
    view: {
      scope: { kind: 'all' }, filter: {}, layoutVersion,
      sort: { groupBy: 'none', sortWithinGroup: 'name', sortOrder: 'asc' },
    },
  }
}

let app: App | null = null
afterEach(() => {
  app?.unmount()
  app = null
  vi.restoreAllMocks()
})

describe('ExportDialog 视图过期确认', () => {
  it('选目录预检遇到视图过期时重建原入口集合，确认新摘要后才能启动', async () => {
    setActivePinia(createPinia())
    const store = useExportStore()
    const original = selection(1)
    const fresh = selection(2)
    const source = { kind: 'view' as const, name: 'all files' }
    const rebuild = vi.fn(() => fresh)
    const initialPreflight: ExportPreflight = {
      count: 2, estimatedBytes: 200, offlineOrMissingCount: 0, nonZeroRotationCount: 0,
      targetWritable: false, insideLibraryWarning: false,
    }
    const preflight = vi.spyOn(store, 'preflightExport').mockResolvedValue(initialPreflight)
    const startExport = vi.spyOn(store, 'startExport').mockResolvedValue('new-export')
    app = renderer.createApp({ ...ExportDialog, render: () => null })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: DialogState }).setupState
    store.openExportDialog(original, source, rebuild)
    await new Promise<void>((resolve) => setImmediate(resolve))

    let finishPreflight!: (value: ExportPreflight) => void
    preflight.mockRejectedValueOnce(new IpcError('ViewStale', 'layout changed'))
      .mockReturnValueOnce(new Promise((resolve) => { finishPreflight = resolve }))
    vi.mocked(open).mockResolvedValueOnce('C:/export-target')
    const picking = state.pickTargetFolder()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(rebuild).toHaveBeenCalledTimes(1)
    expect(preflight).toHaveBeenLastCalledWith(expect.objectContaining({
      selection: fresh, source, targetParent: 'C:/export-target',
    }))
    expect(state.preflight).toBeNull()
    expect(state.canStart).toBe(false)
    expect(state.errorMessage).toBe('export.viewStaleRetry')
    await state.start()
    expect(startExport).not.toHaveBeenCalled()

    const refreshed = { ...initialPreflight, count: 6, estimatedBytes: 600, targetWritable: true }
    finishPreflight(refreshed)
    await picking
    expect(state.preflight).toEqual(refreshed)
    expect(state.canStart).toBe(true)
    expect(state.errorMessage).toBe('export.viewStaleRetry')
    expect(startExport).not.toHaveBeenCalled()
    await state.start()
    expect(startExport).toHaveBeenCalledTimes(1)
    expect(startExport).toHaveBeenCalledWith(expect.objectContaining({
      selection: fresh, source, targetParent: 'C:/export-target',
    }))
  })

  it('重建后的预检再次过期时停止自动重试，保持禁止开始', async () => {
    setActivePinia(createPinia())
    const store = useExportStore()
    const rebuild = vi.fn(() => selection(2))
    const preflight = vi.spyOn(store, 'preflightExport')
      .mockRejectedValue(new IpcError('ViewStale', 'layout changed again'))
    const startExport = vi.spyOn(store, 'startExport').mockResolvedValue('unexpected-export')
    app = renderer.createApp({ ...ExportDialog, render: () => null })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: DialogState }).setupState
    store.openExportDialog(selection(1), { kind: 'selection' }, rebuild)
    await new Promise<void>((resolve) => setImmediate(resolve))

    expect(rebuild).toHaveBeenCalledTimes(1)
    expect(preflight).toHaveBeenCalledTimes(2)
    expect(state.errorMessage).toBe('layout changed again')
    expect(state.preflight).toBeNull()
    expect(state.canStart).toBe(false)
    await state.start()
    expect(startExport).not.toHaveBeenCalled()
  })

  it('改选目录后立即作废旧预检，失败保持禁止开始且重试成功只提交新目录', async () => {
    setActivePinia(createPinia())
    const store = useExportStore()
    const original = selection(1)
    const validPreflight: ExportPreflight = {
      count: 2, estimatedBytes: 200, offlineOrMissingCount: 0, nonZeroRotationCount: 0,
      targetWritable: true, insideLibraryWarning: false,
    }
    const preflight = vi.spyOn(store, 'preflightExport')
      .mockResolvedValue({ ...validPreflight, targetWritable: false })
    const startExport = vi.spyOn(store, 'startExport').mockResolvedValue('new-export')
    vi.mocked(open)
      .mockResolvedValueOnce('C:/old-export')
      .mockResolvedValueOnce('C:/new-export')
      .mockResolvedValueOnce('C:/new-export')
    app = renderer.createApp({ ...ExportDialog, render: () => null })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: DialogState }).setupState
    store.openExportDialog(original, { kind: 'view', name: 'all files' }, null)
    await new Promise<void>((resolve) => setImmediate(resolve))
    preflight.mockResolvedValueOnce(validPreflight)
    await state.pickTargetFolder()
    expect(state.canStart).toBe(true)

    let rejectNewTarget!: (error: Error) => void
    preflight.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectNewTarget = reject }))
    const changeTarget = state.pickTargetFolder()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(state.targetParent).toBe('C:/new-export')
    expect(state.preflight).toBeNull()
    expect(state.canStart).toBe(false)
    rejectNewTarget(new IpcError('target_unavailable', 'new target unavailable'))
    await changeTarget
    expect(state.errorMessage).toBe('new target unavailable')
    expect(state.canStart).toBe(false)
    await state.start()
    expect(startExport).not.toHaveBeenCalled()

    preflight.mockResolvedValueOnce(validPreflight)
    await state.pickTargetFolder()
    expect(state.canStart).toBe(true)
    await state.start()
    expect(startExport).toHaveBeenCalledTimes(1)
    expect(startExport).toHaveBeenCalledWith(expect.objectContaining({
      targetParent: 'C:/new-export', selection: original,
    }))
  })

  it('视图过期后重做预检期间及完成后保留重新确认提示且不自动导出', async () => {
    setActivePinia(createPinia())
    const store = useExportStore()
    const original = selection(1)
    const fresh = selection(2)
    const rebuild = vi.fn(() => fresh)
    const initialPreflight: ExportPreflight = {
      count: 2, estimatedBytes: 200, offlineOrMissingCount: 0, nonZeroRotationCount: 0,
      targetWritable: true, insideLibraryWarning: false,
    }
    const preflight = vi.spyOn(store, 'preflightExport').mockResolvedValue(initialPreflight)
    const startExport = vi.spyOn(store, 'startExport')
      .mockRejectedValue(new IpcError('ViewStale', 'layout changed'))
    app = renderer.createApp({ ...ExportDialog, render: () => null })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: DialogState }).setupState
    store.openExportDialog(original, { kind: 'view', name: 'all files' }, rebuild)
    await nextTick()
    await new Promise<void>((resolve) => setImmediate(resolve))
    state.targetParent = 'C:/export-target'
    expect(state.canStart).toBe(true)

    let finishPreflight!: (value: ExportPreflight) => void
    preflight.mockReturnValueOnce(new Promise((resolve) => { finishPreflight = resolve }))
    const start = state.start()
    await new Promise<void>((resolve) => setImmediate(resolve))
    expect(rebuild).toHaveBeenCalledTimes(1)
    expect(preflight).toHaveBeenLastCalledWith(expect.objectContaining({ selection: fresh }))
    expect(state.starting).toBe(true)
    expect(state.errorMessage).toBe('export.viewStaleRetry')

    const refreshedPreflight = { ...initialPreflight, count: 6, estimatedBytes: 600 }
    finishPreflight(refreshedPreflight)
    await start
    expect(state.preflight).toEqual(refreshedPreflight)
    expect(state.errorMessage).toBe('export.viewStaleRetry')
    expect(state.starting).toBe(false)
    expect(store.dialogOpen).toBe(true)
    expect(startExport).toHaveBeenCalledTimes(1)
    expect(startExport).toHaveBeenCalledWith(expect.objectContaining({ selection: original }))
  })
})
