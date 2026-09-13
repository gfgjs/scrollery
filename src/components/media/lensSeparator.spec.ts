// lensSeparator 纯函数锁测试（2026-09-02 方案 §7.2/§7.3）：文件夹头统计拼装、簇头渲染条件、
// 组徽标 vs 组内位次徽标选择。MediaGridRow.vue 无组件测试基建，判定逻辑全部抽到 lensSeparator.ts。

import { describe, expect, it } from 'vitest'
import type { LayoutRowItem, LayoutRowSeparator } from '../../types/layout'
import {
  formatLensClusterLabel,
  formatLensFolderStats,
  formatLensGroupLabel,
  getLensFolderStats,
  isLensClusterStart,
  lensFolderStatSegments,
  resolveLensCardBadge,
} from './lensSeparator'

function sepRow(extra: Partial<LayoutRowSeparator> = {}): LayoutRowSeparator {
  return { rowType: 'separator', y: 0, height: 36, separatorLabel: '', ...extra }
}

function itemRow(extra: Partial<LayoutRowItem> = {}): LayoutRowItem {
  return {
    id: 1,
    x: 0,
    w: 100,
    h: 100,
    fileSize: 1,
    fileFormat: 'jpg',
    mediaType: 'image',
    isLivePhoto: false,
    durationMs: null,
    thumbStatus: 2,
    thumbPath: null,
    placeholderColor: null,
    isFavorited: false,
    rating: 0,
    colorLabel: 0,
    availability: 'online',
    originalWidth: 10,
    originalHeight: 10,
    sortDatetime: 0,
    ...extra,
  }
}

describe('getLensFolderStats', () => {
  it('duplicateFolder separator 返回三桶计数 + 隐藏标记', () => {
    expect(
      getLensFolderStats(
        sepRow({
          separatorKind: 'duplicateFolder',
          duplicateCount: 2,
          unconfirmedCount: 1,
          uniqueCount: 5,
          uniqueHidden: true,
        }),
      ),
    ).toEqual({ duplicateCount: 2, unconfirmedCount: 1, uniqueCount: 5, uniqueHidden: true })
  })

  it('非 duplicateFolder（组头/日期/文件夹）与普通行返回 null', () => {
    expect(getLensFolderStats(sepRow({ separatorKind: 'duplicateGroup' }))).toBeNull()
    expect(getLensFolderStats(sepRow({ separatorKind: 'date' }))).toBeNull()
    expect(getLensFolderStats(sepRow({ separatorKind: 'folder' }))).toBeNull()
    expect(getLensFolderStats({ rowType: 'normal', y: 0, height: 100, items: [] })).toBeNull()
  })

  it('字段缺省回落 0/false（防御后端旧版本）', () => {
    expect(getLensFolderStats(sepRow({ separatorKind: 'duplicateFolder' }))).toEqual({
      duplicateCount: 0,
      unconfirmedCount: 0,
      uniqueCount: 0,
      uniqueHidden: false,
    })
  })
})

describe('镜头组头/簇头 i18n 文案', () => {
  it('组头把结构化数值交给翻译模板，不再消费后端中文标签', () => {
    const text = formatLensGroupLabel(
      { ordinal: 12, memberCount: 3, folderCount: 2, unitSize: 24 * 1024 * 1024 },
      '',
      ({ ordinal, memberCount, folderCount, unitSize }) =>
        `Duplicate group ${ordinal} · ${memberCount} items · ${folderCount} folders · ${unitSize} each`,
    )
    expect(text).toBe('Duplicate group 12 · 3 items · 2 folders · 24.0 MB each')
  })

  it('簇头只用数值生成 locale 文案，非簇首不生成', () => {
    const translate = ({ ordinal, folderCount, groupCount }: { ordinal: number; folderCount: number; groupCount: number }) =>
      `Related folder cluster ${ordinal} · ${folderCount} folders · ${groupCount} duplicate groups`
    expect(
      formatLensClusterLabel({ start: true, ordinal: 4, folderCount: 3, groupCount: 7 }, translate),
    ).toBe('Related folder cluster 4 · 3 folders · 7 duplicate groups')
    expect(formatLensClusterLabel({ start: false, ordinal: 4, folderCount: 3, groupCount: 7 }, translate)).toBeNull()
  })
})

describe('lensFolderStatSegments / formatLensFolderStats', () => {
  it('三段恒显示（0 也显示），末段按 uniqueHidden 选键（§7.2 隐藏时仍报准确数量）', () => {
    expect(
      lensFolderStatSegments({ duplicateCount: 0, unconfirmedCount: 2, uniqueCount: 5, uniqueHidden: true }),
    ).toEqual([
      { key: 'statDuplicate', n: 0 },
      { key: 'statUnconfirmed', n: 2 },
      { key: 'statUniqueHidden', n: 5 },
    ])
    expect(
      lensFolderStatSegments({ duplicateCount: 3, unconfirmedCount: 0, uniqueCount: 1, uniqueHidden: false })[2],
    ).toEqual({ key: 'statUniqueShown', n: 1 })
  })

  it('formatLensFolderStats 按注入翻译连成「 · 」单行', () => {
    const text = formatLensFolderStats(
      { duplicateCount: 2, unconfirmedCount: 1, uniqueCount: 4, uniqueHidden: false },
      (key, n) => `${key}(${n})`,
    )
    expect(text).toBe('statDuplicate(2) · statUnconfirmed(1) · statUniqueShown(4)')
  })
})

describe('isLensClusterStart', () => {
  it('簇首（parentGroupStart=true 且有结构化簇数值）为真', () => {
    expect(
      isLensClusterStart(
        sepRow({
          separatorKind: 'duplicateFolder',
          parentGroupStart: true,
          parentGroupOrdinal: 1,
          parentGroupFolderCount: 2,
          parentGroupGroupCount: 1,
        }),
      ),
    ).toBe(true)
  })

  it('非簇首 / 缺结构化数值 / 普通行均为假（§7.2 非簇首不渲染簇头行）', () => {
    expect(isLensClusterStart(sepRow({ separatorKind: 'duplicateFolder', parentGroupStart: false }))).toBe(false)
    expect(
      isLensClusterStart(
        sepRow({ separatorKind: 'duplicateFolder', parentGroupStart: true, parentGroupOrdinal: 1 }),
      ),
    ).toBe(false)
    expect(isLensClusterStart(sepRow({ separatorKind: 'duplicateGroup', parentGroupStart: true }))).toBe(false)
    expect(isLensClusterStart({ rowType: 'normal', y: 0, height: 100, items: [] })).toBe(false)
  })
})

describe('resolveLensCardBadge', () => {
  it('groups 模式：组内位次优先，M/N 行为不变（§6.2）', () => {
    expect(
      resolveLensCardBadge(
        itemRow({ duplicateBucket: 'duplicate', duplicateGroupOrdinal: 3, duplicateMemberOrdinal: 2, duplicateMemberCount: 7 }),
      ),
    ).toEqual({ kind: 'memberPosition', groupOrdinal: 3, ordinal: 2, count: 7 })
    expect(
      resolveLensCardBadge(itemRow({ duplicateBucket: 'duplicate', duplicateGroupOrdinal: 1, duplicateMemberOrdinal: 4 })),
    ).toEqual({ kind: 'memberPosition', groupOrdinal: 1, ordinal: 4, count: null })
  })

  it('folders 模式：尚未确认 → 问号（§7.3 不只用灰色）', () => {
    expect(resolveLensCardBadge(itemRow({ duplicateBucket: 'unconfirmed' }))).toEqual({ kind: 'unconfirmed' })
  })

  it('folders 模式：重复且有组序 → 组徽标（memberOrdinal 为 null，§16）', () => {
    expect(resolveLensCardBadge(itemRow({ duplicateBucket: 'duplicate', duplicateGroupOrdinal: 5 }))).toEqual({
      kind: 'groupBadge',
      ordinal: 5,
    })
  })

  it('folders 模式：独有无徽标（不显示「组」）', () => {
    expect(resolveLensCardBadge(itemRow({ duplicateBucket: 'unique' }))).toBeNull()
    expect(resolveLensCardBadge(itemRow({ duplicateBucket: 'duplicate' }))).toBeNull()
  })

  it('普通画廊行（无投影字段）为 null', () => {
    expect(resolveLensCardBadge(itemRow())).toBeNull()
  })
})
