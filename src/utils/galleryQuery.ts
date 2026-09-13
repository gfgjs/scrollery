// src/utils/galleryQuery.ts
// Gallery 视图的 URL query 编解码（S2-b）。纯函数,无 Vue/store 依赖,便于单测。
//
// 职责:把 filterStore 的筛选状态在「store 快照」与「URL query」之间双向映射。decode 处理的是
// 外部输入（URL 可被用户手改/深链传入）,必须防御式解析——白名单枚举、clamp 数值、拒绝非法,
// 任何损坏值回落默认而非直灌类型化字段（对齐 uiStore 启动配置解析的既有姿态）。

import type { LocationQuery } from 'vue-router'
import type { MediaType } from '../types/media'

/** 本模块管理的 query 键。writeUrl 只覆写这些键,其余键（未来 search/pref）原样保留。 */
export const FILTER_QUERY_KEYS = [
  'types',
  'formats',
  'favorite',
  'live',
  'rating',
  'color',
  'from',
  'to',
] as const

/**
 * 当前 UI 允许切换的媒体类型 —— **四大类全部**（S 线 D-008 起文档/音频亦为常显 chip）。decode 白名单据此。
 *
 * 🔴 必须与 `GalleryFilterChips` 提供的类型 chip 同步：encode 把 store 里有什么写什么，decode 却按
 * 本表过滤 —— 白名单漏一类，用户点了「文档」再刷新页面，筛选**静默消失**（URL 里明明写着
 * `types=document`）。这是不对称的经典形态：写侧宽、读侧窄，中间没有任何报错。
 *
 * 类型标成 `MediaType[]` 而非 `string[]`：拼错 `'documents'` 当场编译失败，而不是变成一个
 * 永远匹配不上的死值。
 */
const MEDIA_TYPE_WHITELIST: readonly MediaType[] = ['image', 'video', 'document', 'audio']

/** URL 里的一段文本是否是合法媒体类型。写成谓词，使过滤后的数组自带 `MediaType` 类型。 */
function isMediaType(t: string): t is MediaType {
  return (MEDIA_TYPE_WHITELIST as readonly string[]).includes(t)
}

/**
 * 格式列表归一：形态校验 + 去重 + 截断到上界。**编解码两侧共用**，往返因此闭合。
 *
 * 有意**不校验「是否是已注册格式」**：registry 是运行时并集（内置 ∪ Catalog），随插件安装变化，
 * 而 URL 编解码是纯函数、拿不到也不该拿 Catalog。放一个未注册的扩展名进来的后果只是
 * `file_format IN ('xyz')` 筛出零条 —— 诚实的空结果，不是安全问题。形态与数量有界才是防线。
 */
function sanitizeFormats(raw: readonly string[]): string[] {
  const out: string[] = []
  for (const f of raw) {
    const v = f.trim()
    if (!FORMAT_RE.test(v) || out.includes(v)) continue
    out.push(v)
    if (out.length >= MAX_FORMATS) break
  }
  return out
}

/** 评分 / 颜色档的合法上界（minRating 0–5 星,colorLabel 0–7 档,0 均表示不筛选）。 */
const MAX_RATING = 5
const MAX_COLOR = 7

/**
 * 格式扩展名的合法形态与数量上界（S 线 §8）。
 *
 * URL 是外部输入：`formats=` 后面可以是任意长度的任意字节。有界 + 形态校验是防线本身 ——
 * 每个扩展名都会变成一个 SQL 绑定参数，不设上界等于让 URL 决定 `IN (...)` 有多长。
 *
 * `[a-z0-9]{1,16}` 与后端 Catalog loader 的 `is_valid_format` 同形（`catalog.rs:323`）：
 * 那边是**拒绝**非小写而非转换，这边同样只拒不改 —— 两侧对「什么是合法扩展名」必须同一把尺子，
 * 否则 URL 放进来的东西后端不认，或反之。
 *
 * 上界 64：registry 当前 67 项，全选也就这个量级；给个明确上界胜过让它无界。
 */
const FORMAT_RE = /^[a-z0-9]{1,16}$/
const MAX_FORMATS = 64

/** filterStore 的可序列化快照（与 URL 双向映射的中间形态）。 */
export interface GalleryFilterSnapshot {
  mediaTypes: string[]
  /** 细分格式：规范化小写扩展名（S 线 D-011）。`group` 是 UI 概念，不进 URL。 */
  fileFormats: string[]
  favoritedOnly: boolean
  livePhotoOnly: boolean
  minRating: number
  colorLabel: number
  /** Unix epoch「秒」（与 sort_datetime 同单位,见 GalleryFilterChips）。 */
  dateFrom: number | null
  dateTo: number | null
}

/**
 * MediaGrid 承载的画廊路由判据（与 App.vue showGalleryToolbar 同源:主库/收藏/回收站/
 * 目录/收藏夹内容/人物内容）。列表页 `/collections`、`/persons` 无 MediaGrid,故用带尾斜杠
 * 的前缀区分详情。查看器 / 设置 / 插件不在此列。
 */
export function isGalleryRoute(path: string): boolean {
  return (
    path === '/' ||
    path === '/favorites' ||
    path === '/live-photos' ||
    path === '/recent' ||
    path === '/trash' ||
    path.startsWith('/folder/') ||
    path.startsWith('/collections/') ||
    path.startsWith('/persons/')
  )
}

/**
 * 编码:store 快照 → query 对象。仅输出非默认字段,使干净视图对应干净 URL。
 *
 * 🔴 **写侧也过白名单**（S 线 §8 点名要修的非对称）。原先 encode 是「store 里有什么写什么」，
 * decode 却按白名单收 —— 写宽读窄，中间没有任何报错，症状是 URL 里赫然写着一个刷新后就消失的
 * 筛选。两侧同一把尺子之后，写出去的 URL 就**保证**读得回来（往返闭合），这条性质本身可测。
 */
export function encodeGalleryFilters(s: GalleryFilterSnapshot): Record<string, string> {
  const q: Record<string, string> = {}
  const types = s.mediaTypes.filter(isMediaType)
  if (types.length > 0) q.types = types.join(',')
  const formats = sanitizeFormats(s.fileFormats)
  if (formats.length > 0) q.formats = formats.join(',')
  if (s.favoritedOnly) q.favorite = '1'
  if (s.livePhotoOnly) q.live = '1'
  if (s.minRating > 0) q.rating = String(s.minRating)
  if (s.colorLabel > 0) q.color = String(s.colorLabel)
  if (s.dateFrom != null) q.from = String(s.dateFrom)
  if (s.dateTo != null) q.to = String(s.dateTo)
  return q
}

/** LocationQuery 值可能是 string | null | 数组;取首个字符串值,其余归 undefined。 */
function firstStr(v: LocationQuery[string]): string | undefined {
  if (v == null) return undefined
  if (Array.isArray(v)) {
    const head = v[0]
    return head == null ? undefined : head
  }
  return v
}

/** 严格整数解析 + clamp。非整数 / 非有限值一律回落 0（=不筛选）。 */
function clampInt(raw: string | undefined, max: number): number {
  if (raw == null || raw === '') return 0
  const n = Number(raw)
  if (!Number.isInteger(n)) return 0
  if (n < 0) return 0
  return n > max ? max : n
}

/** 时间戳解析:仅接受有限整数(epoch 秒),否则 null。拒绝 '12abc' / 小数等垃圾。 */
function parseTs(raw: string | undefined): number | null {
  if (raw == null || raw === '') return null
  const n = Number(raw)
  return Number.isInteger(n) ? n : null
}

/** 解码:query → store 快照。防御式——枚举白名单、数值 clamp、非法回落默认。 */
export function decodeGalleryFilters(query: LocationQuery): GalleryFilterSnapshot {
  const rawTypes = (firstStr(query.types) ?? '')
    .split(',')
    .map((t) => t.trim())
    .filter(isMediaType)
  // 去重,保持顺序稳定（供快照相等判定）。
  const mediaTypes = [...new Set(rawTypes)]

  // 格式与 types 共用 sanitizeFormats（编解码同一把尺子 → 往返闭合）。
  const fileFormats = sanitizeFormats((firstStr(query.formats) ?? '').split(','))

  return {
    mediaTypes,
    fileFormats,
    favoritedOnly: firstStr(query.favorite) === '1',
    livePhotoOnly: firstStr(query.live) === '1',
    minRating: clampInt(firstStr(query.rating), MAX_RATING),
    colorLabel: clampInt(firstStr(query.color), MAX_COLOR),
    dateFrom: parseTs(firstStr(query.from)),
    dateTo: parseTs(firstStr(query.to)),
  }
}

/** 快照相等判定（供 echo-guard:URL→store 回填前比对,相等则跳过,断开双向同步回环）。 */
export function galleryFilterSnapshotEqual(
  a: GalleryFilterSnapshot,
  b: GalleryFilterSnapshot,
): boolean {
  // ⚠ 新增筛选维度**必须同步这里**。本函数是读字段、不构造对象,故 TypeScript **不会**因为
  // GalleryFilterSnapshot 加了字段就在这里报错 —— 加字段时 tsc 精确列出的那几处并不包含它。
  // 漏了的症状:echo-guard 认为两个快照相等 → 跳过 URL→store 回填 → 深链里的该维度永不生效,
  // 且没有任何报错。(S 线 P3 加 fileFormats 时实测:tsc 报了 4 处,唯独漏掉这里。)
  return (
    a.mediaTypes.length === b.mediaTypes.length &&
    a.mediaTypes.every((t, i) => t === b.mediaTypes[i]) &&
    a.fileFormats.length === b.fileFormats.length &&
    a.fileFormats.every((f, i) => f === b.fileFormats[i]) &&
    a.favoritedOnly === b.favoritedOnly &&
    a.livePhotoOnly === b.livePhotoOnly &&
    a.minRating === b.minRating &&
    a.colorLabel === b.colorLabel &&
    a.dateFrom === b.dateFrom &&
    a.dateTo === b.dateTo
  )
}
