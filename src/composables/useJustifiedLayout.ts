// src/composables/useJustifiedLayout.ts
// 消费后端行数据并驱动 compute_layout 重新运行。

import { computed, watch, onBeforeUnmount } from 'vue'
import { useMediaStore } from '../stores/mediaStore'
import { useFilterStore } from '../stores/filterStore'
import { useUiStore } from '../stores/uiStore'
import { useViewStore } from '../stores/viewStore'
import { useAiStore } from '../stores/aiStore'
import { useScanStore } from '../stores/scanStore'
import { useDuplicateLensStore } from '../stores/duplicateLensStore'
import { DEFAULTS } from '../constants/defaults'
import { resolveView } from '../utils/resolveView'
import { logger } from '../utils/logger'

export interface UseJustifiedLayoutOptions {
  /**
   * 现在是否允许取数。缺省恒 true。
   *
   * **不变量：不在屏上就不取数。** 失活期的变更只记 dirty，激活时经 `flushIfDeferred()` 用**当前**视图
   * 补算一次（不重放中间态）。
   *
   * 为何需要（真机 round10 #5「进收藏夹先卡 1–2 秒再闪一下所有照片」）：MediaGrid 被 `<KeepAlive>` 缓存，
   * **失活期组件仍活着、watcher 照常跑**——这是有意的（增强/脏标志重算不能丢）。但 compute_layout 是全库
   * 量级的 IPC。落 `/collections` 总览页时 App.vue 会把 activeCollection 清成 null，那句话是**纯粹的状态
   * 卫生**（总览页当然没有选中项），可它被一个**看不见却活着**的 MediaGrid 的重算 watcher 当成了「用户要
   * 看全库」→ 白跑一次 50 万行查询（dev 实测 1–2s），还把随后带 albumId 的那次挤进 mediaStore 串行队列
   * 后面，并让 onActivated 先渲染出那份陈旧全库布局 =「闪一下所有照片」。
   *
   * 根因是 viewStore 里**「无选中」与「要全库」共用同一个 null**：对在屏的网格 null 确实该显示全库，对
   * 离屏的网格同一个 null 就是一道 50 万行的指令。KeepAlive 的代价从来不是内存，是**「不可见」不再蕴含
   * 「不响应」**——组件树里少了一个渲染节点，watcher 图里一个都没少。
   */
  enabled?: () => boolean
}

export function useJustifiedLayout(
  containerWidthRef: () => number,
  options?: UseJustifiedLayoutOptions,
) {
  const media = useMediaStore()
  const filter = useFilterStore()
  const ui = useUiStore()
  const viewStore = useViewStore()
  const ai = useAiStore()
  const scan = useScanStore()
  const lens = useDuplicateLensStore()

  // 扫描/富化期强制宫格(用户裁 2026-07-17,#6):等高布局逐项吃宽高比,扫描期两个 reflow 源
  // (每秒 totalItems 追加全量重算 + 富化每 2s 把占位尺寸换真实宽高比)叠加,行几何持续乱跳;
  // 宫格 cell 与宽高比无关,对两源天然免疫——扫描期(含 enriching,恰是尺寸回填高发段)自动
  // 降级宫格,收尾翻回用户选择并终算一次。用 computed 进 watch 依赖:grid 用户扫描起止
  // 不产生 effective 变化,零多余重算。
  const effectiveLayoutMode = computed(() => (scan.isAnyScanRunning ? 'grid' : ui.layoutMode))

  let resizeTimer: ReturnType<typeof setTimeout> | null = null
  // 失活期累积的重算请求（见 UseJustifiedLayoutOptions.enabled）。
  let deferredWhileDisabled = false

  function isEnabled(): boolean {
    return options?.enabled ? options.enabled() : true
  }

  async function compute(width?: number) {
    const cw = width ?? containerWidthRef()

    // 容器尚未准备好，延迟到下一个 tick 并重试一次。
    if (cw < 100) {
      await new Promise((r) => setTimeout(r, 50))
      const retryW = containerWidthRef()

      if (retryW < 100) {
        logger.warn('[JustifiedLayout] compute() retry failed: width still <100, giving up')
        return
      }
      return compute(retryW)
    }

    const directoryId = viewStore.activeDirectoryId

    // ── 重复镜头(2026-09-02 方案 §1/§4.1):镜头激活时暂停普通筛选 ──────────────────
    // 镜头的集合是「全库可见范围内的精确重复成员」,与普通筛选/视图维度**不交集**(避免隐藏组内
    // 成员),故 scope 维度(directoryId/personId/albumId/智能相册)、filterStore 筛选、searchQuery/
    // 语义参数全部不下发;groupBy/sortWithinGroup/sortOrder/seamless 也不传——镜头内集合与顺序由
    // 后端 duplicateLens 决定（groups 组序/组内序固定，folders 按关联簇与三桶固定排序）。行高与
    // 宫格⇄等高行继续可用(§5.1);扫描期强制宫格与普通路径同款。镜头关闭时走下方原路径,零变化。
    const lensMode = lens.mode
    if (lensMode !== null) {
      await media.computeLayout({
        filters: {},
        containerWidth: cw,
        rowHeight: ui.gridRowHeight,
        gap: DEFAULTS.GRID_GAP,
        layoutMode: effectiveLayoutMode.value,
        duplicateLens: {
          mode: lensMode,
          showUniqueItems: lens.showUniqueItems,
          // 排序契约版本(§10.2):镜头排序语义调整时前后端同步递增;当前 v1 固定。
          orderingVersion: 1,
        },
      })
      return
    }

    // Record<string, unknown> 与 mediaStore.computeLayout 的 filters 入参同型，
    // 可直接增补 scope 维度字段（personId/albumId/...）而无需 any。
    const filters: Record<string, unknown> = filter.toApiFilter()

    // 视图维度 → 扁平 filters 的映射。precedence 决策由 resolveView 单源提供（S2-c1，消除此前与
    // useViewDescriptor 各自内联同一套 precedence 的 🔴 R1-2 双维护）；本 switch 只做 JL 侧机械映射。
    // directory 维度由上方 directoryId param 承接（与原「不进 filters」一致）。
    const resolved = resolveView({
      activePersonId: viewStore.activePersonId,
      activeCollection: viewStore.activeCollection,
      activeSmartAlbum: viewStore.activeSmartAlbum,
      activeDirectoryId: viewStore.activeDirectoryId,
    })
    switch (resolved.kind) {
      case 'person':
        // 人物视图（F6）：限定为包含该人物簇人脸的图像。
        filters.personId = resolved.personId
        break
      case 'collection':
        // 用户夹按 album_items 成员（albumId）限定。
        filters.albumId = resolved.albumId
        break
      case 'systemCollection':
        // 系统夹 ≈ 类型 + is_favorited。
        filters.mediaTypes = [resolved.mediaType]
        filters.favoritedOnly = true
        break
      case 'smartAlbum':
        if (resolved.album === 'favorites') filters.favoritedOnly = true
        else if (resolved.album === 'live-photos') filters.livePhotoOnly = true
        else if (resolved.album === 'recent') filters.recentOnly = true
        else if (resolved.album === 'trash') filters.trashedOnly = true
        break
      case 'directory':
        // directoryId param 已承接，filters 无需额外键。
        break
    }

    if (ui.searchQuery && ui.searchQuery.trim() !== '') {
      filters.searchQuery = ui.searchQuery.trim()
      filters.searchScope = ui.searchScope
    }

    if (ai.isSemanticMode) {
      filters.aiSearch = true
      filters.aiThreshold = ai.similarityThreshold
    }

    await media.computeLayout({
      directoryId,
      filters,
      containerWidth: cw,
      rowHeight: ui.gridRowHeight,
      gap: DEFAULTS.GRID_GAP,
      groupBy: ui.groupBy,
      sortWithinGroup: ui.sortWithinGroup,
      sortOrder: ui.sortOrder,
      // 布局模式（T20）：'grid' 走后端均匀宫格排版，否则等高行。后端产出同一 LayoutRow 枚举。
      // 扫描期经 effectiveLayoutMode 强制宫格(见上),扫毕自动回用户选择。
      layoutMode: effectiveLayoutMode.value,
      // 无缝分组(#1):排序仍按 groupBy 聚合,打包无分隔符、行跨组。
      seamless: ui.seamlessGroups,
    })
  }

  // 防抖调整大小处理程序
  function onResize(newWidth: number) {
    if (resizeTimer) clearTimeout(resizeTimer)
    resizeTimer = setTimeout(() => {
      if (!isEnabled()) {
        deferredWhileDisabled = true
        return
      }
      void compute(newWidth)
    }, ui.resizeDebounceMs)
  }

  /** 宿主进入不应采样几何的过渡期时，撤销尚未执行的旧宽度防抖。 */
  function cancelPendingResize(): void {
    if (!resizeTimer) return
    clearTimeout(resizeTimer)
    resizeTimer = null
  }

  /**
   * watcher/resize 的取数入口：不在屏上只记 dirty、不发 IPC（见 UseJustifiedLayoutOptions.enabled）。
   * 导出的 compute() 不经此闸——那是调用方显式发起的，调用方自知上下文。
   */
  function requestCompute(): boolean {
    if (!isEnabled()) {
      deferredWhileDisabled = true
      return false
    }
    void compute()
    return true
  }

  /**
   * 激活时补算：失活期若有过重算请求，用**当前**视图/筛选算一次（只算一次，不重放中间态——
   * 中间态正是要跳过的东西）。
   *
   * @returns 是否真的算了（调用方据此决定是否刷新可见行）
   */
  async function flushIfDeferred(): Promise<boolean> {
    if (!deferredWhileDisabled) return false
    deferredWhileDisabled = false
    await compute()
    // dirty 是布局请求的附加信号；失活期间保留，激活补算后一起消费。
    media.consumeLayoutDirty()
    return true
  }

  // 当过滤器或活动视图改变时重新计算。
  // totalItems、layoutDirty 与后端事件由 useGalleryTauriSync 统一转入同一个 requestCompute 闸门。
  watch(
    [
      // 筛选维度整体看一个键(filterStore.apiFilterKey = toApiFilter() 的稳定序列化)。
      //
      // 原先手工枚举 mediaTypes/favoritedOnly/…/dateTo 共 7 条,而 compute() 喂后端的筛选部分
      // 恰是 toApiFilter() 的输出——两份列表本应同一份却各抄一次。抄漏不报错,症状是点了 chip
      // 画廊不更新(colorLabel 已漏过一次,T16 遗留)。改看单键后新维度自动进入,不再靠人记得抄。
      //
      // 一处有意的行为变化:只填日期一端时原先会触发一次重算,而 toApiFilter 在两端皆备前不下发
      // dateRange,那次重算与上一次逐字节相同,是可证明的空转,现在不再触发。
      () => filter.apiFilterKey,
      () => viewStore.activeSmartAlbum,
      () => viewStore.activeDirectoryId,
      () => viewStore.activeCollection,
      () => viewStore.activePersonId,
      () => ui.searchQuery,
      () => ui.searchScope,
      () => ui.gridRowHeight,
      () => ui.groupBy,
      () => ui.sortWithinGroup,
      () => ui.sortOrder,
      // 布局模式切换（T20）→ 重算（后端换排版算法，前端同一取行通路）。
      // 看 effective 而非 ui.layoutMode:扫描起止对等高用户 = 模式真实切换(grid⇄justified),
      // 对宫格用户 = 无变化不触发;用户手动切换恒触发。
      () => effectiveLayoutMode.value,
      () => ui.seamlessGroups,
      () => ai.isSemanticMode,
      () => ai.similarityThreshold,
      // 重复镜头(§10.1):模式(groups⇄folders)与独有项开关都是「集合+顺序」的切换 → 重算。
      // 镜头⇄普通画廊的往返同样由 mode 置 null/非 null 驱动本 watch 命中。
      () => lens.mode,
      () => lens.showUniqueItems,
    ],
    () => requestCompute(),
    { flush: 'post' },
  )

  onBeforeUnmount(() => {
    cancelPendingResize()
  })

  return {
    compute,
    onResize,
    cancelPendingResize,
    requestCompute,
    flushIfDeferred,
    layoutVersion: () => media.layoutSummary?.layoutVersion,
  }
}
