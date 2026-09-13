// resolveGalleryEmptyState 单测（S5 阶段11）。空状态「消息 + 下一步动作」决策的唯一源,
// 穷举 precedence(search > filtered > directory > smartAlbum)与动作门控:
//   · add-folder 仅 album='all' 且无人物过滤;
//   · clear-filters 仅 hasActiveFilters(含 🔴 filtered-empty 修错 CTA:此前误落「空库/添加文件夹」)。

import { describe, it, expect } from 'vitest'
import { resolveGalleryEmptyState, type EmptyStateInput } from './resolveEmptyState'

const BASE: EmptyStateInput = {
  searchQuery: '',
  hasActiveFilters: false,
  activeDirectoryId: null,
  activePersonId: null,
  activeSmartAlbum: 'all',
}

describe('resolveGalleryEmptyState（空状态决策单一源）', () => {
  it('默认 all(无搜索/筛选/目录/人物)→ 空库文案 + 添加文件夹', () => {
    expect(resolveGalleryEmptyState(BASE)).toEqual({
      titleKey: 'empty.allPhotosTitle',
      descKey: 'empty.allPhotosDesc',
      params: null,
      action: 'add-folder',
    })
  })

  it('搜索无结果 → search 文案(带 query),无动作;优先于筛选/目录', () => {
    expect(
      resolveGalleryEmptyState({
        ...BASE,
        searchQuery: 'cat',
        hasActiveFilters: true,
        activeDirectoryId: 5,
      }),
    ).toEqual({
      titleKey: 'empty.search',
      descKey: null,
      params: { query: 'cat' },
      action: null,
    })
  })

  it('🔴 筛选无匹配 → filtered 文案 + 清除筛选(修错 CTA,不再引导加目录)', () => {
    expect(resolveGalleryEmptyState({ ...BASE, hasActiveFilters: true })).toEqual({
      titleKey: 'empty.filteredTitle',
      descKey: 'empty.filteredDesc',
      params: null,
      action: 'clear-filters',
    })
  })

  it('筛选激活优先于目录视图(stale filter 藏空文件夹不误显「folder is empty」)', () => {
    expect(
      resolveGalleryEmptyState({ ...BASE, hasActiveFilters: true, activeDirectoryId: 9 }),
    ).toEqual({
      titleKey: 'empty.filteredTitle',
      descKey: 'empty.filteredDesc',
      params: null,
      action: 'clear-filters',
    })
  })

  it('目录视图空(无筛选)→ folder 文案,无动作', () => {
    expect(resolveGalleryEmptyState({ ...BASE, activeDirectoryId: 3 })).toEqual({
      titleKey: 'empty.folder',
      descKey: null,
      params: null,
      action: null,
    })
  })

  it('人物视图恰为 album=all → 空库文案但**不出**加目录动作', () => {
    expect(resolveGalleryEmptyState({ ...BASE, activePersonId: 7 })).toEqual({
      titleKey: 'empty.allPhotosTitle',
      descKey: 'empty.allPhotosDesc',
      params: null,
      action: null,
    })
  })

  it.each([
    ['recent', 'empty.recentlyAdded'],
    ['live-photos', 'empty.livePhotos'],
  ] as const)('智能相册 %s → 对应单行文案,无动作', (album, titleKey) => {
    expect(resolveGalleryEmptyState({ ...BASE, activeSmartAlbum: album })).toEqual({
      titleKey,
      descKey: null,
      params: null,
      action: null,
    })
  })

  it('收藏相册空 → favorites 文案(标题+说明),无动作', () => {
    expect(resolveGalleryEmptyState({ ...BASE, activeSmartAlbum: 'favorites' })).toEqual({
      titleKey: 'empty.favoritesTitle',
      descKey: 'empty.favoritesDesc',
      params: null,
      action: null,
    })
  })

  it('回收站(trash)兜底 → 沿用空库文案但不出加目录动作', () => {
    expect(resolveGalleryEmptyState({ ...BASE, activeSmartAlbum: 'trash' })).toEqual({
      titleKey: 'empty.allPhotosTitle',
      descKey: 'empty.allPhotosDesc',
      params: null,
      action: null,
    })
  })
})
