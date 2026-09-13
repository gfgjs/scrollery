import { describe, expect, it } from 'vitest'
import { summarizeTreeCategories } from './treeCategoryMenu.helpers'

describe('treeCategoryMenu helpers', () => {
  it('五类全选是默认的无筛选摘要', () => {
    expect(summarizeTreeCategories(['image', 'video', 'document', 'audio', 'other'])).toEqual({
      kind: 'all',
      count: 5,
    })
  })

  it('空选集合明确表示只保留目录', () => {
    expect(summarizeTreeCategories([])).toEqual({ kind: 'none', count: 0 })
  })

  it('单选与任意多选分别提供可访问摘要所需的数据', () => {
    expect(summarizeTreeCategories(['audio'])).toEqual({
      kind: 'single',
      category: 'audio',
      count: 1,
    })
    expect(summarizeTreeCategories(['audio', 'other', 'video'])).toEqual({
      kind: 'some',
      count: 3,
    })
  })

  it('单独选择其它时保持文件树专用语义', () => {
    expect(summarizeTreeCategories(['other'])).toEqual({
      kind: 'other',
      category: 'other',
      count: 1,
    })
  })
})
