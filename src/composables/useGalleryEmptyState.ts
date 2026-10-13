// 空状态消息与下一步动作由 resolveGalleryEmptyState 决定。
// 筛选后零结果引导清除筛选；优先级为 search > filtered > directory > smartAlbum。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useUiStore } from '../stores/uiStore'
import { useViewStore } from '../stores/viewStore'
import { useFilterStore } from '../stores/filterStore'
import { resolveGalleryEmptyState } from '../utils/resolveEmptyState'

export function useGalleryEmptyState() {
  const ui = useUiStore()
  const viewStore = useViewStore()
  const filter = useFilterStore()
  const { t } = useI18n()

  const emptyState = computed(() =>
    resolveGalleryEmptyState({
      searchQuery: ui.searchQuery,
      hasActiveFilters: filter.hasActiveFilters,
      activeDirectoryId: viewStore.activeDirectoryId,
      activePersonId: viewStore.activePersonId,
      activeSmartAlbum: viewStore.activeSmartAlbum,
    }),
  )
  // title/description 由决策 key 解 i18n(仅搜索场景带 { query } 插值);说明缺省(descKey=null)不渲染。
  const emptyTitle = computed(() =>
    emptyState.value.params
      ? t(emptyState.value.titleKey, emptyState.value.params)
      : t(emptyState.value.titleKey),
  )
  const emptyDescription = computed(() =>
    emptyState.value.descKey ? t(emptyState.value.descKey) : '',
  )
  // 动作标签:add-folder 复用 sidebar.addFolder,clear-filters 复用既有 toolbar.clearFilters(不新增重复键)。
  const emptyActionLabel = computed(() => {
    switch (emptyState.value.action) {
      case 'add-folder':
        return t('sidebar.addFolder')
      case 'clear-filters':
        return t('toolbar.clearFilters')
      default:
        return ''
    }
  })
  // 空状态「下一步动作」分派(§6.3/§7.1):add-folder 经 request-add-folder 事件复用 FoldersSection 的
  // 完整 addRoot 流程(重叠检测/自动扫描/树选中),不在此复制(事件通道同 folder-stats-changed 惯例);
  // clear-filters 清全局筛选态(filterStore.clearFilters),live 重算即回照片。
  function onEmptyAction() {
    switch (emptyState.value.action) {
      case 'add-folder':
        window.dispatchEvent(new CustomEvent('request-add-folder'))
        break
      case 'clear-filters':
        filter.clearFilters()
        break
    }
  }

  return { emptyState, emptyTitle, emptyDescription, emptyActionLabel, onEmptyAction }
}
