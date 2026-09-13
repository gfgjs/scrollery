import { describe, it, expect, vi } from 'vitest'
import {
  maxBucketCount,
  densityBarWidth,
  findActiveMonthIndex,
  isYearBoundary,
  fractionToMonthIndex,
  nearestSeparatorIndex,
  nearestSeparatorWindow,
  downsampleSeparators,
  buildRowIntensity,
  buildTimeBand,
  logicalYToTimeFrac,
  visibleYearLabelSet,
  stepScrubberIndex,
} from './timelineScrubber.helpers'

// TimelineScrubber 纯映射逻辑回归（Part5 §3.3）。scrubber 视觉/手感无法在 node 环境验证，
// 但其映射数学（index↔y、密度归一化、边界）可测 —— 本 spec 锁定这些最易 off-by-one 的点。

describe('maxBucketCount', () => {
  it('空桶返回 1（防除零的种子值）', () => {
    expect(maxBucketCount([])).toBe(1)
  })
  it('全为 0 时仍返回 1（种子兜底）', () => {
    expect(maxBucketCount([{ count: 0 }, { count: 0 }])).toBe(1)
  })
  it('取最大项数', () => {
    expect(maxBucketCount([{ count: 3 }, { count: 17 }, { count: 5 }])).toBe(17)
  })
})

describe('densityBarWidth', () => {
  it('count=0 → 保底 12%', () => {
    expect(densityBarWidth(0, 100)).toBe(12)
  })
  it('count=maxCount → 满铺 100%', () => {
    expect(densityBarWidth(50, 50)).toBe(100)
  })
  it('半值 → 12 + 50%*88 = 56%', () => {
    expect(densityBarWidth(50, 100)).toBeCloseTo(56)
  })
  it('maxCount=0 异常输入按 1 处理不崩（除零防护）', () => {
    expect(densityBarWidth(0, 0)).toBe(12)
  })
})

describe('findActiveMonthIndex', () => {
  // 三个月：[y=0, y=100, y=300)，末月上界 +∞。
  const buckets = [{ y: 0 }, { y: 100 }, { y: 300 }]

  it('空桶返回 -1', () => {
    expect(findActiveMonthIndex([], 50)).toBe(-1)
  })
  it('y 在首月区间 → 0', () => {
    expect(findActiveMonthIndex(buckets, 50)).toBe(0)
  })
  it('y 落在区间下边界（含左闭）→ 命中该月', () => {
    expect(findActiveMonthIndex(buckets, 100)).toBe(1)
  })
  it('y 在中间月区间内 → 该月', () => {
    expect(findActiveMonthIndex(buckets, 250)).toBe(1)
  })
  it('y 落入末月（+∞ 上界，再大也命中）→ 末月', () => {
    expect(findActiveMonthIndex(buckets, 99999)).toBe(2)
    expect(findActiveMonthIndex(buckets, 300)).toBe(2)
  })
  it('y 在首月之前（未命中任何区间）兜底 → 0', () => {
    expect(findActiveMonthIndex([{ y: 100 }, { y: 200 }], 50)).toBe(0)
  })
})

describe('isYearBoundary', () => {
  // 最新→最旧：2025-03, 2025-02, 2024-12, 2024-11
  const buckets = [{ year: 2025 }, { year: 2025 }, { year: 2024 }, { year: 2024 }]

  it('i=0 恒为年首月', () => {
    expect(isYearBoundary(buckets, 0)).toBe(true)
  })
  it('同年内部不是年边界', () => {
    expect(isYearBoundary(buckets, 1)).toBe(false)
  })
  it('年份变化处是年边界', () => {
    expect(isYearBoundary(buckets, 2)).toBe(true)
  })
  it('下一年的内部月不是年边界', () => {
    expect(isYearBoundary(buckets, 3)).toBe(false)
  })
})

describe('fractionToMonthIndex', () => {
  it('frac=0 → 第 0 月', () => {
    expect(fractionToMonthIndex(0, 12)).toBe(0)
  })
  it('frac=1 → clamp 到 n-1（不越界到 n）', () => {
    expect(fractionToMonthIndex(1, 12)).toBe(11)
  })
  it('frac=0.5，12 月 → floor(6)=6', () => {
    expect(fractionToMonthIndex(0.5, 12)).toBe(6)
  })
  it('monthCount<=0（无月）→ 0', () => {
    expect(fractionToMonthIndex(0.5, 0)).toBe(0)
  })
})

describe('nearestSeparatorIndex', () => {
  // 分组边界按逻辑 y 升序：y = 0 / 100 / 300 / 600，totalHeight = 600。
  const seps = [{ y: 0 }, { y: 100 }, { y: 300 }, { y: 600 }]

  it('空数组返回 -1（调用方回退按比例）', () => {
    expect(nearestSeparatorIndex([], 0.5, 600)).toBe(-1)
  })
  it('frac=0 → 首个分组', () => {
    expect(nearestSeparatorIndex(seps, 0, 600)).toBe(0)
  })
  it('frac=1 → 末个分组（不越界）', () => {
    expect(nearestSeparatorIndex(seps, 1, 600)).toBe(3)
  })
  it('targetY=160 → 吸附到更近的 y=100（索引 1），非 y=300', () => {
    // 160 距 100 为 60、距 300 为 140 → 取 100
    expect(nearestSeparatorIndex(seps, 160 / 600, 600)).toBe(1)
  })
  it('正中间（等距）偏向更靠前者：targetY=200 → 索引 1', () => {
    expect(nearestSeparatorIndex(seps, 200 / 600, 600)).toBe(1)
  })
  it('targetY 超过末分组 → clamp 到末个', () => {
    // totalHeight=1000 使 frac=1 的 targetY=1000 > 末分组 y=600
    expect(nearestSeparatorIndex(seps, 1, 1000)).toBe(3)
  })
  it('targetY 在首分组之前 → clamp 到首个', () => {
    expect(nearestSeparatorIndex([{ y: 100 }, { y: 200 }], 0, 400)).toBe(0)
  })
})

describe('visibleYearLabelSet', () => {
  it('空桶 / 无高度 → 空集', () => {
    expect(visibleYearLabelSet([], 1000, 800).size).toBe(0)
    expect(visibleYearLabelSet([{ y: 0, year: 2026 }], 1000, 0).size).toBe(0)
  })

  it('年份边界间距足够 → 全部显示', () => {
    // 三年各在 y=0/500/1000,totalHeight=1000,trackH=800 → 像素 0/400/... 间距远超 12
    const b = [
      { y: 0, year: 2026 },
      { y: 500, year: 2025 },
      { y: 999, year: 2024 },
    ]
    expect([...visibleYearLabelSet(b, 1000, 800)]).toEqual([0, 1, 2])
  })

  it('稀疏相邻年份像素挤叠 → 每簇只留最靠上一个', () => {
    // 2026 在 y=0;2025/2024 挤在 y=2/4(内容极少),totalHeight=10000,trackH=800
    // 像素:0 / 0.16 / 0.32 → 后两个距 0 不足 12px,被抑制
    const b = [
      { y: 0, year: 2026 },
      { y: 2, year: 2025 },
      { y: 4, year: 2024 },
      { y: 5000, year: 2023 }, // 远处 → 显示
    ]
    expect([...visibleYearLabelSet(b, 10000, 800)]).toEqual([0, 3])
  })

  it('非年份边界的月不计入', () => {
    const b = [
      { y: 0, year: 2026 }, // 边界
      { y: 400, year: 2026 }, // 同年,非边界
      { y: 800, year: 2025 }, // 边界
    ]
    expect([...visibleYearLabelSet(b, 1000, 800)]).toEqual([0, 2])
  })
})

describe('nearestSeparatorWindow', () => {
  // 10 个分隔符,y=0,100,...,900,totalHeight=1000
  const seps = Array.from({ length: 10 }, (_, i) => ({ y: i * 100 }))

  it('空数组 / k<=0 → 空窗', () => {
    expect(nearestSeparatorWindow([], 0.5, 1000, 9)).toEqual({ start: 0, end: 0, center: -1 })
    expect(nearestSeparatorWindow(seps, 0.5, 1000, 0)).toEqual({ start: 0, end: 0, center: -1 })
  })

  it('中部:以最近项为中心两侧各扩 ⌊K/2⌋', () => {
    // frac=0.5 → targetY=500 → center=5;K=5 → start=3,end=8
    expect(nearestSeparatorWindow(seps, 0.5, 1000, 5)).toEqual({ start: 3, end: 8, center: 5 })
  })

  it('近首:窗口贴顶补足 K 个(不越界到负)', () => {
    // frac=0 → center=0;K=5 → start=0,end=5
    expect(nearestSeparatorWindow(seps, 0, 1000, 5)).toEqual({ start: 0, end: 5, center: 0 })
  })

  it('近尾:窗口贴底整体上移补足 K 个', () => {
    // frac=1 → center=9;K=5 → end=10,start=5
    expect(nearestSeparatorWindow(seps, 1, 1000, 5)).toEqual({ start: 5, end: 10, center: 9 })
  })

  it('K 大于总数 → 全取,不越界', () => {
    const w = nearestSeparatorWindow(seps, 0.5, 1000, 99)
    expect(w.start).toBe(0)
    expect(w.end).toBe(10)
  })
})

describe('stepScrubberIndex', () => {
  it('count<=0 → null(无项可导航)', () => {
    expect(stepScrubberIndex(0, 'ArrowDown', 0)).toBeNull()
  })
  it('非导航键 → null', () => {
    expect(stepScrubberIndex(3, 'a', 10)).toBeNull()
    expect(stepScrubberIndex(3, 'Enter', 10)).toBeNull()
  })
  it('ArrowDown 索引 +1(轨道下方=更旧)', () => {
    expect(stepScrubberIndex(3, 'ArrowDown', 10)).toBe(4)
  })
  it('ArrowUp 索引 -1', () => {
    expect(stepScrubberIndex(3, 'ArrowUp', 10)).toBe(2)
  })
  it('边界 clamp:末项 ArrowDown 不越界 / 首项 ArrowUp 不越界', () => {
    expect(stepScrubberIndex(9, 'ArrowDown', 10)).toBe(9)
    expect(stepScrubberIndex(0, 'ArrowUp', 10)).toBe(0)
  })
  it('Home/End 到首/尾', () => {
    expect(stepScrubberIndex(5, 'Home', 10)).toBe(0)
    expect(stepScrubberIndex(5, 'End', 10)).toBe(9)
  })
  it('PageDown/PageUp 跨 page(默认 10),越界 clamp', () => {
    expect(stepScrubberIndex(2, 'PageDown', 100)).toBe(12)
    expect(stepScrubberIndex(2, 'PageUp', 100)).toBe(0)
    expect(stepScrubberIndex(50, 'PageUp', 100, 20)).toBe(30)
  })
  it('cur<0(未定位)以 0 为基', () => {
    expect(stepScrubberIndex(-1, 'ArrowDown', 10)).toBe(1)
    expect(stepScrubberIndex(-1, 'ArrowUp', 10)).toBe(0)
  })
})

describe('downsampleSeparators', () => {
  it('分隔符数 <= 槽数 → 原样返回（小库零改动）', () => {
    const seps = [{ y: 0 }, { y: 50 }, { y: 100 }]
    const out = downsampleSeparators(seps, 100, 10)
    expect(out).toHaveLength(3)
    expect(out.map((s) => s.y)).toEqual([0, 50, 100])
  })

  it('分隔符数 > 槽数 → 每槽取首个代表，长度不超过槽数', () => {
    // y = 0,10,20,...,990（100 个），totalHeight=1000，10 槽 → 每槽跨 100 逻辑高
    const seps = Array.from({ length: 100 }, (_, i) => ({ y: i * 10 }))
    const out = downsampleSeparators(seps, 1000, 10)
    expect(out).toHaveLength(10)
    // 每槽首个：y = 0,100,200,...,900
    expect(out.map((s) => s.y)).toEqual([0, 100, 200, 300, 400, 500, 600, 700, 800, 900])
  })

  it('保留 label 等其余字段（取真实分隔符对象非合成）', () => {
    const seps = Array.from({ length: 20 }, (_, i) => ({ y: i * 10, label: 'g' + i }))
    const out = downsampleSeparators(seps, 200, 5)
    expect(out.length).toBeLessThanOrEqual(5)
    expect(out[0]).toMatchObject({ y: 0, label: 'g0' })
  })

  it('稀疏区留白：分隔符集中在前 30% → 后段槽无代表，输出短于槽数', () => {
    // 30 个分隔符全在 [0,290]，totalHeight=1000，10 槽（每槽跨 100）→ 仅前 3 槽有代表
    const seps = Array.from({ length: 30 }, (_, i) => ({ y: i * 10 }))
    const out = downsampleSeparators(seps, 1000, 10)
    expect(out).toHaveLength(3)
    expect(out.length).toBeLessThan(10)
  })

  it('totalHeight<=0 异常输入不崩（按 1 处理）', () => {
    const seps = Array.from({ length: 5 }, (_, i) => ({ y: i }))
    expect(() => downsampleSeparators(seps, 0, 3)).not.toThrow()
  })
})

describe('buildRowIntensity', () => {
  it('空 separators → 全零数组，长度 = trackH', () => {
    const out = buildRowIntensity([], 1000, 8)
    expect(out).toHaveLength(8)
    expect(Array.from(out).every((v) => v === 0)).toBe(true)
  })

  it('trackH<=0 → 空数组（无行可画）', () => {
    expect(buildRowIntensity([{ y: 0, count: 5 }], 1000, 0)).toHaveLength(0)
    expect(buildRowIntensity([{ y: 0, count: 5 }], 1000, -3)).toHaveLength(0)
  })

  it('单个分隔符覆盖全高 → 归一化后全为 1', () => {
    const out = buildRowIntensity([{ y: 0, count: 42 }], 1000, 4)
    expect(Array.from(out)).toEqual([1, 1, 1, 1])
  })

  it('归一化:最热行 = 1,其余按比例(一日跨多行按 count 铺满)', () => {
    // 两分隔符各占一半高度:前半 count=10、后半 count=20 → 归一化后前半 0.5、后半 1。
    const out = buildRowIntensity(
      [
        { y: 0, count: 10 },
        { y: 500, count: 20 },
      ],
      1000,
      4,
    )
    expect(Array.from(out)).toEqual([0.5, 0.5, 1, 1])
  })

  it('多分隔符映射同一行 → 取覆盖分隔符的平均项数(非求和)', () => {
    // trackH=2(行跨 500)。sep0[0,100)c10、sep1[100,600)c20(跨行0与1)、sep2[600,1000)c30。
    // 行0 覆盖 {sep0,sep1} → (10+20)/2=15;行1 覆盖 {sep1,sep2} → (20+30)/2=25。归一化 → 0.6, 1。
    const seps = [
      { y: 0, count: 10 },
      { y: 100, count: 20 },
      { y: 600, count: 30 },
    ]
    const out = buildRowIntensity(seps, 1000, 2)
    expect(out[0]).toBeCloseTo(0.6, 5)
    expect(out[1]).toBeCloseTo(1, 5)
  })

  it('totalHeight<=0 异常输入不崩(按 1 处理,不越界)', () => {
    expect(() => buildRowIntensity([{ y: 0, count: 3 }], 0, 5)).not.toThrow()
    const out = buildRowIntensity([{ y: 0, count: 3 }], 0, 5)
    expect(out).toHaveLength(5)
  })
})

describe('buildTimeBand', () => {
  it('trackH<=0 → 空 intensity/rowJumpY/ticks', () => {
    const r = buildTimeBand([{ y: 0, count: 5, epochDay: 19802 }], 1000, 0)
    expect(r.intensity).toHaveLength(0)
    expect(r.rowJumpY).toHaveLength(0)
    expect(r.monthTicks).toEqual([])
    expect(r.yearTicks).toEqual([])
  })

  it('folder/none 全 null epochDay → intensity 全零 + rowJumpY 线性回退 + 无刻度', () => {
    const r = buildTimeBand(
      [
        { y: 0, count: 5, epochDay: null },
        { y: 100, count: 5, epochDay: null },
      ],
      1000,
      4,
    )
    expect(Array.from(r.intensity)).toEqual([0, 0, 0, 0])
    // 线性:(r/rows)*H
    expect(Array.from(r.rowJumpY)).toEqual([0, 250, 500, 750])
    expect(r.monthTicks).toEqual([])
    expect(r.yearTicks).toEqual([])
  })

  it('单日 → 全落 row 0(归一化 [1,0,0,0]),rowJumpY 恒为该日 y', () => {
    const r = buildTimeBand([{ y: 0, count: 7, epochDay: 19802 }], 1000, 4)
    expect(Array.from(r.intensity)).toEqual([1, 0, 0, 0])
    expect(Array.from(r.rowJumpY)).toEqual([0, 0, 0, 0])
  })

  it('两相邻日:顶=最新(大 epochDay→row0)、底=最旧,rowJumpY 取各日真实 y', () => {
    // 19802(y=0,c=10) 更新 → row0;19801(y=200,c=20) 更旧 → row1。归一化 max=20 → [0.5,1]。
    const r = buildTimeBand(
      [
        { y: 0, count: 10, epochDay: 19802 },
        { y: 200, count: 20, epochDay: 19801 },
      ],
      1000,
      2,
    )
    expect(Array.from(r.intensity)).toEqual([0.5, 1])
    expect(Array.from(r.rowJumpY)).toEqual([0, 200])
  })

  it('日历空隙:空日行 intensity=0,rowJumpY 在相邻真实日间线性插值(time 坐标命门)', () => {
    // 19802(y=0,c=10) 与 19792(y=300,c=30) 差 10 天;rows=11 → 每天 1 行。
    // rowOf(19802)=0、rowOf(19792)=10;中间 1..9 行无真实日 = 空隙。
    const r = buildTimeBand(
      [
        { y: 0, count: 10, epochDay: 19802 },
        { y: 300, count: 30, epochDay: 19792 },
      ],
      1000,
      11,
    )
    // 归一化 max=30:row0=10/30、row10=1,空隙行全 0。
    expect(r.intensity[0]).toBeCloseTo(1 / 3, 5)
    expect(r.intensity[10]).toBeCloseTo(1, 5)
    expect(r.intensity[5]).toBe(0)
    // rowJumpY:row0=0(最新日),row10=300(最旧日),空隙 row5 线性插值 = 0 + 0.5*300 = 150。
    expect(r.rowJumpY[0]).toBeCloseTo(0, 5)
    expect(r.rowJumpY[10]).toBeCloseTo(300, 5)
    expect(r.rowJumpY[5]).toBeCloseTo(150, 5)
  })

  it('ASC(旧→新)排序:朝向自适应,顶=最旧,rowJumpY 单调不减(评审 R1 回归)', () => {
    // y 升序对应 epochDay 升序(用户点了工具栏「升序」):最旧日在网格顶(y=0)。
    // 旧实现硬钉「顶=最新」→ rowJumpY=[200,100,0] 反向单调,击穿 logicalYToTimeFrac 二分。
    const r = buildTimeBand(
      [
        { y: 0, count: 10, epochDay: 100 },
        { y: 100, count: 20, epochDay: 101 },
        { y: 200, count: 30, epochDay: 102 },
      ],
      1000,
      3,
    )
    expect(Array.from(r.rowJumpY)).toEqual([0, 100, 200]) // 顶=最旧(y=0),与网格同向
    expect(r.intensity[0]).toBeCloseTo(1 / 3, 5)
    expect(r.intensity[1]).toBeCloseTo(2 / 3, 5)
    expect(r.intensity[2]).toBeCloseTo(1, 5)
    // 逆映射随之正确:y=50 → 首个 >=50 的行 1 → 1/2。
    expect(logicalYToTimeFrac(r.rowJumpY, 50)).toBe(0.5)
  })

  it('ASC 下日历刻度行随朝向翻转(月初行按 旧→新 自顶向下)', () => {
    // 与 DESC 日历刻度用例同一批日期,倒转为 ASC:19706(2023-12-15)最旧在顶。
    // rowOf(d) = d - 19706:2024-01-01(19723)→row17、2024-02-01(19754)→row48。
    const r = buildTimeBand(
      [
        { y: 0, count: 5, epochDay: 19706 },
        { y: 100, count: 5, epochDay: 19737 },
        { y: 200, count: 5, epochDay: 19768 },
      ],
      1000,
      63,
    )
    expect(r.monthTicks).toEqual([17, 48])
    expect(r.yearTicks.some((yt) => yt.year === 2024 && yt.row === 17)).toBe(true)
  })

  it('大库常态(天数 > 行数):同行多日去重取该行最靠上一日,rowJumpY 仍单调(此前零覆盖)', () => {
    // 5 天 DESC 压进 3 行:rowOf = round((104-d)/4*2) → d104→0、d103/102→1、d101/100→2。
    const r = buildTimeBand(
      [
        { y: 0, count: 1, epochDay: 104 },
        { y: 10, count: 1, epochDay: 103 },
        { y: 20, count: 1, epochDay: 102 },
        { y: 30, count: 1, epochDay: 101 },
        { y: 40, count: 1, epochDay: 100 },
      ],
      1000,
      3,
    )
    // 每行锚点 = 该行最小 y:row0=0、row1=min(10,20)=10、row2=min(30,40)=30。
    expect(Array.from(r.rowJumpY)).toEqual([0, 10, 30])
    // intensity 按行求和:行0=1、行1=2、行2=2 → 归一化 [0.5, 1, 1]。
    expect(r.intensity[0]).toBeCloseTo(0.5, 5)
    expect(r.intensity[1]).toBeCloseTo(1, 5)
    expect(r.intensity[2]).toBeCloseTo(1, 5)
  })

  it('round-trip:rowJumpY 严格递增时 logicalYToTimeFrac 是其逐行真逆', () => {
    // 空日插值用例的 rowJumpY = [0,30,60,...,300](严格递增)。
    const r = buildTimeBand(
      [
        { y: 0, count: 10, epochDay: 19802 },
        { y: 300, count: 30, epochDay: 19792 },
      ],
      1000,
      11,
    )
    for (let row = 0; row < 11; row++) {
      expect(logicalYToTimeFrac(r.rowJumpY, r.rowJumpY[row])).toBeCloseTo(row / 10, 6)
    }
  })

  it('同日多分隔符(span=0):坍缩为单锚点取最小 y,rowJumpY 恒定不越界', () => {
    const r = buildTimeBand(
      [
        { y: 0, count: 3, epochDay: 19802 },
        { y: 50, count: 4, epochDay: 19802 },
      ],
      1000,
      4,
    )
    expect(Array.from(r.rowJumpY)).toEqual([0, 0, 0, 0])
    expect(r.intensity[0]).toBe(1) // 3+4 同行求和后归一化
    expect(r.monthTicks).toEqual([]) // span=0 不产日历刻度
  })

  it('intensity 与 rowJumpY 长度均 = trackH', () => {
    const r = buildTimeBand([{ y: 0, count: 3, epochDay: 19800 }], 1000, 9)
    expect(r.intensity).toHaveLength(9)
    expect(r.rowJumpY).toHaveLength(9)
  })

  it('日历刻度:跨 2023-12→2024-02 → monthTicks 三个月、yearTicks 含 2024(顶部靠上)', () => {
    // 2023-12-15 = epochDay 19706、2024-01-15 = 19737、2024-02-15 = 19768(闰年)。
    // 月初刻度:2024-01-01(19723)、2024-02-01(19754);2023-12-01(19692) < firstDay 不计。
    // 顶=最新(19768,row0),底=最旧(19706,row 大)。rows 取大值使刻度可分辨。
    const r = buildTimeBand(
      [
        { y: 0, count: 5, epochDay: 19768 }, // 2024-02-15 最新
        { y: 100, count: 5, epochDay: 19737 }, // 2024-01-15
        { y: 200, count: 5, epochDay: 19706 }, // 2023-12-15 最旧
      ],
      1000,
      63, // span=62 天 → 每天约 1 行,月初可分辨
    )
    // 2024-01-01(row45)与 2024-02-01(row14)两个月初落在范围内(2023-12-01 早于 firstDay 不计)。
    expect(r.monthTicks).toEqual([45, 14])
    // yearTicks:首项 = 最旧年 2023 的锚(firstDay=19706 → row62,其 1/1 在域外,R4);
    // 随后 2024-01(mm===0)→ 年标 2024,行号 = 该月初行(与 monthTicks[0] 一致)。
    expect(r.yearTicks).toEqual([
      { row: 62, year: 2023 },
      { row: 45, year: 2024 },
    ])
  })

  it('单年库(不跨 1 月 1 日)也有年标:锚在最旧日行(评审 R4)', () => {
    // 2025-03-10(epochDay 20157)→ 2025-09-20(20351),域内无任何 1 月 1 日。
    const r = buildTimeBand(
      [
        { y: 0, count: 5, epochDay: 20351 }, // 最新在顶(DESC)
        { y: 100, count: 5, epochDay: 20157 },
      ],
      1000,
      50,
    )
    // 旧实现 yearTicks=[](用户失去全部年份锚);现锚在 firstDay 行(DESC → 底部 rows-1)。
    expect(r.yearTicks).toEqual([{ row: 49, year: 2025 }])
    expect(r.monthTicks.length).toBeGreaterThan(0) // 月刻度(4-9 月初)不受影响
  })

  it('跨度超 108 年(损坏 EXIF 离群日):刻度显式截断到最新端并告警,最旧年保锚(评审 R5)', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      // 1900-06-15(epochDay -25402)离群 + 2024-03-20(19802)真实照片;跨 1486 月 > CAP 1300。
      const r = buildTimeBand(
        [
          { y: 0, count: 5, epochDay: 19802 },
          { y: 100, count: 1, epochDay: -25402 },
        ],
        1000,
        200,
      )
      expect(warn).toHaveBeenCalledTimes(1)
      // 截断取最新端:2024(用户照片所在)刻度存活;截断起点 1915-12 → 首个域内年初为 1916。
      expect(r.yearTicks.some((t) => t.year === 2024)).toBe(true)
      expect(r.yearTicks.some((t) => t.year === 1916)).toBe(true)
      expect(r.yearTicks.some((t) => t.year === 1901)).toBe(false) // 中段远古年份被截掉
      // 最旧年 1900 保锚(firstDay 行 = DESC 底部)。
      expect(r.yearTicks[0]).toEqual({ row: 199, year: 1900 })
    } finally {
      warn.mockRestore()
    }
  })
})

describe('logicalYToTimeFrac', () => {
  it('空 / 单行 → 0', () => {
    expect(logicalYToTimeFrac(new Float32Array(0), 100)).toBe(0)
    expect(logicalYToTimeFrac([50], 100)).toBe(0)
  })

  it('单调 rowJumpY 上二分求行比例(逆映射,指示线定位)', () => {
    // rowJumpY = [0,100,200,300,400](5 行);y=200 → 第 2 行 → 2/4 = 0.5。
    const rj = [0, 100, 200, 300, 400]
    expect(logicalYToTimeFrac(rj, 0)).toBe(0)
    expect(logicalYToTimeFrac(rj, 200)).toBe(0.5)
    expect(logicalYToTimeFrac(rj, 400)).toBe(1)
    // y=150 落在 100 与 200 之间 → 第一个 >=150 的行是 index2 → 0.5。
    expect(logicalYToTimeFrac(rj, 150)).toBe(0.5)
    // 超出上界 → clamp 到末行。
    expect(logicalYToTimeFrac(rj, 999)).toBe(1)
  })
})
