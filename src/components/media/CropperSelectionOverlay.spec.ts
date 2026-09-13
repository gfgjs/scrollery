import { describe, expect, it } from 'vitest'

import { normalizedToSelection, selectionToNormalized } from './cropperCoordinates'

describe('CropperSelectionOverlay 坐标适配', () => {
  it('归一化状态与 Cropper.js 显示坐标可无损往返', () => {
    const normalized = { x: 0.125, y: 0.2, width: 0.5, height: 0.6 }
    const selection = normalizedToSelection(normalized, 1600, 900)
    expect(selection).toEqual({ x: 200, y: 180, width: 800, height: 540 })
    expect(selectionToNormalized(selection, 1600, 900)).toEqual(normalized)
  })

  it('90/270° 后只需使用交换后的显示框尺寸，契约仍是变换后坐标系', () => {
    const normalized = { x: 0.1, y: 0.25, width: 0.8, height: 0.5 }
    const portraitSelection = normalizedToSelection(normalized, 900, 1600)
    expect(portraitSelection).toEqual({ x: 90, y: 400, width: 720, height: 800 })
    expect(selectionToNormalized(portraitSelection, 900, 1600)).toEqual(normalized)
  })
})
