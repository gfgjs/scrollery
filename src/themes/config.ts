// src/themes/config.ts
// 主题配置的具名结构转换(方案 §3):扁平持久化 ↔ 运行期具名结构。
//
// IPC 上结构类设置一律是规范 JSON 文本(见 types/config.ts),本文件是主题域唯一的转换点:
// 字符串校验与越界收敛在输入/设置边界一次做完,运行期不再逐字段复查。
//
// 持久化键与结构:
//   appearance            system / light / dark(沿用原有语义,不由本文件转换)
//   theme_light_palette   ThemeSeed 固定结构的内联表
//   theme_dark_palette    ThemeSeed 固定结构的内联表
//   theme_saved_themes    命名主题数组,条目为固定扁平字段,默认空
//   window_material       none / mica / acrylic,新默认 none
//   window_opacity        0–100 整数,默认 90
//
// 命名主题条目用扁平字段(id / name / light_* / dark_* / material / opacity)是刻意的:落 TOML
// 结构数组后逐字段可读可手改,不必引入通用嵌套配置系统;运行期仍用具名 light / dark 结构。
//
// 纯数据转换:不依赖 Vue / Tauri / DOM,Node 侧 Vite 配置可直接 import。

import { normalizeHex } from './colors'
import { clampContrast } from './generate'
import { DEFAULT_DARK_SEED, DEFAULT_LIGHT_SEED, DEFAULT_THEME_DEFINITION } from './presets'
import {
  DEFAULT_WINDOW_OPACITY,
  GALLERY_AUTO,
  THEME_MATERIALS,
  WINDOW_OPACITY_MAX,
  WINDOW_OPACITY_MIN,
  type SavedTheme,
  type ThemeDefinition,
  type ThemeMaterial,
  type ThemeSeed,
} from './types'

/** 主题域消费的设置键(与 Rust schema 的键名一一对应)。 */
export const THEME_SETTING_KEYS = {
  appearance: 'appearance',
  lightPalette: 'theme_light_palette',
  darkPalette: 'theme_dark_palette',
  savedThemes: 'theme_saved_themes',
  windowMaterial: 'window_material',
  windowOpacity: 'window_opacity',
} as const

/** 命名主题的持久化条目:固定扁平字段,与运行期 SavedTheme 一一对应。 */
export interface SavedThemeFlatEntry {
  id: string
  name: string
  light_background: string
  light_foreground: string
  light_accent: string
  light_contrast: number
  light_gallery: string
  dark_background: string
  dark_foreground: string
  dark_accent: string
  dark_contrast: number
  dark_gallery: string
  material: ThemeMaterial
  opacity: number
}

function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {}
}

/** 解析规范 JSON 文本;空值或非法 JSON 返回 undefined(由调用方回落默认)。 */
function parseJsonText(raw: string | null | undefined): unknown {
  if (!raw) return undefined
  try {
    return JSON.parse(raw) as unknown
  } catch {
    return undefined
  }
}

/** 颜色字段:规范化为 #rrggbb,不可解析回落 fallback。 */
function asColor(value: unknown, fallback: string): string {
  return typeof value === 'string' ? (normalizeHex(value) ?? fallback) : fallback
}

/** 画廊字段:auto 或规范 #rrggbb,其余回落 fallback。 */
function asGallery(value: unknown, fallback: string): string {
  if (typeof value !== 'string') return fallback
  const trimmed = value.trim().toLowerCase()
  if (trimmed === GALLERY_AUTO) return GALLERY_AUTO
  return normalizeHex(trimmed) ?? fallback
}

/**
 * 十进制文本转数;空白与非数值一律 NaN(设置里空串表示「未设」,不能读成 0)。
 */
function asFiniteNumber(value: unknown): number {
  if (typeof value === 'number') return value
  if (typeof value !== 'string') return Number.NaN
  const text = value.trim()
  return text === '' ? Number.NaN : Number(text)
}

/** 层次对比度:接受数值或十进制文本,收敛为 0–100 整数,非法回落 fallback。 */
function asContrast(value: unknown, fallback: number): number {
  const n = asFiniteNumber(value)
  return Number.isFinite(n) ? clampContrast(n) : fallback
}

/** 窗口材质;未知值回落 fallback。 */
export function parseMaterial(
  raw: string | null | undefined,
  fallback: ThemeMaterial = 'none',
): ThemeMaterial {
  const value = raw?.trim()
  return THEME_MATERIALS.includes(value as ThemeMaterial)
    ? (value as ThemeMaterial)
    : fallback
}

/** 窗口不透明度:接受十进制文本或数值,收敛为 0–100 整数,非法回落 fallback。 */
export function parseOpacity(
  raw: string | number | null | undefined,
  fallback: number = DEFAULT_WINDOW_OPACITY,
): number {
  const n = asFiniteNumber(raw)
  if (!Number.isFinite(n)) return fallback
  return Math.min(WINDOW_OPACITY_MAX, Math.max(WINDOW_OPACITY_MIN, Math.round(n)))
}

/** 把任意来源(已解析 JSON / 内联对象)收敛为合法 ThemeSeed。 */
export function parseThemeSeed(value: unknown, fallback: ThemeSeed): ThemeSeed {
  const raw = asRecord(value)
  return {
    background: asColor(raw.background, fallback.background),
    foreground: asColor(raw.foreground, fallback.foreground),
    accent: asColor(raw.accent, fallback.accent),
    contrast: asContrast(raw.contrast, fallback.contrast),
    gallery: asGallery(raw.gallery, fallback.gallery),
  }
}

/** 从设置文本解析一份种子(空值 / 非法 JSON / 缺字段均逐项回落)。 */
export function parseThemeSeedText(
  raw: string | null | undefined,
  fallback: ThemeSeed,
): ThemeSeed {
  return parseThemeSeed(parseJsonText(raw), fallback)
}

/** 序列化为规范 JSON 文本(键序固定,颜色规范化,对比度收敛)。 */
export function serializeThemeSeed(seed: ThemeSeed): string {
  const canonical = parseThemeSeed(seed, seed)
  return JSON.stringify(canonical)
}

/** 当前完整主题在设置文本中的四个键(取值均为规范文本)。 */
export interface ThemeDefinitionTexts {
  lightPalette?: string | null
  darkPalette?: string | null
  windowMaterial?: string | null
  windowOpacity?: string | null
}

/** 当代前主题的应用参数:两套配色 + 材质。 */
export function parseThemeDefinition(
  texts: ThemeDefinitionTexts,
  fallback: ThemeDefinition = DEFAULT_THEME_DEFINITION,
): ThemeDefinition {
  return {
    light: parseThemeSeedText(texts.lightPalette, fallback.light),
    dark: parseThemeSeedText(texts.darkPalette, fallback.dark),
    material: parseMaterial(texts.windowMaterial, fallback.material),
    opacity: parseOpacity(texts.windowOpacity, fallback.opacity),
  }
}

/** 把完整主题转成一次批量提交用的键值表(全部为规范文本)。 */
export function toThemeDefinitionPatch(
  definition: ThemeDefinition,
): Record<string, string> {
  return {
    [THEME_SETTING_KEYS.lightPalette]: serializeThemeSeed(definition.light),
    [THEME_SETTING_KEYS.darkPalette]: serializeThemeSeed(definition.dark),
    [THEME_SETTING_KEYS.windowMaterial]: definition.material,
    [THEME_SETTING_KEYS.windowOpacity]: String(parseOpacity(definition.opacity)),
  }
}

/** 命名主题条目 → 扁平持久化结构。 */
export function toFlatSavedTheme(theme: SavedTheme): SavedThemeFlatEntry {
  const light = parseThemeSeed(theme.light, DEFAULT_LIGHT_SEED)
  const dark = parseThemeSeed(theme.dark, DEFAULT_DARK_SEED)
  return {
    id: theme.id.trim(),
    name: theme.name.trim(),
    light_background: light.background,
    light_foreground: light.foreground,
    light_accent: light.accent,
    light_contrast: light.contrast,
    light_gallery: light.gallery,
    dark_background: dark.background,
    dark_foreground: dark.foreground,
    dark_accent: dark.accent,
    dark_contrast: dark.contrast,
    dark_gallery: dark.gallery,
    material: parseMaterial(theme.material),
    opacity: parseOpacity(theme.opacity),
  }
}

/** 扁平持久化结构 → 命名主题(缺字段逐项回落出厂种子,不从别的条目借值)。 */
export function fromFlatSavedTheme(entry: unknown): SavedTheme | null {
  const flat = asRecord(entry)
  const id = typeof flat.id === 'string' ? flat.id.trim() : ''
  const name = typeof flat.name === 'string' ? flat.name.trim() : ''
  // 空 id / 空名称的条目非法:没有名称就没有可展示项,也没有稳定定位依据。
  if (!id || !name) return null
  return {
    id,
    name,
    light: parseThemeSeed(
      {
        background: flat.light_background,
        foreground: flat.light_foreground,
        accent: flat.light_accent,
        contrast: flat.light_contrast,
        gallery: flat.light_gallery,
      },
      DEFAULT_LIGHT_SEED,
    ),
    dark: parseThemeSeed(
      {
        background: flat.dark_background,
        foreground: flat.dark_foreground,
        accent: flat.dark_accent,
        contrast: flat.dark_contrast,
        gallery: flat.dark_gallery,
      },
      DEFAULT_DARK_SEED,
    ),
    material: parseMaterial(typeof flat.material === 'string' ? flat.material : undefined),
    opacity: parseOpacity(flat.opacity as string | number | null | undefined),
  }
}

/**
 * 解析命名主题列表。非法条目丢弃,重复 id 只保留首个(按稳定 ID 定位,不按颜色或名称猜测)。
 */
export function parseSavedThemes(value: unknown): SavedTheme[] {
  if (!Array.isArray(value)) return []
  const seen = new Set<string>()
  const themes: SavedTheme[] = []
  for (const entry of value) {
    const theme = fromFlatSavedTheme(entry)
    if (!theme || seen.has(theme.id)) continue
    seen.add(theme.id)
    themes.push(theme)
  }
  return themes
}

/** 从设置文本解析命名主题列表(空值 / 非法 JSON / 非数组均得到空列表)。 */
export function parseSavedThemesText(raw: string | null | undefined): SavedTheme[] {
  return parseSavedThemes(parseJsonText(raw))
}

/** 序列化为规范 JSON 文本(丢弃空 id / 空名称条目,保持传入顺序)。 */
export function serializeSavedThemes(themes: readonly SavedTheme[]): string {
  const flat = themes.map(toFlatSavedTheme).filter((entry) => entry.id && entry.name)
  return JSON.stringify(flat)
}
