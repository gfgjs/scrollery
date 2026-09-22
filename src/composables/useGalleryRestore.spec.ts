import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { effectScope, nextTick, reactive, type EffectScope } from 'vue'
import type { LayoutRow } from '../types/layout'
import { IPC } from '../constants/ipc'

const invokeIpc = vi.fn()
vi.mock('../utils/ipc', () => ({ invokeIpc: (...args: unknown[]) => invokeIpc(...args) }))
vi.mock('../utils/logger', () => ({ logger: { warn: vi.fn(), error: vi.fn() } }))
vi.mock('../stores/mediaStore', () => ({ useMediaStore: () => ({ isComputingLayout: false }) }))
vi.mock('../stores/uiStore', () => ({ useUiStore: () => ({}) }))
vi.mock('../stores/viewStore', () => ({ useViewStore: () => ({}) }))
const lens = reactive({ mode: 'groups', showUniqueItems: false })
vi.mock('../stores/duplicateLensStore', () => ({ useDuplicateLensStore: () => lens }))
import { useReflowAnchor } from './useReflowAnchor'
import { useLensFocusRestore } from './useLensFocusRestore'

let scope: EffectScope
beforeEach(() => {
  scope = effectScope()
  invokeIpc.mockReset()
  vi.useFakeTimers()
  lens.mode = 'groups'
})
afterEach(() => { scope.stop(); vi.useRealTimers(); vi.unstubAllGlobals() })

it('重排锚点按相交行的屏内偏移解析指定版本,切视图或销毁后迟到坐标无效', async () => {
  const rows = [
    { rowType: 'normal', y: 0, height: 200, items: [{ id: 1 }, { id: 2 }] },
    { rowType: 'normal', y: 204, height: 200, items: [{ id: 3 }, { id: 4 }] },
  ] as LayoutRow[]
  let viewKey = 'all'
  const anchor = scope.run(() => useReflowAnchor({
    gridRef: () => ({}) as HTMLElement, activeRows: () => rows,
    currentLogicalY: () => 250, getViewKey: () => viewKey,
  }))!
  anchor.captureReflowAnchor()
  invokeIpc.mockResolvedValueOnce(1000)
  expect(await anchor.resolveReflowAnchor(2, () => true)).toBe(1046)
  expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_ITEM_Y_BY_ID, { itemId: 3, layoutVersion: 2 })
  let finish!: (value: number) => void
  invokeIpc.mockImplementation(() => new Promise<number>((resolve) => { finish = resolve }))
  const stale = anchor.resolveReflowAnchor(3, () => true)
  viewKey = 'dir-2'
  finish(2000)
  expect(await stale).toBeNull()
  viewKey = 'all'
  const late = anchor.resolveReflowAnchor(4, () => true)
  scope.stop()
  finish(3000)
  expect(await late).toBeNull()
  expect(vi.getTimerCount()).toBe(0)
})

it('镜头恢复只提交当前目标,焦点等待换代后停止;当前布局缺项仍返回顶部', async () => {
  const focus = vi.fn()
  const card = { dataset: { itemId: '7' }, getBoundingClientRect: () => ({ top: 60 }), focus }
  const active = { closest: () => card }
  const grid = { contains: () => true, getBoundingClientRect: () => ({ top: 20 }), querySelector: vi.fn(() => null), focus }
  vi.stubGlobal('document', { activeElement: active, querySelector: () => ({ focus }) })
  let current = true
  const restore = scope.run(() => useLensFocusRestore({ gridRef: () => grid as unknown as HTMLElement, getViewKey: () => lens.mode }))!
  lens.mode = 'folders'
  await nextTick()
  invokeIpc.mockResolvedValueOnce(12000)
  const target = await restore.resolveLensFocus(2, () => current)
  expect(target?.y).toBe(11960)
  expect(invokeIpc).toHaveBeenLastCalledWith(IPC.GET_ITEM_Y_BY_ID, { itemId: 7, layoutVersion: 2 })
  expect(focus).not.toHaveBeenCalled()
  target?.afterRestore?.()
  await nextTick()
  current = false
  await vi.advanceTimersByTimeAsync(160)
  expect(grid.querySelector).toHaveBeenCalledTimes(1)
  expect(focus).not.toHaveBeenCalled()
  lens.mode = 'groups'
  await nextTick()
  invokeIpc.mockResolvedValueOnce(null)
  current = true
  const missing = await restore.resolveLensFocus(3, () => current)
  expect(missing?.y).toBe(0)
  scope.stop()
  missing?.afterRestore?.()
  await nextTick()
  expect(focus).not.toHaveBeenCalled()
})
