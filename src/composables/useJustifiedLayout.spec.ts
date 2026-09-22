import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, reactive, ref, type EffectScope } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import type { LayoutSummary } from '../types/layout'
import { IPC } from '../constants/ipc'

const invokeIpc = vi.fn()
vi.mock('../utils/ipc', () => ({ invokeIpc: (...args: unknown[]) => invokeIpc(...args) }))
vi.mock('../utils/logger', () => ({ logger: { info: vi.fn(), warn: vi.fn(), error: vi.fn() } }))
const ui = reactive({ layoutMode: 'justified', gridRowHeight: 200, resizeDebounceMs: 50,
  searchQuery: '', searchScope: 'filename', groupBy: 'date', sortWithinGroup: 'datetime',
  sortOrder: 'desc', seamlessGroups: false })
const view = reactive({ galleryQueryReady: true, activeDirectoryId: null as number | null,
  activeSmartAlbum: 'all', activeCollection: null, activePersonId: null })
vi.mock('../stores/uiStore', () => ({ useUiStore: () => ui }))
vi.mock('../stores/viewStore', () => ({ useViewStore: () => view }))
vi.mock('../stores/filterStore', () => ({ useFilterStore: () => ({ apiFilterKey: '{}', toApiFilter: () => ({}) }) }))
vi.mock('../stores/aiStore', () => ({ useAiStore: () => ({ isSemanticMode: false, semanticLayoutReady: true }) }))
vi.mock('../stores/scanStore', () => ({ useScanStore: () => ({ isAnyScanRunning: false }) }))
vi.mock('../stores/duplicateLensStore', () => ({ useDuplicateLensStore: () => ({ mode: null, showUniqueItems: false }) }))

import { useJustifiedLayout } from './useJustifiedLayout'
import { useMediaStore } from '../stores/mediaStore'

function summary(version: number): LayoutSummary {
  return { totalRows: 1, totalHeight: 100, layoutVersion: version, orderVersion: version, totalItems: 1, separators: [], monthBuckets: [] }
}

let scope: EffectScope
beforeEach(() => {
  setActivePinia(createPinia())
  vi.useFakeTimers()
  vi.stubGlobal('window', { devicePixelRatio: 1 })
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  invokeIpc.mockReset().mockResolvedValue(summary(1))
  view.activeDirectoryId = null
  view.galleryQueryReady = true
  scope = effectScope()
})
afterEach(() => {
  scope.stop()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('画廊布局入口生命周期', () => {
  it('显式调用和尺寸重试遵守前台门，回屏仅补最终视图，销毁后不再请求', async () => {
    const visible = ref(false)
    let width = 800
    const layout = scope.run(() => useJustifiedLayout(() => width, { enabled: () => visible.value }))!
    await layout.compute()
    view.activeDirectoryId = 7
    await nextTick()
    expect(invokeIpc).not.toHaveBeenCalled()
    visible.value = true
    expect(await layout.flushIfDeferred()).toBe(true)
    expect(invokeIpc).toHaveBeenCalledExactlyOnceWith(IPC.COMPUTE_LAYOUT,
      expect.objectContaining({ params: expect.objectContaining({ directoryId: 7 }) }))
    width = 0
    const retry = layout.compute()
    visible.value = false
    width = 1200
    await vi.advanceTimersByTimeAsync(50)
    await retry
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    visible.value = true
    expect(await layout.flushIfDeferred()).toBe(true)
    expect(invokeIpc).toHaveBeenCalledTimes(2)
    scope.stop()
    await layout.compute()
    expect(invokeIpc).toHaveBeenCalledTimes(2)
  })

  it('失败保留待重算，成功只消费本次开始前的脏代，不吞在途新失效', async () => {
    const media = useMediaStore()
    const layout = scope.run(() => useJustifiedLayout(() => 800))!
    media.invalidateLayout()
    invokeIpc.mockRejectedValueOnce(new Error('query failed'))
    await layout.compute()
    expect(media.layoutDirty).toBe(true)
    expect(await layout.flushIfDeferred()).toBe(true)
    expect(media.layoutDirty).toBe(false)
    let finish!: (value: LayoutSummary) => void
    invokeIpc.mockReturnValueOnce(new Promise<LayoutSummary>((resolve) => { finish = resolve }))
    const inFlight = layout.compute()
    media.invalidateLayout()
    finish(summary(2))
    await inFlight
    expect(media.layoutDirty).toBe(true)
  })
})
