// src/commands/commandDispatch.core.spec.ts
// 命令分发与注册表机制。2026-09-16 由 keybinding.spec.ts(键位同源,顶栏重构 P5-6)与
// registry.spec.ts(注册表机制,P2-1)集中而来:同目录、环境一致(registry 用 createCommandRegistry
// 隔离实例,keybinding 用应用级单例并只装入 viewer-image 一册),原两文件删去;每条场景仍是独立 it。
//
// keybinding:纯函数 + 组合键分发(2026-07-10 深审 HIGH-2:分发器曾只匹配裸键,mod+z 结构上永不命中)。
// registry:ctx 只穿可变调用上下文(store 不入 context,命令 run 内直接 useXxxStore()),故无需 pinia。
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { dispatchKeybinding } from './keybinding'
import { commandRegistry, createCommandRegistry } from './registry'
import type { Command, CommandContext } from './types'
import type { ViewerApi } from '../stores/viewerStore'
import type { MediaType } from '../types/media'

function imgCtx(api: ViewerApi): CommandContext {
  return {
    view: 'viewer',
    activeViewer: {
      kind: 'image',
      mediaType: 'image' as MediaType,
      fileFormat: 'jpg',
      id: 1,
      path: null,
      title: 't',
      api,
      immersive: false,
      fileInfo: null,
    },
    selection: { count: 0, isSingle: false },
    contextTarget: null,
  }
}


describe('dispatchKeybinding 组合键与别名', () => {
  const undo = vi.fn()
  const redo = vi.fn()
  const fs = vi.fn()
  beforeAll(() =>
    commandRegistry.registerAll([
      {
        id: 'test.combo.undo',
        title: 'undo',
        group: 'navigation',
        keybinding: 'mod+z',
        when: (c) => c.view === 'grid',
        run: undo,
      },
      {
        id: 'test.combo.redo',
        title: 'redo',
        group: 'navigation',
        keybinding: 'mod+y',
        keybindingAliases: ['mod+shift+z'],
        when: (c) => c.view === 'grid',
        run: redo,
      },
      {
        id: 'test.combo.fullscreen',
        title: 'fs',
        group: 'navigation',
        keybinding: 'F11',
        when: (c) => c.view === 'grid',
        run: fs,
      },
    ]),
  )

  it('组合键按 when 上下文过滤(查看器上下文不触发网格 mod+z)', () => {
    expect(
      dispatchKeybinding({ key: 'z', ctrlKey: true } as KeyboardEvent, imgCtx({} as ViewerApi)),
    ).toBe(false)
  })
})

function makeCtx(overrides: Partial<CommandContext> = {}): CommandContext {
  return {
    view: 'grid',
    activeViewer: null,
    selection: { count: 0, isSingle: false },
    contextTarget: null,
    ...overrides,
  }
}

function cmd(partial: Partial<Command> & { id: string }): Command {
  return { title: partial.id, group: 'navigation', run: () => {}, ...partial }
}

describe('commandRegistry', () => {

  it('run 守卫 when / isEnabled', () => {
    const r = createCommandRegistry()
    let ran = 0
    r.register(
      cmd({
        id: 'guarded',
        when: (c) => c.selection.count > 0,
        run: () => {
          ran++
        },
      }),
    )
    r.run('guarded', makeCtx()) // when false → 跳过
    expect(ran).toBe(0)
    r.run('guarded', makeCtx({ selection: { count: 1, isSingle: true } })) // when true → 执行
    expect(ran).toBe(1)
    r.register(
      cmd({
        id: 'disabled',
        isEnabled: () => false,
        run: () => {
          ran++
        },
      }),
    )
    r.run('disabled', makeCtx())
    expect(ran).toBe(1) // isEnabled false → 未增
  })
})
