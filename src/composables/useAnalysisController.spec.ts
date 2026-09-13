// useAnalysisController.spec.ts — P1-1 自动续跑 / 资源等待 / 控制顺序（先表征、后修复）。
//
// 契约：AI 与人脸共用唯一 GPU 分析会话槽（后端 gpu_analysis_owner：单锁区 check-and-claim），
// App.vue 启动时并行触发两者自动续跑。修复前：竞争失败方把「会话被对端持有」当普通错误交
// onError，而 running=false 时既无轮询也无重试 → 「期望运行」意图与实际执行脱节，失败方一直停着。
//
// 本文件钉住这些终止条件：
//   · 对端持有会话 → 进资源等待（未 isAnalyzing 也在状态区显示等待原因），不报错误；
//   · 对端释放会话 → 自动续跑（两种抢占顺序对称，不叠出第二个 owner）；
//   · 等待期只读快照，不重复往返 start；
//   · 用户暂停/停止 → 撤回等待意图，不误唤醒；且在途 start 先收尾再发 pause/stop
//     （后端 start 在 spawn_blocking 同步段之后才 launch，cancel 抢跑会被旧 start 反超）；
//   · 真实错误（模型未就绪等）→ 不进等待、不重试，仍走 onError；
//   · 重复自动续传 / 迟到应答不双启动、不复活运行态。
//
// 「资源等待」判定复用后端既有 waitingOn 通道：未运行 + 仍有剩余 + 共享会话被对端持有时输出
// ANALYSIS_BUSY_WAITING_KEY（'analysisBusy'）。用户手动暂停不输出该键，本地等待意图才是唯一
// 真源——故 UI 能把「资源等待（会自动续跑）」与「手动暂停（保留续跑意图）」分开显示。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'
import { IPC } from '../constants/ipc'
import { IpcError } from '../utils/ipc'
import type { AnalysisCommands, BaseAnalysisStatus } from './useAnalysisController'

const invokeIpc = vi.hoisted(() => vi.fn())
vi.mock('../utils/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/ipc')>()
  return { ...actual, invokeIpc }
})
// i18n / logger 仅 providerLabel、日志与起步预检用：node 环境给最小替身，不拉真实 locale 与日志桥。
vi.mock('../i18n', () => ({ default: { global: { t: (key: string) => key } } }))
vi.mock('../utils/logger', () => ({
  logger: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
}))
// 扫描根守卫（startAnalysis 的起步预检）用桩：本测覆盖会话竞争顺序，不覆盖扫描根。
vi.mock('../stores/scanStore', () => ({ useScanStore: () => ({ hasScanRoots: true }) }))

import { ANALYSIS_BUSY_WAITING_KEY, useAnalysisController } from './useAnalysisController'

type Side = 'ai' | 'face'
type Action = 'pause' | 'stop'

const COMMANDS: Record<Side, AnalysisCommands> = {
  ai: {
    getStatus: IPC.GET_AI_STATUS,
    start: IPC.START_AI_ANALYSIS,
    pause: IPC.PAUSE_AI_ANALYSIS,
    restart: IPC.RESTART_AI_ANALYSIS,
    stop: IPC.STOP_AI_ANALYSIS,
  },
  face: {
    getStatus: IPC.GET_FACE_STATUS,
    start: IPC.START_FACE_ANALYSIS,
    pause: IPC.PAUSE_FACE_ANALYSIS,
    restart: IPC.RESTART_FACE_ANALYSIS,
    stop: IPC.STOP_FACE_ANALYSIS,
  },
}

interface Gate {
  promise: Promise<void>
  open: () => void
}

function deferred(): Gate {
  let open!: () => void
  const promise = new Promise<void>((res) => {
    open = res
  })
  return { promise, open }
}

/**
 * 最小假后端：单一共享 GPU 分析会话槽 + 两侧状态快照。
 * - start/restart 被对端持有即拒绝，且不留半应用状态（前端按稳定 code 分流，不看文案）。
 * - 状态 waitingOn 与后端契约同形：未运行 + 有剩余 + 槽被对端持有 → 报等待原因键。
 * - delayNextStart 模拟后端 start「spawn_blocking 同步段之后才 launch」的窗口：命令在该窗口内
 *   未占槽也未开跑，若 cancel 抢跑，旧 start 稍后仍会把会话真正跑起来。
 */
function installFakeBackend(startFailure: Partial<Record<Side, unknown>> = {}) {
  const slot: { owner: Side | null } = { owner: null }
  const running: Record<Side, boolean> = { ai: false, face: false }
  const active: Record<Side, boolean> = { ai: true, face: true }
  const pending: Record<Side, number> = { ai: 3, face: 3 }
  const startCounts: Record<Side, number> = { ai: 0, face: 0 }
  const statusCounts: Record<Side, number> = { ai: 0, face: 0 }
  let heldStatus: { side: Side; gate: Gate } | null = null
  let delayedStart: { side: Side; gate: Gate } | null = null

  function statusOf(side: Side): BaseAnalysisStatus {
    const otherHolds = slot.owner !== null && slot.owner !== side
    const busyWaiting = !running[side] && pending[side] > 0 && otherHolds
    return {
      provider: 'cpu',
      isAnalyzing: running[side],
      analysisActive: active[side],
      totalItems: 5,
      pendingItems: pending[side],
      waitingOn: busyWaiting ? [ANALYSIS_BUSY_WAITING_KEY] : [],
    }
  }

  /** 占槽开跑；被对端持有则按稳定 code 拒绝（不留半应用状态）。 */
  function launch(side: Side): void {
    if (slot.owner !== null && slot.owner !== side) {
      throw new IpcError('AnalysisBusy', 'BUSY')
    }
    slot.owner = side
    running[side] = true
    active[side] = true
  }

  invokeIpc.mockImplementation((cmd: string) => {
    for (const side of ['ai', 'face'] as const) {
      if (cmd === COMMANDS[side].getStatus) {
        statusCounts[side] += 1
        // 快照在**请求时刻**生成:挂起的那一轮放行后落地的是「发出时」的陈旧内容——这才是
        // 「迟到应答」的真实形态(放行后重算会让快照无意间变新,测不出代次守卫)。
        const snapshot = statusOf(side)
        if (heldStatus?.side === side) {
          const gate = heldStatus.gate
          heldStatus = null
          return gate.promise.then(() => snapshot)
        }
        return Promise.resolve(snapshot)
      }
      if (cmd === COMMANDS[side].start || cmd === COMMANDS[side].restart) {
        startCounts[side] += 1
        const failure = startFailure[side]
        if (failure) return Promise.reject(failure)
        const delayed = delayedStart?.side === side ? delayedStart : null
        if (delayed) delayedStart = null
        const settle = () => launch(side)
        return delayed ? delayed.gate.promise.then(settle) : Promise.resolve().then(settle)
      }
      if (cmd === COMMANDS[side].pause) {
        running[side] = false
        if (slot.owner === side) slot.owner = null
        return Promise.resolve(undefined)
      }
      if (cmd === COMMANDS[side].stop) {
        running[side] = false
        active[side] = false
        if (slot.owner === side) slot.owner = null
        return Promise.resolve(undefined)
      }
    }
    return Promise.reject(new Error('unexpected command ' + cmd))
  })

  return {
    slot,
    running,
    active,
    startCounts,
    statusCounts,
    /** 先跑者自然完成：释放共享会话槽（等价于后端完成回调的 release）。 */
    finish(side: Side) {
      running[side] = false
      if (slot.owner === side) slot.owner = null
    },
    /** 下一次该侧状态查询挂起，返回放行函数——用于晚响应用例。 */
    holdNextStatus(side: Side) {
      const gate = deferred()
      heldStatus = { side, gate }
      return gate.open
    },
    /** 下一次该侧 start/restart 延迟到放行才真正 launch——用于「cancel 抢跑」次序用例。 */
    delayNextStart(side: Side) {
      const gate = deferred()
      delayedStart = { side, gate }
      return gate.open
    },
  }
}

function makeController(side: Side) {
  const status = ref<BaseAnalysisStatus>({
    provider: 'cpu',
    isAnalyzing: false,
    analysisActive: true,
    totalItems: 5,
    pendingItems: 3,
    waitingOn: [],
  })
  const onError = vi.fn()
  const controller = useAnalysisController<BaseAnalysisStatus>({
    status,
    commands: COMMANDS[side],
    analyzedCount: () => 0,
    logTag: side === 'ai' ? '[AI]' : '[Face]',
    onError,
  })
  return { status, onError, controller }
}

/** 让挂起的微任务链跑完（替身 IPC 均为立即 resolve 的 promise），直到条件成立。 */
async function flushUntil(cond: () => boolean, hops = 50) {
  for (let i = 0; i < hops && !cond(); i++) await Promise.resolve()
}

const ORDERS: Array<[Side, Side]> = [
  ['ai', 'face'],
  ['face', 'ai'],
]

beforeEach(() => {
  invokeIpc.mockReset()
  vi.useFakeTimers({ toFake: ['setTimeout', 'setInterval', 'clearTimeout', 'clearInterval'] })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useAnalysisController：共享 GPU 分析会话的自动续跑与等待态（P1-1）', () => {
  it.each(ORDERS)(
    '竞争失败方 %s→%s 进入资源等待：未运行也显示等待原因，且不报错误',
    async (first, second) => {
      const backend = installFakeBackend()
      const winner = makeController(first)
      const waiter = makeController(second)

      await winner.controller.maybeAutoResume()
      expect(winner.status.value.isAnalyzing).toBe(true)
      expect(backend.slot.owner).toBe(first)

      await waiter.controller.maybeAutoResume()
      expect(waiter.controller.waitingForSession.value).toBe(true)
      expect(waiter.status.value.isAnalyzing).toBe(false)
      expect(waiter.onError).not.toHaveBeenCalled()
      // 运行中让步等待（isWaitingBlocked）与资源等待是两件事：前者要求本端在跑。
      expect(waiter.controller.isWaitingBlocked.value).toBe(false)

      // 等待期首轮快照：后端出「对端持有共享会话」等待原因，UI 据此出文案。
      await vi.advanceTimersByTimeAsync(2000)
      expect(waiter.status.value.waitingOn).toEqual([ANALYSIS_BUSY_WAITING_KEY])
      expect(backend.slot.owner).toBe(first)
    },
  )

  it.each(ORDERS)('先跑者 %s 完成后等待方 %s 自动续跑，且先跑者不被误唤醒', async (first, second) => {
    const backend = installFakeBackend()
    const winner = makeController(first)
    const waiter = makeController(second)

    await winner.controller.maybeAutoResume()
    await waiter.controller.maybeAutoResume()
    expect(waiter.controller.waitingForSession.value).toBe(true)

    backend.finish(first)
    await nextTick()
    await vi.advanceTimersByTimeAsync(2000)

    expect(waiter.status.value.isAnalyzing).toBe(true)
    expect(waiter.controller.waitingForSession.value).toBe(false)
    expect(backend.slot.owner).toBe(second)
    expect(waiter.onError).not.toHaveBeenCalled()
    expect(winner.status.value.isAnalyzing).toBe(false)
  })

  it('等待期重复 tick 不重复发起启动；拿到会话后停止等待轮询', async () => {
    const backend = installFakeBackend()
    const winner = makeController('ai')
    const waiter = makeController('face')

    await winner.controller.maybeAutoResume()
    await waiter.controller.maybeAutoResume()
    expect(backend.startCounts.face).toBe(1)

    // 等待期重复入队（取消隐藏补跑 / 启动批重入）不双启动。
    await waiter.controller.maybeAutoResume()
    expect(backend.startCounts.face).toBe(1)

    // 对端仍在跑：多个 tick 不得重复往返 start（只读状态快照），也不得产生第二个 owner。
    await vi.advanceTimersByTimeAsync(6000)
    expect(backend.startCounts.face).toBe(1)
    expect(backend.slot.owner).toBe('ai')

    backend.finish('ai')
    await vi.advanceTimersByTimeAsync(2000)
    expect(backend.startCounts.face).toBe(2)
    expect(backend.slot.owner).toBe('face')

    // 已运行：等待轮询结束，不再有额外启动。
    await vi.advanceTimersByTimeAsync(6000)
    expect(backend.startCounts.face).toBe(2)
  })

  it.each<Action>(['pause', 'stop'])('用户 %s 撤回等待意图：对端释放后不误唤醒', async (action) => {
    const backend = installFakeBackend()
    const winner = makeController('ai')
    const waiter = makeController('face')

    await winner.controller.maybeAutoResume()
    await waiter.controller.maybeAutoResume()
    expect(waiter.controller.waitingForSession.value).toBe(true)

    if (action === 'pause') await waiter.controller.pauseAnalysis()
    else await waiter.controller.stopAnalysis()
    expect(waiter.controller.waitingForSession.value).toBe(false)
    expect(waiter.controller.isWaitingBlocked.value).toBe(false)

    backend.finish('ai')
    await nextTick()
    await vi.advanceTimersByTimeAsync(4000)
    expect(waiter.status.value.isAnalyzing).toBe(false)
    expect(backend.slot.owner).toBeNull()
  })

  it('真实错误不进等待态、不重试，仍交 onError', async () => {
    const backend = installFakeBackend({
      face: new IpcError('AiModelNotLoaded', '人脸模型未启用或未下载'),
    })
    const face = makeController('face')

    await face.controller.maybeAutoResume()
    expect(face.controller.waitingForSession.value).toBe(false)
    expect(face.onError).toHaveBeenCalledWith('autoResume', expect.anything())

    await vi.advanceTimersByTimeAsync(6000)
    expect(backend.startCounts.face).toBe(1)
  })

  it('等待期状态响应迟到：用户已撤回意图时不得据陈旧快照续跑', async () => {
    const backend = installFakeBackend()
    const winner = makeController('ai')
    const waiter = makeController('face')

    await winner.controller.maybeAutoResume()
    await waiter.controller.maybeAutoResume()

    const release = backend.holdNextStatus('face')
    await nextTick()
    await vi.advanceTimersByTimeAsync(2000)

    // 用户在状态响应未落定时停止分析：等待意图即刻撤回。
    const stopping = waiter.controller.stopAnalysis()
    release()
    await stopping
    expect(waiter.controller.waitingForSession.value).toBe(false)

    backend.finish('ai')
    await vi.advanceTimersByTimeAsync(4000)
    expect(waiter.status.value.isAnalyzing).toBe(false)
    expect(backend.slot.owner).toBeNull()
  })

  it('并发重复自动续传只发一次 start（合并在途起步）', async () => {
    const backend = installFakeBackend()
    const ai = makeController('ai')
    const openStart = backend.delayNextStart('ai')

    const first = ai.controller.maybeAutoResume()
    const second = ai.controller.maybeAutoResume()
    openStart()
    await first
    await second

    expect(backend.startCounts.ai).toBe(1)
    expect(backend.slot.owner).toBe('ai')
  })

  it.each<[Side, Action]>([
    ['ai', 'pause'],
    ['ai', 'stop'],
    ['face', 'pause'],
    ['face', 'stop'],
  ])(
    '在途 start（%s）已发出后用户 %s：先等 start 收尾再发命令，最终后端确实停住',
    async (side, action) => {
      const backend = installFakeBackend()
      const c = makeController(side)

      const openStart = backend.delayNextStart(side)
      const starting = c.controller.maybeAutoResume()
      await flushUntil(() => backend.startCounts[side] === 1)
      // 前提前置：start 命令确已发出且挂起在后端同步段（尚未占槽、尚未开跑）。
      expect(backend.startCounts[side]).toBe(1)
      expect(backend.running[side]).toBe(false)
      expect(backend.slot.owner).toBeNull()

      const stopping =
        action === 'pause' ? c.controller.pauseAnalysis() : c.controller.stopAnalysis()
      openStart() // 旧 start 此刻才真正 launch
      await starting
      await stopping

      // 不只看 UI：最后实际持有者与运行态必须已停住（cancel 抢跑会被旧 start 反超）。
      expect(backend.running[side]).toBe(false)
      expect(backend.slot.owner).toBeNull()
      expect(c.status.value.isAnalyzing).toBe(false)
      expect(c.status.value.analysisActive).toBe(action === 'pause')
    },
  )

  it.each<Action>(['pause', 'stop'])('起步预检期间用户 %s：不发出这条 start，后端不跑', async (action) => {
    const backend = installFakeBackend()
    const c = makeController('ai')

    const starting = c.controller.startAnalysis() // 预检（扫描根守卫）在途
    const stopping = action === 'pause' ? c.controller.pauseAnalysis() : c.controller.stopAnalysis()
    await starting
    await stopping

    expect(backend.startCounts.ai).toBe(0)
    expect(backend.running.ai).toBe(false)
    expect(backend.slot.owner).toBeNull()
    expect(c.status.value.isAnalyzing).toBe(false)
  })

  it('等待期重试 start 在途时用户停止：迟到成功应答不复活运行态，后端停住', async () => {
    const backend = installFakeBackend()
    const winner = makeController('ai')
    const waiter = makeController('face')

    await winner.controller.maybeAutoResume()
    await waiter.controller.maybeAutoResume()
    expect(waiter.controller.waitingForSession.value).toBe(true)

    backend.finish('ai') // 对端释放共享会话
    const openStart = backend.delayNextStart('face')
    await vi.advanceTimersByTimeAsync(2000) // 等待期 tick 发起重试 start（挂起在同步段）
    expect(backend.startCounts.face).toBe(2)

    const stopping = waiter.controller.stopAnalysis()
    openStart() // 迟到的 start 此刻才真正 launch
    await stopping

    expect(backend.running.face).toBe(false)
    expect(backend.slot.owner).toBeNull()
    expect(waiter.status.value.isAnalyzing).toBe(false)
    expect(waiter.status.value.analysisActive).toBe(false)
    expect(waiter.controller.waitingForSession.value).toBe(false)
  })

  it('自动续传的状态查询在途时用户停止：迟到的可续跑快照不得复活运行态', async () => {
    const backend = installFakeBackend()
    const c = makeController('ai')

    // 该快照在请求时刻生成（未运行 + active=1 + 有剩余）→ 落地时内容仍「看起来可续跑」。
    const release = backend.holdNextStatus('ai')
    const resuming = c.controller.maybeAutoResume()
    await flushUntil(() => backend.statusCounts.ai === 1) // 状态查询确已发出且挂起

    const stopping = c.controller.stopAnalysis() // 状态在途期间用户停止
    release()
    await stopping
    await resuming

    expect(backend.startCounts.ai).toBe(0)
    expect(backend.running.ai).toBe(false)
    expect(backend.active.ai).toBe(false)
    expect(c.status.value.isAnalyzing).toBe(false)
  })

  it('restart 等在途 start 收尾期间用户停止：不再发出破坏性重置', async () => {
    const backend = installFakeBackend()
    const c = makeController('ai')

    const openStart = backend.delayNextStart('ai')
    const starting = c.controller.startAnalysis()
    await flushUntil(() => backend.startCounts.ai === 1)

    const restarting = c.controller.restartAnalysis() // 与 stop 抢同一条在途 start
    const stopping = c.controller.stopAnalysis()
    openStart()
    await starting
    await restarting
    await stopping

    // startCounts 同时计 start/restart：仍为 1 即「破坏性 reset 未发出」。
    expect(backend.startCounts.ai).toBe(1)
    expect(backend.running.ai).toBe(false)
    expect(backend.slot.owner).toBeNull()
    expect(c.status.value.analysisActive).toBe(false)
  })
})
