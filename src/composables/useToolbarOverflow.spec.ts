// useToolbarOverflow 纯核单测(顶栏重构 P3-1)。Priority+ 切分逻辑,与 DOM 无关,可测。
import { describe, it, expect } from 'vitest'
import { computeOverflowSplit } from './useToolbarOverflow'

describe('computeOverflowSplit (Priority+ 纯核)', () => {
  it('空列表 → 无可见无溢出', () => {
    expect(computeOverflowSplit([], 100, 40)).toEqual({ visibleCount: 0, hasOverflow: false })
  })

  it('全放得下 → 全可见,无 ⋯ 按钮(总宽 ≤ 可用宽,含恰好相等)', () => {
    // 90 ≤ 100
    expect(computeOverflowSplit([30, 30, 30], 100, 40)).toEqual({
      visibleCount: 3,
      hasOverflow: false,
    })
    // 100 ≤ 100(恰好):仍全可见,不因「加 ⋯ 会超」而误溢出
    expect(computeOverflowSplit([50, 50], 100, 40)).toEqual({ visibleCount: 2, hasOverflow: false })
  })

  it('放不下 → 预留 ⋯ 按钮宽,从头贪心塞(总 150>100,budget 60,塞 30+30)', () => {
    expect(computeOverflowSplit([30, 30, 30, 30, 30], 100, 40)).toEqual({
      visibleCount: 2,
      hasOverflow: true,
    })
  })

  it('边界:总 90>75,budget 35,只塞得下 1 个 30', () => {
    expect(computeOverflowSplit([30, 30, 30], 75, 40)).toEqual({
      visibleCount: 1,
      hasOverflow: true,
    })
  })

  it('预留 ⋯ 后一个都塞不下 → 0 可见 + 溢出(总 90>50,budget 10,首项 30>10)', () => {
    expect(computeOverflowSplit([30, 30, 30], 50, 40)).toEqual({
      visibleCount: 0,
      hasOverflow: true,
    })
  })

  it('可用宽为 0 → 全溢出', () => {
    expect(computeOverflowSplit([10, 10], 0, 40)).toEqual({ visibleCount: 0, hasOverflow: true })
  })

  it('gap 计入总宽:3×30 + 2×10 gap = 110 > 100 → 溢出(gap=0 时 90≤100 本不溢出)', () => {
    // 无 gap:总 90 ≤ 100 → 全可见;有 gap=10:总 110 > 100 → 触发溢出。
    expect(computeOverflowSplit([30, 30, 30], 100, 40, 0)).toEqual({
      visibleCount: 3,
      hasOverflow: false,
    })
    expect(computeOverflowSplit([30, 30, 30], 100, 40, 10)).toEqual({
      visibleCount: 1,
      hasOverflow: true,
    })
  })

  it('gap 计入贪心:budget=available−⋯−gap, 非首项前加 gap(总 150>100, budget 100−40−10=50, 塞 30 + (10+?)→仅 1)', () => {
    // budget=50:首项 30(used 30);次项需 10+30=40 → 30+40=70>50 → 停, visible=1。
    expect(computeOverflowSplit([30, 30, 30, 30, 30], 100, 40, 10)).toEqual({
      visibleCount: 1,
      hasOverflow: true,
    })
  })
})

/**
 * 内容宽容器(containerFillsWidth:false, 如浮动选区胶囊)的**正反馈刻画**——真机 round10 #3 的回归防线。
 *
 * 纯核本身没错;错在**喂给它的 available 是内生的**。全宽容器(AppToolbar, flex:1)里 available 由父级
 * 决定、与 visibleCount 无关 → overflowButtonWidth 高估几 px 只是早折一项, 稳定。内容宽容器里
 * available = f(visibleCount)(容器宽随折叠自缩), 高估即**正反馈点火器**:每轮 budget 都比当前内容窄,
 * 掉一项 → 内容更窄 → 再掉一项, 一路吞到全折叠, 且与拉宽/收窄方向无关。
 *
 * 故不变量:**内容宽容器的 overflowButtonWidth 必须等于 ⋯ 按钮实际渲染宽**。SelectionActions 靠把同一
 * 常量 inline 绑到按钮 style 上由构造保证;此处刻画「若不等会怎样」, 使该约束的理由不随注释腐烂。
 */
describe('内生 available 的自反馈(内容宽容器不变量)', () => {
  const ITEM_W = 40
  const ITEMS = [ITEM_W, ITEM_W, ITEM_W, ITEM_W, ITEM_W, ITEM_W]
  /** ⋯ 按钮**实际**渲染宽(= .selection-action 的 CSS width, 浮动态 36px)。 */
  const ACTUAL_MORE_W = 36

  /**
   * 模拟内容宽容器的一次 window-resize 重算:探针读回的可用宽 = 当前可见项宽和 + 实际 ⋯ 按钮宽
   * (⋯ 在 flow 内、有溢出时 display 出来 → 被一并量进 clientWidth)。反复迭代看是否收敛。
   */
  function iterate(declaredMoreW: number, startVisible: number, rounds = 10): number {
    let visible = startVisible
    for (let i = 0; i < rounds; i++) {
      const available = ITEM_W * visible + ACTUAL_MORE_W
      visible = computeOverflowSplit(ITEMS, available, declaredMoreW, 0).visibleCount
    }
    return visible
  }

  it('声明宽 = 实际宽 → 任意起点都是不动点, 不吞项', () => {
    // visible=4: available=196, budget=196−36=160=恰好 4 项 → 不动。
    expect(iterate(ACTUAL_MORE_W, 4)).toBe(4)
    expect(iterate(ACTUAL_MORE_W, 2)).toBe(2)
  })

  it('声明宽高估 4px → 每轮掉一项, 一路吞到全折叠(真机 round10 #3 症状:拉宽也继续折)', () => {
    // 4: available=196, budget=156 → 塞 3;3: budget=116 → 2;2: budget=76 → 1;1: budget=36 → 0。
    expect(iterate(40, 4)).toBe(0)
  })

  // **高估与低估的失效模式不对称**——别把前者镜像成后者(本测试初版就是这么写错的)。
  it('声明宽低估 → **不**发散(仍是不动点), 其危害是预留不足致渲染越界, 属 DOM 层、纯核测不到', () => {
    // budget 虚高 4px 但不足以多塞一项(项宽 40 >> 4)→ 停在原处, 无棘轮。
    expect(iterate(32, 4)).toBe(4)
  })

  it('低估跨过项边界时 → 少留了 ⋯ 的位置(占用超可用宽), 这是它真正的坏处', () => {
    // 实际 ⋯ 占 36:budget=160−36=124 → 3 项(120), 占用 120+36=156 ≤ 160 ✓
    expect(computeOverflowSplit(ITEMS, 160, ACTUAL_MORE_W, 0).visibleCount).toBe(3)
    // 声明 0(极端低估):budget=160 → 4 项(160), 占用 160+36=196 > 160 → ⋯ 被挤出容器/末项被裁。
    expect(computeOverflowSplit(ITEMS, 160, 0, 0).visibleCount).toBe(4)
  })
})
