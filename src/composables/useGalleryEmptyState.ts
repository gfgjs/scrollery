// 画廊空状态决策(自 MediaGrid.vue 结构拆分抽出,逻辑逐字保留)。
//
// 空状态决策:消息 + 下一步动作的**单一源**(resolveGalleryEmptyState 纯函数,穷举 spec 锁死)。
// 收敛此前内联在 MediaGrid 的 emptyStateText / showEmptyAction 双 computed 里的 precedence,并修一个
// 真实错 CTA——全局筛选态激活却零结果时,此前落「空库·添加文件夹」把用户引向加目录,而正确的
// 下一步是**清除筛选**(precedence:search > filtered > directory > smartAlbum,详见 utils/resolveEmptyState)。
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
