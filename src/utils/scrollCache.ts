// src/utils/scrollCache.ts
// 模块级滚动位置缓存，跨组件重挂载持久化。用户从画廊→设置→画廊导航时 MediaGrid 组件被销毁重建，
// 但此 Map 存活于 ES 模块作用域（不在组件实例内），故重挂载后仍能读到滚动位置。

export const scrollCache = new Map<string, number>()

/**
 * 画廊滚动缓存键（MediaGrid.getViewKey 委托,单一事实源）。
 * 此前键拼装只存在于 MediaGrid.getViewKey 一处；抽出后由本模块统一维护,防多份拼装漂移。
 *
 * lensMode 非空（重复镜头激活,§8.3）时键形如 `lens-groups`：镜头集合是全库级、与
 * `dir-N`/`album-*` 完全不同的空间,若共用普通键会把镜头滚动位串写进普通画廊的缓存
 * （退出镜头恢复时会跳到镜头的旧位置）。
 *
 * lensShowUniqueItems 仅 folders 镜头有意义（§8.3 scroll cache key 含独有项开关）:
 * true 时键尾追加 `-u1`（`lens-folders-u1`）——开关切换即重生成布局、是两个不同集合,
 * 滚动位必须分条;groups 恒 false 不带后缀（键形稳定,不随无意义维度膨胀）。
 */
export function galleryScrollKey(
  directoryId: number | null,
  smartAlbum: string,
  lensMode?: string | null,
  lensShowUniqueItems?: boolean,
): string {
  if (lensMode) {
    return lensMode === 'folders' && lensShowUniqueItems ? 'lens-folders-u1' : `lens-${lensMode}`
  }
  return directoryId ? `dir-${directoryId}` : `album-${smartAlbum}`
}
