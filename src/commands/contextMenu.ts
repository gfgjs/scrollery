// 媒体右键菜单的命令组成(顶栏重构 P2-3)。收敛前 MediaGrid / MediaDetailOverlay 各自硬编码同一
// 份菜单项(copy/explorer/wallpaper)+ 同一份「壁纸仅图片」条件;现共享项 + 条件在注册表单份定义,
// 本 helper 按 order 合并「注册表共享命令 + 宿主本地命令」并按 when 过滤。
//
// move/copy 仍是各宿主的本地 Command:其 run 闭包捕获组件本地对话框实例(网格批量流 vs 覆盖层
// 单项流,语义都是「操作该目标项」),对话框实例本地化,故不入注册表;其解耦属 P4/P5。

import { commandRegistry, isCommandVisible } from './registry'
import type { Command, CommandContext } from './types'

// 共享命令在右键菜单中的排序锚(与 global.ts 的 order 一致);move/copy 由宿主以本地 Command 插入
// (order 30/40,落在 explorer 与 wallpaper 之间,还原现状顺序 copy·explorer·move·copy·wallpaper)。
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
