// 键位同源(顶栏重构 P5-6):命令的 `keybinding` 常量既是 tooltip 显示源、也是查看器 keydown
// 分发源,杜绝「显示 ≠ 行为」漂移。此前快捷键在两处各写一份(i18n 串如「信息 (i)」+ 组件
// keydown 的 `if (e.key === 'i')`),改标签不改监听、或反之,tooltip 就会撒谎。收敛为单源后
// 改一处两处同步。

import { isMac } from '../utils/platform'
import { commandRegistry } from './registry'
import type { CommandContext } from './types'

/**
 * 把 registry keybinding 常量格式化为紧凑显示串(tooltip 用)。
 * 组合键以 '+' 分隔(如 'mod+z');'mod' 按平台显 ⌘/Ctrl。字面 '+' 键单独处理(非分隔符)。
 */
export function formatKeybinding(kb: string): string {
  if (kb === '+') return '+' // 字面加号键(放大),非组合分隔符
  const sep = isMac ? '' : '+'
  return kb
    .split('+')
    .map((p) => {
      switch (p) {
        case ' ':
          return 'Space'
        case 'ArrowLeft':
          return '←'
        case 'ArrowRight':
          return '→'
        case 'ArrowUp':
          return '↑'
        case 'ArrowDown':
          return '↓'
        case 'Escape':
          return 'Esc'
        case 'mod':
          return isMac ? '⌘' : 'Ctrl'
        case 'shift':
          return isMac ? '⇧' : 'Shift'
        case 'alt':
          return isMac ? '⌥' : 'Alt'
        default:
          // 单字符键大写显示(i → I);多字符(F11 等)原样。
          return p.length === 1 ? p.toUpperCase() : p
      }
    })
    .join(sep)
}

/**
 * 规范化 KeyboardEvent.key 到 keybinding 常量,便于与 command.keybinding 匹配:
 * - '=' 视作 '+'(shift+= 常见,与放大同键);
 * - 单字符统一小写(字母大小写不敏感:i / I 同);
 * - 其余(ArrowLeft 等)原样。
 */
export function normalizeEventKey(e: KeyboardEvent): string {
  if (e.key === '=') return '+'
  if (e.key.length === 1) return e.key.toLowerCase()
  return e.key
}

/**
 * 把带修饰符的 KeyboardEvent 规范化为组合串(修饰符规范序 mod→shift→alt + normalizeEventKey);
 * **无 ctrl/meta/alt 时**:仅 shift+非字符键(e.key.length > 1)产出 'shift+key',否则返 null。
 * shift+单字符(如 shift+=)归裸键路径 normalizeEventKey,不产出 'shift+…' 组合(否则 '+' 这类
 * 绑定会被误前缀而失配)。
 */
export function eventToCombo(e: KeyboardEvent): string | null {
  if (!(e.ctrlKey || e.metaKey || e.altKey)) {
    // 无主修饰键时:仅 shift+非字符键(length > 1)产出组合,单字符键返 null
    if (e.shiftKey) {
      const key = normalizeEventKey(e)
      if (key.length > 1) {
        return 'shift+' + key
      }
    }
    return null
  }
  const parts: string[] = []
  if (e.ctrlKey || e.metaKey) parts.push('mod')
  if (e.shiftKey) parts.push('shift')
  if (e.altKey) parts.push('alt')
  parts.push(normalizeEventKey(e))
  return parts.join('+')
}

/**
 * 据 KeyboardEvent 在全部命令组找 keybinding(或隐藏别名 keybindingAliases)匹配、当前上下文
 * 可见+可用的命令并经 registry 执行;命中返 true(调用方据此 preventDefault)。查询已按 command.when
 * 过滤,故键位天然上下文感知(如网格 mod+z 不会在图片查看器误触——其 when=view'grid' 不通过)。
 * 带修饰符的事件只走组合串匹配(ctrl+i 不会误触发裸 'i');无修饰符走裸键匹配。
 * (2026-07-10 深审 HIGH-2 修复:此前只匹配裸键,mod+z 等组合键结构上永远无法经注册表分发,
 * 网格路径被迫留硬编码散点;本次补组合键后网格 undo/redo 已收敛,见 AppToolbar。)
 * (2026-07-11 command ownership 收敛:zoom/info 等命令按归属移入 view 组后,分发不再限定
 * navigation 组——group 只决定按钮渲染位置,键位可达性跨组一致,上下文安全仍由 when 保证。)
 */
export function dispatchKeybinding(e: KeyboardEvent, ctx: CommandContext): boolean {
  const key = eventToCombo(e) ?? normalizeEventKey(e)
  const cmd = commandRegistry
    .query(ctx)
    .find((c) => c.keybinding === key || c.keybindingAliases?.includes(key))
  if (!cmd) return false
  // 键盘自动重复:命中但声明 ignoreKeyRepeat 的命令(如 F11 全屏)不重复执行,仍返 true 让调用方
  // preventDefault——否则重复键会漏给 WebView2 的原生 F11 处理。挡在**分发器**而非各监听点:
  // AppToolbar(document 层)与 AppShell(window 层)两处都经此函数,逐处打补丁必漏(2026-07-16)。
  if (e.repeat && cmd.ignoreKeyRepeat) return true
  void commandRegistry.run(cmd.id, ctx)
  return true
}
