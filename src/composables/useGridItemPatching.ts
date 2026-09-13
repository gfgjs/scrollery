// 画廊可视项就地 patch(乐观 UI 回写)。自 MediaGrid.vue 结构拆分抽出,逻辑逐字保留。
//
// 乐观更新：把若干项的字段就地写回可见布局行,使变更即时显形。
// visibleRows 是深响应式 ref（仅可视窗口几十项，非百万级），改 item 字段直接触发那一格
// 重渲染，无需 triggerRef —— 与收藏/缩略图回写同一机制。canvas 模式靠 bumpCanvasPatchTick 通知重绘。
import type { LayoutRow, LayoutRowItem } from '../types/layout'

export interface GridItemPatchingDeps {
  activeRows: () => LayoutRow[]
  bumpCanvasPatchTick: () => void
  isSelected: (id: number) => boolean
}

export function useGridItemPatching(deps: GridItemPatchingDeps) {
  function patchVisibleRating(ids: Set<number>, rating: number) {
    // 用判别字段 rowType 收窄到 LayoutRowNormal（其 items 已正确类型化），避免周边的 (row as any) 写法。
    for (const row of deps.activeRows()) {
      if (row.rowType !== 'normal') continue
      for (const item of row.items) {
        if (ids.has(item.id)) item.rating = rating
      }
    }
    deps.bumpCanvasPatchTick()
  }

  // 乐观更新收藏态（镜像 patchVisibleRating）。供 in-grid 收藏 + 详情页回灌信号共用。
  function patchVisibleFavorite(ids: Set<number>, isFavorited: boolean) {
    for (const row of deps.activeRows()) {
      if (row.rowType !== 'normal') continue
      for (const item of row.items) {
        if (ids.has(item.id)) item.isFavorited = isFavorited
      }
    }
    deps.bumpCanvasPatchTick()
  }

  // 乐观更新颜色标签（镜像 patchVisibleRating）。
  function patchVisibleColorLabel(ids: Set<number>, colorLabel: number) {
    for (const row of deps.activeRows()) {
      if (row.rowType !== 'normal') continue
      for (const item of row.items) {
        if (ids.has(item.id)) item.colorLabel = colorLabel
      }
    }
    deps.bumpCanvasPatchTick()
  }

  // 批量路径的乐观刷新：对选区内**可见**项打补丁。SelectAll 态不物化 id，以 isSelected
  // 谓词判定（可见窗口仅几十项，逐项判定廉价）;单项路径仍用上面的 Set 版补丁函数。
  function patchVisibleSelected(apply: (item: LayoutRowItem) => void) {
    for (const row of deps.activeRows()) {
      if (row.rowType !== 'normal') continue
      for (const item of row.items) {
        if (deps.isSelected(item.id)) apply(item)
      }
    }
    deps.bumpCanvasPatchTick()
  }

  return {
    patchVisibleRating,
    patchVisibleFavorite,
    patchVisibleColorLabel,
    patchVisibleSelected,
  }
}
