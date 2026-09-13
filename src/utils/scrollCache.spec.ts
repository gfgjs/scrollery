// 滚动缓存键(方案 §8.3):镜头态 `lens-{mode}`(folders 另按「显示独有项」开关分键
// `lens-folders-u1`)与普通 `dir-N`/`album-*` 空间隔离的锁死测试。
// 此前键拼装只在 MediaGrid.getViewKey 一处;抽到 scrollCache 后 duplicateLensStore 共用,
// 键形回归会同时破坏镜头滚动恢复与返回快照锚点。

import { describe, expect, it } from 'vitest'
import { galleryScrollKey } from './scrollCache'

describe('galleryScrollKey', () => {
  it('无目录 → 智能相册键(普通画廊现状不变)', () => {
    expect(galleryScrollKey(null, 'all')).toBe('album-all')
    expect(galleryScrollKey(null, 'favorites')).toBe('album-favorites')
  })

  it('有目录 → 目录键(普通画廊现状不变)', () => {
    expect(galleryScrollKey(42, 'all')).toBe('dir-42')
  })

  it('镜头态忽略目录/相册维度,键形为 lens-{mode}(§8.3 防串写)', () => {
    expect(galleryScrollKey(42, 'all', 'groups')).toBe('lens-groups')
    expect(galleryScrollKey(null, 'all', 'folders')).toBe('lens-folders')
  })

  it('folders 镜头按「显示独有项」开关分键(§8.3:开关切换即换集合,滚动位不串)', () => {
    expect(galleryScrollKey(null, 'all', 'folders', true)).toBe('lens-folders-u1')
    expect(galleryScrollKey(null, 'all', 'folders', false)).toBe('lens-folders')
  })

  it('groups 镜头开关无意义,恒不带 -u1 后缀(§4.4 白名单不编码)', () => {
    expect(galleryScrollKey(42, 'all', 'groups', true)).toBe('lens-groups')
    expect(galleryScrollKey(42, 'all', 'groups', false)).toBe('lens-groups')
  })

  it('镜头键与普通键空间隔离,同一目录下两态不共写一条缓存', () => {
    expect(galleryScrollKey(42, 'all', 'groups')).not.toBe(galleryScrollKey(42, 'all'))
    expect(galleryScrollKey(42, 'all', 'folders', true)).not.toBe(galleryScrollKey(42, 'all'))
  })
})
