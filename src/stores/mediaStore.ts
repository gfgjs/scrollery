// src/stores/mediaStore.ts
// 布局和媒体状态存储

import { defineStore } from 'pinia'
import { ref, computed, onScopeDispose } from 'vue'
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

import { useUiStore } from './uiStore'

export type LayoutContentKeyParams = {
  directoryId?: number | null
  filters?: Record<string, unknown> | null
  duplicateLens?: DuplicateLensDescriptorDto | null
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
  // ── 布局状态 ────────────────────────────────────────────────────────
  const layoutSummary = ref<LayoutSummary | null>(null)
  // 当前已提交布局的内容语义。普通画廊与两种重复镜头不能复用对方的行；
  // 同一语义内的尺寸/排序重算则继续保留旧布局，维持现有 SWR 体验。
  const layoutSemanticKey = ref<string | null>(null)
  const isComputingLayout = ref(false)
  const layoutDirty = ref(false)

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
  const viewportMeta = ref<Map<number, MediaMeta>>(new Map())
  const pendingMetaIds = new Set<number>()
  let metaTimer: ReturnType<typeof setTimeout> | null = null
  // 宽度重排不会改变媒体元数据；只在切换目录/筛选上下文时丢弃，避免每次 resize 都让信息浮层闪空。
  let viewportMetaScopeKey: string | null = null

  async function flushMeta() {
    metaTimer = null
    if (pendingMetaIds.size === 0) return
    const ids = Array.from(pendingMetaIds)
    pendingMetaIds.clear()
    try {
      const metas = await invokeIpc<MediaMeta[]>(IPC.GET_META_FOR_VIEWPORT, { ids })
      // 重新赋值 Map，使 ref 在消费组件中触发响应式更新。
      const next = new Map(viewportMeta.value)
      for (const m of metas) next.set(m.id, m)
      viewportMeta.value = next
    } catch (e) {
      logger.error('[MediaStore] get_meta_for_viewport FAILED', { error: e })
    }
  }

  /** 确保给定 id 的元数据已加载（防抖、只取一次）。 */
  function ensureMeta(ids: number[]) {
    let added = false
    for (const id of ids) {
      if (!viewportMeta.value.has(id) && !pendingMetaIds.has(id)) {
        pendingMetaIds.add(id)
        added = true
      }
    }
    if (!added) return
    if (metaTimer === null) metaTimer = setTimeout(flushMeta, 120)
  }

  // ── 详情视图 ─────────────────────────────────────────────────────────
  // type='lens':重复镜头(§8.2)只保存布局版本与当前位置，不物化百万级 itemIds；
  // 上一项/下一项由后端按 layoutVersion 缓存一步解析。普通 layout/search 仍保留既有数组上下文。
  type NavigationContext =
    | {
        type: 'layout' | 'search'
        itemIds: number[]
        currentIndex: number
      }
    | {
        type: 'lens'
        layoutVersion: number
        totalCount: number
        currentIndex: number | null
      }

  const navContext = ref<NavigationContext | null>(null)
  const detailItem = ref<MediaDetail | null>(null)
  const isDetailOpen = ref(false)
  let detailRequestGeneration = 0

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
  let pendingComputeParams: ComputeLayoutArgs | null = null
  let pendingComputeGeneration = 0
  let isComputingInternal = false
  let computeGeneration = 0
  let computeSessionGeneration = 0
  let activeComputeSession = 0

  async function computeLayout(params: ComputeLayoutArgs) {
    if (params.containerWidth < 100) {
      logger.warn('[MediaStore] computeLayout: containerWidth too small, skipping')
      return
    }

    const requestGeneration = ++computeGeneration
    const nextContentKey = buildLayoutContentKey(params)
    if (layoutSummary.value && layoutSemanticKey.value !== nextContentKey) {
      layoutSummary.value = null
      layoutSemanticKey.value = null
    }

    if (isComputingInternal) {
      pendingComputeParams = params
      pendingComputeGeneration = requestGeneration
      return
    }

    const session = ++computeSessionGeneration
    activeComputeSession = session
    isComputingInternal = true
    isComputingLayout.value = true

    let currentRequest = { params, generation: requestGeneration }

    while (currentRequest) {
      const { params: currentParams, generation: currentGeneration } = currentRequest
      // 看门狗（问题6）：删除/重加文件夹后曾偶发 compute_layout 卡住（疑似读连接池耗尽，
      // 或与在途扫描/缩略图生成的锁竞争）。若 invoke 永不返回，「正在计算布局」会永久卡住。
      // 超时后复位标志，使 UI 恢复、用户可重试（切换文件夹会触发新的计算）。
      const computeWatchdog = setTimeout(() => {
        if (activeComputeSession === session && isComputingInternal) {
          logger.warn(
            `[MediaStore] computeLayout watchdog fired (>30s, session=${session}) — clearing isComputingLayout`,
          )
          activeComputeSession = 0
          isComputingLayout.value = false
          isComputingInternal = false
          // 仍有更新请求时立即让最新请求接管；旧 IPC 回来后会因 session 过期而丢弃。
          if (pendingComputeParams) {
            const next = pendingComputeParams
            pendingComputeParams = null
            pendingComputeGeneration = 0
            void computeLayout(next)
          }
        }
      }, 30000)
      const nextMetaScopeKey = JSON.stringify({
        directoryId: currentParams.directoryId ?? null,
        filters: currentParams.filters ?? null,
      })
      if (viewportMetaScopeKey !== nextMetaScopeKey) {
        viewportMetaScopeKey = nextMetaScopeKey
        // 切换上下文后丢弃旧元数据；同一上下文内的宽度重排继续复用已有结果。
        if (viewportMeta.value.size > 0) viewportMeta.value = new Map()
        if (metaTimer) {
          clearTimeout(metaTimer)
          metaTimer = null
        }
        pendingMetaIds.clear()
      }
      const ui = useUiStore()
      const needsMeta = ui.thumbInfoElements.some((el) => ['geo', 'camera', 'params'].includes(el))
      let computedSummary: LayoutSummary | null = null

      try {
        computedSummary = await invokeIpc<LayoutSummary>(IPC.COMPUTE_LAYOUT, {
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
            includeMeta: needsMeta,
            // 多档源服务(2026-08-16 阶段2):后端出口按「格尺寸×DPR」选最小满足档。
            dpr: window.devicePixelRatio || 1,
          },
        })
      } catch (e) {
        logger.error('[MediaStore] computeLayout FAILED', { error: e })
      } finally {
        clearTimeout(computeWatchdog)
      }

      // 看门狗或新的会话已经接管时，当前响应不再有提交资格。
      if (activeComputeSession !== session) break

      if (pendingComputeParams) {
        // 计算期间若又收到 resize，只继续追最新参数；中间结果不提交，旧帧保持在屏上。
        currentRequest = {
          params: pendingComputeParams,
          generation: pendingComputeGeneration,
        }
        pendingComputeParams = null
        pendingComputeGeneration = 0
      } else {
        if (computedSummary && currentGeneration === computeGeneration) {
          layoutSummary.value = computedSummary
          layoutSemanticKey.value = buildLayoutContentKey(currentParams)
        }
        break
      }
    }

    if (activeComputeSession === session) {
      activeComputeSession = 0
      isComputingInternal = false
      isComputingLayout.value = false
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
    const nextContext: NavigationContext = {
      type: 'search',
      itemIds: resultIds,
      currentIndex: resultIds.indexOf(id),
    }
    const nextDetail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id })
    if (generation !== detailRequestGeneration) return
    navContext.value = nextContext
    detailItem.value = nextDetail
    isDetailOpen.value = true
  }

  /**
   * 重复镜头(2026-09-02 方案 §8.2)从画廊打开查看器前建立导航上下文。
   * 只保存 layoutVersion/总数，实际相邻项由后端布局缓存解析，避免把 flat_ids 复制到前端。
   * 首次打开时 currentIndex 未知；第一次镜头导航响应会补齐它。路由化查看器的
   * loadFromRoute → openDetail(id)(不带 fromLayout)不会清掉本上下文,closeDetail 统一回收。
   */
  function setLensNavContext(layoutVersion: number, totalCount: number) {
    navContext.value = {
      type: 'lens',
      layoutVersion,
      totalCount,
      currentIndex: null,
    }
  }

  async function openDetail(id: number, fromLayout = false) {
    const generation = ++detailRequestGeneration
    // 从画廊打开时同步清空导航上下文（在 fetch 前）：否则在途窗口内 navContext 仍是旧搜索
    // 上下文，方向键会按旧列表导航；且 IPC 失败时若只在成功后清空，旧上下文将永久残留。
    if (fromLayout) {
      navContext.value = null
    }
    const nextDetail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id })
    if (generation !== detailRequestGeneration) return
    detailItem.value = nextDetail
    isDetailOpen.value = true
  }

  async function navigateDetail(offset: number) {
    if (!detailItem.value) return

    if (navContext.value?.type === 'lens') {
      const generation = ++detailRequestGeneration
      const context = navContext.value
      let result: LensAdjacentMedia | null = null
      try {
        result = await invokeIpc<LensAdjacentMedia | null>(IPC.GET_LENS_ADJACENT_MEDIA, {
          currentId: detailItem.value.id,
          offset,
          layoutVersion: context.layoutVersion,
        })
      } catch (e) {
        // 镜头布局过期/暂未就绪时保持当前项；浏览器绝不退回普通邻接序。
        logger.warn('[MediaStore] lens adjacent lookup failed; keeping current item', {
          error: e,
        })
      }
      // 只接受仍属于同一镜头上下文的响应；过期/边界均不退回普通邻接查询。
      if (generation === detailRequestGeneration && navContext.value === context && result) {
        context.currentIndex = result.index
        context.totalCount = result.totalCount
        detailItem.value = result.detail
      }
      return
    }

    if (navContext.value) {
      const nextIndex = navContext.value.currentIndex + offset
      if (nextIndex >= 0 && nextIndex < navContext.value.itemIds.length) {
        const generation = ++detailRequestGeneration
        const context = navContext.value
        const nextId = context.itemIds[nextIndex]
        const nextDetail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id: nextId })
        // currentIndex 只在响应提交时一并推进：提前推进会让在途窗口内索引超前于展示项，
        // 连按方向键时第三次按键会以错误的显示项为基准导航（跳项/落错位）。
        if (generation === detailRequestGeneration && navContext.value === context) {
          context.currentIndex = nextIndex
          detailItem.value = nextDetail
        }
      }
      return
    }

    const currentId = detailItem.value.id
    const generation = ++detailRequestGeneration
    const adj = await invokeIpc<MediaDetail | null>(IPC.GET_ADJACENT_MEDIA, {
      currentId,
      offset,
    })
    if (
      adj &&
      generation === detailRequestGeneration &&
      detailItem.value?.id === currentId
    ) {
      detailItem.value = adj
    }
  }

  function closeDetail() {
    detailRequestGeneration++
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
    layoutDirty.value = true
  }

  /** 消费脏标志（如果为脏则返回 true，然后重置）。 */
  function consumeLayoutDirty(): boolean {
    if (layoutDirty.value) {
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
    computeLayout,
    fetchRowsByY,
    fetchBucketRows,
    ensureMeta,
    openDetail,
    openDetailFromSearch,
    setLensNavContext,
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
