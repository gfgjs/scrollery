// src/composables/reader/useReaderPanels.ts
// 面板互斥总控（结构拆分自 DocumentViewer.vue §S24）。薄编排工具：不持有状态，只汇总各域已
// 实例化的 show* ref。**新增面板必须登记于装配处的 flags 数组**（原注释原话，红线）——
// 漏登记 = 沉浸态残留面板遮挡 + Esc 跳过面板层直接关掉阅读器，而没有任何门会红。
// showSearch 有意不进 flags 数组：它的关闭需额外清理搜索迭代/高亮，统一走 closeSearch()。
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
