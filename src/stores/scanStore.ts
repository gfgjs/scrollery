// src/stores/scanStore.ts
// 扫描状态管理

import { defineStore } from 'pinia'
import { ref, computed, effectScope } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { generateOperationId, invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { listen } from '@tauri-apps/api/event'
import { useTauriListen } from '../composables/useTauriListen'
import type { ScanRoot } from '../types/media'
import type {
  ScanChannelPayload,
  MediaEnrichedPayload,
  EnrichmentCompletedPayload,
} from '../types/ipc'
import { IPC, EVENTS } from '../constants/ipc'
import i18n from '../i18n'
import { useMediaStore } from './mediaStore'
import { useUiStore } from './uiStore'
import { useToastStore } from './toastStore'

export interface ScanProgress {
  runId: string
  scanned: number
  total: number
  processedBytes: number
  totalBytes: number | null
  currentDir: string
  isRunning: boolean
  status?: 'discovering' | 'scanning' | 'enriching'
}

export interface ScanAggregateProgress {
  phase: 'discovering' | 'scanning' | 'enriching'
  processedFiles: number
  totalFiles: number | null
  processedBytes: number
  totalBytes: number | null
  currentDir: string
  rootCount: number
}

export const useScanStore = defineStore('scan', () => {
  const scanRoots = ref<ScanRoot[]>([])
  const progressMap = ref<Record<number, ScanProgress>>({})
  const isLoadingRoots = ref(false)
  let rootsRequestSerial = 0
  let databaseClearBarrier: Promise<void> | null = null

  const hasScanRoots = computed(() => scanRoots.value.length > 0)
  // 可见根（V21）：隐藏根不进侧栏文件树 / 空态判定。设置页显隐列表用全量 scanRoots（含隐藏，才能开关回来）。
  const visibleScanRoots = computed(() => scanRoots.value.filter((r) => !r.isHidden))
  const isAnyScanRunning = computed(() => Object.values(progressMap.value).some((p) => p.isRunning))
  const aggregateProgress = computed<ScanAggregateProgress | null>(() => {
    const active = Object.values(progressMap.value).filter((p) => p.isRunning)
    if (active.length === 0) return null

    const phase = active.some((p) => p.status === 'enriching')
      ? 'enriching'
      : active.some((p) => p.status === 'scanning')
        ? 'scanning'
        : 'discovering'
    const totalBytesKnown = active.every((p) => p.totalBytes !== null)
    const totalFilesKnown = active.every((p) => p.status === 'enriching' || p.total > 0)

    return {
      phase,
      processedFiles: active.reduce((sum, p) => sum + p.scanned, 0),
      totalFiles: totalFilesKnown ? active.reduce((sum, p) => sum + p.total, 0) : null,
      processedBytes: active.reduce((sum, p) => sum + p.processedBytes, 0),
      totalBytes: totalBytesKnown
        ? active.reduce((sum, p) => sum + (p.totalBytes ?? 0), 0)
        : null,
      currentDir: active.length === 1 ? (active[0].currentDir ?? '') : '',
      rootCount: active.length,
    }
  })

  function scanRunKey(rootId: number, runId: string) {
    return `${rootId}:${runId}`
  }

  // 终态 tombstone 防止同一轮已 stop/error/watchdog 后，已经排队的 Channel 或事件
  // 把 isRunning 重新写回 true。新轮启动时会清理该根旧 tombstone，避免长期累积。
  const terminalScanRuns = new Set<string>()

  // 扫描期统计刷新门限(跨根全局,非每根各一份):首条进度立即刷,其后最多每 2s 一次。
  // 多根并发扫描共享同一时间戳,避免每个根的首批进度各自绕过门限造成刷新洪峰。
  const SCAN_STATS_REFRESH_MS = 2000
  // 初值取 -Infinity 而非 0:门限是严格小于比较,时钟从 0 起步时首条进度也必须立即刷新。
  let lastScanStatsRefreshAt = Number.NEGATIVE_INFINITY

  /**
   * 扫描期刷新统计。force 用于有效终态(快速入库完成 / 富化完成 / stop / error)——
   * 终态必须让统计立刻反映最终计数,不再排队等门限。
   * fire-and-forget:调用点都在事件/Channel 回调深处,失败只记日志。
   */
  function refreshScanStats(force = false) {
    const now = Date.now()
    if (!force && now - lastScanStatsRefreshAt < SCAN_STATS_REFRESH_MS) return
    lastScanStatsRefreshAt = now
    void useMediaStore()
      .loadStats()
      .catch((e) => logger.error('[ScanStore] scan stats refresh FAILED', { error: e }))
  }

  function clearRootRunState(rootId: number) {
    delete progressMap.value[rootId]
    for (const key of enrichmentDone) {
      if (key.startsWith(`${rootId}:`)) enrichmentDone.delete(key)
    }
    for (const key of terminalScanRuns) {
      if (key.startsWith(`${rootId}:`)) terminalScanRuns.delete(key)
    }
    clearEnrichWatchdog(rootId)
    fullScanCompleted.delete(rootId)
  }

  function markScanRunTerminal(rootId: number, runId: string) {
    terminalScanRuns.add(scanRunKey(rootId, runId))
    clearEnrichWatchdog(rootId)
    const progress = progressMap.value[rootId]
    if (progress?.runId === runId) progress.isRunning = false
  }

  function restoreScanRunAfterStopFailure(
    rootId: number,
    runId: string,
    previous: ScanProgress,
  ) {
    // STOP 失败时撤销本地 tombstone，允许后端仍在运行的迟到事件继续恢复进度。
    // 若用户已经启动同根新轮，不能把旧快照写回新轮。
    if (progressMap.value[rootId]?.runId !== runId) return
    terminalScanRuns.delete(scanRunKey(rootId, runId))
    progressMap.value[rootId] = { ...previous }
    if (previous.isRunning && previous.status === 'enriching') {
      armEnrichWatchdog(rootId, runId)
    }
  }

  function isCurrentScanRunAcceptingEvents(rootId: number, runId: string) {
    return (
      progressMap.value[rootId]?.runId === runId &&
      !terminalScanRuns.has(scanRunKey(rootId, runId))
    )
  }

  function isSameRootRun(rootId: number, expectedRunId: string | undefined) {
    return progressMap.value[rootId]?.runId === expectedRunId
  }

  async function waitForDatabaseClear() {
    while (databaseClearBarrier) {
      const barrier = databaseClearBarrier
      await barrier
    }
  }

  function isCurrentRootRequest(requestSerial: number) {
    return requestSerial === rootsRequestSerial && databaseClearBarrier === null
  }

  async function loadScanRoots(): Promise<boolean> {
    let waitedForDatabaseClear = false
    if (databaseClearBarrier) {
      waitedForDatabaseClear = true
      await waitForDatabaseClear()
    }
    const requestSerial = ++rootsRequestSerial
    isLoadingRoots.value = true
    let applied = false
    try {
      const roots = await invokeIpc<ScanRoot[]>(IPC.LIST_SCAN_ROOTS)
      // 根管理动作或更新的刷新请求可能在本请求完成前改变列表；旧响应不得覆盖
      // 新状态（尤其是 remove/add 后的设置页列表）。
      if (isCurrentRootRequest(requestSerial)) {
        scanRoots.value = roots
        applied = true
      }
    } finally {
      if (isCurrentRootRequest(requestSerial)) isLoadingRoots.value = false
    }
    // 调用方通常只关心列表本身；返回值供有后续副作用的流程（如 relink 兜底重扫）
    // 判断这次响应是否跨过了清库屏障或被更新请求淘汰。
    return applied && !waitedForDatabaseClear
  }

  async function addScanRoot(path: string, alias?: string): Promise<ScanRoot> {
    if (databaseClearBarrier) await waitForDatabaseClear()
    const requestSerial = ++rootsRequestSerial
    const root = await invokeIpc<ScanRoot>(IPC.ADD_SCAN_ROOT, { path, alias: alias ?? null })
    if (
      isCurrentRootRequest(requestSerial) &&
      !scanRoots.value.some((r) => r.id === root.id)
    ) {
      scanRoots.value.push(root)
    }
    return root
  }

  async function removeScanRoot(id: number, clearThumbnails = false) {
    if (databaseClearBarrier) await waitForDatabaseClear()
    const expectedRunId = progressMap.value[id]?.runId
    const requestSerial = ++rootsRequestSerial
    if (expectedRunId !== undefined) markScanStopped(id, expectedRunId)
    try {
      const result = clearThumbnails
        ? await invokeIpc<{ cleared_count: number }>(IPC.REMOVE_SCAN_ROOT_WITH_OPTIONS, {
            id,
            clearThumbnails,
          })
        : (await invokeIpc(IPC.REMOVE_SCAN_ROOT, { id }), { cleared_count: 0 })
      // 删除请求可能在等待期间被同根新扫描替换；旧请求不得清理新轮的进度。
      if (!isSameRootRun(id, expectedRunId) || !isCurrentRootRequest(requestSerial)) {
        return result
      }
      scanRoots.value = scanRoots.value.filter((r) => r.id !== id)
      clearRootRunState(id)
      return result
    } catch (error) {
      // 后端先取消扫描再执行删除；即使删除失败，前端也不能继续显示已无 token 的运行态。
      if (expectedRunId !== undefined) markScanStopped(id, expectedRunId)
      throw error
    }
  }

  // 设置页库级显隐（V21）：后端翻转 is_hidden 并 bump data_version（画廊/统计取数缓存失效）。
  // 本地同步该项 isHidden，并触发画廊重排 + 统计刷新，使当前视图立即反映显隐。
  async function setScanRootHidden(id: number, hidden: boolean) {
    if (databaseClearBarrier) await waitForDatabaseClear()
    const requestSerial = ++rootsRequestSerial
    await invokeIpc(IPC.SET_SCAN_ROOT_HIDDEN, { id, hidden })
    if (!isCurrentRootRequest(requestSerial)) return
    const root = scanRoots.value.find((r) => r.id === id)
    if (root) root.isHidden = hidden
    const media = useMediaStore()
    media.invalidateLayout()
    // 显隐直接影响计数 → 强制刷新统计(同时补上失败日志,不留裸 fire-and-forget)。
    refreshScanStats(true)

    // 取消隐藏（unhide）补跑（V21）：被排除期间该根媒体的 thumb/ai/face 状态仍停在 pending
    // （生成侧三条流水线跳过隐藏根，见 db::queries::scan::EXCLUDE_HIDDEN_ROOTS）。重新可见后：
    //   · 缩略图：无需在此显式触发——画廊可见格 on-demand 路径（batch_request_thumbnails）会在
    //     这些项滚入视野时按需生成（status=0 落 needs_gen）；显式全库 incremental 反会连带扫出
    //     其它根的未生成积压，是意外耦合，故不做。
    //   · AI / 人脸：无按需路径，靠 maybeAutoResume 续跑——其内部判 analysisActive && pending>0，
    //     仅当用户已在跑该分析时才补，未启用则空操作（不擅自开销 GPU 重活）。
    if (!hidden) {
      const [{ useAiStore }, { useFaceStore }] = await Promise.all([
        import('./aiStore'),
        import('./faceStore'),
      ])
      if (!isCurrentRootRequest(requestSerial)) return
      void useAiStore().maybeAutoResume()
      void useFaceStore().maybeAutoResume()
      // · 派生封面(video_cover/audio_cover/epub doc_thumb):status=0 行被消费口跳过后
      //   原地暂停,踢一次流水线即续跑。守卫与 useDerivationAutoStart.kick 同款:已在
      //   运行则不重启(start_derivation 会 cancel+restart,把在途任务打回待处理白白重来)。
      //   不走 db:media_enriched 事件——本 store 的监听器会把它当富化进度,置幽灵
      //   isRunning + 10 分钟看门狗。pdf/svg 前端泵组件内部无法在此直踢,靠下次泵触发
      //   (app 启动首泵/可见性变化/任意 media_enriched)续跑。
      const derivationStatus = await invokeIpc<{ isRunning: boolean }>(IPC.DERIVATION_STATUS).catch(
        () => null,
      )
      if (!isCurrentRootRequest(requestSerial)) return
      if (!derivationStatus?.isRunning) {
        void invokeIpc(IPC.START_DERIVATION).catch(() => {})
      }
    }
  }

  // 全局 enrichment 监听 — 仅注册一次。后台 enrichment（EXIF + 尺寸补全）现在是
  // 耗时大头，其进度在 `enriching` 阶段驱动进度条，远在（如今近乎瞬时的）快速入库之后。
  //
  // ⚠️ 传输时序契约(「正在扫描…」永不消失的长期 bug 根因):快扫的 `'completed'` 走
  // Tauri **Channel**,而 `enrichment:completed` 走 Tauri **事件**系统——两条传输彼此
  // **无序**。无图可富化的文件夹(纯视频已补全/重扫)下富化近乎瞬时,`enrichment:completed`
  // 可能先于 Channel 的 `'completed'` 被处理;此前 completed 处理器无条件把状态拉回
  // `{isRunning:true}`,再无任何事件来清零 → 状态栏永久「正在扫描…」。
  // 防线一:enrichmentDone 终态账本——富化终态一旦到达,晚到的 completed 不得复活运行态;
  // 防线二:富化怠速看门狗——enriching 态下事件断流超阈值即判定管线已死,强制收尾
  // (阈值取 10 分钟:视频批 500 项 × MF 逐项探测,单批静默期可达数分钟,不可误杀)。
  // quick 增量扫描账本:首轮必须全量(无目录 mtime 基线),快扫完成后该根才具备
  // quick 剪枝基线(directories.mtime/media_count),此后手动重扫/relink 兜底重扫默认 quick。
  const fullScanCompleted = new Set<number>()
  const enrichmentDone = new Set<string>()
  const enrichWatchdogs = new Map<number, ReturnType<typeof setTimeout>>()
  const ENRICH_WATCHDOG_MS = 10 * 60_000

  function clearEnrichWatchdog(rootId: number) {
    const timer = enrichWatchdogs.get(rootId)
    if (timer !== undefined) {
      clearTimeout(timer)
      enrichWatchdogs.delete(rootId)
    }
  }
  function armEnrichWatchdog(rootId: number, runId: string) {
    clearEnrichWatchdog(rootId)
    enrichWatchdogs.set(
      rootId,
      setTimeout(() => {
        enrichWatchdogs.delete(rootId)
        const p = progressMap.value[rootId]
        if (p?.runId === runId && p.isRunning && p.status === 'enriching') {
          markScanRunTerminal(rootId, runId)
        }
      }, ENRICH_WATCHDOG_MS),
    )
  }

  let enrichListenersReady = false
  // U-1(方案 B):useTauriListen 要求同步 effect scope 上下文(内部 onScopeDispose 仲裁
  // 「listen 落定 vs 作用域销毁」竞态,见该 composable 头注),而本函数是异步惰性函数,
  // 首次调用时并无天然的组件/composable scope 可挂。手工开一个 effectScope 兜底,
  // 在其 run() 的同步回调内调用 useTauriListen——语义等价于原裸 listen()（进程内长驻,
  // 从不解绑)：本 store 是全局单例、无 $dispose/reset 会拆监听的路径,故此 scope 生命周期
  // = 应用级,故意不留 scope.stop() 调用点(若未来 store 长出销毁路径,记得在此接 stop())。
  // 用 detached scope(effectScope(true)):否则若首次触发恰在某个活动 Vue scope 内,本 scope
  // 会挂为其子 scope,父 scope 卸载时连带 stop 拆掉全局监听,「应用级」主张即失效。
  // enrichListenersReady 在任何 await/异步间隙之前就同步置真,并发重复调用天然幂等
  // （第二次调用的同步检查必然先于第一次调用内部任何异步续行观察到 true）。
  // 注册系 fire-and-forget:不再保证 startScan 续行前监听已挂活;富化事件远滞后于扫描启动
  // 且 enrichmentDone 幂等,此弱化可接受(2026-07-21 审查裁定)。
  function ensureEnrichmentListeners() {
    if (enrichListenersReady) return
    enrichListenersReady = true
    const scope = effectScope(true)
    scope.run(() => {
      useTauriListen<MediaEnrichedPayload>(EVENTS.MEDIA_ENRICHED, (e) => {
        const { rootId, runId, enrichedCount, total, processedBytes, totalBytes } = e.payload
        // 幽灵运行态双防线(修「扫描完状态栏仍显示正在扫描」):
        // ① 派生流水线封面落地会复用本事件发画廊刷新哨兵(rootId=0,见 derive/pipeline.rs
        //    write_results),它不是富化进度——照单全收会给不存在的根 0 造出 isRunning 条目,
        //    isAnyScanRunning 即恒真,只能等 10 分钟看门狗兜底。
        // ② 只接受当前确实在跑的根的进度:非运行态/未知根的事件一律忽略,迟到事件
        //    不再复活已收尾的运行态(原 enrichmentDone 防御被此判定包含,保留作显式语义)。
        const tracked = progressMap.value[rootId]
        if (
          rootId === 0 ||
          !runId ||
          !tracked?.isRunning ||
          !isCurrentScanRunAcceptingEvents(rootId, runId) ||
          enrichmentDone.has(scanRunKey(rootId, runId))
        )
          return
        progressMap.value[rootId] = {
          runId,
          scanned: enrichedCount,
          total,
          currentDir: '',
          processedBytes,
          totalBytes,
          isRunning: true,
          status: 'enriching',
        }
        armEnrichWatchdog(rootId, runId)
      })
      useTauriListen<EnrichmentCompletedPayload>(EVENTS.ENRICHMENT_COMPLETED, (e) => {
        const { rootId, runId, errorCode } = e.payload
        const tracked = progressMap.value[rootId]
        if (!tracked || !isCurrentScanRunAcceptingEvents(rootId, runId)) return
        enrichmentDone.add(scanRunKey(rootId, runId))
        markScanRunTerminal(rootId, runId)
        // 后台补全异常终止（T11 可观测）：携带稳定 errorCode → 弹 warning，
        // 让失败对用户可见，而非伪装成「正常完成」只进日志。取消不带 code，不打扰。
        if (errorCode) {
          const toast = useToastStore()
          toast.addToast('warning', i18n.global.t('statusbar.enrichIncomplete'), 6000)
        }
        // 终态强制刷新:使全部补全后的计数精确(不等门限)。
        refreshScanStats(true)
      })
    })
  }

  async function startScan(rootId: number, onComplete?: () => void, quickOverride?: boolean) {
    if (databaseClearBarrier) await waitForDatabaseClear()
    ensureEnrichmentListeners()
    const runId = generateOperationId()
    // 默认策略:已有过成功全量扫描的根走 quick(未变目录在 walker 层免 per-file stat);
    // 首次扫描/显式 false 走全量建立基线。quickOverride 供未来「强制全量重建」入口使用。
    const quick = quickOverride ?? fullScanCompleted.has(rootId)

    // 新一轮扫描:上一轮的富化终态标记作废(允许本轮 completed 正常移交 enriching)。
    for (const key of enrichmentDone) {
      if (key.startsWith(`${rootId}:`)) enrichmentDone.delete(key)
    }
    for (const key of terminalScanRuns) {
      if (key.startsWith(`${rootId}:`)) terminalScanRuns.delete(key)
    }
    clearEnrichWatchdog(rootId)
    progressMap.value[rootId] = {
      runId,
      scanned: 0,
      total: 0,
      processedBytes: 0,
      totalBytes: null,
      currentDir: '',
      isRunning: true,
      status: 'discovering',
    }

    // 扫描中的画廊刷新由 store 级门限承载(见 refreshScanStats):已入库的行(最新在前)
    // 渐进式显示,而不是等整段快速扫描结束才出图;loadStats() 触动 totalItems → 网格重算。
    const ui = useUiStore()
    const toast = useToastStore()

    const channel = new Channel<ScanChannelPayload>()
    channel.onmessage = (msg) => {
      if (msg.type === 'progress') {
        // 扁平联合后 msg 在此分支已 narrow 到 { type:'progress' } & ScanProgressPayload，
        // 顶层字段直接可读，无需 `as unknown as` 强制 cast（P1-5）。
        if (msg.runId !== runId || !isCurrentScanRunAcceptingEvents(rootId, runId)) return
        progressMap.value[rootId] = {
          runId,
          scanned: msg.scanned,
          total: msg.total,
          processedBytes: msg.processedBytes,
          totalBytes: msg.totalBytes,
          currentDir: msg.currentDir,
          isRunning: true,
          status: msg.status,
        }
        if (msg.status === 'scanning') {
          // 门限是 store 级(跨根全局),多根并发扫描不会各自计时形成刷新洪峰。
          refreshScanStats()
        }
      } else if (msg.type === 'completed') {
        if (msg.runId !== runId || progressMap.value[rootId]?.runId !== runId) return
        const enrichmentAlreadyDone = enrichmentDone.has(`${rootId}:${runId}`)
        // enrichment:completed 可能先于 Channel completed 到达。此时该轮已经有 terminal
        // tombstone，但 Channel completed 仍负责写入 quick 基线和触发 onComplete，不能被
        // 自己的终态账本误吞；stop/error 产生的 terminal 仍通过 accepting 检查拒绝。
        if (!enrichmentAlreadyDone && !isCurrentScanRunAcceptingEvents(rootId, runId)) return
        // 快速入库完成：本轮目录 mtime/media_count 基线已写回,下轮重扫可安全 quick。
        fullScanCompleted.add(rootId)
        // 快速入库完成 — 但扫描并未"结束"：移交到 enriching 阶段（由全局监听驱动）并保持运行中。
        // 幂等护栏:若 enrichment:completed(事件传输)已先到,本条 Channel 消息是晚到的
        // 中间态,绝不能把已清零的运行态复活(见 ensureEnrichmentListeners 顶部时序契约)。
        progressMap.value[rootId] = {
          runId,
          scanned: 0,
          total: 0,
          processedBytes: msg.totalBytes,
          totalBytes: msg.totalBytes,
          currentDir: '',
          isRunning: !enrichmentAlreadyDone,
          status: 'enriching',
        }
        if (!enrichmentAlreadyDone) armEnrichWatchdog(rootId, runId)
        // 缺失检测可观测（Part2 §3.2）：本次差集标记了「缺失」项 → 提示用户（非删除、可自动恢复）。
        if (msg.markedMissing > 0) {
          toast.addToast(
            'info',
            i18n.global.t('statusbar.filesMarkedMissing', { count: msg.markedMissing }),
            5000,
          )
        }
        // 终态强制刷新:本轮最终计数立即反映(override 门限)。
        refreshScanStats(true)
        onComplete?.()
      } else if (msg.type === 'error') {
        if (msg.runId !== runId || !isCurrentScanRunAcceptingEvents(rootId, runId)) return
        markScanRunTerminal(rootId, runId)
        logger.error('Scan error for root', { rootId, error: msg.error })
        // 终态强制刷新:失败前的入库结果同样要反映到统计。
        refreshScanStats(true)
        // 扫描失败对用户可见（S7）：此前仅 console.error，失败被静默吞掉。
        toast.addToast('error', i18n.global.t('statusbar.scanError', { error: msg.error }), 6000)
      }
    }

    try {
      await invokeIpc(IPC.START_SCAN, {
        rootId,
        runId,
        onProgress: channel,
        groupBy: ui.groupBy,
        sortWithinGroup: ui.sortWithinGroup,
        sortOrder: ui.sortOrder,
        quick,
      })
    } catch (e) {
      // 旧 startScan 可能在 await 期间被同根新扫描替换；只允许仍属于本轮的异常
      // 收尾改变运行态，不能把新轮误置为停止或清掉它的 watchdog。
      if (isCurrentScanRunAcceptingEvents(rootId, runId)) markScanRunTerminal(rootId, runId)
      throw e
    }
  }

  async function stopScan(rootId: number) {
    const previous = progressMap.value[rootId]
      ? { ...progressMap.value[rootId] }
      : undefined
    const runId = previous?.runId
    if (!runId || !previous) return
    // 先封口本地事件接收，再等待后端取消；Channel/事件与 STOP IPC 无全局顺序保证。
    markScanRunTerminal(rootId, runId)
    try {
      await invokeIpc(IPC.STOP_SCAN, { rootId, runId })
    } catch (error) {
      restoreScanRunAfterStopFailure(rootId, runId, previous)
      throw error
    }
    // stop 的 IPC 返回可能晚于同根 restart；旧 stop 不得清理新轮状态/看门狗。
    if (progressMap.value[rootId]?.runId === runId) {
      markScanRunTerminal(rootId, runId)
      // 终态强制刷新:停在半程的入库结果也要立刻反映。
      refreshScanStats(true)
    }
  }

  // 根管理命令会在后端先撤销扫描 token；命令失败时由调用方显式收尾本地运行态。
  function markScanStopped(rootId: number, expectedRunId?: string) {
    const runId = progressMap.value[rootId]?.runId
    if (!runId || (expectedRunId !== undefined && runId !== expectedRunId)) return
    markScanRunTerminal(rootId, runId)
  }

  function getProgress(rootId: number): ScanProgress | null {
    return progressMap.value[rootId] ?? null
  }

  async function clearDatabase() {
    if (databaseClearBarrier) await waitForDatabaseClear()

    // 立即建立屏障并使旧根请求失效。clear 成功/失败都不会让已取消的旧扫描重新活跃。
    ++rootsRequestSerial
    isLoadingRoots.value = false
    // 统计取数不受本 store 的清库屏障约束:清库等待期间其它 UI 仍可 loadStats 并读回
    // 旧库快照。故开始处先作废一次,成功返回后再作废一次(见下方),让清库期间发起的
    // 查询也失效。门限一并重置:紧接清库后的首条进度应立刻刷新,而不是被清库前的门限压住。
    lastScanStatsRefreshAt = Number.NEGATIVE_INFINITY
    const media = useMediaStore()
    media.invalidateStats()
    for (const [rootId, progress] of Object.entries(progressMap.value)) {
      markScanRunTerminal(Number(rootId), progress.runId)
    }
    progressMap.value = {}
    enrichmentDone.clear()
    for (const rootId of enrichWatchdogs.keys()) clearEnrichWatchdog(rootId)
    fullScanCompleted.clear()

    const clearRequest = invokeIpc(IPC.CLEAR_DATABASE)
    const barrier = clearRequest
      .then(
        () => {
          ++rootsRequestSerial
          scanRoots.value = []
          // 清库已生效:作废清库窗口内(屏障建立之后)发起的统计取数,否则它们带回的是
          // 清空前的库快照,会在清库返回后回写。失败路径不必:库未变,快照仍然有效。
          media.invalidateStats()
        },
        () => {
          ++rootsRequestSerial
        },
      )
      .then(() => {
        if (databaseClearBarrier === barrier) databaseClearBarrier = null
        ++rootsRequestSerial
      })
    databaseClearBarrier = barrier
    try {
      await clearRequest
    } finally {
      // 等本地清库收尾完成后再释放 barrier，等待中的根请求不会穿过清空边界。
      await barrier
    }
  }

  // ── Full Thumbnail Generation ─────────────────────────────────────────────

  interface ThumbGenProgress {
    generated: number
    total: number
    isRunning: boolean
    status: 'idle' | 'running' | 'completed' | 'cancelled' | 'error'
    currentItem?: string
    phase?: string
  }

  const thumbGenProgress = ref<ThumbGenProgress>({
    generated: 0,
    total: 0,
    isRunning: false,
    status: 'idle',
    currentItem: undefined,
    phase: undefined,
  })

  // State for automatic thumbnail generation (triggered by scrolling)
  const autoThumbQueueSize = ref(0)
  const autoThumbInFlight = ref(0)

  // 进度传输(2026-07-18 刷新丢进度根治):Channel → app 级事件 + 后端快照。Channel 生命周期
  // 绑定发起 invoke 的 webview,刷新页面即永久失联而后台生成照跑;事件对重建后的 webview 照常
  // 广播,快照经 FULL_THUMB_GEN_STATUS 在启动时回填(restoreThumbGenProgress,App.vue 挂载调)。
  interface ThumbGenEventPayload {
    generated: number
    total: number
    status: 'error' | 'completed' | 'running' | 'cancelled' | 'idle'
    currentItem?: string
    phase?: string
  }

  // withSideEffects=false 用于启动回填:completed/cancelled 快照只恢复显示,
  // 不再触发 invalidateLayout(启动时布局本就是新取的)。
  function applyThumbGenPayload(msg: ThumbGenEventPayload, withSideEffects = true) {
    thumbGenProgress.value = {
      generated: msg.generated,
      total: msg.total,
      isRunning: msg.status === 'running',
      status: msg.status,
      currentItem: msg.currentItem,
      phase: msg.phase,
    }
    // 当生成完成（完成/取消）时，使布局失效，
    // 以便网格从数据库重新获取最新的 thumb_status/thumb_path。
    if (withSideEffects && (msg.status === 'completed' || msg.status === 'cancelled')) {
      const media = useMediaStore()
      media.invalidateLayout()
    }
  }

  // 共享注册 Promise(而非布尔标记):并发调用都等同一次注册完成,不存在「标记已置真、
  // 监听尚未挂上」的窗口期漏事件。
  let thumbGenListenerPromise: Promise<unknown> | null = null
  function ensureThumbGenListener(): Promise<unknown> {
    thumbGenListenerPromise ??= listen<ThumbGenEventPayload>(EVENTS.THUMB_GEN_PROGRESS, (e) =>
      applyThumbGenPayload(e.payload),
    )
    return thumbGenListenerPromise
  }

  // 启动/刷新恢复:先订阅事件流,再查后端快照回填——生成仍在跑时进度条立即续上。
  // 快照说 running 但 token 已不在的窗口期由后端归一为 cancelled(full_thumb_gen_status)。
  async function restoreThumbGenProgress() {
    await ensureThumbGenListener()
    const snap = await invokeIpc<ThumbGenEventPayload>(IPC.FULL_THUMB_GEN_STATUS).catch(() => null)
    if (!snap || snap.status === 'idle') return
    applyThumbGenPayload(snap, snap.status === 'running')
  }

  // 全量(重置全表重新生成)与增量(只补 thumb_status=0 缺失项)共用同一进度事件与停止命令,
  // 仅 IPC 命令名不同。
  async function startFullThumbnailGeneration() {
    await runThumbnailGeneration(IPC.START_FULL_THUMBNAIL_GENERATION)
  }
  async function startIncrementalThumbnailGeneration() {
    await runThumbnailGeneration(IPC.START_INCREMENTAL_THUMBNAIL_GENERATION)
  }

  async function runThumbnailGeneration(
    command:
      | typeof IPC.START_FULL_THUMBNAIL_GENERATION
      | typeof IPC.START_INCREMENTAL_THUMBNAIL_GENERATION,
  ) {
    if (!hasScanRoots.value) {
      const toast = useToastStore()
      toast.addToast('warning', i18n.global.t('common.addScanFolderFirst'))
      return
    }

    // 先订阅再启动:total==0 时后端在 invoke 返回前就发 completed,晚订阅会漏终态。
    await ensureThumbGenListener()

    thumbGenProgress.value = {
      generated: 0,
      total: 0,
      isRunning: true,
      status: 'running',
      currentItem: undefined,
      phase: undefined,
    }

    try {
      await invokeIpc(command)
    } catch (e) {
      thumbGenProgress.value.isRunning = false
      thumbGenProgress.value.status = 'error'
      throw e
    }
  }

  async function stopFullThumbnailGeneration() {
    await invokeIpc(IPC.STOP_FULL_THUMBNAIL_GENERATION)
    thumbGenProgress.value.isRunning = false
    if (thumbGenProgress.value.status === 'running') {
      thumbGenProgress.value.status = 'cancelled'
    }
  }

  return {
    scanRoots,
    visibleScanRoots,
    progressMap,
    isLoadingRoots,
    hasScanRoots,
    isAnyScanRunning,
    aggregateProgress,
    loadScanRoots,
    addScanRoot,
    removeScanRoot,
    setScanRootHidden,
    startScan,
    stopScan,
    markScanStopped,
    getProgress,
    clearDatabase,
    thumbGenProgress,
    restoreThumbGenProgress,
    startFullThumbnailGeneration,
    startIncrementalThumbnailGeneration,
    stopFullThumbnailGeneration,
    autoThumbQueueSize,
    autoThumbInFlight,
  }
})
