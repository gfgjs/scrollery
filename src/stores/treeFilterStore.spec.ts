import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { ALL_TREE_CATEGORIES } from '../constants/mediaCategoryDescriptors'
import { useFilterStore } from './filterStore'
import { useTreeFilterStore } from './treeFilterStore'

describe('treeFilterStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('默认投影共享四类，auto other 让五类全选', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    expect(filter.mediaTypes).toEqual([])
    expect(store.selectedCategories).toEqual([...ALL_TREE_CATEGORIES])
    expect(store.otherOverride).toBe('auto')
    expect(store.allCategoriesSelected).toBe(true)
    expect(store.hasActiveCategoryFilter).toBe(false)
    expect('treeDisplayMode' in store).toBe(false)
  })

  it('共同类型受限时 other 自动隐藏，但显式打开后保留', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    filter.setMediaTypes(['image', 'video'])
    expect(store.selectedCategories).toEqual(['image', 'video'])
    expect(store.isCategorySelected('other')).toBe(false)

    store.toggleCategory('other')
    expect(store.otherOverride).toBe('on')
    expect(store.selectedCategories).toEqual(['image', 'video', 'other'])
    store.toggleCategory('other')
    expect(store.otherOverride).toBe('off')
    expect(store.selectedCategories).toEqual(['image', 'video'])
  })

  it('只剩其它时保持 tree-only 语义，不把 other 写入 gallery mediaTypes', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    filter.setMediaTypes(['document'])
    store.setOtherOverride('on')
    store.toggleCategory('document')
    expect(filter.mediaTypes).toEqual([])
    expect(store.otherOnly).toBe(true)
    expect(store.selectedCategories).toEqual(['other'])

    store.toggleCategory('other')
    expect(filter.mediaTypes).toEqual([])
    expect(store.selectedCategories).toEqual(['image', 'video', 'document', 'audio'])
  })

  it('树切换共享类型直接改 filterStore，图库入口也会投影到树', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    store.toggleCategory('image')
    expect(filter.mediaTypes).toEqual(['video', 'document', 'audio'])
    expect(store.selectedCategories).toEqual(['video', 'document', 'audio'])

    filter.setMediaTypes(['document'])
    expect(store.selectedCategories).toEqual(['document'])
  })

  it('图库入口选择共享类型会退出文件树的仅目录扩展', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    store.showDirectoriesOnly()

    filter.setMediaTypes(['document'])

    expect(store.directoriesOnly).toBe(false)
    expect(store.selectedCategories).toEqual(['document'])
  })

  it('仅目录是树局部状态，不改变共享类型；重新勾选建立共同选择', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    filter.setMediaTypes(['video'])
    store.showDirectoriesOnly()
    expect(store.selectedCategories).toEqual([])
    expect(filter.mediaTypes).toEqual(['video'])

    store.toggleCategory('audio')
    expect(filter.mediaTypes).toEqual(['audio'])
    expect(store.selectedCategories).toEqual(['audio'])
  })

  it('仅目录状态下勾选其它只显示文件树专用的其它', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    store.showDirectoriesOnly()

    store.toggleCategory('other')

    expect(filter.mediaTypes).toEqual([])
    expect(store.otherOnly).toBe(true)
    expect(store.selectedCategories).toEqual(['other'])
  })

  it('全选/重置恢复图库与文件树一致，other 回到 auto', () => {
    const filter = useFilterStore()
    const store = useTreeFilterStore()
    filter.setMediaTypes(['image'])
    store.setOtherOverride('on')
    store.showDirectoriesOnly()
    store.selectAllCategories()
    expect(filter.mediaTypes).toEqual([])
    expect(store.otherOverride).toBe('auto')
    expect(store.selectedCategories).toEqual([...ALL_TREE_CATEGORIES])
    expect(store.directoriesOnly).toBe(false)
  })

  it('旧 clearCategories 名称保留但语义明确为仅目录', () => {
    const store = useTreeFilterStore()
    store.clearCategories()
    expect(store.directoriesOnly).toBe(true)
    expect(store.selectedCategories).toEqual([])
  })
})
