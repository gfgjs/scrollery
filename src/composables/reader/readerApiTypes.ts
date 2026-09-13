// src/composables/reader/readerApiTypes.ts
// 阅读器渲染器统一方法面（原内联于 DocumentViewer.vue，结构拆分抽出为共享类型，供各 reader
// composable 声明 readerRef 参数类型，无运行时逻辑，不构成 composable 间互引）。
import type { FoliateSearchResult } from '../../vendor/foliate-js/view.js'

export interface ReaderApi {
  next(): void
  prev(): void
  getScrollEl(): HTMLElement | null
  /** foliate 渲染器（BookReader）额外暴露：TOC 跳转。pdf/其它渲染器无此方法。 */
  goToHref?(href: string): void
  /** foliate 渲染器额外暴露：书内搜索（异步生成器，全书流式命中）+ 清除命中高亮。 */
  searchBook?(query: string): AsyncGenerator<FoliateSearchResult, void, unknown> | undefined
  clearSearch?(): void
  /** foliate 渲染器额外暴露：当前阅读位置快照（书签「添加当前位置」）+ 跳到书签位置。 */
  getCurrentLocation?(): { locator: string; label: string; fraction: number } | null
  goToLocator?(locator: string): void
}
