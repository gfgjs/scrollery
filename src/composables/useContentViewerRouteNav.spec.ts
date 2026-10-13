import { afterEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, nextTick, ref, watch, type App } from 'vue'
import { createMemoryHistory, createRouter, useRoute } from 'vue-router'
import { useContentViewerRouteNav } from './useContentViewerRouteNav'
import { useImageEditor } from './useImageEditor'
import { useConfirmDialogState } from './useConfirm'
import type { MediaDetail } from '../types/media'
import type { useMediaStore } from '../stores/mediaStore'

const { invokeIpc } = vi.hoisted(() => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/ipc', () => ({ invokeIpc, IpcError: class extends Error {}, ipcErrorMessage: String }))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})

let app: App | null = null
afterEach(() => {
  useConfirmDialogState().close(false)
  app?.unmount()
  app = null
  invokeIpc.mockReset()
})

async function settle() {
  await new Promise<void>((resolve) => setImmediate(resolve))
  await nextTick()
}

async function mountEditingViewer() {
  const component = { render: () => null }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component }, { path: '/view/:id', component }],
  })
  await router.push('/view/1')
  const detail = ref({ id: 1, viewRotation: 0, mediaType: 'image' } as MediaDetail)
  const editor = useImageEditor()
  editor.open(detail.value)
  editor.rotateCw()
  const loadStates: string[] = []
  const openDetail = vi.fn(async (id: number) => {
    loadStates.push(editor.status.value)
    detail.value = { id, viewRotation: 0, mediaType: 'image' } as MediaDetail
  })
  const media = {
    get detailItem() { return detail.value },
    navContext: { type: 'layout', orderVersion: 1, totalCount: 3, currentIndex: 0 },
    openDetail,
  } as unknown as ReturnType<typeof useMediaStore>
  app = renderer.createApp({
    setup() {
      const navigation = useContentViewerRouteNav({ media, editor, router, route: useRoute() })
      // 与 ContentViewer 相同，实际路由提交后才加载对应资产。
      watch(navigation.routeId, (id) => void navigation.loadFromRoute(id))
      return () => null
    },
  })
  app.use(router)
  app.mount({})
  return { router, editor, detail, openDetail, loadStates }
}

describe('图片编辑的路由退出保护', () => {
  it('并发换图共用放弃确认，取消保留原图编辑，确认后先关闭编辑再加载新图', async () => {
    const { router, editor, detail, openDetail, loadStates } = await mountEditingViewer()
    const dialog = useConfirmDialogState()
    const firstNavigation = router.push('/view/2')
    await settle()
    expect(dialog.state.isOpen).toBe(true)
    expect(router.currentRoute.value.path).toBe('/view/1')
    const firstResolver = dialog.state.resolve
    const latestNavigation = router.push('/view/3')
    await settle()
    expect(dialog.state.resolve).toBe(firstResolver)
    dialog.close(false)
    await Promise.all([firstNavigation, latestNavigation])
    expect(router.currentRoute.value.path).toBe('/view/1')
    expect(detail.value.id).toBe(1)
    expect(editor.status.value).toBe('editing')
    expect(editor.rotate.value).toBe(90)
    expect(openDetail).not.toHaveBeenCalled()

    const confirmedNavigation = router.push('/view/2')
    await settle()
    dialog.close(true)
    await confirmedNavigation
    await settle()
    expect(router.currentRoute.value.path).toBe('/view/2')
    expect(detail.value.id).toBe(2)
    expect(loadStates).toEqual(['idle'])
    expect(editor.status.value).toBe('idle')
  })

  it('保存进行中阻止换图和离开，查看器释放后不残留路由守卫', async () => {
    const { router, editor, detail, openDetail } = await mountEditingViewer()
    let finishSave!: (result: { status: 'saved'; newItemId: number }) => void
    invokeIpc.mockReturnValue(new Promise((resolve) => { finishSave = resolve }))
    const saving = editor.save(1)
    expect(editor.status.value).toBe('saving')

    await router.push('/view/2')
    expect(router.currentRoute.value.path).toBe('/view/1')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/view/1')
    expect(detail.value.id).toBe(1)
    expect(openDetail).not.toHaveBeenCalled()
    expect(useConfirmDialogState().state.isOpen).toBe(false)

    app?.unmount()
    app = null
    await router.push('/view/2')
    expect(router.currentRoute.value.path).toBe('/view/2')
    finishSave({ status: 'saved', newItemId: 4 })
    await saving
  })
})
