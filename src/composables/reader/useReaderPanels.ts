// src/composables/reader/useReaderPanels.ts
// 汇总各面板状态；新增面板须登记到 flags，确保沉浸与 Esc 能统一收起。
// 搜索面板另走 closeSearch()，以清理异步搜索及高亮。
import { computed, type Ref } from 'vue'

export function useReaderPanels(
  flags: Ref<boolean>[],
  closeSearch: () => void,
  showSearch: Ref<boolean>,
) {
  /** 是否有任何面板开着 —— Esc 分层退出据此决定这一下是「关面板」还是「退出阅读器」。 */
  const anyPanelOpen = computed(() => showSearch.value || flags.some((f) => f.value))

  /** 收拢全部面板。 */
  function closeAllPanels() {
    flags.forEach((f) => (f.value = false))
    closeSearch()
  }

  return { anyPanelOpen, closeAllPanels }
}
