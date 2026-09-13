// 阅读专属主题调色板（阅读器方案 R3「主题日夜配对 + 羊皮纸纹理」）。
//
// 与 app chrome 主题（registry.ts 的 BUILTIN_THEMES：墨/素/宣…）**分属两个命名空间**：
// 后者是整个应用外壳的语义色层（30+ 变量），前者只是阅读区正文/背景的一对色 —— 因为好的**阅读**
// 底色（纸白 / 羊皮纸 / 护眼 / 夜间）与好的**界面**底色标准不同。二者解耦：用户可在深色界面里读纸白，
// 或在浅色界面里读夜间（不常见但不禁止）。
//
// 「日夜配对」= 持久化 light-槽 与 dark-槽两个选择（各默认 FOLLOW），随 app 明暗自动切换应用哪一槽，
// 与 app 自身 theme_light/theme_dark 的模型同构。FOLLOW = 跟随应用主题色（保持 R2 起的既有行为，
// 也是零回归默认）。
//
// 纯数据 + 查表，无 DOM / 无副作用，node 环境可单测；对比度门禁见 reader-theme-contract.spec.ts。

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

// 精选阅读调色板。颜色须过 WCAG AA 正文对比度门禁（≥4.5，见 reader-theme-contract.spec.ts）。
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
