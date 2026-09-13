// src/utils/logger.spec.ts
// 前端日志桥核心行为(日志能力重构 S3,方案 §4/§9.3-E):enqueue→flush 契约、阈值自动 flush、
// off 档联动、全局 onerror/unhandledrejection 兜底。
//
// 两处测试基建取舍:
// ① IPC 层经 vi.mock 替身,不触真实 Tauri invoke(同 backupStore.spec.ts 惯例)。
// ② 项目测试环境是纯 node(vitest.config.ts 明文:「无需 DOM → 默认 node 环境即可」,未装
//    jsdom/happy-dom)。`window` 用 vi.stubGlobal 打最小桩(仅 addEventListener),不新增 DOM 依赖。
// ③ logger.ts 是模块级单例(内部 queue/flushTimer 状态无导出 setter),每个 it() 前
//    vi.resetModules() + 动态 re-import,换一份全新模块实例,防止上一个用例的 setInterval 状态
//    跨用例污染(尤其是切换 fake/real timers 时,旧 interval 绑定的是已丢弃的 fake clock)。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { IPC } from '../constants/ipc'

const invokeIpc = vi.fn((..._args: unknown[]) => Promise.resolve<unknown>(undefined))
vi.mock('./ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
}))

type LoggerModule = typeof import('./logger')

const windowListeners = new Map<string, (event: unknown) => void>()

let mod!: LoggerModule

beforeEach(async () => {
  vi.useFakeTimers()
  invokeIpc.mockReset()
  invokeIpc.mockImplementation(() => Promise.resolve(undefined))
  for (const level of ['debug', 'info', 'warn', 'error'] as const) {
    vi.spyOn(console, level).mockImplementation(() => {})
  }
  windowListeners.clear()
  vi.stubGlobal('window', {
    addEventListener: (name: string, cb: (event: unknown) => void) => {
      windowListeners.set(name, cb)
    },
  })

  vi.resetModules()
  mod = await import('./logger')
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('logger 入队与 flush', () => {
  it('flush 前不调 IPC；flush 后一次性送出全部入队事件并清空队列', async () => {
    mod.logger.info('a')
    mod.logger.warn('b', { x: 1 })
    expect(invokeIpc).not.toHaveBeenCalled()

    await mod.flush()

    expect(invokeIpc).toHaveBeenCalledTimes(1)
    const [cmd, args] = invokeIpc.mock.calls[0] as [string, { events: Array<{ msg: string }> }]
    expect(cmd).toBe(IPC.LOG_FRONTEND_EVENTS)
    expect(args.events).toHaveLength(2)
    expect(args.events[0].msg).toBe('a')
    expect(args.events[1].msg).toBe('b')
  })

  it('再次 flush 空队列是 no-op，不重复发送', async () => {
    mod.logger.info('a')
    await mod.flush()
    await mod.flush()
    expect(invokeIpc).toHaveBeenCalledTimes(1)
  })

  it('并发 flush 不重复发送同一批（先原子取走队列再 await）', async () => {
    mod.logger.info('a')
    const first = mod.flush()
    const second = mod.flush()
    await Promise.all([first, second])
    expect(invokeIpc).toHaveBeenCalledTimes(1)
  })

  it('达到 50 条阈值自动 flush，不等 2s 定时器', () => {
    for (let i = 0; i < 50; i++) mod.logger.debug(`event-${i}`)
    // enqueue 内触发 flush() 时同步跑到首个 await 之前 —— invokeIpc 的调用已被同步记录,无需等待。
    expect(invokeIpc).toHaveBeenCalledTimes(1)
    const [, args] = invokeIpc.mock.calls[0] as [string, { events: unknown[] }]
    expect(args.events).toHaveLength(50)
  })

  it('2s 定时器到点自动 flush', async () => {
    mod.logger.error('boom')
    await vi.advanceTimersByTimeAsync(2000)
    expect(invokeIpc).toHaveBeenCalledTimes(1)
  })
})

describe('off 档联动', () => {
  it('setLoggerEnabled(false) 后新日志直接丢弃，不入队不占 flush', async () => {
    mod.setLoggerEnabled(false)
    mod.logger.error('should be dropped')
    await mod.flush()
    expect(invokeIpc).not.toHaveBeenCalled()
  })

  it('关闭时清空在途队列（不是等它自然过期）', async () => {
    mod.logger.info('pending')
    mod.setLoggerEnabled(false)
    await mod.flush()
    expect(invokeIpc).not.toHaveBeenCalled()
  })

  it('重新开启后恢复正常入队', async () => {
    mod.setLoggerEnabled(false)
    mod.setLoggerEnabled(true)
    mod.logger.info('resumed')
    await mod.flush()
    expect(invokeIpc).toHaveBeenCalledTimes(1)
  })
})

describe('installGlobalErrorHandlers', () => {
  it('window error 事件立即 flush，携带 source/url/line/col/stack', () => {
    mod.installGlobalErrorHandlers()
    const err = new Error('boom')
    windowListeners.get('error')?.({
      message: 'boom',
      filename: 'app://index.html',
      lineno: 12,
      colno: 3,
      error: err,
    })

    expect(invokeIpc).toHaveBeenCalledTimes(1)
    const [, args] = invokeIpc.mock.calls[0] as [
      string,
      {
        events: Array<{
          level: string
          source?: string
          url?: string
          line?: number
          col?: number
          stack?: string
        }>
      },
    ]
    expect(args.events[0].level).toBe('error')
    expect(args.events[0].source).toBe('window.onerror')
    expect(args.events[0].url).toBe('app://index.html')
    expect(args.events[0].line).toBe(12)
    expect(args.events[0].col).toBe(3)
    expect(args.events[0].stack).toBe(err.stack)
  })

  it('unhandledrejection 立即 flush，reason 为 Error 时取其 message', () => {
    mod.installGlobalErrorHandlers()
    windowListeners.get('unhandledrejection')?.({ reason: new Error('rejected') })

    expect(invokeIpc).toHaveBeenCalledTimes(1)
    const [, args] = invokeIpc.mock.calls[0] as [
      string,
      { events: Array<{ msg: string; source?: string }> },
    ]
    expect(args.events[0].msg).toBe('rejected')
    expect(args.events[0].source).toBe('unhandledrejection')
  })

  it('unhandledrejection 的 reason 非 Error 时退化为 String(reason)', () => {
    mod.installGlobalErrorHandlers()
    windowListeners.get('unhandledrejection')?.({ reason: 'plain string reason' })

    const [, args] = invokeIpc.mock.calls[0] as [string, { events: Array<{ msg: string }> }]
    expect(args.events[0].msg).toBe('plain string reason')
  })
})
