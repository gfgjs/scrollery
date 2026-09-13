// src/composables/useAnalysisController.ts
// 分析管理控制器（S6/T19 去重）。aiStore 与 faceStore 的"分析管理半部"此前是显式镜像复制
// （faceStore 注释「Mirrors aiStore's analysis-management half」）：2s 轮询 + start/pause/
// restart/stop + 自动续传 + providerLabel + 进度，逻辑雷同，仅端点 / 状态字段 / 错误处理不同。
//
// 本控制器把这条共享控制流参数化:差异收敛为「静态配置（5 个 IPC 命令 + logTag）+ 进度字段
// getter + 2 个回调（onError / onStarted）」,无运行时耦合、无 state 外漏——故是真 DRY,而非
// 把耦合搬成长参数列表。各 store 只持自己的 status ref 与专属逻辑（搜索 / 模型库等），分析半部
// 委托本控制器。
//
// P1-1（共享会话竞争 / 自动续跑）:AI 与人脸共用唯一 GPU 分析会话槽,启动批并行自动续跑时必有一
// 方被拒。被拒方不再当错误上报,而是进「资源等待」——按稳定 code(AnalysisBusy)分流、有界重试、
// 对端释放即续跑;用户暂停/停止则撤回等待意图,并等本控制器在途起步命令收尾后才发 cancel
// (后端 start 在同步段之后才 launch,cancel 抢跑会被旧起步反超)。等待态与「运行中让步等待」
// (isWaitingBlocked)是两回事,与「用户手动暂停」也严格分开,见各自注释。

import { computed, onScopeDispose, ref, watch, type Ref } from 'vue'
import { invokeIpc, type IpcCommand } from '../utils/ipc'
import { logger } from '../utils/logger'
import i18n from '../i18n'
import { useToastStore } from '../stores/toastStore'

/** 分析状态的公共子集——控制器仅依赖这些字段，各 store 的完整 status 类型须含之。 */
export interface BaseAnalysisStatus {
  provider: string
  isAnalyzing: boolean
  analysisActive: boolean
  totalItems: number
  pendingItems: number
  /** 本次轮询时的让步阻塞源快照，仅 isAnalyzing 时非空（可观测性三修 #2）。 */
  waitingOn: string[]
}

/** 该分析种类的 5 个后端命令（AI / face 各一套）。 */
export interface AnalysisCommands {
  getStatus: IpcCommand
  start: IpcCommand
  pause: IpcCommand
  restart: IpcCommand
  stop: IpcCommand
}

export type AnalysisAction = 'start' | 'pause' | 'restart' | 'stop' | 'autoResume'

export interface AnalysisControllerOptions<S extends BaseAnalysisStatus> {
  /** 本 store 持有的分析状态 ref（控制器就地读写其公共字段）。 */
  status: Ref<S>
  commands: AnalysisCommands
  /** 已分析计数 getter——AI 取 analyzedItems、face 取 processedItems（字段名不同，故传 getter）。 */
  analyzedCount: () => number
  /** 日志前缀，如 '[AI]' / '[Face]'。 */
  logTag: string
  /** 动作出错回调——AI 走 logger.error；face 的 start/restart 走 toast、pause/stop 走 logger。 */
  onError: (action: AnalysisAction, e: unknown) => void
  /** 成功启动 / 重启后的钩子——AI 用它清 searchError；face 无（不传）。 */
  onStarted?: () => void
}

/**
 * get_status 的「资源等待」原因键：本端未运行、仍有剩余、而共享 GPU 分析会话被**对端**持有
 * （P1-1）。与后端 `ipc::ai_commands::ANALYSIS_BUSY_WAITING_KEY` 同值；UI 文案映射见
 * ToolsSection.vue 的 `sidebar.waitingOnAnalysisBusy`。用户手动暂停不输出该键（暂停会释放会话）。
 */
export const ANALYSIS_BUSY_WAITING_KEY = 'analysisBusy'

/** 资源等待的有界重试间隔（ms）：与运行中状态轮询同频。每轮只读一份快照，对端释放即续跑。 */
const WAITING_RETRY_INTERVAL_MS = 2000

/** 后端「共享 GPU 分析会话被对端持有」的稳定 code（error.rs `AppError::AnalysisBusy`）。 */
const ANALYSIS_BUSY_CODE = 'AnalysisBusy'

/** 按稳定 code 识别资源竞争；**不**匹配 message（文案随语言与后端措辞变化，不承担分流职责）。 */
function isAnalysisBusy(e: unknown): boolean {
  return (e as { code?: unknown } | null | undefined)?.code === ANALYSIS_BUSY_CODE
}

export function useAnalysisController<S extends BaseAnalysisStatus>(
  opts: AnalysisControllerOptions<S>,
) {
  const { status, commands } = opts

  /** 在途状态请求(2026-07-11 加固批 B-1:in-flight 合并)。 */
  let inFlight: Promise<void> | null = null

  /** 在途起步命令(start / restart，P1-1)。两件事靠它：
   *  ① 合并重复发起——maybeAutoResume 与用户「继续」并发不双启动；
   *  ② 次序门——暂停/停止/重启须先等它收尾：后端 start 在 spawn_blocking 同步段之后才
   *  launch，若 cancel 抢在它前面，旧 start 会稍后把会话真正跑起来（UI 已显示已停）。 */
  let inFlightStart: Promise<void> | null = null

  /**
   * 资源等待：在途起步被「共享分析会话被对端持有」拒绝 → 排队等对端释放后自动续跑。
   * 与「用户手动暂停」严格区分——后者是用户意图（后端 analysis_active 保留，跨重启仍会续传），
   * 本标志只表示本次会话里已排队的续跑，由 pause/stop/restart/成功/真实错误/作用域销毁撤回。
   */
  const waitingForSession = ref(false)
  let waitingTimer: ReturnType<typeof setInterval> | undefined

  /** 操作代次(P1-1)：每次用户控制动作自增；异步续段落地前比对，旧代次的应答整体丢弃——
   *  用户暂停/停止后迟到的 start 成功不得把运行态翻回去，也不得重启等待循环。 */
  let opGeneration = 0

  // 进度停滞追踪（可观测性三修 #3）：与上次拉取相比已分析数是否未变化——用于驱动
  // 「等待 xxx 完成」等待原因文案，只在真正卡住（如让步阻塞源常驻）时出现，
  // 不在正常吞吐间隙误报。首次拉取（lastAnalyzedCount 尚为 null）不算停滞。
  const lastAnalyzedCount = ref<number | null>(null)
  const progressStalled = ref(false)

  /** 从后端拉最新状态（失败静默——状态栏保留最后已知值）。
   *  in-flight 合并:后端读池紧张时单次轮询可拖过 2s,setInterval 不等前次完成会
   *  叠加并发请求,反过来加剧读池竞争(与后端 Pool(Error) 饿死链互为放大器)——
   *  同一时刻至多一个在途请求,后到者共乘其结果。 */
  function fetchStatus(): Promise<void> {
    if (inFlight) return inFlight
    inFlight = (async () => {
      try {
        status.value = await invokeIpc<S>(commands.getStatus)
        const count = opts.analyzedCount()
        progressStalled.value = lastAnalyzedCount.value === count
        lastAnalyzedCount.value = count
      } catch {
        // Silently ignore — keep last known state | 静默忽略 — 保留最后已知状态
      } finally {
        inFlight = null
      }
    })()
    return inFlight
  }

  // 运行中 && 有具体阻塞源 && 进度确实停滞 —— 驱动前端「等待 xxx 完成」提示。
  // 消费点：进度渲染组件（sidebar ToolsSection.vue）负责阻塞源键→中文文案映射与展示样式。
  // 注意与 waitingForSession 区分:本判据要求本端确在运行(让步阻塞),后者是本端未运行、在等
  // 对端释放共享会话。
  const isWaitingBlocked = computed(
    () => status.value.isAnalyzing && status.value.waitingOn.length > 0 && progressStalled.value,
  )

  /** 动作(暂停/停止)后的强制刷新:先吸收在途轮询再发新请求——直接合并可能拿到
   *  动作**之前**发出的陈旧快照,把刚写下的本地状态又翻回去。 */
  async function refreshStatusAfterAction(): Promise<void> {
    if (inFlight) await inFlight
    return fetchStatus()
  }

  // 运行中每 2s 轮询状态（isAnalyzing 翻 true 时起、翻 false 时由 onCleanup 清定时器）。
  watch(
    () => status.value.isAnalyzing,
    (isAnalyzing, _, onCleanup) => {
      if (isAnalyzing) {
        const interval = setInterval(() => {
          void fetchStatus()
        }, 2000)
        onCleanup(() => clearInterval(interval))
      }
    },
  )

  // 进度用 analyzedCount / total（face 的 processedItems 含失败项，故失败时进度条仍到 100%）。
  const analyzeProgress = computed(() => {
    if (status.value.totalItems === 0) return 0
    return Math.round((opts.analyzedCount() / status.value.totalItems) * 100)
  })

  const providerLabel = computed(() => {
    const p = status.value.provider
    if (p === 'directml') return 'DirectML'
    if (p === 'cuda') return 'CUDA'
    if (p === 'coreml') return 'CoreML'
    if (p === 'openvino') return 'OpenVINO'
    if (p === 'cpu') return 'CPU'
    // computed 内取 t：Composer 的 locale 是 ref，t() 读它即被追踪 → 切语言时自动重算。
    return i18n.global.t('settings.aiProviderNotInitialized')
  })

  /** 启动前确认有扫描根，否则提示并拦下（AI / face 共用的前置守门）。 */
  async function ensureScanRoots(): Promise<boolean> {
    const { useScanStore } = await import('../stores/scanStore')
    const scan = useScanStore()
    if (!scan.hasScanRoots) {
      useToastStore().addToast('warning', i18n.global.t('common.addScanFolderFirst'))
      return false
    }
    return true
  }

  /** 进入资源等待:标记「期望运行但共享会话被对端占用」并起有界重试。用户手动暂停不走这里。 */
  function enterWaiting() {
    if (waitingForSession.value) return
    waitingForSession.value = true
    logger.info(
      `${opts.logTag} queued for the shared GPU analysis session | 已排队等待共享 GPU 分析会话`,
    )
    waitingTimer = setInterval(() => {
      void waitingTick()
    }, WAITING_RETRY_INTERVAL_MS)
  }

  /** 退出资源等待:清意图与定时器(成功续跑 / 用户暂停停止 / 重启 / 真实错误 / 作用域销毁)。 */
  function exitWaiting() {
    if (waitingTimer !== undefined) {
      clearInterval(waitingTimer)
      waitingTimer = undefined
    }
    waitingForSession.value = false
  }

  /** 收尾在途起步:暂停/停止/重启前必须先等它收尾。循环防御等待期 tick 在相邻微任务里新起的
   *  一条(等待意图已清,故最多多收一轮)。 */
  async function drainInFlightStart(): Promise<void> {
    while (inFlightStart) await inFlightStart
  }

  /** 等待期单次重试:先读快照,仅在「对端已不持有共享会话」时发起一次 start——对端仍在跑时
   *  只读快照,不重复往返 start(故不会叠出第二个 owner)。 */
  async function waitingTick(): Promise<void> {
    if (!waitingForSession.value || inFlightStart) return
    await fetchStatus()
    if (!waitingForSession.value) return // 快照在途期间用户已暂停/停止
    if (status.value.isAnalyzing) {
      exitWaiting() // 本端已被别处拉起(如另一窗口),不必再排队
      return
    }
    if (status.value.waitingOn.includes(ANALYSIS_BUSY_WAITING_KEY)) return // 对端仍持有
    await requestSessionStart('autoResume')
  }

  /**
   * 发起一次「起步」命令(start / restart)并分流结果、合并重复发起(P1-1):
   *  · 唯一在途:同一时刻至多一条起步命令,重复调用共乘其结果;
   *  · AnalysisBusy(共享分析会话被对端持有)= 可等待状态 → 进资源等待,由 waitingTick 有界
   *    重试,对端释放即自动续跑;**不是**错误,不交 onError;
   *  · restart 例外:破坏性 reset 不排队自动重做,被拒即交 onError,由用户决定何时再来;
   *  · 其余错误(模型未就绪 / 路径不可用等)= 终止重试,交 onError;
   *  · 起步预检(如扫描根守卫)与命令同在一次在途操作内,预检期间用户暂停/停止可撤回它。
   */
  function requestSessionStart(
    action: 'start' | 'autoResume' | 'restart',
    beforeStart?: () => Promise<boolean>,
  ): Promise<void> {
    if (inFlightStart) return inFlightStart
    const generation = opGeneration
    const command = action === 'restart' ? commands.restart : commands.start
    const op = (async () => {
      try {
        if (beforeStart) {
          if (!(await beforeStart())) return
          // 预检期间用户已暂停/停止:不发起这条起步(其意图已被新动作取代)。
          if (generation !== opGeneration) return
        }
        await invokeIpc(command)
        // 期间用户已暂停/停止:后端马上会被 cancel,本地不置运行态(旧应答不得复活)。
        if (generation !== opGeneration) return
        exitWaiting()
        status.value.isAnalyzing = true
        status.value.analysisActive = true
        opts.onStarted?.()
      } catch (e) {
        if (generation !== opGeneration) return
        if (action !== 'restart' && isAnalysisBusy(e)) {
          enterWaiting()
          return
        }
        exitWaiting()
        opts.onError(action, e)
      } finally {
        inFlightStart = null
      }
    })()
    inFlightStart = op
    return op
  }

  /** 启动 / 续传分析（不重置——跳过已处理项）。 */
  function startAnalysis(): Promise<void> {
    // 扫描根守卫作为起步预检放进同一次在途操作内:否则「预检 → 发 start」之间用户按下暂停,
    // 旧 start 仍会落地把运行态翻回来。
    return requestSessionStart('start', ensureScanRoots)
  }

  /** 暂停——保留进度与续传标志（后端 analysis_active=1：跨重启仍会自动续传），释放共享 GPU 槽。
   *  次序:先撤回在途起步的本地落地权并让它在后端收尾,再发 cancel——后端 start 在同步段之后才
   *  launch,若 cancel 抢在它前面,旧 start 会稍后把会话真正跑起来(UI 却已显示已停)。 */
  async function pauseAnalysis() {
    const generation = ++opGeneration
    exitWaiting() // 手动暂停 ≠ 排队等会话:撤回资源等待意图
    try {
      await drainInFlightStart()
      // 收尾期间用户可能已改按停止/重启(新意图取代本动作):不再发这条命令。
      if (generation !== opGeneration) return
      await invokeIpc(commands.pause)
      status.value.isAnalyzing = false
      await refreshStatusAfterAction()
    } catch (e) {
      opts.onError('pause', e)
    }
  }

  /** 从零重新开始——清空后全量重跑。被「对端持有会话」拒绝时**不**排队重做（见
   *  requestSessionStart）：破坏性 reset 不得稍后偷偷执行。次序同 pause，先收尾在途起步。 */
  async function restartAnalysis() {
    const generation = ++opGeneration
    exitWaiting()
    await drainInFlightStart()
    // 收尾期间被停止/后来的重启取代:不得再发这条破坏性 reset。
    if (generation !== opGeneration) return
    await requestSessionStart('restart')
  }

  /** 停止并清除续传标志（不再自动续传）。进度 / 数据保留。次序同 pause。 */
  async function stopAnalysis() {
    const generation = ++opGeneration
    exitWaiting()
    try {
      await drainInFlightStart()
      // 收尾期间被后来的控制动作取代:不再发这条命令(接手方自会落地)。
      if (generation !== opGeneration) return
      await invokeIpc(commands.stop)
      status.value.isAnalyzing = false
      status.value.analysisActive = false
      await refreshStatusAfterAction()
    } catch (e) {
      opts.onError('stop', e)
    }
  }

  /** 启动时续传被中断（崩溃/强退/暂停）且仍有剩余的分析——断点续传入口。后端 analysis_active
   *  是用户「期望运行」意图的持久化,故暂停过的分析跨重启仍会自动续跑;重复调用(启动批 /
   *  取消隐藏补跑)合并在途起步,不双启动。 */
  async function maybeAutoResume(): Promise<void> {
    if (inFlightStart) return inFlightStart
    if (waitingForSession.value) return // 已在资源等待队列里,等唤醒即可
    // 代次在**状态查询之前**记下:暂停/停止若在该查询在途期间落地,迟到的快照即使仍「看起来
    // 可续跑」(active=1 且有剩余)也不得据此起步——那会把用户刚停下的分析又拉起来。
    const generation = opGeneration
    await fetchStatus()
    if (generation !== opGeneration) return
    if (waitingForSession.value) return // 查询在途期间已入等待队列,不重复起步
    if (status.value.isAnalyzing) return
    if (!status.value.analysisActive || status.value.pendingItems <= 0) return
    logger.info(`${opts.logTag} auto-resuming interrupted analysis | 自动续传被中断的分析`)
    await requestSessionStart('autoResume')
  }

  // 控制器随宿主作用域销毁(Pinia store 释放)时清定时器与等待意图;failSilently=true 使无作用域
  // 的纯函数式调用(单测)不告警。
  onScopeDispose(() => {
    ++opGeneration
    exitWaiting()
  }, true)

  return {
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
  }
}
