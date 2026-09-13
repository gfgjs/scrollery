import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import {
  ALL_TREE_CATEGORIES,
  MEDIA_CATEGORY_DESCRIPTORS,
} from '../constants/mediaCategoryDescriptors'
import type { MediaType, TreeCategory } from '../types/media'
import { useFilterStore } from './filterStore'

/** `other` 只属于文件树；auto 会根据共享四类是否受限决定默认显隐。 */
export type TreeOtherOverride = 'auto' | 'on' | 'off'

const COMMON_MEDIA_TYPES = MEDIA_CATEGORY_DESCRIPTORS.map(
  (descriptor) => descriptor.id,
) as readonly MediaType[]

function isTreeCategory(category: TreeCategory): boolean {
  return ALL_TREE_CATEGORIES.includes(category)
}

/**
 * 文件树分类状态。
 *
 * 图片/视频/文档/音频不在这里复制一份：它们直接投影自 gallery filterStore.mediaTypes，
 * 因而从任意入口切换都会同步图库、URL 与文件树。`other`、“仅目录”以及“只看其它”
 * 没有图库对应物，仍由本 store 保留局部状态。
 */
export const useTreeFilterStore = defineStore('treeFilter', () => {
  const filter = useFilterStore()
  const otherOverride = ref<TreeOtherOverride>('auto')
  const directoriesOnly = ref(false)
  // gallery 的空 mediaTypes 表示“不限”，但树仍需能表达“只看其它”。这是一种
  // 文件树专用模式，不复制四类选择；一旦 gallery 重新出现具体共同类型就自动失效。
  const otherOnly = ref(false)

  /** 空数组及显式包含四类都表示共同类型未被限制，保持 URL/API 的既有语义。 */
  const galleryAllCommonCategoriesSelected = computed(() => {
    if (filter.mediaTypes.length === 0) return true
    const selected = new Set(filter.mediaTypes)
    return (
      selected.size === COMMON_MEDIA_TYPES.length &&
      COMMON_MEDIA_TYPES.every((category) => selected.has(category))
    )
  })

  const otherOnlyActive = computed(
    () => otherOnly.value && !directoriesOnly.value && filter.mediaTypes.length === 0,
  )
  const allCommonCategoriesSelected = computed(
    () => !otherOnlyActive.value && galleryAllCommonCategoriesSelected.value,
  )

  const commonCategories = computed<MediaType[]>(() => {
    if (otherOnlyActive.value) return []
    if (allCommonCategoriesSelected.value) return [...COMMON_MEDIA_TYPES]
    const selected = new Set(filter.mediaTypes)
    return COMMON_MEDIA_TYPES.filter((category) => selected.has(category))
  })

  const otherSelected = computed(() => {
    if (directoriesOnly.value) return false
    if (otherOnlyActive.value) return true
    if (otherOverride.value === 'on') return true
    if (otherOverride.value === 'off') return false
    return allCommonCategoriesSelected.value
  })

  /** 传给 useFolderTree / IPC 的稳定、规范顺序分类集合。 */
  const selectedCategories = computed<TreeCategory[]>(() => {
    if (directoriesOnly.value) return []
    return otherSelected.value
      ? [...commonCategories.value, 'other']
      : [...commonCategories.value]
  })

  const allCategoriesSelected = computed(
    () => selectedCategories.value.length === ALL_TREE_CATEGORIES.length,
  )
  const noCategoriesSelected = computed(() => selectedCategories.value.length === 0)
  const hasActiveCategoryFilter = computed(() => !allCategoriesSelected.value)
  const selectedCategoryCount = computed(() => selectedCategories.value.length)
  const categoryFilterKey = computed(() => selectedCategories.value.join(','))

  // 顶栏/URL 选中任一共同类型后，树-only other 模式不再遮住共享投影。
  watch(
    () => filter.mediaTypes,
    (types) => {
      if (types.length > 0) {
        otherOnly.value = false
        directoriesOnly.value = false
      }
    },
    { flush: 'sync' },
  )

  function selectSharedCategory(category: MediaType) {
    // 空/四类全选是“不限”，树上点掉一个类别时展开为其余三类；之后仍由
    // filterStore 负责通知图库、URL 和已有的布局 watcher。
    if (allCommonCategoriesSelected.value) {
      filter.setMediaTypes(COMMON_MEDIA_TYPES.filter((item) => item !== category))
      return
    }
    if (filter.mediaTypes.length === 1 && filter.mediaTypes[0] === category) {
      // [] 对 gallery 是“不限”，不能直接拿来表示树的“无共享类别”。保留
      // filterStore 的空值并用 tree-only 模式承载“其它”或“仅目录”。
      filter.setMediaTypes([])
      if (otherOverride.value === 'off') {
        directoriesOnly.value = true
        otherOnly.value = false
      } else {
        otherOnly.value = true
      }
      return
    }
    filter.toggleMediaType(category)
  }

  function toggleCategory(category: TreeCategory) {
    if (!isTreeCategory(category)) return
    const wasDirectoriesOnly = directoriesOnly.value
    const wasOtherOnly = otherOnlyActive.value
    const wasSelected = selectedCategories.value.includes(category)
    // “仅目录”是局部显示状态；用户重新勾选任一类型时回到文件显示。
    directoriesOnly.value = false
    if (category === 'other') {
      if (wasDirectoriesOnly) {
        // “仅目录”没有共享类型；从这里勾选 Other 时，最小且可解释的结果是
        // 只看文件树专用的 Other，而不是意外恢复全部五类。
        otherOnly.value = true
        otherOverride.value = 'on'
        return
      }
      if (wasOtherOnly) {
        // 退出“只看其它”回到 gallery 空筛选的四类投影；other 显式关闭。
        otherOnly.value = false
        otherOverride.value = 'off'
      } else {
        otherOverride.value = wasSelected ? 'off' : 'on'
      }
      return
    }
    if (wasDirectoriesOnly || wasOtherOnly) {
      // 仅目录状态没有“已选共同类型”；首次勾选应建立一个明确的图库/树共同选择。
      otherOnly.value = false
      filter.setMediaTypes([category])
      return
    }
    selectSharedCategory(category)
  }

  /** 设置共享四类；四类全选收敛为空数组，以保留顶栏/URL 的“不限”表示。 */
  function setSharedCategories(categories: readonly MediaType[]) {
    directoriesOnly.value = false
    otherOnly.value = false
    const requested = new Set(categories)
    const selected = COMMON_MEDIA_TYPES.filter((category) => requested.has(category))
    if (selected.length === 0) {
      // 共同数组的空值属于 gallery“不限”；树端的“无共享类别”需落到专用模式。
      filter.setMediaTypes([])
      if (otherOverride.value === 'off') directoriesOnly.value = true
      else otherOnly.value = true
      return
    }
    filter.setMediaTypes(selected.length === COMMON_MEDIA_TYPES.length ? [] : selected)
  }

  /** 重置共享四类；文件树专用的 `other` / “仅目录”局部状态保持不变。 */
  function resetCommonCategories() {
    filter.setMediaTypes([])
  }

  /** 全选是菜单里的总复位：共享四类回到“不限”，other 回到自动，取消仅目录。 */
  function selectAllCategories() {
    directoriesOnly.value = false
    otherOnly.value = false
    resetCommonCategories()
    otherOverride.value = 'auto'
  }

  /** 明确的“仅目录”语义，不改图库共享筛选。 */
  function showDirectoriesOnly() {
    directoriesOnly.value = true
    otherOnly.value = false
  }

  function setDirectoriesOnly(value: boolean) {
    directoriesOnly.value = value
    if (value) otherOnly.value = false
  }

  function setOtherOverride(value: TreeOtherOverride) {
    otherOverride.value = value
    directoriesOnly.value = false
    otherOnly.value = false
  }

  /** 兼容旧调用名；UI 应使用 showDirectoriesOnly，避免把“清空”误解为无筛选。 */
  function clearCategories() {
    showDirectoriesOnly()
  }

  function isCategorySelected(category: TreeCategory): boolean {
    return selectedCategories.value.includes(category)
  }

  return {
    selectedCategories,
    commonCategories,
    otherOverride,
    otherSelected,
    directoriesOnly,
    otherOnly,
    allCommonCategoriesSelected,
    allCategoriesSelected,
    noCategoriesSelected,
    hasActiveCategoryFilter,
    selectedCategoryCount,
    categoryFilterKey,
    toggleCategory,
    setSharedCategories,
    resetCommonCategories,
    selectAllCategories,
    showDirectoriesOnly,
    setDirectoriesOnly,
    setOtherOverride,
    clearCategories,
    isCategorySelected,
  }
})
