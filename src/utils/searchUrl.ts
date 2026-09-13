// src/utils/searchUrl.ts
// 搜索状态（模式/范围/已提交查询）的 URL query 编解码（S2-b2 Stage 3b）。纯函数,无 Vue/store 依赖。
//
// 搜索是**全局**态(同 filter):searchMode/searchScope/committedQuery 跨视图携带,URL 表达其在当前路径
// 的投影;URL→store 仅初次水合恢复(恢复会经 searchStore.apply 触发真实查询,含语义 IPC)。
//
// 键:mode(mixed 默认 / semantic / normal)、scope(仅 normal 生效,filename 默认)、q(已提交查询)。
// encode 只输出非默认(mixed/filename/空 q 均省略),使无搜索的干净视图对应干净 URL。
// decode 防御式:mode/scope 白名单枚举,损坏值丢弃;q 接受任意非空串(用户查询文本)。

import type { LocationQuery } from 'vue-router'
import type { SearchMode } from '../types/ai'

/** 本模块管理的 query 键。 */
export const SEARCH_QUERY_KEYS = ['q', 'scope', 'mode'] as const

/** searchScope 合法值(与 AppToolbar 的 scope 下拉一致)。 */
const SCOPE_WHITELIST: readonly string[] = [
  'filename',
  'folder',
  'date',
  'device',
  'location',
  'global',
]

const DEFAULT_MODE: SearchMode = 'mixed'
const DEFAULT_SCOPE = 'filename'

/** 搜索快照(供 encode 读 searchStore 读表面)。 */
export interface SearchSnapshot {
  mode: SearchMode
  scope: string
  query: string
}

/** decode 结果:只含 URL 出现且合法的键(供恢复;缺失键不触碰对应状态)。 */
export interface PartialSearch {
  mode?: SearchMode
  scope?: string
  query?: string
}

/** LocationQuery 值取首字符串（数组取首,null 归 undefined）。 */
function firstStr(v: LocationQuery[string]): string | undefined {
  if (v == null) return undefined
  return Array.isArray(v) ? (v[0] ?? undefined) : v
}

/** 编码:快照 → query。仅输出非默认(mixed/filename/空 q 省略)。 */
export function encodeSearch(s: SearchSnapshot): Record<string, string> {
  const q: Record<string, string> = {}
  if (s.mode !== DEFAULT_MODE) q.mode = s.mode
  if (s.scope !== DEFAULT_SCOPE) q.scope = s.scope
  if (s.query.trim() !== '') q.q = s.query
  return q
}

/**
 * 解码:query → Partial。mode/scope 白名单校验,非法丢弃;q 取非空原串。
 * 缺失/非法键不出现在结果里,恢复时不触碰对应状态。
 */
export function decodeSearch(query: LocationQuery): PartialSearch {
  const out: PartialSearch = {}
  const mode = firstStr(query.mode)
  if (mode === 'mixed' || mode === 'semantic' || mode === 'normal') out.mode = mode
  const scope = firstStr(query.scope)
  if (scope !== undefined && SCOPE_WHITELIST.includes(scope)) out.scope = scope
  const q = firstStr(query.q)
  if (q !== undefined && q !== '') out.query = q
  return out
}
