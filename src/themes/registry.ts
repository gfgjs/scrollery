// 内置主题注册表。主题按风格成对注册，明暗槽位共享同一套风格命名。

/** 主题所属的明暗槽位。 */
export type ThemeKind = 'light' | 'dark'

/** 应用提供的三种主题风格。 */
export type ThemeStyle = 'fresh' | 'minimal' | 'tech'

/** 设置页主题卡片使用的四色预览。 */
export interface ThemePreview {
  bg: string
  surface: string
  text: string
  accent: string
}

export interface ThemeDefinition {
  /** data-theme 值，亦是 theme_light/theme_dark 的持久化值。 */
  id: string
  /** 主题所属风格，明暗主题各有一个对应条目。 */
  style: ThemeStyle
  /** i18n key(themes.<style>)。 */
  nameKey: string
  /** 亮/暗槽位归属，决定 data-color-scheme 与原生标题栏明暗。 */
  kind: ThemeKind
  preview: ThemePreview
  /** builtin = 随包静态打包。 */
  source: 'builtin'
}

/** 设置页的一个风格选项及其明暗主题配对。 */
export interface ThemeStyleDefinition {
  /** 稳定的风格值，也是 setThemeStyle 的入参。 */
  id: ThemeStyle
  nameKey: string
  descriptionKey: string
  themes: Readonly<{ light: ThemeDefinition; dark: ThemeDefinition }>
}

const THEME_DEFINITIONS: readonly ThemeDefinition[] = [
  {
    id: 'fresh-light',
    style: 'fresh',
    nameKey: 'themes.fresh',
    kind: 'light',
    preview: { bg: '#edf5f1', surface: '#fcfffd', text: '#132a22', accent: '#0b7c52' },
    source: 'builtin',
  },
  {
    id: 'fresh-dark',
    style: 'fresh',
    nameKey: 'themes.fresh',
    kind: 'dark',
    preview: { bg: '#0d1512', surface: '#18241f', text: '#edf7f2', accent: '#34d399' },
    source: 'builtin',
  },
  {
    id: 'minimal-light',
    style: 'minimal',
    nameKey: 'themes.minimal',
    kind: 'light',
    preview: { bg: '#f1f3f5', surface: '#ffffff', text: '#18181b', accent: '#27272a' },
    source: 'builtin',
  },
  {
    id: 'minimal-dark',
    style: 'minimal',
    nameKey: 'themes.minimal',
    kind: 'dark',
    preview: { bg: '#121214', surface: '#202023', text: '#f4f4f5', accent: '#e4e4e7' },
    source: 'builtin',
  },
  {
    id: 'tech-light',
    style: 'tech',
    nameKey: 'themes.tech',
    kind: 'light',
    preview: { bg: '#ebf1fa', surface: '#fbfdff', text: '#111f38', accent: '#2563eb' },
    source: 'builtin',
  },
  {
    id: 'tech-dark',
    style: 'tech',
    nameKey: 'themes.tech',
    kind: 'dark',
    preview: { bg: '#0e131b', surface: '#1a2330', text: '#eff4fc', accent: '#60a5fa' },
    source: 'builtin',
  },
]

/** 亮/暗槽位的出厂默认主题 id。 */
export const DEFAULT_LIGHT_THEME = 'fresh-light'
export const DEFAULT_DARK_THEME = 'fresh-dark'
export const DEFAULT_THEME_STYLE: ThemeStyle = 'fresh'

/**
 * 内置主题列表。数组保持明暗成对顺序，消费者仍可通过 themesByKind 按槽位筛选。
 */
export const BUILTIN_THEMES: readonly ThemeDefinition[] = THEME_DEFINITIONS

function styleDefinition(
  style: ThemeStyle,
  nameKey: string,
  descriptionKey: string,
): ThemeStyleDefinition {
  const light = THEME_DEFINITIONS.find((theme) => theme.style === style && theme.kind === 'light')
  const dark = THEME_DEFINITIONS.find((theme) => theme.style === style && theme.kind === 'dark')
  // 风格表是本文件内的静态数据，缺少配对条目属于开发期错误；避免把不完整数据暴露给 UI。
  if (!light || !dark) throw new Error(`主题风格缺少明暗配对: ${style}`)
  return {
    id: style,
    nameKey,
    descriptionKey,
    themes: { light, dark },
  }
}

/** 设置页按风格渲染的三项数据。 */
export const THEME_STYLES: readonly ThemeStyleDefinition[] = [
  styleDefinition('fresh', 'themes.fresh', 'themes.freshDescription'),
  styleDefinition('minimal', 'themes.minimal', 'themes.minimalDescription'),
  styleDefinition('tech', 'themes.tech', 'themes.techDescription'),
]

/** 根据主题 id 查找主题定义。 */
export function getTheme(id: string): ThemeDefinition | undefined {
  return BUILTIN_THEMES.find((theme) => theme.id === id)
}

/** 根据风格值查找成对的风格定义。 */
export function getThemeStyle(style: string): ThemeStyleDefinition | undefined {
  return THEME_STYLES.find((definition) => definition.id === style)
}

/** 返回指定风格和明暗槽位对应的主题 id，陌生风格回退到 fresh。 */
export function themeIdForStyle(style: string, kind: ThemeKind): string {
  const definition = getThemeStyle(style) ?? THEME_STYLES[0]
  return kind === 'light' ? definition.themes.light.id : definition.themes.dark.id
}

/** 保留旧消费者的按明暗槽位筛选 API。 */
export function themesByKind(kind: ThemeKind): ThemeDefinition[] {
  return BUILTIN_THEMES.filter((theme) => theme.kind === kind)
}

const LEGACY_THEME_IDS: Readonly<Record<string, string>> = {
  moonlight: 'fresh-light',
  porcelain: 'minimal-light',
  xuan: 'tech-light',
  ink: 'fresh-dark',
  obsidian: 'minimal-dark',
  dai: 'tech-dark',
  light: DEFAULT_LIGHT_THEME,
  dark: DEFAULT_DARK_THEME,
}

/**
 * 归一化槽位主题 id。旧主题 id 和早期的 light/dark 值映射到新风格；
 * 未注册或明暗槽位不匹配的值回退到该槽位的 fresh 默认主题。
 */
export function normalizeThemeId(raw: string | null | undefined, kind: ThemeKind): string {
  const fallback = kind === 'light' ? DEFAULT_LIGHT_THEME : DEFAULT_DARK_THEME
  if (!raw) return fallback
  const mapped = LEGACY_THEME_IDS[raw] ?? raw
  return getTheme(mapped)?.kind === kind ? mapped : fallback
}
