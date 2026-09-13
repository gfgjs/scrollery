// 画廊选区批量操作全集(自 MediaGrid.vue 结构拆分抽出,函数体与调用顺序逐字保留)。
//
// 🔴 时序契约(拆分方案 §3.2):
//   batchDelete → stageDeleted →(退出选择态 watcher)→ commitPendingReflow → fadeOutCells →
//   flipReflow(async () => { compute(); updateVisible(); await whenSettled() })
// 这条链的执行顺序是**用户反馈驱动的行为契约**(删除不立即重排,退出选择才一次性重排),
// 连同 `watch(selection.isSelectionMode)` 一起整体搬运,不得拆开到不同文件。
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { invokeIpc, ipcErrorMessage } from '../utils/ipc'
import { useMediaStore } from '../stores/mediaStore'
import { useToastStore } from '../stores/toastStore'
import { useFilterStore } from '../stores/filterStore'
import { useViewStore } from '../stores/viewStore'
import { useHistoryStore } from '../stores/historyStore'
import { useExportStore } from '../stores/exportStore'
import { useCollectionToast } from './useCollectionToast'
import { useGridFlipReflow } from './useGridFlipReflow'
import { useSelection, type BackendSelectionDescriptor } from './useSelection'
import { IPC } from '../constants/ipc'
import {
  Copy,
  FolderPlus,
  FolderInput,
  CheckSquare,
  CopyMinus,
  Heart,
  HeartOff,
  Ban,
  Trash2,
  Palette,
  Download,
} from '@lucide/vue'
import type { LayoutRowItem } from '../types/layout'
import type { DirNode } from '../types/media'
import type { SelectionCommand } from '../types/selectionCommand'

export interface GallerySelectionOpsDeps {
  compute: (width?: number) => Promise<void>
  updateVisible: (force?: boolean) => Promise<void>
  bucketActive: () => boolean
  /** bucket 引擎的「愿望窗口内段全部落地」兑现点(FLIP 的 Last 快照前置)。 */
  whenSettled: () => Promise<void>
  /** FLIP/fadeOut 的查询根:方案 A 用渲染层 layerRef,bucket 模式用段容器。 */
  flipRootEl: () => HTMLElement | null
  patchVisibleRating: (ids: Set<number>, rating: number) => void
  patchVisibleFavorite: (ids: Set<number>, isFavorited: boolean) => void
  patchVisibleColorLabel: (ids: Set<number>, colorLabel: number) => void
  patchVisibleSelected: (apply: (item: LayoutRowItem) => void) => void
}

export function useGallerySelectionOps(deps: GallerySelectionOpsDeps) {
  const { t } = useI18n()
  const media = useMediaStore()
  const toast = useToastStore()
  const filter = useFilterStore()
  const viewStore = useViewStore()
  const history = useHistoryStore()
  const exportStore = useExportStore()
  const collectionToast = useCollectionToast()
  const selection = useSelection()

  const moveCopyDialog = ref({
    isOpen: false,
    mode: 'move' as 'move' | 'copy',
  })

  async function handleFavorite(itemId: number) {
    // 切换收藏并获取新状态
    const newValue = await media.toggleFavorite(itemId)
    // 收藏（而非取消）时，提示加入收藏夹（需求7 §3.7）。
    if (newValue) collectionToast.showAddToCollection([itemId])
    // 修补可见行中的项以即时反馈
    deps.patchVisibleFavorite(new Set([itemId]), newValue)
    // 在收藏视图（或系统收藏夹，本质是 类型 + is_favorited）中，重算布局以移除刚取消收藏的项。
    if (
      viewStore.activeSmartAlbum === 'favorites' ||
      viewStore.activeCollection?.kind === 'system'
    ) {
      await deps.compute()
      deps.updateVisible()
    }
  }

  // 详情页 / 外部单项改 favorite·rating·colorLabel → 回灌画廊 visibleRows（修复:详情设色/评分/收藏
  // 后画廊缩略图不刷新、需手动刷新）。根因：visibleRows 由网格持有（经 fetchRowsByY 拉取），store 侧
  // 无行缓存可改（R2-2 已删 rowCache/patchRowItem 旁路），故由 itemPatchSignal 通知就地回灌 visibleRows。
  // 仅做视觉回灌（幂等，与 in-grid 同步内联 patch 无冲突）；筛选剔除重算仍由各 in-grid handler 负责。
  watch(
    () => media.itemPatchSignal,
    (p) => {
      if (!p) return
      const ids = new Set([p.id])
      if (p.field === 'rating') deps.patchVisibleRating(ids, p.value as number)
      else if (p.field === 'colorLabel') deps.patchVisibleColorLabel(ids, p.value as number)
      else if (p.field === 'isFavorited') deps.patchVisibleFavorite(ids, p.value as boolean)
    },
  )

  // 缩略图 hover 快捷评分（单项）：落库 + 乐观刷新星级。
  // 若「≥N 星」筛选激活且新评分跌破阈值，该项应离开视图 → 重算（镜像收藏视图取消收藏时的重算）。
  async function handleRate(itemId: number, value: number) {
    await media.setRating(itemId, value)
    deps.patchVisibleRating(new Set([itemId]), value)
    if (filter.minRating > 0 && value < filter.minRating) {
      await deps.compute()
      deps.updateVisible()
    }
  }

  // ── 选区批量操作的公共出入口（R1-2/S4）────────────────────────────────────────

  // 选区 → 后端描述符：全选走 selectAll（payload 恒定，id 物化收敛后端）;
  // 语义搜索视图不可 SQL 描述（toBackendDescriptor 返 null）时回退 Explicit 物化。
  function selectionDescriptor(): BackendSelectionDescriptor {
    return selection.toBackendDescriptor() ?? { kind: 'explicit', ids: selection.materializeIds() }
  }

  // 选区批量设色（SelectionToolbar 色块 / Ban 清除触发）：落库 + 乐观刷新 + 必要时重算。
  // value=0 清除。镜像 batchFavorite + handleRate 的「按色筛选下改后不匹配则重算移除」。
  async function batchColor(value: number) {
    if (selection.selectedCount.value === 0) return
    const n = await media.batchSetColorLabel(selectionDescriptor(), value)
    deps.patchVisibleSelected((it) => {
      it.colorLabel = value
    })
    if (filter.colorLabel > 0 && value !== filter.colorLabel) {
      await deps.compute()
      deps.updateVisible()
    }
    toast.addToast(
      'success',
      value === 0
        ? t('selection.colorCleared', { count: n })
        : t('selection.colorSet', { count: n }),
    )
  }

  // ── 批量操作 ──

  // 加入收藏夹（T21）：把选区交给「加入收藏夹」chips 提示（挑已有夹 / 新建）。复用收藏后同款 UX,
  // prefix 传「选中 N 项」（非收藏动作，不显「已收藏」）。
  async function addSelectionToCollection() {
    const ids = selection.materializeIds()
    if (ids.length === 0) return
    await collectionToast.showAddToCollection(ids, t('selection.selected', { count: ids.length }))
  }

  // 导出选区(方案 A §4):选区工具条/右键菜单统一走这一个入口,生成同一 SelectionDescriptor。
  // ViewStale 重试回调(P3):现逻辑原样搬入——重取当前选区(可能因语义搜索等返回 null,由
  // ExportDialog 按「回调返回 null = 重试失败」处理,不再由本函数兜底 fallback 到 materializeIds)。
  function startExportSelection() {
    if (selection.selectedCount.value === 0) return
    exportStore.openExportDialog(selectionDescriptor(), { kind: 'selection' }, () =>
      selection.toBackendDescriptor(),
    )
  }

  async function batchFavorite() {
    if (selection.selectedCount.value === 0) return
    // R1-2/S4：描述符直传（全选百万项 payload 恒定）;受影响计数以后端返回为准。
    const n = await invokeIpc<number>(IPC.BATCH_TOGGLE_FAVORITE, {
      selection: selectionDescriptor(),
      value: true,
    })
    deps.patchVisibleSelected((it) => {
      it.isFavorited = true
    })
    await media.loadStats()
    toast.addToast('success', t('selection.favorited', { count: n }))
  }

  async function batchUnfavorite() {
    if (selection.selectedCount.value === 0) return
    const n = await invokeIpc<number>(IPC.BATCH_TOGGLE_FAVORITE, {
      selection: selectionDescriptor(),
      value: false,
    })
    deps.patchVisibleSelected((it) => {
      it.isFavorited = false
    })
    await media.loadStats()
    toast.addToast('success', t('selection.unfavorited', { count: n }))
  }

  // 删除/移除后的平滑重排动画（FLIP + 淡出）已抽到 useGridFlipReflow（自包含 DOM 工具，仅依赖
  // 一个根元素 getter）。仅用于删除/移除路径，绝不挂到滚动驱动的 updateVisible（避免与虚拟滚动 +
  // renderAnchor 打架）。B2:根引擎感知——方案 A 用渲染层 layerRef,bucket 模式用段容器
  // (两分支卡片同为 [data-item-id],FLIP 按 id 匹配跨段照常成立)。
  const { flipReflow, fadeOutCells } = useGridFlipReflow(() => deps.flipRootEl())

  // ── 暂存删除（置灰 + 退出选择时一次重排 + 撤销）─────────────────────────────────
  // 用户反馈：每次删除立即重算仍闪一下。改为：删除**即落库**（进回收站）但**不立即重排**，
  // 仅把项加入 pendingDeleteIds 置灰、保持选中；待**退出选择模式**（Esc/清空）时一次性
  // fadeOut + FLIP 重排移除全部暂存项。撤销经 toast「撤销」chip → restore_items 恢复。
  const pendingDeleteIds = ref<Set<number>>(new Set())
  function isPendingDelete(id: number): boolean {
    return pendingDeleteIds.value.has(id)
  }

  async function batchDelete() {
    // 跳过已暂存的项（避免对同一选区重复 soft_delete / 重复 toast）。
    const ids = selection.materializeIds().filter((id) => !pendingDeleteIds.value.has(id))
    if (ids.length === 0) return
    // 即落库（进回收站），但不重排——仅置灰暂存，退出选择时统一重排。
    // 破坏性操作必须有失败反馈（审查 R0-5）：落库失败时绝不进暂存集（否则前端置灰、
    // 后端未删，退出选择时重排会「凭空消失」未删除的项），toast 告知后原样保留选区。
    // R1-2 注：删除仍物化 id 后以 Explicit 描述符传参——暂存置灰集/撤销闭包/FLIP 重排都
    // 需要具体 id，SelectAll 深迁移（对全选表达暂存态）属 T18 后续。
    try {
      await invokeIpc(IPC.SOFT_DELETE_ITEMS, { selection: { kind: 'explicit', ids } })
    } catch (e) {
      toast.addToast('error', t('selection.deleteFailed', { error: ipcErrorMessage(e) }))
      return
    }
    stageDeleted(ids)
    // 撤销无时限：登记进 historyStore（会话内长存 + Ctrl+Z 可达），toast 只是即时入口。
    // 此前撤销回调寄生在 toast 对象上，6 秒后随 removeToast 一起蒸发——而后端 restore_items 无时效校验、
    // 全库无 purge，数据本就永久可恢复（真机 round10 #7）。删照片另有回收站作跨重启入口。
    const undoId = history.pushUndoable({
      undo: () => undoDelete(ids),
      redo: () => redoDelete(ids),
      undoMessage: t('selection.restored', { count: ids.length }),
      redoMessage: t('selection.deleted', { count: ids.length }),
    })
    toast.addToast('info', t('selection.deleted', { count: ids.length }), 6000, [
      { label: t('common.undo'), onClick: () => history.undoIfTop(undoId) },
    ])
  }

  /** 把 ids 记入暂存置灰集（删除后项仍在布局里，退出选择时才一次性重排）。 */
  function stageDeleted(ids: number[]) {
    const next = new Set(pendingDeleteIds.value)
    ids.forEach((id) => next.add(id))
    pendingDeleteIds.value = next
  }

  async function undoDelete(ids: number[]) {
    // **不自捕获**：唯一调用方是 historyStore.undo()，它的 catch 会丢弃失效记录并 toast
    // ('common.undoFailed'，与此前这里的自捕获同文案)。抛出去让错误路径唯一，避免「既报错又报成功」
    // （store 见不到异常就会照常弹 undoMessage）。
    await invokeIpc(IPC.RESTORE_ITEMS, { selection: { kind: 'explicit', ids } })
    const stillStaged = ids.some((id) => pendingDeleteIds.value.has(id))
    if (stillStaged) {
      // 仍在暂存（未退出选择）：项还在前端布局里（soft_delete 未触发重算），仅去掉置灰即可。
      // **不重算**——否则 compute() 会把其它仍 is_deleted=1 的暂存项一并移除（误伤）。
      const next = new Set(pendingDeleteIds.value)
      ids.forEach((id) => next.delete(id))
      pendingDeleteIds.value = next
    } else {
      // 已退出选择被重排移除：需重算把恢复的项带回。
      await deps.compute()
      deps.updateVisible()
    }
  }

  /** 重做删除。与首次删除的区别：可能已不在选择态（撤销时重算把项带回了布局），那就得再重算移除。 */
  async function redoDelete(ids: number[]) {
    await invokeIpc(IPC.SOFT_DELETE_ITEMS, { selection: { kind: 'explicit', ids } })
    if (selection.isSelectionMode.value) {
      stageDeleted(ids)
    } else {
      await deps.compute()
      deps.updateVisible()
    }
  }

  // 退出选择模式 → 一次性提交暂存项的重排（fadeOut 暂存项 + 幸存项 FLIP 滑入），随后清空暂存集。
  async function commitPendingReflow() {
    if (pendingDeleteIds.value.size === 0) return
    const ids = Array.from(pendingDeleteIds.value)
    pendingDeleteIds.value = new Set() // 先清空，避免重入
    await fadeOutCells(ids)
    await flipReflow(async () => {
      await deps.compute()
      deps.updateVisible()
      // B2:bucket 模式的行数据在版本换代后异步回填——等愿望窗口内段全部落地,
      // FLIP 的 Last 快照才能读到重排后的真实位置(否则 DOM 为空,动画静默失效)。
      if (deps.bucketActive()) await deps.whenSettled()
    })
  }

  watch(
    () => selection.isSelectionMode.value,
    (now, prev) => {
      // true → false 即「退出选择状态」：把暂存的删除一次性重排掉。
      if (prev && !now) commitPendingReflow()
    },
  )

  function startBatchMove() {
    const ids = selection.materializeIds()
    if (ids.length === 0) return
    moveCopyDialog.value.mode = 'move'
    moveCopyDialog.value.isOpen = true
  }

  function startBatchCopy() {
    const ids = selection.materializeIds()
    if (ids.length === 0) return
    moveCopyDialog.value.mode = 'copy'
    moveCopyDialog.value.isOpen = true
  }

  // 选区批量动作的数据驱动清单(C1):把此前 9 条写死的 @emit 收敛为单一 :commands 数据源传入
  // SelectionToolbar。handler 全是既有函数引用(零迁移);顺序保持改造前 parity(折叠自尾部起,
  // copy→move→delete…最先折入 ⋯ 菜单)。icon/labelKey 在此单源声明,条上 tooltip 与
  // 折叠后菜单行三处共用。「✕ 取消选择 / 拖拽手柄 / 计数」是壳的固定件,不进本数组、不参与折叠。
  const selectionCommands: SelectionCommand[] = [
    {
      key: 'selectAll',
      icon: CheckSquare,
      labelKey: 'common.selectAll',
      run: () => selection.selectAll(),
    },
    {
      key: 'invert',
      icon: CopyMinus,
      labelKey: 'selection.invert',
      run: () => selection.invertSelection(),
    },
    {
      key: 'favorite',
      icon: Heart,
      labelKey: 'selection.favorite',
      run: () => batchFavorite(),
    },
    {
      key: 'unfavorite',
      icon: HeartOff,
      labelKey: 'selection.unfavorite',
      run: () => batchUnfavorite(),
    },
    {
      key: 'addToCollection',
      icon: FolderPlus,
      labelKey: 'selection.addToCollection',
      run: () => addSelectionToCollection(),
    },
    {
      key: 'export',
      icon: Download,
      labelKey: 'selection.export',
      run: () => startExportSelection(),
    },
    // colors 单元:点色块即 run(色值);Ban(clearColor)走独立按钮 run(0)。icon(Palette)/labelKey 供折叠后菜单行用。
    {
      key: 'colors',
      icon: Palette,
      labelKey: 'selection.colorLabel',
      kind: 'colors',
      run: (v) => batchColor(v ?? 0),
    },
    {
      key: 'clearColor',
      icon: Ban,
      labelKey: 'selection.clearColor',
      run: () => batchColor(0),
    },
    {
      key: 'delete',
      icon: Trash2,
      labelKey: 'selection.delete',
      danger: true,
      run: () => batchDelete(),
    },
    // move 组首:其前渲染 divider(与 copy 同属「移动/复制」折叠单元)。
    {
      key: 'move',
      icon: FolderInput,
      labelKey: 'common.moveTo',
      groupStart: true,
      run: () => startBatchMove(),
    },
    {
      key: 'copy',
      icon: Copy,
      labelKey: 'common.copyTo',
      run: () => startBatchCopy(),
    },
  ]

  async function onMoveCopyConfirm(targetNode: DirNode | null) {
    const ids = selection.materializeIds()
    // targetNode 为 FolderTreeSelectorDialog 选中的 DirNode,以 id（目录 id）为落点。
    if (ids.length === 0 || targetNode?.id == null) return
    moveCopyDialog.value.isOpen = false

    // T6：经 historyStore 走 relocate_media_items / copy_media_items_db（DB 级、可撤销），
    // 与拖图落点 performMediaDrop 同一路径（DRY）。history 内部 refresh() 已重载文件夹树（实时计数）
    // + 重算网格,故不再手动 compute/loadStats/计数调整;copy 走 _db 直接建行,无需再 startScan 重扫。
    const mode = moveCopyDialog.value.mode
    try {
      const n =
        mode === 'copy'
          ? await history.copyMedia(ids, targetNode.id, `复制 ${ids.length} 项`)
          : await history.moveMedia(ids, targetNode.id, `移动 ${ids.length} 项`)
      if (n > 0) {
        // 移动后源项已离开当前视图 → 清选区;复制保留选区（项仍在原处）。
        if (mode === 'move') selection.clearSelection()
        toast.addToast(
          'success',
          mode === 'copy'
            ? t('common.copiedCount', { count: n })
            : t('common.movedCount', { count: n }),
        )
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      toast.addToast(
        'error',
        mode === 'copy'
          ? t('common.copyFailed', { error: msg })
          : t('common.moveFailed', { error: msg }),
      )
    }
  }

  return {
    moveCopyDialog,
    handleFavorite,
    handleRate,
    selectionDescriptor,
    batchColor,
    pendingDeleteIds,
    isPendingDelete,
    startBatchMove,
    startBatchCopy,
    startExportSelection,
    selectionCommands,
    onMoveCopyConfirm,
  }
}
