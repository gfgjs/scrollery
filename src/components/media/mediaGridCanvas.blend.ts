import { DEFAULTS } from '../../constants/defaults'
import type { CachedImage } from '../../composables/useCanvasThumbPipeline'
import type { GalleryBlendSettings } from '../../types/config'
import type { LayoutRow, LayoutRowItem } from '../../types/layout'
import { coverRect, visibleRowRange } from './mediaGridCanvas.helpers'

/** 创建可复用的低分辨率融合底图；只借用现有缩略图，不读取像素或另发解码请求。 */
export function createGalleryBlendLayer() {
  const source = document.createElement('canvas')
  const blurred = document.createElement('canvas')
  const sourceCtx = source.getContext('2d')!
  const blurredCtx = blurred.getContext('2d')!
  const scale = 0.5
  const gap = DEFAULTS.GRID_GAP
  const spread = gap / 2
  let padding = 0
  let strength = 1
  let edgeWidth = 0

  return {
    draw(
      ctx: CanvasRenderingContext2D,
      rows: readonly LayoutRow[],
      currentY: number,
      width: number,
      height: number,
      getImage: (item: LayoutRowItem) => CachedImage | null,
      placeholder: string,
      isPendingDelete: (id: number) => boolean,
      demoPrivacy: boolean,
      settings: GalleryBlendSettings,
    ) {
      padding = Math.ceil(settings.blurRadius * 3)
      strength = settings.strength / 100
      edgeWidth = settings.edgeWidth
      const bufferW = Math.ceil((width + padding * 2) * scale)
      const bufferH = Math.ceil((height + padding * 2) * scale)
      if (source.width !== bufferW || source.height !== bufferH) {
        source.width = blurred.width = bufferW
        source.height = blurred.height = bufferH
      }
      sourceCtx.setTransform(1, 0, 0, 1, 0, 0)
      sourceCtx.clearRect(0, 0, bufferW, bufferH)
      sourceCtx.setTransform(scale, 0, 0, scale, padding * scale, padding * scale)
      const range = visibleRowRange(rows, currentY - padding, currentY + height + padding)
      const footprint = new Path2D()

      for (let index = range.start; index < range.end; index++) {
        const row = rows[index]
        if (row.rowType !== 'normal') continue
        for (const item of row.items) {
          const x = item.x - spread
          const y = row.y - currentY - spread
          const w = item.w + gap
          const h = item.h + gap
          footprint.rect(x, y, w, h)
          // 色块先接满缝隙再整体模糊，避免逐图透明光晕叠在底色上留下暗线。
          sourceCtx.fillStyle = item.placeholderColor ?? placeholder
          sourceCtx.fillRect(x, y, w, h)
          const image = getImage(item)?.src
          const sw = image instanceof HTMLImageElement ? image.naturalWidth : (image?.width ?? 0)
          const sh = image instanceof HTMLImageElement ? image.naturalHeight : (image?.height ?? 0)
          if (!image || sw <= 0 || sh <= 0) continue
          const crop = coverRect(sw, sh, item.w, item.h)
          sourceCtx.save()
          if (isPendingDelete(item.id)) {
            sourceCtx.filter = 'grayscale(1) brightness(0.7)'
            sourceCtx.globalAlpha = 0.45
          } else if (item.availability === 'missing') {
            sourceCtx.filter = 'grayscale(1) brightness(0.85)'
            sourceCtx.globalAlpha = 0.5
          } else if (item.availability === 'offline') {
            sourceCtx.filter = 'grayscale(0.7)'
            sourceCtx.globalAlpha = 0.72
          }
          if (demoPrivacy) {
            const privacyBlur = Math.max(6, Math.min(item.w, item.h) * 0.1) * scale
            const base = sourceCtx.filter === 'none' ? '' : sourceCtx.filter
            sourceCtx.filter = `${base} blur(${privacyBlur}px)`
            sourceCtx.beginPath()
            sourceCtx.rect(x, y, w, h)
            sourceCtx.clip()
          }
          sourceCtx.drawImage(image, crop.sx, crop.sy, crop.sw, crop.sh, x, y, w, h)
          sourceCtx.restore()
        }
      }

      // 整屏仅模糊一次半分辨率底图；主体、文字、选区仍由原绘制流程清晰覆盖。
      blurredCtx.clearRect(0, 0, bufferW, bufferH)
      blurredCtx.filter = `blur(${settings.blurRadius * scale}px)`
      blurredCtx.drawImage(source, 0, 0)
      ctx.save()
      // 只填格子与半条间隙的并集，分组标题和末行的空白保留原背景。
      ctx.clip(footprint)
      ctx.globalAlpha = strength
      ctx.drawImage(blurred, -padding, -padding, bufferW / scale, bufferH / scale)
      ctx.restore()
      if (edgeWidth === 0) return

      // 图缘仅用窄过渡接入缝隙颜色，避免在缩略图内部形成模糊边框。
      // 在半分辨率下统一生成，避免为每张缩略图分配额外画布。
      sourceCtx.clearRect(-padding, -padding, bufferW / scale, bufferH / scale)
      for (let index = range.start; index < range.end; index++) {
        const row = rows[index]
        if (row.rowType !== 'normal') continue
        for (const item of row.items) {
          const x = item.x
          const y = row.y - currentY
          const w = item.w
          const h = item.h
          const feather = Math.min(edgeWidth, Math.min(w, h) / 2)
          const left = sourceCtx.createLinearGradient(x, 0, x + feather, 0)
          left.addColorStop(0, '#fff')
          left.addColorStop(1, 'rgba(255,255,255,0)')
          sourceCtx.fillStyle = left
          sourceCtx.fillRect(x, y, feather, h)
          const right = sourceCtx.createLinearGradient(x + w, 0, x + w - feather, 0)
          right.addColorStop(0, '#fff')
          right.addColorStop(1, 'rgba(255,255,255,0)')
          sourceCtx.fillStyle = right
          sourceCtx.fillRect(x + w - feather, y, feather, h)
          const top = sourceCtx.createLinearGradient(0, y, 0, y + feather)
          top.addColorStop(0, '#fff')
          top.addColorStop(1, 'rgba(255,255,255,0)')
          sourceCtx.fillStyle = top
          sourceCtx.fillRect(x, y, w, feather)
          const bottom = sourceCtx.createLinearGradient(0, y + h, 0, y + h - feather)
          bottom.addColorStop(0, '#fff')
          bottom.addColorStop(1, 'rgba(255,255,255,0)')
          sourceCtx.fillStyle = bottom
          sourceCtx.fillRect(x, y + h - feather, w, feather)
        }
      }
      blurredCtx.save()
      blurredCtx.filter = 'none'
      blurredCtx.globalCompositeOperation = 'destination-in'
      blurredCtx.drawImage(source, 0, 0)
      blurredCtx.restore()
    },
    drawEdges(
      ctx: CanvasRenderingContext2D,
      x: number,
      y: number,
      w: number,
      h: number,
      sampleY: number,
    ) {
      if (edgeWidth === 0) return
      ctx.save()
      // 底图已包含可用态和演示打码，贴回时不重复施加灰化或透明度。
      ctx.filter = 'none'
      ctx.globalAlpha = strength
      ctx.drawImage(
        blurred,
        (x + padding) * scale,
        (sampleY + padding) * scale,
        w * scale,
        h * scale,
        x,
        y,
        w,
        h,
      )
      ctx.restore()
    },
    dispose() {
      source.width = source.height = blurred.width = blurred.height = 0
    },
  }
}
