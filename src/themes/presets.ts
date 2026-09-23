// src/themes/presets.ts
// 内置预设(方案 §4.1/§6):默认种子来自 presets/*.json,预设只是完整参数的快捷填入。
//
// 权威默认种子只写一份 —— presets/default-light.json / default-dark.json;本文件 import 它们
// 供前端使用,Rust schema 以 include_str! 引用同一对文件作结构默认值,禁止另写一份 HEX 默认。
// 其他预设同样只保存种子,预览与色板一律现算(不存生成结果)。
//
// 纯数据与纯函数:不依赖 Vue / Tauri / DOM,Node 侧 Vite 配置可直接 import。

import coldNightDark from './presets/cold-night-dark.json'
import coldNightLight from './presets/cold-night-light.json'
import defaultDark from './presets/default-dark.json'
import defaultLight from './presets/default-light.json'
import forestDark from './presets/forest-dark.json'
import forestLight from './presets/forest-light.json'
import mintDark from './presets/mint-dark.json'
import mintLight from './presets/mint-light.json'
import warmPaperDark from './presets/warm-paper-dark.json'
import warmPaperLight from './presets/warm-paper-light.json'
import {
  DEFAULT_WINDOW_OPACITY,
  type ThemeDefinition,
  type ThemePreset,
  type ThemeSeed,
} from './types'

/** 出厂浅色种子(中性底色 + 绿色强调)。 */
export const DEFAULT_LIGHT_SEED: ThemeSeed = { ...defaultLight }

/** 出厂深色种子。 */
export const DEFAULT_DARK_SEED: ThemeSeed = { ...defaultDark }

/** 出厂完整主题:两套默认种子 + 纯色窗口(新默认 none)。 */
export const DEFAULT_THEME_DEFINITION: ThemeDefinition = {
  light: DEFAULT_LIGHT_SEED,
  dark: DEFAULT_DARK_SEED,
  material: 'none',
  opacity: DEFAULT_WINDOW_OPACITY,
  visualStyle: 'standard',
}

export const PRESET_NEUTRAL = 'neutral'
export const PRESET_WARM_PAPER = 'warm-paper'
export const PRESET_COLD_NIGHT = 'cold-night'
export const PRESET_MINT = 'mint'
export const PRESET_FOREST = 'forest'

/**
 * 内置三套预设:中性、暖纸、冷夜。顺序即设置页展示顺序,中性即默认(与
 * DEFAULT_THEME_DEFINITION 同一份参数);材质与不透明度是预设参数的一部分。
 */
export const BUILTIN_PRESETS: readonly ThemePreset[] = [
  { id: PRESET_NEUTRAL, nameKey: 'themes.neutral', definition: DEFAULT_THEME_DEFINITION },
  {
    id: PRESET_WARM_PAPER,
    nameKey: 'themes.warmPaper',
    definition: {
      light: { ...warmPaperLight },
      dark: { ...warmPaperDark },
      material: 'mica',
      opacity: 92,
      visualStyle: 'standard',
    },
  },
  {
    id: PRESET_COLD_NIGHT,
    nameKey: 'themes.coldNight',
    definition: {
      light: { ...coldNightLight },
      dark: { ...coldNightDark },
      material: 'acrylic',
      opacity: 88,
      visualStyle: 'standard',
    },
  },
  {
    id: PRESET_MINT,
    nameKey: 'themes.mint',
    definition: {
      light: { ...mintLight },
      dark: { ...mintDark },
      material: 'none',
      opacity: DEFAULT_WINDOW_OPACITY,
      visualStyle: 'mint',
    },
  },
  {
    id: PRESET_FOREST,
    nameKey: 'themes.forest',
    definition: {
      light: { ...forestLight },
      dark: { ...forestDark },
      material: 'none',
      opacity: DEFAULT_WINDOW_OPACITY,
      visualStyle: 'forest',
    },
  },
]

function seedEquals(a: ThemeSeed, b: ThemeSeed): boolean {
  return (
    a.background === b.background &&
    a.foreground === b.foreground &&
    a.accent === b.accent &&
    a.contrast === b.contrast &&
    a.gallery === b.gallery
  )
}

/** 两份完整参数是否逐字段相同(不比较引用)。 */
export function definitionEquals(a: ThemeDefinition, b: ThemeDefinition): boolean {
  return (
    seedEquals(a.light, b.light) &&
    seedEquals(a.dark, b.dark) &&
    a.material === b.material &&
    a.opacity === b.opacity &&
    a.visualStyle === b.visualStyle
  )
}

/**
 * 当前参数与内置预设的匹配结果:恰好命中一套时返回其 id,无匹配或多套同配色匹配返回 null。
 *
 * 不保存「选中主题 id」:界面据此显示预设名或「自定义」(方案 §3)。
 */
export function matchPresetId(
  definition: ThemeDefinition,
  presets: readonly ThemePreset[] = BUILTIN_PRESETS,
): string | null {
  const hits = presets.filter((preset) => definitionEquals(preset.definition, definition))
  return hits.length === 1 ? hits[0].id : null
}
