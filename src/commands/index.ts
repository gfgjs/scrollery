// builtins 注册入口 + 消费方桶。app 初始化(main.ts)调 registerBuiltins() 把内建命令装入单例
// 注册表;各消费方(右键菜单 / 上下文工具栏 / 命令面板 / keymap)从此桶取 API。

import { commandRegistry } from './registry'
import { globalCommands } from './builtins/global'
import { gridCommands } from './builtins/grid'
import { viewerImageCommands } from './builtins/viewer-image'
import { viewerVideoCommands } from './builtins/viewer-video'
import { viewerAudioCommands } from './builtins/viewer-audio'
import { viewerReaderCommands } from './builtins/viewer-reader'

let registered = false

/** 幂等注册所有内建命令到应用级单例注册表(重复调用无副作用,便于 HMR/多次 mount)。 */
export function registerBuiltins(): void {
  if (registered) return
  registered = true
  commandRegistry.registerAll(globalCommands)
  commandRegistry.registerAll(gridCommands)
  commandRegistry.registerAll(viewerImageCommands)
  commandRegistry.registerAll(viewerVideoCommands)
  commandRegistry.registerAll(viewerAudioCommands)
  commandRegistry.registerAll(viewerReaderCommands)
}

export { commandRegistry, resolveCommandTitle, isCommandVisible, isCommandEnabled, isCommandActive } from './registry'
export { buildCommandContext } from './context'
export { resolveMediaContextCommands } from './contextMenu'
export { formatKeybinding, dispatchKeybinding, normalizeEventKey } from './keybinding'
export type { Command, CommandContext, CommandGroup, ContextTarget } from './types'
