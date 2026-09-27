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

describe('classicMode.apply · toggle', () => {
  it('all:翻转语义相反——翻转已选项 = 加入排除集', () => {
    // all 态下 id=3 当前「已选」(不在 excluded),toggle 应把它挖掉 → excluded 增加 3
    expectAll(classicMode.apply(all(), { type: 'toggle', id: 3 }, ctx), [3])
  })
})

describe('classicMode.apply · range', () => {
  it('explicit:并入布局序闭区间', () => {
    expectExplicit(
      classicMode.apply(explicit(1), { type: 'range', anchorId: 2, toId: 4 }, ctx),
      [1, 2, 3, 4],
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

const s = (...ids: number[]) => new Set(ids)
const sorted = (set: Set<number>) => [...set].sort((a, b) => a - b)

describe('applyRangeInvert（框选扫过区间的一次反转）', () => {

  it('混合区逐格反转:已选→消、未选→选(需求2)', () => {
    // 基线选中 2/4;区间 [1..5] → 1/3/5 变选中,2/4 变取消
    expect(sorted(applyRangeInvert(s(2, 4), [1, 2, 3, 4, 5]))).toEqual([1, 3, 5])
  })

  it('收缩回弹:区间缩小,移出区间的项恢复基线态', () => {
    const base = s() // 空基线
    // 先扫 [1..4] → 全选;缩回 [1..2] → 3/4 因移出区间恢复基线(未选)
    expect(sorted(applyRangeInvert(base, [1, 2, 3, 4]))).toEqual([1, 2, 3, 4])
    expect(sorted(applyRangeInvert(base, [1, 2]))).toEqual([1, 2])
  })

  it('不修改入参 baseline(纯函数)', () => {
    const base = s(1, 2)
    applyRangeInvert(base, [2, 3])
    expect(sorted(base)).toEqual([1, 2])
  })

})
