// resolveView 单测（S2-c1）。这是「视图维度 → backend」决策的唯一源（消 🔴 R1-2 双维护后），
// 必须穷举锁死 precedence 与各维度映射。用例覆盖 viewStore 可达的互斥状态 + 优先级顺序。

import { describe, it, expect } from 'vitest'
import { resolveView, type ViewState } from './resolveView'
import type { Collection } from '../types/media'

const BASE: ViewState = {
  activePersonId: null,
  activeCollection: null,
  activeSmartAlbum: 'all',
  activeDirectoryId: null,
}

function collection(overrides: Partial<Collection>): Collection {
  return {
    id: 1,
    name: 'c',
    kind: 'user',
    mediaTypeFilter: null,
    itemCount: 0,
    sortOrder: 0,
    ...overrides,
  }
}

describe('resolveView（视图维度单一决策源）', () => {
  it('默认（smartAlbum all，其余空）→ smartAlbum all', () => {
    expect(resolveView(BASE)).toEqual({ kind: 'smartAlbum', album: 'all' })
  })

  it('activePersonId → person', () => {
    expect(resolveView({ ...BASE, activePersonId: 7 })).toEqual({ kind: 'person', personId: 7 })
  })

  it('用户收藏夹 → collection(albumId)', () => {
    expect(resolveView({ ...BASE, activeCollection: collection({ id: 42, kind: 'user' }) })).toEqual(
      { kind: 'collection', albumId: 42 },
    )
  })

  it('系统夹带 mediaTypeFilter → systemCollection(mediaType)', () => {
    expect(
      resolveView({
        ...BASE,
        activeCollection: collection({ id: 9, kind: 'system', mediaTypeFilter: 'video' }),
      }),
    ).toEqual({ kind: 'systemCollection', mediaType: 'video' })
  })

  it('系统夹无 mediaTypeFilter → 回落 collection(albumId)', () => {
    expect(
      resolveView({
        ...BASE,
        activeCollection: collection({ id: 9, kind: 'system', mediaTypeFilter: null }),
      }),
    ).toEqual({ kind: 'collection', albumId: 9 })
  })

  it.each(['favorites', 'live-photos', 'recent', 'trash'] as const)(
    'smartAlbum %s → smartAlbum(该 album)',
    (album) => {
      expect(resolveView({ ...BASE, activeSmartAlbum: album })).toEqual({
        kind: 'smartAlbum',
        album,
      })
    },
  )

  it('smartAlbum all + 目录选中 → directory', () => {
    expect(resolveView({ ...BASE, activeDirectoryId: 5 })).toEqual({
      kind: 'directory',
      directoryId: 5,
    })
  })

  // ── precedence（互斥被破坏时的兜底顺序，对齐两处投影原控制流）──
  it('person 优先于其它一切', () => {
    expect(
      resolveView({
        activePersonId: 1,
        activeCollection: collection({ id: 2 }),
        activeSmartAlbum: 'favorites',
        activeDirectoryId: 3,
      }),
    ).toEqual({ kind: 'person', personId: 1 })
  })

  it('collection 优先于 smart-album 与 directory', () => {
    expect(
      resolveView({
        ...BASE,
        activeCollection: collection({ id: 2, kind: 'user' }),
        activeSmartAlbum: 'favorites',
        activeDirectoryId: 3,
      }),
    ).toEqual({ kind: 'collection', albumId: 2 })
  })

  it('非 all 的 smart-album 优先于 directory', () => {
    expect(resolveView({ ...BASE, activeSmartAlbum: 'trash', activeDirectoryId: 3 })).toEqual({
      kind: 'smartAlbum',
      album: 'trash',
    })
  })
})
