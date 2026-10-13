// 阅读主题只控制正文与页面背景，可独立于应用外壳主题选择。
// 明暗两槽分别保存选择，随应用明暗切换；FOLLOW 表示跟随应用配色。

/** 「跟随应用主题」哨兵值：不属任何调色板条目，resolveReaderColors 遇之回落 app 颜色。 */
export const READER_THEME_FOLLOW = 'follow'

/** 一个阅读主题：一对正文/背景色 + 明暗归属；texture 为可选羊皮纸纹理开关。 */
export interface ReaderTheme {
  /** 持久化 id（doc_reader_theme_light/dark 与每书 prefs.theme 的值）。 */
  id: string
  /** i18n key（readerThemes.<id>）。 */
  nameKey: string
  /** 明/暗归属：决定该主题落在 light 槽还是 dark 槽（据 app 当前明暗择槽）。 */
  kind: 'light' | 'dark'
  /** 正文色（注入 iframe section 文档，见 readerStyles.buildReaderCSS）。 */
  text: string
  /** 页面背景色。 */
  background: string
  /** 是否叠加羊皮纸纹理（极淡 CSS 渐变，仅羊皮纸开）。缺省 false。 */
  texture?: boolean
}

// 精选阅读调色板。正文与背景的设计对比度要求为 ≥4.5。
// light：纸白（中性偏暖白）/ 羊皮纸（暖黄 + 纹理）/ 护眼（低饱和青绿）。
// dark：夜间（中性深灰）/ 石墨（近黑冷调）。
export const READER_THEMES: readonly ReaderTheme[] = [
  { id: 'paper', nameKey: 'readerThemes.paper', kind: 'light', text: '#2b2b2b', background: '#faf9f7' },
  {
    id: 'sepia',
    nameKey: 'readerThemes.sepia',
    kind: 'light',
    text: '#5b4636',
    background: '#f4ecd8',
    texture: true,
  },
  { id: 'green', nameKey: 'readerThemes.green', kind: 'light', text: '#2f3a26', background: '#dfe9d3' },
  { id: 'night', nameKey: 'readerThemes.night', kind: 'dark', text: '#cfcfcf', background: '#1a1a1a' },
  { id: 'graphite', nameKey: 'readerThemes.graphite', kind: 'dark', text: '#b9bdc6', background: '#0e1014' },
]

export function getReaderTheme(id: string): ReaderTheme | undefined {
  return READER_THEMES.find((t) => t.id === id)
}

/** 某明暗槽位可选的阅读主题（不含 FOLLOW，FOLLOW 由 UI 单独置顶）。 */
export function readerThemesByKind(kind: 'light' | 'dark'): ReaderTheme[] {
  return READER_THEMES.filter((t) => t.kind === kind)
}

/**
 * 归一化某槽位的阅读主题 id：FOLLOW 原样保留；有效且 kind 相符的 id 保留；其余（空 / 未注册 /
 * kind 不符，如卸载的外置主题或跨槽误存）一律回落 FOLLOW（= 跟随应用，永不产生无色变量）。
 */
export function normalizeReaderThemeId(raw: string | null | undefined, kind: 'light' | 'dark'): string {
  if (!raw || raw === READER_THEME_FOLLOW) return READER_THEME_FOLLOW
  return getReaderTheme(raw)?.kind === kind ? raw : READER_THEME_FOLLOW
}
