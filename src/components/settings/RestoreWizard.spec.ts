import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, h, nextTick, reactive, ref, ssrContextKey, type App } from 'vue'
import RestoreWizard from './RestoreWizard.vue'
import type { RestoreStageResult } from '../../stores/backupStore'

const { restoreStage, restoreDiscard, restoreArm, relaunchApp, addToast } = vi.hoisted(() => ({
  restoreStage: vi.fn<(path: string) => Promise<RestoreStageResult>>(),
  restoreDiscard: vi.fn<(stage: RestoreStageResult) => Promise<void>>(),
  restoreArm: vi.fn<(stage: RestoreStageResult) => Promise<void>>(),
  relaunchApp: vi.fn<() => Promise<void>>(),
  addToast: vi.fn(),
}))
vi.mock('../../stores/backupStore', () => ({
  useBackupStore: () => ({ restoreStage, restoreDiscard, restoreArm, relaunchApp }),
}))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ locale: ref('zh-CN') }) }))
vi.mock('../../i18n/backupMessages', () => ({ useBackupI18n: () => ({ t: (key: string) => key }) }))
vi.mock('../../utils/ipc', () => ({ IpcError: class extends Error {} }))
vi.mock('../ui/UiDialog.vue', () => ({ default: { render: () => null } }))
vi.mock('../ui/UiButton.vue', () => ({ default: { render: () => null } }))
vi.mock('../ui/UiCheckbox.vue', () => ({ default: { render: () => null } }))

const renderer = createRenderer<object, object>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText() {}, setElementText() {}, patchProp() {}, insert() {}, remove() {},
  parentNode: () => null, nextSibling: () => null,
})
const TestWizard = Object.assign({}, RestoreWizard, { render: () => null })
interface WizardState {
  phase: string
  summary: RestoreStageResult | null
  typedConfirmation: string
  busy: boolean
  closing: boolean
  closeWizard: () => Promise<void>
  validatePackage: () => Promise<void>
  armAndRestart: () => Promise<void>
}
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}
function stage(backupId: string): RestoreStageResult {
  return {
    backupId, stagingToken: `token-${backupId}`, stagingDir: `/staging/${backupId}`, schemaVersion: 1, kind: 'backup',
    createdAtUtc: '2026-10-04T00:00:00Z', counts: { items: 1, albums: 0, tags: 0, namedPersons: 0 },
    roots: [], externalDocumentVersions: 0, appdataDocumentCount: 0,
  }
}
async function settle() {
  await nextTick()
  await new Promise<void>((resolve) => setImmediate(resolve))
  await nextTick()
}
let app: App | null = null
function mount(open = false) {
  const props = reactive({ open, packagePath: 'A' })
  const onClose = vi.fn()
  let instance!: { $: { setupState: WizardState } }
  app = renderer.createApp({
    render: () => h(TestWizard, { ...props, onClose, ref: (value: unknown) => { instance = value as typeof instance } }),
  })
  app.provide(ssrContextKey, {})
  app.mount({})
  return { props, state: instance.$.setupState, onClose }
}
beforeEach(() => {
  vi.resetAllMocks()
  restoreDiscard.mockResolvedValue(undefined)
  restoreArm.mockResolvedValue(undefined)
  relaunchApp.mockResolvedValue(undefined)
})
afterEach(async () => {
  app?.unmount()
  app = null
  await settle()
})

describe('RestoreWizard 暂存库生命周期', () => {
  it('串行释放切包的迟到暂存，取消须等清理成功，外部关闭和卸载仍收尾并反馈失败', async () => {
    const firstStage = deferred<RestoreStageResult>()
    const firstDiscard = deferred<void>()
    const cancelDiscard = deferred<void>()
    const closedStage = deferred<RestoreStageResult>()
    restoreStage.mockImplementation((path) => {
      if (path === 'A') return firstStage.promise
      if (path === 'C') return closedStage.promise
      return Promise.resolve(stage(path))
    })
    let bDiscards = 0
    restoreDiscard.mockImplementation((result) => {
      if (result.backupId === 'A') return firstDiscard.promise
      if (result.backupId === 'B') {
        if (++bDiscards === 1) return Promise.reject(new Error('cleanup failed'))
        return cancelDiscard.promise
      }
      if (result.backupId === 'D') return Promise.reject(new Error('unmounted cleanup failed'))
      return Promise.resolve()
    })
    const { props, state, onClose } = mount()
    props.open = true
    await settle()
    props.packagePath = 'B'
    await settle()
    expect(restoreStage.mock.calls).toEqual([['A']])
    firstStage.resolve(stage('A'))
    await settle()
    expect(restoreDiscard).toHaveBeenCalledWith(stage('A'))
    expect(state.summary).toBeNull()
    expect(restoreStage.mock.calls).toEqual([['A']])
    firstDiscard.resolve()
    await settle()
    expect(restoreStage.mock.calls).toEqual([['A'], ['B']])
    expect(state.summary?.backupId).toBe('B')

    await state.closeWizard()
    expect(onClose).not.toHaveBeenCalled()
    expect(state.phase).toBe('error')
    const closing = state.closeWizard()
    await settle()
    expect(state.busy).toBe(true)
    expect(state.closing).toBe(true)
    expect(onClose).not.toHaveBeenCalled()
    cancelDiscard.resolve()
    await closing
    expect(onClose).toHaveBeenCalledTimes(1)
    props.open = false
    await settle()

    props.packagePath = 'C'
    props.open = true
    await settle()
    props.open = false
    await settle()
    closedStage.resolve(stage('C'))
    await settle()
    expect(restoreDiscard).toHaveBeenCalledWith(stage('C'))
    expect(state.summary).toBeNull()
    props.packagePath = 'D'
    props.open = true
    await settle()
    expect(state.summary?.backupId).toBe('D')
    app!.unmount()
    app = null
    await settle()
    expect(restoreDiscard).toHaveBeenCalledWith(stage('D'))
    expect(addToast).toHaveBeenCalledWith('error', 'backup.restoreErrorUnknown')
  })

  it('首次打开即预检，arm期间与成功移交后关闭或卸载不得清理待恢复库', async () => {
    restoreStage.mockResolvedValue(stage('A'))
    const arming = deferred<void>()
    restoreArm.mockReturnValue(arming.promise)
    const { props, state, onClose } = mount(true)
    await settle()
    expect(restoreStage).toHaveBeenCalledWith('A')
    state.typedConfirmation = 'backup.restoreConfirmWord'
    const restoring = state.armAndRestart()
    await state.armAndRestart()
    expect(restoreArm).toHaveBeenCalledTimes(1)
    await state.closeWizard()
    props.open = false
    await settle()
    expect(onClose).not.toHaveBeenCalled()
    expect(restoreDiscard).not.toHaveBeenCalled()
    app!.unmount()
    app = null
    arming.resolve()
    await restoring
    await settle()
    expect(relaunchApp).toHaveBeenCalledTimes(1)
    expect(restoreDiscard).not.toHaveBeenCalled()
  })
})
