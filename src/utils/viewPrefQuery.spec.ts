// viewPrefQuery 纯函数单测(S2-b2)：view-pref 的 URL 编解码。
// 重点锁定「URL 权威、缺失键保持 persist」的 Partial 语义(区别于 filter 的缺失回落默认)+ 防御式白名单。

import { describe, it, expect } from 'vitest'
import { encodeViewPref, decodeViewPref, VIEW_PREF_DEFAULTS } from './viewPrefQuery'
import type { LocationQuery } from 'vue-router'

describe('viewPrefQuery（S2-b2 view-pref URL 编解码）', () => {
  it('encode：全默认 → 空对象(默认视图对应干净 URL)', () => {
    expect(encodeViewPref(VIEW_PREF_DEFAULTS)).toEqual({})
  })

  it('encode：仅输出非默认字段', () => {
    expect(
      encodeViewPref({
        groupBy: 'folder',
        sortWithinGroup: 'filename',
        sortOrder: 'asc',
        layoutMode: 'grid',
      }),
    ).toEqual({ group: 'folder', sort: 'filename', order: 'asc', layout: 'grid' })
  })

  it('encode：部分非默认只出对应键', () => {
    expect(
      encodeViewPref({ ...VIEW_PREF_DEFAULTS, groupBy: 'none' }),
    ).toEqual({ group: 'none' })
  })

  it('decode：缺失键不出现在结果(保持 persist 不覆盖)——空 query → 空 Partial', () => {
    expect(decodeViewPref({})).toEqual({})
  })

  it('decode：仅出现的合法键进结果', () => {
    const q: LocationQuery = { group: 'folder', order: 'asc' }
    expect(decodeViewPref(q)).toEqual({ groupBy: 'folder', sortOrder: 'asc' })
  })

  it('decode：全键合法 → 全量 Partial', () => {
    const q: LocationQuery = {
      group: 'none',
      sort: 'similarity',
      order: 'desc',
      layout: 'grid',
    }
    expect(decodeViewPref(q)).toEqual({
      groupBy: 'none',
      sortWithinGroup: 'similarity',
      sortOrder: 'desc',
      layoutMode: 'grid',
    })
  })

  it('decode：防御式——非法值当作未提供而丢弃(回落 persist)', () => {
    const q: LocationQuery = {
      group: 'weekly', // 非法
      sort: 'random', // 非法
      order: 'ASC', // 大小写不匹配 → 非法
      layout: 'masonry', // 非法
    }
    expect(decodeViewPref(q)).toEqual({})
  })

  it('decode：数组值取首(vue-router 重复键)', () => {
    const q: LocationQuery = { group: ['folder', 'none'] }
    expect(decodeViewPref(q)).toEqual({ groupBy: 'folder' })
  })

  it('往返：encode(非默认) → decode 还原同值', () => {
    const snap = {
      groupBy: 'folder' as const,
      sortWithinGroup: 'filename' as const,
      sortOrder: 'asc' as const,
      layoutMode: 'grid' as const,
    }
    const round = decodeViewPref(encodeViewPref(snap) as LocationQuery)
    expect(round).toEqual(snap)
  })
})
