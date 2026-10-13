import { beforeEach, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { IPC } from '../../constants/ipc'
import { useReaderBookPrefs } from './useReaderBookPrefs'
import type { ReaderBookPrefs } from '../../types/reader'

const { invoke, addToast } = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  addToast: vi.fn(),
}))
vi.mock('../../utils/ipc', () => ({ invokeIpc: invoke, ipcErrorMessage: String }))
vi.mock('../../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

beforeEach(() => vi.resetAllMocks())

it('读取偏好失败阻止按默认值开卷，重试后修改主题保留原有编码、重排和引擎', async () => {
  const stored: ReaderBookPrefs = {
    v: 1, encoding: 'gb18030', reflow: true, theme: 'sepia', markdownEngine: 'vditor',
  }
  const failure = { code: 'Db', message: 'read failed' }
  invoke.mockRejectedValueOnce(failure).mockResolvedValueOnce(JSON.stringify(stored)).mockResolvedValueOnce(undefined)
  const requestRemount = vi.fn()
  const prefs = useReaderBookPrefs({ id: ref(41), requestRemount })

  // DocumentViewer.load() 等此调用成功才挂载正文；拒绝后由父层显示加载错误及重试入口。
  await expect(prefs.loadForItem(41)).rejects.toBe(failure)
  expect(invoke).toHaveBeenCalledExactlyOnceWith(IPC.GET_READER_BOOK_PREFS, { itemId: 41 })
  expect(requestRemount).not.toHaveBeenCalled()

  await prefs.loadForItem(41)
  expect(prefs.bookEncoding.value).toBe('gb18030')
  expect(prefs.bookReflow.value).toBe(true)
  expect(prefs.bookMarkdownEngine.value).toBe('vditor')
  await prefs.onBookPrefsChange({
    encoding: prefs.bookEncoding.value,
    reflow: prefs.bookReflow.value,
    zhConvert: prefs.bookZhConvert.value,
    theme: 'night',
  })
  expect(invoke).toHaveBeenLastCalledWith(IPC.SET_READER_BOOK_PREFS, {
    itemId: 41, prefs: expect.any(String),
  })
  expect(JSON.parse(String(invoke.mock.calls[2]?.[1]?.prefs))).toEqual({ ...stored, theme: 'night' })
  expect(requestRemount).not.toHaveBeenCalled()
})

it('成功读取空偏好仍使用默认值，换文档后忽略旧读取结果', async () => {
  let finishOld!: (value: string) => void
  const oldRead = new Promise<string>(resolve => { finishOld = resolve })
  invoke.mockReturnValueOnce(oldRead).mockResolvedValueOnce(null)
  const id = ref(41)
  const prefs = useReaderBookPrefs({ id, requestRemount: vi.fn() })
  const loadingOld = prefs.loadForItem(41)
  await Promise.resolve()
  expect(invoke).toHaveBeenCalledWith(IPC.GET_READER_BOOK_PREFS, { itemId: 41 })
  id.value = 42
  prefs.resetLocal()
  await prefs.loadForItem(42)
  finishOld(JSON.stringify({ v: 1, encoding: 'big5', reflow: true, markdownEngine: 'vditor', theme: 'sepia' }))
  await loadingOld
  expect(prefs.bookEncoding.value).toBe('')
  expect(prefs.bookReflow.value).toBe(false)
  expect(prefs.bookZhConvert.value).toBe('')
  expect(prefs.bookReaderTheme.value).toBe('')
  expect(prefs.bookMarkdownEngine.value).toBe('native')
  expect(invoke).toHaveBeenCalledTimes(2)
})
