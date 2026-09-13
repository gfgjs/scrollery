// src/composables/useAdjustPreview.ts
// E3 调色预览(D-104):在后端 E0 预览位图之上,用 canvas 逐像素应用双端同源公式。
// 禁 CSS filter(与 Rust 公式数学不等价,设计 §1.3 实证);blob URL 同源,getImageData
// 无 taint 风险(D-109)。重绘经 rAF 节流:一帧内多次滑杆输入只合并绘制最后一份参数。

import { ref } from 'vue'

import {
  adjustCoefficients,
  applyAdjustToPixels,
  isNeutralAdjust,
  type AdjustParams,
} from './adjustFormula'

/** 最小像素缓冲面(node 测试环境无真实 ImageData,面向结构注入)。 */
export interface AdjustImageData {
  readonly data: Uint8ClampedArray
  readonly width: number
  readonly height: number
}

/** 最小 2D 上下文面;真实 CanvasRenderingContext2D 结构兼容。 */
export interface AdjustCanvasContext {
  drawImage(image: { naturalWidth: number; naturalHeight: number }, dx: number, dy: number): void
  getImageData(sx: number, sy: number, sw: number, sh: number): AdjustImageData
  createImageData(sw: number, sh: number): AdjustImageData
  putImageData(imagedata: AdjustImageData, dx: number, dy: number): void
}

/** 最小 canvas 面;真实 HTMLCanvasElement 结构兼容。 */
export interface AdjustCanvasLike {
  width: number
  height: number
  getContext(contextId: '2d'): AdjustCanvasContext | null
}

// vitest 跑在 node 环境(无 rAF):退化为 16ms 定时器,节流语义不变。
const scheduleFrame: (callback: () => void) => number =
  typeof requestAnimationFrame === 'function'
    ? (callback) => requestAnimationFrame(() => callback())
    : (callback) => setTimeout(callback, 16) as unknown as number
const cancelFrame: (id: number) => void =
  typeof cancelAnimationFrame === 'function'
    ? (id) => cancelAnimationFrame(id)
    : (id) => clearTimeout(id)

export function useAdjustPreview() {
  const canvasRef = ref<AdjustCanvasLike | null>(null)
  /** 基准像素已抓取,canvas 可展示(EditOverlay 据此决定 img/canvas 切换)。 */
  const ready = ref(false)

  let base: AdjustImageData | null = null
  let scratch: AdjustImageData | null = null
  let frameId: number | null = null
  let pending: AdjustParams | null = null

  /**
   * 预览 `<img>` 装载完成后调用:按位图尺寸重设 canvas、绘入源并抓取基准像素。
   * 抓取失败(极端环境 taint / 无 2D 上下文)返回 false,调用方维持纯 `<img>` 展示。
   */
  function setSource(image: { naturalWidth: number; naturalHeight: number } | null): boolean {
    base = null
    scratch = null
    ready.value = false
    const canvas = canvasRef.value
    if (!image || !canvas || !image.naturalWidth || !image.naturalHeight) return false
    const context = canvas.getContext('2d')
    if (!context) return false
    try {
      canvas.width = image.naturalWidth
      canvas.height = image.naturalHeight
      context.drawImage(image, 0, 0)
      base = context.getImageData(0, 0, image.naturalWidth, image.naturalHeight)
    } catch {
      base = null
      return false
    }
    ready.value = true
    return true
  }

  /** rAF 节流入口:同一帧内的多次调用只按最后一份参数绘一次。 */
  function schedule(params: AdjustParams): void {
    pending = { ...params }
    if (frameId !== null) return
    frameId = scheduleFrame(() => {
      frameId = null
      const next = pending
      pending = null
      if (next) paint(next)
    })
  }

  function paint(params: AdjustParams): void {
    const canvas = canvasRef.value
    if (!canvas || !base) return
    const context = canvas.getContext('2d')
    if (!context) return
    if (isNeutralAdjust(params)) {
      context.putImageData(base, 0, 0)
      return
    }
    if (!scratch || scratch.width !== base.width || scratch.height !== base.height) {
      scratch = context.createImageData(base.width, base.height)
    }
    scratch.data.set(base.data)
    applyAdjustToPixels(scratch.data, adjustCoefficients(params))
    context.putImageData(scratch, 0, 0)
  }

  function dispose(): void {
    if (frameId !== null) cancelFrame(frameId)
    frameId = null
    pending = null
    base = null
    scratch = null
    ready.value = false
  }

  return { canvasRef, ready, setSource, schedule, dispose }
}
