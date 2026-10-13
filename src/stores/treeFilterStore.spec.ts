import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useFilterStore } from './filterStore'
import { useTreeFilterStore } from './treeFilterStore'
import type { MediaType } from '../types/media'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('文件树仅目录与其它类别的切换', () => {
  it.each<{ label: string; types: MediaType[] }>([
    { label: '图库已选图片', types: ['image'] },
    { label: '图库未限制类别', types: [] },
  ])('$label 时，从仅目录勾选其它只显示其它，再勾选视频建立新选择', ({ types }) => {
    const filter = useFilterStore()
    const tree = useTreeFilterStore()
    filter.setMediaTypes(types)

    // 对应文件树菜单的“仅目录”动作，它本身应保留图库已有筛选。
    tree.showDirectoriesOnly()
    expect(tree.selectedCategories).toEqual([])
    expect(filter.mediaTypes).toEqual(types)

    tree.toggleCategory('other')
    expect(tree.directoriesOnly).toBe(false)
    expect(tree.selectedCategories).toEqual(['other'])
    expect(tree.categoryFilterKey).toBe('other')
    expect(tree.selectedCategoryCount).toBe(1)
    expect(tree.isCategorySelected('image')).toBe(false)
    // 树专用“其它”沿用现有约定：共享数组为空，由 otherOnly 表达树的局部选择。
    expect(filter.mediaTypes).toEqual([])

    tree.toggleCategory('video')
    expect(tree.otherOnly).toBe(false)
    expect(tree.selectedCategories).toEqual(['video', 'other'])
    expect(filter.toApiFilter().mediaTypes).toEqual(['video'])
  })
})
