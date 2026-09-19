// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, expect, it, vi } from 'vitest'
import { contrastRatio } from '../utils/color'
import { minContrast } from './colors'
import { CONTRAST_MAX, CONTRAST_MIN, MIN_CONTROL_CONTRAST, MIN_TEXT_CONTRAST, PALETTE_CSS_VARS, THEME_PALETTE_VARS, generateTheme, paletteToCssVars } from './generate'
import { BUILTIN_PRESETS, DEFAULT_DARK_SEED, DEFAULT_LIGHT_SEED, DEFAULT_THEME_DEFINITION } from './presets'
import { type ThemeMode, type ThemePalette, type SavedTheme } from './types'
import { runInNewContext } from 'node:vm'
import { buildBootstrapScript } from './bootstrapScript'
import { THEME_CACHE_KEY, THEME_CACHE_VERSION, buildThemeCache, parseThemeCache } from './snapshot'
import { THEME_SETTING_KEYS, parseSavedThemes, parseThemeDefinition, serializeThemeSeed, toFlatSavedTheme, toThemeDefinitionPatch } from './config'
import { GLASS_ATTR, applyMaterial, applyPalette } from './apply'

describe('主题可读性', () => {
  const MODES: readonly ThemeMode[] = ['light', 'dark']

  /** 有含义控件边界的实际承载面(与生成时判定的一致)。 */
  function controlBases(palette: ThemePalette): string[] {
    return [palette.background, palette.surface, palette.elevated]
  }

  describe('generateTheme:种子不变性与确定性', () => {
    it('层次对比度不改变用户种子,只改变派生层', () => {
      for (const mode of MODES) {
        const seed = mode === 'light' ? DEFAULT_LIGHT_SEED : DEFAULT_DARK_SEED
        const low = generateTheme({ ...seed, contrast: CONTRAST_MIN }, mode)
        const high = generateTheme({ ...seed, contrast: CONTRAST_MAX }, mode)
        for (const key of ['background', 'textPrimary', 'accent', 'canvas'] as const) {
          expect(low[key], key).toBe(high[key])
        }
        expect(high.textSecondary).not.toBe(low.textSecondary)
        expect(high.border).not.toBe(low.border)
        expect(high.divider).not.toBe(low.divider)
      }
    })
  })

  describe('generateTheme:显式画廊底色(含反极性)', () => {
    it('浅色主题配深色画廊:用户前景不变,画廊文字按画廊底色单独派生', () => {
      const palette = generateTheme({ ...DEFAULT_LIGHT_SEED, gallery: '#141821' }, 'light')
      expect(palette.canvas).toBe('#141821')
      expect(palette.canvasGap).toBe('#141821')
      // 用户前景不被画廊设置改写——它仍服务窗口底面。
      expect(palette.background).toBe(DEFAULT_LIGHT_SEED.background)
      expect(palette.textPrimary).toBe(DEFAULT_LIGHT_SEED.foreground)
      // 原前景在深色画廊上不可读,画廊文字必须另派生并达标。
      expect(contrastRatio(palette.textPrimary, palette.canvas)).toBeLessThan(MIN_TEXT_CONTRAST)
      expect(contrastRatio(palette.canvasText, palette.canvas)).toBeGreaterThanOrEqual(
        MIN_TEXT_CONTRAST,
      )
      expect(contrastRatio(palette.canvasTextSecondary, palette.canvas)).toBeGreaterThanOrEqual(
        MIN_TEXT_CONTRAST,
      )
      // 占位块与纸面从画廊底色出发,而不是从窗口底色。
      expect(palette.canvasPlaceholder).not.toBe(palette.canvasGap)
      // 与 auto 的结果不同:占位块取自画廊底色,不是窗口底色。
      expect(palette.canvasPlaceholder).not.toBe(
        generateTheme(DEFAULT_LIGHT_SEED, 'light').canvasPlaceholder,
      )
    })

    it('深色主题配浅色画廊同样可用', () => {
      const palette = generateTheme({ ...DEFAULT_DARK_SEED, gallery: '#f5f5f5' }, 'dark')
      expect(palette.canvas).toBe('#f5f5f5')
      expect(contrastRatio(palette.canvasText, palette.canvas)).toBeGreaterThanOrEqual(
        MIN_TEXT_CONTRAST,
      )
      expect(contrastRatio(palette.textPrimary, palette.canvas)).toBeLessThan(MIN_TEXT_CONTRAST)
    })
  })

  describe('generateTheme:预设对比门槛', () => {
    it('正文与辅助文字在全部承载面上 ≥4.5', () => {
      for (const mode of MODES) {
        for (const preset of BUILTIN_PRESETS) {
          const p = generateTheme(preset.definition[mode], mode)
          const label = preset.id + ' ' + mode
          for (const base of [p.background, p.surface, p.elevated]) {
            expect(
              contrastRatio(p.textPrimary, base),
              label + ' 正文 × ' + base,
            ).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
            expect(
              contrastRatio(p.textSecondary, base),
              label + ' 辅助文字 × ' + base,
            ).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
          }
        }
      }
    })

    it('有含义的控件边界与控件轨道 ≥3;装饰性分隔线不套该门槛', () => {
      for (const mode of MODES) {
        for (const preset of BUILTIN_PRESETS) {
          const p = generateTheme(preset.definition[mode], mode)
          const label = preset.id + ' ' + mode
          const bases = controlBases(p)
          expect(minContrast(p.controlBorder, bases), label + ' 控件边界').toBeGreaterThanOrEqual(
            MIN_CONTROL_CONTRAST,
          )
          expect(minContrast(p.controlTrack, bases), label + ' 开关轨道').toBeGreaterThanOrEqual(
            MIN_CONTROL_CONTRAST,
          )
          expect(minContrast(p.divider, bases), label + ' 分隔线').toBeLessThan(MIN_CONTROL_CONTRAST)
        }
      }
    })

    it('强调色文字 ≥4.5,强调填充文字在填充与其悬停色上 ≥4.5', () => {
      for (const mode of MODES) {
        for (const preset of BUILTIN_PRESETS) {
          const p = generateTheme(preset.definition[mode], mode)
          const label = preset.id + ' ' + mode
          expect(
            minContrast(p.accentText, [p.background, p.surface, p.selection]),
            label + ' 强调色文字',
          ).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
          expect(contrastRatio(p.textOnAccent, p.accent), label + ' 强调填充').toBeGreaterThanOrEqual(
            MIN_TEXT_CONTRAST,
          )
          expect(
            contrastRatio(p.textOnAccent, p.accentHover),
            label + ' 强调填充悬停',
          ).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
        }
      }
    })

    it('状态填充上的文字在各自语义色上 ≥4.5', () => {
      const onKeys = {
        success: 'textOnSuccess',
        warning: 'textOnWarning',
        error: 'textOnError',
        info: 'textOnInfo',
      } as const
      for (const mode of MODES) {
        for (const preset of BUILTIN_PRESETS) {
          const p = generateTheme(preset.definition[mode], mode)
          const label = preset.id + ' ' + mode
          for (const key of ['success', 'warning', 'error', 'info'] as const) {
            expect(contrastRatio(p[onKeys[key]], p[key]), label + ' ' + key).toBeGreaterThanOrEqual(
              MIN_TEXT_CONTRAST,
            )
          }
        }
      }
    })
  })

  describe('色板 → CSS 变量映射', () => {
    it('映射键集与色板字段完全一致(新增用途必须同批接线)', () => {
      const paletteKeys = Object.keys(generateTheme(DEFAULT_LIGHT_SEED, 'light')).sort()
      expect(Object.keys(PALETTE_CSS_VARS).sort()).toEqual(paletteKeys)
    })
  })
})

describe('首帧恢复', () => {
  const script = buildBootstrapScript()

  const lightPalette = generateTheme(DEFAULT_THEME_DEFINITION.light, 'light')

  const darkPalette = generateTheme(DEFAULT_THEME_DEFINITION.dark, 'dark')

  const lightVars = paletteToCssVars(lightPalette)

  const darkVars = paletteToCssVars(darkPalette)

  interface BootOptions {
    systemDark?: boolean
    /** null = 无 plugin-os 注入(裸浏览器 dev),走 UA 兜底。 */
    platform?: string | null
    userAgent?: string
  }

  /** 执行真实首帧脚本:覆盖 Vue 挂载前无法依赖 store 的路径。 */
  function boot(raw: string | null, options: BootOptions = {}) {
    const attributes: Record<string, string> = {}
    const styleProps: Record<string, string> = {}
    const platform = options.platform === undefined ? 'windows' : options.platform
    const win: Record<string, unknown> = {
      matchMedia: () => ({ matches: options.systemDark === true }),
      navigator: { userAgent: options.userAgent ?? 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' },
    }
    if (platform !== null) win.__TAURI_OS_PLUGIN_INTERNALS__ = { platform }
    runInNewContext(script, {
      localStorage: { getItem: (key: string) => (key === THEME_CACHE_KEY ? raw : null) },
      window: win,
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

  function cacheText(overrides: Record<string, unknown> = {}): string {
    return JSON.stringify({
      ...buildThemeCache('system', lightPalette, darkPalette, 'none', 90),
      ...overrides,
    })
  }

  describe('首帧脚本:有效缓存', () => {
    it('外观偏好优先于系统明暗', () => {
      const dark = boot(cacheText({ appearance: 'dark' }))
      expect(dark.attributes['data-color-scheme']).toBe('dark')
      expect(dark.styleProps).toEqual(darkVars)
      expect(boot(cacheText({ appearance: 'light' }), { systemDark: true }).styleProps).toEqual(
        lightVars,
      )
    })

    it('跟随系统时按系统明暗取对应一套', () => {
      const dark = boot(cacheText({ appearance: 'system' }), { systemDark: true })
      expect(dark.attributes['data-color-scheme']).toBe('dark')
      expect(dark.styleProps).toEqual(darkVars)
    })

    it('Windows + 材质非 none:恢复 data-glass 与不透明度', () => {
      const { attributes, styleProps } = boot(cacheText({ material: 'acrylic', opacity: 88 }))

      expect(attributes['data-glass']).toBe('acrylic')
      expect(styleProps['--window-opacity']).toBe('88%')
    })
  })

  describe('首帧脚本:无效缓存整份丢弃', () => {
    const missingOne: Record<string, string> = { ...lightVars }

    delete missingOne['--color-bg-primary']

    const cases: Array<[string, string]> = [
    ['版本不符', cacheText({ v: THEME_CACHE_VERSION + 1 })],
    ['变量缺失', cacheText({ light: missingOne })],
    ['值含 var()', cacheText({ light: { ...lightVars, '--color-accent': 'var(--x)' } })],
    ['JSON 损坏', '{broken']
    ]

    it.each(cases)('%s:脚本与解析器都判无效,且一个颜色都不写', (_name, raw) => {
      expect(parseThemeCache(raw)).toBeNull()

      const { attributes, styleProps } = boot(raw)

      expect(Object.keys(styleProps)).toHaveLength(0)
      expect(attributes['data-glass']).toBeUndefined()
      // 明暗属性仍要写:默认主题 CSS 靠它选深浅。
      expect(attributes['data-color-scheme']).toBe('light')
    })

    it('缓存缺失且系统为深色:明暗属性取深色,默认 CSS 的深色块生效', () => {
      const { attributes } = boot(null, { systemDark: true })

      expect(attributes['data-color-scheme']).toBe('dark')
    })
  })
})

describe('主题持久化', () => {
  const SAVED_THEME: SavedTheme = {
    id: 'mine-1',
    name: '我的配色',
    light: { ...DEFAULT_LIGHT_SEED, accent: '#1d4ed8', contrast: 50 },
    dark: { ...DEFAULT_DARK_SEED, gallery: '#101820' },
    material: 'acrylic',
    opacity: 85,
  }

  describe('当前主题的应用参数', () => {
    it('只有深色被改过时浅色仍取当前配置,不生成默认值', () => {
      const currentLight = { ...DEFAULT_LIGHT_SEED, accent: '#b21f26' }
      const definition = parseThemeDefinition({
        lightPalette: serializeThemeSeed(currentLight),
        darkPalette: '{broken',
        windowMaterial: 'nope',
        windowOpacity: 'abc',
      })
      expect(definition.light).toEqual(currentLight)
      expect(definition.dark).toEqual(DEFAULT_DARK_SEED)
      expect(definition.material).toBe('none')
      expect(definition.opacity).toBe(90)
    })

    it('应用参数往返:提交文本再读回得到同值', () => {
      const definition = { ...DEFAULT_THEME_DEFINITION, material: 'acrylic' as const, opacity: 66 }
      const patch = toThemeDefinitionPatch(definition)
      expect(
        parseThemeDefinition({
          lightPalette: patch[THEME_SETTING_KEYS.lightPalette],
          darkPalette: patch[THEME_SETTING_KEYS.darkPalette],
          windowMaterial: patch[THEME_SETTING_KEYS.windowMaterial],
          windowOpacity: patch[THEME_SETTING_KEYS.windowOpacity],
        }),
      ).toEqual(definition)
    })
  })

  describe('命名主题的扁平转换与往返', () => {
    it('重复 id 只保留首个(按稳定 ID 定位)', () => {
      const themes = parseSavedThemes([
        toFlatSavedTheme(SAVED_THEME),
        toFlatSavedTheme({ ...SAVED_THEME, name: '同 id 的另一份' }),
      ])
      expect(themes.map((theme) => theme.name)).toEqual(['我的配色'])
    })
  })
})

describe('主题发布', () => {
  /** 只需要 apply 层用到的那几件 DOM 能力(不引 jsdom,项目默认 node 环境)。 */
  function fakeRoot() {
    const attributes = new Map<string, string>()
    const props = new Map<string, string>()
    const setProperty = vi.fn((name: string, value: string) => {
      props.set(name, value)
    })
    const removeProperty = vi.fn((name: string) => {
      props.delete(name)
    })
    const root = {
      attributes,
      style: { setProperty, removeProperty },
      setProperty,
      removeProperty,
      setAttribute: (name: string, value: string) => {
        attributes.set(name, value)
      },
      removeAttribute: (name: string) => {
        attributes.delete(name)
      },
    }
    return { root, attributes, props, setProperty, removeProperty }
  }

  describe('applyPalette', () => {
    it('同值重复发布不再写样式(每帧发布不为未变的变量制造工作)', () => {
      const { root, setProperty } = fakeRoot()
      const palette = generateTheme(DEFAULT_THEME_DEFINITION.light, 'light')

      applyPalette(palette, root as unknown as HTMLElement)
      const first = setProperty.mock.calls.length
      applyPalette(palette, root as unknown as HTMLElement)

      expect(first).toBe(THEME_PALETTE_VARS.length)
      expect(setProperty.mock.calls.length).toBe(first)
    })
  })

  describe('applyMaterial', () => {
    it('从玻璃切到纯色会移除既有属性(不留上一材质的残值)', () => {
      const { root, attributes } = fakeRoot()
      applyMaterial('mica', 90, { root: root as unknown as HTMLElement, windows: true })
      applyMaterial('none', 90, { root: root as unknown as HTMLElement, windows: true })

      expect(attributes.has(GLASS_ATTR)).toBe(false)
    })
  })
})
