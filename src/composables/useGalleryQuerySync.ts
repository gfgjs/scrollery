// src/composables/useGalleryQuerySync.ts
// Gallery 视图状态 ↔ URL query 双向同步(S2-b/S2-b2)。挂载于 App.vue(不随 KeepAlive 波动),仅画廊路由生效。
//
// 同步维度:①filter(S2-b,全局态、零 persist;字段 types/favorite/live/rating/color/from/to)
// ②view-pref(S2-b2,uiStore 经 app_config 持久化;字段 group/sort/order/layout)③search(S2-b2,
// 恢复经 searchStore.apply 触发真实查询,含语义 IPC;字段 q/scope/mode)④duplicate lens(2026-09-02
// 重复项方案 §10.1;字段 duplicates/duplicateUnique)。恢复顺序 filter→view-pref→search→lens,
// 使 setMode 的语义 group 覆盖发生在 view-pref 之后,让 aiStore.previousGroupBy 捕获的是 URL/persist
// 的真实 group 而非默认。**search 恢复触发 IPC 属真机验收重点**。
// **镜头键是唯一「URL 是事实源、store 是镜像」的键组**:水合之外的路由 query 变化也会回灌
// duplicateLensStore(见下方 route.query watcher);其余键组维持「URL→store 仅初次水合」。
//
// 同步模型:filterStore 是全局态,URL 表达「全局筛选在当前路径的投影」;store→URL 连续写,URL→store 仅初次
// 水合。回环用值相等守卫 + hydrated 门双重断开。
//
// view-pref 差异:①持久化语义——用户裁决「URL 权威、覆盖持久值」,decode 只返回 Partial,applyViewPref
// 只对出现的键赋值且 persist=false(缺失键保持 persist;persist=false 是因为 URL 恢复不是新的持久动作,
// 不应改动持久默认);故 hydration 门须等 startupConfigPromise,否则 persist 的 .then 晚到
// 会覆盖已从 URL 水合的值。②persist=false 的临时态不写 URL——语义搜索(aiStore.isSemanticMode 为真时)
// 把 groupBy/sort 临时改 'none'/'similarity' 时,writeUrl 删掉这两键,避免刷新以 URL 覆盖 persist、污染
// aiStore.previousGroupBy。
//
// 用 router.replace(非 push)写 URL,状态变化不污染历史。

import { watch, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { LocationQuery } from 'vue-router'
import type { MediaType } from '../types/media'
import { useFilterStore } from '../stores/filterStore'
import { useUiStore } from '../stores/uiStore'
import { useAiStore } from '../stores/aiStore'
import { useSearchStore } from '../stores/searchStore'
import {
  FILTER_QUERY_KEYS,
  isGalleryRoute,
  encodeGalleryFilters,
  decodeGalleryFilters,
  galleryFilterSnapshotEqual,
  type GalleryFilterSnapshot,
} from '../utils/galleryQuery'
import {
  VIEW_PREF_QUERY_KEYS,
  encodeViewPref,
  decodeViewPref,
  type ViewPrefSnapshot,
} from '../utils/viewPrefQuery'
import {
  SEARCH_QUERY_KEYS,
  encodeSearch,
  decodeSearch,
  type SearchSnapshot,
} from '../utils/searchUrl'
import {
  DUPLICATE_LENS_QUERY_KEYS,
  parseDuplicateLensQuery,
  encodeDuplicateLensQuery,
  normalizeDuplicateLensQuery,
} from '../utils/duplicateLensQuery'
import { useDuplicateLensStore } from '../stores/duplicateLensStore'
import { useViewStore } from '../stores/viewStore'

const MANAGED = new Set<string>([
  ...FILTER_QUERY_KEYS,
  ...VIEW_PREF_QUERY_KEYS,
  ...SEARCH_QUERY_KEYS,
  // 重复镜头键(§4.4):MANAGED 意味着「写侧只从 store 投影,读侧水合进 store」,preserved 不保留
  // 它们——镜头关闭时 encode 输出空对象,残留的垃圾键随下一次 writeUrl 自然消失。
  ...DUPLICATE_LENS_QUERY_KEYS,
])

/** 取 LocationQuery 值的首字符串（数组取首,null 归 undefined）,供相等比对与非管理键保留。 */
function firstStr(v: LocationQuery[string]): string | undefined {
  if (v == null) return undefined
  return Array.isArray(v) ? (v[0] ?? undefined) : v
}

export function useGalleryQuerySync(): void {
  const route = useRoute()
  const router = useRouter()
  const filter = useFilterStore()
  const ui = useUiStore()
  const ai = useAiStore()
  const search = useSearchStore()
  const lens = useDuplicateLensStore()
  const view = useViewStore()

  function snapshot(): GalleryFilterSnapshot {
    return {
      mediaTypes: [...filter.mediaTypes],
      fileFormats: [...filter.fileFormats],
      favoritedOnly: filter.favoritedOnly,
      livePhotoOnly: filter.livePhotoOnly,
      minRating: filter.minRating,
      colorLabel: filter.colorLabel,
      dateFrom: filter.dateFrom,
      dateTo: filter.dateTo,
    }
  }

  function applySnapshot(s: GalleryFilterSnapshot) {
    // setMediaTypes/setFileFormats 做引用替换 → filter.apiFilterKey 重算 → useJustifiedLayout 的
    // 重算 watch 命中。
    // decodeGalleryFilters 已按 MediaType 白名单收敛；快照仍保留 string[] 兼容旧纯函数调用方。
    filter.setMediaTypes(s.mediaTypes as MediaType[])
    filter.setFileFormats(s.fileFormats)
    filter.favoritedOnly = s.favoritedOnly
    filter.livePhotoOnly = s.livePhotoOnly
    filter.minRating = s.minRating
    filter.colorLabel = s.colorLabel
    filter.dateFrom = s.dateFrom
    filter.dateTo = s.dateTo
  }

  function vpSnapshot(): ViewPrefSnapshot {
    return {
      groupBy: ui.groupBy,
      sortWithinGroup: ui.sortWithinGroup,
      sortOrder: ui.sortOrder,
      layoutMode: ui.layoutMode,
    }
  }

  /** URL→store 应用 view-pref:只对 URL 出现的键赋值,且 persist=false(URL 权威、不改持久默认)。 */
  function applyViewPref(query: LocationQuery) {
    const vp = decodeViewPref(query)
    if (vp.groupBy !== undefined) ui.setGroupBy(vp.groupBy, false)
    if (vp.sortWithinGroup !== undefined) ui.setSortWithinGroup(vp.sortWithinGroup, false)
    // sortOrder/layoutMode 直接赋 ref(sortOrder 本无 persist;layoutMode 走直接赋值避开 setLayoutMode 的
    // 持久化——URL 恢复不应写持久配置)。exposed ref 直接赋值是既有范式(GalleryViewControls 亦如此改 sortOrder)。
    if (vp.sortOrder !== undefined) ui.sortOrder = vp.sortOrder
    if (vp.layoutMode !== undefined) ui.layoutMode = vp.layoutMode
  }

  function searchSnapshot(): SearchSnapshot {
    return { mode: search.mode, scope: search.scope, query: search.committedQuery }
  }

  /**
   * URL→store 应用 search:mode 先(setMode 经 aiStore 协调 group/sort 与子类型复位)→ scope →
   * apply(按当前模式路由到真实执行,语义走 IPC)。仅初次水合调用。**恢复会触发查询执行(含语义 IPC),
   * 是真机验收重点**——门禁只能验编解码纯函数,验不了 IPC 实际行为。
   */
  function applySearch(query: LocationQuery) {
    const s = decodeSearch(query)
    if (s.mode !== undefined) search.setMode(s.mode)
    if (s.scope !== undefined) search.setScope(s.scope)
    if (s.query !== undefined) search.apply(s.query)
  }

  /** route.query 与目标 flat map 是否等价（仅字符串键值,数组取首值）。 */
  function sameQuery(current: LocationQuery, target: Record<string, string>): boolean {
    const curKeys = Object.keys(current)
    const tgtKeys = Object.keys(target)
    if (curKeys.length !== tgtKeys.length) return false
    for (const k of tgtKeys) {
      if (firstStr(current[k]) !== target[k]) return false
    }
    return true
  }

  // store → URL:把当前 filter + view-pref + search + 镜头键投影到当前画廊路径的 query。
  function writeUrl() {
    if (!isGalleryRoute(route.path)) return
    const encoded: Record<string, string> = {
      ...encodeGalleryFilters(snapshot()),
      ...encodeViewPref(vpSnapshot()),
      ...encodeSearch(searchSnapshot()),
      // 镜头键(§10.1 URL 是事实源):store.mode 是 URL 的镜像,这里的 encode 是**保持共存**的
      // 重投影——镜头激活期间任何其他键变化(filter/view-pref/search 触发的 writeUrl)都必须把
      // duplicates/duplicateUnique 原样写回,否则镜头键会从 URL 上被抹掉。镜头关闭时输出空对象,
      // 不写这两个键。
      ...encodeDuplicateLensQuery({ mode: lens.mode, showUniqueItems: lens.showUniqueItems }),
    }
    // 镜像 persist=false:语义结果激活时 groupBy/sort 是临时覆盖,不写入 URL(见文件头注)。
    if (ai.isSemanticMode) {
      delete encoded.group
      delete encoded.sort
    }
    const preserved: Record<string, string> = {}
    for (const [k, v] of Object.entries(route.query)) {
      if (!MANAGED.has(k)) {
        const s = firstStr(v)
        if (s !== undefined) preserved[k] = s
      }
    }
    const next = { ...preserved, ...encoded }
    if (sameQuery(route.query, next)) return
    void router.replace({ query: next })
  }

  /**
   * URL→store 的镜头键同步。镜头是唯一「URL 是事实源、store 是镜像」的键组（§10.1）——路由变化
   * （enterLens/exitLens 的 push、setMode 的 replace、浏览器返回、深链）一律先收敛进 store。
   * filter/view-pref/search 仍维持「URL→store 仅初次水合」,此处不碰。
   * 赋值前做相等守卫:duplicateLensStore 的动作已乐观写 store,push 落地后的这次同步应为零写入,
   * 天然断开回环。
   */
  function syncLensFromUrl(query: LocationQuery) {
    const parsed = parseDuplicateLensQuery(query)
    if (lens.mode !== parsed.mode) lens.mode = parsed.mode
    if (lens.showUniqueItems !== parsed.showUniqueItems) lens.showUniqueItems = parsed.showUniqueItems
  }

  /**
   * 镜头键垃圾清理(§4.4「其他组合规范化移除」):URL 中两个镜头键与规范形(normalizeDuplicateLensQuery)
   * 不一致(垃圾值/非法组合/孤儿 duplicateUnique)时,发一次 replace 把 query 规范化为展平形态——
   * 非管理键按首值保留,两个镜头键按 §4.4 裁剪。规范形时零导航。
   */
  function normalizeLensInUrl() {
    const q = route.query
    const canonical = normalizeDuplicateLensQuery(q)
    if (
      (firstStr(q.duplicates) ?? undefined) === canonical.duplicates &&
      (firstStr(q.duplicateUnique) ?? undefined) === canonical.duplicateUnique
    ) {
      return
    }
    // 非规范形 → 重写为规范 query;键的摘取/展平/规范键并回全部复用共享纯函数
    // (与 store 写侧同一实现,勿在此内联复制——F-021 双源漂移告诫)。
    void router.replace({ query: canonical })
  }

  // URL → store:仅初次水合调用。filter 相等则跳过(断开回环);view-pref 只覆盖 URL 出现的键。
  function readUrl() {
    if (!isGalleryRoute(route.path)) return
    const decoded = decodeGalleryFilters(route.query)
    if (!galleryFilterSnapshotEqual(decoded, snapshot())) applySnapshot(decoded)
    applyViewPref(route.query)
    applySearch(route.query)
    // 镜头键:水合即同步(深链 ?duplicates=groups 直开镜头),顺带规范化垃圾输入。
    syncLensFromUrl(route.query)
    normalizeLensInUrl()
  }

  let hydrated = false

  // 单一 writeUrl 驱动源:filter/view-pref 任一字段变或画廊路径变即触发,hydrated 前不写。
  //
  // ⚠ 本列表是手工枚举,必须与 snapshot()/GalleryFilterSnapshot 字段集保持一致——漏一项无
  // 编译/测试信号,只表现为该筛选 URL 停旧值、刷新静默丢失(R-02:8161b12 漏了 fileFormats,
  // 即 F-021「修枚举点时另一处同形枚举复发」)。有意不收敛为 apiFilterKey:单端日期(from/to)
  // 也要写 URL,而 apiFilterKey 有意吞掉单端变化。
  watch(
    [
      () => route.path,
      () => filter.mediaTypes,
      () => filter.fileFormats,
      () => filter.favoritedOnly,
      () => filter.livePhotoOnly,
      () => filter.minRating,
      () => filter.colorLabel,
      () => filter.dateFrom,
      () => filter.dateTo,
      () => ui.groupBy,
      () => ui.sortWithinGroup,
      () => ui.sortOrder,
      () => ui.layoutMode,
      () => search.mode,
      () => search.scope,
      () => search.committedQuery,
    ],
    () => {
      if (hydrated) writeUrl()
    },
  )

  // 镜头键的 URL→store 同步(水合之后):任何来源的路由 query 变化(本模块 writeUrl 的 replace、
  // duplicateLensStore 动作的 push/replace、浏览器返回、侧栏/深链导航)都先按 §10.1 收敛进 store,
  // 再规范化 URL 上的垃圾镜头键。writeUrl 自身的 replace 也会触发本 watcher——此时 parse 结果与
  // store 相等、规范形一致,两次守卫都零写入,回环自然终止。
  watch(
    () => route.query,
    (q) => {
      if (!hydrated || !isGalleryRoute(route.path)) return
      syncLensFromUrl(q)
      normalizeLensInUrl()
    },
  )

  // 初次水合置于 router ready 且 startupConfigPromise resolve 之后:前者保证 route.query 为深链
  // 真实值,后者保证 persist 已先落 uiStore(URL 出现的键随后覆盖=URL 权威)。startupConfigPromise
  // 失败也放行(catch→null),不因配置 IPC 失败卡死 URL 同步。nextTick 再退一拍,避开 persist 赋值
  // 微任务的 flush 顺序依赖。
  void Promise.all([router.isReady(), ui.startupConfigPromise.catch(() => null)])
    .then(() => nextTick())
    .then(() => {
      readUrl()
      hydrated = true
      view.galleryQueryReady = true
    })
}
