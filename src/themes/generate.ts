// src/themes/generate.ts
// 唯一的颜色生成入口(方案 §4.2/§6):种子 + 模式 → 用途明确的有限颜色集合。
//
// 契约:generateTheme 不访问 DOM、存储或框架;相同种子与模式必定产生相同色板;层次对比度
// 只改变派生层,不改动三个用户种子,也不作用为整页滤镜。输出全部是 hex / rgb / rgba 实色,
// 不向 Canvas 传 var() 或 color-mix()——组件、Canvas、预览与启动样式消费同一结果。
//
// 系数表是首版起始值,集中在 THEME_COEFFICIENTS 内调优(输入语义不变)。有对比门槛的用途
// (正文/辅助文字、有含义的控件边界、强调色文字)不写死系数,而是按门槛反推混色量:固定系数
// 在任意用户配色上都无法保证达标。

import type { ThemeMode, ThemePalette, ThemeSeed, ThemeVisualStyle } from './types'
import { GALLERY_AUTO } from './types'
import { shellBackgroundFor } from './visualStyles'
import {
  ensureContrast,
  isLightColor,
  mix,
  mixUntilContrast,
  pickReadableText,
  withAlpha,
} from './colors'

export const CONTRAST_MIN = 0
export const CONTRAST_MAX = 100

/** 正文与辅助文字的最低对比(WCAG AA)。 */
export const MIN_TEXT_CONTRAST = 4.5
/** 有含义控件边界的最低对比(装饰性分隔线不套用)。 */
export const MIN_CONTROL_CONTRAST = 3

const WHITE = '#ffffff'
const BLACK = '#000000'

/**
 * 统一系数表(方案 §4.2 起始规则)。入参是归一化后的层次对比度 c = contrast / 100。
 * 每个用途只在此写一次,组件不得重复计算。
 */
export const THEME_COEFFICIENTS = {
  /** 辅助文字。 */
  textSecondary: (c: number) => 0.66 + 0.1 * c,
  /** 次要元信息。 */
  textTertiary: (c: number) => 0.48 + 0.12 * c,
  /** 普通边框(结构性分隔,无对比门槛)。 */
  border: (c: number) => 0.08 + 0.06 * c,
  /** 强边框(选中框、浮层描边)。 */
  borderStrong: (c: number) => 0.14 + 0.1 * c,
  /** 发丝级边框。 */
  borderSubtle: (c: number) => 0.05 + 0.03 * c,
  /** 装饰性分隔线:刻意比普通边框更轻,不承担控件边界职责。 */
  divider: (c: number) => 0.06 + 0.04 * c,
  /** 悬停背景。 */
  hover: (c: number) => 0.04 + 0.04 * c,
  /** 凹陷面(亮色)。 */
  inset: (c: number) => 0.04 + 0.04 * c,
  /** 深色普通面板:向 foreground 提亮。 */
  darkPanel: (c: number) => 0.025 + 0.025 * c,
  /** 深色浮层。 */
  darkFloat: (c: number) => 0.06 + 0.04 * c,
  /** 选中浅背景:background 向 accent 混合。 */
  selection: (c: number) => 0.1 + 0.06 * c,
  /** 控件填充(输入框/下拉底色)。 */
  inputBg: (c: number) => 0.03 + 0.02 * c,
  /** 无缩略图格面:从实际画廊底色向前景方向派生。 */
  canvasPlaceholder: (c: number) => 0.1 + 0.06 * c,
  /** 纸面仿文本行。 */
  docPaperLine: (c: number) => 0.14 + 0.08 * c,
} as const

/** 与层次对比度无关的固定混色量:入参/语义与上表同类,只是不随 c 变化。 */
export const THEME_CONSTANTS = {
  /** 浅色普通面板:向白混合。 */
  lightPanel: 0.35,
  /** 浅色浮层:向白混合更多。 */
  lightFloat: 0.7,
  /** 深色凹陷面:向黑混合。 */
  darkInset: 0.35,
  /** 强调色悬停:向填充文字的反方向微调(亮色压暗、深色提亮),两侧都提升文字对比。 */
  accentHover: 0.12,
  /** 控件边界起始混色量:门槛本就达标时不至于淡成看不见。 */
  controlFloor: 0.14,
  /** 开关轨道比描边多出的余量:整块填充需要更清楚的边界。 */
  controlTrackExtra: 0.5,
  /** 浅色纸面:向白混合。 */
  docPaper: 0.55,
  /** 深色纸面:向前景微提亮。 */
  docPaperDark: 0.035,
} as const

/** 状态语义色:各自保持语义,不随 accent 变成同一种颜色(方案 §4.2)。 */
const STATUS_COLORS: Record<ThemeMode, { success: string; warning: string; error: string; info: string }> = {
  light: { success: '#18702f', warning: '#a14705', error: '#b21f26', info: '#0959a4' },
  dark: { success: '#34c759', warning: '#ff9f0a', error: '#ff665d', info: '#64d2ff' },
}

/** 文档类别徽标:品牌语义色,两模式共用,不随主题漂移。 */
const BADGE_DOC_COLORS = {
  word: '#2b579a',
  excel: '#217346',
  ppt: '#c43e1c',
  md: '#4b8bf4',
  generic: '#6b7280',
} as const

/**
 * 图片上方徽标与评分的专用色:压在照片上,底色不可知,故用固定遮罩 + 高亮类别点,
 * 两模式共用、不随主题漂移(方案 §4.2「图片上方的时长／徽标继续使用专用遮罩色」)。
 *
 * 这里是与色板同批产出的单点来源:渲染侧不再保留第二份颜色常量(旧声明点见
 * components/media/mediaGridCanvas.palette.ts 的 CANONICAL_BADGE_COLORS,由 B 批迁移删除)。
 */
export const CANONICAL_MEDIA_COLORS = {
  scrim: 'rgba(0, 0, 0, 0.6)',
  markLive: '#ffaaaa',
  markAudio: '#5dd39e',
  markDocument: '#ffd166',
  ratingAmber: '#fbbf24',
} as const

/** 层次对比度收敛为 0–100 整数;非法值落 0(边界校验已在配置层做过,此处只防 NaN)。 */
export function clampContrast(value: number): number {
  if (!Number.isFinite(value)) return CONTRAST_MIN
  return Math.min(CONTRAST_MAX, Math.max(CONTRAST_MIN, Math.round(value)))
}

/**
 * 有含义控件边界的承载面:它实际出现在基础底面、普通面板与浮层之上,任一层不达标都算不达标,
 * 故生成时对这些面取最低对比(mixUntilContrast 内部按全部 bases 判定)。
 */
function controlBases(background: string, surface: string, elevated: string): string[] {
  return [background, surface, elevated]
}

/** 主内容与外壳按同一表面公式派生，边线按各自实际承载面校正。 */
function deriveSurfaceColors(
  background: string,
  foreground: string,
  accent: string,
  c: number,
  light: boolean,
  includeInputBg = false,
) {
  const surface = light
    ? mix(background, WHITE, THEME_CONSTANTS.lightPanel)
    : mix(background, foreground, THEME_COEFFICIENTS.darkPanel(c))
  const elevated = light
    ? mix(background, WHITE, THEME_CONSTANTS.lightFloat)
    : mix(background, foreground, THEME_COEFFICIENTS.darkFloat(c))
  const inputBg = mix(background, foreground, THEME_COEFFICIENTS.inputBg(c))
  const bases = controlBases(background, surface, elevated)
  if (includeInputBg) bases.push(inputBg)
  return {
    surface,
    elevated,
    inset: light
      ? mix(background, foreground, THEME_COEFFICIENTS.inset(c))
      : mix(background, BLACK, THEME_CONSTANTS.darkInset),
    hover: mix(background, foreground, THEME_COEFFICIENTS.hover(c)),
    selection: mix(background, accent, THEME_COEFFICIENTS.selection(c)),
    textSecondary: mix(background, foreground, THEME_COEFFICIENTS.textSecondary(c)),
    textTertiary: mix(background, foreground, THEME_COEFFICIENTS.textTertiary(c)),
    border: mix(background, foreground, THEME_COEFFICIENTS.border(c)),
    borderStrong: mix(background, foreground, THEME_COEFFICIENTS.borderStrong(c)),
    borderSubtle: mix(background, foreground, THEME_COEFFICIENTS.borderSubtle(c)),
    divider: mix(background, foreground, THEME_COEFFICIENTS.divider(c)),
    inputBg,
    controlBorder: mixUntilContrast(
      background, foreground, bases, MIN_CONTROL_CONTRAST, THEME_CONSTANTS.controlFloor,
    ),
    controlTrack: mixUntilContrast(
      background, foreground, bases,
      MIN_CONTROL_CONTRAST + THEME_CONSTANTS.controlTrackExtra,
      THEME_CONSTANTS.controlFloor,
    ),
  }
}

/**
 * 生成一份完整色板。
 *
 * seed 的三个颜色为规范 #rrggbb、gallery 为 auto 或规范 #rrggbb——校验发生在输入/设置
 * 边界(见 config.ts),此处不再逐字段复核;坏值由 colors.ts 的降级行为兜底。
 */
export function generateTheme(
  seed: ThemeSeed,
  mode: ThemeMode,
  visualStyle: ThemeVisualStyle = 'standard',
): ThemePalette {
  const c = clampContrast(seed.contrast) / 100
  const light = mode === 'light'
  const { background, foreground, accent } = seed

  const main = deriveSurfaceColors(background, foreground, accent, c, light)
  const { surface, elevated, inset, controlBorder, controlTrack, textSecondary, textTertiary } = main

  // 选中浅背景与强调色悬停:两者都与强调色相关,先算出来供强调色文字一并判定。
  const selection = main.selection
  const accentHover = mix(accent, light ? BLACK : WHITE, THEME_CONSTANTS.accentHover)
  // 强调色文字以 accent 为起点,必要时向黑/白调整;它读在基础底面、面板与强调色浅底(选中态)
  // 三种底上,任一处不可读都算不达标。
  const accentText = ensureContrast(accent, [background, surface, selection], MIN_TEXT_CONTRAST)

  const shellBackground = shellBackgroundFor(background, accent, mode, visualStyle)
  const shell = visualStyle === 'standard'
    ? main
    : deriveSurfaceColors(shellBackground, foreground, accent, c, light, true)
  const shellBases = [shellBackground, shell.surface, shell.elevated]
  const shellTextPrimary = visualStyle === 'standard'
    ? foreground
    : ensureContrast(foreground, shellBases, MIN_TEXT_CONTRAST)
  const shellTextSecondary = visualStyle === 'standard'
    ? textSecondary
    : ensureContrast(shell.textSecondary, shellBases, MIN_TEXT_CONTRAST)
  const shellTextTertiary = visualStyle === 'standard'
    ? textTertiary
    : ensureContrast(shell.textTertiary, shellBases, MIN_TEXT_CONTRAST)

  // 画廊/纸面从**实际**画廊底色派生:指定了 gallery 的区间不能被错误地按窗口底色计算。
  const canvas = seed.gallery === GALLERY_AUTO ? background : seed.gallery
  // 纸面按**画廊底色自己的极性**派生(而非界面模式):显式反极性画廊下,浅色界面配深色画廊
  // 不该冒出一张发亮的白纸,反之亦然。默认 auto 时画廊=窗口底色,与模式极性一致,结果不变。
  const canvasIsLight = isLightColor(canvas)
  const docPaper = canvasIsLight
    ? mix(canvas, WHITE, THEME_CONSTANTS.docPaper)
    : mix(canvas, foreground, THEME_CONSTANTS.docPaperDark)
  // 画廊底上的文字必须相对**实际**画廊底色可读:显式指定反极性底色时(浅色主题配深色画廊),
  // 用户前景在原底上可读、在画廊上不可读,故这里单独派生,而不是复用 textPrimary。
  const canvasText = ensureContrast(foreground, [canvas], MIN_TEXT_CONTRAST)
  const canvasTextSecondary = ensureContrast(textSecondary, [canvas], MIN_TEXT_CONTRAST)

  const status = STATUS_COLORS[mode]
  // 状态浅底的 alpha(单一入口):状态文本会压在自己的浅底上,alpha 越高、底色越靠近文本色,
  // 该组合的对比越低。深色档曾用 0.16,使 error × errorSubtle(面板底)只有 4.44:1 不达 AA;
  // 降到 0.08(与旧六主题同档)后三套深色预设实测 ≥5.0:1。
  const subtleAlpha = light ? 0.1 : 0.08

  return {
    mode,

    background,
    surface,
    elevated,
    inset,
    hover: main.hover,
    selection,
    // 遮罩叠在照片/内容之上,确需透明(方案 §4.2 保留 alpha 的用途之一)。
    overlay: light ? withAlpha(foreground, 0.45) : withAlpha(BLACK, 0.6),

    shellBackground,
    shellSurface: shell.surface,
    shellElevated: shell.elevated,
    shellInset: shell.inset,
    shellHover: shell.hover,
    shellSelection: shell.selection,
    shellTextPrimary,
    shellTextSecondary,
    shellTextTertiary,
    shellTextPlaceholder: shellTextTertiary,
    shellAccentText: visualStyle === 'standard'
      ? accentText
      : ensureContrast(accent, [shellBackground, shell.surface, shell.selection], MIN_TEXT_CONTRAST),
    shellBorder: shell.border,
    shellBorderStrong: shell.borderStrong,
    shellBorderSubtle: shell.borderSubtle,
    shellDivider: shell.divider,
    shellInputBg: shell.inputBg,
    shellControlBorder: shell.controlBorder,
    shellControlTrack: shell.controlTrack,
    shellToggleThumb: light ? WHITE : shellBackground,
    shellScrollbarThumb: withAlpha(visualStyle === 'standard' ? foreground : shellTextPrimary, 0.16),
    shellScrollbarThumbHover: withAlpha(visualStyle === 'standard' ? foreground : shellTextPrimary, 0.28),

    textPrimary: foreground,
    textSecondary,
    textTertiary,
    // 占位文字与元信息同档:现有主题即同值,且占位符不是正文,不另立更高门槛。
    textPlaceholder: textTertiary,
    // 反色文字取背景极性的一端:亮色主题为白,深色主题为窗口底色。
    textInverse: light ? WHITE : background,

    accent,
    accentHover,
    accentText,
    textOnAccent: pickReadableText(accent),

    border: main.border,
    borderStrong: main.borderStrong,
    borderSubtle: main.borderSubtle,
    divider: main.divider,
    controlBorder,
    controlTrack,
    focusRing: withAlpha(accent, 0.45),
    inputBg: main.inputBg,
    toggleThumb: light ? WHITE : background,

    canvas,
    canvasGap: canvas,
    canvasPlaceholder: mix(canvas, foreground, THEME_COEFFICIENTS.canvasPlaceholder(c)),
    docPaper,
    docPaperLine: mix(docPaper, foreground, THEME_COEFFICIENTS.docPaperLine(c)),
    // 缩略图描边叠在图片上,底色不可知,只能靠 alpha 勾边(DOM 与 Canvas 同源)。
    thumbOutline: withAlpha(foreground, 0.08),
    canvasText,
    canvasTextSecondary,

    scrollbarThumb: withAlpha(foreground, 0.16),
    scrollbarThumbHover: withAlpha(foreground, 0.28),
    scrollbarTrack: 'transparent',

    success: status.success,
    successSubtle: withAlpha(status.success, subtleAlpha),
    textOnSuccess: pickReadableText(status.success),
    warning: status.warning,
    warningSubtle: withAlpha(status.warning, subtleAlpha),
    textOnWarning: pickReadableText(status.warning),
    error: status.error,
    errorSubtle: withAlpha(status.error, subtleAlpha),
    textOnError: pickReadableText(status.error),
    info: status.info,
    infoSubtle: withAlpha(status.info, subtleAlpha),
    textOnInfo: pickReadableText(status.info),

    badgeDocWord: BADGE_DOC_COLORS.word,
    badgeDocExcel: BADGE_DOC_COLORS.excel,
    badgeDocPpt: BADGE_DOC_COLORS.ppt,
    badgeDocMd: BADGE_DOC_COLORS.md,
    badgeDocGeneric: BADGE_DOC_COLORS.generic,

    badgeScrim: CANONICAL_MEDIA_COLORS.scrim,
    badgeMarkLive: CANONICAL_MEDIA_COLORS.markLive,
    badgeMarkAudio: CANONICAL_MEDIA_COLORS.markAudio,
    badgeMarkDocument: CANONICAL_MEDIA_COLORS.markDocument,
    ratingAmber: CANONICAL_MEDIA_COLORS.ratingAmber,
  }
}

/**
 * 色板字段 → CSS 变量的唯一映射(apply 层据此写入,不再另立别名链)。
 *
 * 一个字段服务多个同义 token 是有意的:bg-primary 与 bg-secondary 共用基础背景(侧栏、顶栏、
 * 底栏保持融合布局),accent 同时承担强调填充、输入框聚焦边界与开关激活色。mode 不落 CSS。
 * 这里只做映射,不含算法——颜色永远来自 generateTheme。
 */
export const PALETTE_CSS_VARS: Readonly<Record<keyof ThemePalette, readonly string[]>> = {
  mode: [],
  background: ['--color-bg-primary', '--color-bg-secondary'],
  surface: ['--color-bg-surface'],
  elevated: ['--color-bg-elevated'],
  inset: ['--color-bg-inset'],
  hover: ['--color-bg-hover', '--color-sidebar-hover-bg'],
  selection: ['--color-bg-active', '--color-accent-subtle', '--color-sidebar-active-bg'],
  overlay: ['--color-bg-overlay'],
  shellBackground: ['--color-shell-bg-primary', '--color-shell-bg-secondary'],
  shellSurface: ['--color-shell-bg-surface'],
  shellElevated: ['--color-shell-bg-elevated'],
  shellInset: ['--color-shell-bg-inset'],
  shellHover: ['--color-shell-bg-hover', '--color-shell-sidebar-hover-bg'],
  shellSelection: ['--color-shell-bg-active', '--color-shell-accent-subtle', '--color-shell-sidebar-active-bg'],
  shellTextPrimary: ['--color-shell-text-primary'],
  shellTextSecondary: ['--color-shell-text-secondary'],
  shellTextTertiary: ['--color-shell-text-tertiary'],
  shellTextPlaceholder: ['--color-shell-text-placeholder'],
  shellAccentText: ['--color-shell-accent-text', '--color-shell-sidebar-active-text'],
  shellBorder: ['--color-shell-border'],
  shellBorderStrong: ['--color-shell-border-strong'],
  shellBorderSubtle: ['--color-shell-border-subtle'],
  shellDivider: ['--color-shell-divider'],
  shellInputBg: ['--color-shell-input-bg'],
  shellControlBorder: ['--color-shell-input-border'],
  shellControlTrack: ['--color-shell-toggle-bg'],
  shellToggleThumb: ['--color-shell-toggle-thumb'],
  shellScrollbarThumb: ['--color-shell-scrollbar-thumb'],
  shellScrollbarThumbHover: ['--color-shell-scrollbar-thumb-hover'],
  textPrimary: ['--color-text-primary'],
  textSecondary: ['--color-text-secondary'],
  textTertiary: ['--color-text-tertiary'],
  textPlaceholder: ['--color-text-placeholder'],
  textInverse: ['--color-text-inverse'],
  accent: ['--color-accent', '--color-input-border-focus', '--color-toggle-bg-active'],
  accentHover: ['--color-accent-hover'],
  accentText: ['--color-accent-text', '--color-sidebar-active-text'],
  textOnAccent: ['--color-text-on-accent'],
  border: ['--color-border'],
  borderStrong: ['--color-border-strong'],
  borderSubtle: ['--color-border-subtle'],
  divider: ['--color-divider'],
  controlBorder: ['--color-input-border'],
  controlTrack: ['--color-toggle-bg'],
  focusRing: ['--color-focus-ring'],
  inputBg: ['--color-input-bg'],
  toggleThumb: ['--color-toggle-thumb'],
  canvas: ['--color-bg-canvas'],
  canvasGap: ['--color-bg-canvas-gap'],
  canvasPlaceholder: ['--color-bg-canvas-placeholder'],
  docPaper: ['--color-doc-paper'],
  docPaperLine: ['--color-doc-paper-line'],
  thumbOutline: ['--color-thumb-outline'],
  // 画廊底文字:Canvas 直接投影语义字段,DOM 侧由画廊分隔行/时间轴文字消费这两个变量。
  // 显式 gallery 可与窗口底色反极性,故两者都取「实际画廊底色」派生值。
  canvasText: ['--color-canvas-text'],
  canvasTextSecondary: ['--color-canvas-text-secondary'],
  scrollbarThumb: ['--color-scrollbar-thumb'],
  scrollbarThumbHover: ['--color-scrollbar-thumb-hover'],
  scrollbarTrack: ['--color-scrollbar-track'],
  success: ['--color-success'],
  successSubtle: ['--color-success-subtle'],
  textOnSuccess: ['--color-text-on-success'],
  warning: ['--color-warning'],
  warningSubtle: ['--color-warning-subtle'],
  textOnWarning: ['--color-text-on-warning'],
  error: ['--color-error'],
  errorSubtle: ['--color-error-subtle'],
  textOnError: ['--color-text-on-error'],
  info: ['--color-info'],
  infoSubtle: ['--color-info-subtle'],
  textOnInfo: ['--color-text-on-info'],
  badgeDocWord: ['--color-badge-doc-word'],
  badgeDocExcel: ['--color-badge-doc-excel'],
  badgeDocPpt: ['--color-badge-doc-ppt'],
  badgeDocMd: ['--color-badge-doc-md'],
  badgeDocGeneric: ['--color-badge-doc-generic'],
  badgeScrim: ['--color-badge-scrim'],
  badgeMarkLive: ['--color-badge-mark-live'],
  badgeMarkAudio: ['--color-badge-mark-audio'],
  badgeMarkDocument: ['--color-badge-mark-document'],
  ratingAmber: ['--color-rating-amber'],
}

/** 色板覆盖的全部 CSS 变量名(去重后的扁平清单;首帧缓存与启动样式按此表取键)。 */
export const THEME_PALETTE_VARS: readonly string[] = [
  ...new Set(Object.values(PALETTE_CSS_VARS).flat()),
]

/**
 * 色板 → CSS 变量表。apply 层、首帧缓存与预览**只经此函数**取值,不得各自另建映射。
 * 返回顺序即 THEME_PALETTE_VARS 顺序,便于逐项写入(无表达式、无 var() 引用)。
 */
export function paletteToCssVars(palette: ThemePalette): Record<string, string> {
  const vars: Record<string, string> = {}
  for (const [field, names] of Object.entries(PALETTE_CSS_VARS) as [
    keyof ThemePalette,
    readonly string[],
  ][]) {
    const value = palette[field]
    for (const name of names) vars[name] = value
  }
  return vars
}
