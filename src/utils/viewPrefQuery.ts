// src/utils/viewPrefQuery.ts
// view-preference(分组/组内排序/排序方向/布局)的 URL query 编解码（S2-b2）。纯函数,无 Vue/store 依赖。
//
// 与 filter(galleryQuery.ts)的关键差异 = **持久化语义**:filter 纯内存(缺省即默认,decode 可全量填默认);
// 而 view-pref 是 uiStore 经 app_config 持久化的偏好。用户裁决「URL 权威、覆盖持久值」→ 语义是:
//   - URL **存在**某键 → 以该值覆盖 persist(URL 权威);
//   - URL **缺失**某键 → 保持 persist 的值(不覆盖)。
// 故 decode 返回 **Partial**(只含 URL 中出现且合法的键),调用方只对出现的键赋值,缺失键不动 persist。
// encode 只输出非默认值,使默认视图对应干净 URL(与 galleryQuery 同姿态)。
//
// 防御式:值是外部输入(URL 可深链/手改),枚举一律白名单,损坏值当作「未提供」丢弃(回落 persist)。

import type { LocationQuery } from 'vue-router'

/** 本模块管理的 query 键。 */
export const VIEW_PREF_QUERY_KEYS = ['group', 'sort', 'order', 'layout'] as const

export type GroupBy = 'date' | 'folder' | 'none'
export type SortWithin = 'datetime' | 'filename' | 'similarity'
export type SortOrder = 'asc' | 'desc'
export type LayoutMode = 'justified' | 'grid'

/** view-pref 完整快照(供 encode 读当前 uiStore 值)。 */
export interface ViewPrefSnapshot {
  groupBy: GroupBy
  sortWithinGroup: SortWithin
  sortOrder: SortOrder
  layoutMode: LayoutMode
}

/** decode 结果:只含 URL 中出现且合法的键(URL 权威覆盖,缺失键保持 persist)。 */
export interface PartialViewPref {
  groupBy?: GroupBy
  sortWithinGroup?: SortWithin
  sortOrder?: SortOrder
  layoutMode?: LayoutMode
}

/** 各维度默认值(与 uiStore ref 初值一致)。encode 据此裁剪默认值,decode 与之无关。 */
export const VIEW_PREF_DEFAULTS: ViewPrefSnapshot = {
  groupBy: 'date',
  sortWithinGroup: 'datetime',
  sortOrder: 'desc',
  layoutMode: 'justified',
}

/** LocationQuery 值取首字符串(数组取首,null 归 undefined)。 */
function firstStr(v: LocationQuery[string]): string | undefined {
  if (v == null) return undefined
  return Array.isArray(v) ? (v[0] ?? undefined) : v
}

/** 编码:当前快照 → query 对象。仅输出非默认字段,默认视图对应干净 URL。 */
export function encodeViewPref(s: ViewPrefSnapshot): Record<string, string> {
  const q: Record<string, string> = {}
  if (s.groupBy !== VIEW_PREF_DEFAULTS.groupBy) q.group = s.groupBy
  if (s.sortWithinGroup !== VIEW_PREF_DEFAULTS.sortWithinGroup) q.sort = s.sortWithinGroup
  if (s.sortOrder !== VIEW_PREF_DEFAULTS.sortOrder) q.order = s.sortOrder
  if (s.layoutMode !== VIEW_PREF_DEFAULTS.layoutMode) q.layout = s.layoutMode
  return q
}

/**
 * 解码:query → Partial。仅纳入 URL 中出现且通过白名单校验的键;缺失/非法键**不出现在结果里**
 * (调用方据此保持 persist 不覆盖)。这与 filter 的「缺失即回落默认」不同——见文件头注的持久化语义差异。
 */
export function decodeViewPref(query: LocationQuery): PartialViewPref {
  const out: PartialViewPref = {}
  const group = firstStr(query.group)
  if (group === 'date' || group === 'folder' || group === 'none') out.groupBy = group
  const sort = firstStr(query.sort)
  if (sort === 'datetime' || sort === 'filename' || sort === 'similarity') out.sortWithinGroup = sort
  const order = firstStr(query.order)
  if (order === 'asc' || order === 'desc') out.sortOrder = order
  const layout = firstStr(query.layout)
  if (layout === 'justified' || layout === 'grid') out.layoutMode = layout
  return out
}
