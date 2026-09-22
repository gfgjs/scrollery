// src/stores/mediaStore.ts
// 布局和媒体状态存储

import { defineStore } from 'pinia'
import { ref, shallowRef, triggerRef, computed, onScopeDispose } from 'vue'
import type { LayoutRow, LayoutSummary, MediaMeta } from '../types/layout'
import type { MediaDetail, AppStats, LensAdjacentMedia } from '../types/media'
import type { DuplicateLensDescriptorDto } from '../types/view'
import { IPC } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { createLatestWriteQueue } from '../utils/latestWrite'
// type-only：无运行时环（useSelection → useViewDescriptor → mediaStore 的反向链在编译期被擦除）。
import type { BackendSelectionDescriptor } from '../composables/useSelection'
import { DEFAULTS } from '../constants/defaults'
import { useViewIds } from '../composables/useViewIds'

export type LayoutContentKeyParams = {
  directoryId?: number | null
  filters?: Record<string, unknown> | null
  duplicateLens?: DuplicateLensDescriptorDto | null
}

/** 每个布局请求都有明确结局；排队调用不能在布局尚未处理时报告完成。 */
export type LayoutComputeResult = 'committed' | 'superseded' | 'failed'

/** 查看器保存导航身份；普通视图与镜头通过后端索引查邻居，独立搜索沿其结果列表。 */
export type NavigationContext =
  | { type: 'search'; itemIds: readonly number[]; currentIndex: number }
  | { type: 'layout'; orderVersion: number; totalCount: number; currentIndex: number | null }
  | { type: 'lens'; layoutVersion: number; totalCount: number; currentIndex: number | null }

/** 邻接缓存与实时请求使用相同结果结构，提交时同步更新计数与详情。 */
export interface AdjacentDetail {
  detail: MediaDetail
  index: number | null
  totalCount: number
}

/**
 * 对布局筛选做按键排序的轻量序列化。
 * `JSON.stringify` 的结果依赖调用方插入顺序，不能直接作为布局内容键。
 */
function stableSerialize(value: unknown): string {
  if (value === null || typeof value !== 'object') {
    const serialized = JSON.stringify(value)
    return serialized === undefined ? 'null' : serialized
  }
  if (Array.isArray(value)) return `[${value.map(stableSerialize).join(',')}]`

  const entries = Object.entries(value as Record<string, unknown>)
    .filter(([, item]) => item !== undefined)
    .sort(([left], [right]) => left.localeCompare(right))
  return `{${entries
    .map(([key, item]) => `${JSON.stringify(key)}:${stableSerialize(item)}`)
    .join(',')}}`
}

/**
 * 生成布局的内容语义键。
 *
 * 几何参数（宽度、行高等）不进键：同一内容重排继续保留旧布局的 SWR 行为；
 * 内容集合变化必须换键，防止普通画廊、groups 和 folders 互相显示旧行。
 */
export function buildLayoutContentKey(params: LayoutContentKeyParams = {}): string {
  const lens = params.duplicateLens
  if (lens?.mode === 'groups') return 'lens:groups'
  if (lens?.mode === 'folders') {
    return `lens:folders:u${lens.showUniqueItems ? 1 : 0}`
  }

  const directory = params.directoryId == null ? 'all' : String(params.directoryId)
  return `normal:d${directory}:f${stableSerialize(params.filters ?? {})}`
}

export const useMediaStore = defineStore('media', () => {
  const viewIds = useViewIds()
  // ── 布局状态 ────────────────────────────────────────────────────────
  const layoutSummary = ref<LayoutSummary | null>(null)
  // 当前已提交布局的内容语义。普通画廊与两种重复镜头不能复用对方的行；
  // 同一语义内的尺寸/排序重算则继续保留旧布局，维持现有 SWR 体验。
  const layoutSemanticKey = ref<string | null>(null)
  const isComputingLayout = ref(false)
  const layoutDirty = ref(false)
  const layoutDirtyRevision = ref(0)

  // 单项标量改动信号（详情页 / 外部就地改 favorite / rating / colorLabel 时发出）。
  // 画廊显示的 visibleRows 由 MediaGrid 持有（经 fetchRowsByY 拉取），store 侧无行缓存（R2-2 已删
  // rowCache/patchRowItem 旁路）。MediaGrid 监听本信号把改动回灌它持有的 visibleRows。seq 单调自增，保证即便
  // (id,field,value) 重复也触发 watch。in-grid 操作另有同步内联 patch（即时反馈），与本信号的
  // 回灌幂等无冲突。
  type ItemFieldPatch = {
    id: number
    field: 'isFavorited' | 'rating' | 'colorLabel'
    value: number | boolean
    seq: number
  }
  const itemPatchSignal = ref<ItemFieldPatch | null>(null)
  let patchSeq = 0
  function signalItemPatch(id: number, field: ItemFieldPatch['field'], value: number | boolean) {
    itemPatchSignal.value = { id, field, value, seq: ++patchSeq }
  }

  // ── 可视区懒加载元数据（EXIF / GPS / 文件名 / 目录路径） ──────────────────
  // 重型字段已从常驻布局缓存剥离；仅在卡片信息浮层开启时按窗口拉取。
  const viewportMeta = shallowRef<Map<number, MediaMeta>>(new Map())
  const META_BATCH_LIMIT = 256
  const META_RECENT_LIMIT = 256
  let metaWindow = new Set<number>()
  let pendingMetaIds = new Set<number>()
  let metaTimer: ReturnType<typeof setTimeout> | null = null
  let metaEpoch = 0
  let metaDisposed = false
  let metaInFlight: { epoch: number; ids: Set<number> } | null = null
  let viewportMetaScopeKey: string | null = null

  function clearMetaWindow() {
    metaEpoch++
    if (metaTimer !== null) clearTimeout(metaTimer)
    metaTimer = null
    metaWindow.clear()
    pendingMetaIds.clear()
    if (viewportMeta.value.size > 0) viewportMeta.value = new Map()
    // 已发 IPC 占用实际槽位至结束；旧代完成只释放槽，不回填、不增加并发。
  }

  function trimMeta(cache: Map<number, MediaMeta>) {
    let outside = 0
    for (const id of cache.keys()) if (!metaWindow.has(id)) outside++
    for (const id of cache.keys()) {
      if (outside <= META_RECENT_LIMIT) break
      if (!metaWindow.has(id)) { cache.delete(id); outside-- }
    }
  }

  async function flushMeta() {
    metaTimer = null
    if (metaDisposed || metaInFlight || pendingMetaIds.size === 0) return
    const ids: number[] = []
    for (const id of pendingMetaIds) {
      ids.push(id)
      pendingMetaIds.delete(id)
      if (ids.length === META_BATCH_LIMIT) break
    }
    const request = { epoch: metaEpoch, ids: new Set(ids) }
    metaInFlight = request
    try {
      const metas = await invokeIpc<MediaMeta[]>(IPC.GET_META_FOR_VIEWPORT, { ids })
      if (metaDisposed || request.epoch !== metaEpoch) return
      const next = new Map(viewportMeta.value)
      for (const m of metas) next.set(m.id, m)
      trimMeta(next)
      // DOM 与 Canvas 继续消费 Map 引用更新，但复制规模只随窗口密度变化。
      viewportMeta.value = next
    } catch (error) {
      if (!metaDisposed && request.epoch === metaEpoch) {
        logger.error('[MediaStore] get_meta_for_viewport FAILED', { error })
      }
    } finally {
      metaInFlight = null
      // 后续批次无需逐批防抖；失败的本批不自动重试，下一次有效窗口可以重新提交。
      if (!metaDisposed && pendingMetaIds.size > 0) void flushMeta()
    }
  }

  /** 用最新窗口替换待发集合；空窗口关闭取数并撤销旧响应。 */
  function ensureMeta(ids: number[]) {
    if (metaDisposed) return
    if (ids.length === 0) { clearMetaWindow(); return }
    metaWindow = new Set(ids)
    const next = new Map(viewportMeta.value)
    // 触达窗口中的缓存项移到末尾，保留的离窗项按最近使用次序淘汰。
    for (const id of metaWindow) {
      const existing = next.get(id)
      if (existing) { next.delete(id); next.set(id, existing) }
    }
    trimMeta(next)
    if (viewportMeta.value.size > 0) viewportMeta.value = next
    pendingMetaIds = new Set(ids.filter((id) => !next.has(id) &&
      !(metaInFlight?.epoch === metaEpoch && metaInFlight.ids.has(id))))
    if (pendingMetaIds.size > 0 && metaTimer === null && !metaInFlight) {
      metaTimer = setTimeout(flushMeta, 120)
    }
  }

  onScopeDispose(() => { metaDisposed = true; clearMetaWindow() })

  // ── 详情视图 ─────────────────────────────────────────────────────────
  const navContext = shallowRef<NavigationContext | null>(null)
  const detailItem = ref<MediaDetail | null>(null)
  const isDetailOpen = ref(false)
  const detailPending = ref(false)
  let detailRequestGeneration = 0
  let navOrderIntentEpoch = 0

  // ── 统计 ────────────────────────────────────────────────────────────────
  const stats = ref<AppStats | null>(null)

  // 统计取数的 single-flight 合并（扫描期每个在跑的根都按秒级节奏调 get_app_stats，
  // 一次全表聚合，此前各自裸 invoke 会把并发压到同一条查询上）。约定（仅限同一 epoch）：
  //   · 至多一个在飞请求 + 至多一个尾随请求，共用同一 drain Promise；
  //   · 在飞期间到达的请求只置尾随标志，不另开 Promise；
  //   · 调用方 await 到自己那次请求之后的新快照；
  //   · 失败时整个 drain 一起拒绝并释放槽位，已排的尾随被丢弃（下次调用重新取，
  //     失败路径不保证尾随新快照）。
  // epoch：dispose / 清库后旧快照来自清空前的库，必须丢弃；invalidateStats 会清空槽位，
  // 跨 epoch 的旧请求可能短暂重叠，单飞只保证同 epoch 内。
  let statsEpoch = 0
  type StatsDrain = { epoch: number; trailing: boolean; promise: Promise<void> }
  let statsDrain: StatsDrain | null = null

  async function fetchStatsSnapshot(epoch: number) {
    const next = await invokeIpc<AppStats>(IPC.GET_STATS)
    // 越代（dispose / 清库）不得回写。
    if (epoch !== statsEpoch) return
    stats.value = next
  }

  /**
   * 刷新统计；同 epoch 内并发调用合并为一个 drain（在飞 + 至多一个尾随）。
   * 返回值经 pinia 动作包装后是**新** promise,调用方要么 await,要么显式 .catch——
   * 这里不替调用方兜未处理拒绝(内部 drain promise 由包装层的 then/catch 接住)。
   */
  function loadStats(): Promise<void> {
    const current = statsDrain
    if (current && current.epoch === statsEpoch) {
      current.trailing = true
      return current.promise
    }
    const epoch = statsEpoch
    const drain: StatsDrain = { epoch, trailing: false, promise: Promise.resolve() }
    statsDrain = drain
    drain.promise = (async () => {
      try {
        do {
          drain.trailing = false
          await fetchStatsSnapshot(epoch)
        } while (drain.trailing && epoch === statsEpoch)
      } finally {
        // 成功/失败都必须释放槽位（失败后仍要能重新请求）；越代时新 drain 已接管，不得清掉别人。
        if (statsDrain === drain) statsDrain = null
      }
    })()
    return drain.promise
  }

  /** 使在飞/排队的统计取数失效（清库、作用域销毁时调用）；旧快照不得越代回写。 */
  function invalidateStats() {
    statsEpoch += 1
    statsDrain = null
  }

  onScopeDispose(invalidateStats)

  // ── 计算属性 ─────────────────────────────────────────────────────────────
  const totalItems = computed(() => stats.value?.totalItems ?? 0)
  const viewTotalItems = computed(() => layoutSummary.value?.totalItems ?? 0)
  const totalHeight = computed(() => layoutSummary.value?.totalHeight ?? 0)
  const totalRows = computed(() => layoutSummary.value?.totalRows ?? 0)
  const layoutVersion = computed(() => layoutSummary.value?.layoutVersion ?? 0)
  const orderVersion = computed(() => layoutSummary.value?.orderVersion ?? 0)

  // ── 动作 ───────────────────────────────────────────────────────────────

  // computeLayout 入参形状（pending/current 复用，替代 any）。
  type ComputeLayoutArgs = {
    directoryId?: number | null
    filters?: Record<string, unknown>
    containerWidth: number
    rowHeight?: number
    gap?: number
    groupBy?: string
    sortWithinGroup?: string
    sortOrder?: string
    layoutMode?: string
    /** 无缝分组(#1):排序仍按 groupBy 聚合,打包无分隔符、行跨组连续。 */
    seamless?: boolean
    /**
     * 重复镜头描述符(2026-09-02 方案 §10.2)。缺省 = 普通画廊;存在时后端忽略普通排序/分组,
     * 按 lens 模式产出重复组 separator 的 LayoutRow(folders 模式后端显式报 DuplicateLensUnsupported)。
     */
    duplicateLens?: DuplicateLensDescriptorDto
  }
  type ComputeRequest = {
    params: ComputeLayoutArgs
    generation: number
    orderEpoch: number
    resolve: (result: LayoutComputeResult) => void
  }
  let pendingCompute: ComputeRequest | null = null
  let activeCompute: ComputeRequest | null = null
  let computeGeneration = 0
  let requestedOrderKey: string | null = null
  const orderIntentEpoch = ref(0)

  /** 新集合或顺序尚未提交时撤销全集操作资格；已有缓存可供同版本恢复。 */
  function invalidateViewOrder() {
    orderIntentEpoch.value++
    viewIds.setExpectedVersion(null)
  }

  function computeLayout(params: ComputeLayoutArgs): Promise<LayoutComputeResult> {
    if (params.containerWidth < 100) {
      logger.warn('[MediaStore] computeLayout: containerWidth too small, skipping')
      return Promise.resolve('superseded')
    }

    const requestGeneration = ++computeGeneration
    const nextContentKey = buildLayoutContentKey(params)
    if (viewportMetaScopeKey !== nextContentKey) {
      viewportMetaScopeKey = nextContentKey
      clearMetaWindow()
    }
    const nextOrderKey = stableSerialize([nextContentKey,
      params.duplicateLens?.orderingVersion ?? [params.groupBy ?? 'date',
        params.sortWithinGroup ?? 'datetime', params.sortOrder ?? 'desc']])
    if (requestedOrderKey !== nextOrderKey) {
      requestedOrderKey = nextOrderKey
      invalidateViewOrder()
    }
    if (layoutSummary.value && layoutSemanticKey.value !== nextContentKey) {
      layoutSummary.value = null
      layoutSemanticKey.value = null
    }

    return new Promise((resolve) => {
      const request = { params, generation: requestGeneration, orderEpoch: orderIntentEpoch.value, resolve }
      if (activeCompute) {
        pendingCompute?.resolve('superseded')
        pendingCompute = request
      } else {
        void runCompute(request)
      }
    })
  }

  async function runCompute(request: ComputeRequest): Promise<void> {
      activeCompute = request
      isComputingLayout.value = true
      const { params: currentParams, generation: currentGeneration } = request
      let computeWatchdog: ReturnType<typeof setTimeout> | undefined
      // 超时完成本次等待，释放槽位；原 IPC 的迟到成功/失败只会结算已结束的 race。
      const timeout = new Promise<never>((_, reject) => {
        computeWatchdog = setTimeout(() => reject(new Error('compute_layout timed out after 30s')), 30000)
      })
      try {
        const computedSummary = await Promise.race([invokeIpc<LayoutSummary>(IPC.COMPUTE_LAYOUT, {
          params: {
            directoryId: currentParams.directoryId ?? null,
            filters: currentParams.filters ?? null,
            containerWidth: currentParams.containerWidth,
            rowHeight: currentParams.rowHeight ?? DEFAULTS.GRID_ROW_HEIGHT,
            gap: currentParams.gap ?? DEFAULTS.GRID_GAP,
            groupBy: currentParams.groupBy ?? 'date',
            sortWithinGroup: currentParams.sortWithinGroup ?? 'datetime',
            sortOrder: currentParams.sortOrder ?? 'desc',
            layoutMode: currentParams.layoutMode ?? 'justified',
            seamless: currentParams.seamless ?? false,
            // 重复镜头(§10.2):缺省 null = 普通画廊;镜头态下后端按 duplicateLens 决定集合与顺序。
            duplicateLens: currentParams.duplicateLens ?? null,
            // 多档源服务(2026-08-16 阶段2):后端出口按「格尺寸×DPR」选最小满足档。
            dpr: window.devicePixelRatio || 1,
          },
        }), timeout])
        if (currentGeneration === computeGeneration) {
          if (request.orderEpoch === orderIntentEpoch.value) {
            viewIds.setExpectedVersion(currentParams.duplicateLens ? null : computedSummary.orderVersion)
          }
          layoutSummary.value = computedSummary
          layoutSemanticKey.value = buildLayoutContentKey(currentParams)
          request.resolve('committed')
        } else {
          request.resolve('superseded')
        }
      } catch (e) {
        logger.error('[MediaStore] computeLayout FAILED', { error: e })
        request.resolve(currentGeneration === computeGeneration ? 'failed' : 'superseded')
      } finally {
        clearTimeout(computeWatchdog)
        activeCompute = null
        const next = pendingCompute
        pendingCompute = null
        if (next) {
          void runCompute(next)
        } else {
          isComputingLayout.value = false
        }
      }
  }

  async function fetchRowsByY(topY: number, bottomY: number): Promise<LayoutRow[]> {
    const version = layoutSummary.value?.layoutVersion

    try {
      const rows = await invokeIpc<LayoutRow[]>(IPC.GET_LAYOUT_ROWS_BY_Y, {
        topY,
        bottomY,
        layoutVersion: version,
      })
      return rows
    } catch (e) {
      logger.error(`[MediaStore] fetchRowsByY(${topY}, ${bottomY}) FAILED`, { error: e })
      throw e
    }
  }

  // T16 方案B:按段取行(半开区间 [startY, endY),精确段归属——区别于 fetchRowsByY 的
  // 视口相交语义,那会把邻段边界行掺进来)。供 useBucketVirtualScroll 段挂载取数。
  async function fetchBucketRows(startY: number, endY: number): Promise<LayoutRow[]> {
    const version = layoutSummary.value?.layoutVersion
    try {
      return await invokeIpc<LayoutRow[]>(IPC.GET_BUCKET_ROWS, {
        startY,
        endY,
        layoutVersion: version,
      })
    } catch (e) {
      logger.error(`[MediaStore] fetchBucketRows(${startY}, ${endY}) FAILED`, { error: e })
      throw e
    }
  }

  async function openDetailFromSearch(id: number, resultIds: number[]) {
    const generation = ++detailRequestGeneration
    detailPending.value = true
    const nextContext: NavigationContext = {
      type: 'search',
      itemIds: resultIds,
      currentIndex: resultIds.indexOf(id),
    }
    navContext.value = nextContext
    try {
      const nextDetail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id })
      if (generation !== detailRequestGeneration) return
      detailItem.value = nextDetail
      isDetailOpen.value = true
    } finally {
      if (generation === detailRequestGeneration) detailPending.value = false
    }
  }

  /**
   * 重复镜头(2026-09-02 方案 §8.2)从画廊打开查看器前建立导航上下文。
   * 只保存 layoutVersion/总数，实际相邻项由后端布局缓存解析，避免把 flat_ids 复制到前端。
   * 首次打开时 currentIndex 未知；第一次镜头导航响应会补齐它。路由化查看器的
   * loadFromRoute → openDetail(id)(不带 fromLayout)不会清掉本上下文,closeDetail 统一回收。
   */
  function setLensNavContext(layoutVersion: number, totalCount: number) {
    navOrderIntentEpoch = orderIntentEpoch.value
    navContext.value = {
      type: 'lens',
      layoutVersion,
      totalCount,
      currentIndex: null,
    }
  }

  /** 从普通画廊打开时只保存顺序代，不为查看器申请全集 ID。 */
  function setLayoutNavContext(id: number) {
    navOrderIntentEpoch = orderIntentEpoch.value
    const version = orderVersion.value
    const index = viewIds.indexOf(id)
    navContext.value = version > 0 && viewIds.isExpected(version)
      ? { type: 'layout', orderVersion: version, totalCount: viewTotalItems.value,
          currentIndex: index >= 0 ? index : null }
      : null
  }

  /** 上下文身份与当前已提交视图都匹配才允许翻页或消费预取缓存。 */
  function isNavContextCurrent(context: NavigationContext | null): context is NavigationContext {
    if (!context || navContext.value !== context || detailPending.value) return false
    if (context.type === 'search') return true
    if (navOrderIntentEpoch !== orderIntentEpoch.value) return false
    if (context.type === 'lens') return context.layoutVersion > 0 &&
      context.layoutVersion === layoutVersion.value && layoutSemanticKey.value?.startsWith('lens:') === true
    return context.orderVersion > 0 && context.orderVersion === orderVersion.value &&
      viewIds.isExpected(context.orderVersion)
  }

  async function openDetail(id: number, fromLayout = false) {
    const generation = ++detailRequestGeneration
    detailPending.value = true
    // 打开新条目时立即绑定来源,在详情完成前暂停邻接操作,避免按旧展示项推进新上下文。
    if (fromLayout) {
      setLayoutNavContext(id)
    }
    try {
      const nextDetail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id })
      if (generation !== detailRequestGeneration) return
      const context = navContext.value
      if (context?.type === 'search') {
        const index = context.itemIds.indexOf(id)
        if (index >= 0) context.currentIndex = index
        else navContext.value = null
      } else if (context) {
        const index = context.type === 'layout' ? viewIds.indexOf(id) : -1
        context.currentIndex = index >= 0 ? index : null
      }
      triggerRef(navContext)
      detailItem.value = nextDetail
      isDetailOpen.value = true
    } finally {
      if (generation === detailRequestGeneration) detailPending.value = false
    }
  }

  /** 翻页与预加载共用的单步查询；响应只属于发起时的上下文与当前项。 */
  async function fetchAdjacentDetail(context: NavigationContext, currentId: number, offset: number): Promise<AdjacentDetail | null> {
    if (!isNavContextCurrent(context) || detailItem.value?.id !== currentId) return null
    let result: AdjacentDetail | null
    if (context.type === 'lens') {
      result = await invokeIpc<LensAdjacentMedia | null>(IPC.GET_LENS_ADJACENT_MEDIA, {
        currentId, offset, layoutVersion: context.layoutVersion,
      })
    } else if (context.type === 'search') {
      const index = context.currentIndex + offset
      if (context.itemIds[context.currentIndex] !== currentId || index < 0 || index >= context.itemIds.length) return null
      const detail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id: context.itemIds[index] })
      result = { detail, index, totalCount: context.itemIds.length }
    } else {
      const index = context.currentIndex === null ? null : context.currentIndex + offset
      const detail = await invokeIpc<MediaDetail | null>(IPC.GET_ADJACENT_MEDIA, {
        currentId, offset, orderVersion: context.orderVersion,
      })
      result = detail ? { detail, index, totalCount: context.totalCount } : null
    }
    return isNavContextCurrent(context) && detailItem.value?.id === currentId ? result : null
  }

  /** 缓存命中与实时响应走同一提交点，撤销更早的详情请求。 */
  function applyAdjacentDetail(context: NavigationContext, currentId: number, result: AdjacentDetail): boolean {
    if (!isNavContextCurrent(context) || detailItem.value?.id !== currentId) return false
    if (context.type === 'search' && (result.index === null || context.itemIds[result.index] !== result.detail.id)) return false
    detailRequestGeneration++
    context.currentIndex = result.index
    if (context.type !== 'search') context.totalCount = result.totalCount
    triggerRef(navContext)
    detailItem.value = result.detail
    return true
  }

  async function navigateDetail(offset: number) {
    const context = navContext.value
    const currentId = detailItem.value?.id
    if (!isNavContextCurrent(context) || currentId === undefined) return
    const generation = ++detailRequestGeneration
    try {
      const result = await fetchAdjacentDetail(context, currentId, offset)
      if (result && generation === detailRequestGeneration) applyAdjacentDetail(context, currentId, result)
    } catch (error) {
      logger.warn('[MediaStore] adjacent lookup failed; keeping current item', { error })
    }
  }

  function closeDetail() {
    detailRequestGeneration++
    detailPending.value = false
    isDetailOpen.value = false
    detailItem.value = null
    navContext.value = null
  }

  /**
   * 重连自动恢复（T13 §3.7 离线 UX 验收点）：卷插拔时重取当前查看项的可用态。
   * **只**回写 availability 字段（不换 detailItem 引用）——避免触发覆盖层上重置 zoom / 人脸 /
   * exotic gate 的重 watch；卷态变化极罕见（拔插），单次 IPC 成本可忽略。
   */
  async function refreshDetailAvailability() {
    const cur = detailItem.value
    if (!cur) return
    const fresh = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id: cur.id })
    // 仅当仍是同一项时回写（防拔插期间用户已切换/关闭导致错写）。
    if (detailItem.value?.id === cur.id) detailItem.value.availability = fresh.availability
  }

  /** 将布局标记为过时 — 下次网格可见时应重新计算。 */
  function invalidateLayout() {
    layoutDirtyRevision.value++
    layoutDirty.value = true
  }

  /** 只消费请求开始前捕获的脏代；在途新失效必须留给后续请求。 */
  function consumeLayoutDirty(expectedRevision = layoutDirtyRevision.value): boolean {
    if (layoutDirty.value && expectedRevision === layoutDirtyRevision.value) {
      layoutDirty.value = false
      return true
    }
    return false
  }

  async function toggleFavorite(id: number): Promise<boolean> {
    const newVal = await invokeIpc<boolean>(IPC.TOGGLE_FAVORITE, { itemId: id })
    if (stats.value) {
      stats.value.totalFavorited += newVal ? 1 : -1
    }
    // 详情页切收藏 → 通知画廊回灌 visibleRows（in-grid 收藏另走同步内联 patch，幂等无冲突）。
    signalItemPatch(id, 'isFavorited', newVal)
    return newVal
  }

  async function setRating(id: number, rating: number) {
    await invokeIpc<void>(IPC.SET_RATING, { itemId: id, rating })
    // 详情页评分 → 通知画廊回灌 visibleRows（修复:此前详情改评分画廊不刷新、需手动刷新页面）。
    signalItemPatch(id, 'rating', rating)
  }

  /** 批量评分:对选区一次性设为 rating（0-5,0=清空）。返回受影响行数。
   *  R1-2/S4:入参 SelectionDescriptor——全选不整包传 id,后端解析后分块 UPDATE。 */
  async function batchSetRating(
    selection: BackendSelectionDescriptor,
    rating: number,
  ): Promise<number> {
    return await invokeIpc<number>(IPC.BATCH_SET_RATING, { selection, rating })
  }

  /** 设置颜色标签（0=清除 / 1-7 色档，T16）。镜像 setRating。 */
  async function setColorLabel(id: number, colorLabel: number) {
    await invokeIpc<void>(IPC.SET_COLOR_LABEL, { itemId: id, colorLabel })
    // 详情页设色 → 通知画廊回灌 visibleRows（修复:此前详情设色画廊不刷新、需手动刷新页面）。
    signalItemPatch(id, 'colorLabel', colorLabel)
  }

  /** 批量设色:对选区一次性设为 colorLabel。返回受影响行数。镜像 batchSetRating（R1-2/S4）。 */
  async function batchSetColorLabel(
    selection: BackendSelectionDescriptor,
    colorLabel: number,
  ): Promise<number> {
    return await invokeIpc<number>(IPC.BATCH_SET_COLOR_LABEL, { selection, colorLabel })
  }

  // 旋转写队列(审查 F-09):快速连点时并发 IPC 写取得 DB 锁的顺序不保证等于点击顺序,
  // 旧角度可能最后落库。latest-write-wins 串行化后每 item 至多一个在途写,终值恒为最后一击。
  // 写失败:本会话保持乐观角度(当前画面仍正确),重开自然回退持久值——只留证据不打断看图。
  const rotationWrites = createLatestWriteQueue<number, number>(
    (id, rotation) => invokeIpc<void>(IPC.SET_VIEW_ROTATION, { itemId: id, rotation }),
    (id, rotation, e) => {
      logger.error(`[mediaStore] setViewRotation failed: item=${id} rotation=${rotation}`, {
        error: e,
      })
    },
  )

  /**
   * 持久化看图台展示旋转（归一化 0/90/180/270，V20）。后端再归一化一次兜底。
   * 与 setRating/setColorLabel 不同：旋转不进网格（LayoutRowItem 无此列），故不发 itemPatchSignal，
   * 只落库——查看器本地已乐观更新 detail.viewRotation。
   * 经 latest-write-wins 队列串行化(审查 F-09),同步返回;失败在队列内捕获。
   */
  function setViewRotation(id: number, rotation: number) {
    rotationWrites.push(id, rotation)
  }

  // 播放位置写队列(镜像 rotationWrites,V23):latest-write-wins,防连续 seek/定时上报并发写乱序。
  const playbackWrites = createLatestWriteQueue<number, number>(
    (id, ms) => invokeIpc<void>(IPC.SET_PLAYBACK_POSITION, { itemId: id, ms }),
    (id, ms, e) => {
      logger.error(`[mediaStore] setPlaybackPosition failed: item=${id} ms=${ms}`, {
        error: e,
      })
    },
  )

  /**
   * 持久化播放器播放位置记忆(ms;V23)。后端再 clamp 一次兜底。镜像 setViewRotation:
   * 不进网格,只落库,经 latest-write-wins 队列串行化(同构审查 F-09 手法)。
   */
  function setPlaybackPosition(id: number, ms: number) {
    playbackWrites.push(id, ms)
  }

  return {
    layoutSummary,
    layoutSemanticKey,
    isComputingLayout,
    layoutDirty,
    layoutDirtyRevision,
    itemPatchSignal,
    detailItem,
    isDetailOpen,
    navContext,
    stats,
    viewportMeta,
    totalItems,
    viewTotalItems,
    totalHeight,
    totalRows,
    layoutVersion,
    orderVersion,
    computeLayout,
    invalidateViewOrder,
    fetchRowsByY,
    fetchBucketRows,
    ensureMeta,
    openDetail,
    openDetailFromSearch,
    setLensNavContext,
    setLayoutNavContext,
    isNavContextCurrent,
    fetchAdjacentDetail,
    applyAdjacentDetail,
    refreshDetailAvailability,
    closeDetail,
    navigateDetail,
    loadStats,
    invalidateStats,
    toggleFavorite,
    setRating,
    batchSetRating,
    setColorLabel,
    batchSetColorLabel,
    setViewRotation,
    setPlaybackPosition,
    invalidateLayout,
    consumeLayoutDirty,
  }
})
