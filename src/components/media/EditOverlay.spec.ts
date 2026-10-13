import { afterEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, h, ref, ssrContextKey, type App } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import EditOverlay from './EditOverlay.vue'
import { useImageEditor } from '../../composables/useImageEditor'
import { useToastStore } from '../../stores/toastStore'
import { IPC } from '../../constants/ipc'
import type { MediaDetail } from '../../types/media'

const invokeIpc = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>()
const startScan = vi.fn<(rootId: number) => Promise<void>>()
const confirm = vi.fn<() => Promise<{ confirmed: boolean }>>()
const previewUrl = ref<string | null>(null)

vi.mock('../../utils/ipc', () => ({
  invokeIpc: (command: string, args?: Record<string, unknown>) => invokeIpc(command, args),
  IpcError: class extends Error {},
  ipcErrorMessage: (error: Error) => error.message,
}))
vi.mock('../../stores/scanStore', () => ({ useScanStore: () => ({ startScan }) }))
vi.mock('../../composables/useConfirm', () => ({ useConfirm: () => ({ confirm }) }))
vi.mock('../../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string, params?: { fileName?: string }) => params?.fileName ?? key,
  }),
}))
vi.mock('../../composables/useEditPreview', () => ({
  useEditPreview: () => ({
    url: previewUrl, loading: ref(false), error: ref(null),
    sourceWidth: ref(10), sourceHeight: ref(10), load: vi.fn(), dispose: vi.fn(),
  }),
}))
vi.mock('../ui/UiIconButton.vue', () => ({ default: { render: () => null } }))
vi.mock('./CropperSelectionOverlay.vue', () => ({
  default: { render: () => h('div', { class: 'test-crop-control' }) },
}))
vi.mock('@lucide/vue', () => {
  const icon = { render: () => null }
  return { RotateCcw: icon, RotateCw: icon, FlipHorizontal: icon, FlipVertical: icon, Crop: icon, Undo2: icon }
})

// 保留真实编辑保存状态与组件回调，只替代磁盘 IPC、扫描服务和 node 缺失的 DOM。
const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

interface OverlayState {
  toggleCropTool: () => void
  handleSave: () => Promise<void>
  handleClose: () => Promise<void>
  onRescanNow: () => Promise<void>
}

let app: App | null = null
afterEach(() => {
  app?.unmount()
  app = null
  previewUrl.value = null
  invokeIpc.mockReset()
  startScan.mockReset()
  confirm.mockReset()
  vi.clearAllTimers()
  vi.useRealTimers()
})

describe('图片编辑保存后入库恢复', () => {
  it('旋转后重新开启裁剪沿用所选比例，保存的裁剪像素仍为正方形', async () => {
    setActivePinia(createPinia())
    invokeIpc.mockResolvedValue({ status: 'saved', newItemId: 13 })
    const editor = useImageEditor()
    const detail = { id: 12, fileFormat: 'png', viewRotation: 0 } as MediaDetail
    editor.open(detail)
    editor.setNaturalSize(1200, 800)
    app = renderer.createApp({ ...EditOverlay, render: () => null }, { detail, editor })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: OverlayState }).setupState

    state.toggleCropTool()
    editor.setCropRatio('1:1')
    expect(editor.cropToPixelRect()).toEqual({ x: 200, y: 0, width: 800, height: 800 })
    editor.rotateCw()
    expect(editor.crop.value).toBeNull()
    state.toggleCropTool()
    state.toggleCropTool()
    expect(editor.cropRatio.value).toBe('1:1')

    await state.handleSave()
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SAVE_EDITED_IMAGE, expect.objectContaining({
      itemId: 12,
      ops: expect.objectContaining({
        rotate: 90,
        crop: { x: 0, y: 200, width: 800, height: 800 },
      }),
    }))
  })

  it('部分保存后以提交参数为基准保护后续修改，取消放弃后仍可继续编辑', async () => {
    setActivePinia(createPinia())
    let resolveSave!: (result: unknown) => void
    invokeIpc.mockReturnValue(new Promise((resolve) => { resolveSave = resolve }))
    confirm.mockResolvedValue({ confirmed: false })
    const editor = useImageEditor()
    const detail = { id: 12, fileFormat: 'png', viewRotation: 0 } as MediaDetail
    editor.open(detail)
    editor.setBrightness(25)
    const onClose = vi.fn()
    app = renderer.createApp({ ...EditOverlay, render: () => null }, { detail, editor, onClose })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: OverlayState }).setupState

    const saving = state.handleSave()
    // 保存基准必须是已提交的 25，异步返回时的状态不能被顺带标记已保存。
    editor.setBrightness(50)
    resolveSave({ status: 'savedNeedsIndex', pathHint: 'draft-edited.jpeg', rootId: 7, recoveryCode: 'index_failed' })
    await saving
    await state.handleClose()
    expect(confirm).toHaveBeenCalledTimes(1)
    expect(onClose).not.toHaveBeenCalled()
    expect(editor.brightness.value).toBe(50)
    expect(editor.status.value).toBe('partial')
    expect(editor.savedNeedsIndex.value).toEqual({ pathHint: 'draft-edited.jpeg', rootId: 7 })

    // 恢复到实际保存过的参数后，不再重复询问放弃已经落盘的改动。
    editor.setBrightness(25)
    await state.handleClose()
    expect(confirm).toHaveBeenCalledTimes(1)
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(invokeIpc).toHaveBeenCalledTimes(1)
  })

  it('保存期间冻结编辑控件和裁剪交互，失败后保留原参数并恢复编辑', async () => {
    setActivePinia(createPinia())
    let rejectSave!: (reason: Error) => void
    invokeIpc.mockReset()
    invokeIpc.mockReturnValue(new Promise((_, reject) => { rejectSave = reject }))
    previewUrl.value = 'blob:preview'
    const editor = useImageEditor()
    const detail = { id: 12, fileFormat: 'png', viewRotation: 0 } as MediaDetail
    editor.open(detail)
    editor.setNaturalSize(100, 100)
    editor.setCropRect({ x: 0.1, y: 0.1, width: 0.8, height: 0.8 })
    editor.setBrightness(25)
    editor.format.value = 'png'
    const onSaved = vi.fn()
    app = renderer.createApp({ ...EditOverlay, render: () => null }, { detail, editor, onSaved })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: OverlayState }).setupState
    const saving = state.handleSave()
    const html = await renderToString(createSSRApp({ ...EditOverlay, setup: () => state }, { detail, editor }))
    expect(html.match(/<fieldset[^>]*class="edit-overlay__toolbar"[^>]*>/)?.[0] ?? '').toContain('disabled')
    expect(html.match(/<fieldset[^>]*class="edit-overlay__output"[^>]*>/)?.[0] ?? '').toContain('disabled')
    expect(html).not.toContain('test-crop-control')
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SAVE_EDITED_IMAGE, expect.objectContaining({
      ops: expect.objectContaining({ crop: { x: 10, y: 10, width: 80, height: 80 }, adjust: { brightness: 25, contrast: 0, saturation: 0 } }),
      output: expect.objectContaining({ format: 'png' }),
    }))

    rejectSave(new Error('编码失败'))
    await saving
    expect(onSaved).not.toHaveBeenCalled()
    expect(editor.status.value).toBe('error')
    expect(editor.crop.value).toEqual({ x: 0.1, y: 0.1, width: 0.8, height: 0.8 })
    expect(editor.brightness.value).toBe(25)
    expect(editor.format.value).toBe('png')
    const recovered = await renderToString(createSSRApp({ ...EditOverlay, setup: () => state }, { detail, editor }))
    expect(recovered.match(/<fieldset[^>]*class="edit-overlay__toolbar"[^>]*>/)?.[0]).not.toContain('disabled')
    expect(recovered.match(/<fieldset[^>]*class="edit-overlay__output"[^>]*>/)?.[0]).not.toContain('disabled')
    expect(recovered).toContain('test-crop-control')
  })

  it('重扫启动失败保留已保存副本提示并允许重试，成功后关闭', async () => {
    vi.useFakeTimers()
    setActivePinia(createPinia())
    invokeIpc.mockResolvedValue({
      status: 'savedNeedsIndex', pathHint: 'draft-edited.jpeg', rootId: 7, recoveryCode: 'index_failed',
    })
    startScan.mockRejectedValueOnce(new Error('扫描根不可用')).mockResolvedValue(undefined)
    const editor = useImageEditor()
    const detail = { id: 12, fileFormat: 'png', viewRotation: 0 } as MediaDetail
    editor.open(detail)
    editor.rotateCw()
    const onClose = vi.fn()
    app = renderer.createApp({ ...EditOverlay, render: () => null }, { detail, editor, onClose })
    app.provide(ssrContextKey, {})
    const mounted = app.mount({})
    const state = (mounted.$ as unknown as { setupState: OverlayState }).setupState
    await state.handleSave()

    await state.onRescanNow()
    expect(onClose).not.toHaveBeenCalled()
    expect(editor.status.value).toBe('partial')
    expect(editor.savedNeedsIndex.value).toEqual({ pathHint: 'draft-edited.jpeg', rootId: 7 })
    expect(useToastStore().toasts.map(({ type }) => type)).toEqual(['error'])
    const html = await renderToString(createSSRApp({ ...EditOverlay, setup: () => state }, { detail, editor }))
    expect(html).toContain('draft-edited.jpeg')
    expect(html).toContain('edit.rescanNow')

    await state.onRescanNow()
    expect(startScan.mock.calls).toEqual([[7], [7]])
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(useToastStore().toasts.map(({ type }) => type)).toEqual(['error', 'success'])
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    expect(invokeIpc).toHaveBeenCalledWith(IPC.SAVE_EDITED_IMAGE, expect.objectContaining({ itemId: 12 }))
  })
})
