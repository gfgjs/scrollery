// src/utils/logger.ts
// 前端日志桥(日志能力重构方案 §4/§9.3-E,S3):替换 84 处裸 console.*,把前端日志汇入后端
// tracing——同一套 JSONL 文件/UI 环形缓冲/重复压缩治理,agent 与用户不用再分两处找日志。

import { invokeIpc } from './ipc'
import { IPC } from '../constants/ipc'

export type LogLevel = 'debug' | 'info' | 'warn' | 'error'

/** 与后端 `FrontendLogEvent`(system_commands.rs)字段一一对应,camelCase 经 Tauri 自动转 snake_case。 */
interface LogEvent {
  level: LogLevel
  msg: string
  operationId?: string
  /** 事件来源:组件/模块名,或 `window.onerror`/`unhandledrejection` 等全局兜底标识。 */
  source?: string
  url?: string
  line?: number
  col?: number
  stack?: string
  /** 自由结构化上下文(如 `{ itemId, rootId }`),落后端 `attributes.context`。 */
  fields?: Record<string, unknown>
}

// 触发阈值(方案 §9.3-E):2s 定时或 50 条阈值先到先 flush。
const FLUSH_INTERVAL_MS = 2000
const FLUSH_THRESHOLD = 50
// 防御性上限:IPC 持续失败(如后端卡死)时不让队列无界膨胀吃内存,保留最近的、丢最旧的
// ——与后端 non_blocking lossy 丢弃策略同一取舍(方案 §8 风险条目),仅无声丢弃不足为虑量级。
const MAX_QUEUE_SIZE = 1000

let queue: LogEvent[] = []
let flushTimer: ReturnType<typeof setInterval> | null = null
let flushing = false
// off 档默认按「未关闭」处理:configStore.loadConfig() 尚未回填前,极短窗口内的日志值得保留
// (方案 §9.3-E「启动时/切换时经事件同步开关态」——真正的 off 由 configStore 加载后同步过来)。
let enabled = true

function ensureFlushTimer(): void {
  if (flushTimer !== null) return
  flushTimer = setInterval(() => {
    void flush()
  }, FLUSH_INTERVAL_MS)
}

function enqueue(event: LogEvent): void {
  if (!enabled) return
  if (import.meta.env.DEV) {
    // dev 构建同步镜像 console(方案 §9.3-E),保留现有开发体验;生产仅入队,不白付 console 格式化成本。
    // 本文件在 eslint.config.js 的 app/no-console 规则里整体豁免(唯一合法的 console 桥接点)。
    console[event.level](`[${event.source ?? 'app'}]`, event.msg, event.fields ?? '')
  }
  queue.push(event)
  if (queue.length > MAX_QUEUE_SIZE) {
    queue = queue.slice(queue.length - MAX_QUEUE_SIZE)
  }
  ensureFlushTimer()
  if (queue.length >= FLUSH_THRESHOLD) void flush()
}

function log(level: LogLevel, msg: string, fields?: Record<string, unknown>): void {
  enqueue({ level, msg, fields })
}

/**
 * off 档联动(方案 §4):后端 logLevel=off 时前端队列直接丢弃,不白付 IPC。由 `configStore` 在
 * `loadConfig()` 回填与 `setLogLevel()` 切换时调用,是这里"是否记录"判断的唯一事实源。
 */
export function setLoggerEnabled(nextEnabled: boolean): void {
  enabled = nextEnabled
  if (!enabled) queue = []
}

/** 把队列中的事件一次性送后端;并发调用安全(先原子取走队列再 await,不会重复发送同一条)。 */
export async function flush(): Promise<void> {
  if (flushing || queue.length === 0) return
  flushing = true
  const events = queue
  queue = []
  try {
    await invokeIpc<void>(IPC.LOG_FRONTEND_EVENTS, { events })
  } catch (e) {
    if (import.meta.env.DEV) {
      // flush 自身失败时无法再走 logger(会递归入队),仅 dev 可见;本文件整体豁免 no-console。
      console.error('[logger] flush failed, events dropped:', e)
    }
  } finally {
    flushing = false
  }
}

export const logger = {
  debug: (msg: string, fields?: Record<string, unknown>) => log('debug', msg, fields),
  info: (msg: string, fields?: Record<string, unknown>) => log('info', msg, fields),
  warn: (msg: string, fields?: Record<string, unknown>) => log('warn', msg, fields),
  error: (msg: string, fields?: Record<string, unknown>) => log('error', msg, fields),
}

/**
 * 全局兜底(方案 §4/§9.4 S3 step 2):`window.onerror` + `unhandledrejection`,push 后立即 flush
 * (不等 2s/50 条批次)。由 `main.ts` 在应用挂载前调用一次;幂等(重复调用只是重复挂监听,
 * 调用方保证只调一次)。
 */
export function installGlobalErrorHandlers(): void {
  window.addEventListener('error', (event: ErrorEvent) => {
    enqueue({
      level: 'error',
      msg: event.message || 'window.onerror',
      source: 'window.onerror',
      url: event.filename,
      line: event.lineno,
      col: event.colno,
      stack: event.error instanceof Error ? event.error.stack : undefined,
    })
    void flush()
  })
  window.addEventListener('unhandledrejection', (event: PromiseRejectionEvent) => {
    const reason = event.reason as unknown
    enqueue({
      level: 'error',
      msg: reason instanceof Error ? reason.message : String(reason),
      source: 'unhandledrejection',
      stack: reason instanceof Error ? reason.stack : undefined,
    })
    void flush()
  })
}
