// src/themes/types.ts
// 主题域运行期类型(方案 docs/designs/2026-09-16-主题配色全新设计.md §3/§4)。
//
// 数据链只有一条:SettingsSeed(用户输入) → generateTheme(纯函数) → ThemePalette(用途明确的
// 有限颜色集合) → CSS 变量 / Canvas。预设与“我的主题”都是完整 ThemeDefinition 的快照,
// 不参与运行时继承;旧的 registry/strength 注册与浓度模型不在此表达。

/** 明暗槽位:只表示这套配色属于哪一档,与 appearance 的 system 无关。 */
export type ThemeMode = 'light' | 'dark'

/** 窗口材质。none 为纯色(新默认);mica / acrylic 仅 Windows 生效。 */
export type ThemeMaterial = 'none' | 'mica' | 'acrylic'

/** 画廊底色跟随界面底面的取值。 */
export const GALLERY_AUTO = 'auto'

export const THEME_MODES: readonly ThemeMode[] = ['light', 'dark']

export const THEME_MATERIALS: readonly ThemeMaterial[] = ['none', 'mica', 'acrylic']

/** 窗口不透明度默认值(0–100 整数),仅材质开启时使用。 */
export const DEFAULT_WINDOW_OPACITY = 90

export const WINDOW_OPACITY_MIN = 0
export const WINDOW_OPACITY_MAX = 100

/**
 * 一份配色的全部用户输入。
 *
 * background / foreground / accent 是规范 #rrggbb;gallery 是 auto 或规范 #rrggbb。
 * 字符串在设置边界统一校验(见 config.ts),内部使用不再重复检查。
 */
export interface ThemeSeed {
  background: string
  foreground: string
  accent: string
  /** 层次对比度,0–100 整数。 */
  contrast: number
  /** 画廊底色:auto 或规范 #rrggbb。 */
  gallery: string
}

/** 完整主题参数:两套配色 + 材质。预设、命名主题与当前应用配色都是本结构。 */
export interface ThemeDefinition {
  light: ThemeSeed
  dark: ThemeSeed
  material: ThemeMaterial
  /** 窗口不透明度,0–100 整数。 */
  opacity: number
}

/** 用户命名的主题快照:保存后继续调色不自动更新它。 */
export interface SavedTheme extends ThemeDefinition {
  id: string
  name: string
}

/** 内置预设:完整参数的快捷填入,选中即成为一份独立草稿。 */
export interface ThemePreset {
  /** 稳定 id,直接用于定位与持久化。 */
  id: string
  /** 名称 i18n key(内置预设分组展示)。 */
  nameKey: string
  definition: ThemeDefinition
}

/**
 * 生成结果:覆盖现有主题 CSS 有意义 --color-* 接口的有限颜色集合。
 *
 * 字段名按用途命名(而非按 token 命名),与 CSS 变量的对应关系见 generate.ts 的
 * PALETTE_CSS_VARS:一个字段可服务多个同义 token(bg-primary/bg-secondary 共享 background)。
 * 最终颜色为 hex / rgb / rgba,不向 Canvas 传 var() 或 color-mix()。
 */
export interface ThemePalette {
  mode: ThemeMode

  /* ── 底面与层次 ───────────────────────────────────────────────────────── */
  /** 基础背景:窗口共享底面,侧栏/顶栏/底栏同色(--color-bg-primary / -secondary)。 */
  background: string
  /** 普通面板/卡片。 */
  surface: string
  /** 浮层:菜单、弹窗、粘性头部。 */
  elevated: string
  /** 凹陷区:色槽、内嵌块。 */
  inset: string
  /** 悬停背景。 */
  hover: string
  /** 选中浅背景(强调色浅底),亦作 --color-bg-active。 */
  selection: string
  /** 遮罩:确实需要透明的用途保留 alpha。 */
  overlay: string

  /* ── 文字 ─────────────────────────────────────────────────────────────── */
  /** 正文。 */
  textPrimary: string
  /** 辅助文字。 */
  textSecondary: string
  /** 次要元信息。 */
  textTertiary: string
  /** 输入占位。 */
  textPlaceholder: string
  /** 反色文字(位于深色底之上)。 */
  textInverse: string

  /* ── 强调色 ───────────────────────────────────────────────────────────── */
  accent: string
  accentHover: string
  /** 强调色文字:必要时已向黑/白调整以保证在实际底色上可读。 */
  accentText: string
  /** 强调色填充上的文字:按计算对比选黑或白。 */
  textOnAccent: string

  /* ── 边框与控件 ───────────────────────────────────────────────────────── */
  /** 普通边框(结构性分隔)。 */
  border: string
  borderStrong: string
  borderSubtle: string
  /** 装饰性分隔线:不承担控件边界职责,不套用 3:1 控件门槛。 */
  divider: string
  /** 有含义的控件边界(输入框、下拉、开关描边):满足 3:1。 */
  controlBorder: string
  /** 开关轨道等有含义控件底面:满足 3:1。 */
  controlTrack: string
  /** 焦点环(alpha)。 */
  focusRing: string
  /** 控件填充(输入框、下拉底色)。 */
  inputBg: string
  /** 开关滑钮。 */
  toggleThumb: string

  /* ── 画廊与纸面 ───────────────────────────────────────────────────────── */
  /** 画廊底色:auto 时等于 background。 */
  canvas: string
  /** 格缝与清屏色:与画廊底色一致。 */
  canvasGap: string
  /** 无缩略图格面:从实际画廊底色派生。 */
  canvasPlaceholder: string
  /** 文本文档卡纸面:从实际画廊底色派生。 */
  docPaper: string
  /** 纸面仿文本行。 */
  docPaperLine: string
  /** 缩略图 1px 内描边(alpha)。 */
  thumbOutline: string
  /** 画廊底上的正文(时间轴刻度、画布内文本):从**实际**画廊底色派生,不改动用户 foreground。 */
  canvasText: string
  /** 画廊底上的辅助文字。 */
  canvasTextSecondary: string

  /* ── 滚动条 ───────────────────────────────────────────────────────────── */
  scrollbarThumb: string
  scrollbarThumbHover: string
  scrollbarTrack: string

  /* ── 状态语义色(不随强调色变化) ─────────────────────────────────────── */
  success: string
  successSubtle: string
  textOnSuccess: string
  warning: string
  warningSubtle: string
  textOnWarning: string
  error: string
  errorSubtle: string
  textOnError: string
  info: string
  infoSubtle: string
  textOnInfo: string

  /* ── 媒体类别徽标(品牌语义色) ───────────────────────────────────────── */
  badgeDocWord: string
  badgeDocExcel: string
  badgeDocPpt: string
  badgeDocMd: string
  badgeDocGeneric: string

  /* ── 图片上方徽标与评分(专用遮罩,不随主题漂移) ─────────────────────── */
  badgeScrim: string
  badgeMarkLive: string
  badgeMarkAudio: string
  badgeMarkDocument: string
  ratingAmber: string
}
