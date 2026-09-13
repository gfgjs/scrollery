import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  useAdjustPreview,
  type AdjustCanvasContext,
  type AdjustCanvasLike,
  type AdjustImageData,
} from './useAdjustPreview'

// node 测试环境无真实 canvas:构造结构兼容的假 canvas,记录绘制调用。
function makeImageData(width: number, height: number): AdjustImageData {
  return { data: new Uint8ClampedArray(width * height * 4), width, height }
}

function makeCanvas(): {
  canvas: AdjustCanvasLike
  drawn: unknown[]
  puts: AdjustImageData[]
} {
  const drawn: unknown[] = []
  const puts: AdjustImageData[] = []
  let backing = makeImageData(0, 0)
  const context: AdjustCanvasContext = {
    drawImage(image) {
      drawn.push(image)
      const source = image as { naturalWidth: number; naturalHeight: number }
      backing = makeImageData(source.naturalWidth, source.naturalHeight)
      // 模拟位图内容:黄金向量 saturation=100 组的 8-bit 码值 153/102/51。
      for (let i = 0; i < backing.data.length; i += 4) {
        backing.data[i] = 153
        backing.data[i + 1] = 102
        backing.data[i + 2] = 51
        backing.data[i + 3] = 255
      }
    },
    getImageData: () => backing,
    createImageData: (w, h) => makeImageData(w, h),
    putImageData(data) {
      puts.push({ data: Uint8ClampedArray.from(data.data), width: data.width, height: data.height })
    },
  }
  const canvas: AdjustCanvasLike = { width: 0, height: 0, getContext: () => context }
  return { canvas, drawn, puts }
}

const image = { naturalWidth: 2, naturalHeight: 1 }

describe('useAdjustPreview', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('setSource 抓取基准像素并置 ready;无 canvas 时安全失败', () => {
    const preview = useAdjustPreview()
    expect(preview.setSource(image)).toBe(false)
    expect(preview.ready.value).toBe(false)

    const { canvas, drawn } = makeCanvas()
    preview.canvasRef.value = canvas
    expect(preview.setSource(image)).toBe(true)
    expect(preview.ready.value).toBe(true)
    expect(drawn).toHaveLength(1)
    expect(canvas.width).toBe(2)
  })

  it('一帧内多次 schedule 只按最后一份参数绘一次(rAF 节流)', () => {
    const preview = useAdjustPreview()
    const { canvas, puts } = makeCanvas()
    preview.canvasRef.value = canvas
    preview.setSource(image)

    preview.schedule({ brightness: 40, contrast: 0, saturation: 0 })
    preview.schedule({ brightness: 80, contrast: 0, saturation: 0 })
    preview.schedule({ brightness: 0, contrast: 0, saturation: 100 })
    expect(puts).toHaveLength(0)

    vi.runOnlyPendingTimers()
    expect(puts).toHaveLength(1)
    // 生效的是最后一份参数:输出应为黄金向量 saturation=100 的期望码值 197/95/0。
    expect([...puts[0].data.slice(0, 4)]).toEqual([197, 95, 0, 255])
  })

  it('全零参数直接回贴基准像素;dispose 后不再绘制', () => {
    const preview = useAdjustPreview()
    const { canvas, puts } = makeCanvas()
    preview.canvasRef.value = canvas
    preview.setSource(image)

    preview.schedule({ brightness: 0, contrast: 0, saturation: 0 })
    vi.runOnlyPendingTimers()
    expect(puts).toHaveLength(1)
    expect([...puts[0].data.slice(0, 4)]).toEqual([153, 102, 51, 255])

    preview.schedule({ brightness: 50, contrast: 0, saturation: 0 })
    preview.dispose()
    vi.runOnlyPendingTimers()
    expect(puts).toHaveLength(1)
    expect(preview.ready.value).toBe(false)
  })
})
