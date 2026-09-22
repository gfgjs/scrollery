// Canvas 网格图像缓存/加载/失败收口 + 视口外预取(idle+draw 尾双通道),从
// MediaGridCanvas.vue 下沉(方案 2.2 ⑤)。工厂在宿主 <script setup> 顶层只调用一次;
// draw()/drawCell 热路径只调用已绑定好的函数引用,不得在 rAF 回调或逐格循环内重新
// 构造闭包或再次调用本工厂(§3.1 红线)。
import { buildThumbUrl } from './useThumbLoader'
import { isThumbLoadDeferred } from './useThumbLoadGate'
import { DEFAULTS } from '../constants/defaults'
import {
  CANVAS_PREFETCH_AHEAD_FACTOR,
  CANVAS_PREFETCH_BEHIND_FACTOR,
} from '../utils/galleryPrefetchWindow'
import { createCanvasThumbState, type CachedThumb } from '../components/media/canvasThumbState'
import {
  bitmapBucketH,
  bitmapPrepParams,
  canvasPrefetchBudgets,
  canvasPrefetchItems,
  runPrefetchWalk,
} from '../components/media/mediaGridCanvas.helpers'
import type { LayoutRow, LayoutRowItem } from '../types/layout'
import { performanceRecorder } from '../perf/performanceRecorder'

// ── 图像缓存(命令式,非响应式)——真 LRU:命中即重插到队尾,驱逐从队首(最久未用)────
// 值为「解码期预缩放」产物:ImageBitmap(fetch → createImageBitmap,解码/缩放走 Chromium
// 解码线程池,主线程零阻塞;已裁到格纵横比 + 缩到高度桶,驻留内存较 480px 全图降约一个量级)
// 或 HTMLImageElement(createImageBitmap 不支持的源——SVG 无内在尺寸,故直显——的经典回退)。
// renderSig 记录位图规格(数据签名+高度桶+格纵横比):规格过期时旧图按 stale 续画、后台重备,
// thumbSize 滑杆拖动不闪占位。状态机本体(LRU/sig 失效/请求去重/失败裁决)抽在
// canvasThumbState 纯模块(②),由 characterization 测锁行为;本组件只做 IO 与绘制编排。
export type ThumbSrc = ImageBitmap | HTMLImageElement
export type CachedImage = CachedThumb<ThumbSrc>

export interface CanvasThumbPipelineOptions {
  cacheDir: () => string
  onRequestThumb: (id: number) => Promise<void>
  onCancelThumb: (id: number) => void
  onRegenerateThumb: (id: number) => void
  scheduleDraw: () => void
  /** 视口宽/高(每帧读取的热路径 getter):拆两个数值访问器而非单个 {w,h} 对象,
   * 避免 samePrefetchPlan/ensurePrefetchPlan 每帧新建对象字面量(热路径零新增分配)。 */
  viewportW: () => number
  viewportH: () => number
}

export function useCanvasThumbPipeline(opts: CanvasThumbPipelineOptions) {
  // 极密模式一屏可达数千格，固定 2000 会出现“当前屏尚未画完，最上方刚落地位图已被队尾
  // 驱逐”的自激抖动，故条目上限提高为 12000，让 60px / DPR1 场景主要由条目数保护；
  // DPR2/大卡片再由下方 512MB 字节预算封顶——两者任一达到即驱逐，内存上界不随之放大。
  const MAX_CACHE = 12_000
  // 字节预算(2026-07-10 审查 B11):条目上限与格子尺寸倒挂——顶桶(480)大格一条 ImageBitmap
  // ≈1-3MB，极密(64 桶)一条约几十 KB。512MB 与高条目上限双约束使缓存容纳极密模式可见屏+
  // 方向前瞻，大图模式仍由真实解码体积决定驱逐。
  const MAX_CACHE_BYTES = 512 * 1024 * 1024
  // 闸门放行时一屏可有数千个 480px 源图；逐格同步 fetch/createImageBitmap 会阻塞滚动主线程，
  // 故限制全局在途量，完成/失败释放槽位后由重绘继续补齐。64 足以持续喂满 Chromium 解码线程池，
  // 同时把一次放行的同步工作从 O(同屏格数)降到常数。
  const MAX_IN_FLIGHT_THUMBS = 64
  // 生成尚无 URL，不占位图 loading 槽；单独限制为两批，避免密集视口一次排入数千项。
  const MAX_THUMB_REQUESTS = DEFAULTS.THUMB_BATCH_SIZE * 2
  const thumbRequests = new Map<number, Promise<void>>()
  const visibleIds = new Set<number>()
  /** stale 可续画升级的让位余量:槽接近满时把最后若干槽留给真正缺图的冷格(升级靠后)。 */
  const STALE_UPGRADE_HEADROOM = 8
  /** 每侧保护条目上限,避免密集网格扫描无界增长。 */
  const NEAR_PROTECT_MAX_PER_SIDE = 512
  /**
   * 预取为可见需求保留的在途额度上限(全局 64 槽的 1/4)。不按 visibleDemand.size 全额保留:
   * 一屏冷格可达数百上千,全额保留会把前向预取整段饿死,反而让后续帧继续全冷;保留一个有界
   * 下限,既保证新入屏冷格在下一帧能立刻拿到槽,又让预取在同批完成回调之间仍有推进空间。
   */
  const VISIBLE_DEMAND_RESERVE_MAX = 16
  // ImageBitmap 须显式 close 释放解码内存(否则钉住到 GC);HTMLImageElement 交 GC。
  const thumbState = createCanvasThumbState<ThumbSrc>(
    MAX_CACHE,
    (src) => {
      if (src instanceof ImageBitmap) src.close()
    },
    {
      maxBytes: MAX_CACHE_BYTES,
      // SVG 回退的 HTMLImageElement:浏览器可自行回收栅格,按内在尺寸估算(常远小于位图)。
      byteCost: (src) =>
        src instanceof ImageBitmap
          ? src.width * src.height * 4
          : src.naturalWidth * src.naturalHeight * 4 || 0,
    },
  )

  interface AbortableThumbLoad {
    controller: AbortController
    /** true 仅表示仍在 fetch/读取 Blob，可安全中止并立即归还全局槽位。 */
    abortable: boolean
    /** 创建时的管线代次；不可取消解码完成也必须经过所有权校验。 */
    epoch: number
  }

  // 大跨度远跳时，旧视口尚在 fetch/blob 阶段的任务应让位给最新视口；已进入 createImageBitmap
  // 解码的任务不能真正取消，继续计入 loadingCount，避免“账面释放、实际仍解码”的隐性任务风暴。
  const thumbLoads = new Map<number, AbortableThumbLoad>()
  let pipelineEpoch = 0

  /**
   * 可见真实需求(本帧可见、无位图可画、有可加载源、尚未起载)的有限账本(S2):
   * 预取/idle 分片据此**保留**空槽,而不是等槽满了才去取消——否则「完成回调 → 下一 draw」
   * 之间 idle 仍可把 64 槽重新填满,新入屏的冷格整轮等不到服务。每帧由
   * prioritizeVisibleThumbLoads 重建;某项真正起载即出账(其占用的槽已是需求本身)。
   */
  const visibleDemand = new Set<number>()
  /** 腾槽的保留集(可见在途 + 可见需求 + 两侧近带)。逐帧 clear 复用,不每帧新建 Set。 */
  const protectIds = new Set<number>()

  // ── 有限录制期指标:候选等槽(§7.3 拆段)────────────────────────────────────────
  // 只记「真正缺图的格子」首次因槽满被挡下 → 实际起载的等待;非录制态零写入零分配,且录制
  // 会话代次(可连续 start/stop)变化时整体清空,上一会话的等待起点不污染新会话。
  const MAX_PENDING_WAITS = 2048
  const pendingWaitSince = new Map<number, number>()
  let pendingWaitEpoch = 0

  function notePendingWait(id: number): void {
    if (!performanceRecorder.isActive()) return
    if (!syncPendingWaitEpoch()) return
    if (pendingWaitSince.size >= MAX_PENDING_WAITS) {
      const oldest = pendingWaitSince.keys().next()
      if (!oldest.done) pendingWaitSince.delete(oldest.value)
    }
    if (!pendingWaitSince.has(id)) pendingWaitSince.set(id, performance.now())
  }

  function settlePendingWait(id: number): void {
    // 结算同样要对会话代次:上一会话留下的等待起点若被新会话的起载顺手取走,会记出一条
    // 跨越 stop/start 边界(甚至跨到未录制区间)的假等待。
    if (!syncPendingWaitEpoch()) return
    const since = pendingWaitSince.get(id)
    if (since === undefined) return
    pendingWaitSince.delete(id)
    performanceRecorder.recordSpan('gallery.thumbWait', performance.now() - since)
  }

  /** 会话代次对账:录制会话切换(可连续 start/stop)时清空等待账本;非录制态直接放弃记账。 */
  function syncPendingWaitEpoch(): boolean {
    if (!performanceRecorder.isActive()) {
      pendingWaitSince.clear()
      return false
    }
    const epoch = performanceRecorder.recordingEpoch()
    if (epoch !== pendingWaitEpoch) {
      pendingWaitEpoch = epoch
      pendingWaitSince.clear()
    }
    return true
  }

  // ── 异步分段的会话归属 ──────────────────────────────────────────────────────────
  // 每段跨 await:必须锚定**发起时**的录制会话,回包落在别的会话(或已停止)时一律不记,
  // 否则上一会话的 fetch/解码耗时会计进新会话的分布。
  function beginSpanCapture(): number | null {
    return performanceRecorder.isActive() ? performanceRecorder.recordingEpoch() : null
  }

  function canRecordCapture(capture: number | null): boolean {
    return (
      capture !== null && performanceRecorder.isActive() && performanceRecorder.recordingEpoch() === capture
    )
  }

  /**
   * 失活时 fetch 可以中止，但已进入解码的 Promise 不可取消。后者必须失去回写资格，
   * 仅在完成时释放 ImageBitmap，不能污染缓存或触发后台重绘。
   */
  function invalidateAsyncOwnership(releaseNonAbortable = true): void {
    pipelineEpoch++
    cancelThumbRequests()
    visibleIds.clear()
    pendingWaitSince.clear()
    visibleDemand.clear()
    cancelPrefetchPlan()
    for (const [id, load] of thumbLoads) {
      if (load.abortable) load.controller.abort()
      if (load.abortable || releaseNonAbortable) thumbState.cancelLoad(id)
      thumbLoads.delete(id)
    }
  }

  function isCurrentLoad(id: number, load: AbortableThumbLoad): boolean {
    return load.epoch === pipelineEpoch && thumbLoads.get(id) === load
  }

  /**
   * 取该格当前可画的图源;无可用源则(限并发)异步备装,返回 null 让本帧先画占位。Canvas
   * 可见区在快滚闸门期间仍允许加载;闸门只把预取窗收缩为仅前向,保证快速拖动也持续出真实图。
   * 两级失效:数据签名(status|path)变化 → 硬失效重载(等价 useThumbLoader 的 [thumbPath,
   * thumbStatus] watch,「生成完成」「封面自愈」自动刷新,不留裂图/旧图);渲染规格(高度桶/
   * 格纵横比)变化 → 旧位图按 stale 续画,后台重备新规格(thumbSize 滑杆/视图切换不闪占位)。
   * @param purpose 'visible' = 绘制循环里的可见格(可占用最后空槽);'prefetch' = 视口外预取,
   * 为可见真实需求保留额度,不越界抢槽。
   */
  function getImage(
    item: LayoutRowItem,
    purpose: 'visible' | 'prefetch' = 'visible',
  ): CachedImage | null {
    const id = item.id
    const sig = itemDataSig(item)
    thumbState.syncSig(id, sig)
    const cached = thumbState.get(id) ?? null
    const bucketH = bitmapBucketH(item.h, window.devicePixelRatio || 1)
    // 纵横比量化到 1/8 步:justified 的项纵横比 = 图片纵横比(容器 reflow 下不变量),量化挡掉
    // 布局取整的浮点噪声,避免 resize 抖动触发无谓重备。
    const renderSig = `${sig}#${bucketH}x${Math.round((item.w / Math.max(1, item.h)) * 8)}`
    if (cached && cached.renderSig === renderSig) return cached // 规格现行,飞掠中照常绘制
    // 至此:无图,或仅渲染规格过期——以下各分支一律返回 cached(stale 续画;无则占位)。
    if (thumbState.isFailed(id)) return cached
    if (thumbState.isLoading(id)) return cached

    // 可见区与预取共用同一在途上限。达到上限时保留 stale/占位；任一任务落定都会
    // scheduleDraw，再按当前最终视口继续补槽，避免旧视口任务风暴拖住滚动。
    const inFlight = thumbState.loadingCount()
    // 预取额外为可见需求留槽:可见冷格已有独立账本,预取不占用它们尚在等待的额度。
    if (purpose === 'prefetch' && inFlight >= prefetchInFlightLimit()) return cached
    if (inFlight >= MAX_IN_FLIGHT_THUMBS) {
      // 真正缺图(无任何可画位图)的格子等槽:记下起点,实际起载时结算为 gallery.thumbWait。
      if (purpose === 'visible' && !cached) notePendingWait(id)
      return cached
    }
    // stale 可续画升级靠后:位图已能画,槽接近满时先把余量留给真正缺图的格子(冷格优先)。
    if (cached && inFlight + STALE_UPGRADE_HEADROOM > MAX_IN_FLIGHT_THUMBS) return cached

    const url = buildThumbUrl(item.thumbStatus, item.thumbPath, opts.cacheDir())
    if (!url) {
      // 快滚只消费已经存在的缩略图/原图源；待生成项无法在当前帧显示，若逐格上抛会绕过
      // loadingCount（尚无 fetch）并一次创建数千 queue 请求，重新引入任务风暴。
      if (purpose === 'prefetch' || isThumbLoadDeferred()) return cached
      if (thumbRequests.has(id) || thumbRequests.size >= MAX_THUMB_REQUESTS) return cached
      // status 0(待生成)/ status 3 无路径 → 上抛一次请求(父层生成/解析后回填 → sig 变 → 自动重载)。
      if ((item.thumbStatus === 0 || item.thumbStatus === 3) && thumbState.requestThumbOnce(id)) {
        const request = opts.onRequestThumb(id)
        thumbRequests.set(id, request)
        const settle = () => {
          // 离屏后同 id 可能已重新请求；旧 Promise 不得释放新额度或在失活后触发绘制。
          if (thumbRequests.get(id) !== request) return
          thumbRequests.delete(id)
          opts.scheduleDraw()
        }
        void request.then(settle, settle)
      }
      return cached
    }
    // IO 本身异步，但 fetch/createImageBitmap 的同步创建仍有成本；全局在途上限负责波次推进，
    // 不让 60px 模式在一次 draw 内创建数千个 Promise/请求。
    settlePendingWait(id)
    visibleDemand.delete(id) // 该需求已起载:它占用的槽即需求本身,不再额外留槽
    const load: AbortableThumbLoad = {
      controller: new AbortController(),
      abortable: true,
      epoch: pipelineEpoch,
    }
    thumbState.markLoading(id)
    thumbLoads.set(id, load)
    void loadBitmap(id, url, sig, renderSig, item.w, item.h, bucketH, item.thumbStatus, load)
    return cached
  }

  function cancelThumbRequests(keep?: ReadonlySet<number>): void {
    for (const id of thumbRequests.keys()) {
      if (keep?.has(id)) continue
      thumbRequests.delete(id)
      thumbState.cancelThumbRequest(id)
      opts.onCancelThumb(id)
    }
  }

  /**
   * 解码期预缩放管线:fetch → createImageBitmap(解码在 Chromium 解码线程池,主线程零阻塞;
   * 对照经典路径:onload 只代表字节到位,首次 drawImage 才在主线程同步解码)→ cover 裁剪到
   * 格纵横比 + 缩到高度桶(只缩不放)。CSP connect-src 已含 asset:(阅读器线遗产),fetch 与
   * Image() 走同一 asset 协议与运行时授权。createImageBitmap 对无内在尺寸的源(SVG 直显)会
   * reject → 回退经典 Image() 解码;HTTP 层失败(404)不回退,免对同一坏 URL 二次请求。
   */
  async function loadBitmap(...args: Parameters<typeof loadBitmapImpl>): Promise<void> {
    const id = args[0]
    const load = args[8]
    const capture = beginSpanCapture()
    const startedAt = capture === null ? 0 : performance.now()
    try {
      await loadBitmapImpl(...args)
    } finally {
      // 已中止任务可能已从 Map 主动删除；身份复核避免迟到 finally 删掉同 id 的新代请求。
      if (thumbLoads.get(id) === load) thumbLoads.delete(id)
      // 端到端跨多个 await:回包落在别的录制会话(或已停止)时按发起会话作废,不记入新会话。
      if (canRecordCapture(capture)) {
        performanceRecorder.recordSpan('gallery.bitmapLoad', performance.now() - startedAt)
      }
    }
  }

  async function loadBitmapImpl(
    id: number,
    url: string,
    dataSig: string,
    renderSig: string,
    cellW: number,
    cellH: number,
    bucketH: number,
    status: number,
    load: AbortableThumbLoad,
  ) {
    // 录制期分段(§7.3):fetch/blob → 源解码 → 裁剪缩放。capture 锚定发起时的录制会话——
    // 未录制则不记,落在别的会话不记(避免会话开始前的加载或跨会话回包污染新会话分布);
    // 非录制态只多一次 isActive 判定,零额外分配。
    const capture = beginSpanCapture()
    const monitored = capture !== null
    let segmentAt = monitored ? performance.now() : 0
    let resp: Response
    try {
      resp = await fetch(url, { signal: load.controller.signal })
    } catch {
      if (load.controller.signal.aborted || !isCurrentLoad(id, load)) return
      // 网络层异常(协议/授权边缘):交经典路径再试一次,其 onerror 收口自愈/失败标记。
      load.abortable = false
      await loadViaImage(id, url, dataSig, renderSig, status, load)
      return
    }
    if (!resp.ok) {
      if (isCurrentLoad(id, load)) onLoadFailed(id, status, dataSig, load)
      return
    }
    try {
      const blob = await resp.blob()
      if (canRecordCapture(capture)) {
        const at = performance.now()
        performanceRecorder.recordSpan('gallery.thumbFetch', at - segmentAt)
        segmentAt = at
      }
      // Blob 已完整到内存；之后 abort 无法中断 createImageBitmap，继续占 loading 槽直至落定。
      load.abortable = false
      const full = await createImageBitmap(blob)
      if (canRecordCapture(capture)) {
        const at = performance.now()
        performanceRecorder.recordSpan('gallery.thumbSourceDecode', at - segmentAt)
        segmentAt = at
      }
      let out: ImageBitmap
      try {
        const p = bitmapPrepParams(full.width, full.height, cellW, cellH, bucketH)
        out =
          p.outW !== null && p.outH !== null
            ? await createImageBitmap(full, p.sx, p.sy, p.sw, p.sh, {
                resizeWidth: p.outW,
                resizeHeight: p.outH,
                resizeQuality: 'high',
              })
            : await createImageBitmap(full, p.sx, p.sy, p.sw, p.sh)
      } finally {
        // 裁剪/缩放阶段 reject 时也必须释放整图，否则回退 Image() 后会同时钉住两份解码资源。
        full.close()
      }
      if (canRecordCapture(capture)) {
        performanceRecorder.recordSpan('gallery.thumbPrep', performance.now() - segmentAt)
      }
      if (!isCurrentLoad(id, load)) {
        out.close()
        return
      }
      // 数据已换代(自愈/重生成)→ commitLoad 内部关闭位图并拒收,不重绘。
      if (!thumbState.commitLoad(id, dataSig, { src: out, renderSig })) return
      if (!isCurrentLoad(id, load)) return
      opts.scheduleDraw()
    } catch {
      if (load.controller.signal.aborted || !isCurrentLoad(id, load)) return
      // 解码 reject(SVG 无内在尺寸/坏图)→ 经典 Image() 回退(SVG 可正常栅格化,坏图则 onerror 收口)。
      load.abortable = false
      await loadViaImage(id, url, dataSig, renderSig, status, load)
    }
  }

  /**
   * 大跨度跳转时取消仍在 fetch/blob 的旧视口任务，立即把 64 槽让给最新视口。进入解码后的
   * 任务不动：createImageBitmap 无取消 API，伪释放只会让实际解码数突破全局上限。
   */
  function cancelAbortableThumbLoads(keepIds?: ReadonlySet<number>, freeSlots = MAX_IN_FLIGHT_THUMBS) {
    let cancelled = 0
    for (const [id, load] of thumbLoads) {
      if (thumbState.loadingCount() <= MAX_IN_FLIGHT_THUMBS - freeSlots) break
      if (!load.abortable) continue
      if (keepIds?.has(id)) continue
      load.controller.abort()
      thumbLoads.delete(id)
      if (thumbState.cancelLoad(id)) cancelled++
    }
    if (cancelled > 0 && performanceRecorder.isActive()) {
      performanceRecorder.count('gallery.bitmapLoadsCancelled', cancelled)
    }
  }

  /**
   * 可见需求优先(S1/S2),每帧绘制前调用一次,输出两件事:
   *  ① 重建 visibleDemand 账本——本帧可见、无位图可画、有可加载源、尚未起载的格子。预取/idle
   *     分片据此**保留**空槽,而不是等槽满了再取消;否则「完成回调 → 下一 draw」之间 idle 仍
   *     能把 64 槽重新填满,新入屏的冷格整轮等不到服务。
   *  ② 槽满时只为**真实缺图**的可见项数量腾槽:可见全热、或只剩 stale 可续画升级时一个也不
   *     取消(S1 旧实现按可见格数腾槽,「可见全热 + 槽满」也会取消 64 个前向任务)。腾槽对象
   *     排除可见在途项与前后近带(各 0.5 屏)内的在途预取,只取消更外侧、已无近期价值的
   *     fetch;已进入解码的任务不可中止,本就不在取消范围内,不因腾槽被伪释放。
   * 若近带 64 项全是在途预取,可让位者为零,冷格只能等它们落定——只保护,不承诺严格不等槽。
   */
  function prioritizeVisibleThumbLoads(rows: LayoutRow[], start: number, end: number) {
    visibleDemand.clear()
    protectIds.clear()
    visibleIds.clear()
    for (let i = start; i < end; i++) {
      const row = rows[i]
      if (row.rowType !== 'normal') continue
      for (const item of row.items) {
        const id = item.id
        if (thumbRequests.has(id)) visibleIds.add(id)
        // 先按当前 status/path 对账:旧签名留下的缓存/失败标记会把「换了源的真冷项」误判成
        // 有位图可画,于是既不腾槽也不记账。签名与 getImage 同源(itemDataSig)。
        thumbState.syncSig(id, itemDataSig(item))
        if (thumbState.isLoading(id)) {
          protectIds.add(id) // 可见在途:让位对象里没有它,不取消
          continue
        }
        if (thumbState.isFailed(id) || thumbState.peek(id) !== undefined) continue // 有位图可画
        if (!isLoadableThumbSource(item)) continue // 无可加载源(待生成/待解析)不占调度
        visibleDemand.add(id)
        protectIds.add(id)
      }
    }
    // 生成请求只保留实际视口；即使没有可加载 URL，也必须清掉离屏排队与退避请求。
    cancelThumbRequests(visibleIds)
    if (visibleDemand.size === 0 || thumbState.loadingCount() < MAX_IN_FLIGHT_THUMBS) return
    addNearProtectIds(rows, start, end, 1)
    addNearProtectIds(rows, start, end, -1)
    cancelAbortableThumbLoads(protectIds, Math.min(MAX_IN_FLIGHT_THUMBS, visibleDemand.size))
  }

  /** 数据签名(单源):硬失效判定与可见冷热分类必须读同一份,否则两套派生会各自漂移。 */
  function itemDataSig(item: LayoutRowItem): string {
    return `${item.thumbStatus}|${item.thumbPath ?? ''}`
  }

  /** 预取可用的在途额度 = 全局上限 − 为可见真实需求保留的有界下限(见 VISIBLE_DEMAND_RESERVE_MAX)。 */
  function prefetchInFlightLimit(): number {
    return MAX_IN_FLIGHT_THUMBS - Math.min(visibleDemand.size, VISIBLE_DEMAND_RESERVE_MAX)
  }

  /** 该行项当前是否有可加载的缩略图源(status 1 已生成 / status 3 直显,且路径已在)。 */
  function isLoadableThumbSource(item: LayoutRowItem): boolean {
    return (item.thumbStatus === 1 || item.thumbStatus === 3) && !!item.thumbPath
  }

  /** 两侧各保护半屏,保留刚离屏且可能回滚复用的 fetch;按行距和条目数止步。 */
  function addNearProtectIds(rows: LayoutRow[], start: number, end: number, step: 1 | -1) {
    if (start >= end) return
    const edge = rows[step > 0 ? end - 1 : start]
    const edgeY = step > 0 ? edge.y + edge.height : edge.y
    const limitY = edgeY + step * opts.viewportH() * CANVAS_PREFETCH_BEHIND_FACTOR
    let count = 0
    for (let i = step > 0 ? end : start - 1; i >= 0 && i < rows.length; i += step) {
      const row = rows[i]
      if (step > 0 ? row.y > limitY : row.y + row.height < limitY) break
      if (row.rowType !== 'normal') continue
      for (const item of row.items) {
        protectIds.add(item.id)
        if (++count >= NEAR_PROTECT_MAX_PER_SIDE) return
      }
    }
  }

  /**
   * 经典 Image() 解码回退:缓存整图,draw 端 coverRect 裁剪(与旧路径行为逐字一致)。
   * 返回 Promise 交由调用方 await —— 回退在 onload/onerror 之前**不得**归还任务身份:身份一被
   * loadBitmap 的 finally 删除,isCurrentLoad 就会拒绝迟到回包,既不提交位图也不释放 loading
   * 槽,该格从此长期停在占位并占住一个全局在途额度(§9.2 资源所有权缺口)。
   */
  function loadViaImage(
    id: number,
    url: string,
    dataSig: string,
    renderSig: string,
    status: number,
    load: AbortableThumbLoad,
  ): Promise<void> {
    // 回退同样跨 await(onload/onerror):锚定发起会话,回包落在别的会话时不计入新会话。
    const capture = beginSpanCapture()
    const startedAt = capture === null ? 0 : performance.now()
    return new Promise<void>((settle) => {
      const img = new Image()
      img.onload = () => {
        if (canRecordCapture(capture)) {
          performanceRecorder.recordSpan('gallery.imageFallback', performance.now() - startedAt)
          performanceRecorder.count('gallery.imageFallbackLoaded')
        }
        // 数据已变 → commitLoad 拒收(HTMLImageElement 无需显式释放,closeSrc no-op 交 GC)。
        if (isCurrentLoad(id, load) && thumbState.commitLoad(id, dataSig, { src: img, renderSig })) {
          if (isCurrentLoad(id, load)) opts.scheduleDraw()
        }
        settle()
      }
      img.onerror = () => {
        if (canRecordCapture(capture)) {
          performanceRecorder.recordSpan('gallery.imageFallback', performance.now() - startedAt)
          performanceRecorder.count('gallery.imageFallbackFailed')
        }
        if (isCurrentLoad(id, load)) onLoadFailed(id, status, dataSig, load)
        settle()
      }
      img.src = url
    })
  }

  /** 加载失败收口:已生成却 404(多为封面被 LRU 驱逐)→ 上抛一次自愈;标失败停重试。父层复位
   *  status=0 后 sig 变 → 失败态清 → 重生成完成再自动加载(修复 MVP 的「裂图永久缓存」)。
   *  陈旧守卫(0597a19,裁决在 failLoad 内):数据已换代的在途失败一律作废——否则旧 sig 的
   *  迟到 404 会重复 emit 自愈,且把失败标记毒化到新 sig 头上。 */
  function onLoadFailed(id: number, status: number, dataSig: string, load: AbortableThumbLoad) {
    if (!isCurrentLoad(id, load)) return
    const verdict = thumbState.failLoad(id, status, dataSig)
    if (verdict === 'heal') opts.onRegenerateThumb(id)
    // 失败同样释放全局在途槽；若不重绘，最后一波恰好全失败时后续可见项会永久等不到补槽。
    if (verdict !== 'stale' && isCurrentLoad(id, load)) opts.scheduleDraw()
  }

  // ── 视口外预取(对齐 DOM「挂载窗口大于视口」的隐式预载)────────────────────────────
  // 「像素余量 × 自适应条目预算」双约束:开闸态前方约 1.25 屏、后方约 0.5 屏;关闸态
  // (飞掠/拖拽)收缩为仅前向(1.25 屏)、后方 0——2026-07-17 阶段 6 实测(headless 基准,
  // burst 速度剖面)证明「关闸=停一切视口外预取」正是稍快速滚动可见换图波的根因(滞回带
  // 让闸门整段钉住,格子进视口才起载,一个加载时延后才逐格弹出)。前向宽度实测定档
  // (1.0 屏余 ~939 冷格·帧、1.25 屏余 ~530,基线 4911),故与开闸态同宽;真飞掠期的浪费
  // 上界仍由 64 全局在途槽 + prioritizeVisibleThumbLoads 的旧 fetch 取消兜底。
  // 推进双通道:idle 小分片(深填)+ draw 尾同步小分片(保底)——真机 WebView2 滚动帧
  // 远忙于 headless,idle 空隙不可靠;draw 尾分片有硬墙钟上限,不与绘制抢帧。
  // 屏数因子单点定义在 utils/galleryPrefetchWindow(B 的窗口契约同源),避免「1.25 屏」在多处
  // 各写一份字面量后各自漂移;关闸态后向为 0,由 canvasPrefetchBudgets 的 behindFactor ≤ 0 语义表达。
  const GATED_PREFETCH_BEHIND_FACTOR = 0
  const PREFETCH_SLICE_MAX_STARTS = 64
  const PREFETCH_SLICE_BUDGET_MS = 2
  const PREFETCH_DRAW_MAX_STARTS = 16
  const PREFETCH_DRAW_BUDGET_MS = 1
  const PREFETCH_IDLE_TIMEOUT_MS = 80

  interface PrefetchPlan {
    rows: LayoutRow[]
    y0: number
    viewportW: number
    viewportH: number
    firstVisible: number
    lastVisible: number
    direction: -1 | 0 | 1
    visibleCells: number
    /** 关闸态计划(仅前向、后方 0):与开闸态计划互斥,闸门翻转即重建。 */
    gated: boolean
    iterators: Generator<LayoutRowItem>[]
    iteratorIndex: number
    processed: number
    done: boolean
  }

  let prefetchPlan: PrefetchPlan | null = null
  let prefetchIdleId: number | null = null
  let prefetchTimerId: ReturnType<typeof setTimeout> | null = null
  let scrollDirection: -1 | 0 | 1 = 0

  function cancelScheduledPrefetch() {
    if (prefetchIdleId !== null) {
      window.cancelIdleCallback(prefetchIdleId)
      prefetchIdleId = null
    }
    if (prefetchTimerId !== null) {
      globalThis.clearTimeout(prefetchTimerId)
      prefetchTimerId = null
    }
  }

  function cancelPrefetchPlan() {
    cancelScheduledPrefetch()
    prefetchPlan = null
  }

  /** KeepAlive 失活：保留 LRU，但撤销旧请求及其异步回写资格。 */
  function pause() {
    invalidateAsyncOwnership()
  }

  function samePrefetchPlan(
    plan: PrefetchPlan | null,
    rows: LayoutRow[],
    y0: number,
    firstVisible: number,
    lastVisible: number,
    visibleCells: number,
    gated: boolean,
  ): boolean {
    return (
      plan !== null &&
      plan.rows === rows &&
      plan.y0 === y0 &&
      plan.viewportW === opts.viewportW() &&
      plan.viewportH === opts.viewportH() &&
      plan.firstVisible === firstVisible &&
      plan.lastVisible === lastVisible &&
      plan.direction === scrollDirection &&
      plan.visibleCells === visibleCells &&
      plan.gated === gated
    )
  }

  function nextPrefetchItem(plan: PrefetchPlan): LayoutRowItem | null {
    while (plan.iteratorIndex < plan.iterators.length) {
      const next = plan.iterators[plan.iteratorIndex].next()
      if (!next.done) return next.value
      plan.iteratorIndex++
    }
    plan.done = true
    return null
  }

  /**
   * 沿计划游标推进一片(idle 与 draw 尾两通道共用):预算按**新启动数**计——暖项
   * (已缓存/在途)走查交给墙钟上限,不再吃掉启动预算(阶段 6 根因之二)。
   */
  function walkPlan(
    plan: PrefetchPlan,
    deadline: IdleDeadline | null,
    maxStarts: number,
    budgetMs: number,
  ): boolean {
    const result = runPrefetchWalk(
      {
        next: () => nextPrefetchItem(plan),
        ensure: (item) => void getImage(item, 'prefetch'),
        isLoading: (id) => thumbState.isLoading(id),
        loadingCount: () => thumbState.loadingCount(),
        now: () => performance.now(),
        // didTimeout 的兜底回调没有真实余量语义,视作无 deadline(仍受墙钟/启动预算约束)。
        idleTimeRemaining: deadline && !deadline.didTimeout ? () => deadline.timeRemaining() : null,
      },
      { maxStarts, budgetMs, maxInFlight: prefetchInFlightLimit() },
    )
    if (result.exhausted) plan.done = true
    plan.processed += result.started
    if (performanceRecorder.isActive()) {
      performanceRecorder.setGauge('gallery.prefetchedCells', plan.processed)
      performanceRecorder.count('gallery.prefetchSlices')
      performanceRecorder.count('gallery.prefetchLoadsStarted', result.started)
    }
    return result.exhausted
  }

  function runPrefetchSlice(deadline: IdleDeadline | null) {
    prefetchIdleId = null
    prefetchTimerId = null
    const plan = prefetchPlan
    if (!plan || plan.done) return
    walkPlan(plan, deadline, PREFETCH_SLICE_MAX_STARTS, PREFETCH_SLICE_BUDGET_MS)
    if (!plan.done && thumbState.loadingCount() < prefetchInFlightLimit()) schedulePrefetchSlice()
  }

  /**
   * draw 尾同步小分片:滚动中每帧保底推进预取,不依赖 idle 空隙(真机 WebView2 帧忙时
   * rIC 可整段饿死)。硬预算 1ms/16 启动,64 全局槽满时直接让路,不与绘制/可见区抢资源。
   */
  function runDrawPrefetchSlice() {
    const plan = prefetchPlan
    if (!plan || plan.done) return
    // 可见真实需求尚在等槽时,draw 尾分片把空槽留给它们,不占用预留额度。
    if (thumbState.loadingCount() >= prefetchInFlightLimit()) return
    walkPlan(plan, null, PREFETCH_DRAW_MAX_STARTS, PREFETCH_DRAW_BUDGET_MS)
  }

  function schedulePrefetchSlice() {
    if (!prefetchPlan || prefetchPlan.done || prefetchIdleId !== null || prefetchTimerId !== null) {
      return
    }
    if ('requestIdleCallback' in window) {
      prefetchIdleId = window.requestIdleCallback(runPrefetchSlice, {
        timeout: PREFETCH_IDLE_TIMEOUT_MS,
      })
    } else {
      // 旧 WebView 无 rIC 时仍切到下一宏任务，并由 2ms/64 项双上限保护主线程。
      prefetchTimerId = globalThis.setTimeout(() => runPrefetchSlice(null), 0)
    }
  }

  function ensurePrefetchPlan(
    rows: LayoutRow[],
    y0: number,
    firstVisible: number,
    lastVisible: number,
    visibleCells: number,
    gated: boolean,
  ) {
    if (samePrefetchPlan(prefetchPlan, rows, y0, firstVisible, lastVisible, visibleCells, gated)) {
      schedulePrefetchSlice()
      return
    }

    cancelPrefetchPlan()
    const viewW = opts.viewportW()
    const viewH = opts.viewportH()
    // 关闸态收缩:仅滚动方向前向,后方 0(behindFactor ≤ 0 → 预算严格 0)。
    const aheadFactor = CANVAS_PREFETCH_AHEAD_FACTOR
    const behindFactor = gated ? GATED_PREFETCH_BEHIND_FACTOR : CANVAS_PREFETCH_BEHIND_FACTOR
    const budgets = canvasPrefetchBudgets(visibleCells, aheadFactor, behindFactor)
    const aheadStep: 1 | -1 = scrollDirection < 0 ? -1 : 1
    const aheadStart = aheadStep > 0 ? lastVisible + 1 : firstVisible - 1
    const behindStart = aheadStep > 0 ? firstVisible - 1 : lastVisible + 1
    prefetchPlan = {
      rows,
      y0,
      viewportW: viewW,
      viewportH: viewH,
      firstVisible,
      lastVisible,
      direction: scrollDirection,
      visibleCells,
      gated,
      iterators: [
        canvasPrefetchItems(rows, aheadStart, aheadStep, y0, viewH, viewH * aheadFactor, budgets.ahead),
        canvasPrefetchItems(
          rows,
          behindStart,
          aheadStep === 1 ? -1 : 1,
          y0,
          viewH,
          viewH * behindFactor,
          budgets.behind,
        ),
      ],
      iteratorIndex: 0,
      processed: 0,
      done: false,
    }
    schedulePrefetchSlice()
  }

  function setScrollDirection(dir: -1 | 0 | 1) {
    scrollDirection = dir
  }

  function getPrefetchProgress(): number {
    return prefetchPlan?.processed ?? 0
  }

  /** 组件卸载清理(§3.4:宿主 onBeforeUnmount 显式调用,顺序须复刻现状不变量)。 */
  function dispose() {
    // 不可取消的经典回退不额外触发 cancelLoad；clear 会统一清账，迟到回包仍被 epoch 拒绝。
    invalidateAsyncOwnership(false)
    thumbState.clear() // ImageBitmap 须显式释放,不能只清 Map(closeSrc 在 clear 内逐条执行)
  }

  return {
    getImage,
    prioritizeVisibleThumbLoads,
    ensurePrefetchPlan,
    runDrawPrefetchSlice,
    cancelPrefetchPlan,
    pause,
    setScrollDirection,
    dispose,
    thumbState,
    getPrefetchProgress,
  }
}

export type CanvasThumbPipeline = ReturnType<typeof useCanvasThumbPipeline>
