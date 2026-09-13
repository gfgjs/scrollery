import { ALL_TREE_CATEGORIES } from '../../../constants/mediaCategoryDescriptors'
import type { TreeCategory } from '../../../types/media'

export type TreeCategorySummary =
  | { kind: 'all'; count: number }
  | { kind: 'none'; count: 0 }
  | { kind: 'single'; category: Exclude<TreeCategory, 'other'>; count: 1 }
  | { kind: 'other'; category: 'other'; count: 1 }
  | { kind: 'some'; count: number }

/**
 * 只做菜单摘要所需的纯计算；具体文案由组件按当前 locale 翻译。
 * 空集合不是“无效状态”，它表示文件树只保留目录行。
 */
export function summarizeTreeCategories(
  selected: readonly TreeCategory[],
): TreeCategorySummary {
  const count = selected.length
  if (count === ALL_TREE_CATEGORIES.length) return { kind: 'all', count }
  if (count === 0) return { kind: 'none', count: 0 }
  if (count === 1) {
    const category = selected[0]
    return category === 'other'
      ? { kind: 'other', category, count: 1 }
      : { kind: 'single', category, count: 1 }
  }
  return { kind: 'some', count }
}

export function isTreeCategorySelected(
  selected: readonly TreeCategory[],
  category: TreeCategory,
): boolean {
  return selected.includes(category)
}
