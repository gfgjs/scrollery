// src/stores/settingsPersistence.ts
// 中央设置读写服务(设置集中保存,2026-09-16)。
//
// 全局用户偏好的唯一前端入口:所有偏好模块经此处读取权威快照、提交改动;本模块负责批量合并、
// 防抖、代次守卫、失败可见与跨窗口事件同步,消费方不再各自落盘。
//
// 关键约束(对应设计 §5/§7):
// - 本地即时预览:采集时立即并入本地值表,拖动音量/位移等连续操作不因等后端回执而迟滞。
// - 权威值不吞预览:任何回执/事件应用权威快照后,仍叠加「同代次的未确认修改」,拖拽不被打断。
// - 防抖:空闲 300ms 提交,连续操作最长 1s 提交一次;同一批只发一次 IPC、只写一次文件。
// - 失败恢复:写盘失败回退到**最后确认快照**,再叠加仍然有效的较新预览,并给出可见提示。
// - 代次守卫:patch 在**采集时**记录 generation,发送前复核;重置或跨窗口重置事件会丢弃
//   旧代次的待提交与排队批次,避免用新代次写回旧值。
// - 过期响应丢弃:只有不低于当前 revision 的回执与事件才被采纳。
// - 缓存仅 themeSnapshot:除首帧主题快照外不写任何 localStorage 偏好。
import { readonly, ref, shallowRef } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { listenAppEvent } from '../utils/appEvents'
import { IPC, EVENTS } from '../constants/ipc'
import { logger } from '../utils/logger'
import type { SettingsChange, SettingsSnapshot, StartupPayload } from '../types/config'

/** 连续操作的空闲提交间隔(ms)。 */
const DEBOUNCE_IDLE_MS = 300
/** 连续操作的最长等待:持续拖动也会每 1s 落盘一次(ms)。 */
const DEBOUNCE_MAX_MS = 1000

/** 一次等待回执的消费者(一个批次可能被多次写入共享,故是一组)。 */
interface Deferred {
  resolve: () => void
  reject: (error: unknown) => void
}

/** 已进入发送队列、等待串行发送的一批修改。`generation` 在采集时记录,发送前复核。 */
interface QueuedBatch {
  patch: Record<string, string>
  generation: number
  deferreds: Deferred[]
}

/** 是否已拿到权威快照。未就绪时读取落 fallback、写入被拒(占位值不得被当成用户修改写回)。 */
const settingsReady = ref(false)

/**
 * 当前应显示的规范值表 = 最后确认快照 + 同代次未确认修改(排队批次、待提交集合)。
 * 整体替换(shallowRef + 整份 replace),不逐键改写——消费方的 computed 依赖的是「换了一份」。
 * 对外经只读视图导出:权威值只能由本模块写入,消费方不得直接改它。
 */
const valuesRef = shallowRef<Record<string, string>>({})

/** 只读的当前值表(整体替换语义;消费方经 readSetting 取单键,或 watch 整份变化)。 */
export const settingsValues = readonly(valuesRef)

/** 应用失败(文件已保存、运行期未能生效)的键,供设置页如实展示。 */
const settingsApplyFailed = shallowRef<string[]>([])

/** 已保存但需重启应用才生效的键(外部编辑热应用经事件带来)。 */
const settingsRestartRequired = shallowRef<string[]>([])

/**
 * **已确认**的权威值(不含任何未落盘的本地预览)。
 *
 * 给少数「显示/渲染不能先于提交切换」的消费点用:例如查看器渲染色域——后端按
 * ConfigManager 的单源值解析色域 URL,前端若在提交前就切显示态,渲染请求会与写盘竞速,
 * 偶发按旧色域渲染。其余普通 UI 一律用 settingsValues(即时预览,拖拽跟手)。
 */
const settingsConfirmedValues = shallowRef<Record<string, string>>({})

/** 最后确认的权威值(失败回滚的基线;与展示值分开,展示值上还叠着未确认预览)。 */
let confirmedValues: Record<string, string> = {}
/** 当前代次:采集时记入批次,发送前复核;仅在重置时由后端递增。 */
let currentGeneration = 0

/**
 * 当前代次的只读视图。**重置会推进它**:消费方据此丢弃会话内的临时覆盖(如 URL 传入的视图
 * 偏好、拖拽中的本地预览)——重置后即使某个键的权威值恰好与重置前相同(值没变、会话覆盖却
 * 该失效),也必须以权威值为准,不能让临时覆盖继续留在界面上。
 */
const settingsGeneration = ref(0)
/** 当前 revision:只接受不低于它的回执与事件。 */
let currentRevision = 0

/** 待提交集合(防抖合并的前沿)与其代次、等待者。 */
let pendingPatch: Record<string, string> = {}
let pendingGeneration = 0
let pendingDeferreds: Deferred[] = []
let pendingTimer: ReturnType<typeof setTimeout> | null = null
/** 本轮连续操作的起始时刻,用于最长等待判定。 */
let pendingSince = 0

/** 发送队列与串行 drain。 */
const sendQueue: QueuedBatch[] = []
let drainPromise: Promise<void> | null = null

/**
 * 写盘失败计数与最近一次失败值。flush 在等待窗口的起止各取一次计数:
 * 只要窗口内发生过失败(即使后续批次成功),flush 就必须 reject——后续成功不能抹掉前面的失败,
 * 否则退出流程会把「有东西没保存」误判成已保存。
 */
let failureCount = 0
let lastFailure: unknown = null

/** 重置在途(去重共享同一 Promise);期间暂停采集,避免把旧代次的预览写进新代次。 */
let resetInProgress: Promise<SettingsChange> | null = null

/** 启动批只发一次,多处调用共享同一 Promise。 */
let startupPromise: Promise<StartupPayload> | null = null
/** 事件监听注册 Promise(幂等,镜像 useConfigFile 的共享注册写法)。 */
let listenersPromise: Promise<void> | null = null

/** 「权威快照已应用」订阅者(非组件场景用;组件内优先 watch(settingsValues))。 */
const appliedCallbacks = new Set<(snapshot: SettingsSnapshot) => void>()

/** 统一错误提示出口:由 App 层注册,避免本模块直接依赖 toast store 造成循环 import。 */
let errorReporter: ((message: string) => void) | null = null
/** 文案构造(App 层注入 i18n;缺省用中文兜底,便于模块独立测试)。 */
let writeFailedFormatter: (() => string) | null = null
let applyFailedFormatter: ((keys: string[]) => string) | null = null

/** 注册统一错误提示回调(App 层装配一次)。写盘失败经此提示,消费方无需各自弹错。 */
export function setSettingsErrorReporter(reporter: ((message: string) => void) | null): void {
  errorReporter = reporter
}

/** 注册「保存失败」文案构造(App 层注入 i18n)。 */
export function setSettingsWriteFailedFormatter(formatter: (() => string) | null): void {
  writeFailedFormatter = formatter
}

/** 注册「已保存但部分项未应用」的文案构造(App 层注入 i18n)。 */
export function setSettingsApplyFailedFormatter(
  formatter: ((keys: string[]) => string) | null,
): void {
  applyFailedFormatter = formatter
}

function formatWriteFailure(): string {
  return writeFailedFormatter?.() ?? '设置保存失败,已恢复为上一次确认的值。'
}

function formatApplyFailed(keys: string[]): string {
  return applyFailedFormatter?.(keys) ?? '设置已保存,但部分项未能应用: ' + keys.join(', ')
}

/** 读一个设置项:返回规范文本;未就绪或缺失返回 undefined(响应式,依赖中央快照)。 */
export function readSetting(key: string): string | undefined {
  return valuesRef.value[key]
}

function createDeferred(): { promise: Promise<void>; deferred: Deferred } {
  let deferred!: Deferred
  const promise = new Promise<void>((resolve, reject) => {
    deferred = { resolve, reject }
  })
  // 内部兜底挂一个 handler:消费方漏写 .catch 时不会产生未处理拒绝(其自身 .catch 照常生效)。
  promise.catch(() => {})
  return { promise, deferred }
}

function settleOk(batch: { deferreds: Deferred[] }): void {
  for (const d of batch.deferreds) d.resolve()
}

function settleErr(batch: { deferreds: Deferred[] }, error: unknown): void {
  for (const d of batch.deferreds) d.reject(error)
}

/**
 * 重算展示值:最后确认值打底,再按「旧 → 新」叠加同代次的未确认修改。
 * 代次已变的批次与待提交项一律不叠(它们已被取消,不能把旧值显示成当前值)。
 */
function rebuildDisplayedValues(): void {
  let merged: Record<string, string> = { ...confirmedValues }
  for (const batch of sendQueue) {
    if (batch.generation === currentGeneration) merged = { ...merged, ...batch.patch }
  }
  if (Object.keys(pendingPatch).length > 0 && pendingGeneration === currentGeneration) {
    merged = { ...merged, ...pendingPatch }
  }
  valuesRef.value = merged
}

/**
 * 应用一份权威快照:推进代次/revision、记下确认值,再叠加仍未确认的预览。
 *
 * **代次推进的处理集中在此**,不只在事件路径做:无论来源是重置回执、跨窗口事件还是主动重取,
 * 只要快照带来的 generation 变了,就说明发生过重置——本窗口旧代次的待提交与排队批次一律作废
 * (它们的值已被重置掉,留着会以新代次写回旧值,或让界面停在旧值上)。
 */
function applySnapshot(snapshot: SettingsSnapshot): void {
  const generationChanged = snapshot.generation !== currentGeneration
  // 代次推进即旧代次预览作废;先取消再落值,重建时不会把旧预览叠回去。
  if (generationChanged) cancelPendingEdits()
  currentGeneration = snapshot.generation
  settingsGeneration.value = snapshot.generation
  currentRevision = snapshot.revision
  confirmedValues = { ...snapshot.values }
  settingsConfirmedValues.value = { ...snapshot.values }
  settingsReady.value = true
  rebuildDisplayedValues()
  if (generationChanged) dropStaleQueuedBatches()
  for (const callback of appliedCallbacks) callback(snapshot)
}

/** 记下并如实报告「已保存、运行期应用失败」的键(不把部分成功报成完全成功)。 */
function applyChange(change: SettingsChange): void {
  settingsApplyFailed.value = [...change.apply_failed]
  settingsRestartRequired.value = [...change.restart_required]
  applySnapshot(change.snapshot)
  if (change.apply_failed.length === 0) return
  logger.warn('settings saved but apply failed', { keys: change.apply_failed })
  errorReporter?.(formatApplyFailed(change.apply_failed))
}

/** 清掉待提交定时器。 */
function clearPendingTimer(): void {
  if (pendingTimer !== null) {
    clearTimeout(pendingTimer)
    pendingTimer = null
  }
}

/**
 * 取消未发送的待提交集合:取消不是保存失败,故对其等待者 resolve(而非 reject)。
 * 重置、或跨窗口重置事件带来新代次时调用——旧代次的预览不得写回,也不得继续显示。
 */
function cancelPendingEdits(): void {
  clearPendingTimer()
  const deferreds = pendingDeferreds
  pendingPatch = {}
  pendingGeneration = 0
  pendingDeferreds = []
  pendingSince = 0
  settleOk({ deferreds })
}

/** 丢弃代次已变化的排队批次(尚在发送中的那个由 drain 在回执后按代次判定,不在此移除)。 */
function dropStaleQueuedBatches(): void {
  for (let i = sendQueue.length - 1; i >= 0; i -= 1) {
    const batch = sendQueue[i]
    if (batch.generation === currentGeneration) continue
    sendQueue.splice(i, 1)
    settleOk(batch)
  }
}

/** 把待提交集合移入发送队列(非防抖写入与 flush 共用)。 */
function enqueuePending(): void {
  if (Object.keys(pendingPatch).length === 0) return
  const batch: QueuedBatch = {
    patch: pendingPatch,
    // 代次在采集时记录,不在这里读 currentGeneration:重置后残留的待提交项必须以旧代次
    // 被识别并丢弃,而不是被贴上新一代的标签写回去。
    generation: pendingGeneration,
    deferreds: pendingDeferreds,
  }
  pendingPatch = {}
  pendingGeneration = 0
  pendingDeferreds = []
  pendingSince = 0
  clearPendingTimer()
  sendQueue.push(batch)
  void drain()
}

/** 串行发送队列:同一时刻只有一个请求在途,保证「最后的值最后落盘」。 */
function drain(): Promise<void> {
  drainPromise ??= (async () => {
    try {
      while (sendQueue.length > 0) {
        const batch = sendQueue[0]
        // 发送前复核代次:重置后队列里的旧批次直接丢弃,不占用一次写盘。
        if (batch.generation !== currentGeneration) {
          sendQueue.shift()
          settleOk(batch)
          rebuildDisplayedValues()
          continue
        }
        try {
          const change = await invokeIpc<SettingsChange>(IPC.SET_APP_SETTINGS, {
            patch: batch.patch,
            generation: batch.generation,
          })
          removeQueued(batch)
          // 回执期间代次可能已被重置事件推进:此时该回执针对的是已作废的旧配置,只作取消处理。
          if (batch.generation !== currentGeneration) {
            settleOk(batch)
          } else {
            // 过期响应丢弃:只有不低于当前 revision 的回执才被采纳。
            if (change.snapshot.revision >= currentRevision) applyChange(change)
            settleOk(batch)
          }
        } catch (e) {
          removeQueued(batch)
          if (batch.generation !== currentGeneration) {
            // 重置已使该批次作废:后端拒绝不算保存失败,不提示、不回滚。
            settleOk(batch)
          } else {
            logger.error('settings write failed', { keys: Object.keys(batch.patch), error: e })
            failureCount += 1
            lastFailure = e
            // 失败恢复:回到最后确认快照(较新预览由下方统一重算叠加)。
            errorReporter?.(formatWriteFailure())
            settleErr(batch, e)
          }
        }
        // 批次已离开队列:重算展示值——未确认预览只保留仍在队列/待提交中的那些,
        // 被丢弃的回执或失败的批次不会把值留在展示层。
        rebuildDisplayedValues()
      }
    } finally {
      drainPromise = null
    }
  })()
  return drainPromise
}

function removeQueued(batch: QueuedBatch): void {
  const index = sendQueue.indexOf(batch)
  if (index >= 0) sendQueue.splice(index, 1)
}

/** 安排防抖提交:空闲 300ms 提交;本轮已持续 ≥1s 则立即提交(最长等待)。 */
function schedulePending(): void {
  const now = Date.now()
  if (pendingSince === 0) pendingSince = now
  if (now - pendingSince >= DEBOUNCE_MAX_MS) {
    enqueuePending()
    return
  }
  clearPendingTimer()
  const remaining = Math.min(DEBOUNCE_IDLE_MS, DEBOUNCE_MAX_MS - (now - pendingSince))
  pendingTimer = setTimeout(() => {
    pendingTimer = null
    enqueuePending()
  }, remaining)
}

/**
 * 提交一批键值(规范文本)。
 *
 * 返回值在**该批真正落盘完成**时 resolve(防抖写入同样如此,便于统一 `.catch` 收口);
 * 保存失败时 reject,同时已按最后确认值回滚并给出可见提示。
 *
 * @param patch 键 → 规范文本。
 * @param options.debounce 连续操作(拖动/滑块)传 true:合并进待提交集合,空闲 300ms / 最长 1s 落盘。
 */
export function writeSettings(
  patch: Record<string, string>,
  options?: { debounce?: boolean },
): Promise<void> {
  if (!settingsReady.value) {
    // 快照未到达:此刻的改动基于占位默认值,写回会覆盖用户真实偏好——按契约不采集、不提交。
    logger.warn('writeSettings before settings ready, ignored', { keys: Object.keys(patch) })
    return Promise.resolve()
  }
  if (resetInProgress !== null) {
    // 重置期间暂停采集:此刻的写入属于被重置掉的那份配置,采集它会把旧值带进新代次。
    logger.warn('writeSettings during reset, ignored', { keys: Object.keys(patch) })
    return Promise.resolve()
  }

  if (Object.keys(pendingPatch).length === 0) pendingGeneration = currentGeneration
  pendingPatch = { ...pendingPatch, ...patch }
  const { promise, deferred } = createDeferred()
  pendingDeferreds.push(deferred)

  // 本地即时预览:先更新展示值(在 pending 并入之后重算),UI 立刻跟手。
  rebuildDisplayedValues()

  if (options?.debounce) {
    schedulePending()
    return promise
  }
  // 立即提交:连带已在待提交集合里的键一起发,同一批只写一次文件。
  enqueuePending()
  return promise
}

/**
 * 立即提交全部待保存改动并等待落盘完成(退出前 / 重置前 / 用户显式保存)。
 *
 * 循环条件同时看「待提交集合」与「发送队列」:等待在途请求期间新到的防抖写入(其定时器或
 * 立即提交都可能落在窗口内)必须一并提交并等待,否则 flush 会漏掉刚被拖出来的值。
 * 只要等待窗口内发生过写盘失败即 reject——退出流程据此拒绝退出;窗口内后续批次的成功
 * 不会抹掉前面的失败。
 */
export async function flushSettings(): Promise<void> {
  if (resetInProgress !== null) {
    // 重置在途:优先级更高,其成败由 reset 的调用方处理,此处不让重置的失败冒充「保存失败」。
    await resetInProgress.catch(() => {})
    return
  }
  const failuresBefore = failureCount
  let guard = 0
  // 每轮都把待提交集合发出,并重新检查它——收拢等待窗口内新产生的防抖写入。
  while (
    (sendQueue.length > 0 || drainPromise !== null || Object.keys(pendingPatch).length > 0) &&
    guard < 1000
  ) {
    guard += 1
    enqueuePending()
    const current = drainPromise
    if (current) await current
    else break
  }
  if (failureCount > failuresBefore) throw lastFailure
}

/**
 * 恢复默认设置:暂停采集,取消尚未发送的防抖预览,等待已发出的请求结束,再整份重置。
 * 重置期间重复调用共享同一 Promise(去重);失败回退到最后确认快照并原样抛出。
 */
export function resetSettings(): Promise<SettingsChange> {
  resetInProgress ??= (async () => {
    try {
      // 1) 取消尚未发送的预览(它的值即将被重置掉),再等已发出的请求结束。
      cancelPendingEdits()
      rebuildDisplayedValues()
      while (drainPromise !== null) await drainPromise
      // 2) 整份重置(后端按 schema 生成默认模板,原子替换一次)。
      const change = await invokeIpc<SettingsChange>(IPC.CLEAR_SETTINGS)
      // 回执期间可能已收到更新的快照(另一窗口的写入、外部编辑):此时这份重置回执已过期,
      // 采纳它会把配置倒退回去——与普通提交回执适用同一条 revision 守卫。
      if (change.snapshot.revision >= currentRevision) {
        // 重置成功即「磁盘已是默认值」:此前那些未落盘的修改已不存在,失败计数随之清零,
        // 免得随后的 flush 报出一个早已作废的失败。
        failureCount = 0
        lastFailure = null
        applyChange(change)
      }
      return change
    } catch (e) {
      logger.error('settings reset failed', { error: e })
      // 失败恢复:保留重置前最后确认的值与内存态(已取消的预览不恢复),并给出可见提示。
      rebuildDisplayedValues()
      errorReporter?.(formatWriteFailure())
      throw e
    } finally {
      // 兜底恢复采集:不因异常永久禁用保存。
      resetInProgress = null
    }
  })()
  return resetInProgress
}

/** 订阅「权威快照已应用」;返回取消订阅函数(组件内优先 watch(settingsValues))。 */
export function onSettingsApplied(callback: (snapshot: SettingsSnapshot) => void): () => void {
  appliedCallbacks.add(callback)
  return () => appliedCallbacks.delete(callback)
}

/**
 * 启动初始化:设置快照与内部状态一次往返取回,多处调用共享同一 Promise。
 * 只应用设置;firstLaunch/guideSeen 交由 App 层消费(快照刷新不重放引导状态)。
 */
export function initializeSettings(): Promise<StartupPayload> {
  startupPromise ??= (async () => {
    // 监听必须**先注册完成**再取快照:否则「取快照期间到达的重置/外部编辑事件」会丢失,
    // 而随后返回的旧快照还会把过期值装回来(revision 守卫只能拦住乱序,拦不住漏收)。
    // 注册失败不阻断启动:设置照常可读可写,只是本窗口没有跨窗口热同步。
    await ensureSettingsBridge()
    const payload = await invokeIpc<StartupPayload>(IPC.GET_STARTUP_CONFIG)
    // 事件可能先到:只在初始快照不更旧时应用(同一条准入规则)。
    if (payload.settings.revision >= currentRevision) applySnapshot(payload.settings)
    return payload
  })().catch((e) => {
    // 读取失败:保持未就绪(不把占位默认值当用户修改写回),并清掉缓存 Promise 允许重试。
    startupPromise = null
    throw e
  })
  return startupPromise
}

/**
 * 从后端重取权威快照(外部编辑 config.toml 后)。只读不写:不触发任何保存,
 * 避免「外部编辑器刚改的值被内存旧值回写」。
 */
export async function refreshSettingsFromBackend(): Promise<void> {
  try {
    const snapshot = await invokeIpc<SettingsSnapshot>(IPC.GET_SETTINGS_SNAPSHOT)
    if (snapshot.revision >= currentRevision) applySnapshot(snapshot)
  } catch (e) {
    logger.error('refresh settings snapshot failed', { error: e })
  }
}

/** config-file-changed 事件的载荷:统一发完整快照。 */
interface ConfigFileChangedPayload {
  snapshot: SettingsSnapshot
  keys: string[]
  restart_required: string[]
  apply_failed: string[]
}

/**
 * 挂全局跨窗口监听(幂等):收到完整快照即应用,不按 keys 选择性刷新、不触发保存。
 * 事件带来的代次推进(重置广播)同时丢弃本窗口旧代次的待提交与排队批次。
 */
export function installSettingsBridge(): void {
  void ensureSettingsBridge()
}

/**
 * 挂全局跨窗口监听并等待注册真正完成(幂等,共享同一 Promise)。
 *
 * 启动流程必须 await 它再取初始快照:listen 本身是异步的,若只发起不等待,从「开始取快照」到
 * 「监听就绪」之间到达的重置/外部编辑事件会被漏掉,而随后返回的旧快照又会把过期值装回来。
 * 注册失败不抛出(无 Tauri runtime 的浏览器/测试环境本就如此),由调用方决定是否重试。
 */
export function ensureSettingsBridge(): Promise<void> {
  if (listenersPromise !== null) return listenersPromise
  // listen 是异步的(内部要注册回调),事件总线不可用(无 Tauri runtime 的浏览器/测试环境)时
  // 以拒绝的形式失败——故必须挂 .catch,否则成为未处理拒绝。此路失败不影响设置读写本身,
  // 只是没有跨窗口热同步;注册句柄保留,便于测试复位后重试。
  listenersPromise = listenAppEvent<ConfigFileChangedPayload>(
    EVENTS.CONFIG_FILE_CHANGED,
    (event) => {
      const payload = event.payload
      if (!payload?.snapshot) return
      // 按 revision 守卫:迟到的旧事件不能把旧快照装回来。
      if (payload.snapshot.revision < currentRevision) return
      settingsApplyFailed.value = [...(payload.apply_failed ?? [])]
      settingsRestartRequired.value = [...(payload.restart_required ?? [])]
      // 代次/预览作废的处理集中在 applySnapshot(事件、重置回执、主动重取共用同一准入规则)。
      applySnapshot(payload.snapshot)
    },
  )
    .then(() => {})
    .catch((e) => {
      logger.warn('settings bridge unavailable, cross-window sync disabled', { error: e })
    })
  return listenersPromise
}

/** 供测试复位模块级状态(node 环境各用例之间隔离)。 */
export function resetSettingsModuleStateForTests(): void {
  clearPendingTimer()
  valuesRef.value = {}
  settingsApplyFailed.value = []
  settingsRestartRequired.value = []
  settingsReady.value = false
  confirmedValues = {}
  settingsConfirmedValues.value = {}
  currentGeneration = 0
  settingsGeneration.value = 0
  currentRevision = 0
  pendingPatch = {}
  pendingGeneration = 0
  pendingDeferreds = []
  pendingSince = 0
  sendQueue.length = 0
  drainPromise = null
  failureCount = 0
  lastFailure = null
  resetInProgress = null
  startupPromise = null
  listenersPromise = null
  appliedCallbacks.clear()
  errorReporter = null
  writeFailedFormatter = null
  applyFailedFormatter = null
}

export {
  settingsReady,
  settingsApplyFailed,
  settingsRestartRequired,
  settingsGeneration,
  settingsConfirmedValues,
}
export type { SettingsChange, SettingsSnapshot, StartupPayload }
