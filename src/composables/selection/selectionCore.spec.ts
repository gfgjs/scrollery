// 选区核心：经典选择与扫选共用纯函数环境，保留范围、反选及不可变性回归。

import { describe, it, expect } from 'vitest'
import { classicMode } from './classicMode'
import { applyRangeInvert } from './sweep'
import type { SelectionContext, SelectionState } from './types'

// 构造测试上下文:rangeBetween 在给定布局序数组上取闭区间,镜像 useViewIds.rangeBetween 语义。
function makeCtx(viewIds: number[]): SelectionContext {
  return {
    viewIds,
    totalCount: viewIds.length,
    rangeBetween: (a, b) => {
      const ai = viewIds.indexOf(a)
      const bi = viewIds.indexOf(b)
      if (ai === -1 || bi === -1) return []
      const lo = Math.min(ai, bi)
      const hi = Math.max(ai, bi)
      return viewIds.slice(lo, hi + 1)
    },
  }
}

const explicit = (...ids: number[]): SelectionState => ({ kind: 'explicit', ids: new Set(ids) })
const all = (...excluded: number[]): SelectionState => ({ kind: 'all', excluded: new Set(excluded) })

// 断言 explicit 态的 id 集合（顺序无关）
function expectExplicit(state: SelectionState, ids: number[]) {
  expect(state.kind).toBe('explicit')
  if (state.kind === 'explicit') {
    expect([...state.ids].sort((a, b) => a - b)).toEqual([...ids].sort((a, b) => a - b))
  }
}
function expectAll(state: SelectionState, excluded: number[]) {
  expect(state.kind).toBe('all')
  if (state.kind === 'all') {
    expect([...state.excluded].sort((a, b) => a - b)).toEqual([...excluded].sort((a, b) => a - b))
  }
}

const ctx = makeCtx([1, 2, 3, 4, 5])

describe('classicMode.apply · replace', () => {
  it('从 all 也落到 explicit 单元素（替换语义清空全选）', () => {
    expectExplicit(classicMode.apply(all(2), { type: 'replace', id: 4 }, ctx), [4])
  })
})

describe('classicMode.apply · toggle', () => {
  it('explicit:未选则加入', () => {
    expectExplicit(classicMode.apply(explicit(1, 2), { type: 'toggle', id: 3 }, ctx), [1, 2, 3])
  })
  it('explicit:已选则移除', () => {
    expectExplicit(classicMode.apply(explicit(1, 2, 3), { type: 'toggle', id: 2 }, ctx), [1, 3])
  })
  it('all:翻转语义相反——翻转已选项 = 加入排除集', () => {
    // all 态下 id=3 当前「已选」(不在 excluded),toggle 应把它挖掉 → excluded 增加 3
    expectAll(classicMode.apply(all(), { type: 'toggle', id: 3 }, ctx), [3])
  })
  it('all:翻转已排除项 = 移出排除集（恢复选中）', () => {
    expectAll(classicMode.apply(all(3), { type: 'toggle', id: 3 }, ctx), [])
  })
})

describe('classicMode.apply · range', () => {
  it('explicit:并入布局序闭区间', () => {
    expectExplicit(
      classicMode.apply(explicit(1), { type: 'range', anchorId: 2, toId: 4 }, ctx),
      [1, 2, 3, 4],
    )
  })
  it('all:区间表示「选中这些」→ 从排除集移除', () => {
    // all 排除 [2,3,4],对 [2..4] 做 range → 这些恢复选中 → excluded 清空
    expectAll(classicMode.apply(all(2, 3, 4), { type: 'range', anchorId: 2, toId: 4 }, ctx), [])
  })
  it('端点不在视图（rangeBetween 返空）→ explicit 选区不变', () => {
    expectExplicit(
      classicMode.apply(explicit(1), { type: 'range', anchorId: 99, toId: 4 }, ctx),
      [1],
    )
  })
})

describe('classicMode.apply · selectAll', () => {
  it('归一为 all 态、排除集为空（不物化 id）', () => {
    expectAll(classicMode.apply(explicit(1, 2), { type: 'selectAll' }, ctx), [])
  })
})


describe('classicMode.apply · invert', () => {
  it('explicit → 全集补集', () => {
    expectExplicit(classicMode.apply(explicit(1, 3, 5), { type: 'invert' }, ctx), [2, 4])
  })
  it('all{excluded} → explicit{excluded}（补集恰为排除集,廉价路径）', () => {
    expectExplicit(classicMode.apply(all(2, 4), { type: 'invert' }, ctx), [2, 4])
  })
})

describe('classicMode.apply · 纯函数不变性', () => {
  it('不修改入参 state 的 Set', () => {
    const state = explicit(1, 2, 3)
    const snapshot = state.kind === 'explicit' ? [...state.ids] : []
    classicMode.apply(state, { type: 'toggle', id: 9 }, ctx)
    classicMode.apply(state, { type: 'range', anchorId: 1, toId: 5 }, ctx)
    classicMode.apply(state, { type: 'invert' }, ctx)
    expect(state.kind === 'explicit' && [...state.ids]).toEqual(snapshot)
  })
})

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

})
