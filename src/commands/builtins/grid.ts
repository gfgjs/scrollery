// 网格导航命令显示在标题栏；值选择器等复合控件由 AppToolbar 承载。

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
    // 标题与高亮随全屏态切换；全屏态由 useWindowMode 提供。
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
