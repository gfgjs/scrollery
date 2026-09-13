import { describe, it, expect } from 'vitest'
import { clampRevealScrollTop, bodyRevealNeeded } from './accordion.helpers'

// 场景基准:headerH=36、4 个区块、viewportH=600。
// 主体属于 index=i 的区块:topInset=(i+1)*36,bottomInset=(3-i)*36。

describe('clampRevealScrollTop', () => {
  it('主体已完全可见 → 不动(返回 cur)', () => {
    // index=1(工具):topInset=72 bottomInset=72;bodyFlowTop=100 bodyH=200
    // 可行区间 [100+200+72-600, 100-72] = [-228, 28];cur=10 在区间内
    expect(clampRevealScrollTop(10, 100, 200, 600, 72, 72)).toBe(10)
  })

  it('标题粘底(主体在视口下方)→ 最小下滚,主体底恰贴粘底堆叠之上', () => {
    // index=3(管理,末位):topInset=144 bottomInset=0;主体在 5000 处高 120,cur=1000
    // alignBottom = 5000+120+0-600 = 4520
    expect(clampRevealScrollTop(1000, 5000, 120, 600, 144, 0)).toBe(4520)
  })

  it('标题粘顶(主体在视口上方)→ 最小上滚,主体顶恰贴粘顶堆叠之下', () => {
    // index=0(图库):topInset=36 bottomInset=108;主体在 40 处高 200,cur=8000
    // alignTop = 40-36 = 4
    expect(clampRevealScrollTop(8000, 40, 200, 600, 36, 108)).toBe(4)
  })

  it('主体高于视口(区间为空)→ 对齐顶部而非底部', () => {
    // 文件夹区(index=2):topInset=108 bottomInset=36;主体高 100000 远超视口
    // alignTop = 2000-108 = 1892;无论 cur 在上/下都取 alignTop
    expect(clampRevealScrollTop(0, 2000, 100_000, 600, 108, 36)).toBe(1892)
    expect(clampRevealScrollTop(50_000, 2000, 100_000, 600, 108, 36)).toBe(1892)
  })

  it('主体恰好填满可用区 → 区间单点,任意 cur 都收敛到该点', () => {
    // viewportH=600 topInset=72 bottomInset=72 → 可用高 456;bodyH=456
    // alignTop = alignBottom = 1000-72 = 928
    expect(clampRevealScrollTop(0, 1000, 456, 600, 72, 72)).toBe(928)
    expect(clampRevealScrollTop(5000, 1000, 456, 600, 72, 72)).toBe(928)
  })

  it('零 inset 退化为普通 nearest 语义', () => {
    // 主体 [300, 500),视口 600:cur=400(主体顶在视口内)→ 不动? 完全可见区间 [−100, 300]
    expect(clampRevealScrollTop(400, 300, 200, 600, 0, 0)).toBe(300) // 上滚露出顶
    expect(clampRevealScrollTop(100, 300, 200, 600, 0, 0)).toBe(100) // 已可见不动
  })
})

// 「点击意图化」判据:主体是否已被粘性堆叠挤出可用带(交集 ≤ eps)。场景基准同上:headerH=36、4 区块。
describe('bodyRevealNeeded', () => {
  it('图库(index=0)被推出顶部 → 主体全在带上方 → 需揭示', () => {
    // topInset=36 bottomInset=108;主体在 40 处高 200,cur=8000
    // 可用带 [8036, 8000+600-108=8492];主体 [40,240] 与带无交 → 揭示
    expect(bodyRevealNeeded(8000, 40, 200, 600, 36, 108)).toBe(true)
  })

  it('管理(index=3,末位)被推出底部 → 主体全在带下方 → 需揭示', () => {
    // topInset=144 bottomInset=0;主体在 5000 处高 120,cur=1000
    // 可用带 [1144, 1600];主体 [5000,5120] 远在带下 → 揭示
    expect(bodyRevealNeeded(1000, 5000, 120, 600, 144, 0)).toBe(true)
  })

  it('主体完全可见 → 不揭示(照常折叠)', () => {
    // 可用带 [82, 538];主体 [100,300] 完全在带内 → 不揭示
    expect(bodyRevealNeeded(10, 100, 200, 600, 72, 72)).toBe(false)
  })

  it('超高主体(文件夹)滚进树时始终与带相交 → 不误判为揭示', () => {
    // index=2:topInset=108 bottomInset=36;主体在 2000 处高 100000,cur=50000
    // 可用带 [50108, 50564] 落在主体 [2000,102000] 内 → 交集满带 → 不揭示,折叠照常生效
    expect(bodyRevealNeeded(50000, 2000, 100_000, 600, 108, 36)).toBe(false)
  })

  it('主体仅露一条(> eps)→ 仍算可见,不揭示', () => {
    // 零 inset,视口 600;主体顶恰在视口底上方 5px:bodyFlowTop=595 bodyH=200 cur=0
    // 可用带 [0,600];交集 [595,600]=5 > eps(1) → 不揭示
    expect(bodyRevealNeeded(0, 595, 200, 600, 0, 0)).toBe(false)
  })

  it('主体几乎贴着带边(交集 ≤ eps)→ 视为不可见 → 揭示', () => {
    // 主体顶恰在视口底:bodyFlowTop=600 bodyH=200 cur=0;交集 0 → 揭示
    expect(bodyRevealNeeded(0, 600, 200, 600, 0, 0)).toBe(true)
  })

  it('空主体(bodyH=0)→ 不揭示,交给 toggle', () => {
    expect(bodyRevealNeeded(0, 100, 0, 600, 36, 36)).toBe(false)
  })
})
