// src/utils/viewRoute.ts
// 视图维度 ↔ URL path 的映射（S2-c 视图路由化）。纯函数、无 Vue/store 依赖,便于单测。
//
// 设计要点(见 findings 会话续23):
// - smart-album(5 档)与 folder 筛选态(模式B,groupBy≠folder)各自占一条可寻址路径 → 深链/刷新可恢复。
// - folder 滚动锚点态(模式A,groupBy=folder 点文件夹设 pendingScrollDirId)**不进 URL**——它是全库单列表
//   内的滚动位置而非视图,进 URL 会「每滚一下改一次 history」。故本模块只表达 smart-album + folder 筛选。
// - collection/person 详情路由(/collections/:id、/persons/:id)由 App.vue watcher 专门分支处理(需实体
//   元数据/异步加载),不在本模块的映射范围;本模块只覆盖「主库血统」的 smart-album + folder 视图。

import type { SmartAlbum } from '../types/ui'

/** smart-album → 路径(单一真源)。all 落主库根 '/',其余各占独立路径。 */
const SMART_ALBUM_PATHS: Record<SmartAlbum, string> = {
  all: '/',
  favorites: '/favorites',
  'live-photos': '/live-photos',
  recent: '/recent',
  trash: '/trash',
}

/** 路径 → smart-album 的反查表,由上表机械反转(避免两处各列一遍导致漂移)。 */
const PATH_TO_SMART_ALBUM: Record<string, SmartAlbum> = Object.fromEntries(
  Object.entries(SMART_ALBUM_PATHS).map(([album, path]) => [path, album]),
) as Record<string, SmartAlbum>

/** smart-album → 导航目标路径(供侧栏点击 router.push)。 */
export function smartAlbumToPath(album: SmartAlbum): string {
  return SMART_ALBUM_PATHS[album]
}

/** folder 筛选态 → 导航目标路径(模式B,可寻址)。 */
export function folderToPath(id: number): string {
  return `/folder/${id}`
}

/** 画廊路径解析出的视图维度(供 App.vue watcher 从 route 回填 viewStore)。 */
export type RouteView =
  | { kind: 'smartAlbum'; album: SmartAlbum }
  | { kind: 'directory'; id: number }

/**
 * 把画廊 path 解析为它表达的视图维度。命中 = smart-album(5 档)或 folder 筛选态(/folder/<数字>);
 * 非此类路径(collection/person 详情、查看器、设置等)返回 null。
 * folder 仅匹配纯数字 id;非数字 /folder/xxx 视为不可解析 → null(防御式,对齐 galleryQuery 姿态)。
 */
export function routeToView(path: string): RouteView | null {
  const album = PATH_TO_SMART_ALBUM[path]
  if (album) return { kind: 'smartAlbum', album }
  const m = path.match(/^\/folder\/(\d+)$/)
  if (m) return { kind: 'directory', id: Number(m[1]) }
  return null
}

/**
 * 「主库血统」画廊路由判据 = 能被 routeToView 解析的路径(smart-album + folder 筛选)。
 * 这恰是 S2-c 拆分前全部停在 '/' 的视图集合——供 SemanticSearchPanel 等原先绑 `route.path === '/'`
 * 的 UI 平移到新的多路径结构而行为不变(collection/person 详情此前即非 '/'、不在此列)。
 */
export function isPrimaryGalleryRoute(path: string): boolean {
  return routeToView(path) !== null
}
