// filterStore 单测（S 线 P3）。重点锁两件事：
//  1. `apiFilterKey` —— 画廊重算 watch 的**唯一**筛选源。它必须「该变的时候变、不该变的时候不变」，
//     漏一个维度的症状是**点了 chip 画廊不更新**（colorLabel 已这样漏过一次，T16 遗留）。
//  2. `fileFormats` 维度的语义（空=不限、清除筛选一并清空、hasActiveFilters 纳入）。

import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useFilterStore } from './filterStore'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('fileFormats 维度', () => {
  it('默认空 = 该维度不限：toApiFilter 不下发该字段', () => {
    const f = useFilterStore()
    expect(f.fileFormats).toEqual([])
    expect(f.toApiFilter().fileFormats).toBeUndefined()
  })

  it('toggle 增删；重复 toggle 回到原态', () => {
    const f = useFilterStore()
    f.toggleFileFormat('png')
    expect(f.fileFormats).toEqual(['png'])
    f.toggleFileFormat('jpg')
    expect(f.fileFormats).toEqual(['png', 'jpg'])
    f.toggleFileFormat('png')
    expect(f.fileFormats).toEqual(['jpg'])
  })

  it('toggle 做引用替换（apiFilterKey 与画廊 watch 依赖它重算）', () => {
    const f = useFilterStore()
    const before = f.fileFormats
    f.toggleFileFormat('png')
    expect(f.fileFormats).not.toBe(before)
  })

  it('纳入 hasActiveFilters（否则「清除筛选」chip 不出现，格式筛选清不掉）', () => {
    const f = useFilterStore()
    expect(f.hasActiveFilters).toBe(false)
    f.toggleFileFormat('png')
    expect(f.hasActiveFilters).toBe(true)
  })

  /** D-011：清除筛选同时清空媒体大类与细分格式。 */
  it('clearFilters 一并清空格式', () => {
    const f = useFilterStore()
    f.toggleMediaType('image')
    f.toggleFileFormat('png')
    f.clearFilters()
    expect(f.fileFormats).toEqual([])
    expect(f.mediaTypes).toEqual([])
    expect(f.hasActiveFilters).toBe(false)
  })

  it('非空时 toApiFilter 下发具体扩展名（group 是 UI 概念，不进 API）', () => {
    const f = useFilterStore()
    f.setFileFormats(['jpg', 'jpeg'])
    expect(f.toApiFilter().fileFormats).toEqual(['jpg', 'jpeg'])
  })
})

// ── apiFilterKey：画廊重算 watch 的唯一筛选源 ──────────────────────────────────
//
// 断言的是「键变了/没变」而非键等于某个具体字符串：它只用于 watch 比对，值本身无语义。
// 钉具体值就是把实现细节焊死 —— 改个字段名就红，却什么 bug 也抓不到。

describe('apiFilterKey 随每个下发维度变化', () => {
  const mutations: Array<[string, (f: ReturnType<typeof useFilterStore>) => void]> = [
    ['媒体大类', (f) => f.toggleMediaType('image')],
    // 🔴 本条是 P3 新增维度：漏了就是「点格式 chip 画廊不更新」。
    ['细分格式', (f) => f.toggleFileFormat('png')],
    ['收藏', (f) => (f.favoritedOnly = true)],
    ['Live', (f) => (f.livePhotoOnly = true)],
    ['评分', (f) => (f.minRating = 3)],
    // colorLabel 曾漏入画廊 watch（T16 遗留）→ 切色不重算。收敛为单键后由本条守住。
    ['颜色标签', (f) => (f.colorLabel = 2)],
  ]
  for (const [name, mutate] of mutations) {
    it(name, () => {
      const f = useFilterStore()
      const before = f.apiFilterKey
      mutate(f)
      expect(f.apiFilterKey).not.toBe(before)
    })
  }

  it('日期两端皆备时变化（谓词此时才真正下发）', () => {
    const f = useFilterStore()
    f.dateFrom = 1_700_000_000
    const oneEnd = f.apiFilterKey
    f.dateTo = 1_700_086_400
    expect(f.apiFilterKey).not.toBe(oneEnd)
  })

  /**
   * 只填一端**不**改键 —— 这是 P3 收敛 watch 时**有意的**行为变化，故显式钉住而非留作巧合。
   *
   * 理由：`toApiFilter()` 在两端皆备前不下发 `dateRange`，故此时重算跑出的筛选与上次逐字节
   * 相同 = 可证明的空转。原先手工枚举 `() => filter.dateFrom` 会触发那次空转。
   */
  it('只填日期一端不改键（那次重算是可证明的空转）', () => {
    const f = useFilterStore()
    const before = f.apiFilterKey
    f.dateFrom = 1_700_000_000
    expect(f.apiFilterKey).toBe(before)
  })

  /**
   * 反面：与下发内容**无关**的变动不得改键，否则每次都白跑一次全量重算（百万级库上是真实代价）。
   * 评分 3→5 会改键（值不同），但 0→0、空数组→空数组这类「没实际变化」不该改。
   */
  it('赋同值不改键（不白跑重算）', () => {
    const f = useFilterStore()
    f.setFileFormats(['png'])
    const before = f.apiFilterKey
    f.setFileFormats(['png'])
    expect(f.apiFilterKey).toBe(before)
  })
})
