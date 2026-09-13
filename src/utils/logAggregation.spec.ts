// logAggregation 纯函数单测(日志能力重构 S5 P1):按 error.code 分组计数 + 首末时间 + 样本。
// span 耗时聚合(span 埋点线 W2)见文末 aggregateSpanDurations 用例组。
import { describe, it, expect } from 'vitest'
import { aggregateErrorEntries, aggregateSpanDurations } from './logAggregation'
import type { LogEntry } from '../types/logEntry'

let seq = 0
function entry(overrides: Partial<LogEntry> = {}): LogEntry {
  return {
    ts: '2026-07-20T21:00:00.000+08:00',
    level: 'ERROR',
    target: 't',
    session_id: 's',
    operation_id: null,
    msg: 'boom',
    attributes: {},
    _seq: seq++,
    ...overrides,
  }
}

describe('aggregateErrorEntries（S5 P1 错误聚合）', () => {
  it('无 error.code 的行不参与聚合(裸文本 warn/error)', () => {
    const rows = aggregateErrorEntries([entry({ attributes: {} })])
    expect(rows).toEqual([])
  })

  it('非字符串/空字符串 error.code 同样跳过', () => {
    const rows = aggregateErrorEntries([
      entry({ attributes: { 'error.code': 42 } }),
      entry({ attributes: { 'error.code': '' } }),
    ])
    expect(rows).toEqual([])
  })

  it('同一 code 多条聚合为一行:count 累加、firstTs/lastTs 取遇见顺序首末、样本取首条', () => {
    const rows = aggregateErrorEntries([
      entry({ ts: '2026-07-20T21:00:00.000+08:00', attributes: { 'error.code': 'Io' }, msg: 'first io error' }),
      entry({ ts: '2026-07-20T21:05:00.000+08:00', attributes: { 'error.code': 'Io' }, msg: 'second io error' }),
      entry({ ts: '2026-07-20T21:10:00.000+08:00', attributes: { 'error.code': 'Io' }, msg: 'third io error' }),
    ])
    expect(rows).toEqual([
      {
        code: 'Io',
        count: 3,
        firstTs: '2026-07-20T21:00:00.000+08:00',
        lastTs: '2026-07-20T21:10:00.000+08:00',
        sampleMsg: 'first io error',
      },
    ])
  })

  it('多个不同 code 按 count 降序排列', () => {
    const rows = aggregateErrorEntries([
      entry({ attributes: { 'error.code': 'Db' } }),
      entry({ attributes: { 'error.code': 'Io' } }),
      entry({ attributes: { 'error.code': 'Io' } }),
      entry({ attributes: { 'error.code': 'Io' } }),
    ])
    expect(rows.map((r) => r.code)).toEqual(['Io', 'Db'])
    expect(rows[0].count).toBe(3)
    expect(rows[1].count).toBe(1)
  })

  it('空输入返回空数组', () => {
    expect(aggregateErrorEntries([])).toEqual([])
  })

  it('乱序输入(如 liveEntries 与 historyEntries 拼接后时间倒挂)仍按时间戳大小算出正确的 first/lastTs', () => {
    const rows = aggregateErrorEntries([
      // 遇见顺序:先"更晚"的,再"更早"的——模拟 live(近期)拼在 history(更早)之前。
      entry({ ts: '2026-07-20T21:10:00.000+08:00', attributes: { 'error.code': 'Io' } }),
      entry({ ts: '2026-07-20T21:00:00.000+08:00', attributes: { 'error.code': 'Io' } }),
      entry({ ts: '2026-07-20T21:05:00.000+08:00', attributes: { 'error.code': 'Io' } }),
    ])
    expect(rows[0].firstTs).toBe('2026-07-20T21:00:00.000+08:00')
    expect(rows[0].lastTs).toBe('2026-07-20T21:10:00.000+08:00')
  })
})

describe('aggregateSpanDurations（span 埋点线 W2 耗时聚合）', () => {
  it('按 span_name 分组:次数累加、总耗时累加、均值/最大值正确', () => {
    const rows = aggregateSpanDurations([
      entry({ attributes: { span_name: 'pipeline:ai', duration_ms: 100 } }),
      entry({ attributes: { span_name: 'pipeline:ai', duration_ms: 300 } }),
      entry({ attributes: { span_name: 'pipeline:ai', duration_ms: 200 } }),
    ])
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({
      name: 'pipeline:ai',
      count: 3,
      totalMs: 600,
      avgMs: 200,
      maxMs: 300,
    })
  })

  it('多个不同 span_name 按 totalMs 降序排列(非按次数)', () => {
    const rows = aggregateSpanDurations([
      // ipc:search_media: 2 次但总耗时更大。
      entry({ attributes: { span_name: 'ipc:search_media', duration_ms: 900 } }),
      entry({ attributes: { span_name: 'ipc:search_media', duration_ms: 900 } }),
      // pipeline:derive: 5 次但总耗时更小——验证排序键是 totalMs 不是 count。
      entry({ attributes: { span_name: 'pipeline:derive', duration_ms: 10 } }),
      entry({ attributes: { span_name: 'pipeline:derive', duration_ms: 10 } }),
      entry({ attributes: { span_name: 'pipeline:derive', duration_ms: 10 } }),
      entry({ attributes: { span_name: 'pipeline:derive', duration_ms: 10 } }),
      entry({ attributes: { span_name: 'pipeline:derive', duration_ms: 10 } }),
    ])
    expect(rows.map((r) => r.name)).toEqual(['ipc:search_media', 'pipeline:derive'])
    expect(rows[0].totalMs).toBe(1800)
    expect(rows[1].totalMs).toBe(50)
  })

  it('lastTs 在乱序输入下仍按时间戳大小算出正确的最大值(不是遇见顺序的最后一条)', () => {
    const rows = aggregateSpanDurations([
      // 遇见顺序:先"更晚"的,再"更早"的——模拟 live(近期)拼在 history(更早)之前。
      entry({ ts: '2026-07-20T21:10:00.000+08:00', attributes: { span_name: 'ipc:save_version', duration_ms: 5 } }),
      entry({ ts: '2026-07-20T21:00:00.000+08:00', attributes: { span_name: 'ipc:save_version', duration_ms: 5 } }),
      entry({ ts: '2026-07-20T21:05:00.000+08:00', attributes: { span_name: 'ipc:save_version', duration_ms: 5 } }),
    ])
    expect(rows[0].lastTs).toBe('2026-07-20T21:10:00.000+08:00')
  })

  it('非 span 条目(缺 span_name/duration_ms)被忽略', () => {
    const rows = aggregateSpanDurations([
      entry({ attributes: {} }),
      entry({ attributes: { 'error.code': 'Io' } }),
      entry({ attributes: { span_name: 'ipc:foo' } }), // 缺 duration_ms
      entry({ attributes: { duration_ms: 42 } }), // 缺 span_name
    ])
    expect(rows).toEqual([])
  })

  it('duration_ms 非数字(字符串/NaN/Infinity)时该行被忽略', () => {
    const rows = aggregateSpanDurations([
      entry({ attributes: { span_name: 'ipc:foo', duration_ms: '100' } }),
      entry({ attributes: { span_name: 'ipc:foo', duration_ms: Number.NaN } }),
      entry({ attributes: { span_name: 'ipc:foo', duration_ms: Number.POSITIVE_INFINITY } }),
      entry({ attributes: { span_name: 'ipc:foo', duration_ms: 50 } }),
    ])
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ name: 'ipc:foo', count: 1, totalMs: 50 })
  })

  it('span_name 非字符串或空字符串时该行被忽略', () => {
    const rows = aggregateSpanDurations([
      entry({ attributes: { span_name: 42, duration_ms: 10 } }),
      entry({ attributes: { span_name: '', duration_ms: 10 } }),
    ])
    expect(rows).toEqual([])
  })

  it('超过 20 个不同 span_name 时按 totalMs 降序只取前 20', () => {
    const entries: LogEntry[] = []
    for (let i = 0; i < 25; i++) {
      // totalMs 与索引反相关,故 span-0 总耗时最大、span-24 最小——最小的 5 个应被截掉。
      entries.push(entry({ attributes: { span_name: `ipc:span-${i}`, duration_ms: 1000 - i } }))
    }
    const rows = aggregateSpanDurations(entries)
    expect(rows).toHaveLength(20)
    expect(rows[0].name).toBe('ipc:span-0')
    expect(rows.map((r) => r.name)).not.toContain('ipc:span-24')
    expect(rows.map((r) => r.name)).not.toContain('ipc:span-20')
  })

  it('空输入返回空数组', () => {
    expect(aggregateSpanDurations([])).toEqual([])
  })
})
