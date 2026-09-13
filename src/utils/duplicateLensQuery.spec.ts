// duplicateLensQuery 纯函数单测（2026-09-02 方案 P0）：重复镜头的 URL 编解码。
// 重点锁 §4.4 契约：duplicateUnique 只认字面 '1' 且仅 folders 生效、垃圾值一律回落「关闭」、
// normalize 的键移除行为,以及 encode/parse 往返闭合。

import { describe, it, expect } from 'vitest'
import type { LocationQuery } from 'vue-router'
import {
  parseDuplicateLensQuery,
  normalizeDuplicateLensQuery,
  encodeDuplicateLensQuery,
  type DuplicateLensQueryState,
} from './duplicateLensQuery'

const OFF: DuplicateLensQueryState = { mode: null, showUniqueItems: false }

describe('parseDuplicateLensQuery（§4.4 URL 契约）', () => {
  it('缺失 duplicates → off', () => {
    expect(parseDuplicateLensQuery({})).toEqual(OFF)
  })

  it('?duplicates=groups → groups（不认 duplicateUnique）', () => {
    expect(parseDuplicateLensQuery({ duplicates: 'groups' })).toEqual({
      mode: 'groups',
      showUniqueItems: false,
    })
  })

  it('?duplicates=folders → folders', () => {
    expect(parseDuplicateLensQuery({ duplicates: 'folders' })).toEqual({
      mode: 'folders',
      showUniqueItems: false,
    })
  })

  it('?duplicates=folders&duplicateUnique=1 → folders + 显示独有项', () => {
    expect(parseDuplicateLensQuery({ duplicates: 'folders', duplicateUnique: '1' })).toEqual({
      mode: 'folders',
      showUniqueItems: true,
    })
  })

  it('duplicateUnique 只认字面 \'1\'（0/true/2 均为 false）', () => {
    for (const v of ['0', 'true', '2']) {
      expect(
        parseDuplicateLensQuery({ duplicates: 'folders', duplicateUnique: v }).showUniqueItems,
      ).toBe(false)
    }
  })

  it('?duplicates=xxx / ?duplicates=（空串）→ off', () => {
    expect(parseDuplicateLensQuery({ duplicates: 'xxx' })).toEqual(OFF)
    expect(parseDuplicateLensQuery({ duplicates: '' })).toEqual(OFF)
  })

  it('?duplicates=groups&duplicateUnique=1 → groups 不接受独有项开关', () => {
    expect(parseDuplicateLensQuery({ duplicates: 'groups', duplicateUnique: '1' })).toEqual({
      mode: 'groups',
      showUniqueItems: false,
    })
  })

  it('?duplicateUnique=1（无 duplicates）→ off', () => {
    expect(parseDuplicateLensQuery({ duplicateUnique: '1' })).toEqual(OFF)
  })

  it('防御式:null 值（bare ?duplicates）/大小写不匹配/垃圾一律 off', () => {
    expect(parseDuplicateLensQuery({ duplicates: null })).toEqual(OFF)
    expect(parseDuplicateLensQuery({ duplicates: 'Groups' })).toEqual(OFF)
    expect(parseDuplicateLensQuery({ duplicates: ['FOLDERS'] })).toEqual(OFF)
  })

  it('数组值取首（vue-router 重复键）', () => {
    expect(parseDuplicateLensQuery({ duplicates: ['folders', 'groups'] }).mode).toBe('folders')
    expect(
      parseDuplicateLensQuery({ duplicates: 'folders', duplicateUnique: ['1', '0'] })
        .showUniqueItems,
    ).toBe(true)
  })
})

describe('normalizeDuplicateLensQuery（键移除行为）', () => {
  it('非法 duplicates 移除该键,非管理键保留', () => {
    expect(normalizeDuplicateLensQuery({ types: 'image', duplicates: 'xxx' })).toEqual({
      types: 'image',
    })
  })

  it('空串/数组残片/null 值的 duplicates 一律移除', () => {
    expect(normalizeDuplicateLensQuery({ duplicates: '' })).toEqual({})
    expect(normalizeDuplicateLensQuery({ duplicates: null })).toEqual({})
    expect(normalizeDuplicateLensQuery({ duplicates: ['groups', 'folders'] })).toEqual({
      duplicates: 'groups',
    })
  })

  it('duplicateUnique 仅在 duplicates=folders 且字面 \'1\' 时保留', () => {
    expect(
      normalizeDuplicateLensQuery({ duplicates: 'folders', duplicateUnique: '1' }),
    ).toEqual({ duplicates: 'folders', duplicateUnique: '1' })
  })

  it('groups 模式下 duplicateUnique 移除', () => {
    expect(
      normalizeDuplicateLensQuery({ duplicates: 'groups', duplicateUnique: '1' }),
    ).toEqual({ duplicates: 'groups' })
  })

  it('镜头关闭时 duplicateUnique 孤儿键移除', () => {
    expect(normalizeDuplicateLensQuery({ duplicateUnique: '1' })).toEqual({})
    expect(normalizeDuplicateLensQuery({ duplicates: 'xxx', duplicateUnique: '1' })).toEqual({})
  })

  it('duplicateUnique 非 \'1\' 一律移除（folders 亦然）', () => {
    for (const v of ['0', 'true', '2', '']) {
      expect(normalizeDuplicateLensQuery({ duplicates: 'folders', duplicateUnique: v })).toEqual({
        duplicates: 'folders',
      })
    }
  })

  it('合法 query 规范化后不变（幂等）', () => {
    const q: LocationQuery = { types: 'image', duplicates: 'folders', duplicateUnique: '1' }
    const once = normalizeDuplicateLensQuery(q)
    expect(once).toEqual({ types: 'image', duplicates: 'folders', duplicateUnique: '1' })
    expect(normalizeDuplicateLensQuery(once)).toEqual(once)
  })

  it('normalize 不改变 parse 语义:parse(normalize(q)) === parse(q)', () => {
    const garbage: LocationQuery[] = [
      { duplicates: 'xxx', duplicateUnique: '1' },
      { duplicates: 'folders', duplicateUnique: '0' },
      { duplicates: 'groups', duplicateUnique: '1' },
      { duplicateUnique: '1' },
      { duplicates: 'folders', duplicateUnique: '1' },
    ]
    for (const q of garbage) {
      expect(parseDuplicateLensQuery(normalizeDuplicateLensQuery(q))).toEqual(
        parseDuplicateLensQuery(q),
      )
    }
  })
})

describe('encodeDuplicateLensQuery', () => {
  it('mode=null → 空对象（镜头关闭对应干净 URL）', () => {
    expect(encodeDuplicateLensQuery(OFF)).toEqual({})
  })

  it('groups → 仅 duplicates 键', () => {
    expect(encodeDuplicateLensQuery({ mode: 'groups', showUniqueItems: false })).toEqual({
      duplicates: 'groups',
    })
  })

  it('folders+showUnique → duplicates + duplicateUnique=1', () => {
    expect(encodeDuplicateLensQuery({ mode: 'folders', showUniqueItems: true })).toEqual({
      duplicates: 'folders',
      duplicateUnique: '1',
    })
  })

  it('folders 默认不写 duplicateUnique', () => {
    expect(encodeDuplicateLensQuery({ mode: 'folders', showUniqueItems: false })).toEqual({
      duplicates: 'folders',
    })
  })

  it('写侧同样过白名单:groups 误带 showUniqueItems=true 也不写出 duplicateUnique', () => {
    expect(encodeDuplicateLensQuery({ mode: 'groups', showUniqueItems: true })).toEqual({
      duplicates: 'groups',
    })
  })
})

describe('encode/parse 往返闭合', () => {
  const STATES: DuplicateLensQueryState[] = [
    OFF,
    { mode: 'groups', showUniqueItems: false },
    { mode: 'folders', showUniqueItems: false },
    { mode: 'folders', showUniqueItems: true },
  ]

  it('全部合法状态 encode → parse 还原同值', () => {
    for (const s of STATES) {
      expect(parseDuplicateLensQuery(encodeDuplicateLensQuery(s))).toEqual(s)
    }
  })

  it('normalize → parse 与直接 parse 同语义（§4.4 规则不因规范化漂移）', () => {
    for (const s of STATES) {
      expect(parseDuplicateLensQuery(normalizeDuplicateLensQuery(encodeDuplicateLensQuery(s)))).toEqual(s)
    }
  })
})
