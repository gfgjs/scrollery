// 共享右键命令由注册表提供；移动/复制命令捕获宿主自己的对话框，故由宿主传入。

import { commandRegistry, isCommandVisible } from './registry'
import type { Command, CommandContext } from './types'

// 宿主的移动/复制命令按 order 插在资源管理器与壁纸命令之间。
const SHARED_CONTEXT_IDS = ['global.copyImage', 'global.showInExplorer', 'global.setWallpaper']

/**
 * 解析媒体右键菜单应显示的命令(注册表共享命令 + 宿主本地命令),按 when 过滤 + order 升序。
 * 返回 Command[](宿主再各自映射为 ContextMenuItem,保持 commands/ 层不依赖组件类型)。
 */
export function resolveMediaContextCommands(
  ctx: CommandContext,
  localCommands: Command[] = [],
): Command[] {
  const shared = SHARED_CONTEXT_IDS.map((id) => commandRegistry.get(id)).filter(
    (c): c is Command => c != null,
  )
  return [...shared, ...localCommands]
    .filter((c) => isCommandVisible(c, ctx))
    .sort((a, b) => (a.order ?? 1e9) - (b.order ?? 1e9))
}
