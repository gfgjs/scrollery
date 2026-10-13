import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useFilterStore } from './filterStore'
import { decodeGalleryFilters } from '../utils/galleryQuery'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('日期筛选的零时间戳边界', () => {
  it('从 URL 恢复零起点时保留日期谓词，切换终点会更新查询键，清空后取消筛选', () => {
    const filter = useFilterStore()
    const restored = decodeGalleryFilters({ from: '0', to: '86400' })
    filter.dateFrom = restored.dateFrom
    filter.dateTo = restored.dateTo

    expect(filter.hasActiveFilters).toBe(true)
    expect(filter.toApiFilter().dateRange).toEqual({ from: 0, to: 86400 })
    const previousKey = filter.apiFilterKey
    filter.dateTo = 172800
    expect(filter.apiFilterKey).not.toBe(previousKey)
    expect(JSON.parse(filter.apiFilterKey)).toMatchObject({
      dateRange: { from: 0, to: 172800 },
    })

    filter.clearFilters()
    expect(filter.hasActiveFilters).toBe(false)
    expect(filter.toApiFilter().dateRange).toBeUndefined()
  })

  it('零终点也属于已填写边界，仅缺少一端时不下发日期谓词', () => {
    const filter = useFilterStore()
    filter.dateTo = 0
    expect(filter.hasActiveFilters).toBe(false)
    expect(filter.toApiFilter().dateRange).toBeUndefined()

    filter.dateFrom = 0
    expect(filter.hasActiveFilters).toBe(true)
    expect(filter.toApiFilter().dateRange).toEqual({ from: 0, to: 0 })

    filter.dateTo = null
    expect(filter.hasActiveFilters).toBe(false)
    expect(filter.toApiFilter().dateRange).toBeUndefined()
  })
})
