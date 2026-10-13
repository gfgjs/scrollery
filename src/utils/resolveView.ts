// 画廊布局与后端视图描述符共用此视图维度决策，避免全选目标集与显示集漂移。
//
// 注意:viewStore 保证四维度互斥（setActiveX 清其余三 + activeSmartAlbum 缺省 'all'）,故 precedence
// 在可达状态下等价于「取唯一被设定的维度」;这里显式保留优先级顺序以对齐两处投影的原控制流。

import type { Collection } from '../types/media'
import type { SmartAlbum } from '../types/ui'

/** resolveView 的输入:viewStore 的四个互斥视图维度快照。 */
export interface ViewState {
  activePersonId: number | null
  activeCollection: Collection | null
  activeSmartAlbum: SmartAlbum
  activeDirectoryId: number | null
}

/**
 * 归一后的当前视图。各投影据此映射到自身形态：
 * - person / collection / directory：JL → 扁平 filters.personId/albumId 或 directoryId param；
 *   VD → scope 判别联合。
 * - systemCollection：两处均落为 filter overlay（mediaTypes=[mediaType] + favoritedOnly）,scope 仍 all。
 * - smartAlbum：favorites/live-photos/recent 落 filter overlay;trash 在 VD 为 scope、JL 为 trashedOnly;
 *   all 为空视图（无额外筛选,directory param 由调用方另接）。
 */
export type ResolvedView =
  | { kind: 'person'; personId: number }
  | { kind: 'collection'; albumId: number }
  | { kind: 'systemCollection'; mediaType: string }
  | { kind: 'directory'; directoryId: number }
  | { kind: 'smartAlbum'; album: SmartAlbum }

/**
 * 按 person > collection > smart-album(favorites/live/recent/trash) > directory > all 的优先级,
 * 归一当前视图维度，供 useJustifiedLayout 与 useViewDescriptor 各自投影。
 */
export function resolveView(v: ViewState): ResolvedView {
  if (v.activePersonId != null) {
    return { kind: 'person', personId: v.activePersonId }
  }
  if (v.activeCollection) {
    const c = v.activeCollection
    // 系统夹 ≈ 类型 + 收藏（语义落在 filter overlay,而非独立 scope）。
    if (c.kind === 'system' && c.mediaTypeFilter) {
      return { kind: 'systemCollection', mediaType: c.mediaTypeFilter }
    }
    return { kind: 'collection', albumId: c.id }
  }
  if (v.activeSmartAlbum === 'favorites') return { kind: 'smartAlbum', album: 'favorites' }
  if (v.activeSmartAlbum === 'live-photos') return { kind: 'smartAlbum', album: 'live-photos' }
  if (v.activeSmartAlbum === 'recent') return { kind: 'smartAlbum', album: 'recent' }
  if (v.activeSmartAlbum === 'trash') return { kind: 'smartAlbum', album: 'trash' }
  // smartAlbum 'all'：有目录选中则降为 directory,否则纯 all 视图。
  if (v.activeDirectoryId != null) {
    return { kind: 'directory', directoryId: v.activeDirectoryId }
  }
  return { kind: 'smartAlbum', album: 'all' }
}
