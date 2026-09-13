// useRovingTabindex 纯核单测(S3)。nextRovingIndex 的索引数学:方向键→下一焦点索引,跳过 disabled、
// 端点环绕、朝向门控。与 DOM/焦点无关,可穷举。DOM 焦点管理部分属 ⏸GUI,不在此测。
import { describe, it, expect } from 'vitest'
import { nextRovingIndex } from './useRovingTabindex'

// 便捷:全可用的 N 项 mask。
const on = (n: number): boolean[] => Array<boolean>(n).fill(true)

describe('nextRovingIndex（roving tabindex 纯核）', () => {
  it('空组 → null(任何键)', () => {
    expect(nextRovingIndex('ArrowRight', 0, [])).toBeNull()
    expect(nextRovingIndex('Home', 0, [])).toBeNull()
  })

  it('horizontal(默认):Right/Left 前后移一位', () => {
    expect(nextRovingIndex('ArrowRight', 0, on(3))).toBe(1)
    expect(nextRovingIndex('ArrowLeft', 2, on(3))).toBe(1)
  })

  it('horizontal:Right 到末尾环绕到首,Left 到首环绕到末', () => {
    expect(nextRovingIndex('ArrowRight', 2, on(3))).toBe(0)
    expect(nextRovingIndex('ArrowLeft', 0, on(3))).toBe(2)
  })

  it('horizontal 不响应 Up/Down → null(不 preventDefault,放行原生)', () => {
    expect(nextRovingIndex('ArrowDown', 0, on(3))).toBeNull()
    expect(nextRovingIndex('ArrowUp', 1, on(3))).toBeNull()
  })

  it('vertical:Up/Down 响应,Left/Right 不响应', () => {
    expect(nextRovingIndex('ArrowDown', 0, on(3), 'vertical')).toBe(1)
    expect(nextRovingIndex('ArrowUp', 0, on(3), 'vertical')).toBe(2)
    expect(nextRovingIndex('ArrowRight', 0, on(3), 'vertical')).toBeNull()
    expect(nextRovingIndex('ArrowLeft', 0, on(3), 'vertical')).toBeNull()
  })

  it('both:四向都响应', () => {
    expect(nextRovingIndex('ArrowRight', 0, on(2), 'both')).toBe(1)
    expect(nextRovingIndex('ArrowDown', 0, on(2), 'both')).toBe(1)
  })

  it('Home/End 恒响应(不论朝向),取首/末可用项', () => {
    expect(nextRovingIndex('Home', 2, on(3))).toBe(0)
    expect(nextRovingIndex('End', 0, on(3))).toBe(2)
    expect(nextRovingIndex('Home', 2, on(3), 'vertical')).toBe(0)
    expect(nextRovingIndex('End', 0, on(3), 'both')).toBe(2)
  })

  it('跳过 disabled 项:Right 从 0 越过禁用的 1 落到 2', () => {
    // 索引 1 禁用
    expect(nextRovingIndex('ArrowRight', 0, [true, false, true])).toBe(2)
  })

  it('跳过 disabled + 环绕:Right 从 2 越过禁用的 0 落到 1', () => {
    expect(nextRovingIndex('ArrowRight', 2, [false, true, true])).toBe(1)
  })

  it('Home/End 跳过端点 disabled 项', () => {
    // 首项禁用 → Home 落到 1;末项禁用 → End 落到 1
    expect(nextRovingIndex('Home', 2, [false, true, false])).toBe(1)
    expect(nextRovingIndex('End', 0, [false, true, false])).toBe(1)
  })

  it('仅当前项可用 → 焦点不动(返回自身)', () => {
    expect(nextRovingIndex('ArrowRight', 1, [false, true, false])).toBe(1)
  })

  it('全部 disabled → null(无可聚焦项)', () => {
    expect(nextRovingIndex('ArrowRight', 0, [false, false])).toBeNull()
    expect(nextRovingIndex('Home', 0, [false, false])).toBeNull()
  })

  it('未处理键 → null(如 Enter/Tab 放行)', () => {
    expect(nextRovingIndex('Enter', 0, on(3))).toBeNull()
    expect(nextRovingIndex('Tab', 0, on(3))).toBeNull()
  })

  it('当前项本身 disabled 时仍能移到其它可用项(from 越界式定位)', () => {
    // from=1 禁用,Right 找 2、Left 找 0
    expect(nextRovingIndex('ArrowRight', 1, [true, false, true])).toBe(2)
    expect(nextRovingIndex('ArrowLeft', 1, [true, false, true])).toBe(0)
  })
})
