// src/commands/commandDispatch.core.spec.ts
// 命令分发与注册表机制。2026-09-16 由 keybinding.spec.ts(键位同源,顶栏重构 P5-6)与
// registry.spec.ts(注册表机制,P2-1)集中而来:同目录、环境一致(registry 用 createCommandRegistry
// 隔离实例,keybinding 用应用级单例并只装入 viewer-image 一册),原两文件删去;每条场景仍是独立 it。
//
// keybinding:纯函数 + 组合键分发(2026-07-10 深审 HIGH-2:分发器曾只匹配裸键,mod+z 结构上永不命中)。
// registry:ctx 只穿可变调用上下文(store 不入 context,命令 run 内直接 useXxxStore()),故无需 pinia。
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { formatKeybinding, normalizeEventKey, eventToCombo, dispatchKeybinding } from './keybinding'
import {
  commandRegistry,
  createCommandRegistry,
  resolveCommandTitle,
  isCommandVisible,
  isCommandEnabled,
  isCommandActive,
} from './registry'
import { viewerImageCommands } from './builtins/viewer-image'
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

describe('formatKeybinding(P5-6)', () => {
  it('方向键→箭头符号、字面 + 键、空格键、单字母大写', () => {
    expect(formatKeybinding('ArrowLeft')).toBe('←')
    expect(formatKeybinding('ArrowRight')).toBe('→')
    expect(formatKeybinding('+')).toBe('+')
    expect(formatKeybinding('-')).toBe('-')
    expect(formatKeybinding(' ')).toBe('Space')
    expect(formatKeybinding('i')).toBe('I')
  })

  it('mod 组合按平台(node 降级非 mac → Ctrl+Z)', () => {
    expect(formatKeybinding('mod+z')).toBe('Ctrl+Z')
  })
})

describe('normalizeEventKey(P5-6)', () => {
  it("'=' 归 '+'、字母小写、方向键原样", () => {
    expect(normalizeEventKey({ key: '=' } as KeyboardEvent)).toBe('+')
    expect(normalizeEventKey({ key: 'I' } as KeyboardEvent)).toBe('i')
    expect(normalizeEventKey({ key: 'ArrowLeft' } as KeyboardEvent)).toBe('ArrowLeft')
  })
})

describe('dispatchKeybinding(P5-6)', () => {
  beforeAll(() => commandRegistry.registerAll(viewerImageCommands))

  it('图片上下文:方向键/加减/i 命中 viewer 命令并执行', () => {
    const api = {
      prev: vi.fn(),
      next: vi.fn(),
      zoomIn: vi.fn(),
      zoomOut: vi.fn(),
      toggleInfo: vi.fn(),
    }
    const ctx = imgCtx(api as ViewerApi)
    expect(dispatchKeybinding({ key: 'ArrowLeft' } as KeyboardEvent, ctx)).toBe(true)
    expect(api.prev).toHaveBeenCalledOnce()
    expect(dispatchKeybinding({ key: 'ArrowRight' } as KeyboardEvent, ctx)).toBe(true)
    expect(api.next).toHaveBeenCalledOnce()
    expect(dispatchKeybinding({ key: '=' } as KeyboardEvent, ctx)).toBe(true) // = → +
    expect(api.zoomIn).toHaveBeenCalledOnce()
    expect(dispatchKeybinding({ key: '-' } as KeyboardEvent, ctx)).toBe(true)
    expect(api.zoomOut).toHaveBeenCalledOnce()
    expect(dispatchKeybinding({ key: 'I' } as KeyboardEvent, ctx)).toBe(true) // I → i
    expect(api.toggleInfo).toHaveBeenCalledOnce()
  })

  it('无匹配键返 false', () => {
    expect(dispatchKeybinding({ key: 'q' } as KeyboardEvent, imgCtx({} as ViewerApi))).toBe(false)
  })
})

function gridCtx(): CommandContext {
  return {
    view: 'grid',
    activeViewer: null,
    selection: { count: 0, isSingle: false },
    contextTarget: null,
  }
}

describe('eventToCombo(组合键规范化)', () => {
  it('mod→shift→alt 规范序;ctrl 与 meta 同归 mod', () => {
    expect(eventToCombo({ key: 'z', ctrlKey: true } as KeyboardEvent)).toBe('mod+z')
    expect(eventToCombo({ key: 'z', metaKey: true } as KeyboardEvent)).toBe('mod+z')
    expect(eventToCombo({ key: 'Z', ctrlKey: true, shiftKey: true } as KeyboardEvent)).toBe(
      'mod+shift+z',
    )
  })

  it('无 ctrl/meta/alt(含仅 shift)返 null——shift 是打出字符的固有成分', () => {
    expect(eventToCombo({ key: '+' } as KeyboardEvent)).toBe(null)
    expect(eventToCombo({ key: '+', shiftKey: true } as KeyboardEvent)).toBe(null)
  })

  it('shift+非字符键(e.key.length > 1)产出 shift+key;shift+单字符仍返 null', () => {
    expect(eventToCombo({ key: 'ArrowLeft', shiftKey: true } as KeyboardEvent)).toBe(
      'shift+ArrowLeft',
    )
    expect(eventToCombo({ key: '=', shiftKey: true } as KeyboardEvent)).toBe(null)
  })
})

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

  it('mod+z / mod+y 组合命中;别名 mod+shift+z 亦命中且不抢 mod+z', () => {
    expect(dispatchKeybinding({ key: 'z', ctrlKey: true } as KeyboardEvent, gridCtx())).toBe(true)
    expect(undo).toHaveBeenCalledOnce()
    expect(dispatchKeybinding({ key: 'y', metaKey: true } as KeyboardEvent, gridCtx())).toBe(true)
    expect(redo).toHaveBeenCalledOnce()
    expect(
      dispatchKeybinding({ key: 'z', ctrlKey: true, shiftKey: true } as KeyboardEvent, gridCtx()),
    ).toBe(true)
    expect(redo).toHaveBeenCalledTimes(2)
    expect(undo).toHaveBeenCalledOnce() // mod+shift+z 未误触 undo
  })

  it('F11 裸多字符键命中;带修饰符的事件不回落裸键(ctrl+F11 ≠ F11)', () => {
    expect(dispatchKeybinding({ key: 'F11' } as KeyboardEvent, gridCtx())).toBe(true)
    expect(fs).toHaveBeenCalledOnce()
    expect(dispatchKeybinding({ key: 'F11', ctrlKey: true } as KeyboardEvent, gridCtx())).toBe(
      false,
    )
    expect(fs).toHaveBeenCalledOnce()
  })

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
  it('register / get / all 保持注册顺序', () => {
    const r = createCommandRegistry()
    r.register(cmd({ id: 'a' }))
    r.register(cmd({ id: 'b' }))
    expect(r.get('a')?.id).toBe('a')
    expect(r.get('missing')).toBeUndefined()
    expect(r.all().map((c) => c.id)).toEqual(['a', 'b'])
  })

  it('重复 id 覆盖 + 告警', () => {
    const r = createCommandRegistry()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    r.register(cmd({ id: 'a', title: 'first' }))
    r.register(cmd({ id: 'a', title: 'second' }))
    expect(r.get('a')?.title).toBe('second')
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  it('query 按 when 过滤', () => {
    const r = createCommandRegistry()
    r.register(cmd({ id: 'always' }))
    r.register(cmd({ id: 'sel', when: (c) => c.selection.count > 0 }))
    expect(r.query(makeCtx()).map((c) => c.id)).toEqual(['always'])
    expect(r.query(makeCtx({ selection: { count: 2, isSingle: false } })).map((c) => c.id)).toEqual([
      'always',
      'sel',
    ])
  })

  it('query 按 group 过滤 + order 升序(无 order 排末尾,稳定)', () => {
    const r = createCommandRegistry()
    r.register(cmd({ id: 'n2', group: 'navigation', order: 2 }))
    r.register(cmd({ id: 'n1', group: 'navigation', order: 1 }))
    r.register(cmd({ id: 'ov', group: 'overflow' }))
    r.register(cmd({ id: 'nNoOrder', group: 'navigation' }))
    const nav = r.query(makeCtx(), { group: 'navigation' })
    expect(nav.map((c) => c.id)).toEqual(['n1', 'n2', 'nNoOrder'])
  })

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

  it('run 未知 id 安全告警不抛', () => {
    const r = createCommandRegistry()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(() => r.run('nope', makeCtx())).not.toThrow()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  it('resolveCommandTitle 惰性求值(函数每次求值)', () => {
    expect(resolveCommandTitle(cmd({ id: 'a', title: 'lit' }))).toBe('lit')
    let n = 0
    const c = cmd({ id: 'b', title: () => 'dyn' + ++n })
    expect(resolveCommandTitle(c)).toBe('dyn1')
    expect(resolveCommandTitle(c)).toBe('dyn2')
  })

  it('isCommandVisible/Enabled/Active 缺省语义', () => {
    const ctx = makeCtx()
    const bare = cmd({ id: 'bare' })
    expect(isCommandVisible(bare, ctx)).toBe(true)
    expect(isCommandEnabled(bare, ctx)).toBe(true)
    expect(isCommandActive(bare, ctx)).toBe(false)
  })
})
