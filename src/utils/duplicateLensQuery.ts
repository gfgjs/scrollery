// src/utils/duplicateLensQuery.ts
// 重复镜头（2026-09-02 方案 §4.4）的 URL query 编解码。纯函数,无 Vue/store 依赖,便于单测。
//
// 与 galleryQuery.ts 同一防御式姿态:URL 是外部输入（可手改/深链）,枚举一律白名单、开关只认
// 字面 '1',任何垃圾值回落「镜头关闭」而非直灌类型化字段。镜头是临时状态:关闭时 URL 必须干净
// （无 duplicates 键）,故 encode(null) → 空对象、normalize 把非法/冗余键直接移除而非留垃圾。

import type { LocationQuery } from 'vue-router'
import type { DuplicateLensModeDto } from '../types/view'

/** 本模块管理的 query 键（§4.4）。已并入 useGalleryQuerySync 的 MANAGED 集（store→URL 投影）。 */
export const DUPLICATE_LENS_QUERY_KEYS = ['duplicates', 'duplicateUnique'] as const

/** 重复镜头的 URL 状态（mode=null = 镜头关闭,即普通画廊）。 */
export interface DuplicateLensQueryState {
  /** null = 镜头关闭（URL 无 duplicates 键）。 */
  mode: DuplicateLensModeDto | null
  /** 仅 folders 模式下有意义；groups/off 恒 false。 */
  showUniqueItems: boolean
}

/** LocationQuery 值取首字符串(数组取首,null 归 undefined)。与 galleryQuery/viewPrefQuery 同款。 */
function firstStr(v: LocationQuery[string]): string | undefined {
  if (v == null) return undefined
  return Array.isArray(v) ? (v[0] ?? undefined) : v
}

/** duplicates 键的白名单解析:非 groups/folders（含空串/垃圾/null）一律视为镜头关闭。 */
function parseMode(raw: string | undefined): DuplicateLensModeDto | null {
  return raw === 'groups' || raw === 'folders' ? raw : null
}

/**
 * 解析 URL query 中的重复镜头键（duplicates / duplicateUnique）。
 * 容忍任意垃圾输入：duplicates 值非 groups/folders 视为关闭；
 * duplicateUnique 只认字面 '1' 且仅在 folders 模式生效（§4.4）。
 */
export function parseDuplicateLensQuery(query: LocationQuery): DuplicateLensQueryState {
  const mode = parseMode(firstStr(query.duplicates))
  return {
    mode,
    showUniqueItems: mode === 'folders' && firstStr(query.duplicateUnique) === '1',
  }
}

/**
 * 规范化:返回写侧 flat query（Record<string,string>,与 useGalleryQuerySync.writeUrl 的
 * preserved/encoded 同形）——非管理键取首字符串值原样保留,两个管理键按 §4.4 裁剪：
 *   - duplicates 值非法（垃圾/空串/数组残片）→ 移除该键;
 *   - duplicateUnique 仅在 duplicates=folders 且字面 '1' 时保留,其余一律移除
 *     （含 groups 模式 / 镜头关闭 / 值非 '1'）。
 * 幂等:normalize(normalize(q)) 深等于 normalize(q)。
 */
export function normalizeDuplicateLensQuery(query: LocationQuery): Record<string, string> {
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(query)) {
    if (k === 'duplicates' || k === 'duplicateUnique') continue
    const s = firstStr(v)
    if (s !== undefined) out[k] = s
  }
  const mode = parseMode(firstStr(query.duplicates))
  if (mode !== null) {
    out.duplicates = mode
    if (mode === 'folders' && firstStr(query.duplicateUnique) === '1') out.duplicateUnique = '1'
  }
  return out
}

/**
 * 编码为 URL query 参数对象（Record<string,string>,与 encodeGalleryFilters 同形）:
 * mode=null → 空对象;folders+showUnique → {duplicates:'folders', duplicateUnique:'1'}。
 * 写侧同样过白名单:groups/off 状态即使误带 showUniqueItems=true 也不写出 duplicateUnique,
 * 保证「写出去的 URL 必然读得回来」（往返闭合,对齐 galleryQuery 的写读同尺纪律）。
 */
export function encodeDuplicateLensQuery(state: DuplicateLensQueryState): Record<string, string> {
  if (state.mode === null) return {}
  const q: Record<string, string> = { duplicates: state.mode }
  if (state.mode === 'folders' && state.showUniqueItems) q.duplicateUnique = '1'
  return q
}
