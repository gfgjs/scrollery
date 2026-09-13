// src/stores/aiStore.ts
// AI store — 管理引擎状态、语义搜索状态和分析进度。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { invokeIpc, ipcErrorMessage } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'
import { useMediaStore } from './mediaStore'
import { useUiStore } from './uiStore'
import { useToastStore } from './toastStore'
import { useAnalysisController } from '../composables/useAnalysisController'
import type { AiStatusSummary, SearchMode, ModelRegistry, ModelDownloadProgress } from '../types/ai'

export const useAiStore = defineStore('ai', () => {
  // ── State ─────────────────────────────────────────────────────────────────
  const status = ref<AiStatusSummary>({
    provider: '',
    gpuName: '',
    vramGb: 0,
    batchSize: 0,
    activeFixedBatch: null,
    clipLoaded: false,
    totalItems: 0,
    analyzedItems: 0,
    pendingItems: 0,
    errorItems: 0,
    isAnalyzing: false,
    analysisActive: false,
    waitingOn: [],
  })

  const searchMode = ref<SearchMode>('mixed')
  const activeMixedQueryType = ref<'semantic' | 'normal' | 'none'>('none')
  const semanticQuery = ref('')
  const matchCount = ref(0)
  const similarityThreshold = ref(0.2)
  const isSearching = ref(false)
  const searchError = ref<string | null>(null)
  const previousGroupBy = ref<'date' | 'folder' | 'none'>('date')

  // 分析管理半部（轮询 / start·pause·restart·stop / 自动续传 / 进度 / providerLabel）委托共享
  // 控制器（S6 去重，与 faceStore 共用 useAnalysisController）。AI 专属:onStarted 清 searchError；
  // analyzedCount 取 analyzedItems。
  const analysis = useAnalysisController<AiStatusSummary>({
    status,
    commands: {
      getStatus: IPC.GET_AI_STATUS,
      start: IPC.START_AI_ANALYSIS,
      pause: IPC.PAUSE_AI_ANALYSIS,
      restart: IPC.RESTART_AI_ANALYSIS,
      stop: IPC.STOP_AI_ANALYSIS,
    },
    analyzedCount: () => status.value.analyzedItems,
    logTag: '[AI]',
    onError: (action, e) => {
      // start/restart 的后端拒绝（GPU 槽被人脸占用 / 模型未装）须用户可见 → toast
      // （2026-07-10 审查 U2,对齐 faceStore 同场景）；其余记 logger。
      if (action === 'start' || action === 'restart') {
        useToastStore().addToast('error', ipcErrorMessage(e))
      } else {
        logger.error(`[AI] ${action} 分析出错 | analysis error`, { error: e })
      }
    },
    onStarted: () => {
      searchError.value = null
    },
  })
  const {
    fetchStatus,
    analyzeProgress,
    providerLabel,
    isWaitingBlocked,
    waitingForSession,
    startAnalysis,
    pauseAnalysis,
    restartAnalysis,
    stopAnalysis,
    maybeAutoResume,
  } = analysis

  // ── Computed ──────────────────────────────────────────────────────────────
  const isSemanticMode = computed(
    () =>
      searchMode.value === 'semantic' ||
      (searchMode.value === 'mixed' && activeMixedQueryType.value === 'semantic'),
  )

  // ── Actions ───────────────────────────────────────────────────────────────

  /** 按需初始化 AI 引擎（懒加载） */
  async function initEngine() {
    try {
      await invokeIpc(IPC.DETECT_AI_PROVIDER)
      await fetchStatus()
    } catch (e) {
      logger.error('[AI] Init engine error | 初始化引擎错误', { error: e })
    }
  }

  /** 非破坏重试失败项（审查 F9）：Error→Pending 复位后走既有 start 复跑，不触碰已完成向量 */
  async function retryFailedItems() {
    try {
      const n = await invokeIpc<number>(IPC.RETRY_FAILED_AI_ITEMS)
      if (n > 0) await startAnalysis()
      await fetchStatus()
    } catch (e) {
      useToastStore().addToast('error', ipcErrorMessage(e))
    }
  }

  /** 重置所有嵌入向量并重新分析 */
  async function rebuildEmbeddings() {
    try {
      await invokeIpc(IPC.REBUILD_EMBEDDINGS)
      // 后端已清空嵌入并吊销在途搜索(P1-3):前端同步清展示态,别让画廊继续按旧向量排序。
      clearSemanticSearch()
      await fetchStatus()
    } catch (e) {
      logger.error('[AI] Rebuild embeddings error | 重建嵌入向量错误', { error: e })
    }
  }

  // 搜索代次令牌（2026-07-10 审查 U4）：两次查询并发在途时，后完成的**旧**应答会覆盖
  // matchCount/加载态并触发 relayout（画廊显示 A 结果而 semanticQuery 是 B）。与
  // ContentViewer 人脸加载的 faceToken 同一模式：应答落地前比对代次，旧应答整体丢弃
  //（连 invalidateLayout 一起）。
  //
  // P1-3 补齐两侧：①后端现在按请求身份线性化提交，被取代的查询返回 null（不再落库），
  // 前端收到 null 同样不动计数/布局；②清空、切模型、重建嵌入都会递增本令牌，让在途应答
  // 整体作废——本地意图与后端吊销同步生效，不靠应答到达的先后。
  let searchToken = 0

  /**
   * 清空语义搜索（P1-3）。本地先收尾、再发后端清空：
   * - 本地令牌换代 + 清 loading/matchCount/错误 ⇒ 在途应答（含旧的 null）整体落不到新状态上，
   *   spinner 立刻停，不会因为后端工作段晚跑而永远挂着；
   * - 后端 clear_semantic_search 入口当场吊销在途请求、工作段按代次守卫擦库；
   * - 后端擦除失败不回滚本地清空意图（用户已按下清空），只记日志。
   *
   * `skipWhenIdle` 给 mixed 模式的普通文字提交用:那条路径每次防抖提交都会走到,若本地没有
   * 语义在场(无在途请求、无已展示结果),就没有需要吊销或擦除的东西,不必每键打一次 IPC;
   * 一旦语义在场,该吊销的照样吊销。显式清空/切模式/切模型/重建嵌入一律不跳过。
   */
  function clearSemanticSearch({ skipWhenIdle = false } = {}) {
    const semanticInPlay =
      isSearching.value || semanticQuery.value !== '' || matchCount.value > 0
    searchToken++
    semanticQuery.value = ''
    matchCount.value = 0
    isSearching.value = false
    searchError.value = null
    if (skipWhenIdle && !semanticInPlay) return
    void invokeIpc(IPC.CLEAR_SEMANTIC_SEARCH).catch((e) => {
      logger.error('[AI] Clear semantic search error | 清空语义搜索错误', { error: e })
    })
    useMediaStore().invalidateLayout()
  }

  /** 运行语义搜索查询 */
  async function runSemanticSearch(query: string, limit = 1000) {
    const token = ++searchToken
    if (!query.trim()) {
      if (searchMode.value === 'mixed') {
        activeMixedQueryType.value = 'none'
        const ui = useUiStore()
        if (ui.sortWithinGroup === 'similarity') ui.setSortWithinGroup('datetime', false)
        if (ui.groupBy === 'none') ui.setGroupBy(previousGroupBy.value, false)
      }
      clearSemanticSearch()
      return
    }

    isSearching.value = true
    searchError.value = null
    semanticQuery.value = query

    if (searchMode.value === 'mixed' && activeMixedQueryType.value !== 'semantic') {
      activeMixedQueryType.value = 'semantic'
      const ui = useUiStore()
      ui.searchQuery = ''
      if (ui.sortWithinGroup !== 'similarity') ui.setSortWithinGroup('similarity', false)
      if (ui.groupBy !== 'none') {
        previousGroupBy.value = ui.groupBy
        ui.setGroupBy('none', false)
      }
    }

    try {
      const count = await invokeIpc<number | null>(IPC.SEMANTIC_SEARCH_CMD, {
        query,
        limit,
      })
      if (token !== searchToken) return // 旧应答:丢弃(U4)
      // null = 本次请求已被更新的查询/清空/切模型取代(后端未写结果集):保持当前视图,
      // 不刷新布局——旧查询的排名不该盖到新视图上(P1-3)。
      if (count === null) return
      matchCount.value = count
      // 结果已存于 DB 的 ai_search_results 表，这里只需 invalidate layout 让 MediaGrid 重载。
      useMediaStore().invalidateLayout()
    } catch (e) {
      // 展示文案统一走 ipcErrorMessage（2026-07-10 审查 U7）:String(e) 会带 "IpcError: " 前缀。
      if (token === searchToken) searchError.value = ipcErrorMessage(e)
    } finally {
      // 旧请求的 finally 不得提前灭新请求的 spinner(U4)。归属仍只看本地令牌:后端侧吊销
      // (重启分析/重建嵌入/切模型)不会给前端发新令牌,不在此收尾 spinner 会永远转(P1-3)。
      if (token === searchToken) isSearching.value = false
    }
  }

  function setNormalSearchQueryInMixedMode(query: string) {
    const ui = useUiStore()
    ui.searchQuery = query
    // 退出语义查询(mixed 下切普通文字,或清空文字)一律走清空意图:递增本地令牌 + 后端入口
    // 吊销 + 守卫擦库。此前这里只清 `semanticQuery`,在途语义应答仍能写回 matchCount 并刷新
    // 布局——画廊会跳成语义排名,而用户已经在看普通搜索结果(P1-3)。
    clearSemanticSearch({ skipWhenIdle: true })
    if (!query.trim()) {
      activeMixedQueryType.value = 'none'
    } else {
      activeMixedQueryType.value = 'normal'
      if (ui.sortWithinGroup === 'similarity') ui.setSortWithinGroup('datetime', false)
      if (ui.groupBy === 'none') ui.setGroupBy(previousGroupBy.value, false)
    }
  }

  function toggleSearchMode() {
    if (searchMode.value === 'mixed') {
      setSearchMode('semantic')
    } else if (searchMode.value === 'semantic') {
      setSearchMode('normal')
    } else {
      setSearchMode('mixed')
    }
  }

  function setSearchMode(mode: SearchMode) {
    searchMode.value = mode
    const ui = useUiStore()

    // 切换模式时重置查询和类型
    ui.searchQuery = ''
    activeMixedQueryType.value = 'none'
    // P1-3:语义结果集随模式切换一起作废(本地令牌换代 + 后端入口吊销 + 守卫擦库),
    // 否则退到常规搜索后仍挂着上一次语义排名的结果行。
    clearSemanticSearch()

    if (mode === 'semantic') {
      if (ui.sortWithinGroup !== 'similarity') {
        ui.setSortWithinGroup('similarity', false)
      }
      if (ui.groupBy !== 'none') {
        previousGroupBy.value = ui.groupBy
        ui.setGroupBy('none', false)
      }
    } else {
      // normal 与 mixed（初始态）都恢复常规排序/分组
      if (ui.sortWithinGroup === 'similarity') {
        ui.setSortWithinGroup('datetime', false)
      }
      if (ui.groupBy === 'none') {
        ui.setGroupBy(previousGroupBy.value, false)
      }
    }

    useMediaStore().invalidateLayout()
  }

  /** 重新加载 AI 引擎 */
  async function reloadAiEngine(): Promise<void> {
    try {
      await invokeIpc(IPC.RELOAD_AI_ENGINE)
      await fetchStatus()
    } catch (e) {
      logger.error('[AI] Reload engine error | 重载引擎错误', { error: e })
      throw e
    }
  }

  // ── 模型注册表 / 模型库（Layer B）──────────────────

  /** 列出内置模型注册表（含安装/激活状态） */
  async function listModelRegistry(): Promise<ModelRegistry> {
    return await invokeIpc<ModelRegistry>(IPC.LIST_MODEL_REGISTRY)
  }

  /** 切换激活模型到某 batch 变体（校验已安装；重同步状态；重载引擎）。`imageFile` = 该变体图像 onnx 文件名。 */
  async function setActiveModel(imageFile: string): Promise<void> {
    await invokeIpc(IPC.SET_ACTIVE_MODEL, { imageFile })
    // P1-3:切模型 = 换向量空间。后端已吊销在途搜索并擦除旧结果集,前端同步清展示态,
    // 不让画廊继续按旧模型的排名与计数显示。
    clearSemanticSearch()
    await fetchStatus()
  }

  /** 下载某变体的资产（图像+extra+共享文本塔+vocab），经 Channel 流式回传进度。 */
  function downloadModel(
    imageFile: string,
    onProgress: (p: ModelDownloadProgress) => void,
  ): Promise<void> {
    const ch = new Channel<ModelDownloadProgress>()
    ch.onmessage = onProgress
    return invokeIpc(IPC.DOWNLOAD_MODEL, { imageFile, onProgress: ch })
  }

  return {
    // state
    status,
    searchMode,
    activeMixedQueryType,
    semanticQuery,
    similarityThreshold,
    isSearching,
    searchError,
    matchCount,
    // computed
    analyzeProgress,
    providerLabel,
    isWaitingBlocked,
    waitingForSession,
    isSemanticMode,
    // actions
    fetchStatus,
    initEngine,
    startAnalysis,
    pauseAnalysis,
    restartAnalysis,
    stopAnalysis,
    maybeAutoResume,
    retryFailedItems,
    rebuildEmbeddings,
    runSemanticSearch,
    clearSemanticSearch,
    setNormalSearchQueryInMixedMode,
    toggleSearchMode,
    setSearchMode,
    reloadAiEngine,
    listModelRegistry,
    setActiveModel,
    downloadModel,
  }
})
