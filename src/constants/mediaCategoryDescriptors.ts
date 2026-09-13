// 四类媒体的共享展示元数据；共同筛选状态由 filterStore 持有，文件树只投影这些描述符。

import type { Component } from 'vue'
import { File as FileIcon, FileText, Image as ImageIcon, Music, Video } from '@lucide/vue'
import type { TreeCategory } from '../types/media'

export type KnownTreeCategory = Exclude<TreeCategory, 'other'>

export interface MediaCategoryDescriptor {
  readonly id: KnownTreeCategory
  readonly icon: Component
  readonly labelKey: string
}

/** 顶栏与文件树共用的四类展示顺序、图标和文案 key。 */
export const MEDIA_CATEGORY_DESCRIPTORS = [
  { id: 'image', icon: ImageIcon, labelKey: 'toolbar.filterImages' },
  { id: 'video', icon: Video, labelKey: 'toolbar.filterVideos' },
  { id: 'document', icon: FileText, labelKey: 'toolbar.filterDocuments' },
  { id: 'audio', icon: Music, labelKey: 'toolbar.filterAudios' },
] as const satisfies readonly MediaCategoryDescriptor[]

export interface TreeCategoryDescriptor {
  readonly id: TreeCategory
  readonly icon: Component
  readonly labelKey: string
  readonly hintKey?: string
}

/** 文件树专用的“其它”描述符；它不是图库 MediaType。 */
export const TREE_OTHER_CATEGORY_DESCRIPTOR = {
  id: 'other',
  icon: FileIcon,
  labelKey: 'sidebar.treeCategoryOther',
  hintKey: 'sidebar.treeCategoryOtherHint',
} as const satisfies TreeCategoryDescriptor

/** 文件树的五类选项；`other` 是树专用项，不可传给顶栏 MediaType。 */
export const TREE_CATEGORY_DESCRIPTORS = [
  ...MEDIA_CATEGORY_DESCRIPTORS,
  TREE_OTHER_CATEGORY_DESCRIPTOR,
] as const satisfies readonly TreeCategoryDescriptor[]

export const ALL_TREE_CATEGORIES: readonly TreeCategory[] = TREE_CATEGORY_DESCRIPTORS.map(
  (descriptor) => descriptor.id,
)
