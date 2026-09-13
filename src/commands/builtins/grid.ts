// 网格视图导航命令(顶栏重构 P3):撤销/重做/全屏从 AppToolbar 迁入标题栏 ContextualToolbar
// (navigation 组=主图标按钮)。数据类命令直接 useXxxStore()(store 不入 context,见 types.ts)。
//
// 密集控件(面包屑/筛选/搜索/分组·排序/行高)留 AppToolbar(Phase G 已随之并入标题栏单行),
// 不入注册表(是值选择器/复合控件,非单动作命令)。

import { markRaw } from 'vue'
import { Undo2, Redo2, Maximize2 } from '@lucide/vue'
import type { Command } from '../types'
import { useHistoryStore } from '../../stores/historyStore'
import { isFullscreen, toggleFullscreen } from '../../composables/useWindowMode'
import i18n from '../../i18n'

const t = (k: string) => i18n.global.t(k)

export const gridCommands: Command[] = [
  {
    id: 'grid.undo',
    title: () => t('toolbar.undo'),
    icon: markRaw(Undo2),
    group: 'navigation',
    order: 10,
    keybinding: 'mod+z',
    when: (ctx) => ctx.view === 'grid',
    isEnabled: () => useHistoryStore().canUndo,
    run: () => useHistoryStore().undo(),
  },
  {
    id: 'grid.redo',
    title: () => t('toolbar.redo'),
    icon: markRaw(Redo2),
    group: 'navigation',
    order: 20,
    keybinding: 'mod+y',
    // mac/惯用重做别名;隐藏匹配不进 tooltip(避免提示串过长)。
    keybindingAliases: ['mod+shift+z'],
    when: (ctx) => ctx.view === 'grid',
    isEnabled: () => useHistoryStore().canRedo,
    run: () => useHistoryStore().redo(),
  },
  {
    id: 'grid.toggleFullscreen',
    // 标题随态切换(进入/退出);图标暂静态 Maximize2 + isActive 高亮(动态图标待 P5 播放/暂停一并做)。
    // 全屏态来自 useWindowMode(窗口三态唯一所有者),不再经 uiStore(2026-07-16 迁出,见其 Fullscreen 段注)。
    title: () => (isFullscreen.value ? t('toolbar.exitFullscreen') : t('toolbar.fullscreen')),
    icon: markRaw(Maximize2),
    group: 'navigation',
    order: 30,
    // 与 e.key === 'F11' 字面一致(裸键匹配大小写敏感;tooltip 亦显 F11)。
    keybinding: 'F11',
    // 按住 F11 的自动重复不重复执行:重复键 ~30ms 一发、窗口转换约 ~100ms,不挡则窗口来回抽搐。
    ignoreKeyRepeat: true,
    when: (ctx) => ctx.view === 'grid',
    isActive: () => isFullscreen.value,
    run: () => toggleFullscreen(),
  },
]
