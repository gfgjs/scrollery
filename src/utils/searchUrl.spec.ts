// searchUrl 纯函数单测(S2-b2 Stage 3b)：搜索状态的 URL 编解码。
// 锁定默认裁剪(mixed/filename/空 q 省略)+ 防御式白名单(mode/scope)+ q 原串保真。

import { describe, it, expect } from 'vitest'
import { encodeSearch, decodeSearch } from './searchUrl'
import type { LocationQuery } from 'vue-router'

describe('searchUrl（S2-b2 搜索 URL 编解码）', () => {
  it('encode：全默认(mixed/filename/空 q) → 空对象', () => {
    expect(encodeSearch({ mode: 'mixed', scope: 'filename', query: '' })).toEqual({})
  })

  it('encode：仅输出非默认字段', () => {
    expect(encodeSearch({ mode: 'semantic', scope: 'global', query: '猫' })).toEqual({
      mode: 'semantic',
      scope: 'global',
      q: '猫',
    })
  })

  it('encode：空白 q(仅空格)视为空,不输出', () => {
    expect(encodeSearch({ mode: 'mixed', scope: 'filename', query: '   ' })).toEqual({})
  })

  it('encode：normal 模式 + 默认范围 + 有查询 → 只出 mode 与 q', () => {
    expect(encodeSearch({ mode: 'normal', scope: 'filename', query: 'IMG_1234' })).toEqual({
      mode: 'normal',
      q: 'IMG_1234',
    })
  })

  it('decode：空 query → 空 Partial', () => {
    expect(decodeSearch({})).toEqual({})
  })

  it('decode：合法 mode/scope/q 全纳入', () => {
    const q: LocationQuery = { mode: 'semantic', scope: 'device', q: 'sunset' }
    expect(decodeSearch(q)).toEqual({ mode: 'semantic', scope: 'device', query: 'sunset' })
  })

  it('decode：防御式——非法 mode/scope 丢弃', () => {
    const q: LocationQuery = { mode: 'fuzzy', scope: 'planet', q: 'x' }
    expect(decodeSearch(q)).toEqual({ query: 'x' })
  })

  it('decode：空 q 不纳入(视为无查询)', () => {
    expect(decodeSearch({ q: '' })).toEqual({})
  })

  it('decode：数组值取首', () => {
    const q: LocationQuery = { mode: ['normal', 'semantic'], q: ['a', 'b'] }
    expect(decodeSearch(q)).toEqual({ mode: 'normal', query: 'a' })
  })

  it('往返：encode(非默认) → decode 还原同值', () => {
    const snap = { mode: 'semantic' as const, scope: 'location', query: '海边 日落' }
    const round = decodeSearch(encodeSearch(snap) as LocationQuery)
    expect(round).toEqual({ mode: 'semantic', scope: 'location', query: '海边 日落' })
  })
})
