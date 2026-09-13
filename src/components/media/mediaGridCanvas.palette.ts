// Canvas 网格调色板(挂载/换尺寸/换主题/换浓度时读一次并缓存;canvas 需具体色值,不能用 CSS 变量)。
// 纯数据 + 一个 DOM 读取函数,从 MediaGridCanvas.vue 下沉(方案 2.2 ①)。
import { resolveTokenColor } from '../../utils/cssColor'
import type { docBadgeKind } from './mediaGrid.helpers'

/**
 * 媒体徽标/评分的共享色板源。
 *
 * 这些值同时由 variables.css 提供给 DOM 与 Canvas;这里仅作 computed style
 * 读取失败时的同源 fallback,不要在六套主题里各自覆写。
 */
export const CANONICAL_BADGE_COLORS = {
  scrim: 'rgba(0, 0, 0, 0.6)',
  markLive: '#ffaaaa',
  markAudio: '#5dd39e',
  markDocument: '#ffd166',
  ratingAmber: '#fbbf24',
} as const

export interface Palette {
  accent: string
  sepText: string
  /** 无 ThumbHash 格子的格面底色。亮色主题刻意比 gallery canvas 更暗，
   *  暗色主题则稍亮，降低“整片底色 ↔ 彩色缩略图”反复切换的明度闪烁。 */
  canvasPlaceholder: string
  /** 格缝/整幅清屏色 = --color-bg-canvas-gap,与画廊底色统一；格面占位色另保留区分度。 */
  canvasGap: string
  /** 格内 1px 内描边 = --color-thumb-outline:白底图贴亮底/暗图贴暗底时勾出图与底的边界。
   *  用户诉求是"加阴影"——canvas 每帧逐格 shadowBlur 走中间面模糊,数百格 60fps 不可承受;
   *  1px 零模糊描边等效达成分隔且近零成本,DOM 侧 .media-thumb::before 同 token 同源。 */
  thumbOutline: string
  /** 选中态目标圆角 = --radius-lg(px 数值;DOM .media-thumb--selected 的 border-radius)。 */
  radiusLg: number
  /** 分隔行文字色 = --color-text-primary。 */
  textPrimary: string
  /** 文本文档卡纸面/仿文本行(--color-doc-paper(-line),S5 收敛的可主题化 token)。 */
  docPaper: string
  docPaperLine: string
  /** 文本卡扩展名徽章按格式配色(--color-badge-doc-*)。 */
  badgeDoc: Record<ReturnType<typeof docBadgeKind>, string>
  /** B1 canonical 徽标源;B4 用于 scrim + 类别点/短边,不让类别色承载整块文字背景。 */
  badgeScrim: string
  badgeMarkLive: string
  badgeMarkAudio: string
  badgeMarkDocument: string
  /** 评分星专用 amber,不复用 warning。 */
  ratingAmber: string
  /** 圆角 token(徽章/药丸)。 */
  radiusXs: number
  radiusSm: number
  /** 等宽字体栈 = --font-mono(时长/信息行/扩展名)。 */
  fontMono: string
}

export const defaultPalette: Palette = {
  accent: '#4a9',
  sepText: '#888',
  canvasPlaceholder: '#cfd4d7',
  canvasGap: '#c0c6ca',
  thumbOutline: 'rgba(0, 0, 0, 0.07)',
  radiusLg: 10,
  textPrimary: '#eee',
  docPaper: '#f5f2ea',
  docPaperLine: '#d8d2c4',
  badgeDoc: { generic: '#6c757d', md: '#4a5568', word: '#2b579a', excel: '#217346', ppt: '#d24726' },
  badgeScrim: CANONICAL_BADGE_COLORS.scrim,
  badgeMarkLive: CANONICAL_BADGE_COLORS.markLive,
  badgeMarkAudio: CANONICAL_BADGE_COLORS.markAudio,
  badgeMarkDocument: CANONICAL_BADGE_COLORS.markDocument,
  ratingAmber: CANONICAL_BADGE_COLORS.ratingAmber,
  radiusXs: 4,
  radiusSm: 6,
  fontMono: 'ui-monospace, SFMono-Regular, Consolas, monospace',
}

/**
 * 从 el 的 computed style 读一份新调色板对象(不再原地重写模块级 let,拆分后改为纯函数,
 * 调用点自行重新赋值——见 MediaGridCanvas.vue §3.2:取值时序不变,仍是"挂载/换尺寸/换主题
 * 时读一次,之后每帧引用同一对象引用"）。
 */
export function readPalette(el: HTMLElement): Palette {
  const s = getComputedStyle(el)
  const g = (name: string, fallback: string) => s.getPropertyValue(name).trim() || fallback
  // 含 var() 的 color-mix 浓度表达式(2026-09-06 起:底色 wash 五件 + 文字 ramp 两件)须解成
  // 具体色(fillStyle 吃不下 var());其余 token 仍是纯色,直接读原值。
  const gc = (name: string, fallback: string) => resolveTokenColor(el, name, fallback)
  return {
    accent: g('--color-accent', '#4a9'),
    sepText: gc('--color-text-secondary', '#888'),
    canvasPlaceholder: gc('--color-bg-canvas-placeholder', '#cfd4d7'),
    canvasGap: gc('--color-bg-canvas-gap', '#c0c6ca'),
    thumbOutline: g('--color-thumb-outline', 'rgba(0, 0, 0, 0.07)'),
    radiusLg: parseFloat(g('--radius-lg', '10px')) || 10,
    textPrimary: gc('--color-text-primary', '#eee'),
    docPaper: gc('--color-doc-paper', '#f5f2ea'),
    docPaperLine: gc('--color-doc-paper-line', '#d8d2c4'),
    badgeDoc: {
      generic: g('--color-badge-doc-generic', '#6c757d'),
      md: g('--color-badge-doc-md', '#4a5568'),
      word: g('--color-badge-doc-word', '#2b579a'),
      excel: g('--color-badge-doc-excel', '#217346'),
      ppt: g('--color-badge-doc-ppt', '#d24726'),
    },
    badgeScrim: g('--color-badge-scrim', CANONICAL_BADGE_COLORS.scrim),
    badgeMarkLive: g('--color-badge-mark-live', CANONICAL_BADGE_COLORS.markLive),
    badgeMarkAudio: g('--color-badge-mark-audio', CANONICAL_BADGE_COLORS.markAudio),
    badgeMarkDocument: g('--color-badge-mark-document', CANONICAL_BADGE_COLORS.markDocument),
    ratingAmber: g('--color-rating-amber', CANONICAL_BADGE_COLORS.ratingAmber),
    radiusXs: parseFloat(g('--radius-xs', '4px')) || 4,
    radiusSm: parseFloat(g('--radius-sm', '6px')) || 6,
    fontMono: g('--font-mono', 'ui-monospace, SFMono-Regular, Consolas, monospace'),
  }
}
