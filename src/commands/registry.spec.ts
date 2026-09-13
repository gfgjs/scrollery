// registry 单测(顶栏重构 P2-1)。锁注册表机制:注册/覆盖/query(when·group·order)/run 守卫/
// 标题惰性求值。ctx 只穿可变调用上下文(store 不入 context,命令 run 内直接 useXxxStore()),
// 故测试无需 pinia。
import { describe, it, expect, vi } from 'vitest'
import {
  createCommandRegistry,
  resolveCommandTitle,
  isCommandVisible,
  isCommandEnabled,
  isCommandActive,
} from './registry'
import type { Command, CommandContext } from './types'

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
    const c = cmd({ id: 'b', title: () => `dyn${++n}` })
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
