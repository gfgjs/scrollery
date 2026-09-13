// src/themes/reader-theme-contract.spec.ts
// 阅读主题契约 + 对比度门禁（阅读器方案 R3）。与 app 主题契约（theme-contract.spec.ts）同源思想，
// 但阅读主题只有一对正文/背景色，故门禁核心是**可读性**：正文与背景的 WCAG 对比度须达 AA 正文阈值。
// 新增或调整阅读主题若把对比度调塌，此测直接红——防「好看但读不清」的底色进产品。
import { describe, it, expect } from 'vitest'
import {
  READER_THEMES,
  READER_THEME_FOLLOW,
  getReaderTheme,
  readerThemesByKind,
  normalizeReaderThemeId,
} from './readerThemes'
import { contrastRatio, parseColorToRgb } from '../utils/color'

// AA 正文对比度阈值。阅读正文虽偏大，但仍按最严的正文标准把关（大字 3.0 不够稳）。
const AA_BODY = 4.5

describe('阅读主题契约', () => {
  it('id 唯一且非空', () => {
    const ids = READER_THEMES.map((t) => t.id)
    expect(ids.every((id) => id.length > 0)).toBe(true)
    expect(new Set(ids).size).toBe(ids.length)
    // FOLLOW 是哨兵，不得混入调色板条目（否则 UI 会把它当可选主题渲染两次）。
    expect(ids).not.toContain(READER_THEME_FOLLOW)
  })

  it('每主题正文/背景均为可解析颜色', () => {
    for (const t of READER_THEMES) {
      expect(parseColorToRgb(t.text), `${t.id}.text 非法`).not.toBeNull()
      expect(parseColorToRgb(t.background), `${t.id}.background 非法`).not.toBeNull()
    }
  })

  it('正文↔背景对比度达 AA 正文阈值（≥4.5）', () => {
    for (const t of READER_THEMES) {
      const ratio = contrastRatio(t.text, t.background)
      expect(ratio, `${t.id} 对比度 ${ratio.toFixed(2)} 低于 AA ${AA_BODY}`).toBeGreaterThanOrEqual(
        AA_BODY,
      )
    }
  })

  it('kind 与背景明暗自洽（light 主题背景更亮、dark 更暗）', () => {
    for (const t of READER_THEMES) {
      const bg = parseColorToRgb(t.background)!
      const brightness = (bg.r + bg.g + bg.b) / 3
      if (t.kind === 'light') expect(brightness, `${t.id} 标 light 但背景偏暗`).toBeGreaterThan(180)
      else expect(brightness, `${t.id} 标 dark 但背景偏亮`).toBeLessThan(80)
    }
  })

  it('明暗两槽各至少有一个可选主题', () => {
    expect(readerThemesByKind('light').length).toBeGreaterThanOrEqual(1)
    expect(readerThemesByKind('dark').length).toBeGreaterThanOrEqual(1)
  })
})

describe('normalizeReaderThemeId', () => {
  it('空 / null / FOLLOW → FOLLOW', () => {
    expect(normalizeReaderThemeId(null, 'light')).toBe(READER_THEME_FOLLOW)
    expect(normalizeReaderThemeId('', 'dark')).toBe(READER_THEME_FOLLOW)
    expect(normalizeReaderThemeId(READER_THEME_FOLLOW, 'light')).toBe(READER_THEME_FOLLOW)
  })

  it('有效且 kind 相符 → 原样保留', () => {
    expect(normalizeReaderThemeId('paper', 'light')).toBe('paper')
    expect(normalizeReaderThemeId('night', 'dark')).toBe('night')
  })

  it('kind 不符 / 未注册 → 回落 FOLLOW（防跨槽误存 / 已卸载主题）', () => {
    expect(normalizeReaderThemeId('paper', 'dark')).toBe(READER_THEME_FOLLOW) // paper 是 light
    expect(normalizeReaderThemeId('night', 'light')).toBe(READER_THEME_FOLLOW) // night 是 dark
    expect(normalizeReaderThemeId('no-such-theme', 'light')).toBe(READER_THEME_FOLLOW)
  })

  it('getReaderTheme 命中 / 未命中', () => {
    expect(getReaderTheme('sepia')?.kind).toBe('light')
    expect(getReaderTheme('nope')).toBeUndefined()
  })
})
