import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { effectScope, reactive, type EffectScope } from 'vue'
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
