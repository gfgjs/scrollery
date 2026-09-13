// src/stores/logWindowStore.spec.ts
// 独立日志窗口 store 的核心逻辑(日志能力重构 S4,方案 §5):过滤(级别/target/文本)、
// Pause/Resume/Clear 分离、renderCap 裁剪、历史分页累积。
//
// 测试基建同 backupStore.spec.ts 惯例:mock `@tauri-apps/api/event` 的 `listen`,手动回放
// 捕获的回调模拟 log:batch 事件到达——不触真实 Tauri IPC/事件桥。

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { EVENTS } from '../constants/ipc'
import type { LogEntry } from '../types/logEntry'

const invokeIpc = vi.fn((..._args: unknown[]) => Promise.resolve<unknown>(undefined))
vi.mock('../utils/ipc', () => ({
  invokeIpc: (...args: unknown[]) => invokeIpc(...(args as [])),
}))

const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }))
const listeners = new Map<string, (event: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

import {
  useLogWindowStore,
  RENDER_CAP_MIN,
  RENDER_CAP_MAX,
  RENDER_CAP_DEFAULT,
} from './logWindowStore'

/** 覆盖式设置 renderCap:store.setRenderCap 会钳制到 [RENDER_CAP_MIN, RENDER_CAP_MAX](生产
 * 硬约束,方案 §5),故测试 FIFO 裁剪行为不能用「3」这类越界小值,直接改内部 ref 绕开钳制。 */
function forceRenderCap(store: ReturnType<typeof useLogWindowStore>, n: number) {
  store.renderCap = n
}

function envelope(over: Partial<LogEntry> = {}): Omit<LogEntry, '_seq'> {
  return {
    ts: '2026-07-20T21:00:00.000+08:00',
    level: 'info',
    target: 'scrollery::scanner',
    session_id: 's-test',
    operation_id: null,
    msg: 'hello',
    attributes: {},
    ...over,
  }
}

function emitBatch(entries: Array<Omit<LogEntry, '_seq'>>) {
  listeners.get(EVENTS.LOG_BATCH)?.({ payload: entries })
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeIpc.mockReset()
  invokeIpc.mockImplementation(() => Promise.resolve(undefined))
  listeners.clear()
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb)
    return Promise.resolve(() => {})
  })
})

describe('logWindowStore 实时流 + 过滤', () => {
  /// reviewer 深审 2026-07-20 修复的回归测试:settingsMap.ts 的 logLevel 提供 trace 选项,
  /// 若日志窗口的级别过滤集合漏了 trace,用户切到 trace 档后这些行会被永久过滤掉且无法勾选加回。
  it('trace 级别默认在勾选集合内(不被过滤规则挡住)', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ level: 'trace' })])

    expect(store.filteredLive).toHaveLength(1)
  })

  it('log:batch 事件追加进 liveEntries,信封字段原样保留', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ msg: 'first' }), envelope({ msg: 'second', level: 'warn' })])

    expect(store.liveEntries).toHaveLength(2)
    expect(store.liveEntries[0].msg).toBe('first')
    expect(store.liveEntries[1].level).toBe('warn')
  })

  it('级别过滤:取消勾选某级别后,filteredLive 排除该级别但 liveEntries 原样保留', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ level: 'debug' }), envelope({ level: 'error' })])

    store.toggleLevel('debug')

    expect(store.filteredLive).toHaveLength(1)
    expect(store.filteredLive[0].level).toBe('error')
    expect(store.liveEntries).toHaveLength(2)
  })

  it('target 前缀过滤:大小写不敏感,只匹配前缀', () => {
    const store = useLogWindowStore()
    emitBatch([
      envelope({ target: 'scrollery::pipeline::thumb' }),
      envelope({ target: 'scrollery::frontend' }),
    ])

    store.targetFilter = 'SCROLLERY::PIPELINE'

    expect(store.filteredLive).toHaveLength(1)
    expect(store.filteredLive[0].target).toBe('scrollery::pipeline::thumb')
  })

  it('文本子串过滤:大小写不敏感,匹配 msg', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ msg: 'Scan root AUTH failed' }), envelope({ msg: 'thumbnail generated' })])

    store.textFilter = 'auth'

    expect(store.filteredLive).toHaveLength(1)
    expect(store.filteredLive[0].msg).toBe('Scan root AUTH failed')
  })
})

describe('logWindowStore Pause/Resume/Clear', () => {
  it('暂停期间到达的批次不进入 liveEntries,计入 pendingCount', () => {
    const store = useLogWindowStore()
    store.setPaused(true)
    emitBatch([envelope({ msg: 'a' }), envelope({ msg: 'b' })])

    expect(store.liveEntries).toHaveLength(0)
    expect(store.pendingCount).toBe(2)
  })

  it('恢复时把暂停期间积压的条目按到达顺序一次性合入,并清零 pendingCount', () => {
    const store = useLogWindowStore()
    store.setPaused(true)
    emitBatch([envelope({ msg: 'a' })])
    emitBatch([envelope({ msg: 'b' })])

    store.setPaused(false)

    expect(store.liveEntries.map((e) => e.msg)).toEqual(['a', 'b'])
    expect(store.pendingCount).toBe(0)
  })

  /// reviewer 深审 2026-07-20 修复的回归测试:后端环形缓冲对"暂停"无感知、持续 ~100ms 一批
  /// 推送,若 pendingEntries 不裁剪,长时间暂停会让它无界增长(内存泄漏级问题)。
  it('暂停期间积压超过 renderCap 时按 FIFO 裁剪,只保留最新的一批(不会无界增长)', () => {
    const store = useLogWindowStore()
    forceRenderCap(store, RENDER_CAP_MIN)
    store.setPaused(true)

    const batch = Array.from({ length: RENDER_CAP_MIN + 10 }, (_, i) => envelope({ msg: `m${i}` }))
    emitBatch(batch)

    expect(store.pendingCount).toBe(RENDER_CAP_MIN)
    store.setPaused(false)
    expect(store.liveEntries).toHaveLength(RENDER_CAP_MIN)
    expect(store.liveEntries[0].msg).toBe('m10')
    expect(store.liveEntries[store.liveEntries.length - 1].msg).toBe(`m${RENDER_CAP_MIN + 9}`)
  })

  it('clearView 清空 liveEntries 与暂停期间的积压,不影响后端/文件', () => {
    const store = useLogWindowStore()
    emitBatch([envelope()])
    store.setPaused(true)
    emitBatch([envelope()])

    store.clearView()

    expect(store.liveEntries).toHaveLength(0)
    expect(store.pendingCount).toBe(0)
    // clearView 后恢复:不应把 clear 前的积压重新吐出来。
    store.setPaused(false)
    expect(store.liveEntries).toHaveLength(0)
  })
})

describe('logWindowStore renderCap', () => {
  it('默认上限生效前不裁剪;超过后按 FIFO 丢最旧,保留最新', () => {
    const store = useLogWindowStore()
    forceRenderCap(store, 3)
    emitBatch([
      envelope({ msg: '1' }),
      envelope({ msg: '2' }),
      envelope({ msg: '3' }),
      envelope({ msg: '4' }),
    ])

    expect(store.liveEntries.map((e) => e.msg)).toEqual(['2', '3', '4'])
  })

  it('setRenderCap 钳制到 [MIN, MAX] 区间', () => {
    const store = useLogWindowStore()
    store.setRenderCap(1)
    expect(store.renderCap).toBe(RENDER_CAP_MIN)
    store.setRenderCap(999_999)
    expect(store.renderCap).toBe(RENDER_CAP_MAX)
  })

  it('默认值符合方案 §5 MVP(10k)', () => {
    const store = useLogWindowStore()
    expect(store.renderCap).toBe(RENDER_CAP_DEFAULT)
  })
})

describe('logWindowStore 历史分页', () => {
  it('openHistoryFile 加载首页(beforeLine=null,取文件当前末尾),historyHasMoreOlder 由后端返回值驱动', async () => {
    invokeIpc.mockImplementation((cmd: unknown, args: unknown) => {
      const a = args as { beforeLine: number | null }
      if (a.beforeLine === null) {
        return Promise.resolve({
          lines: [envelope({ msg: 'latest' })],
          totalLines: 600,
          hasMoreOlder: true,
          oldestLoadedLine: 599,
        })
      }
      return Promise.resolve({ lines: [], totalLines: 600, hasMoreOlder: false, oldestLoadedLine: 0 })
    })

    const store = useLogWindowStore()
    await store.openHistoryFile('scrollery.2026-07-19.log')

    expect(store.selectedHistoryFile).toBe('scrollery.2026-07-19.log')
    expect(store.historyEntries.map((e) => e.msg)).toEqual(['latest'])
    expect(store.historyHasMoreOlder).toBe(true)
  })

  it('loadOlderHistory 把更早的一页前插到已加载条目之前,并把上一页的 oldestLoadedLine 原样带回下一次请求', async () => {
    invokeIpc.mockImplementation((cmd: unknown, args: unknown) => {
      const a = args as { beforeLine: number | null }
      if (a.beforeLine === null) {
        return Promise.resolve({
          lines: [envelope({ msg: 'newer' })],
          totalLines: 2,
          hasMoreOlder: true,
          oldestLoadedLine: 1,
        })
      }
      expect(a.beforeLine).toBe(1) // 必须原样带回上一页的 oldestLoadedLine,不是重新算的相对偏移
      return Promise.resolve({
        lines: [envelope({ msg: 'older' })],
        totalLines: 2,
        hasMoreOlder: false,
        oldestLoadedLine: 0,
      })
    })

    const store = useLogWindowStore()
    await store.openHistoryFile('scrollery.2026-07-19.log')
    await store.loadOlderHistory()

    expect(store.historyEntries.map((e) => e.msg)).toEqual(['older', 'newer'])
    expect(store.historyHasMoreOlder).toBe(false)
  })

  /// reviewer 深审 2026-07-20 修复的回归测试(store 层):即便被浏览的文件在两次分页调用之间
  /// 持续被写入(total_lines 增长),只要后端如实按锚点返回,store 侧靠"原样带回 oldestLoadedLine"
  /// 就不会自己再引入相对偏移的漂移——锚点稳定性本身由 Rust 侧 slice_page_bounds 单测锁定,这里
  /// 锁定的是 store 侧确实按契约传递锚点而非自作主张重新计算。
  it('文件持续增长期间,store 不会自行重算锚点(总行数变化不影响下一次 beforeLine 取值)', async () => {
    let call = 0
    invokeIpc.mockImplementation((cmd: unknown, args: unknown) => {
      call += 1
      const a = args as { beforeLine: number | null }
      if (call === 1) {
        expect(a.beforeLine).toBeNull()
        return Promise.resolve({
          lines: [envelope({ msg: 'p1' })],
          totalLines: 100, // 首次打开时文件共 100 行
          hasMoreOlder: true,
          oldestLoadedLine: 70,
        })
      }
      // 第二次调用时文件已涨到 105 行(被写入了新内容),但 beforeLine 必须仍是上一页给的 70。
      expect(a.beforeLine).toBe(70)
      return Promise.resolve({
        lines: [envelope({ msg: 'p2' })],
        totalLines: 105,
        hasMoreOlder: false,
        oldestLoadedLine: 40,
      })
    })

    const store = useLogWindowStore()
    await store.openHistoryFile('scrollery.2026-07-20.log')
    await store.loadOlderHistory()

    expect(store.historyEntries.map((e) => e.msg)).toEqual(['p2', 'p1'])
  })

  it('切换到另一天的历史文件时重置分页状态(不与前一天残留混杂)', async () => {
    invokeIpc.mockImplementation((cmd: unknown, args: unknown) => {
      const a = args as { fileName: string }
      return Promise.resolve({
        lines: [envelope({ msg: `from-${a.fileName}` })],
        totalLines: 1,
        hasMoreOlder: false,
        oldestLoadedLine: 0,
      })
    })

    const store = useLogWindowStore()
    await store.openHistoryFile('scrollery.2026-07-19.log')
    await store.openHistoryFile('scrollery.2026-07-20.log')

    expect(store.historyEntries).toHaveLength(1)
    expect(store.historyEntries[0].msg).toBe('from-scrollery.2026-07-20.log')
  })
})

describe('logWindowStore 正则搜索(S5 P1)', () => {
  it('substring 模式(默认)行为不变', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ msg: 'Scan root AUTH failed' }), envelope({ msg: 'thumbnail generated' })])
    store.textFilter = 'auth'

    expect(store.filteredLive.map((e) => e.msg)).toEqual(['Scan root AUTH failed'])
    expect(store.textFilterInvalid).toBe(false)
  })

  it('regex 模式:合法正则按 test() 匹配 msg,大小写不敏感', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ msg: 'item 42 failed' }), envelope({ msg: 'item abc failed' })])
    store.textFilterMode = 'regex'
    store.textFilter = 'item \\d+'

    expect(store.filteredLive.map((e) => e.msg)).toEqual(['item 42 failed'])
    expect(store.textFilterInvalid).toBe(false)
  })

  it('regex 模式:非法正则不清空视图(降级为不过滤),并置 textFilterInvalid', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ msg: 'a' }), envelope({ msg: 'b' })])
    store.textFilterMode = 'regex'
    store.textFilter = '(unclosed'

    expect(store.filteredLive).toHaveLength(2)
    expect(store.textFilterInvalid).toBe(true)
  })

  it('toggleTextFilterMode 在 substring/regex 间切换', () => {
    const store = useLogWindowStore()
    expect(store.textFilterMode).toBe('substring')
    store.toggleTextFilterMode()
    expect(store.textFilterMode).toBe('regex')
    store.toggleTextFilterMode()
    expect(store.textFilterMode).toBe('substring')
  })
})

describe('logWindowStore 过滤 preset(S5 P1)', () => {
  it('saveCurrentAsPreset 快照当前过滤态;applyPreset 还原', () => {
    const store = useLogWindowStore()
    store.toggleLevel('debug')
    store.targetFilter = 'scrollery::pipeline'
    store.textFilter = 'fail'
    store.textFilterMode = 'regex'

    store.saveCurrentAsPreset('管道失败')

    // 清空当前态,验证 applyPreset 能整体还原。
    store.toggleLevel('debug')
    store.targetFilter = ''
    store.textFilter = ''
    store.textFilterMode = 'substring'

    store.applyPreset('管道失败')

    expect(store.levelFilter.has('debug')).toBe(false)
    expect(store.targetFilter).toBe('scrollery::pipeline')
    expect(store.textFilter).toBe('fail')
    expect(store.textFilterMode).toBe('regex')
  })

  it('同名 preset 再次保存覆盖旧值,不重复堆积', () => {
    const store = useLogWindowStore()
    store.textFilter = 'v1'
    store.saveCurrentAsPreset('X')
    store.textFilter = 'v2'
    store.saveCurrentAsPreset('X')

    expect(store.presets.filter((p) => p.name === 'X')).toHaveLength(1)
    expect(store.presets.find((p) => p.name === 'X')?.text).toBe('v2')
  })

  it('deletePreset 移除指定 preset', () => {
    const store = useLogWindowStore()
    store.saveCurrentAsPreset('Y')
    expect(store.presets.some((p) => p.name === 'Y')).toBe(true)

    store.deletePreset('Y')
    expect(store.presets.some((p) => p.name === 'Y')).toBe(false)
  })

  it('空白 name 不保存', () => {
    const store = useLogWindowStore()
    const before = store.presets.length
    store.saveCurrentAsPreset('   ')
    expect(store.presets.length).toBe(before)
  })
})

describe('logWindowStore 诊断(S5 P1)', () => {
  it('refreshDiagnostics 调用 get_log_diagnostics 并写入 diagnostics', async () => {
    invokeIpc.mockResolvedValueOnce({
      droppedLines: 3,
      dedupActive: [{ target: 't', code: 'Io', swallowedInWindow: 5, windowAgeSecs: 10 }],
    })
    const store = useLogWindowStore()

    await store.refreshDiagnostics()

    expect(store.diagnostics?.droppedLines).toBe(3)
    expect(store.diagnostics?.dedupActive).toHaveLength(1)
  })
})

describe('logWindowStore 直方图(S5 P2)', () => {
  it('loadHistogram 传 fileName/bucket 并写入 histogram,loading 态正确翻转', async () => {
    let resolvePromise: (v: unknown) => void = () => {}
    invokeIpc.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolvePromise = resolve
        }),
    )
    const store = useLogWindowStore()

    const p = store.loadHistogram('scrollery.2026-07-20.log', 'hour')
    expect(store.histogramLoading).toBe(true)

    resolvePromise({ buckets: [{ bucket: '2026-07-20T21:00:00', level: 'INFO', count: 2 }], totalLines: 2, parsedLines: 2 })
    await p

    expect(store.histogramLoading).toBe(false)
    expect(store.histogram?.buckets).toHaveLength(1)
    expect(invokeIpc).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ fileName: 'scrollery.2026-07-20.log', bucket: 'hour' }),
    )
  })
})

describe('logWindowStore 诊断包导出(S5 P2)', () => {
  it('exportDiagnosticsPackage 透传后端返回值', async () => {
    invokeIpc.mockResolvedValueOnce({
      dir: 'C:/logs/diagnostics',
      zipPath: 'C:/logs/diagnostics/x.zip',
      sizeBytes: 1234,
      redactedMatches: 2,
    })
    const store = useLogWindowStore()

    const result = await store.exportDiagnosticsPackage()

    expect(result.zipPath).toBe('C:/logs/diagnostics/x.zip')
    expect(result.redactedMatches).toBe(2)
  })
})

describe('logWindowStore 双窗格上下文(S5 P2)', () => {
  it('selectEntry 选中后 contextView 从未过滤的 liveEntries 里取 ±半径上下文', () => {
    const store = useLogWindowStore()
    const batch = Array.from({ length: 40 }, (_, i) => envelope({ msg: `m${i}` }))
    emitBatch(batch)

    const targetSeq = store.liveEntries[20]._seq
    store.selectEntry(targetSeq)

    expect(store.contextView).not.toBeNull()
    expect(store.contextView?.entries[store.contextView.selectedIndex]._seq).toBe(targetSeq)
    expect(store.contextView?.entries.length).toBeLessThanOrEqual(31) // 2*15+1
  })

  it('再次 selectEntry 同一 seq 取消选中(contextView 变 null)', () => {
    const store = useLogWindowStore()
    emitBatch([envelope({ msg: 'a' })])
    const seq = store.liveEntries[0]._seq

    store.selectEntry(seq)
    expect(store.contextView).not.toBeNull()
    store.selectEntry(seq)
    expect(store.contextView).toBeNull()
  })

  it('未选中(selectedSeq=null)时 contextView 为 null', () => {
    const store = useLogWindowStore()
    expect(store.contextView).toBeNull()
  })

  it('在 historyEntries 里也能找到并展开上下文(不局限于 liveEntries)', async () => {
    invokeIpc.mockImplementation(() =>
      Promise.resolve({
        lines: Array.from({ length: 5 }, (_, i) => envelope({ msg: `h${i}` })),
        totalLines: 5,
        hasMoreOlder: false,
        oldestLoadedLine: 0,
      }),
    )
    const store = useLogWindowStore()
    await store.openHistoryFile('scrollery.2026-07-20.log')

    const seq = store.historyEntries[2]._seq
    store.selectEntry(seq)

    expect(store.contextView?.entries).toHaveLength(5)
    expect(store.contextView?.entries[store.contextView.selectedIndex]._seq).toBe(seq)
  })
})
