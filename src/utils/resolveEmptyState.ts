// src/utils/resolveEmptyState.ts
// 画廊「空状态」的**单一决策源**（S5 阶段11）。纯函数,无 store / i18n / Vue 依赖。
//
// 背景:空状态「显示哪条消息 + 提供什么下一步动作」的 precedence 此前内联在 MediaGrid.vue 的
// emptyStateText / showEmptyAction 两个 computed 里（藏在 2290 行组件中）,存在一个真实的错 CTA
// 缺陷:全局筛选态激活却零结果时,消息落到「空库 · 添加文件夹」,把用户引向加目录——而正确的
// 下一步是**清除筛选**(设计 §7.1「所有 empty state 都包含下一步:…清除筛选…」)。这里把决策抽为
// 纯函数并穷举 spec 锁死,呈现层(t() 解 key / 图标组件 / handler)留在组件。
//
// 决策只产出 i18n **key** 与动作**枚举**,不产出渲染好的字符串/图标组件——保持纯、可测、与 i18n 解耦。

import type { SmartAlbum } from '../types/ui'

/** resolveGalleryEmptyState 的输入:空状态呈现所需的视图/筛选维度快照。 */
export interface EmptyStateInput {
  /** 文件名搜索的已提交查询（空串=无搜索）。 */
  searchQuery: string
  /** 是否存在激活的结构化筛选(类型/收藏/实况/评分/色标/日期,见 filterStore.hasActiveFilters)。 */
  hasActiveFilters: boolean
  activeDirectoryId: number | null
  activePersonId: number | null
  activeSmartAlbum: SmartAlbum
}

/** 空状态的「下一步动作」种类。null=该场景不提供动作。 */
export type EmptyStateActionKind = 'add-folder' | 'clear-filters'

/** 归一后的空状态描述:消息 key + 可选说明 key + 插值参数 + 下一步动作。 */
export interface EmptyStateDescriptor {
  /** 标题文案的 i18n key。 */
  titleKey: string
  /** 说明文案的 i18n key;null=无说明(单行空状态)。 */
  descKey: string | null
  /** 消息插值参数(仅搜索场景带 { query });null=无。 */
  params: Record<string, string> | null
  /** 下一步动作;null=不提供。 */
  action: EmptyStateActionKind | null
}

/**
 * 按 search > filtered > directory > smartAlbum 的优先级归一空状态。
 * 与 MediaGrid 原 emptyStateText / showEmptyAction 控制流一一对应,并修正 filtered-empty 错 CTA。
 */
export function resolveGalleryEmptyState(input: EmptyStateInput): EmptyStateDescriptor {
  // 1) 文件名搜索无结果:最具体的用户意图(用户主动键入查询),优先于一切。
  if (input.searchQuery !== '') {
    return {
      titleKey: 'empty.search',
      descKey: null,
      params: { query: input.searchQuery },
      action: null,
    }
  }
  // 2) 🔴 筛选无匹配(修错 CTA):置于视图维度**之上**。无论身处哪个视图,激活的筛选都是
  //    「藏住照片」的直接原因,「清除筛选」是通用逃生口——filter 是全局持久态,切视图不清,极易
  //    留下 stale filter 静默藏空(在文件夹里看不到照片,却不知是上个视图残留的筛选)。说明文案
  //    不宣称当前视图有照片,只建议清除筛选,故对「真空 + 恰有 stale filter」也诚实。
  if (input.hasActiveFilters) {
    return {
      titleKey: 'empty.filteredTitle',
      descKey: 'empty.filteredDesc',
      params: null,
      action: 'clear-filters',
    }
  }
  // 3) 目录视图空(无筛选)。
  if (input.activeDirectoryId != null) {
    return { titleKey: 'empty.folder', descKey: null, params: null, action: null }
  }
  // 4) 智能相册。
  const album = input.activeSmartAlbum
  if (album === 'all') {
    // 空库「下一步」=添加文件夹;仅无人物过滤时出(人物视图恰为 album='all' 时不引导加目录,
    // 对齐原 showEmptyAction 的 activePersonId==null 门控)。
    return {
      titleKey: 'empty.allPhotosTitle',
      descKey: 'empty.allPhotosDesc',
      params: null,
      action: input.activePersonId == null ? 'add-folder' : null,
    }
  }
  if (album === 'recent') {
    return { titleKey: 'empty.recentlyAdded', descKey: null, params: null, action: null }
  }
  if (album === 'favorites') {
    return {
      titleKey: 'empty.favoritesTitle',
      descKey: 'empty.favoritesDesc',
      params: null,
      action: null,
    }
  }
  if (album === 'live-photos') {
    return { titleKey: 'empty.livePhotos', descKey: null, params: null, action: null }
  }
  // 默认兜底(如 trash):沿用「空库」文案但不出加目录动作(对齐原 showEmptyAction 仅 album==='all' 出)。
  return {
    titleKey: 'empty.allPhotosTitle',
    descKey: 'empty.allPhotosDesc',
    params: null,
    action: null,
  }
}
