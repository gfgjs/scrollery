// src/types/logEntry.ts
// 日志窗口(日志能力重构 S4,方案 §5)的信封条目类型。
//
// 字段刻意保持后端 JSONL 信封的原样 snake_case(方案 §3.3:`ts/level/target/session_id/
// operation_id/msg/attributes`),而非本仓其余 IPC 层惯用的 camelCase——因为这些字段来自
// `Vec<serde_json::Value>`(log:batch 事件负载 / read_log_file_page 历史页),不经过任何
// `#[serde(rename_all = "camelCase")]` struct,原样透传自 JSONL 文件的字面字段名(与 agent
// 直接 `rg`/`jq` 该文件时看到的键名一致,前后端两侧对同一份数据无需两套字段名心智负担)。
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
