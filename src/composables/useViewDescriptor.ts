// 当前画廊视图到后端 ViewDescriptor 的装配点。
//
// 🔴 与 useJustifiedLayout.compute() 的扁平 filters 装配**一一对应**（同一 UI 状态、两种投影：
// compute 喂扁平 MediaFilter 给 compute_layout；本函数拆成 scope + GalleryFilter 给 SelectAll
// 解析）。任何一侧新增视图维度（新的智能相册 / scope / 筛选字段），必须同步另一侧，否则
// 「全选批量操作的目标集」会与「画廊实际显示集」漂移（D1 单一事实源告诫）。

import { useMediaStore } from '../stores/mediaStore'
import { useFilterStore } from '../stores/filterStore'
import { useUiStore } from '../stores/uiStore'
import { useViewStore } from '../stores/viewStore'
import { useAiStore } from '../stores/aiStore'
import { resolveView } from '../utils/resolveView'
import type { GalleryFilterDto, ViewDescriptorDto, ViewScopeDto } from '../types/view'

/**
 * 构造描述「当前画廊视图」的 ViewDescriptor（供 SelectAll 选区跨 IPC 传后端解析）。
 *
 * @returns 视图描述符；语义搜索模式返回 null（后端 view_to_sql 拒绝 SemanticSearch，
 *   调用方须回退 Explicit 物化路径）。
 */
export function buildCurrentViewDescriptor(): ViewDescriptorDto | null {
  const media = useMediaStore()
  const filter = useFilterStore()
  const ui = useUiStore()
  const viewStore = useViewStore()
  const ai = useAiStore()

  // 语义搜索：有序、非纯 SQL（v1 走 ai_search 既有路径），不可 SQL 描述。
  if (ai.isSemanticMode) return null

  const f: GalleryFilterDto = { ...filter.toApiFilter() }
  let scope: ViewScopeDto = { kind: 'all' }

  // resolveView 与布局计算共用维度优先级；此处只投影到 scope 与 filter。
  const resolved = resolveView({
    activePersonId: viewStore.activePersonId,
    activeCollection: viewStore.activeCollection,
    activeSmartAlbum: viewStore.activeSmartAlbum,
    activeDirectoryId: viewStore.activeDirectoryId,
  })
  switch (resolved.kind) {
    case 'person':
      scope = { kind: 'person', personId: resolved.personId }
      break
    case 'collection':
      scope = { kind: 'collection', albumId: resolved.albumId }
      break
    case 'systemCollection':
      // 系统夹 ≈ 类型 + 收藏（scope 仍为 all，语义落在 filter）。
      f.mediaTypes = [resolved.mediaType]
      f.favoritedOnly = true
      break
    case 'smartAlbum':
      if (resolved.album === 'favorites') f.favoritedOnly = true
      else if (resolved.album === 'live-photos') f.livePhotoOnly = true
      else if (resolved.album === 'recent') f.recentOnly = true
      else if (resolved.album === 'trash') scope = { kind: 'trash' }
      // 'all' → scope 保持 all
      break
    case 'directory':
      scope = { kind: 'directory', directoryId: resolved.directoryId }
      break
  }

  if (ui.searchQuery && ui.searchQuery.trim() !== '') {
    f.searchQuery = ui.searchQuery.trim()
    f.searchScope = ui.searchScope
  }

  // 重复镜头态：本描述符**有意**不带 duplicateLens。镜头激活时全部消费入口已被 browse-only
  // gate 阻断（§8.1）——选区被清空且无任何再选中入口（Ctrl+A/框选/右键/卡片勾选全阻断），
  // 「导出当前视图」按钮在镜头态不渲染（GalleryViewControls），故本函数在镜头下不可达。
  // 镜头态的批量/导出入口受 gate 阻断，故此描述符不携带 duplicateLens。
  return {
    scope,
    filter: f,
    sort: {
      groupBy: ui.groupBy,
      sortWithinGroup: ui.sortWithinGroup,
      sortOrder: ui.sortOrder,
    },
    layoutVersion: media.layoutVersion,
  }
}
