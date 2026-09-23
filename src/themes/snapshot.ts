// src/themes/snapshot.ts
// 首帧缓存(方案 docs/designs/2026-09-16-主题配色全新设计.md §7):只取**后端已确认**配置
// 生成的完整 CSS 变量与外观偏好,供阻塞式 public/theme-bootstrap.js 在样式解析前恢复,消除首帧闪色。
//
// 三条硬约束:
//  - 只由已确认快照写入(settingsConfirmedValues / onSettingsApplied);草稿与未确认写入不得进缓存。
//  - 新键新结构:不读旧的 scrollery.themeSnapshot.v1,结构只接受当前版本,失效即回默认主题 CSS。
//  - 缓存是可丢弃的派生值:任何情况下都不得反向恢复用户偏好,损坏只损失首帧加速。

import type { AppearanceMode } from '../types/ui'
import { THEME_PALETTE_VARS, paletteToCssVars } from './generate'
import type { ThemeMaterial, ThemePalette, ThemeVisualStyle } from './types'
import { THEME_VISUAL_STYLES } from './visualStyles'

/** 缓存键(与旧 scrollery.themeSnapshot.v1 无关;bootstrap 内是同一字面量,由契约测试钉住)。 */
export const THEME_CACHE_KEY = 'scrollery.themeCache.v3'
/** 缓存结构版本;读写双方都只接受这一版,不向前兼容。 */
export const THEME_CACHE_VERSION = 3

export interface ThemeCache {
  v: number
  /** 外观偏好(system/light/dark):首帧据此解析实际明暗,缓存不存「解析结果」。 */
  appearance: AppearanceMode
  /** 浅/深两套最终 CSS 变量(完整变量名 → 颜色);两套键集恒相同。 */
  light: Record<string, string>
  dark: Record<string, string>
  material: ThemeMaterial
  visualStyle: ThemeVisualStyle
  /** 窗口不透明度,0–100 整数。 */
  opacity: number
}

const APPEARANCE_MODES: readonly AppearanceMode[] = ['system', 'light', 'dark']
const MATERIALS: readonly ThemeMaterial[] = ['none', 'mica', 'acrylic']

/** 具体颜色:hex / rgb / rgba / transparent。var()、color-mix()、空串一律不算。 */
const CONCRETE_COLOR = /^(#[0-9a-f]{6}|rgba?[(][0-9., ]+[)]|transparent)$/

/**
 * 变量表必须与 generate.ts 的期待键集**一一对应**:键数与键名都一致(缺项、多项、空表都无效),
 * 且每个值都是具体颜色。首帧只做整份恢复,不做部分覆盖——残缺表会把默认 CSS 的浅色值留在原地
 * 而只覆盖一半,拼出两套配色的混合观感。
 */
function isCompleteVars(raw: unknown): raw is Record<string, string> {
  if (typeof raw !== 'object' || raw === null) return false
  const vars = raw as Record<string, unknown>
  const names = Object.keys(vars)
  if (names.length !== THEME_PALETTE_VARS.length) return false
  for (const name of names) {
    const value = vars[name]
    if (typeof value !== 'string' || !CONCRETE_COLOR.test(value)) return false
    if (!THEME_PALETTE_VARS.includes(name)) return false
  }
  return true
}

/** 构造缓存:两套色板与材质一次收齐,纯函数、不落盘。 */
export function buildThemeCache(
  appearance: AppearanceMode,
  light: ThemePalette,
  dark: ThemePalette,
  material: ThemeMaterial,
  opacity: number,
  visualStyle: ThemeVisualStyle,
): ThemeCache {
  return {
    v: THEME_CACHE_VERSION,
    appearance,
    light: paletteToCssVars(light),
    dark: paletteToCssVars(dark),
    material,
    visualStyle,
    opacity,
  }
}

/** 收敛缓存文本为缓存结构;版本不符、字段类型不符或损坏一律 null(不回退、不修补)。 */
export function parseThemeCache(raw: string | null): ThemeCache | null {
  if (!raw) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return null
  }
  if (typeof parsed !== 'object' || parsed === null) return null
  const cache = parsed as Record<string, unknown>
  if (cache.v !== THEME_CACHE_VERSION) return null
  if (!APPEARANCE_MODES.includes(cache.appearance as AppearanceMode)) return null
  if (!MATERIALS.includes(cache.material as ThemeMaterial)) return null
  if (!THEME_VISUAL_STYLES.includes(cache.visualStyle as ThemeVisualStyle)) return null
  if (!isCompleteVars(cache.light) || !isCompleteVars(cache.dark)) return null
  // 越界/非整数一律丢弃整份缓存,不做夹取:夹取会把「不是当前格式」的输入修饰成看似合法的缓存。
  if (
    typeof cache.opacity !== 'number' ||
    !Number.isInteger(cache.opacity) ||
    cache.opacity < 0 ||
    cache.opacity > 100
  ) {
    return null
  }
  return {
    v: THEME_CACHE_VERSION,
    appearance: cache.appearance as AppearanceMode,
    light: cache.light,
    dark: cache.dark,
    material: cache.material as ThemeMaterial,
    visualStyle: cache.visualStyle as ThemeVisualStyle,
    opacity: cache.opacity,
  }
}

/** 读缓存;localStorage 不可用或内容损坏返回 null(调用方回默认主题 CSS)。 */
export function readThemeCache(): ThemeCache | null {
  try {
    return parseThemeCache(localStorage.getItem(THEME_CACHE_KEY))
  } catch {
    return null
  }
}

/** 写缓存;localStorage 不可用只损失首帧加速,静默降级(不抛、不提示用户)。 */
export function writeThemeCache(cache: ThemeCache): void {
  try {
    localStorage.setItem(THEME_CACHE_KEY, JSON.stringify(cache))
  } catch {
    /* 配额/隐私模式:缓存是可丢弃派生值,降级即可 */
  }
}
