// src/types/logEntry.ts
// 日志窗口的信封条目类型。
//
// 字段保留后端 JSONL 信封的 snake_case：`ts/level/target/session_id/
// operation_id/msg/attributes`。事件与历史页直接透传 JSONL 字段。
export interface LogEntry {
  ts: string
  level: string
  target: string
  session_id: string | null
  operation_id: string | null
  msg: string
  attributes: Record<string, unknown>
  /** 前端本地追加的稳定标识(单调递增),供虚拟列表 getItemKey 用——不进 JSONL、不来自后端。 */
  _seq: number
}

// 与 settingsMap.ts 的 logLevel 选项(trace/debug/info/warn/error/off)对齐——漏了 trace 会导致
// 用户把全局日志级别调到 trace 后,trace 行在日志窗口里被过滤规则永久挡住且无勾选框能加回
// (reviewer 深审 2026-07-20 修复)。
export const LOG_LEVELS = ['trace', 'debug', 'info', 'warn', 'error'] as const
export type LogLevelFilter = (typeof LOG_LEVELS)[number]
