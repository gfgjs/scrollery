import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { IPC } from '../../constants/ipc'
import { useMarkdownAutoSave } from './useMarkdownAutoSave'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('../../utils/ipc', () => ({ invokeIpc: invoke }))

beforeEach(() => {
  vi.useFakeTimers()
  invoke.mockReset()
})
afterEach(() => vi.useRealTimers())

it('覆盖写入后回执失败时，撤销仍强制补写原文而不沿用旧磁盘基线', async () => {
  let diskContent = '# 原文'
  let failCommit!: (error: unknown) => void
  const commit = new Promise<void>((_, reject) => { failCommit = reject })
  invoke.mockImplementationOnce(async (_command, args: { content: string }) => {
    diskContent = args.content
    await commit
  }).mockImplementationOnce(async (_command, args: { content: string }) => {
    diskContent = args.content
  })
  const editor = useMarkdownAutoSave({ itemId: 41, initialSource: diskContent })
  try {
    editor.updateSource('# 新稿')
    const saving = editor.saveNow()
    editor.updateSource('# 原文')
    // 覆盖文件与版本指针提交分属两种资源；IPC 失败不能证明文件未变。
    failCommit({ code: 'Db', message: 'commit failed after source replacement' })
    expect(await saving).toBe(true)
    expect(diskContent).toBe('# 原文')
    expect(invoke).toHaveBeenCalledTimes(2)
    expect(editor.saveState.value).toBe('saved')
  } finally {
    editor.dispose()
  }
})

it('保存期间撤销到原文仍会排队写回且确认磁盘内容后才标记已保存', async () => {
  let finishWrite!: () => void
  const writing = new Promise<void>((resolve) => { finishWrite = resolve })
  invoke.mockReturnValueOnce(writing).mockResolvedValueOnce(undefined)
  const editor = useMarkdownAutoSave({ itemId: 41, initialSource: '# 原文' })
  try {
    editor.updateSource('# 新稿')
    await vi.advanceTimersByTimeAsync(1000)
    expect(invoke).toHaveBeenCalledTimes(1)

    editor.updateSource('# 原文')
    expect(editor.saveState.value).toBe('saving')
    const saving = editor.saveNow()
    finishWrite()
    expect(await saving).toBe(true)

    expect(invoke.mock.calls.map(([, args]) => args.content)).toEqual(['# 新稿', '# 原文'])
    expect(editor.saveState.value).toBe('saved')
  } finally {
    finishWrite()
    editor.dispose()
  }
})

it('离开前保存立即冲刷防抖且失败阻止离开，重试仍保存原文档', async () => {
  let failWrite!: (error: unknown) => void
  const writing = new Promise<void>((_, reject) => { failWrite = reject })
  invoke.mockReturnValueOnce(writing).mockResolvedValueOnce(undefined)
  const options = { itemId: 41, initialSource: '# 原文' }
  const editor = useMarkdownAutoSave(options)
  try {
    editor.updateSource('# 未保存的修改')
    options.itemId = 42
    const leaving = editor.saveNow()
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(editor.saveState.value).toBe('saving')
    const failure = { code: 'permission_denied' }
    failWrite(failure)
    expect(await leaving).toBe(false)
    expect(editor.saveError.value).toEqual(failure)
    expect(editor.saveState.value).toBe('error')

    expect(await editor.saveNow()).toBe(true)
    expect(invoke).toHaveBeenLastCalledWith(IPC.SAVE_VERSION, {
      itemId: 41, content: '# 未保存的修改', label: null, parentId: null,
      target: 'overwrite', source: 'user',
    })
    expect(invoke).toHaveBeenCalledTimes(2)
    expect(editor.saveState.value).toBe('saved')
    expect(vi.getTimerCount()).toBe(0)
  } finally {
    editor.dispose()
  }
})
