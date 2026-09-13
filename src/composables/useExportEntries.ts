// src/composables/useExportEntries.ts
// U-2(方案 A):相册卡片右键「导出相册」+ 当前视图工具栏「导出当前视图」共用的 descriptor 组装
// 逻辑,从组件里抽出以保持组件单一职责——CollectionsView/GalleryViewControls 只管 UI 触发,不关心
// SelectionDescriptor/ExportSource 的具体形状。
//
// 两个入口都产出 SelectAll{view, excludedIds:[]}(整相册 / 整视图,不受调用组件自身选区态影响)+
// 对应 ExportSource,交 exportStore.openExportDialog 唤出既有对话框(同选区工具条入口的既有姿态,
// ExportDialog.vue 不感知调用方——MediaGrid.vue:1535 startExportSelection 同款)。

import { useMediaStore } from '../stores/mediaStore'
import { useUiStore } from '../stores/uiStore'
import { useViewIds } from './useViewIds'
import { buildCurrentViewDescriptor } from './useViewDescriptor'
import type { BackendSelectionDescriptor } from './useSelection'
import type { ExportSource } from '../stores/exportStore'
import type { ViewDescriptorDto } from '../types/view'
import type { Collection } from '../types/media'

export interface ExportEntryPayload {
  selection: BackendSelectionDescriptor
  source: ExportSource
}

export function useExportEntries() {
  const media = useMediaStore()
  const ui = useUiStore()
  const viewIds = useViewIds()

  /**
   * 相册卡片右键「导出相册」:整相册(scope=collection),不叠加当前画廊的筛选/搜索状态——
   * 发起点在 CollectionsView,与「当前正显示的画廊视图」是两件事(D1 单一事实源只约束同一视图的
   * 两种投影,不要求与己无关的相册卡片继承筛选态)。排序沿用当前 UI 排序偏好(groupBy/
   * sortWithinGroup/sortOrder),与 buildCurrentViewDescriptor 同源,不另造一套排序默认值。
   *
   * 防御(与 CollectionsView.onCardContextMenu 早退同一 why,双保险——调用方门控失守时本函数仍拒绝):
   * 系统夹不承载 album_items,scope=collection 的相册导出对它会错集;系统夹的导出路径是打开后用
   * 工具栏「导出当前视图」。非 'user' 一律拒绝(返回 null),调用方须判空。
   */
  function buildAlbumExportPayload(c: Collection): ExportEntryPayload | null {
    if (c.kind !== 'user') return null
    const view: ViewDescriptorDto = {
      scope: { kind: 'collection', albumId: c.id },
      filter: {},
      sort: { groupBy: ui.groupBy, sortWithinGroup: ui.sortWithinGroup, sortOrder: ui.sortOrder },
      layoutVersion: media.layoutVersion,
    }
    return {
      selection: { kind: 'selectAll', view, excludedIds: [] },
      source: { kind: 'album', id: c.id, name: c.name },
    }
  }

  /**
   * 工具栏「导出当前视图」:按当前筛选/排序/搜索状态导出全部命中项(非当前选区)。语义搜索模式下
   * buildCurrentViewDescriptor 返回 null(后端 view_to_sql 拒绝解析非纯 SQL 视图)——回退到已物化的
   * 当前视图布局序全集(同 useSelection.materializeIds 对 all 态语义搜索场景的既有回退姿态)。
   * @param viewName 供 manifest 记录的来源名(由调用方按 i18n 传入,本函数不持有 i18n 依赖)。
   */
  function buildCurrentViewExportPayload(viewName: string): ExportEntryPayload {
    const view = buildCurrentViewDescriptor()
    const selection: BackendSelectionDescriptor = view
      ? { kind: 'selectAll', view, excludedIds: [] }
      : { kind: 'explicit', ids: [...viewIds.allIds()] }
    return {
      selection,
      source: { kind: 'view', name: viewName },
    }
  }

  return { buildAlbumExportPayload, buildCurrentViewExportPayload }
}
