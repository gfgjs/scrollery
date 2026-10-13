import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { useDocEditVersion, type UseDocEditVersionDeps } from './useDocEditVersion'

const { invokeIpc, confirm, addToast } = vi.hoisted(() => ({
  invokeIpc: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  confirm: vi.fn<() => Promise<boolean>>(),
  addToast: vi.fn(),
}))

vi.mock('../../utils/ipc', () => ({ invokeIpc, ipcErrorMessage: String }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

function setup(options: Partial<UseDocEditVersionDeps> = {}) {
  const id = ref(1)
  const requestRemount = vi.fn()
  const scope = effectScope()
  const editor = scope.run(() => useDocEditVersion({ id, requestRemount, ...options }))!
  editor.textContent.value = '文档 A 正文'
  editor.currentVersionId.value = 10
  editor.startEdit()
  editor.editBuffer.value = '文档 A 修改'
  return { id, requestRemount, scope, editor }
}

beforeEach(() => {
  vi.resetAllMocks()
  invokeIpc.mockResolvedValue(undefined)
  confirm.mockResolvedValue(true)
})

describe('文档编辑的异步隔离', () => {

  it.each([
    { target: 'overwrite', failure: 'write' },
    { target: 'overwrite', failure: 'refresh' },
    { target: 'version', failure: 'refresh' },
  ] as const)('$target 保存已改变生效正文但 $failure 失败后，撤销原文仍须保存且可重试', async ({ target, failure }) => {
    const { editor, scope } = setup()
    let diskContent = '文档 A 正文'
    let diskVersion: number | null = 10
    let fail = true
    invokeIpc.mockImplementation(async (command, args) => {
      if (command === IPC.SAVE_VERSION) {
        if (args?.target === 'overwrite') {
          diskContent = String(args.content)
          diskVersion = null
          // 文件已替换，但随后的数据库提交或回执失败。
          if (fail && failure === 'write') throw new Error('commit failed after replacement')
        }
        return 11
      }
      if (command === IPC.SET_CURRENT_VERSION) {
        diskContent = '文档 A 修改'
        diskVersion = 11
      }
      if (command === IPC.GET_DOCUMENT_TEXT) {
        if (fail && failure === 'refresh') throw new Error('refresh failed')
        return diskContent
      }
      if (command === IPC.GET_CURRENT_VERSION) return diskVersion === null ? null : { id: diskVersion }
    })
    try {
      const saved = target === 'overwrite' ? editor.overwriteSource() : editor.saveNewVersion()
      expect(await saved).toBe(false)
      expect(diskContent).toBe('文档 A 修改')
      expect(editor.editing.value).toBe(true)

      editor.editBuffer.value = '文档 A 正文'
      // 父层依赖 dirty 判断是否提示保存或直接放行；旧基线不能掩盖尚未落盘的撤销。
      expect(editor.dirty.value).toBe(true)
      fail = false
      expect(await editor.overwriteSource()).toBe(true)
      expect(diskContent).toBe('文档 A 正文')
      expect(editor.textContent.value).toBe(diskContent)
      expect(editor.dirty.value).toBe(false)
      expect(editor.editing.value).toBe(false)
    } finally {
      scope.stop()
    }
  })

  it('保存失败保留草稿并返回失败，重试共享在途保存且完成激活和刷新后才放行', async () => {
    const { editor, requestRemount, scope } = setup()
    invokeIpc.mockRejectedValueOnce({ code: 'permission_denied' })
    try {
      expect(await editor.saveNewVersion()).toBe(false)
      expect(editor.editing.value).toBe(true)
      expect(editor.editBuffer.value).toBe('文档 A 修改')
      expect(editor.dirty.value).toBe(true)
      expect(editor.saving.value).toBe(false)
      expect(requestRemount).not.toHaveBeenCalled()

      const saved = deferred<number>()
      invokeIpc.mockReturnValueOnce(saved.promise)
      invokeIpc.mockResolvedValueOnce(undefined)
      invokeIpc.mockResolvedValueOnce('文档 A 修改')
      invokeIpc.mockResolvedValueOnce({ id: 11 })
      const saving = editor.saveNewVersion()
      expect(editor.saveNewVersion()).toBe(saving)
      expect(editor.overwriteSource()).toBe(saving)
      expect(editor.saving.value).toBe(true)
      expect(confirm).not.toHaveBeenCalled()
      saved.resolve(11)

      expect(await saving).toBe(true)
      expect(editor.saving.value).toBe(false)
      expect(editor.editing.value).toBe(false)
      expect(editor.dirty.value).toBe(false)
      expect(editor.textContent.value).toBe('文档 A 修改')
      expect(editor.currentVersionId.value).toBe(11)
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_CURRENT_VERSION, { itemId: 1, versionId: 11 })
      expect(requestRemount).toHaveBeenCalledExactlyOnceWith(false)
    } finally {
      scope.stop()
    }
  })

  it('覆盖保存期间新增的草稿不会被旧成功回执关闭或允许离开', async () => {
    const pendingPrefs = deferred<void>()
    const onSourceOverwritten = vi.fn()
    const { editor, scope } = setup({ beforeSourceOverwrite: () => pendingPrefs.promise, onSourceOverwritten })
    const saved = deferred<void>()
    invokeIpc.mockReturnValueOnce(saved.promise)
    invokeIpc.mockResolvedValueOnce('文档 A 修改')
    invokeIpc.mockResolvedValueOnce(null)
    try {
      const saving = editor.overwriteSource()
      await Promise.resolve()
      expect(invokeIpc).not.toHaveBeenCalled()
      expect(onSourceOverwritten).not.toHaveBeenCalled()
      pendingPrefs.resolve()
      await new Promise(resolve => setImmediate(resolve))
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SAVE_VERSION, {
        itemId: 1, content: '文档 A 修改', label: null, parentId: 10, target: 'overwrite',
      })
      editor.editBuffer.value = '保存期间继续输入的内容'
      saved.resolve()

      const result = await saving
      expect(editor.editing.value).toBe(true)
      expect(result).toBe(false)
      expect(editor.editBuffer.value).toBe('保存期间继续输入的内容')
      expect(editor.dirty.value).toBe(true)
      expect(editor.saving.value).toBe(false)
      expect(editor.textContent.value).toBe('文档 A 修改')
      expect(onSourceOverwritten).toHaveBeenCalledExactlyOnceWith(1)
    } finally {
      scope.stop()
    }
  })

  it('保存途中切换文档仍只激活原文档版本且保留新文档编辑状态', async () => {
    const saved = deferred<number>()
    invokeIpc.mockReturnValueOnce(saved.promise)
    const { id, requestRemount, scope, editor } = setup()
    try {
      const saving = editor.saveNewVersion()
      expect(invokeIpc).toHaveBeenCalledWith(IPC.SAVE_VERSION, {
        itemId: 1, content: '文档 A 修改', label: null, parentId: 10, target: 'version',
      })
      id.value = 2
      editor.textContent.value = '文档 B 正文'
      editor.currentVersionId.value = 20
      editor.startEdit()
      editor.editBuffer.value = '文档 B 修改'

      saved.resolve(11)
      await saving

      expect(invokeIpc).toHaveBeenLastCalledWith(IPC.SET_CURRENT_VERSION, {
        itemId: 1, versionId: 11,
      })
      expect(invokeIpc).toHaveBeenCalledTimes(2)
      expect(editor.textContent.value).toBe('文档 B 正文')
      expect(editor.currentVersionId.value).toBe(20)
      expect(editor.editing.value).toBe(true)
      expect(editor.editBuffer.value).toBe('文档 B 修改')
      expect(requestRemount).not.toHaveBeenCalled()
      editor.editBuffer.value = '文档 B 正文'
      expect(editor.dirty.value).toBe(false)
    } finally {
      scope.stop()
    }
  })

  it('覆盖已发出后切换文档，旧基线失效状态与迟到回执均不污染新文档', async () => {
    const saved = deferred<number>()
    invokeIpc.mockReturnValueOnce(saved.promise)
    const { id, requestRemount, scope, editor } = setup()
    try {
      const saving = editor.overwriteSource()
      await Promise.resolve()
      expect(invokeIpc).toHaveBeenCalledTimes(1)
      editor.editBuffer.value = '文档 A 正文'
      expect(editor.dirty.value).toBe(true)

      id.value = 2
      editor.textContent.value = '文档 B 正文'
      editor.currentVersionId.value = 20
      editor.startEdit()
      expect(editor.dirty.value).toBe(false)
      saved.resolve(11)
      expect(await saving).toBe(false)
      expect(editor.dirty.value).toBe(false)
      expect(editor.editing.value).toBe(true)
      expect(editor.editBuffer.value).toBe('文档 B 正文')
      expect(editor.currentVersionId.value).toBe(20)
      expect(invokeIpc).toHaveBeenCalledTimes(1)
      expect(requestRemount).not.toHaveBeenCalled()
    } finally {
      scope.stop()
    }
  })

  it('覆盖确认期间切换文档后不写入任何源文件', async () => {
    const confirmation = deferred<boolean>()
    confirm.mockReturnValueOnce(confirmation.promise)
    const { id, requestRemount, scope, editor } = setup()
    try {
      const saving = editor.overwriteSource()
      expect(confirm).toHaveBeenCalledTimes(1)
      id.value = 2
      editor.textContent.value = '文档 B 正文'
      editor.currentVersionId.value = 20
      editor.startEdit()
      editor.editBuffer.value = '文档 B 修改'
      confirmation.resolve(true)
      await saving

      expect(invokeIpc).not.toHaveBeenCalled()
      expect(editor.textContent.value).toBe('文档 B 正文')
      expect(editor.editing.value).toBe(true)
      expect(requestRemount).not.toHaveBeenCalled()
    } finally {
      scope.stop()
    }
  })

  it('刷新正文期间切换文档会丢弃旧正文且不刷新新文档版本', async () => {
    const text = deferred<string>()
    invokeIpc.mockReturnValueOnce(text.promise)
    const { id, requestRemount, scope, editor } = setup()
    try {
      const refreshing = editor.refreshText()
      expect(invokeIpc).toHaveBeenCalledWith(IPC.GET_DOCUMENT_TEXT, { itemId: 1 })
      id.value = 2
      editor.textContent.value = '文档 B 正文'
      editor.currentVersionId.value = 20
      text.resolve('文档 A 旧请求结果')
      await refreshing

      expect(editor.textContent.value).toBe('文档 B 正文')
      expect(editor.currentVersionId.value).toBe(20)
      expect(invokeIpc).toHaveBeenCalledTimes(1)
      expect(requestRemount).not.toHaveBeenCalled()
    } finally {
      scope.stop()
    }
  })
})
