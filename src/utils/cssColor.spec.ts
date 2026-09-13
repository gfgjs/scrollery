// cssColor 单测（局部修复 2026-09-12）：computed style 对未注册自定义属性只做 var() 代换，
// color-mix()/calc() 等函数原样保留，因此「无 var() 即纯色」的旧判定会把复杂表达式当具体色返回，
// 下游 parseColorToRgb 解析失败落到硬编码底色（媒体网格黑条）。
// node 环境无 DOM：用最小假 document/getComputedStyle 覆盖探针路径，不 mock resolveTokenColor 本身。
import { describe, it, expect, afterEach, vi } from 'vitest'
import { resolveTokenColor } from './cssColor'
import { readPalette } from '../components/media/mediaGridCanvas.palette'

// 真实回读串（浅色主题 color-mix 型 token 声明形态，如 --color-bg-primary/--color-bg-canvas-gap）：
// var() 已展开，color-mix()/calc() 保留。
const RAW_COLOR_MIX = 'color-mix(in oklch, #edf5f1 calc(0.6 * 100%), #ffffff)'
// 真实探针回读串：探针元素 color: var(--token) 后 computed color 序列化为 oklch，近中性色色相为 none。
const PROBE_OKLCH = 'oklch(0.977804 0.005983 none)'

interface FakeEl {
  style: Record<string, string>
  parent: FakeScope | null
  removed: boolean
  remove(): void
}

interface FakeScope {
  style: Record<string, string>
  children: FakeEl[]
  appendChild(el: FakeEl): void
}

/** 最小 DOM 替身：scope 提供 token 表，探针元素的 computed color 固定为 probeColor。 */
function createDom(tokens: Record<string, string>, probeColor: string) {
  const scope: FakeScope = {
    style: {},
    children: [],
    appendChild(el: FakeEl) {
      el.parent = scope
      scope.children.push(el)
    },
  }
  const probes: FakeEl[] = []
  const documentMock = {
    createElement() {
      const el: FakeEl = {
        style: {},
        parent: null,
        removed: false,
        remove() {
          el.removed = true
          if (el.parent) el.parent.children = el.parent.children.filter((c) => c !== el)
        },
      }
      probes.push(el)
      return el
    },
  }
  const getComputedStyleMock = (el: unknown) => {
    if (el === scope) return { getPropertyValue: (name: string) => tokens[name] ?? '', color: '' }
    return { getPropertyValue: () => '', color: probeColor }
  }
  vi.stubGlobal('document', documentMock)
  vi.stubGlobal('getComputedStyle', getComputedStyleMock)
  return { scope, probes, el: scope as unknown as HTMLElement }
}

afterEach(() => vi.unstubAllGlobals())

describe('resolveTokenColor', () => {
  it('简单具体色走快路径：原样返回且不建探针', () => {
    const dom = createDom({ '--color-text-primary': '#1a1a1a' }, PROBE_OKLCH)
    expect(resolveTokenColor(dom.el, '--color-text-primary', '#eee')).toBe('#1a1a1a')
    expect(dom.probes).toHaveLength(0)
  })

  it('computed 已展开 var 的 color-mix 表达式交 DOM 探针，返回探针具体色', () => {
    const dom = createDom({ '--color-bg-primary': RAW_COLOR_MIX }, PROBE_OKLCH)
    expect(resolveTokenColor(dom.el, '--color-bg-primary', '#222')).toBe(PROBE_OKLCH)
    // 探针确实用 token 取值（而非把 raw 原样丢回）。
    expect(dom.probes[0]?.style.color).toBe('var(--color-bg-primary)')
  })

  it('token 缺省：回退 fallback，不建探针', () => {
    const dom = createDom({}, PROBE_OKLCH)
    expect(resolveTokenColor(dom.el, '--color-bg-primary', '#222')).toBe('#222')
    expect(dom.probes).toHaveLength(0)
  })

  it('探针结果为空：回退 fallback', () => {
    const dom = createDom({ '--color-bg-primary': RAW_COLOR_MIX }, '')
    expect(resolveTokenColor(dom.el, '--color-bg-primary', '#222')).toBe('#222')
  })

  it('清理：探针从 scope 移除', () => {
    const dom = createDom({ '--color-bg-primary': RAW_COLOR_MIX }, PROBE_OKLCH)
    resolveTokenColor(dom.el, '--color-bg-primary', '#222')
    expect(dom.probes[0]?.removed).toBe(true)
    expect(dom.scope.children).toHaveLength(0)
  })
})

describe('readPalette（集成）', () => {
  it('色相为 none 的 oklch 底色经探针解析为 canvasGap，纯色 token 仍走快路径', () => {
    // 任一 color-mix 型 token 都走探针路径，这里取 readPalette 中仍由 gc() 解析的 canvasGap。
    const dom = createDom(
      { '--color-bg-canvas-gap': RAW_COLOR_MIX, '--color-text-primary': '#1a1a1a' },
      PROBE_OKLCH,
    )
    const palette = readPalette(dom.el)
    expect(palette.canvasGap).toBe(PROBE_OKLCH)
    expect(palette.textPrimary).toBe('#1a1a1a')
    // 只有 color-mix 型 token 建探针：纯色 token 走快路径。
    expect(dom.probes).toHaveLength(1)
  })
})
