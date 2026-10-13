import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, ref } from 'vue'
import { useGallerySelectionOps } from './useGallerySelectionOps'
import { useGridItemPatching } from './useGridItemPatching'
import type { LayoutRow, LayoutRowItem } from '../types/layout'

const { batchSetColorLabel, toggleFavorite, setRating, loadStats, invokeIpc, addToast, moveMedia, clearSelection, filterState, viewState } = vi.hoisted(() => ({
  batchSetColorLabel: vi.fn(), toggleFavorite: vi.fn(), setRating: vi.fn(), loadStats: vi.fn(), invokeIpc: vi.fn(), addToast: vi.fn(),
  moveMedia: vi.fn(), clearSelection: vi.fn(),
  filterState: { colorLabel: 0, minRating: 0, favoritedOnly: false },
  viewState: { activeSmartAlbum: 'all', activeCollection: null as { kind: 'system' } | null },
}))
let selectedId = 1
const selectionEpoch = ref(0)
const itemPatchSignal = ref<{ id: number; field: 'isFavorited' | 'rating' | 'colorLabel'; value: boolean | number; seq: number } | null>(null)
vi.mock('../stores/mediaStore', () => ({ useMediaStore: () => ({
  batchSetColorLabel, toggleFavorite, setRating, loadStats,
  get itemPatchSignal() { return itemPatchSignal.value },
}) }))
vi.mock('../stores/toastStore', () => ({ useToastStore: () => ({ addToast }) }))
vi.mock('../stores/filterStore', () => ({ useFilterStore: () => filterState }))
vi.mock('../stores/viewStore', () => ({ useViewStore: () => viewState }))
vi.mock('../utils/ipc', () => ({ invokeIpc, ipcErrorMessage: (error: Error) => error.message }))
vi.mock('../stores/historyStore', () => ({ useHistoryStore: () => ({ moveMedia }) }))
vi.mock('../stores/exportStore', () => ({ useExportStore: () => ({}) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('./useCollectionToast', () => ({ useCollectionToast: () => ({}) }))
vi.mock('./useGridFlipReflow', () => ({ useGridFlipReflow: () => ({}) }))
vi.mock('./useSelection', () => ({
  useSelection: () => ({
    isSelectionMode: ref(true), selectedCount: ref(1),
    selectionEpoch, clearSelection, materializeIds: () => [selectedId],
    toBackendDescriptor: () => ({ kind: 'explicit', ids: [selectedId] }),
  }),
}))

beforeEach(() => {
  vi.resetAllMocks()
  selectedId = 1
  selectionEpoch.value = 0
  itemPatchSignal.value = null
  toggleFavorite.mockImplementation(async (id: number) => {
    itemPatchSignal.value = { id, field: 'isFavorited', value: false, seq: 1 }
    return false
  })
  setRating.mockImplementation(async (id: number, value: number) => {
    itemPatchSignal.value = { id, field: 'rating', value, seq: 1 }
  })
  filterState.colorLabel = 0
  filterState.minRating = 0
  filterState.favoritedOnly = false
  viewState.activeSmartAlbum = 'all'
  viewState.activeCollection = null
})

function favoriteFixture() {
  const original = { id: 1, isFavorited: true, rating: 4, colorLabel: 4 } as LayoutRowItem
  const other = { id: 2, isFavorited: true, rating: 4, colorLabel: 4 } as LayoutRowItem
  const row: LayoutRow = { rowType: 'normal', y: 0, height: 100, items: [original, other] }
  // 模拟后端重取已不再匹配标记条件的资产；仅改图标不会使布局行离开。
  const compute = vi.fn(async () => { row.items = [other] })
  const bumpCanvasPatchTick = vi.fn()
  const patching = useGridItemPatching({
    activeRows: () => [row], isSelected: (id) => id === selectedId, bumpCanvasPatchTick,
    readContext: () => ({ selectionEpoch: 1, layoutVersion: 1, viewKey: 'favorites', semanticKey: 'favorites' }),
    compute,
  })
  const scope = effectScope()
  const ops = scope.run(() => useGallerySelectionOps({
    ...patching,
    compute, whenSettled: async () => {}, flipRootEl: () => null,
  }))!
  return { row, compute, bumpCanvasPatchTick, scope, ops }
}

describe('画廊批量标记回写归属', () => {
  it('移动回包前改选其它资产时保留新选区，成功提示仍对应已完成的移动', async () => {
    let finish!: (count: number) => void
    moveMedia.mockReturnValue(new Promise<number>((resolve) => { finish = resolve }))
    const { scope, ops } = favoriteFixture()
    try {
      ops.startBatchMove()
      const pending = ops.onMoveCopyConfirm({ id: 9 } as import('../types/media').DirNode)
      expect(ops.moveCopyDialog.value.isOpen).toBe(false)
      expect(moveMedia).toHaveBeenCalledWith([1], 9, '移动 1 项')

      selectedId = 2
      selectionEpoch.value++
      finish(1)
      await pending

      expect(clearSelection).not.toHaveBeenCalled()
      expect(ops.selectionDescriptor()).toEqual({ kind: 'explicit', ids: [2] })
      expect(addToast).toHaveBeenCalledWith('success', 'common.movedCount')
    } finally {
      scope.stop()
    }
  })

  it('设色回包前改选其它资产时不误改新选区，而是重取当前视图', async () => {
    let finish!: (count: number) => void
    batchSetColorLabel.mockReturnValue(new Promise<number>((resolve) => { finish = resolve }))
    addToast.mockReset()
    selectedId = 1
    let selectionEpoch = 1
    const original = { id: 1, colorLabel: 0 } as LayoutRowItem
    const other = { id: 2, colorLabel: 0 } as LayoutRowItem
    const rows: LayoutRow[] = [{ rowType: 'normal', y: 0, height: 100, items: [original, other] }]
    const compute = vi.fn(async () => { original.colorLabel = 3 })
    const patching = useGridItemPatching({
      activeRows: () => rows,
      bumpCanvasPatchTick: vi.fn(),
      isSelected: (id) => id === selectedId,
      readContext: () => ({ selectionEpoch, layoutVersion: 1, viewKey: 'all', semanticKey: 'all' }),
      compute,
    })
    const scope = effectScope()
    const ops = scope.run(() => useGallerySelectionOps({
      ...patching,
      compute, whenSettled: async () => {}, flipRootEl: () => null,
    }))!
    try {
      const pending = ops.batchColor(3)
      expect(batchSetColorLabel).toHaveBeenCalledWith({ kind: 'explicit', ids: [1] }, 3)
      selectedId = 2
      selectionEpoch++
      finish(1)
      await pending
      expect(other.colorLabel).toBe(0)
      expect(original.colorLabel).toBe(3)
      expect(compute).toHaveBeenCalledTimes(1)
      expect(addToast).toHaveBeenCalledWith('success', 'selection.colorSet')
    } finally {
      scope.stop()
    }
  })
})
