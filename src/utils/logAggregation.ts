// src/utils/logAggregation.ts
// 错误聚合(日志能力重构 S5 P1,方案 §5「按 error.code 分组计数+首末时间+样本展开」)。
// 复用 AppError 稳定 code 契约(error.rs Serialize 实现的 attributes['error.code']),比文本模板
// 挖掘便宜且精确——裸文本 warn/error(无 error.code)不参与聚合,轻量模板启发式非本期范围。
import type { LogEntry } from '../types/logEntry'

export interface ErrorAggregateRow {
  code: string
  count: number
  firstTs: string
  lastTs: string
  sampleMsg: string
}

/**
 * 按 `attributes['error.code']` 分组统计。firstTs/lastTs 按时间戳字符串比较算出(RFC3339 固定
 * 宽度、同一会话固定时区偏移,字典序即时间序),**不假设输入按时间正序**——调用方常把
 * liveEntries 与 historyEntries 拼接喂入,History 标签打开的若正是当天仍在被实时写入的同一份
 * 文件,两个数组会在时间上重叠甚至倒挂(reviewer 深审 2026-07-20 修复:早期实现按"遇见顺序"
 * 直接赋值 lastTs,在此场景下会把 first/last 算反)。sampleMsg 仍取遇见顺序的首条(只是一个
 * 代表性样本,不要求严格对应时间最早)。结果按 count 降序(最吵的错误在前)。
 */
export function aggregateErrorEntries(entries: readonly LogEntry[]): ErrorAggregateRow[] {
  const byCode = new Map<string, ErrorAggregateRow>()
  for (const e of entries) {
    const code = e.attributes?.['error.code']
    if (typeof code !== 'string' || !code) continue
    const row = byCode.get(code)
    if (row) {
      row.count += 1
      if (e.ts < row.firstTs) row.firstTs = e.ts
      if (e.ts > row.lastTs) row.lastTs = e.ts
    } else {
      byCode.set(code, { code, count: 1, firstTs: e.ts, lastTs: e.ts, sampleMsg: e.msg })
    }
  }
  return Array.from(byCode.values()).sort((a, b) => b.count - a.count)
}

// span 耗时聚合(span 埋点线 W2,方案 docs/worklogs/2026-07-21-span埋点与worker日志汇入 §「span
// 事件契约」)。判据:`attributes.span_name` 为非空字符串 且 `attributes.duration_ms` 为(有限)
// 数字——target 只是辅助信号,不作硬判据(后端 D-311/D-312 已固定 target=scrollery::span,但前端
// 不强依赖它,防止未来出现同形状事件挂了别的 target)。

export interface SpanAggregateRow {
  name: string
  count: number
  totalMs: number
  avgMs: number
  maxMs: number
  lastTs: string
}

/** Top N 截断(方案:「totalMs 降序取前 20」)。 */
const SPAN_AGGREGATE_TOP_N = 20

/**
 * 按 `attributes.span_name` 分组统计 duration_ms:次数/总耗时/均值/最大值/最近时间戳。
 * lastTs 与 `aggregateErrorEntries` 同一姿态——按时间戳字符串比较取 max(RFC3339 固定宽度、
 * 同一会话固定时区偏移,字典序即时间序),**不假设输入按时间正序**(liveEntries 与
 * historyEntries 拼接后可能时间倒挂,S5 reviewer 深审修复过的同一类坑,这里从一开始就按
 * 正确姿态实现,不留「遇见顺序」的隐患)。结果按 totalMs 降序,取前
 * [`SPAN_AGGREGATE_TOP_N`]——总耗时最大的 span 排前面,比单纯次数更能反映"哪里最值得看"。
 */
export function aggregateSpanDurations(entries: readonly LogEntry[]): SpanAggregateRow[] {
  const byName = new Map<
    string,
    { count: number; totalMs: number; maxMs: number; lastTs: string }
  >()
  for (const e of entries) {
    const name = e.attributes?.['span_name']
    const duration = e.attributes?.['duration_ms']
    if (typeof name !== 'string' || !name) continue
    if (typeof duration !== 'number' || !Number.isFinite(duration)) continue
    const row = byName.get(name)
    if (row) {
      row.count += 1
      row.totalMs += duration
      if (duration > row.maxMs) row.maxMs = duration
      if (e.ts > row.lastTs) row.lastTs = e.ts
    } else {
      byName.set(name, { count: 1, totalMs: duration, maxMs: duration, lastTs: e.ts })
    }
  }
  return Array.from(byName.entries())
    .map(([name, r]) => ({
      name,
      count: r.count,
      totalMs: r.totalMs,
      avgMs: r.totalMs / r.count,
      maxMs: r.maxMs,
      lastTs: r.lastTs,
    }))
    .sort((a, b) => b.totalMs - a.totalMs)
    .slice(0, SPAN_AGGREGATE_TOP_N)
}
