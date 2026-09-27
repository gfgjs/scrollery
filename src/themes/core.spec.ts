// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, expect, it } from 'vitest'
import { contrastRatio } from '../utils/color'
import { MIN_TEXT_CONTRAST, generateTheme, paletteToCssVars } from './generate'
import { DEFAULT_LIGHT_SEED, DEFAULT_THEME_DEFINITION } from './presets'
import { runInNewContext } from 'node:vm'
import { buildBootstrapScript } from './bootstrapScript'
import { THEME_CACHE_KEY, buildThemeCache } from './snapshot'

describe('主题可读性', () => {


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
      ...buildThemeCache('system', lightPalette, darkPalette, 'none', 90, 'standard'),
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
  })
})
