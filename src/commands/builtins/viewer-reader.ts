// 阅读器命令按当前引擎暴露的能力显示，避免同为 Markdown 却调用未挂载的面板。

import { markRaw } from 'vue'
import { List, Search, Bookmark, Type } from '@lucide/vue'
import type { Command, CommandContext } from '../types'
import i18n from '../../i18n'

const t = (k: string) => i18n.global.t(k)

export const viewerReaderCommands: Command[] = [
  {
    id: 'viewer.reader.toc',
    title: () => t('doc.toc'),
    icon: markRaw(List),
    group: 'navigation',
    order: 10,
    when: (ctx: CommandContext) => !!ctx.activeViewer?.api?.toggleToc,
    run: (ctx) => ctx.activeViewer?.api?.toggleToc?.(),
  },
  {
    id: 'viewer.reader.search',
    title: () => t('doc.search'),
    icon: markRaw(Search),
    group: 'navigation',
    order: 20,
    when: (ctx: CommandContext) => !!ctx.activeViewer?.api?.toggleSearch,
    run: (ctx) => ctx.activeViewer?.api?.toggleSearch?.(),
  },
  {
    id: 'viewer.reader.bookmarks',
    title: () => t('doc.bookmarks'),
    icon: markRaw(Bookmark),
    group: 'navigation',
    order: 30,
    when: (ctx: CommandContext) => !!ctx.activeViewer?.api?.toggleBookmarks,
    run: (ctx) => ctx.activeViewer?.api?.toggleBookmarks?.(),
  },
  {
    id: 'viewer.reader.settings',
    title: () => t('doc.readerSettings'),
    icon: markRaw(Type),
    group: 'navigation',
    order: 40,
    when: (ctx: CommandContext) => !!ctx.activeViewer?.api?.toggleSettings,
    run: (ctx) => ctx.activeViewer?.api?.toggleSettings?.(),
  },
]
