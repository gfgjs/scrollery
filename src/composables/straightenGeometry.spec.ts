import { describe, expect, it } from 'vitest'
import golden from '../fixtures/straightenGeometryGolden.json'
import {
  maximumAspectInscribedSize,
  STRAIGHTEN_MAX_DEGREES,
  STRAIGHTEN_MIN_DEGREES,
} from './straightenGeometry'

describe('straightenGeometry', () => {
  it('与 Rust 消费同一组最大内接矩形黄金向量', () => {
    for (const vector of golden) {
      expect(maximumAspectInscribedSize(vector.width, vector.height, vector.angle)).toEqual({
        width: vector.innerWidth,
        height: vector.innerHeight,
      })
    }
  })

  it('钉死闭区间端点', () => {
    expect(STRAIGHTEN_MIN_DEGREES).toBe(-45)
    expect(STRAIGHTEN_MAX_DEGREES).toBe(45)
  })
})
