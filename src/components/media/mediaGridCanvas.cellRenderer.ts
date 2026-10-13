// Canvas 网格 drawCell 复合入口(选中动画几何 + 占位/图 + 覆盖层编排)。工厂 createCellRenderer
// 在宿主 <script setup> 顶层只调用一次,注入依赖一次性绑定;返回的渲染函数是每帧热路径,
// 内部只调用已绑定的 painters/infoOverlay 函数引用,不重新构造闭包。
import { colorLabelHex } from '../../constants/colorLabels'
import { formatDuration } from '../../utils/format'
import type { LayoutRowItem, MediaMeta } from '../../types/layout'
import type { CachedImage } from '../../composables/useCanvasThumbPipeline'
import {
  coverRect,
  SelectionAnimTracker,
  SELECT_EASE,
  SELECT_ANIM_MS,
} from './mediaGridCanvas.helpers'
import { isTextCardFormat, isTextCardFallback } from './mediaGrid.helpers'
import {
  drawCheckbox,
  drawHandle,
  drawPlayIcon,
  drawDuration,
  drawAvailBadge,
  drawExtText,
  drawTextCard,
  drawStars,
  drawHeart,
  drawLensBadge,
} from './mediaGridCanvas.painters'
import type { Palette } from './mediaGridCanvas.palette'
import type { InfoOverlayCtx } from './mediaGridCanvas.infoOverlay'
import type { DemoInfoFormatter } from '../../utils/demoAlias'
import { computeThumbBottomLayout, thumbActionsFit } from './thumbBottomLayout'

export interface CellRendererDeps {
  getPalette: () => Palette
  selAnim: SelectionAnimTracker
  getImage: (item: LayoutRowItem) => CachedImage | null
  isBlendEnabled: () => boolean
  /** 将混合底图羽化到图内，覆盖层和选区随后清晰绘制。 */
  drawBlendEdges: (
    ctx: CanvasRenderingContext2D,
    item: LayoutRowItem,
    y: number,
    hoverSurface: boolean,
  ) => void
  isPendingDelete: (id: number) => boolean
  isSelected: (id: number) => boolean
  compactCells: () => boolean
  showThumbInfo: () => boolean
  /** 演示打码开关(2026-09-16):开启时缩略图位图在 drawImage 阶段强模糊(只改显示,不新增模糊位图)。 */
  demoPrivacy: () => boolean
  /** 画布设备像素比:模糊半径按设备像素给定,格尺寸与 DPR 变化时观感一致。 */
  devicePixelRatio: () => number
  /** 信息浮窗演示文案组装(关闭时返回 null → 走真实文案)。 */
  demoInfo: () => DemoInfoFormatter | null
  thumbInfoElements: () => readonly string[]
  showDragHandle: () => boolean
  isSelectionMode: () => boolean
  pendingDeleteLabel: () => string
  availMissingLabel: () => string
  availOfflineLabel: () => string
  /** 重复镜头激活:镜头卡片徽标绘制开关,普通画廊恒 false(零绘制开销)。 */
  lensActive: () => boolean
  /** 镜头卡片徽标文本:宿主经 lensSeparator.resolveLensCardBadge + t 组装;M/N /「组 N」/
   *  问号兜底文本;null = 无徽标(独有/普通画廊)。 */
  lensCardBadgeText: (item: LayoutRowItem) => string | null
  viewportMeta: () => Map<number, MediaMeta>
  /** 当前操作控件归 DOM 的项；静态文字和角标始终由 Canvas 提供。 */
  interactionCellId: () => number
  /** 动态预览首帧已呈现的项，Canvas 仅绘制其信息与装饰。 */
  previewCellId: () => number
  drawInfoOverlay: (
    ctx: CanvasRenderingContext2D,
    item: LayoutRowItem,
    x: number,
    y: number,
    w: number,
    h: number,
    loaded: boolean,
    ctxArgs: InfoOverlayCtx,
  ) => void
}

/**
 * 工厂返回复合绘制入口；悬停面可借用已选图源，并明确让出操作控件。
 * @returns 本帧是否画出了位图
 * (现行或 stale;false = 该格仍是占位/文本卡,探针冷格口径)——与原 drawCell 语义一致。
 */
export function createCellRenderer(deps: CellRendererDeps) {
  // drawInfoOverlay 的入参对象在每帧每格都会传入,若逐次字面量新建则数千格/帧造成热路径
  // 分配抖动;改持单例并就地刷新字段,drawInfoOverlay 内部只读不跨帧持有引用,故安全复用。
  const infoOverlayCtx: InfoOverlayCtx = {
    palette: deps.getPalette(),
    viewportMeta: deps.viewportMeta(),
    thumbInfoElements: deps.thumbInfoElements(),
    showThumbInfo: deps.showThumbInfo(),
    demoInfo: deps.demoInfo(),
    bottom: computeThumbBottomLayout(0, false, false, false),
  }
  return function drawCell(
    ctx: CanvasRenderingContext2D,
    item: LayoutRowItem,
    sy: number,
    now: number,
    imageOverride?: CachedImage | null,
    forceInteractive = false,
  ): boolean {
    const palette = deps.getPalette()
    const missing = item.availability === 'missing'
    const offline = item.availability === 'offline'
    const pending = deps.isPendingDelete(item.id)
    const selected = deps.isSelected(item.id)
    // 选中程度 0..1(动画中为 ease 插值,过冲曲线可短暂 >1):仅驱动圆角/描边环。
    const d = deps.selAnim.degree(item.id, selected, now, SELECT_EASE, SELECT_ANIM_MS)
    const interactive = forceInteractive || deps.interactionCellId() === item.id
    const previewing = deps.previewCellId() === item.id
    const hasStatus = missing || offline || pending || !!item.durationMs
    const splitStatus = item.w < 112 && !!item.durationMs && (missing || offline || pending)
    const actionsFit = thumbActionsFit(item.w, item.h, hasStatus, splitStatus)
    // 可操作小格常态提前预留同一槽，容纳不了的极密格沿用 compact 省略。
    const bottom = computeThumbBottomLayout(
      item.w,
      deps.lensActive(),
      !actionsFit,
      hasStatus,
      infoOverlayCtx.bottom,
      splitStatus,
    )

    // 选中几何对齐 DOM:内容尺寸保持不变,只过渡圆角与 2px 内描边。
    const x = item.x
    const y = sy
    const w = item.w
    const h = item.h
    const rad = Math.max(0, 2 + (palette.radiusLg - 2) * d)
    const ent = imageOverride === undefined ? deps.getImage(item) : imageOverride
    const source = ent?.src
    const sw = source instanceof HTMLImageElement ? source.naturalWidth : (source?.width ?? 0)
    const sh = source instanceof HTMLImageElement ? source.naturalHeight : (source?.height ?? 0)
    const loaded = sw > 0 && sh > 0

    // 整卡包裹①:暂存删除 = DOM .media-card--pending-delete 的整卡 grayscale+brightness(0.7)
    // + opacity 0.45——filter 作用于全部子元素(徽章/描边环随卡灰化),canvas 以外层包裹等价。
    ctx.save()
    const baseAlpha = pending ? 0.45 : 1
    if (pending) {
      ctx.filter = 'grayscale(1) brightness(0.7)'
      ctx.globalAlpha = baseAlpha
    }

    // 包裹②圆角裁剪:只在选中/动画中付出(d>0)。基础态 DOM 的 2px 圆角在格间隙旁不可辨,
    // 而极密网格同屏数千格,逐格无谓 clip 是热路径开销——刻意跳过(与 DOM compact 纪律同思路)。
    const clipping = d > 0.001
    ctx.save()
    if (clipping) {
      ctx.beginPath()
      ctx.roundRect(x, y, w, h, rad)
      ctx.clip()
    }

    // 1. 占位底色 + 缩略图 —— missing/offline 用 ctx.filter 复刻 DOM 的 grayscale/opacity。
    if (!previewing) {
      ctx.save()
      if (missing) {
        ctx.filter = 'grayscale(1) brightness(0.85)'
        ctx.globalAlpha = 0.5 * baseAlpha
      } else if (offline) {
        ctx.filter = 'grayscale(0.7)'
        ctx.globalAlpha = 0.72 * baseAlpha
      }
      // 无 ThumbHash 的格子使用独立格面色，和统一的画廊背景保持区分，避免等待态糊成一片。
      ctx.fillStyle = item.placeholderColor ?? palette.canvasPlaceholder
      ctx.fillRect(x, y, w, h)
      if (source && loaded) {
        const s = source
        // 现行规格位图已在解码期裁到格纵横比,coverRect 退化为全源平贴;stale 位图(规格过期
        // 续画)与 Image 回退(整图)则靠它保住 cover 语义不变形。
        const c = coverRect(sw, sh, w, h)
        // 演示打码:同一次 drawImage 上加强模糊——请求/解码/缓存全部原样,只改这次显示的像素
        //(不生成第二套模糊位图)。半径随格尺寸取,并与外层 missing/offline 灰化 filter 复合
        //(拼接而非覆盖);先按格矩形 clip、再把目标矩形外扩一个半径绘制,模糊边缘落在格内,
        //不越界污染邻格,也不出现格缘渐隐。
        //
        // 两种半径分开口径:blurLogical 是**逻辑坐标**下的扩边量(与 x/y/w/h 同一坐标系),
        // blurDevice 才是交给 ctx.filter 的值(filter 不随 CTM 缩放,须按 DPR 放大才等观感)。
        // 拿同一个放大过的值去扩边会让 DPR=2 时裁切幅度翻倍,故两个量必须分开。
        const blurLogical = deps.demoPrivacy() ? Math.max(6, Math.min(w, h) * 0.1) : 0
        if (blurLogical > 0) {
          const blurDevice = blurLogical * deps.devicePixelRatio()
          const baseFilter = ctx.filter
          ctx.save()
          ctx.beginPath()
          ctx.rect(x, y, w, h)
          ctx.clip()
          ctx.filter =
            baseFilter && baseFilter !== 'none'
              ? `${baseFilter} blur(${blurDevice}px)`
              : `blur(${blurDevice}px)`
          ctx.drawImage(
            s,
            c.sx,
            c.sy,
            c.sw,
            c.sh,
            x - blurLogical,
            y - blurLogical,
            w + blurLogical * 2,
            h + blurLogical * 2,
          )
          ctx.restore()
        } else {
          ctx.drawImage(s, c.sx, c.sy, c.sw, c.sh, x, y, w, h)
        }
      }
      // 未出图占位内容(对齐 DOM !isLoaded 分支):文本文档走「文本卡」,其余画居中扩展名。
      if (!loaded) {
        // epub 补充降级(与 DOM isTextCard 同构):封面盖棺失败(status=2)也出文本卡。
        if (
          !deps.compactCells() &&
          (isTextCardFormat(item.mediaType, item.fileFormat) ||
            isTextCardFallback(item.mediaType, item.fileFormat, item.thumbStatus))
        ) {
          drawTextCard(ctx, x, y, w, h, item.fileFormat, palette)
        } else if (item.fileFormat) {
          drawExtText(ctx, x, y, w, h, item.fileFormat, palette)
        }
      }
      // 融合画廊的已加载图不描普通边线，让图缘接入底图；选中描边在后面独立绘制。
      if (!loaded || !deps.isBlendEnabled()) {
        ctx.strokeStyle = palette.thumbOutline
        ctx.lineWidth = 1
        ctx.strokeRect(x + 0.5, y + 0.5, w - 1, h - 1)
      }
      ctx.restore()
      if (loaded && deps.isBlendEnabled()) deps.drawBlendEdges(ctx, item, y, forceInteractive)
    }

    // 2. 颜色标签色条(顶缘 4px;compact 亦显,对齐 DOM)。
    if (item.colorLabel > 0) {
      const hex = colorLabelHex(item.colorLabel)
      if (hex) {
        ctx.fillStyle = hex
        ctx.fillRect(x, y, w, 4)
      }
    }

    // 3. 非 compact 常显覆盖层(对齐 DOM overlays 的绘制序:信息浮窗 → 播放/时长 → 星级/红心)。
    //    静态信息与角标始终由 Canvas 绘制，操作控件可让位给单例 DOM。
    if (!deps.compactCells()) {
      infoOverlayCtx.palette = palette
      infoOverlayCtx.viewportMeta = deps.viewportMeta()
      infoOverlayCtx.thumbInfoElements = deps.thumbInfoElements()
      infoOverlayCtx.showThumbInfo = deps.showThumbInfo()
      infoOverlayCtx.demoInfo = deps.demoInfo()
      deps.drawInfoOverlay(ctx, item, x, y, w, h, loaded, infoOverlayCtx)
      if (item.mediaType === 'video' && !previewing) drawPlayIcon(ctx, x, y, w, h)
      if (item.durationMs)
        drawDuration(
          ctx,
          x,
          y,
          w,
          h,
          formatDuration(item.durationMs),
          palette,
          bottom.durationBottom - 6,
        )
      // 常显只读星级(rating>0,左下,琥珀填充星;交互 5 星条归悬停卡)。
      // 与交互五星共用操作槽，可用态另占状态行。
      if (!interactive && actionsFit && item.rating > 0 && !deps.lensActive())
        drawStars(
          ctx,
          x,
          y,
          h,
          item.rating,
          palette,
          bottom.ratingBottom - 4,
          Math.min(12, (w - 20) / 5),
        )
      // 常显收藏红心(已收藏 + 设置勾选 favorite;hover 出现的场景归悬停卡)。
      if (
        !interactive &&
        actionsFit &&
        !deps.lensActive() &&
        item.isFavorited &&
        deps.showThumbInfo() &&
        deps.thumbInfoElements().includes('favorite')
      ) {
        drawHeart(ctx, x + w - 22, y + h - bottom.favoriteBottom - 18, 14)
      }
    }

    // 4. 可用态角标(左下,让出左上给拖拽手柄;清晰,不受灰化;compact 亦显,对齐 DOM)。
    if (missing)
      drawAvailBadge(
        ctx,
        x,
        y,
        h,
        deps.availMissingLabel(),
        'rgba(220, 53, 69, 0.92)',
        palette,
        bottom.actionHeight,
      )
    else if (offline)
      drawAvailBadge(
        ctx,
        x,
        y,
        h,
        deps.availOfflineLabel(),
        'rgba(108, 117, 125, 0.92)',
        palette,
        bottom.actionHeight,
      )

    // 5. 暂存删除角标(左下红底,同 DOM .media-card__pending-badge 样式,z 序盖过可用态角标;
    //    随外层 pending filter 灰化,与 DOM 的 filter 继承行为一致)。
    if (pending)
      drawAvailBadge(
        ctx,
        x,
        y,
        h,
        deps.pendingDeleteLabel(),
        'rgba(220, 53, 69, 0.92)',
        palette,
        bottom.actionHeight,
      )

    // 5.5 重复镜头卡片徽标(右上;与 DOM .media-card__lens-badge 同信息位,
    //     Canvas/DOM 两引擎一致):groups = 组内位次 M/N;folders = 重复卡片「组 N」文本 /
    //     尚未确认卡片问号(低成本:同 drawLensBadge 角标底 + '?' 文本,问号绘制不单开图标路径);
    //     独有/普通画廊恒 null 零绘制开销。徽标选择在宿主注入的 lensCardBadgeText
    //     (lensSeparator.resolveLensCardBadge 统一判定,DOM 同源)。
    if (deps.lensActive()) {
      const badgeText = deps.lensCardBadgeText(item)
      if (badgeText) drawLensBadge(ctx, x, y, w, badgeText, palette)
    }

    ctx.restore() // 结束圆角裁剪

    // 6. 选中态:2px accent 内描边,不铺整卡遮罩也不缩小内容。
    if (d > 0.001) {
      const a = Math.max(0, Math.min(1, d))
      ctx.beginPath()
      ctx.roundRect(x + 1, y + 1, Math.max(0, w - 2), Math.max(0, h - 2), Math.max(0, rad - 1))
      ctx.strokeStyle = palette.accent
      ctx.lineWidth = 2
      ctx.globalAlpha = a * baseAlpha
      ctx.stroke()
      ctx.globalAlpha = baseAlpha
    }

    // 7. 选择模式 checkbox(所有格常显,对齐 DOM --selection-mode;hover 出现的场景归悬停卡)。
    //    compact 不画:与 DOM 一致(MediaThumbCompact 无 checkbox,选中态由内描边表达)。
    if (!deps.compactCells() && deps.isSelectionMode() && !interactive) {
      drawCheckbox(ctx, x, y, w, selected, palette)
    }

    // 8. 拖拽手柄(已选中格左上,对齐 DOM .media-thumb__drag-handle)。selected 蕴含选择模式;
    //    刻意**不按 compact 门控** —— Canvas 画一笔零节点成本,compact 也提供拖到文件夹
    //    (裁决:Canvas 主推、全支持;DOM compact 才不提供)。
    //    showDragHandle 用户开关:关闭时不绘,hitHandleAt 同步短路(见彼处)。
    if (selected && deps.showDragHandle() && !interactive) drawHandle(ctx, x, y, palette)

    ctx.restore() // 结束暂存删除整卡灰化
    return loaded
  }
}
