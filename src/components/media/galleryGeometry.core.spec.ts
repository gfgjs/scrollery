// 核心回归：按风险保留独立用例，同域夹具集中；不以展示细节作为验收门槛。
import { describe, it, expect } from 'vitest'
import { hitTestCell, visibleRowRange,  } from './mediaGridCanvas.helpers'
import { type LayoutRow, type LayoutRowItem } from '../../types/layout'
import { thumbGeometry, thumbTopToLogicalY } from './mediaScrollbar.helpers'

describe('网格命中与位图几何', () => {
  /** 构造一个正常行的最小 item(只填命中相关字段)。 */
  function item(id: number, x: number, w: number): LayoutRowItem {
    return {
      id,
      x,
      w,
      h: 60,
      fileSize: 0,
      fileFormat: '',
      mediaType: 'image',
      isLivePhoto: false,
      durationMs: null,
      thumbStatus: 1,
      thumbPath: null,
      placeholderColor: null,
      isFavorited: false,
      rating: 0,
      colorLabel: 0,
      availability: 'online',
      originalWidth: 0,
      originalHeight: 0,
      sortDatetime: 0,
    }
  }


  describe('hitTestCell', () => {

    it('大量有序行使用二分命中，不退化为从首行扫描', () => {
      const manyRows: LayoutRow[] = Array.from({ length: 8192 }, (_, i) => ({
        rowType: 'normal' as const,
        y: i * 60,
        height: 60,
        items: [item(i, 0, 50)],
      }))
      let numericReads = 0
      const observed = new Proxy(manyRows, {
        get(target, prop, receiver) {
          if (typeof prop === 'string' && /^\d+$/.test(prop)) numericReads++
          return Reflect.get(target, prop, receiver)
        },
      })
      expect(hitTestCell(observed, 10, 7000 * 60 + 10)?.id).toBe(7000)
      expect(numericReads).toBeLessThan(50)
    })
  })

  describe('visibleRowRange', () => {

    it('大量有序行只读取对数数量的行', () => {
      const manyRows: LayoutRow[] = Array.from({ length: 8192 }, (_, i) => ({
        rowType: 'normal' as const,
        y: i * 60,
        height: 60,
        items: [item(i, 0, 50)],
      }))
      let numericReads = 0
      const observed = new Proxy(manyRows, {
        get(target, prop, receiver) {
          if (typeof prop === 'string' && /^\d+$/.test(prop)) numericReads++
          return Reflect.get(target, prop, receiver)
        },
      })
      expect(visibleRowRange(observed, 7000 * 60 + 1, 7002 * 60 - 1)).toEqual({
        start: 7000,
        end: 7002,
      })
      expect(numericReads).toBeLessThan(50)
    })
  })
})

describe('逻辑滚动条', () => {
  describe('mediaScrollbar.helpers', () => {

    it('拖拽映射与拇指几何互逆(round-trip,含钳高形态)', () => {
      const total = 30_000_000
      const trackH = 900
      for (const y of [0, 123_456, 15_000_000, total - trackH]) {
        const g = thumbGeometry(y, total, trackH)!
        expect(thumbTopToLogicalY(g.top, total, trackH, g.height)).toBeCloseTo(y, 4)
      }
    })
  })
})
