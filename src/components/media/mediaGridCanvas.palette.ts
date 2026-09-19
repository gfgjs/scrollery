// Canvas 网格调色板:把主题生成的色板投影成网格绘制所需的字段(方案 §6)。
//
// Canvas 需要具体色值(fillStyle 吃不下 var()/color-mix()),故颜色**只**来自 store 发布的
// ThemePalette——DOM 与 Canvas 同一份生成结果,这里不保留第二套默认色、也不逐帧 getComputedStyle。
// 只有非颜色的尺寸/字体度量仍从 DOM 读一次(它们来自 variables.css,与主题无关)。
import type { ThemePalette } from '../../themes/types'
import type { docBadgeKind } from './mediaGrid.helpers'

/** 网格绘制消费的调色板:字段按绘制用途命名,值全部来自传入的 ThemePalette。 */
export interface Palette {
  accent: string
  /** 分隔行辅助文字(计数/副标题)。取画廊底派生的辅助文字:显式 gallery 可与窗口底色反极性，
   *  用窗口 textSecondary 会在「暗界面 + 浅色画廊」上读不清。 */
  sepText: string
  /** 无缩略图格子的格面底色。亮色主题刻意比 gallery canvas 更暗，
   *  暗色主题则稍亮，降低“整片底色 ↔ 彩色缩略图”反复切换的明度闪烁。 */
  canvasPlaceholder: string
  /** 格缝/整幅清屏色 = canvasGap,与画廊底色统一；格面占位色另保留区分度。 */
  canvasGap: string
  /** 格内 1px 内描边:白底图贴亮底/暗图贴暗底时勾出图与底的边界。
   *  用户诉求是“加阴影”——canvas 每帧逐格 shadowBlur 走中间面模糊,数百格 60fps 不可承受;
   *  1px 零模糊描边等效达成分隔且近零成本,DOM 侧 .media-thumb::before 同变量同源。 */
  thumbOutline: string
  /** 选中态目标圆角 = --radius-lg(px 数值;DOM .media-thumb--selected 的 border-radius)。 */
  radiusLg: number
  /** 分隔行标题文字。同上取画廊底派生的正文,不拿窗口 textPrimary。 */
  textPrimary: string
  /** 文本文档卡纸面/仿文本行(--color-doc-paper(-line),从实际画廊底色派生)。 */
  docPaper: string
  docPaperLine: string
  /** 文本卡扩展名徽章按格式配色(--color-badge-doc-*)。 */
  badgeDoc: Record<ReturnType<typeof docBadgeKind>, string>
  /** 图片上方徽标/评分的专用遮罩与类别点色:品牌语义色,不随主题漂移。 */
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

/** 非颜色的度量值:与主题无关,仍按现有机制从 DOM 读一次。 */
export interface PaletteMetrics {
  radiusLg: number
  radiusXs: number
  radiusSm: number
  fontMono: string
}

/** 度量回退:仅用于挂载测量前的理论调用点;颜色一律来自传入色板,此处不设任何颜色默认值。 */
export const PALETTE_METRICS_FALLBACK: PaletteMetrics = {
  radiusLg: 10,
  radiusXs: 4,
  radiusSm: 6,
  fontMono: 'monospace',
}

/** 读一次尺寸/字体度量(挂载与换尺寸时调用;不含任何颜色读取)。 */
export function readPaletteMetrics(el: HTMLElement): PaletteMetrics {
  const s = getComputedStyle(el)
  const num = (name: string, fallback: number) => parseFloat(s.getPropertyValue(name)) || fallback
  return {
    radiusLg: num('--radius-lg', PALETTE_METRICS_FALLBACK.radiusLg),
    radiusXs: num('--radius-xs', PALETTE_METRICS_FALLBACK.radiusXs),
    radiusSm: num('--radius-sm', PALETTE_METRICS_FALLBACK.radiusSm),
    fontMono: s.getPropertyValue('--font-mono').trim() || PALETTE_METRICS_FALLBACK.fontMono,
  }
}

/**
 * 色板 + 度量 → 网格调色板(纯投影,无算法、无默认值)。
 *
 * 调用点:挂载 / 换尺寸 / 主题换代。之后每帧引用同一对象引用,不再触碰 DOM。
 */
export function projectPalette(theme: ThemePalette, metrics: PaletteMetrics): Palette {
  return {
    accent: theme.accent,
    sepText: theme.canvasTextSecondary,
    canvasPlaceholder: theme.canvasPlaceholder,
    canvasGap: theme.canvasGap,
    thumbOutline: theme.thumbOutline,
    radiusLg: metrics.radiusLg,
    textPrimary: theme.canvasText,
    docPaper: theme.docPaper,
    docPaperLine: theme.docPaperLine,
    badgeDoc: {
      generic: theme.badgeDocGeneric,
      md: theme.badgeDocMd,
      word: theme.badgeDocWord,
      excel: theme.badgeDocExcel,
      ppt: theme.badgeDocPpt,
    },
    badgeScrim: theme.badgeScrim,
    badgeMarkLive: theme.badgeMarkLive,
    badgeMarkAudio: theme.badgeMarkAudio,
    badgeMarkDocument: theme.badgeMarkDocument,
    ratingAmber: theme.ratingAmber,
    radiusXs: metrics.radiusXs,
    radiusSm: metrics.radiusSm,
    fontMono: metrics.fontMono,
  }
}
