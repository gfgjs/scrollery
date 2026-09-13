// src/stores/duplicateLensStore.ts
// 主画廊重复镜头状态（docs/designs/2026-09-02-主画廊重复项浏览方案.md §10.1）。pinia id 'duplicate-lens'。
//
// 红线（方案原文，勿凭直觉放松）：
// - **URL 是镜头模式的事实源**：本 store 的 mode/showUniqueItems 只是路由 query 的镜像。一切变更
//   必须经本文件的动作落到 URL（enterLens/exitLens 用 push 制造历史节点，setMode/setShowUniqueItems
//   用 replace 不制造节点，§4.2/§4.3），再由 useGalleryQuerySync 的 URL→store 同步收敛；浏览器返回、
//   深链等外部导航改 URL 时同样由该同步回写本 store。禁止绕过 URL 直接改 mode。
// - **return snapshot 只属当前会话**：returnFullPath 不入 URL、不持久化，退出镜头
//   即清空；深链直开镜头（无快照）时退出回普通「/」（§4.3）。滚动位的恢复不走快照：退出后普通
//   画廊的重算链（layoutVersion watcher）自己从 scrollCache 普通键读回进入前的滚动位（缓存是
//   模块级 Map,镜头会话不触碰普通键）,快照无需另存滚动/视图键,故不设 §10.1 草案里的
//   returnAnchorItemId/returnViewKey 字段（P2 收口裁定,见 docs/designs/2026-09-02-主画廊重复项浏览方案.md）。
// - **大数组/成员列表绝不进本 store**（§10.1）——成员/文件夹图留在后端布局缓存与 mediaStore/layout。

import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { LocationQueryRaw } from 'vue-router'
import type { DuplicateLensModeDto } from '../types/view'
import {
  DUPLICATE_LENS_QUERY_KEYS,
  encodeDuplicateLensQuery,
} from '../utils/duplicateLensQuery'
// 直接复用应用路由单例：store 动作在组件事件回调外也可能触发（键盘/深链恢复），
// useRouter() 的注入依赖 setup 上下文不可靠；router/index.ts 不反向依赖任何 store，无环。
import router from '../router'

export const useDuplicateLensStore = defineStore('duplicate-lens', () => {
  /** null = 镜头关闭（普通画廊）。与 URL ?duplicates=… 镜像。 */
  const mode = ref<DuplicateLensModeDto | null>(null)
  /** 仅 folders 模式有意义；groups/off 恒 false（§4.4 白名单）。与 URL ?duplicateUnique=1 镜像。 */
  const showUniqueItems = ref(false)
  /**
   * §8.1 browse-only 谓词（镜头是否激活）：SelectionToolbar/checkbox/快捷动作/框选/右键/
   * Ctrl+A 等全部 browse-only 阻断点的统一开关,勿在各阻断点重写 `mode !== null`。
   */
  const isLensActive = computed(() => mode.value !== null)

  // ── 返回快照（§10.1,会话内一次性）────────────────────────────────────────────
  /** 进入镜头前的完整路由（含普通筛选 query），exitLens 原样 push 回去。 */
  const returnFullPath = ref<string | null>(null)

  // 快照不变量（§4.3「return snapshot 只属当前会话」）:mode 归 null 即清场。exitLens 的主动
  // 退出已先行清空,此处幂等;系统返回/深链重定向等**不经 exitLens** 的路径（useGalleryQuerySync
  // 的 URL→store 同步把 mode 收敛为 null）由本守卫兜住,防陈旧快照被下一次镜头会话的 exitLens
  // 误消费（否则深链进镜头后退出会被推回上一会话的旧路由）。
  // flush:'sync'——不依赖批处理边界:同一 flush 内 null→X→null 的往返(去重 watcher 会因
  // 新旧值相等跳过回调)也逐次清场,不变量在任何写入序列下都即时成立;回调只写一个 ref,零开销。
  watch(
    mode,
    (m) => {
      if (m !== null) return
      returnFullPath.value = null
    },
    { flush: 'sync' },
  )

  /** 当前 query 去掉镜头两键后，合并规范编码的镜头键（普通筛选/视图偏好/search 等键原样共存,§4.4）。 */
  function mergedLensQuery(next: { mode: DuplicateLensModeDto | null; showUniqueItems: boolean }) {
    const merged: Record<string, string | null | (string | null)[]> = {}
    for (const [k, v] of Object.entries(router.currentRoute.value.query)) {
      if ((DUPLICATE_LENS_QUERY_KEYS as readonly string[]).includes(k)) continue
      merged[k] = v
    }
    return { ...merged, ...encodeDuplicateLensQuery(next) } as LocationQueryRaw
  }

  /** enterLens('groups')：工具栏 chip 的进入语义（§4.1 未激活点击进入 groups）。 */
  function enterLens(next: DuplicateLensModeDto) {
    if (mode.value === next) return
    // 快照只在「普通画廊 → 镜头」的边界拍一次：镜头内切模式/开关不重拍，
    // 返回锚点始终指向真正的进入前状态。
    if (mode.value === null) {
      returnFullPath.value = router.currentRoute.value.fullPath
    }
    mode.value = next
    showUniqueItems.value = false
    // push（非 replace）：进入镜头制造一个历史节点，系统返回 = 退出镜头（§4.3）。
    void router.push({ query: mergedLensQuery({ mode: next, showUniqueItems: false }) })
  }

  /** 退出镜头：恢复返回快照（路由、筛选随之经 URL 水合复原）并清空快照。滚动位由退出后的
   * 画廊重算链从 scrollCache 普通键读回（镜头会话不写普通键,进入前的位仍在）,无需在此恢复。 */
  function exitLens() {
    if (mode.value === null && returnFullPath.value === null) return
    const target = returnFullPath.value
    // 先清再走：快照是一次性消费，避免导航完成前的间隙被二次消费或残留到下一会话。
    returnFullPath.value = null
    mode.value = null
    showUniqueItems.value = false
    if (target) {
      void router.push(target)
    } else {
      // 深链直接打开镜头（无返回快照）→ 退出回普通「/」（§4.3）。普通筛选键由 querySync 的
      // store→URL 投影按 filterStore 现值重建，不在此拼接。
      void router.push({ path: '/' })
    }
  }

  /** 镜头内切换排列（groups ⇄ folders，§4.2）。replace：不为每次切换制造历史节点。 */
  function setMode(next: DuplicateLensModeDto) {
    if (mode.value === null || mode.value === next) return
    mode.value = next
    // groups 下 unique 无意义（§4.4 白名单不编码），落回 false 保持写读闭合。
    if (next === 'groups') showUniqueItems.value = false
    void router.replace({
      query: mergedLensQuery({ mode: next, showUniqueItems: showUniqueItems.value }),
    })
  }

  /** 「显示独有项」开关（§4.2）。仅 folders 有意义。 */
  function setShowUniqueItems(next: boolean) {
    if (mode.value !== 'folders' || showUniqueItems.value === next) return
    showUniqueItems.value = next
    void router.replace({
      query: mergedLensQuery({ mode: 'folders', showUniqueItems: next }),
    })
  }

  return {
    mode,
    showUniqueItems,
    isLensActive,
    returnFullPath,
    enterLens,
    exitLens,
    setMode,
    setShowUniqueItems,
  }
})
