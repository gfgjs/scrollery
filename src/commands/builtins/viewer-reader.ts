// 阅读器查看器专属命令(顶栏重构 P5 余项)。DocumentViewer 经 viewerStore.populate 暴露 ViewerApi
// (toggleToc/toggleSearch/toggleBookmarks/toggleSettings),本册命令经 ctx.activeViewer.api 调用。
//
// when 谓词=**foliate 渲染器**(epub/txt/md)—— pdf 无 TOC/搜索/书签/排版设置(只有翻页模式),
// 故这些命令不对 pdf 显示。resolveViewerKind 映射:epub→'epub'、txt→'text'、md→'markdown'。
// 登记为 navigation 组 → 标题栏主按钮区(ContextualToolbar 仅渲染 navigation 组)。
// 过渡态(承 P5-5=保留局部控件):与 DocumentViewer 自带阅读工具栏并存(双入口)。

import { markRaw } from 'vue'
import { List, Search, Bookmark, Type } from '@lucide/vue'
import type { Command, CommandContext } from '../types'
import i18n from '../../i18n'

const t = (k: string) => i18n.global.t(k)

/** foliate 渲染器判据:epub/txt/md 才有 TOC/搜索/书签/排版设置(pdf 无)。 */
function isFoliateReader(ctx: CommandContext): boolean {
  const k = ctx.activeViewer?.kind
  return k === 'epub' || k === 'text' || k === 'markdown'
}

export const viewerReaderCommands: Command[] = [
  {
    id: 'viewer.reader.toc',
    title: () => t('doc.toc'),
    icon: markRaw(List),
    group: 'navigation',
    order: 10,
    when: isFoliateReader,
    run: (ctx) => ctx.activeViewer?.api?.toggleToc?.(),
  },
  {
    id: 'viewer.reader.search',
    title: () => t('doc.search'),
    icon: markRaw(Search),
    group: 'navigation',
    order: 20,
    when: isFoliateReader,
    run: (ctx) => ctx.activeViewer?.api?.toggleSearch?.(),
  },
  {
    id: 'viewer.reader.bookmarks',
    title: () => t('doc.bookmarks'),
    icon: markRaw(Bookmark),
    group: 'navigation',
    order: 30,
    when: isFoliateReader,
    run: (ctx) => ctx.activeViewer?.api?.toggleBookmarks?.(),
  },
  {
    id: 'viewer.reader.settings',
    title: () => t('doc.readerSettings'),
    icon: markRaw(Type),
    group: 'navigation',
    order: 40,
    when: isFoliateReader,
    run: (ctx) => ctx.activeViewer?.api?.toggleSettings?.(),
  },
]
