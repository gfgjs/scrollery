import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useHistoryStore } from './historyStore'
import { IPC } from '../constants/ipc'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), toast: vi.fn(), refresh: vi.fn() }))
vi.mock('../utils/ipc', () => ({
  invokeIpc: mocks.invoke,
  parseAppError: (error: { code: string; message: string }) => Object.assign(new Error(error.message), error),
  ipcErrorMessage: (error: unknown) => String(error),
  moveRecoveryOf: () => null,
}))
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('./toastStore', () => ({ useToastStore: () => ({ addToast: mocks.toast }) }))
vi.mock('./scanStore', () => ({ useScanStore: () => ({ startScan: vi.fn() }) }))
vi.mock('./directoryMoveRecoveryStore', () => ({ useDirectoryMoveRecoveryStore: () => ({}) }))

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.invoke.mockReset()
  vi.stubGlobal('window', { dispatchEvent: mocks.refresh })
})
afterEach(() => vi.unstubAllGlobals())

const dbError = { code: 'Db', message: '数据库访问异常' }
const moved = (id: number) => ({ id, fromDirId: id + 10, targetDirId: 20 })

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

it.each(['success', 'failure'] as const)('撤销回调在新删除登记后以 %s 结束，保留正确的撤销顺序和重做分支', async (outcome) => {
  const history = useHistoryStore()
  const pending = deferred<void>()
  const undoA = vi.fn<() => Promise<void>>().mockReturnValueOnce(pending.promise).mockResolvedValue(undefined)
  const a = history.pushUndoable({
    undo: undoA, redo: async () => {}, undoMessage: '恢复 A', redoMessage: '删除 A',
  })
  const undoing = history.undo()
  // 收藏夹、人物和媒体删除均可在另一次撤销在途时完成，并从调用方登记新历史。
  const undoB = vi.fn(async () => {})
  const b = history.pushUndoable({
    undo: undoB, redo: async () => {}, undoMessage: '恢复 B', redoMessage: '删除 B',
  })
  if (outcome === 'success') pending.resolve(undefined)
  else pending.reject(new Error('恢复 A 失败'))
  await undoing

  expect(history.undoStack).toMatchObject(outcome === 'success' ? [{ id: b }] : [{ id: a }, { id: b }])
  expect(history.redoStack).toEqual([])
  expect(history.busy).toBe(false)
  await history.undoIfTop(b)
  expect(undoB).toHaveBeenCalledOnce()
  await history.undo()
  expect(undoA).toHaveBeenCalledTimes(outcome === 'success' ? 1 : 2)
  expect(history.undoStack).toEqual([])
})

it.each(['complete', 'partial'] as const)('媒体移动撤销 %s 返回前登记新编辑，不复活旧重做分支', async (outcome) => {
  const history = useHistoryStore()
  mocks.invoke.mockResolvedValueOnce({ completed: [moved(1), moved(2)], failure: null })
  await history.moveMedia([1, 2], 20, '移动')
  const pending = deferred<unknown>()
  mocks.invoke.mockReturnValueOnce(pending.promise)
  const undoing = history.undo()
  const b = history.pushUndoable({
    undo: async () => {}, redo: async () => {}, undoMessage: '恢复 B', redoMessage: '删除 B',
  })
  pending.resolve(outcome === 'complete'
    ? { completed: [moved(1), moved(2)], failure: null }
    : { completed: [moved(1)], failure: { id: 2, error: dbError } })
  await undoing

  expect(history.undoStack).toMatchObject(outcome === 'complete'
    ? [{ id: b }]
    : [{ type: 'moveMedia', items: [{ id: 2, fromDirId: 12 }] }, { id: b }])
  expect(history.redoStack).toEqual([])
  expect(history.busy).toBe(false)
})

it('批量移动部分失败时保留成功项的撤销入口并报告失败', async () => {
  const history = useHistoryStore()
  mocks.invoke.mockResolvedValueOnce({ completed: [moved(1), moved(2)], failure: { id: 3, error: dbError } })

  await expect(history.moveMedia([1, 2, 3], 20, '移动')).rejects.toMatchObject(dbError)
  expect(history.undoStack).toEqual([{
    type: 'moveMedia', items: [{ id: 1, fromDirId: 11 }, { id: 2, fromDirId: 12 }], targetDirId: 20, label: '移动',
  }])
  expect(history.canUndo).toBe(true)
  expect(history.busy).toBe(false)
  expect(mocks.refresh).toHaveBeenCalledTimes(1)

  mocks.invoke.mockResolvedValueOnce({ completed: [moved(1), moved(2)], failure: null })
  await history.undo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.RELOCATE_MEDIA_ITEMS, {
    moves: [{ id: 1, targetDirId: 11 }, { id: 2, targetDirId: 12 }],
  })
  expect(history.undoStack).toHaveLength(0)
  expect(history.redoStack).toHaveLength(1)
})

it('移动撤销和重做部分完成时拆分实际成功项并保留失败项供重试', async () => {
  const history = useHistoryStore()
  mocks.invoke.mockResolvedValueOnce({ completed: [moved(1), moved(2), moved(3), moved(4), moved(5)], failure: null })
  await history.moveMedia([1, 2, 3, 4, 5], 20, '移动')
  // 1 已被移回原目录，是无操作；2、3 撤销成功，4 失败，5 未执行。
  mocks.invoke.mockResolvedValueOnce({ completed: [moved(2), moved(3)], failure: { id: 4, error: dbError } })
  await history.undo()
  expect(history.undoStack).toMatchObject([{ items: [{ id: 4 }, { id: 5 }] }])
  expect(history.redoStack).toMatchObject([{ items: [{ id: 2 }, { id: 3 }] }])

  // IPC 本身失败，没有足够结果拆分；保留原意图供再次重试。
  mocks.invoke.mockRejectedValueOnce(new Error('connection lost'))
  await history.redo()
  expect(history.redoStack).toMatchObject([{ items: [{ id: 2 }, { id: 3 }] }])

  mocks.invoke.mockResolvedValueOnce({ completed: [moved(2)], failure: { id: 3, error: dbError } })
  await history.redo()
  expect(history.undoStack).toMatchObject([{ items: [{ id: 4 }, { id: 5 }] }, { items: [{ id: 2 }] }])
  expect(history.redoStack).toMatchObject([{ items: [{ id: 3 }] }])

  mocks.invoke.mockResolvedValueOnce({ completed: [moved(3)], failure: null })
  await history.redo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.RELOCATE_MEDIA_ITEMS, { moves: [{ id: 3, targetDirId: 20 }] })
  expect(history.redoStack).toHaveLength(0)
  expect(history.undoStack).toMatchObject([{ items: [{ id: 4 }, { id: 5 }] }, { items: [{ id: 2 }] }, { items: [{ id: 3 }] }])
  expect(history.busy).toBe(false)
  expect(mocks.refresh).toHaveBeenCalledTimes(5)
})

it('批量复制部分失败时仅记录实际副本并保留准确撤销入口', async () => {
  const history = useHistoryStore()
  // 1 是同目录无操作，2、3 已复制，4 失败，5 尚未执行。
  mocks.invoke.mockResolvedValueOnce({
    completed: [{ srcId: 2, newId: 102 }, { srcId: 3, newId: 103 }],
    failure: { id: 4, error: dbError },
  })
  mocks.refresh.mockImplementationOnce(() => {
    expect(history.undoStack).toMatchObject([{ srcIds: [2, 3], createdIds: [102, 103] }])
  })

  await expect(history.copyMedia([1, 2, 3, 4, 5], 20, '复制')).rejects.toMatchObject(dbError)
  expect(history.undoStack).toEqual([{
    type: 'copyMedia', srcIds: [2, 3], targetDirId: 20, createdIds: [102, 103], label: '复制',
  }])
  expect(history.canUndo).toBe(true)
  expect(history.busy).toBe(false)
  expect(mocks.refresh).toHaveBeenCalledTimes(1)

  mocks.invoke.mockResolvedValueOnce(undefined)
  await history.undo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.REMOVE_MEDIA_ITEMS_HARD, { ids: [102, 103] })
  expect(history.undoStack).toHaveLength(0)
  expect(history.redoStack).toMatchObject([{ srcIds: [2, 3] }])
})

it('复制重做失败时保留未完成源且重试不重复复制成功项', async () => {
  const history = useHistoryStore()
  history.redoStack.push({
    type: 'copyMedia', srcIds: [1, 2, 3, 4], targetDirId: 20, createdIds: [101, 102, 103, 104], label: '复制',
  })
  mocks.invoke.mockRejectedValueOnce(new Error('connection lost'))
  await history.redo()
  expect(history.redoStack).toMatchObject([{ srcIds: [1, 2, 3, 4] }])
  expect(history.undoStack).toHaveLength(0)

  // 1 变为同目录无操作；2 完成，3 失败，4 未执行。
  mocks.invoke.mockResolvedValueOnce({ completed: [{ srcId: 2, newId: 202 }], failure: { id: 3, error: dbError } })
  await history.redo()
  expect(history.undoStack).toMatchObject([{ srcIds: [2], createdIds: [202] }])
  expect(history.redoStack).toMatchObject([{ srcIds: [3, 4] }])

  mocks.invoke.mockResolvedValueOnce({ completed: [{ srcId: 3, newId: 203 }, { srcId: 4, newId: 204 }], failure: null })
  await history.redo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.COPY_MEDIA_ITEMS_DB, { moves: [{ id: 3, targetDirId: 20 }, { id: 4, targetDirId: 20 }] })
  expect(history.redoStack).toHaveLength(0)
  expect(history.undoStack).toMatchObject([{ srcIds: [2], createdIds: [202] }, { srcIds: [3, 4], createdIds: [203, 204] }])
  expect(mocks.refresh).toHaveBeenCalledTimes(3)
  expect(history.busy).toBe(false)

  mocks.invoke.mockResolvedValueOnce(undefined)
  await history.undo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.REMOVE_MEDIA_ITEMS_HARD, { ids: [203, 204] })
  mocks.invoke.mockResolvedValueOnce(undefined)
  await history.undo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.REMOVE_MEDIA_ITEMS_HARD, { ids: [202] })
})

it('复制撤销部分回收失败后保留原记录并可重试及重做', async () => {
  const history = useHistoryStore()
  const copies = new Set([101, 102])
  mocks.invoke.mockResolvedValueOnce({ completed: [{ srcId: 1, newId: 101 }, { srcId: 2, newId: 102 }], failure: null })
  await history.copyMedia([1, 2], 20, '复制')
  mocks.invoke.mockImplementationOnce(async () => {
    copies.delete(101)
    throw new Error('second file unavailable')
  })

  await history.undo()
  expect([...copies]).toEqual([102])
  expect(history.undoStack).toEqual([{
    type: 'copyMedia', srcIds: [1, 2], targetDirId: 20, createdIds: [101, 102], label: '复制',
  }])
  expect(history.redoStack).toHaveLength(0)
  expect(history.canUndo).toBe(true)
  expect(history.busy).toBe(false)
  expect(mocks.refresh).toHaveBeenCalledTimes(2)

  // 后端容忍第一项已经不存在；仍用原 ID 集合完成剩余回收。
  mocks.invoke.mockImplementationOnce(async (_command: string, args: { ids: number[] }) => {
    args.ids.forEach((id) => copies.delete(id))
  })
  await history.undo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.REMOVE_MEDIA_ITEMS_HARD, { ids: [101, 102] })
  expect(copies.size).toBe(0)
  expect(history.undoStack).toHaveLength(0)
  expect(history.redoStack).toMatchObject([{ srcIds: [1, 2], createdIds: [101, 102] }])

  mocks.invoke.mockResolvedValueOnce({ completed: [{ srcId: 1, newId: 201 }, { srcId: 2, newId: 202 }], failure: null })
  await history.redo()
  expect(mocks.invoke).toHaveBeenLastCalledWith(IPC.COPY_MEDIA_ITEMS_DB, { moves: [{ id: 1, targetDirId: 20 }, { id: 2, targetDirId: 20 }] })
  expect(history.redoStack).toHaveLength(0)
  expect(history.undoStack).toMatchObject([{ srcIds: [1, 2], createdIds: [201, 202] }])
  expect(history.busy).toBe(false)
})
