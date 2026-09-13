// src/components/media/lensSeparator.ts
// 重复镜头（2026-09-02 方案 §7/§11.2）separator 与卡片徽标的纯判定函数。
// MediaGridRow.vue 是 SFC 无组件测试基建，把「文件夹头统计拼装」「簇头渲染条件」
// 「组徽标 vs 组内位次徽标选择」抽到本模块配 vitest 单测（§7.2/§7.3）。
// 只依赖 types/layout 的行数据形状，不依赖 Vue/i18n——文案由调用方注入翻译。

import type { LayoutRow, LayoutRowItem, LayoutRowSeparator } from '../../types/layout'
import { formatFileSize } from '../../utils/format'

// ── 镜头组头/簇头文案（数值由后端提供，句式由前端 locale 提供）──────────────

export interface LensGroupHeaderValues {
  ordinal?: number
  memberCount?: number
  folderCount?: number
  unitSize?: number
}

export interface LensClusterHeaderValues {
  start?: boolean
  ordinal?: number
  folderCount?: number
  groupCount?: number
}

/** groups 组头的翻译注入函数；unitSize 已先按前端规则格式化。 */
export type LensGroupHeaderTranslate = (params: {
  ordinal: number
  memberCount: number
  folderCount: number
  unitSize: string
}) => string

/** folders 簇头的翻译注入函数。 */
export type LensClusterHeaderTranslate = (params: {
  ordinal: number
  folderCount: number
  groupCount: number
}) => string

/** 组头结构化数值缺失时返回调用方兜底；完整数值时由当前 locale 生成文案。 */
export function formatLensGroupLabel(
  values: LensGroupHeaderValues,
  fallback: string,
  translate: LensGroupHeaderTranslate,
): string {
  if (
    values.ordinal == null ||
    values.memberCount == null ||
    values.folderCount == null ||
    values.unitSize == null
  ) {
    return fallback
  }
  return translate({
    ordinal: values.ordinal,
    memberCount: values.memberCount,
    folderCount: values.folderCount,
    unitSize: formatFileSize(values.unitSize),
  })
}

/** 仅簇首且数值完整时返回 locale 文案；路径标签不参与簇头格式化。 */
export function formatLensClusterLabel(
  values: LensClusterHeaderValues,
  translate: LensClusterHeaderTranslate,
): string | null {
  if (
    values.start !== true ||
    values.ordinal == null ||
    values.folderCount == null ||
    values.groupCount == null
  ) {
    return null
  }
  return translate({
    ordinal: values.ordinal,
    folderCount: values.folderCount,
    groupCount: values.groupCount,
  })
}

// ── 文件夹头统计（§7.2）─────────────────────────────────────────────────────

/** 文件夹头第二行统计所需的行内字段（§11.2 separator 增量投影）。 */
export interface LensFolderStats {
  duplicateCount: number
  unconfirmedCount: number
  uniqueCount: number
  uniqueHidden: boolean
}

/**
 * 取文件夹头统计：仅 duplicateFolder 型 separator 携带三桶计数；
 * 其他行类型/分隔符类别返回 null（调用方不渲染统计行）。
 */
export function getLensFolderStats(row: LayoutRow): LensFolderStats | null {
  if (row.rowType !== 'separator' || row.separatorKind !== 'duplicateFolder') return null
  const sep = row as LayoutRowSeparator
  return {
    duplicateCount: sep.duplicateCount ?? 0,
    unconfirmedCount: sep.unconfirmedCount ?? 0,
    uniqueCount: sep.uniqueCount ?? 0,
    uniqueHidden: sep.uniqueHidden ?? false,
  }
}

/** 统计段的 i18n 键名（duplicatesLens.* 命名空间下的叶子键）。 */
export type LensFolderStatKey = 'statDuplicate' | 'statUnconfirmed' | 'statUniqueShown' | 'statUniqueHidden'

/** 单个统计段：键 + 计数（键决定文案形态，n 决定数值）。 */
export interface LensFolderStatSegment {
  key: LensFolderStatKey
  n: number
}

/**
 * 统计行三段拼装（§7.2「{n} 重复 · {n} 尚未确认 · {n} 独有（已隐藏/已显示）」）：
 * 三段恒全部显示（0 也显示，独有项隐藏时数量仍准确）；末段按 uniqueHidden 选
 * 「已显示/已隐藏」键。分隔符「 · 」由 formatLensFolderStats 统一拼接。
 */
export function lensFolderStatSegments(stats: LensFolderStats): LensFolderStatSegment[] {
  return [
    { key: 'statDuplicate', n: stats.duplicateCount },
    { key: 'statUnconfirmed', n: stats.unconfirmedCount },
    stats.uniqueHidden
      ? { key: 'statUniqueHidden', n: stats.uniqueCount }
      : { key: 'statUniqueShown', n: stats.uniqueCount },
  ]
}

/** 翻译注入形态：key 为统计段键，n 为计数。组件传 t 的薄包装，spec 传 stub。 */
export type LensStatTranslate = (key: LensFolderStatKey, n: number) => string

/** 三段经注入翻译后用「 · 」连成统计行文案。 */
export function formatLensFolderStats(stats: LensFolderStats, translate: LensStatTranslate): string {
  return lensFolderStatSegments(stats)
    .map(({ key, n }) => translate(key, n))
    .join(' · ')
}

// ── 关联簇头（§7.1/§7.2）───────────────────────────────────────────────────

/**
 * 簇头渲染条件：仅 duplicateFolder 型且为簇首（parentGroupStart=true 且数值完整）的
 * 文件夹头显示关联簇信息,避免叠加两层长期 sticky（§7.2）;非簇首/其他分隔符类别不渲染。
 * parentGroup* 是 folders 模式的投影字段（§11.2）,故绑定 separatorKind 收紧契约。
 */
export function isLensClusterStart(row: LayoutRow): boolean {
  if (row.rowType !== 'separator' || row.separatorKind !== 'duplicateFolder') return false
  return (
    row.parentGroupStart === true &&
    row.parentGroupOrdinal != null &&
    row.parentGroupFolderCount != null &&
    row.parentGroupGroupCount != null
  )
}

// ── 卡片徽标（§6.2 groups / §7.3 folders）─────────────────────────────────

/** 卡片右上角镜头徽标的三种形态。 */
export type LensCardBadge =
  | /** groups 模式（§6.2）：组内位次 M/N（第 M/共 N 项）。 */
    { kind: 'memberPosition'; groupOrdinal: number; ordinal: number; count: number | null }
  | /** folders 模式（§7.3）：重复卡片「组 N」——组员跨目录分散无组内序（§16）。 */
    { kind: 'groupBadge'; ordinal: number }
  | /** folders 模式（§7.3）：尚未确认卡片问号角标。 */
    { kind: 'unconfirmed' }

/**
 * 徽标选择（优先序即模式序）：
 * 1. 组内位次（groups 模式恒 bucket=duplicate + memberOrdinal）优先——M/N 行为不变；
 * 2. folders 模式尚未确认 → 问号；
 * 3. folders 模式重复且有组序 → 「组 N」；独有（bucket=unique）无组序 → null 不显徽标；
 * 4. 普通画廊行无投影字段 → null。
 */
export function resolveLensCardBadge(item: LayoutRowItem): LensCardBadge | null {
  if (item.duplicateMemberOrdinal != null) {
    return {
      kind: 'memberPosition',
      groupOrdinal: item.duplicateGroupOrdinal ?? 0,
      ordinal: item.duplicateMemberOrdinal,
      count: item.duplicateMemberCount ?? null,
    }
  }
  if (item.duplicateBucket === 'unconfirmed') return { kind: 'unconfirmed' }
  if (item.duplicateBucket === 'duplicate' && item.duplicateGroupOrdinal != null) {
    return { kind: 'groupBadge', ordinal: item.duplicateGroupOrdinal }
  }
  return null
}
