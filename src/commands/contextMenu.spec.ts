// 右键菜单收敛单测(顶栏重构 P2-3)。锁行为对拍:收敛后菜单组成 + 顺序 + 壁纸-仅图片条件
// 与收敛前硬编码版一致(copy·explorer·move·copy·wallpaper;非图片无壁纸;无目标不显示共享项)。
import { describe, it, expect, beforeAll } from 'vitest'
import { registerBuiltins, commandRegistry } from './index'
import { resolveMediaContextCommands } from './contextMenu'
import type { Command, CommandContext, ContextTarget } from './types'

beforeAll(() => registerBuiltins())

function makeCtx(contextTarget: ContextTarget | null): CommandContext {
  return {
    view: 'grid',
    activeViewer: null,
    selection: { count: 0, isSingle: false },
    contextTarget,
  }
}

// 宿主本地 move/copy(顺序锚 30/40,落在 explorer 与 wallpaper 之间——还原现状菜单顺序)。
const localMoveCopy: Command[] = [
  { id: 'x.move', title: 'move', group: 'organize', order: 30, run: () => {} },
  { id: 'x.copy', title: 'copy', group: 'organize', order: 40, run: () => {} },
]

describe('registerBuiltins + resolveMediaContextCommands', () => {
  it('注册 3 个 global 上下文命令', () => {
    expect(commandRegistry.get('global.copyImage')).toBeDefined()
    expect(commandRegistry.get('global.showInExplorer')).toBeDefined()
    expect(commandRegistry.get('global.setWallpaper')).toBeDefined()
  })

  it('图片目标:copy·explorer·move·copy·wallpaper(与收敛前顺序一致)', () => {
    const ctx = makeCtx({ id: 1, mediaType: 'image' })
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual([
      'global.copyImage',
      'global.showInExplorer',
      'x.move',
      'x.copy',
      'global.setWallpaper',
    ])
  })

  it('视频目标:无壁纸(壁纸-仅图片条件生效)', () => {
    const ctx = makeCtx({ id: 1, mediaType: 'video' })
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual(['global.copyImage', 'global.showInExplorer', 'x.move', 'x.copy'])
    expect(ids).not.toContain('global.setWallpaper')
  })

  it('目标类型未知(item 查找失败降级):通用命令仍显示,壁纸不显示', () => {
    const ctx = makeCtx({ id: 1 })
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual(['global.copyImage', 'global.showInExplorer', 'x.move', 'x.copy'])
  })

  it('无目标(contextTarget=null):共享命令全不显示,仅剩本地 move/copy', () => {
    const ctx = makeCtx(null)
    const ids = resolveMediaContextCommands(ctx, localMoveCopy).map((c) => c.id)
    expect(ids).toEqual(['x.move', 'x.copy'])
  })
})
