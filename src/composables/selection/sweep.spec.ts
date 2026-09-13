// selection/sweep.ts 的 applyRangeInvert 单元测试。
// 覆盖「本体滑动 = 对划过区间做一次反转」的核心语义:空基线进入选择态、起点已选取消(需求1)、
// 混合区逐格反转(需求2)、相对基线幂等(回弹不重复翻转)、入参不可变、重复 id 鲁棒。
// 这条扫选路径此前零单测(属项目「按风险补测」的高风险交互类)——先以纯函数钉死语义。

import { describe, it, expect } from 'vitest'
import { applyRangeInvert } from './sweep'

const s = (...ids: number[]) => new Set(ids)
const sorted = (set: Set<number>) => [...set].sort((a, b) => a - b)

describe('applyRangeInvert（框选扫过区间的一次反转）', () => {
  it('空基线 + 区间 → 全部选中(进入选择态的基本情形,等价旧 select)', () => {
    expect(sorted(applyRangeInvert(s(), [1, 2, 3]))).toEqual([1, 2, 3])
  })

  it('全选基线 + 同区间 → 全部取消(需求1:已选中格滑动=取消选中)', () => {
    expect(sorted(applyRangeInvert(s(1, 2, 3), [1, 2, 3]))).toEqual([])
  })

  it('混合区逐格反转:已选→消、未选→选(需求2)', () => {
    // 基线选中 2/4;区间 [1..5] → 1/3/5 变选中,2/4 变取消
    expect(sorted(applyRangeInvert(s(2, 4), [1, 2, 3, 4, 5]))).toEqual([1, 3, 5])
  })

  it('区间外的基线项不受影响', () => {
    // 基线含区间外的 10;区间 [1..3] 只翻 1..3,10 保留
    expect(sorted(applyRangeInvert(s(10, 2), [1, 2, 3]))).toEqual([1, 3, 10])
  })

  it('相对基线幂等:同一(基线,区间)重复应用得同一结果', () => {
    const base = s(2, 4)
    const range = [1, 2, 3, 4, 5]
    const once = applyRangeInvert(base, range)
    const twice = applyRangeInvert(base, range) // 每次都相对原基线,非在上次结果上累积
    expect(sorted(once)).toEqual(sorted(twice))
  })

  it('收缩回弹:区间缩小,移出区间的项恢复基线态', () => {
    const base = s() // 空基线
    // 先扫 [1..4] → 全选;缩回 [1..2] → 3/4 因移出区间恢复基线(未选)
    expect(sorted(applyRangeInvert(base, [1, 2, 3, 4]))).toEqual([1, 2, 3, 4])
    expect(sorted(applyRangeInvert(base, [1, 2]))).toEqual([1, 2])
  })

  it('重复 id 鲁棒:区间含重复项仍只翻一次(判据基于 baseline 而非累积翻转)', () => {
    // 1 在基线未选,区间含两个 1 → 仍选中(翻一次),不是翻两次回到未选
    expect(sorted(applyRangeInvert(s(), [1, 1, 2]))).toEqual([1, 2])
    // 2 在基线已选,区间含两个 2 → 取消(翻一次)
    expect(sorted(applyRangeInvert(s(2), [2, 2, 3]))).toEqual([3])
  })

  it('不修改入参 baseline(纯函数)', () => {
    const base = s(1, 2)
    applyRangeInvert(base, [2, 3])
    expect(sorted(base)).toEqual([1, 2])
  })

  it('接受任意 Iterable 作为区间(如 Set)', () => {
    expect(sorted(applyRangeInvert(s(2), new Set([1, 2, 3])))).toEqual([1, 3])
  })
})
