// 框选扫过区间的一次反转：保留区间语义与不可变性回归。

import { describe, it, expect } from 'vitest'
import { applyRangeInvert } from './sweep'

const s = (...ids: number[]) => new Set(ids)
const sorted = (set: Set<number>) => [...set].sort((a, b) => a - b)

describe('applyRangeInvert（框选扫过区间的一次反转）', () => {
  it('混合区逐格反转:已选→消、未选→选(需求2)', () => {
    // 基线选中 2/4;区间 [1..5] → 1/3/5 变选中,2/4 变取消
    expect(sorted(applyRangeInvert(s(2, 4), [1, 2, 3, 4, 5]))).toEqual([1, 3, 5])
  })
})
