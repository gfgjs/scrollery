// galleryQuery 编解码单测（S2-b）。重点锁防御式解析:URL 是外部输入,损坏/越界/非法值必须
// 回落默认而非直灌 store。同时锁 encode/decode 往返一致与快照相等（echo-guard 依赖它）。

import { describe, it, expect } from 'vitest'
import type { LocationQuery } from 'vue-router'
import {
  isGalleryRoute,
  encodeGalleryFilters,
  decodeGalleryFilters,
  galleryFilterSnapshotEqual,
  type GalleryFilterSnapshot,
} from './galleryQuery'

const EMPTY: GalleryFilterSnapshot = {
  mediaTypes: [],
  fileFormats: [],
  favoritedOnly: false,
  livePhotoOnly: false,
  minRating: 0,
  colorLabel: 0,
  dateFrom: null,
  dateTo: null,
}

describe('isGalleryRoute', () => {
  it('MediaGrid 承载路由判真', () => {
    for (const p of [
      '/',
      '/favorites',
      '/live-photos',
      '/recent',
      '/trash',
      '/folder/5',
      '/collections/3',
      '/persons/7',
    ]) {
      expect(isGalleryRoute(p)).toBe(true)
    }
  })
  it('列表页/查看器/设置等判假', () => {
    for (const p of [
      '/collections',
      '/persons',
      '/settings',
      '/settings/advanced',
      '/plugins',
      '/doc/1',
      '/audio/1',
      '/view/1',
      '/hgallery-lab',
    ]) {
      expect(isGalleryRoute(p)).toBe(false)
    }
  })
})

describe('encodeGalleryFilters', () => {
  it('默认快照编码为空对象（干净视图=干净 URL）', () => {
    expect(encodeGalleryFilters(EMPTY)).toEqual({})
  })
  it('各字段激活各出一键', () => {
    const s: GalleryFilterSnapshot = {
      mediaTypes: ['image', 'video'],
      fileFormats: ['png', 'jpg'],
      favoritedOnly: true,
      livePhotoOnly: true,
      minRating: 3,
      colorLabel: 4,
      dateFrom: 1719763200,
      dateTo: 1719849599,
    }
    expect(encodeGalleryFilters(s)).toEqual({
      types: 'image,video',
      formats: 'png,jpg',
      favorite: '1',
      live: '1',
      rating: '3',
      color: '4',
      from: '1719763200',
      to: '1719849599',
    })
  })
  it('rating/color 为 0 不出键', () => {
    expect(encodeGalleryFilters({ ...EMPTY, minRating: 0, colorLabel: 0 })).toEqual({})
  })
})

describe('decodeGalleryFilters（防御式解析）', () => {
  it('空 query 解码为默认快照', () => {
    expect(decodeGalleryFilters({})).toEqual(EMPTY)
  })

  it('合法 query 正确解码', () => {
    const q: LocationQuery = {
      types: 'image,video',
      formats: 'png,jpg',
      favorite: '1',
      live: '1',
      rating: '3',
      color: '4',
      from: '1719763200',
      to: '1719849599',
    }
    expect(decodeGalleryFilters(q)).toEqual({
      mediaTypes: ['image', 'video'],
      fileFormats: ['png', 'jpg'],
      favoritedOnly: true,
      livePhotoOnly: true,
      minRating: 3,
      colorLabel: 4,
      dateFrom: 1719763200,
      dateTo: 1719849599,
    })
  })

  it('未知媒体类型被白名单剔除,合法项保留', () => {
    expect(decodeGalleryFilters({ types: 'image,evil,video,../etc' }).mediaTypes).toEqual([
      'image',
      'video',
    ])
  })

  /**
   * 🔴 白名单必须覆盖**UI 能切的每一类**,否则 encode/decode 不对称。
   *
   * 形态:encode 把 store 里有什么写什么(宽),decode 按白名单过滤(窄)。白名单漏一类,用户点了
   * 「文档」chip 再刷新,筛选**静默消失** —— URL 里明明写着 `types=document`,却没有任何报错。
   * S 线 D-008 把文档/音频提为常显 chip 时,这条正是那个「顺手就会漏」的地方。
   *
   * 可证伪性:改造前白名单是 `['image','video']`,本用例当场红。
   */
  it('四大类全部通过白名单(文档/音频是常显 chip,漏了会让深链静默丢筛选)', () => {
    expect(decodeGalleryFilters({ types: 'image,video,document,audio' }).mediaTypes).toEqual([
      'image',
      'video',
      'document',
      'audio',
    ])
  })

  /** encode↔decode 往返:UI 能切的类型集合原样回来,一个不丢一个不多。 */
  it('四大类往返编解码无损', () => {
    const types = ['image', 'video', 'document', 'audio']
    const q = encodeGalleryFilters({ ...EMPTY, mediaTypes: types })
    expect(decodeGalleryFilters(q as LocationQuery).mediaTypes).toEqual(types)
  })

  it('重复媒体类型去重', () => {
    expect(decodeGalleryFilters({ types: 'image,image,video' }).mediaTypes).toEqual([
      'image',
      'video',
    ])
  })

  // ── 细分格式（S 线 P3 / D-011）────────────────────────────────────────────

  it('格式:形态合法则保留,去重且保序', () => {
    expect(decodeGalleryFilters({ formats: 'png,jpg,png' }).fileFormats).toEqual(['png', 'jpg'])
  })

  /**
   * 🔴 形态校验拒绝而非「修正」—— 与后端 Catalog loader 的 `is_valid_format`（`catalog.rs:323`）
   * 同一把尺子：那边也是拒非小写而非转换。两侧尺子不同的后果是 URL 放进来的东西后端不认（或反之）。
   *
   * 样本逐条对应一种垃圾形态：大写 / 含点 / 路径穿越 / 超长 / 空段。
   */
  it('格式:非法形态一律剔除(不做大小写"修正")', () => {
    const got = decodeGalleryFilters({
      formats: `PNG,.jpg,../etc,${'a'.repeat(17)},,png`,
    }).fileFormats
    expect(got).toEqual(['png'])
  })

  /**
   * 数量有界:每个扩展名都会变成一个 SQL 绑定参数,不封顶等于让 URL 决定 `IN (...)` 有多长。
   * 可证伪性:上界 64,故给 70 个合法扩展名,断言只收 64。
   */
  it('格式:数量截断到上界(URL 不得决定 IN 列表有多长)', () => {
    const many = Array.from({ length: 70 }, (_, i) => `f${i}`).join(',')
    expect(decodeGalleryFilters({ formats: many }).fileFormats).toHaveLength(64)
  })

  /**
   * 🔴 **写侧也过白名单**（S 线 §8 点名要修的非对称）。
   *
   * 原先 encode 是「store 里有什么写什么」而 decode 按白名单收 —— 写宽读窄。两侧同一把尺子之后,
   * 写出去的 URL 就**保证**读得回来。可证伪性:store 里塞进垃圾类型/格式,encode 不得把它们写进 URL。
   */
  it('encode 侧同样过白名单:垃圾值不写进 URL(写宽读窄的非对称)', () => {
    const q = encodeGalleryFilters({
      ...EMPTY,
      mediaTypes: ['image', 'evil'],
      fileFormats: ['png', 'PNG', '../x'],
    })
    expect(q.types).toBe('image')
    expect(q.formats).toBe('png')
  })

  it('格式:空/缺席 → 空数组(该维度不限)', () => {
    expect(decodeGalleryFilters({}).fileFormats).toEqual([])
    expect(decodeGalleryFilters({ formats: '' }).fileFormats).toEqual([])
  })

  it('favorite/live 仅 "1" 为真,其余为假', () => {
    expect(decodeGalleryFilters({ favorite: '1' }).favoritedOnly).toBe(true)
    expect(decodeGalleryFilters({ favorite: 'true' }).favoritedOnly).toBe(false)
    expect(decodeGalleryFilters({ favorite: '0' }).favoritedOnly).toBe(false)
    expect(decodeGalleryFilters({ live: '1' }).livePhotoOnly).toBe(true)
  })

  it('rating 越界 clamp,非法回落 0', () => {
    expect(decodeGalleryFilters({ rating: '3' }).minRating).toBe(3)
    expect(decodeGalleryFilters({ rating: '99' }).minRating).toBe(5)
    expect(decodeGalleryFilters({ rating: '-2' }).minRating).toBe(0)
    expect(decodeGalleryFilters({ rating: 'abc' }).minRating).toBe(0)
    expect(decodeGalleryFilters({ rating: '2.5' }).minRating).toBe(0)
  })

  it('color 越界 clamp（上界 7）', () => {
    expect(decodeGalleryFilters({ color: '4' }).colorLabel).toBe(4)
    expect(decodeGalleryFilters({ color: '99' }).colorLabel).toBe(7)
    expect(decodeGalleryFilters({ color: 'x' }).colorLabel).toBe(0)
  })

  it('时间戳仅接受有限整数,垃圾/小数回落 null', () => {
    expect(decodeGalleryFilters({ from: '1719763200' }).dateFrom).toBe(1719763200)
    expect(decodeGalleryFilters({ from: '12abc' }).dateFrom).toBeNull()
    expect(decodeGalleryFilters({ from: '12.5' }).dateFrom).toBeNull()
    expect(decodeGalleryFilters({ to: '' }).dateTo).toBeNull()
  })

  it('数组型 query 值取首项', () => {
    expect(decodeGalleryFilters({ rating: ['3', '5'] }).minRating).toBe(3)
    expect(decodeGalleryFilters({ favorite: ['1'] }).favoritedOnly).toBe(true)
  })
})

describe('encode/decode 往返一致', () => {
  it('populated 快照 encode→decode 还原', () => {
    const s: GalleryFilterSnapshot = {
      mediaTypes: ['video'],
      fileFormats: ['mp4'],
      favoritedOnly: true,
      livePhotoOnly: false,
      minRating: 5,
      colorLabel: 2,
      dateFrom: 1700000000,
      dateTo: 1700086399,
    }
    expect(decodeGalleryFilters(encodeGalleryFilters(s))).toEqual(s)
  })
  it('默认快照往返仍为默认', () => {
    expect(decodeGalleryFilters(encodeGalleryFilters(EMPTY))).toEqual(EMPTY)
  })
})

describe('galleryFilterSnapshotEqual', () => {
  it('全等判真', () => {
    expect(galleryFilterSnapshotEqual(EMPTY, { ...EMPTY })).toBe(true)
  })
  it('任一字段不同判假', () => {
    expect(galleryFilterSnapshotEqual(EMPTY, { ...EMPTY, favoritedOnly: true })).toBe(false)
    expect(galleryFilterSnapshotEqual(EMPTY, { ...EMPTY, minRating: 1 })).toBe(false)
    expect(galleryFilterSnapshotEqual(EMPTY, { ...EMPTY, dateFrom: 1 })).toBe(false)
  })
  /**
   * 🔴 新增筛选维度必须同步 `galleryFilterSnapshotEqual` —— 而**类型系统在这里帮不上忙**。
   *
   * 实测(S 线 P3 加 fileFormats 时):`GalleryFilterSnapshot` 加字段后 `vue-tsc` 精确报出 4 处
   * 必改点(全是**构造**快照的地方),唯独漏掉本函数 —— 它只**读**字段,少读一个不是类型错。
   *
   * 漏了的症状:echo-guard 认为两个快照相等 → 跳过 URL→store 回填 → 深链里的格式筛选永不生效,
   * 且没有任何报错。故这条断言是这个维度上唯一的防线。
   */
  it('fileFormats 不同判假(echo-guard 的漏配 tsc 报不出来)', () => {
    expect(
      galleryFilterSnapshotEqual({ ...EMPTY, fileFormats: ['png'] }, { ...EMPTY }),
    ).toBe(false)
    expect(
      galleryFilterSnapshotEqual(
        { ...EMPTY, fileFormats: ['png', 'jpg'] },
        { ...EMPTY, fileFormats: ['jpg', 'png'] },
      ),
    ).toBe(false)
  })

  it('mediaTypes 顺序/长度不同判假', () => {
    expect(
      galleryFilterSnapshotEqual(
        { ...EMPTY, mediaTypes: ['image', 'video'] },
        { ...EMPTY, mediaTypes: ['video', 'image'] },
      ),
    ).toBe(false)
    expect(
      galleryFilterSnapshotEqual(
        { ...EMPTY, mediaTypes: ['image'] },
        { ...EMPTY, mediaTypes: ['image', 'video'] },
      ),
    ).toBe(false)
  })
})
