// scanStore 单测：根文件夹显隐（V21）。锁三件事：
//  1. visibleScanRoots —— 侧栏文件树的可见根源。隐藏根必须被滤掉、可见根必须留下。
//  2. setScanRootHidden —— 调对 IPC、本地 isHidden 同步翻转、并触发画廊重排 + 统计刷新
//     （漏 invalidateLayout 的症状是「设置页点了隐藏、画廊却不更新」）。
//  3. unhide 补跑 —— 取消隐藏才触发 AI/人脸 maybeAutoResume（隐藏时绝不触发）；缩略图靠
//     画廊 on-demand 路径补跑，故此处不校验缩略图触发。

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import type { ScanRoot } from '../types/media'
import { IPC } from '../constants/ipc'

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

describe('scanStore 根列表响应隔离', () => {

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
})

describe('scanStore 同根 stop/restart 迟到响应隔离', () => {

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
