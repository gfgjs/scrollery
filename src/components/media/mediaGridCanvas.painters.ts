// Canvas 网格单格绘制原语 + 分隔行绘制,从 MediaGridCanvas.vue 下沉(方案 2.2 ②)。
// 纯绘制函数:不再闭包捕获模块级 palette/props,改为显式接收 palette(和 drawSeparator
// 额外接收 groupBy)参数——签名调整,不改变任何取值时序或绘制结果(§3.2)。
import type { LayoutRowSeparator } from '../../types/layout'
import { docBadgeKind } from './mediaGrid.helpers'
import { starPoints, clampStickyLabelY } from './mediaGridCanvas.helpers'
import type { Palette } from './mediaGridCanvas.palette'

// 拖拽手柄几何(drawHandle 绘制 / hitHandleAt 命中 同源,防两处漂移——§3.3 单一来源,
// useCanvasHitTest.ts 从本模块 import,不得复制一份常量)。
export const HANDLE_INSET = 4
export const HANDLE_SIZE = 20

/** 选择模式 checkbox(右上,对齐 DOM .media-thumb__checkbox:20px 圆、白描边、选中 accent 底+白勾)。 */
export function drawCheckbox(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  checked: boolean,
  palette: Palette,
) {
  const size = 20
  const cx = x + w - 4 - size / 2
  const cy = y + 4 + size / 2
  ctx.beginPath()
  ctx.arc(cx, cy, size / 2, 0, Math.PI * 2)
  ctx.fillStyle = checked ? palette.accent : 'rgba(0, 0, 0, 0.3)'
  ctx.fill()
  ctx.strokeStyle = checked ? palette.accent : 'rgba(255, 255, 255, 0.9)'
  ctx.lineWidth = 2
  ctx.stroke()
  if (checked) {
    // 白勾(lucide Check 的两段折线近似)
    ctx.beginPath()
    ctx.moveTo(cx - 4.5, cy + 0.5)
    ctx.lineTo(cx - 1.5, cy + 3.5)
    ctx.lineTo(cx + 4.5, cy - 3.5)
    ctx.strokeStyle = '#fff'
    ctx.lineWidth = 2
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.stroke()
  }
}

/** 拖拽手柄(左上,对齐 DOM .media-thumb__drag-handle:圆角深底 + GripVertical 六点)。
 *  仅已选中格绘制(compact 亦绘 —— Canvas 画一笔零节点成本,故不按 compact 门控,符合裁决);
 *  (x,y) 为缩放后格左上角(随选中动画),命中判定 hitHandleAt 用 settled(0.85)几何,稳态两者对齐。 */
export function drawHandle(ctx: CanvasRenderingContext2D, x: number, y: number, palette: Palette) {
  const hx = x + HANDLE_INSET
  const hy = y + HANDLE_INSET
  ctx.beginPath()
  ctx.roundRect(hx, hy, HANDLE_SIZE, HANDLE_SIZE, palette.radiusSm)
  ctx.fillStyle = 'rgba(0, 0, 0, 0.5)'
  ctx.fill()
  // GripVertical 六点(2 列 × 3 行,白)
  ctx.fillStyle = 'rgba(255, 255, 255, 0.9)'
  const colXs = [hx + HANDLE_SIZE * 0.38, hx + HANDLE_SIZE * 0.62]
  const rowYs = [hy + HANDLE_SIZE * 0.3, hy + HANDLE_SIZE * 0.5, hy + HANDLE_SIZE * 0.7]
  for (const cxp of colXs) {
    for (const cyp of rowYs) {
      ctx.beginPath()
      ctx.arc(cxp, cyp, 1.3, 0, Math.PI * 2)
      ctx.fill()
    }
  }
}

/** 视频播放三角(半透明圆底 + 白三角,居中)。 */
export function drawPlayIcon(ctx: CanvasRenderingContext2D, x: number, sy: number, w: number, h: number) {
  const cx = x + w / 2
  const cy = sy + h / 2
  // 直径封顶 44px 对齐 DOM .badge-video 圆碟:悬停弹卡时圆钮不再明显缩放/变形。
  const r = Math.min(22, Math.min(w, h) * 0.16)
  ctx.fillStyle = 'rgba(0, 0, 0, 0.5)'
  ctx.beginPath()
  ctx.arc(cx, cy, r, 0, Math.PI * 2)
  ctx.fill()
  const t = r * 0.55
  ctx.fillStyle = '#fff'
  ctx.beginPath()
  ctx.moveTo(cx - t * 0.5, cy - t)
  ctx.lineTo(cx - t * 0.5, cy + t)
  ctx.lineTo(cx + t, cy)
  ctx.closePath()
  ctx.fill()
}

/** 时长角标(右下,深底白字 mono,圆角对齐 DOM .badge)。 */
export function drawDuration(
  ctx: CanvasRenderingContext2D,
  x: number,
  sy: number,
  w: number,
  h: number,
  text: string,
  palette: Palette,
) {
  ctx.font = `10px ${palette.fontMono}`
  ctx.textBaseline = 'alphabetic'
  ctx.textAlign = 'left'
  const padX = 4
  const tw = ctx.measureText(text).width
  const bw = tw + padX * 2
  const bh = 14
  const bx = x + w - bw - 6
  const by = sy + h - bh - 6
  ctx.beginPath()
  ctx.roundRect(bx, by, bw, bh, palette.radiusSm)
  ctx.fillStyle = 'rgba(0, 0, 0, 0.55)'
  ctx.fill()
  ctx.fillStyle = '#fff'
  ctx.fillText(text, bx + padX, by + bh - 4)
}

/** 可用态/暂存删除角标(左上,彩底白字,圆角对齐 DOM)。 */
export function drawAvailBadge(
  ctx: CanvasRenderingContext2D,
  x: number,
  sy: number,
  h: number,
  text: string,
  bg: string,
  palette: Palette,
) {
  ctx.font = '700 9px system-ui, -apple-system, sans-serif'
  ctx.textBaseline = 'alphabetic'
  ctx.textAlign = 'left'
  const padX = 5
  const tw = ctx.measureText(text).width
  const bw = tw + padX * 2
  const bh = 14
  const bx = x + 4
  // 左下角(让出左上给拖拽手柄;与 DOM .media-thumb__avail / .media-card__pending-badge 的 bottom:4 对齐)。
  const by = sy + h - 4 - bh
  ctx.beginPath()
  ctx.roundRect(bx, by, bw, bh, palette.radiusSm)
  ctx.fillStyle = bg
  ctx.fill()
  ctx.fillStyle = '#fff'
  ctx.fillText(text, bx + padX, by + bh - 4)
}

/** 未出图占位:居中扩展名(对齐 DOM .media-thumb__ext,mono 700 14px 半透明白)。 */
export function drawExtText(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  fmt: string,
  palette: Palette,
) {
  ctx.font = `700 14px ${palette.fontMono}`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  ctx.fillStyle = 'rgba(255, 255, 255, 0.4)'
  ctx.fillText(fmt.toUpperCase(), x + w / 2, y + h / 2, Math.max(0, w - 8))
}

/** 文本文档卡(对齐 DOM .media-thumb__textcard §3.4):纸面 + 装订线 + 仿文本行 + 品牌色扩展名徽章。 */
export function drawTextCard(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  fmt: string,
  palette: Palette,
) {
  ctx.fillStyle = palette.docPaper
  ctx.fillRect(x, y, w, h)
  // 顶端细装订线(border-top: 3px rgba(0,0,0,.06))
  ctx.fillStyle = 'rgba(0, 0, 0, 0.06)'
  ctx.fillRect(x, y, w, 3)
  // 仿文本行(padding 14%/12%,行高 5px 圆角 2,宽 65/100/92/55%)
  const padX = w * 0.12
  const padY = h * 0.14
  const innerW = w - padX * 2
  const lineH = 5
  const gap = (h - padY * 2) * 0.09
  ctx.fillStyle = palette.docPaperLine
  const widths = [0.65, 1, 0.92, 0.55]
  let ly = y + padY
  for (const f of widths) {
    ctx.beginPath()
    ctx.roundRect(x + padX, ly, innerW * f, lineH, 2)
    ctx.fill()
    ly += lineH + gap
  }
  // 扩展名徽章(左下,按格式品牌色)
  const text = (fmt || '').toUpperCase()
  ctx.font = `700 11px ${palette.fontMono}`
  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  const tw = ctx.measureText(text).width
  const bh = 17
  const bw = tw + 12
  const bx = x + padX
  const by = y + h - padY - bh
  ctx.beginPath()
  ctx.roundRect(bx, by, bw, bh, 3)
  ctx.fillStyle = palette.badgeDoc[docBadgeKind(fmt)]
  ctx.fill()
  ctx.fillStyle = '#fff'
  ctx.fillText(text, bx + 6, by + bh / 2)
}

/** 常显只读星级(左下,rating 颗琥珀填充星 + 深色投影;对齐 DOM StarRating readonly)。 */
export function drawStars(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  h: number,
  rating: number,
  palette: Palette,
  lift = 0,
) {
  const size = 12
  const gap = 2
  // lift:左下角有可用态/待删角标时上抬一档避让(与 DOM .media-thumb--missing 的 rating-slot 抬升对齐)。
  const top = y + h - 4 - size - lift
  ctx.save()
  ctx.shadowColor = 'rgba(0, 0, 0, 0.7)'
  ctx.shadowBlur = 3
  ctx.shadowOffsetY = 1
  ctx.fillStyle = palette.ratingAmber
  for (let i = 0; i < rating; i++) {
    const pts = starPoints(x + 6 + i * (size + gap) + size / 2, top + size / 2, size / 2)
    ctx.beginPath()
    ctx.moveTo(pts[0][0], pts[0][1])
    for (let p = 1; p < pts.length; p++) ctx.lineTo(pts[p][0], pts[p][1])
    ctx.closePath()
    ctx.fill()
  }
  ctx.restore()
}

/** 常显收藏红心(右下,实心 #ff4757 + 投影;对齐 DOM .media-thumb__fav.fav-always-visible)。 */
export function drawHeart(ctx: CanvasRenderingContext2D, x: number, y: number, s: number) {
  ctx.save()
  ctx.shadowColor = 'rgba(0, 0, 0, 0.6)'
  ctx.shadowBlur = 3
  ctx.shadowOffsetY = 1
  ctx.fillStyle = '#ff4757'
  ctx.beginPath()
  ctx.moveTo(x + s / 2, y + s * 0.3)
  ctx.bezierCurveTo(x + s / 2, y + s * 0.06, x, y + s * 0.06, x, y + s * 0.36)
  ctx.bezierCurveTo(x, y + s * 0.62, x + s / 2, y + s * 0.82, x + s / 2, y + s)
  ctx.bezierCurveTo(x + s / 2, y + s * 0.82, x + s, y + s * 0.62, x + s, y + s * 0.36)
  ctx.bezierCurveTo(x + s, y + s * 0.06, x + s / 2, y + s * 0.06, x + s / 2, y + s * 0.3)
  ctx.closePath()
  ctx.fill()
  ctx.restore()
}

// ── 分隔行(对齐 DOM .date-separator/.separator-content:图标 + 日期/数量)──
// lucide Folder 的 24×24 path(图标随分组方式:folder=文件夹,其余=日历)。
const FOLDER_ICON_PATH =
  'M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z'
let folderIconPath2D: Path2D | null = null
// lucide CopyCheck 的 24×24 路径(重复组头图标,§6.1 对齐 DOM MediaGridRow 的 CopyCheck;
// 前方方块含对勾 + 后方方块轮廓,以空格分隔的子路径并成一个 Path2D)。
const COPY_CHECK_ICON_PATH =
  'm12 15 2 2 4-4 M22 10V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h2 M8 8h12a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2Z'
let copyCheckIconPath2D: Path2D | null = null
// lucide Network 的 24×24 路径(folders 文件夹头簇首行的关联簇图标,§7.1 对齐 DOM 的
// Network:顶部小方块经连线接左右两个小方块,子路径并成一个 Path2D)。
const NETWORK_ICON_PATH =
  'M17 16h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1Z M3 16h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1Z M10 2h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1Z M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3 M12 12V8'
let networkIconPath2D: Path2D | null = null

export function drawSeparatorIcon(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  size: number,
  kind: 'folder' | 'calendar' | 'duplicate' | 'network',
  palette: Palette,
) {
  ctx.save()
  ctx.translate(x, y)
  ctx.scale(size / 24, size / 24)
  ctx.strokeStyle = palette.sepText
  ctx.lineWidth = 2
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  if (kind === 'folder') {
    if (!folderIconPath2D) folderIconPath2D = new Path2D(FOLDER_ICON_PATH)
    ctx.stroke(folderIconPath2D)
  } else if (kind === 'duplicate') {
    if (!copyCheckIconPath2D) copyCheckIconPath2D = new Path2D(COPY_CHECK_ICON_PATH)
    ctx.stroke(copyCheckIconPath2D)
  } else if (kind === 'network') {
    if (!networkIconPath2D) networkIconPath2D = new Path2D(NETWORK_ICON_PATH)
    ctx.stroke(networkIconPath2D)
  } else {
    // lucide Calendar:圆角矩形 + 双吊耳 + 横隔线
    ctx.beginPath()
    ctx.roundRect(3, 4, 18, 18, 2)
    ctx.moveTo(16, 2)
    ctx.lineTo(16, 6)
    ctx.moveTo(8, 2)
    ctx.lineTo(8, 6)
    ctx.moveTo(3, 10)
    ctx.lineTo(21, 10)
    ctx.stroke()
  }
  ctx.restore()
}

/** 文件夹头行文字组(§7.2):宿主经 i18n 组装;cluster 仅簇首非空(§7.2 避免两层 sticky)。 */
export interface LensFolderHeaderLines {
  cluster: string | null
  stats: string
}

/**
 * 文件夹头(§7.2/§11.2,canvas 对齐 DOM MediaGridRow 的 h3 分支):簇首三行(簇头行/路径行/
 * 三桶统计行)、其余两行;行内 sticky 只钳文字 y,不铺底板(标题行与画廊底融合,2026-09-12
 * 用户裁决);内容高于 36px 行盒时 clampStickyLabelY 自然回退为随行滚动(与 DOM sticky 行为一致)。
 */
export function drawLensFolderSeparator(
  ctx: CanvasRenderingContext2D,
  row: LayoutRowSeparator,
  sy: number,
  palette: Palette,
  lines: LensFolderHeaderLines | null,
) {
  const padX = 8
  const iconSize = 13
  const iconGap = 6
  const clusterLineH = lines?.cluster ? 12 : 0
  const pathLineH = 14
  const statLineH = 11
  const contentH = clusterLineH + pathLineH + statLineH
  const flowY = sy + 2
  const py = clampStickyLabelY(flowY, sy + row.height, contentH)

  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  let y = py
  // 簇头行(§7.1):Network 图标 + accent 文本(图标/文本双通道,不只靠颜色)。
  if (lines?.cluster) {
    drawSeparatorIcon(ctx, padX, y + 1, 10, 'network', palette)
    ctx.font = '500 10px system-ui, -apple-system, sans-serif'
    ctx.fillStyle = palette.accent
    ctx.fillText(lines.cluster, padX + 10 + 4, y + clusterLineH / 2 + 1)
    y += clusterLineH
  }
  // 路径行(§7.2 第一行):folder 图标 + 文件夹显示路径。
  drawSeparatorIcon(ctx, padX, y + 1, iconSize, 'folder', palette)
  ctx.font = '600 12px system-ui, -apple-system, sans-serif'
  ctx.fillStyle = palette.textPrimary
  ctx.fillText(row.separatorLabel, padX + iconSize + iconGap, y + pathLineH / 2 + 1)
  y += pathLineH
  // 统计行(§7.2 第二行):三桶文案,左缩进对齐路径文本,次要色弱化层级。
  if (lines) {
    ctx.font = '400 10px system-ui, -apple-system, sans-serif'
    ctx.fillStyle = palette.sepText
    ctx.fillText(lines.stats, padX + iconSize + iconGap, y + statLineH / 2)
  }
}

export function drawSeparator(
  ctx: CanvasRenderingContext2D,
  row: LayoutRowSeparator,
  sy: number,
  palette: Palette,
  groupBy: string,
  count?: number,
  lensFolder?: LensFolderHeaderLines | null,
  lensGroupLabel?: string | null,
) {
  // 文件夹头(folders 镜头)独立分支:两/三行结构,与通用单行分隔符不同构。
  if (row.separatorKind === 'duplicateFolder') {
    drawLensFolderSeparator(ctx, row, sy, palette, lensFolder ?? null)
    return
  }
  const isDupGroup = row.separatorKind === 'duplicateGroup'
  const label = isDupGroup ? lensGroupLabel ?? row.separatorLabel : row.separatorLabel
  const iconSize = 16
  const gap = 8
  const contentH = 20
  const padX = 4
  const flowY = sy + Math.max(2, (row.height - contentH) / 2)
  // 行内 sticky 只钳标签 y:重复组头(§6.1)与 folder 分隔符行内跟随,date 维持随滚即走。
  // 标题行不再铺底色/投影/分隔线(2026-09-12 裁决:与画廊底沉浸融合)。
  const sticky = isDupGroup || groupBy === 'folder'
  const py = sticky ? clampStickyLabelY(flowY, sy + row.height, contentH) : flowY

  ctx.font = '600 13px system-ui, -apple-system, sans-serif'
  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  const textW = ctx.measureText(label).width
  const iconKind = isDupGroup ? 'duplicate' : groupBy === 'folder' ? 'folder' : 'calendar'
  drawSeparatorIcon(ctx, padX, py + 2, iconSize, iconKind, palette)
  ctx.fillStyle = palette.textPrimary
  const textX = padX + iconSize + gap
  ctx.fillText(label, textX, py + contentH / 2)
  // 重复组头 label 已含完整统计(「重复组 N · M 项 · …」),不再追加 count 数字避免信息重复;
  // date/folder 分隔符维持「label + 数量」现状。
  if (count != null && !isDupGroup) {
    ctx.font = '400 12px system-ui, -apple-system, sans-serif'
    ctx.fillStyle = palette.sepText
    ctx.fillText(String(count), textX + textW + gap, py + contentH / 2)
  }
}

/** 重复镜头组内位次角标(§6.2,M/N;右上,对齐 DOM .media-card__lens-badge 的 scrim 底)。 */
export function drawLensBadge(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  text: string,
  palette: Palette,
) {
  ctx.font = '700 9px system-ui, -apple-system, sans-serif'
  ctx.textBaseline = 'alphabetic'
  ctx.textAlign = 'left'
  const padX = 5
  const tw = ctx.measureText(text).width
  const bw = tw + padX * 2
  const bh = 14
  const bx = x + w - bw - 4
  const by = y + 4
  ctx.beginPath()
  ctx.roundRect(bx, by, bw, bh, palette.radiusSm)
  ctx.fillStyle = 'rgba(0, 0, 0, 0.55)'
  ctx.fill()
  ctx.fillStyle = '#fff'
  ctx.fillText(text, bx + padX, by + bh - 4)
}
