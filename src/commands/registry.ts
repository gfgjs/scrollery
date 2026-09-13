// CommandRegistry(顶栏重构 L3):一套注册表被多处消费——上下文工具栏(L4)、右键菜单、
// 未来命令面板、统一 keymap。本文件是注册/查询/求值/执行的**纯机制**;具体命令在 builtins/*。

import type { Command, CommandContext, CommandGroup } from './types'
import { logger } from '../utils/logger'

// order 缺省时的排序权重(排在所有显式 order 之后;同权重保持注册顺序——Array.sort 稳定)。
// 用有限大值而非 Infinity:Infinity - Infinity = NaN 会破坏比较器。
const DEFAULT_ORDER = 1e9

/** 解析命令标题(支持 i18n 惰性求值:函数形式每次取当前语言串)。 */
export function resolveCommandTitle(cmd: Command): string {
  return typeof cmd.title === 'function' ? cmd.title() : cmd.title
}

/** 命令在当前上下文是否可见(缺省 when → 恒可见)。 */
export function isCommandVisible(cmd: Command, ctx: CommandContext): boolean {
  return cmd.when ? cmd.when(ctx) : true
}

/** 命令在当前上下文是否可用(缺省 isEnabled → 恒可用)。 */
export function isCommandEnabled(cmd: Command, ctx: CommandContext): boolean {
  return cmd.isEnabled ? cmd.isEnabled(ctx) : true
}

/** 命令在当前上下文是否高亮态(缺省 isActive → 否)。 */
export function isCommandActive(cmd: Command, ctx: CommandContext): boolean {
  return cmd.isActive ? cmd.isActive(ctx) : false
}

export interface QueryOptions {
  /** 仅返回该组的命令(如 'navigation')。 */
  group?: CommandGroup
}

export interface CommandRegistry {
  register(cmd: Command): void
  registerAll(cmds: Command[]): void
  get(id: string): Command | undefined
  all(): Command[]
  /** 返回 when 通过(+可选 group 过滤)的命令,按 order 升序(稳定,同 order 保持注册序)。 */
  query(ctx: CommandContext, opts?: QueryOptions): Command[]
  /** 执行命令;when/isEnabled 不通过则跳过(keybinding 在错误上下文误触时安全)。 */
  run(id: string, ctx: CommandContext): void | Promise<void>
}

export function createCommandRegistry(): CommandRegistry {
  const commands = new Map<string, Command>()

  function register(cmd: Command): void {
    if (commands.has(cmd.id)) {
      // 重复 id = 注册 bug(或 dev HMR 重注册);覆盖并告警,便于开发期发现冲突。
      logger.warn(`[commands] 重复注册命令 id: ${cmd.id}(已覆盖)`)
    }
    commands.set(cmd.id, cmd)
  }

  function registerAll(cmds: Command[]): void {
    for (const c of cmds) register(c)
  }

  function get(id: string): Command | undefined {
    return commands.get(id)
  }

  function all(): Command[] {
    return [...commands.values()]
  }

  function query(ctx: CommandContext, opts?: QueryOptions): Command[] {
    const group = opts?.group
    return [...commands.values()]
      .filter((c) => (group ? c.group === group : true))
      .filter((c) => isCommandVisible(c, ctx))
      .sort((a, b) => (a.order ?? DEFAULT_ORDER) - (b.order ?? DEFAULT_ORDER))
  }

  function run(id: string, ctx: CommandContext): void | Promise<void> {
    const cmd = commands.get(id)
    if (!cmd) {
      logger.warn(`[commands] 执行未知命令 id: ${id}`)
      return
    }
    // 守卫:when/isEnabled 不通过则跳过(keybinding 误触错误上下文时安全;工具栏按钮本就仅在
    // 可见+可用态渲染,此处为双保险)。
    if (!isCommandVisible(cmd, ctx) || !isCommandEnabled(cmd, ctx)) return
    return cmd.run(ctx)
  }

  return { register, registerAll, get, all, query, run }
}

/** 应用级单例:builtins 在 app 初始化时 registerAll 到此,各消费方(L4/右键/命令面板/keymap)共享。 */
export const commandRegistry = createCommandRegistry()
