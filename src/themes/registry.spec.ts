import { describe, expect, it } from 'vitest'
import {
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  THEME_STYLES,
  getThemeStyle,
  normalizeThemeId,
  themesByKind,
  themeIdForStyle,
} from './registry'

describe('主题风格注册表', () => {
  it('注册三种风格并为每种风格提供明暗配对', () => {
    expect(THEME_STYLES.map((style) => style.id)).toEqual(['fresh', 'minimal', 'tech'])
    for (const style of THEME_STYLES) {
      expect(style.themes.light.id).toBe(`${style.id}-light`)
      expect(style.themes.dark.id).toBe(`${style.id}-dark`)
      expect(getThemeStyle(style.id)).toBe(style)
    }
    expect(BUILTIN_THEMES).toHaveLength(6)
    expect(themesByKind('light')).toHaveLength(3)
    expect(themesByKind('dark')).toHaveLength(3)
  })

  it('把旧主题 id 和 legacy 明暗值归一化到新主题', () => {
    expect(normalizeThemeId('moonlight', 'light')).toBe('fresh-light')
    expect(normalizeThemeId('porcelain', 'light')).toBe('minimal-light')
    expect(normalizeThemeId('xuan', 'light')).toBe('tech-light')
    expect(normalizeThemeId('ink', 'dark')).toBe('fresh-dark')
    expect(normalizeThemeId('obsidian', 'dark')).toBe('minimal-dark')
    expect(normalizeThemeId('dai', 'dark')).toBe('tech-dark')
    expect(normalizeThemeId('light', 'dark')).toBe(DEFAULT_DARK_THEME)
    expect(normalizeThemeId('dark', 'light')).toBe(DEFAULT_LIGHT_THEME)
    expect(normalizeThemeId('unknown', 'light')).toBe(DEFAULT_LIGHT_THEME)
  })

  it('根据风格返回明暗主题 id', () => {
    expect(themeIdForStyle('fresh', 'light')).toBe('fresh-light')
    expect(themeIdForStyle('minimal', 'dark')).toBe('minimal-dark')
    expect(themeIdForStyle('tech', 'light')).toBe('tech-light')
    expect(themeIdForStyle('unknown', 'dark')).toBe(DEFAULT_DARK_THEME)
  })
})
