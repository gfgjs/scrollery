// src/composables/reader/useReaderBookPrefs.spec.ts
// characterization:needsRemount 键集合判定 → requestRemount(true)(方案 §5 风险面 4)。
// 红线(源文件头注释 §3 风险2):仅 encoding/reflow/zhConvert 变化才 remount;theme 是纯显示层，
// 单独变化**不**触发 requestRemount(BookReader watch 实时重着色)。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { effectScope, ref } from 'vue'
import { setActivePinia, createPinia } from 'pinia'

const { invokeIpc } = vi.hoisted(() => ({
  invokeIpc: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(() => Promise.resolve()),
}))
vi.mock('../../utils/ipc', () => ({
  invokeIpc,
  ipcErrorMessage: (e: unknown) => String(e),
}))
vi.mock('../../constants/ipc', () => ({
  IPC: { GET_READER_BOOK_PREFS: 'get_reader_book_prefs', SET_READER_BOOK_PREFS: 'set_reader_book_prefs' },
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

import { useReaderBookPrefs } from './useReaderBookPrefs'

function withPrefs() {
  const requestRemount = vi.fn()
  const id = ref(1)
  const deps = { id, requestRemount }
  const scope = effectScope()
  const api = scope.run(() => useReaderBookPrefs(deps))!
  return { api, id, requestRemount, scope }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

describe('useReaderBookPrefs:needsRemount 键集合 → requestRemount(true)(characterization)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    invokeIpc.mockClear()
  })

  it('encoding 变化:写 prefs 成功后调用 requestRemount(true)', async () => {
    const { api, requestRemount, scope } = withPrefs()
    await api.onBookPrefsChange({ encoding: 'gbk', reflow: false, zhConvert: '', theme: '' })
    expect(requestRemount).toHaveBeenCalledTimes(1)
    expect(requestRemount).toHaveBeenCalledWith(true)
    scope.stop()
  })

  it('reflow 变化:调用 requestRemount(true)', async () => {
    const { api, requestRemount, scope } = withPrefs()
    await api.onBookPrefsChange({ encoding: '', reflow: true, zhConvert: '', theme: '' })
    expect(requestRemount).toHaveBeenCalledWith(true)
    scope.stop()
  })

  it('zhConvert 变化:调用 requestRemount(true)', async () => {
    const { api, requestRemount, scope } = withPrefs()
    await api.onBookPrefsChange({ encoding: '', reflow: false, zhConvert: 's2t', theme: '' })
    expect(requestRemount).toHaveBeenCalledWith(true)
    scope.stop()
  })

  it('仅 theme 变化(非 remount 键):不调用 requestRemount,但仍落 ref + 写库', async () => {
    const { api, requestRemount, scope } = withPrefs()
    await api.onBookPrefsChange({ encoding: '', reflow: false, zhConvert: '', theme: 'sepia' })
    expect(requestRemount).not.toHaveBeenCalled()
    expect(api.bookReaderTheme.value).toBe('sepia')
    expect(invokeIpc).toHaveBeenCalledWith('set_reader_book_prefs', expect.objectContaining({ itemId: 1 }))
    scope.stop()
  })

  it('全字段与当前值相同(空变更):不调用 requestRemount', async () => {
    const { api, requestRemount, scope } = withPrefs()
    await api.onBookPrefsChange({ encoding: '', reflow: false, zhConvert: '', theme: '' })
    expect(requestRemount).not.toHaveBeenCalled()
    scope.stop()
  })

  it('写库失败(SET_READER_BOOK_PREFS reject):early return,不调用 requestRemount', async () => {
    const { api, requestRemount, scope } = withPrefs()
    invokeIpc.mockImplementationOnce(() => Promise.reject(new Error('db busy')))
    await api.onBookPrefsChange({ encoding: 'utf8', reflow: false, zhConvert: '', theme: '' })
    expect(requestRemount).not.toHaveBeenCalled()
    scope.stop()
  })

  it('切书后丢弃旧书较晚返回的偏好，保留新书状态', async () => {
    const { api, id, scope } = withPrefs()
    const oldPrefs = deferred<unknown>()
    invokeIpc
      .mockReturnValueOnce(oldPrefs.promise)
      .mockResolvedValueOnce(JSON.stringify({ v: 1, encoding: 'utf8', reflow: true, theme: 'sepia' }))

    const first = api.loadForItem(1)
    id.value = 2
    api.resetLocal()
    await api.loadForItem(2)
    oldPrefs.resolve(JSON.stringify({ v: 1, encoding: 'gbk', reflow: false, theme: 'old' }))
    await first

    expect(api.bookEncoding.value).toBe('utf8')
    expect(api.bookReflow.value).toBe(true)
    expect(api.bookReaderTheme.value).toBe('sepia')
    scope.stop()
  })

  it('旧书偏好写入完成时若已切书，不得重挂载新书', async () => {
    const { api, id, requestRemount, scope } = withPrefs()
    const write = deferred<unknown>()
    invokeIpc.mockReturnValueOnce(write.promise)

    const pending = api.onBookPrefsChange({
      encoding: 'gbk',
      reflow: false,
      zhConvert: '',
      theme: '',
    })
    id.value = 2
    api.resetLocal()
    write.resolve(undefined)
    await pending

    expect(requestRemount).not.toHaveBeenCalled()
    scope.stop()
  })
})
