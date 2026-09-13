// 键位同源单测(顶栏重构 P5-6)。formatKeybinding / normalizeEventKey 纯函数;dispatchKeybinding
// 用真单例注册表 + 手搭 ctx(不经 pinia)验证「按 keybinding 分发到 viewer 命令并执行」。
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { formatKeybinding, normalizeEventKey, eventToCombo, dispatchKeybinding } from './keybinding'
import { commandRegistry } from './registry'
import { viewerImageCommands } from './builtins/viewer-image'
import type { CommandContext } from './types'
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

// ── 组合键(2026-07-10 深审 HIGH-2 修复:此前分发器只匹配裸键,mod+z 结构上永不命中)──────
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
