// 信息浮窗(对齐 DOM .media-thumb__info-overlay:底部渐变 + 徽章行 + 信息行)组装 + 逐帧缓存,
// 从 MediaGridCanvas.vue 下沉(方案 2.2 ③)。工厂封装 infoGen/infoLineCache 两个命令式状态,
// 宿主在 <script setup> 顶层只调用一次 createInfoOverlayCache(),数据/设置换代时调 invalidate()
// (对应原 infoGen++ + infoLineCache.clear())。
import type { LayoutRowItem, MediaMeta } from '../../types/layout'
import { formatFileSize } from '../../utils/format'
import { buildThumbInfoLines, typeBadgeOf } from './mediaGrid.helpers'
import { truncateToWidth } from './mediaGridCanvas.helpers'
import type { Palette } from './mediaGridCanvas.palette'
import type { DemoInfoFormatter } from '../../utils/demoAlias'

/** drawInfoOverlay 每次调用显式接收的上下文(替代原闭包读取 props/palette)。 */
export interface InfoOverlayCtx {
  palette: Palette
  viewportMeta: Map<number, MediaMeta>
  thumbInfoElements: readonly string[]
  showThumbInfo: boolean
  /** 演示打码的信息浮窗文案替换(宿主注入;关闭时为 null → 走真实文案)。 */
  demoInfo: DemoInfoFormatter | null
}

// showInfo 关闭时的空文本块:模块级复用,避免逐帧新建对象。
const NO_DEMO_TEXT: { lines: string[]; demoSize: string | null } = { lines: [], demoSize: null }

export function createInfoOverlayCache() {
  // 行组装含 toLocaleString 等昂贵调用,不能逐帧逐格现算:按 id 缓存,gen 随数据/设置换代,
  // key 另含 sortDatetime 与格宽(截断随宽变)。
  let infoGen = 0
  // 缓存一个 id 的**整块**浮窗文本(信息行 + 演示化的文件大小角标文本):二者同一 gen 口径,
  // 演示开关翻转即整块失效,不会出现「行换了、大小没换」的混合文案。
  const infoLineCache = new Map<number, { key: string; lines: string[]; demoSize: string | null }>()

  function infoBundleFor(
    ctx: CanvasRenderingContext2D,
    item: LayoutRowItem,
    maxW: number,
    ctxArgs: InfoOverlayCtx,
  ): { lines: string[]; demoSize: string | null } {
    const key = `${infoGen}|${item.sortDatetime}|${Math.round(maxW)}`
    const hit = infoLineCache.get(item.id)
    if (hit && hit.key === key) return hit
    const meta = ctxArgs.viewportMeta.get(item.id)
    // 演示文案只在缓存未命中路径组装(逐帧逐格热路径零额外分配)。
    const demo = ctxArgs.demoInfo ? ctxArgs.demoInfo(item, meta) : null
    const raw = buildThumbInfoLines(item, meta, ctxArgs.thumbInfoElements, demo)
    ctx.font = `10px ${ctxArgs.palette.fontMono}`
    const measure = (s: string) => ctx.measureText(s).width
    const lines = raw.map((l) => truncateToWidth(l, maxW, measure))
    if (infoLineCache.size > 4096) infoLineCache.clear() // 防长会话无界增长
    const bundle = { key, lines, demoSize: demo ? demo.fileSize : null }
    infoLineCache.set(item.id, bundle)
    return bundle
  }

  function drawInfoOverlay(
    ctx: CanvasRenderingContext2D,
    item: LayoutRowItem,
    x: number,
    y: number,
    w: number,
    h: number,
    loaded: boolean,
    ctxArgs: InfoOverlayCtx,
  ) {
    const { palette, showThumbInfo: showInfo, thumbInfoElements: els } = ctxArgs
    const sim = item.similarity
    // 文本块先取(同一 id 逐帧命中缓存):信息行 + 演示化的文件大小角标文本。
    const bundle = showInfo ? infoBundleFor(ctx, item, w - 12, ctxArgs) : NO_DEMO_TEXT
    // 徽章收集(条件逐字对齐 DOM 模板 v-if):ORIG/THUMB/大小/类型按设置,相似度/LIVE 常显。
    // 类型角标的判定不在此逐字重写,走 helpers.typeBadgeOf 与 DOM 共享单源。
    const badges: Array<{ text: string; px: number; mark?: string }> = []
    if (sim == null && loaded && showInfo && els.includes('status')) {
      if (item.thumbStatus === 3) badges.push({ text: 'ORIG', px: 11 })
      else if (item.thumbStatus === 1)
        badges.push({ text: 'THUMB', px: 11 })
    }
    if (sim == null && item.fileSize && showInfo && els.includes('size')) {
      // 演示打码:文件大小示例化(存在性判定仍用真实 fileSize,不新增原来没有的角标)。
      badges.push({ text: bundle.demoSize ?? formatFileSize(item.fileSize), px: 11 })
    }
    if (sim != null) badges.push({ text: `${Math.round(sim * 100)}%`, px: 11 })
    if (item.isLivePhoto) badges.push({ text: 'LIVE', px: 11, mark: palette.badgeMarkLive })
    if (showInfo && els.includes('type')) {
      const kind = typeBadgeOf(item.mediaType, item.fileFormat, item.thumbStatus)
      if (kind === 'audio') badges.push({ text: 'AUDIO', px: 11, mark: palette.badgeMarkAudio })
      else if (kind === 'document')
        badges.push({ text: 'DOC', px: 11, mark: palette.badgeMarkDocument })
      else if (kind === 'raw') badges.push({ text: 'RAW', px: 11 })
    }
    const lines = bundle.lines
    if (badges.length === 0 && lines.length === 0) return

    // 底部渐变(to top: 0.85 → 70% 处 0.5 → 顶透明;padding-top 24 即渐隐区)
    const lineH = 12
    const badgesH = badges.length > 0 ? 16 : 0
    const overlayH = Math.min(h, 24 + badgesH + lines.length * lineH + 6)
    const oy = y + h - overlayH
    const grad = ctx.createLinearGradient(0, oy, 0, y + h)
    grad.addColorStop(0, 'rgba(0, 0, 0, 0)')
    grad.addColorStop(0.3, 'rgba(0, 0, 0, 0.5)')
    grad.addColorStop(1, 'rgba(0, 0, 0, 0.85)')
    ctx.fillStyle = grad
    ctx.fillRect(x, oy, w, overlayH)

    const linesTop = y + h - 6 - lines.length * lineH
    // 徽章行(左起单行排布,溢出截断队尾;DOM 为 flex-wrap,canvas 单行在常规格宽下等价)
    if (badges.length > 0) {
      let bx = x + 6
      const by = linesTop - 16
      for (const b of badges) {
      ctx.font = `600 ${b.px}px system-ui, -apple-system, sans-serif`
        ctx.textAlign = 'left'
        ctx.textBaseline = 'middle'
        const tw = ctx.measureText(b.text).width
        const markSpace = b.mark ? 9 : 0
        const bw = tw + 10 + markSpace
        if (bx + bw > x + w - 6) break
        ctx.beginPath()
        ctx.roundRect(bx, by, bw, 16, palette.radiusXs)
        ctx.fillStyle = palette.badgeScrim
        ctx.fill()
        if (b.mark) {
          ctx.beginPath()
          ctx.arc(bx + 6, by + 8, 2.5, 0, Math.PI * 2)
          ctx.fillStyle = b.mark
          ctx.fill()
        }
        ctx.fillStyle = '#fff'
        ctx.fillText(b.text, bx + 5 + markSpace, by + 8)
        bx += bw + 4
      }
    }
    // 信息行(mono 10px 白字 + 文字投影)
    if (lines.length > 0) {
      ctx.save()
      ctx.font = `10px ${palette.fontMono}`
      ctx.textAlign = 'left'
      ctx.textBaseline = 'alphabetic'
      ctx.fillStyle = '#fff'
      ctx.shadowColor = 'rgba(0, 0, 0, 0.8)'
      ctx.shadowBlur = 2
      ctx.shadowOffsetY = 1
      for (let i = 0; i < lines.length; i++) {
        ctx.fillText(lines[i], x + 6, linesTop + i * lineH + 9)
      }
      ctx.restore()
    }
  }

  function invalidate() {
    infoGen++
    infoLineCache.clear()
  }

  return { drawInfoOverlay, invalidate }
}

export type InfoOverlayCache = ReturnType<typeof createInfoOverlayCache>
