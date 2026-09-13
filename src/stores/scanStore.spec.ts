// scanStore 单测：根文件夹显隐（V21）。锁三件事：
//  1. visibleScanRoots —— 侧栏文件树的可见根源。隐藏根必须被滤掉、可见根必须留下。
//  2. setScanRootHidden —— 调对 IPC、本地 isHidden 同步翻转、并触发画廊重排 + 统计刷新
//     （漏 invalidateLayout 的症状是「设置页点了隐藏、画廊却不更新」）。
//  3. unhide 补跑 —— 取消隐藏才触发 AI/人脸 maybeAutoResume（隐藏时绝不触发）；缩略图靠
//     画廊 on-demand 路径补跑，故此处不校验缩略图触发。

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import type { ScanRoot } from '../types/media'
import { IPC, EVENTS } from '../constants/ipc'

const operationIds = vi.hoisted(() => ({ next: 0 }))
const invokeIpc = vi.fn((..._args: unknown[]) => Promise.resolve<unknown>(undefined))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
  generateOperationId: () => {
    operationIds.next += 1
    return operationIds.next === 1 ? 'test-run' : `test-run-${operationIds.next}`
  },
}))

// Channel 桩:startScan 只需构造后赋 onmessage 回调,不触碰真实 Tauri window transform。
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class Channel {
    onmessage: ((_msg: unknown) => void) | undefined
  },
}))

// Tauri 事件桩:记录 handler 供测试内手动投递事件(缩略图进度改走 app 级事件,见 store 注释)。
const eventListeners = new Map<string, (e: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    eventListeners.set(name, cb)
    return Promise.resolve(() => {})
  },
}))

// mediaStore 的副作用（画廊重排 / 统计刷新）用桩，只断言被调用，不真跑取数。
const invalidateLayout = vi.fn()
const loadStats = vi.fn()
const invalidateStats = vi.fn()
vi.mock('./mediaStore', () => ({
  useMediaStore: () => ({ invalidateLayout, loadStats, invalidateStats }),
}))

// logger 走桩:统计刷新的失败路径只应留下日志,不得冒未处理拒绝。
vi.mock('../utils/logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

// uiStore 为 startScan 提供视图序参数(后台 enrichment 补全顺序)。
const uiPrefs = { groupBy: 'date', sortWithinGroup: 'datetime', sortOrder: 'desc' }
vi.mock('./uiStore', () => ({
  useUiStore: () => uiPrefs,
}))

// AI/人脸 store 用桩：setScanRootHidden 在 unhide 分支动态 import 它们并调 maybeAutoResume。
// vi.mock 同时拦截静态与动态 import，故此桩对 `await import('./aiStore')` 生效。
const aiAutoResume = vi.fn()
const faceAutoResume = vi.fn()
vi.mock('./aiStore', () => ({
  useAiStore: () => ({ maybeAutoResume: aiAutoResume }),
}))
vi.mock('./faceStore', () => ({
  useFaceStore: () => ({ maybeAutoResume: faceAutoResume }),
}))

import { useScanStore } from './scanStore'
import { logger } from '../utils/logger'

function root(id: number, isHidden: boolean): ScanRoot {
  return {
    id,
    path: `/r${id}`,
    alias: `R${id}`,
    scanStatus: 'idle',
    scanProgress: 0,
    totalFiles: 0,
    lastScanAt: null,
    isActive: true,
    createdAt: 0,
    updatedAt: 0,
    isHidden,
  }
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

beforeEach(() => {
  setActivePinia(createPinia())
  operationIds.next = 0
  invokeIpc.mockReset()
  invokeIpc.mockImplementation(() => Promise.resolve(undefined))
  invalidateLayout.mockClear()
  loadStats.mockClear()
  loadStats.mockImplementation(() => Promise.resolve())
  invalidateStats.mockClear()
  aiAutoResume.mockClear()
  faceAutoResume.mockClear()
  eventListeners.clear()
})

describe('scanStore 根文件夹显隐（V21）', () => {
  it('visibleScanRoots 滤掉隐藏根、保留可见根', () => {
    const scan = useScanStore()
    scan.scanRoots = [root(1, false), root(2, true), root(3, false)]
    expect(scan.visibleScanRoots.map((r) => r.id)).toEqual([1, 3])
  })

  it('setScanRootHidden 调对 IPC + 本地翻转 isHidden + 触发画廊/统计刷新', async () => {
    const scan = useScanStore()
    scan.scanRoots = [root(1, false), root(2, false)]

    await scan.setScanRootHidden(2, true)

    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_SCAN_ROOT_HIDDEN, { id: 2, hidden: true })
    expect(scan.scanRoots.find((r) => r.id === 2)?.isHidden).toBe(true)
    expect(scan.visibleScanRoots.map((r) => r.id)).toEqual([1])
    expect(invalidateLayout).toHaveBeenCalledOnce()
    expect(loadStats).toHaveBeenCalledOnce()
    // 隐藏（非 unhide）不得触发 AI/人脸补跑，也不得踢派生流水线。
    expect(aiAutoResume).not.toHaveBeenCalled()
    expect(faceAutoResume).not.toHaveBeenCalled()
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.DERIVATION_STATUS)
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.START_DERIVATION)
  })

  it('setScanRootHidden 取消隐藏后根回到可见集 + 触发 AI/人脸补跑', async () => {
    const scan = useScanStore()
    scan.scanRoots = [root(1, true)]

    await scan.setScanRootHidden(1, false)

    expect(invokeIpc).toHaveBeenCalledWith(IPC.SET_SCAN_ROOT_HIDDEN, { id: 1, hidden: false })
    expect(scan.visibleScanRoots.map((r) => r.id)).toEqual([1])
    // unhide 补跑：AI/人脸各续跑一次（maybeAutoResume 内部再判 analysisActive）。
    expect(aiAutoResume).toHaveBeenCalledOnce()
    expect(faceAutoResume).toHaveBeenCalledOnce()
    // 派生封面踢重启：先查状态（mock 返回 undefined = 未运行）再 start。
    expect(invokeIpc).toHaveBeenCalledWith(IPC.DERIVATION_STATUS)
    expect(invokeIpc).toHaveBeenCalledWith(IPC.START_DERIVATION)
  })
})

describe('scanStore 增量重扫 quick 接线(阶段3)', () => {
  it('首扫 quick=false,快扫完成后同一根重扫默认 quick=true', async () => {
    const scan = useScanStore()
    const scanArgs: Array<Record<string, unknown>> = []
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        const record = (args ?? {}) as Record<string, unknown>
        scanArgs.push(record)
        // 模拟后端快扫完成回传:目录 mtime/media_count 基线已写回。
        const channel = record.onProgress as { onmessage: (m: unknown) => void } | undefined
        channel?.onmessage({
          type: 'completed',
          rootId: 99,
          runId: 'test-run',
          totalItems: 0,
          totalBytes: 0,
          elapsedMs: 1,
          markedMissing: 0,
        })
      }
      return Promise.resolve(undefined)
    })

    await scan.startScan(99)
    expect(scanArgs[0]?.quick).toBe(false)

    await scan.startScan(99)
    expect(scanArgs[1]?.quick).toBe(true)
  })

  it('累计扫描体积，并忽略旧 runId 的迟到进度', async () => {
    let channel: { onmessage?: (message: unknown) => void } | undefined
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        channel = (args as { onProgress: typeof channel })?.onProgress
      }
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    await scan.startScan(7)

    channel?.onmessage?.({
      type: 'progress',
      rootId: 7,
      runId: 'stale-run',
      scanned: 99,
      total: 100,
      processedBytes: 99_000,
      totalBytes: 100_000,
      currentDir: 'stale',
      status: 'scanning',
    })
    expect(scan.aggregateProgress?.processedBytes).toBe(0)

    channel?.onmessage?.({
      type: 'progress',
      rootId: 7,
      runId: 'test-run',
      scanned: 2,
      total: 0,
      processedBytes: 1024,
      totalBytes: null,
      currentDir: 'current',
      status: 'scanning',
    })
    expect(scan.aggregateProgress).toMatchObject({
      phase: 'scanning',
      processedFiles: 2,
      processedBytes: 1024,
      totalBytes: null,
    })
  })
})

describe('scanStore 根列表响应隔离', () => {
  it('较早的 list_scan_roots 响应不能覆盖较新的刷新结果', async () => {
    const first = deferred<ScanRoot[]>()
    const second = deferred<ScanRoot[]>()
    let listCount = 0
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd !== IPC.LIST_SCAN_ROOTS) return Promise.resolve(undefined)
      listCount += 1
      return listCount === 1 ? first.promise : second.promise
    })

    const scan = useScanStore()
    const oldLoad = scan.loadScanRoots()
    const newLoad = scan.loadScanRoots()
    second.resolve([root(2, false)])
    expect(await newLoad).toBe(true)
    first.resolve([root(1, false)])
    expect(await oldLoad).toBe(false)

    expect(scan.scanRoots.map((item) => item.id)).toEqual([2])
    expect(scan.isLoadingRoots).toBe(false)
  })

  it('clearDatabase 使清库前的根列表响应失效，并阻挡清库期间启动扫描', async () => {
    const oldList = deferred<ScanRoot[]>()
    const clearResult = deferred<unknown>()
    let startCount = 0
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.LIST_SCAN_ROOTS) return oldList.promise
      if (cmd === IPC.CLEAR_DATABASE) return clearResult.promise
      if (cmd === IPC.START_SCAN) {
        startCount += 1
        return Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    const oldLoad = scan.loadScanRoots()
    const clear = scan.clearDatabase()
    const start = scan.startScan(7)
    expect(startCount).toBe(0)

    clearResult.resolve(undefined)
    await clear
    oldList.resolve([root(1, false)])
    await oldLoad
    await start

    expect(scan.scanRoots).toEqual([])
    expect(startCount).toBe(1)
    expect(scan.progressMap[7]?.isRunning).toBe(true)
  })

  it('clearDatabase 不让清库前的 add/remove 响应覆盖清库后的根与新进度', async () => {
    const oldAdd = deferred<ScanRoot>()
    const oldRemove = deferred<{ cleared_count: number }>()
    const clearResult = deferred<unknown>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.ADD_SCAN_ROOT) return oldAdd.promise
      if (cmd === IPC.REMOVE_SCAN_ROOT_WITH_OPTIONS) return oldRemove.promise
      if (cmd === IPC.CLEAR_DATABASE) return clearResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    scan.scanRoots = [root(1, false)]
    scan.progressMap[7] = {
      runId: 'old-run',
      scanned: 1,
      total: 2,
      processedBytes: 10,
      totalBytes: 20,
      currentDir: 'old',
      isRunning: true,
      status: 'scanning',
    }
    const add = scan.addScanRoot('/r2')
    const remove = scan.removeScanRoot(7, false)
    const clear = scan.clearDatabase()

    clearResult.resolve(undefined)
    await clear
    scan.progressMap[7] = {
      runId: 'new-run',
      scanned: 0,
      total: 0,
      processedBytes: 0,
      totalBytes: null,
      currentDir: '',
      isRunning: true,
      status: 'discovering',
    }
    oldAdd.resolve(root(2, false))
    oldRemove.resolve({ cleared_count: 0 })
    await add
    await remove

    expect(scan.scanRoots).toEqual([])
    expect(scan.progressMap[7]).toMatchObject({ runId: 'new-run', isRunning: true })
  })

  it('清库会使清库前的 hidden continuation 失效，不触发 unhide 副作用', async () => {
    const hiddenResult = deferred<unknown>()
    const clearResult = deferred<unknown>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.SET_SCAN_ROOT_HIDDEN) return hiddenResult.promise
      if (cmd === IPC.CLEAR_DATABASE) return clearResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    scan.scanRoots = [root(1, true)]
    const hidden = scan.setScanRootHidden(1, false)
    const clear = scan.clearDatabase()

    hiddenResult.resolve(undefined)
    clearResult.resolve(undefined)
    await hidden
    await clear

    expect(scan.scanRoots).toEqual([])
    expect(aiAutoResume).not.toHaveBeenCalled()
    expect(faceAutoResume).not.toHaveBeenCalled()
    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.DERIVATION_STATUS)
  })

  it('清库在派生状态查询期间开始时，不会启动派生流水线', async () => {
    const derivationStatus = deferred<{ isRunning: boolean }>()
    const clearResult = deferred<unknown>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.DERIVATION_STATUS) return derivationStatus.promise
      if (cmd === IPC.CLEAR_DATABASE) return clearResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    scan.scanRoots = [root(1, true)]
    const hidden = scan.setScanRootHidden(1, false)
    await vi.waitFor(() => expect(invokeIpc).toHaveBeenCalledWith(IPC.DERIVATION_STATUS))

    const clear = scan.clearDatabase()
    derivationStatus.resolve({ isRunning: false })
    clearResult.resolve(undefined)
    await hidden
    await clear

    expect(invokeIpc).not.toHaveBeenCalledWith(IPC.START_DERIVATION)
  })
})

describe('scanStore 扫描期统计刷新门限(store 级跨根全局)', () => {
  function progressMsg(rootId: number, runId: string, status = 'scanning') {
    return {
      type: 'progress',
      rootId,
      runId,
      scanned: 1,
      total: 2,
      processedBytes: 10,
      totalBytes: 20,
      currentDir: '',
      status,
    }
  }

  async function startTwoRoots() {
    const channels: Array<{ onmessage?: (msg: unknown) => void }> = []
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        channels.push((args as { onProgress: { onmessage?: (msg: unknown) => void } }).onProgress)
      }
      return Promise.resolve(undefined)
    })
    const scan = useScanStore()
    await scan.startScan(7)
    await scan.startScan(8)
    return { scan, channels }
  }

  it('首条进度立即刷,其余依 2s 门限;多根首批不各自绕限形成洪峰', async () => {
    // 用受控时钟替代 fake timers:门限判定只看 Date.now 差值。
    let now = 0
    const dateNow = vi.spyOn(Date, 'now').mockImplementation(() => now)
    try {
      const { channels } = await startTwoRoots()

      channels[0]?.onmessage?.(progressMsg(7, 'test-run'))
      // 首条进度立即刷新(初值 -Infinity,时钟从 0 起步也不被门限吞掉)。
      expect(loadStats).toHaveBeenCalledTimes(1)

      now = 1999
      channels[1]?.onmessage?.(progressMsg(8, 'test-run-2'))
      // 另一根的首批进度走的是同一全局门限,不再各自计时刷新。
      expect(loadStats).toHaveBeenCalledTimes(1)

      now = 2000
      channels[0]?.onmessage?.(progressMsg(7, 'test-run'))
      expect(loadStats).toHaveBeenCalledTimes(2)
    } finally {
      dateNow.mockRestore()
    }
  })

  it('快速入库完成/富化完成等有效终态强制刷新,不受门限压制', async () => {
    const { channels } = await startTwoRoots()
    channels[0]?.onmessage?.(progressMsg(7, 'test-run'))
    expect(loadStats).toHaveBeenCalledTimes(1)

    // 终态紧跟在进度之后(远不到 2s 门限)仍必须刷新:统计要立刻反映本轮最终计数。
    channels[0]?.onmessage?.({
      type: 'completed',
      rootId: 7,
      runId: 'test-run',
      totalItems: 2,
      totalBytes: 20,
      elapsedMs: 1,
      markedMissing: 0,
    })
    expect(loadStats).toHaveBeenCalledTimes(2)

    eventListeners.get(EVENTS.ENRICHMENT_COMPLETED)?.({ payload: { rootId: 7, runId: 'test-run' } })
    expect(loadStats).toHaveBeenCalledTimes(3)
  })

  it('停止与扫描错误同样强制刷新;统计取数失败只记日志,不冒未处理拒绝', async () => {
    const { scan, channels } = await startTwoRoots()
    channels[0]?.onmessage?.(progressMsg(7, 'test-run'))
    expect(loadStats).toHaveBeenCalledTimes(1)

    await scan.stopScan(7)
    expect(loadStats).toHaveBeenCalledTimes(2)

    loadStats.mockRejectedValueOnce(new Error('stats down'))
    channels[1]?.onmessage?.({
      type: 'error',
      rootId: 8,
      runId: 'test-run-2',
      error: 'boom',
    })
    expect(loadStats).toHaveBeenCalledTimes(3)
    // 让失败 promise 的 catch 回调跑完:失败只进日志(errorCode 之外无用户可见副作用)。
    await vi.waitFor(() => expect(logger.error).toHaveBeenCalled())
  })

  it('clearDatabase 开始与成功后各作废一次统计取数,失败只作废开始时那次', async () => {
    const clearResult = deferred<unknown>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.CLEAR_DATABASE) return clearResult.promise
      return Promise.resolve(undefined)
    })
    const scan = useScanStore()

    const clear = scan.clearDatabase()
    // 开始处即作废:清库等待期间其它 UI 仍可 loadStats,屏障管不到 mediaStore。
    expect(invalidateStats).toHaveBeenCalledTimes(1)

    clearResult.resolve(undefined)
    await clear
    // 成功返回后再作废一次(在释放屏障前),清库窗口内发起的查询也不得回写。
    expect(invalidateStats).toHaveBeenCalledTimes(2)

    // 旧根事件在清库后不再触发统计刷新。
    loadStats.mockClear()
    eventListeners.get(EVENTS.MEDIA_ENRICHED)?.({
      payload: { rootId: 7, enrichedCount: 1, total: 1 },
    })
    eventListeners.get(EVENTS.ENRICHMENT_COMPLETED)?.({ payload: { rootId: 7, runId: 'test-run' } })
    expect(loadStats).not.toHaveBeenCalled()
  })

  it('clearDatabase 失败时不再作废(库未变,快照仍有效)', async () => {
    const clearResult = deferred<unknown>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.CLEAR_DATABASE) return clearResult.promise
      return Promise.resolve(undefined)
    })
    const scan = useScanStore()

    const clear = scan.clearDatabase()
    expect(invalidateStats).toHaveBeenCalledTimes(1)
    clearResult.reject(new Error('clear failed'))
    await expect(clear).rejects.toThrow('clear failed')
    expect(invalidateStats).toHaveBeenCalledTimes(1)
  })
})

describe('scanStore 富化事件幽灵运行态防线(扫描完状态栏仍「正在扫描」修复)', () => {
  it('rootId=0 画廊刷新哨兵与未在跑根的富化事件一律忽略,不造幽灵运行态', async () => {
    const scan = useScanStore()
    await scan.startScan(7) // 注册富化监听并置根 7 运行态

    const emitEnriched = eventListeners.get(EVENTS.MEDIA_ENRICHED)!
    // 派生流水线封面落地复用本事件发画廊刷新哨兵(rootId=0),不是富化进度。
    emitEnriched({
      payload: {
        rootId: 0,
        enrichedCount: 1,
        total: 9,
        processedBytes: 10,
        totalBytes: 90,
      },
    })
    expect(scan.progressMap[0]).toBeUndefined()
    // 未知/未在跑的根照旧忽略。
    emitEnriched({
      payload: {
        rootId: 5,
        enrichedCount: 2,
        total: 9,
        processedBytes: 20,
        totalBytes: 90,
      },
    })
    expect(scan.progressMap[5]).toBeUndefined()

    // 在跑的根正常推进——防线不得过度拦截真实进度。
    emitEnriched({
      payload: {
        rootId: 7,
        runId: 'test-run',
        enrichedCount: 3,
        total: 9,
        processedBytes: 30,
        totalBytes: 90,
      },
    })
    expect(scan.progressMap[7]).toMatchObject({
      scanned: 3,
      total: 9,
      isRunning: true,
      status: 'enriching',
    })
  })

  it('enrichment:completed 终态后,迟到的富化进度不复活运行态', async () => {
    const scan = useScanStore()
    await scan.startScan(7)
    const emitEnriched = eventListeners.get(EVENTS.MEDIA_ENRICHED)!
    const emitCompleted = eventListeners.get(EVENTS.ENRICHMENT_COMPLETED)!
    emitCompleted({ payload: { rootId: 7, runId: 'test-run' } })
    expect(scan.progressMap[7]?.isRunning).toBe(false)

    emitEnriched({
      payload: {
        rootId: 7,
        runId: 'test-run',
        enrichedCount: 4,
        total: 9,
        processedBytes: 40,
        totalBytes: 90,
      },
    })
    expect(scan.progressMap[7]?.isRunning).toBe(false)
    expect(scan.isAnyScanRunning).toBe(false)
  })
})

describe('scanStore 同根 stop/restart 迟到响应隔离', () => {
  it('stop 在 STOP IPC 返回前就封口事件，等待期间的迟到进度不得写回', async () => {
    const stopResult = deferred<unknown>()
    let channel: { onmessage?: (message: unknown) => void } | undefined
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        channel = (args as { onProgress: typeof channel })?.onProgress
        return Promise.resolve(undefined)
      }
      if (cmd === IPC.STOP_SCAN) return stopResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    await scan.startScan(7)
    const stop = scan.stopScan(7)

    expect(scan.progressMap[7]).toMatchObject({ runId: 'test-run', isRunning: false })
    channel?.onmessage?.({
      type: 'progress',
      rootId: 7,
      runId: 'test-run',
      scanned: 9,
      total: 10,
      processedBytes: 90,
      totalBytes: 100,
      currentDir: 'late',
      status: 'scanning',
    })
    eventListeners.get(EVENTS.MEDIA_ENRICHED)?.({
      payload: {
        rootId: 7,
        runId: 'test-run',
        enrichedCount: 9,
        total: 10,
        processedBytes: 90,
        totalBytes: 100,
      },
    })

    expect(scan.progressMap[7]).toMatchObject({
      runId: 'test-run',
      scanned: 0,
      isRunning: false,
      status: 'discovering',
    })
    stopResult.resolve(undefined)
    await stop
  })

  it('STOP IPC 失败时恢复本地运行态并重新接受后续进度', async () => {
    const stopResult = deferred<unknown>()
    let channel: { onmessage?: (message: unknown) => void } | undefined
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        channel = (args as { onProgress: typeof channel })?.onProgress
        return Promise.resolve(undefined)
      }
      if (cmd === IPC.STOP_SCAN) return stopResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    await scan.startScan(7)
    const stop = scan.stopScan(7)
    expect(scan.progressMap[7]?.isRunning).toBe(false)

    stopResult.reject(new Error('stop failed'))
    await expect(stop).rejects.toThrow('stop failed')
    expect(scan.progressMap[7]).toMatchObject({
      runId: 'test-run',
      isRunning: true,
      status: 'discovering',
    })

    channel?.onmessage?.({
      type: 'progress',
      rootId: 7,
      runId: 'test-run',
      scanned: 2,
      total: 5,
      processedBytes: 20,
      totalBytes: 50,
      currentDir: 'recovered',
      status: 'scanning',
    })
    expect(scan.progressMap[7]).toMatchObject({
      scanned: 2,
      total: 5,
      isRunning: true,
      status: 'scanning',
    })
  })

  it('enrichment:completed 先到时仍接收迟到 Channel completed 的基线和回调', async () => {
    const onComplete = vi.fn()
    let channel: { onmessage?: (message: unknown) => void } | undefined
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        channel = (args as { onProgress: typeof channel })?.onProgress
      }
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    await scan.startScan(7, onComplete)
    eventListeners.get(EVENTS.ENRICHMENT_COMPLETED)?.({
      payload: { rootId: 7, runId: 'test-run' },
    })
    expect(scan.progressMap[7]?.isRunning).toBe(false)

    channel?.onmessage?.({
      type: 'completed',
      rootId: 7,
      runId: 'test-run',
      totalItems: 5,
      totalBytes: 50,
      elapsedMs: 1,
      markedMissing: 0,
    })
    expect(scan.progressMap[7]).toMatchObject({
      runId: 'test-run',
      isRunning: false,
      status: 'enriching',
      processedBytes: 50,
    })
    expect(onComplete).toHaveBeenCalledOnce()
  })

  it('markScanStopped 传入 expectedRunId 时只收尾对应运行轮次', () => {
    const scan = useScanStore()
    scan.progressMap[7] = {
      runId: 'old-run',
      scanned: 0,
      total: 0,
      processedBytes: 0,
      totalBytes: null,
      currentDir: '',
      isRunning: true,
      status: 'discovering',
    }

    scan.markScanStopped(7, 'different-run')
    expect(scan.progressMap[7]?.isRunning).toBe(true)
    scan.markScanStopped(7, 'old-run')
    expect(scan.progressMap[7]?.isRunning).toBe(false)

    scan.progressMap[7] = { ...scan.progressMap[7]!, runId: 'new-run', isRunning: true }
    scan.markScanStopped(7, 'old-run')
    expect(scan.progressMap[7]).toMatchObject({ runId: 'new-run', isRunning: true })
  })

  it('stop 完成后同 run 的迟到 Channel 进度与 completed 不得复活运行态', async () => {
    let channel: { onmessage?: (message: unknown) => void } | undefined
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) {
        channel = (args as { onProgress: typeof channel })?.onProgress
      }
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    await scan.startScan(7)
    await scan.stopScan(7)
    expect(scan.progressMap[7]?.isRunning).toBe(false)

    channel?.onmessage?.({
      type: 'progress',
      rootId: 7,
      runId: 'test-run',
      scanned: 9,
      total: 10,
      processedBytes: 90,
      totalBytes: 100,
      currentDir: 'late',
      status: 'scanning',
    })
    channel?.onmessage?.({
      type: 'completed',
      rootId: 7,
      runId: 'test-run',
      totalItems: 10,
      totalBytes: 100,
      elapsedMs: 1,
      markedMissing: 0,
    })

    expect(scan.progressMap[7]?.isRunning).toBe(false)
    expect(scan.progressMap[7]?.status).toBe('discovering')
  })

  it('startScan 与立即 stopScan 不丢失待启动的本地运行态', async () => {
    const startResult = deferred<unknown>()
    let stopArgs: unknown
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.START_SCAN) return startResult.promise
      if (cmd === IPC.STOP_SCAN) {
        stopArgs = args
        return Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    const start = scan.startScan(7)
    const stop = scan.stopScan(7)
    await stop
    expect(stopArgs).toEqual({ rootId: 7, runId: 'test-run' })
    expect(scan.progressMap[7]?.isRunning).toBe(false)
    startResult.resolve(undefined)
    await start
  })

  it('旧 stop 返回不清理已经安装的新运行态', async () => {
    const stopResult = deferred<unknown>()
    let stopArgs: unknown
    invokeIpc.mockImplementation((cmd: unknown, args?: unknown) => {
      if (cmd === IPC.STOP_SCAN) {
        stopArgs = args
        return stopResult.promise
      }
      return Promise.resolve(undefined)
    })
    const scan = useScanStore()
    scan.progressMap[7] = {
      runId: 'old-run',
      scanned: 1,
      total: 2,
      processedBytes: 10,
      totalBytes: 20,
      currentDir: 'old',
      isRunning: true,
      status: 'scanning',
    }

    const oldStop = scan.stopScan(7)
    scan.progressMap[7] = {
      ...scan.progressMap[7],
      runId: 'new-run',
      isRunning: true,
    }
    stopResult.resolve(undefined)
    await oldStop

    expect(stopArgs).toEqual({ rootId: 7, runId: 'old-run' })
    expect(scan.progressMap[7]).toMatchObject({ runId: 'new-run', isRunning: true })
  })

  it('旧 remove 响应不清理已经安装的新运行态', async () => {
    const removeResult = deferred<unknown>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.REMOVE_SCAN_ROOT) return removeResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    scan.scanRoots = [root(7, false)]
    scan.progressMap[7] = {
      runId: 'old-run',
      scanned: 1,
      total: 2,
      processedBytes: 10,
      totalBytes: 20,
      currentDir: 'old',
      isRunning: true,
      status: 'scanning',
    }

    const oldRemove = scan.removeScanRoot(7)
    scan.progressMap[7] = {
      ...scan.progressMap[7],
      runId: 'new-run',
      isRunning: true,
    }
    removeResult.resolve(undefined)
    await oldRemove

    expect(scan.scanRoots).toEqual([root(7, false)])
    expect(scan.progressMap[7]).toMatchObject({ runId: 'new-run', isRunning: true })
  })

  it('旧带缩略图清理的根删除响应不清理已经安装的新运行态', async () => {
    const removeResult = deferred<{ cleared_count: number }>()
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.REMOVE_SCAN_ROOT_WITH_OPTIONS) return removeResult.promise
      return Promise.resolve(undefined)
    })

    const scan = useScanStore()
    scan.scanRoots = [root(7, false)]
    scan.progressMap[7] = {
      runId: 'old-run',
      scanned: 1,
      total: 2,
      processedBytes: 10,
      totalBytes: 20,
      currentDir: 'old',
      isRunning: true,
      status: 'scanning',
    }

    const oldRemove = scan.removeScanRoot(7, true)
    scan.progressMap[7] = {
      ...scan.progressMap[7],
      runId: 'new-run',
      isRunning: true,
    }
    removeResult.resolve({ cleared_count: 0 })
    await oldRemove

    expect(scan.scanRoots).toEqual([root(7, false)])
    expect(scan.progressMap[7]).toMatchObject({ runId: 'new-run', isRunning: true })
  })

  it('旧 startScan 异常不清理已经安装的新运行态', async () => {
    const oldStart = deferred<unknown>()
    let startCount = 0
    invokeIpc.mockImplementation((cmd: unknown) => {
      if (cmd === IPC.START_SCAN) {
        startCount += 1
        return startCount === 1 ? oldStart.promise : Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const scan = useScanStore()
    const oldRun = scan.startScan(7)
    await vi.waitFor(() => expect(startCount).toBe(1))

    await scan.startScan(7)
    expect(scan.progressMap[7]?.runId).toBe('test-run-2')
    oldStart.reject(new Error('stale start failed'))
    await expect(oldRun).rejects.toThrow('stale start failed')

    expect(scan.progressMap[7]).toMatchObject({ runId: 'test-run-2', isRunning: true })
  })
})

describe('scanStore 缩略图生成进度恢复(2026-07-18 刷新丢进度根治)', () => {
  it('快照 running → 回填运行态(无副作用);随后 completed 事件 → 终态 + 布局失效', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.FULL_THUMB_GEN_STATUS
          ? { generated: 42, total: 100, status: 'running', phase: 'GPU' }
          : undefined,
      ),
    )
    const scan = useScanStore()
    await scan.restoreThumbGenProgress()

    expect(scan.thumbGenProgress.isRunning).toBe(true)
    expect(scan.thumbGenProgress.generated).toBe(42)
    expect(scan.thumbGenProgress.total).toBe(100)
    expect(invalidateLayout).not.toHaveBeenCalled()

    // 事件流接续(监听已在 restore 时注册):completed 到达 → 终态并触发布局失效(取新 thumb_path)。
    eventListeners.get(EVENTS.THUMB_GEN_PROGRESS)?.({
      payload: { generated: 100, total: 100, status: 'completed' },
    })
    expect(scan.thumbGenProgress.isRunning).toBe(false)
    expect(scan.thumbGenProgress.status).toBe('completed')
    expect(invalidateLayout).toHaveBeenCalledOnce()
  })

  it('快照 completed(非运行)→ 只恢复显示,不触发布局失效', async () => {
    invokeIpc.mockImplementation((cmd: unknown) =>
      Promise.resolve(
        cmd === IPC.FULL_THUMB_GEN_STATUS
          ? { generated: 7, total: 7, status: 'completed' }
          : undefined,
      ),
    )
    const scan = useScanStore()
    await scan.restoreThumbGenProgress()

    expect(scan.thumbGenProgress.status).toBe('completed')
    expect(scan.thumbGenProgress.isRunning).toBe(false)
    expect(invalidateLayout).not.toHaveBeenCalled()
  })
})
