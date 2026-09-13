import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { BUILTIN_THEMES, normalizeThemeId } from './registry'

const snapshotScript = readFileSync(new URL('../../public/theme-snapshot.js', import.meta.url), 'utf8')
const startupHtml = readFileSync(new URL('../../index.html', import.meta.url), 'utf8')

/** 执行真实首帧脚本，覆盖 Vue 挂载前无法依赖 store 的路径。 */
function firstFrame(snapshot: string | null, systemDark = false, storageUnavailable = false) {
  const attributes: Record<string, string> = {}
  const styleProps: Record<string, string> = {}
  runInNewContext(snapshotScript, {
    localStorage: {
      getItem: () => {
        if (storageUnavailable) throw new Error('storage unavailable')
        return snapshot
      },
    },
    window: { matchMedia: () => ({ matches: systemDark }) },
    document: {
      documentElement: {
        setAttribute: (name: string, value: string) => {
          attributes[name] = value
        },
        style: {
          setProperty: (name: string, value: string) => {
            styleProps[name] = value
          },
        },
      },
    },
  })
  return { attributes, styleProps }
}

describe('主题首帧与运行时一致', () => {
  it.each(BUILTIN_THEMES)('$id 的首帧命中当前主题', (theme) => {
    const { attributes } = firstFrame(
      JSON.stringify({ appearance: theme.kind, [theme.kind]: theme.id }),
    )
    expect(attributes['data-theme']).toBe(theme.id)
    expect(attributes['data-color-scheme']).toBe(theme.kind)
  })

  it.each([
    'moonlight', 'porcelain', 'xuan', 'ink', 'obsidian', 'dai', 'light', 'dark', 'unknown', '', null,
  ])(
    '旧值或无效值 %s 与注册表使用相同归一化规则',
    (raw) => {
      for (const kind of ['light', 'dark'] as const) {
        const { attributes } = firstFrame(JSON.stringify({ appearance: kind, [kind]: raw }))
        expect(attributes['data-theme']).toBe(normalizeThemeId(raw, kind))
      }
    },
  )

  it('跟随系统使用对应的明暗配色', () => {
    const snapshot = JSON.stringify({ appearance: 'system', light: 'tech-light', dark: 'tech-dark' })
    expect(firstFrame(snapshot, false).attributes['data-theme']).toBe('tech-light')
    expect(firstFrame(snapshot, true).attributes['data-theme']).toBe('tech-dark')
  })

  it('快照缺失、损坏或存储不可用时按系统明暗使用清新风格', () => {
    for (const snapshot of [null, '{broken', 'null']) {
      expect(firstFrame(snapshot).attributes['data-theme']).toBe('fresh-light')
      expect(firstFrame(snapshot, true).attributes['data-theme']).toBe('fresh-dark')
    }
    expect(firstFrame(null, true, true).attributes['data-theme']).toBe('fresh-dark')
  })

  it('快照携带浓度并在首帧写入 --theme-tint-scale', () => {
    expect(firstFrame(JSON.stringify({ tint: 60 })).styleProps['--theme-tint-scale']).toBe('0.6')
    expect(firstFrame(JSON.stringify({ tint: 100 })).styleProps['--theme-tint-scale']).toBe('1')
    expect(firstFrame(JSON.stringify({ tint: 0 })).styleProps['--theme-tint-scale']).toBe('0')
    expect(firstFrame(JSON.stringify({ tint: 20 })).styleProps['--theme-tint-scale']).toBe('0.2')
  })

  it('浓度缺位(旧版本快照)或越界时落默认 60,与 themes/strength.ts 的默认一致', () => {
    for (const tint of [undefined, -1, -20, 130, 999, '60', null]) {
      const { styleProps } = firstFrame(JSON.stringify({ tint }))
      expect(styleProps['--theme-tint-scale'], `tint=${String(tint)}`).toBe('0.6')
    }
  })

  it('快照携带文字浓度并在首帧写入 --theme-text-scale', () => {
    expect(firstFrame(JSON.stringify({ text: 75 })).styleProps['--theme-text-scale']).toBe('0.75')
    expect(firstFrame(JSON.stringify({ text: 100 })).styleProps['--theme-text-scale']).toBe('1')
    expect(firstFrame(JSON.stringify({ text: 40 })).styleProps['--theme-text-scale']).toBe('0.4')
  })

  it('文字浓度缺位或越界时落默认 100,与 themes/strength.ts 的默认一致', () => {
    for (const text of [undefined, 0, 10, 130, 999, '75', null]) {
      const { styleProps } = firstFrame(JSON.stringify({ text }))
      expect(styleProps['--theme-text-scale'], `text=${String(text)}`).toBe('1')
    }
  })
})

describe('启动层底色与主题锚点一致', () => {
  // 底色自 2026-09-06 起是 color-mix 浓度表达式(消费 --theme-tint-scale),锚点=满浓度原色;
  // 此处断言锚点与主题预览一致,保证首帧底色随浓度走且与水合后同源。
  function startupBgAnchor(themeId: string): string | null {
    const selector = `html[data-theme='${themeId}']`
    const start = startupHtml.indexOf(selector)
    if (start < 0) return null
    const block = startupHtml.slice(start, startupHtml.indexOf('}', start))
    const m = block.match(/--startup-bg:\s*color-mix\(in oklch, (#[0-9a-fA-F]{6}) /)
    return m?.[1] ?? null
  }

  it.each(BUILTIN_THEMES)('$id 的启动层底色锚点 = 主题预览 bg', (theme) => {
    expect(startupBgAnchor(theme.id), `${theme.id} 启动层底色锚点漂移`).toBe(theme.preview.bg)
  })

  // 启动层文字自 2026-09-06 起也是 color-mix 浓度表达式(消费 --theme-text-scale),
  // 与底色同法断言锚点=预览文字色。
  function startupTextAnchor(themeId: string): string | null {
    const selector = `html[data-theme='${themeId}']`
    const start = startupHtml.indexOf(selector)
    if (start < 0) return null
    const block = startupHtml.slice(start, startupHtml.indexOf('}', start))
    const m = block.match(/--startup-text:\s*color-mix\(in oklch, (#[0-9a-fA-F]{6}) /)
    return m?.[1] ?? null
  }

  it.each(BUILTIN_THEMES)('$id 的启动层文字锚点 = 主题预览 text', (theme) => {
    expect(startupTextAnchor(theme.id), `${theme.id} 启动层文字锚点漂移`).toBe(theme.preview.text)
  })

  it.each(BUILTIN_THEMES)('$id 的启动层强调色和主题预览一致', (theme) => {
    const selector = `html[data-theme='${theme.id}']`
    const start = startupHtml.indexOf(selector)
    const block = startupHtml.slice(start, startupHtml.indexOf('}', start))
    expect(block).toContain(`--startup-accent: ${theme.preview.accent};`)
  })
})
