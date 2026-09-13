// viewRoute 纯函数单测(S2-c)：视图维度 ↔ path 双向映射 + 主库血统判据。
// path→view 是外部输入解析(URL 可深链/手改),防御式判据须锁死。

import { describe, it, expect } from 'vitest'
import {
  smartAlbumToPath,
  folderToPath,
  routeToView,
  isPrimaryGalleryRoute,
} from './viewRoute'
import type { SmartAlbum } from '../types/ui'

const ALBUMS: SmartAlbum[] = ['all', 'favorites', 'live-photos', 'recent', 'trash']

describe('viewRoute（S2-c 视图↔路径映射）', () => {
  it('smartAlbumToPath：5 档各映射到约定路径,all→/', () => {
    expect(smartAlbumToPath('all')).toBe('/')
    expect(smartAlbumToPath('favorites')).toBe('/favorites')
    expect(smartAlbumToPath('live-photos')).toBe('/live-photos')
    expect(smartAlbumToPath('recent')).toBe('/recent')
    expect(smartAlbumToPath('trash')).toBe('/trash')
  })

  it('folderToPath：数字 id → /folder/:id', () => {
    expect(folderToPath(5)).toBe('/folder/5')
    expect(folderToPath(0)).toBe('/folder/0')
  })

  it('往返一致：smartAlbumToPath → routeToView 还原同一 album', () => {
    for (const a of ALBUMS) {
      expect(routeToView(smartAlbumToPath(a))).toEqual({ kind: 'smartAlbum', album: a })
    }
  })

  it('routeToView：/folder/<数字> → directory', () => {
    expect(routeToView('/folder/12')).toEqual({ kind: 'directory', id: 12 })
    expect(routeToView('/folder/0')).toEqual({ kind: 'directory', id: 0 })
  })

  it('routeToView：非视图路径 → null(collection/person 详情、查看器、设置等)', () => {
    expect(routeToView('/collections/3')).toBeNull()
    expect(routeToView('/persons/7')).toBeNull()
    expect(routeToView('/collections')).toBeNull()
    expect(routeToView('/persons')).toBeNull()
    expect(routeToView('/view/9')).toBeNull()
    expect(routeToView('/settings')).toBeNull()
    expect(routeToView('/plugins')).toBeNull()
  })

  it('routeToView：防御式——非数字 folder id 不解析为 directory', () => {
    expect(routeToView('/folder/abc')).toBeNull()
    expect(routeToView('/folder/')).toBeNull()
    expect(routeToView('/folder/5/extra')).toBeNull()
  })

  it('isPrimaryGalleryRoute：主库血统(smart-album + folder 筛选)为真,其余为假', () => {
    // 拆分前全部停在 '/' 的视图集合(供 SemanticSearchPanel 平移用)。
    expect(isPrimaryGalleryRoute('/')).toBe(true)
    expect(isPrimaryGalleryRoute('/favorites')).toBe(true)
    expect(isPrimaryGalleryRoute('/trash')).toBe(true)
    expect(isPrimaryGalleryRoute('/live-photos')).toBe(true)
    expect(isPrimaryGalleryRoute('/recent')).toBe(true)
    expect(isPrimaryGalleryRoute('/folder/42')).toBe(true)
    // collection/person 详情此前即非 '/'、不在此列。
    expect(isPrimaryGalleryRoute('/collections/3')).toBe(false)
    expect(isPrimaryGalleryRoute('/persons/7')).toBe(false)
    expect(isPrimaryGalleryRoute('/settings')).toBe(false)
  })
})
