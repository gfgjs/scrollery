// src/stores/logWindowStore.ts
// 独立日志窗口的状态(日志能力重构 S4,方案 §5/§9.4 S4):实时流(环形缓冲 + 过滤)+
// 历史(按日 JSONL 分页)。只在日志窗口(main.ts 按 window label 分流出的独立挂载路径)
// 内实例化——主窗口从不 use 本 store,不产生额外订阅/内存开销。

import { defineStore } from 'pinia'
import { ref, shallowRef, computed, watch } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { IPC, EVENTS } from '../constants/ipc'
import { useTauriListen } from '../composables/useTauriListen'
import { readSetting, writeSettings } from './settingsPersistence'
import { parseSettingJson } from '../composables/settingsValues'
import { LOG_LEVELS, type LogEntry, type LogLevelFilter } from '../types/logEntry'

/** 环形缓冲上限的默认值/可调区间(方案 §5 MVP「默认 10k 行,设置可调 5k-20k」)。
 * 这是**前端**本地保留行数——后端安全阀见 logging.rs::RING_BUFFER_CAPACITY(20k,不可调)。 */
export const RENDER_CAP_DEFAULT = 10_000
export const RENDER_CAP_MIN = 5_000
export const RENDER_CAP_MAX = 20_000

const HISTORY_PAGE_SIZE = 500

let seqCounter = 0
function tagEntries(raw: unknown[]): LogEntry[] {
  return raw.map((v) => ({ ...(v as Omit<LogEntry, '_seq'>), _seq: seqCounter++ }))
}

export const useLogWindowStore = defineStore('logWindow', () => {
  // ── 实时流 ──────────────────────────────────────────────────────────
  const liveEntries = shallowRef<LogEntry[]>([])
  const paused = ref(false)
  const pendingCount = ref(0)
  let pendingEntries: LogEntry[] = []
  const renderCap = ref(RENDER_CAP_DEFAULT)

  function appendLive(entries: LogEntry[]) {
    const merged = liveEntries.value.concat(entries)
    liveEntries.value =
      merged.length > renderCap.value ? merged.slice(merged.length - renderCap.value) : merged
  }

  function onBatch(raw: unknown) {
    if (!Array.isArray(raw) || raw.length === 0) return
    const entries = tagEntries(raw as unknown[])
    if (paused.value) {
      // 暂停期间后端环形缓冲对"暂停"无感知、持续 ~100ms 一批推送(方案 §5 已知取舍);
      // 若不裁剪,长时间暂停会让 pendingEntries 无界增长(reviewer 深审 2026-07-20 修复)。
      // 与 appendLive 同一 FIFO 裁剪口径,只保留最新的 renderCap 条。
      const merged = pendingEntries.concat(entries)
      pendingEntries =
        merged.length > renderCap.value ? merged.slice(merged.length - renderCap.value) : merged
      pendingCount.value = pendingEntries.length
      return
    }
    appendLive(entries)
  }

  // 只要日志窗口这个 Vue app 实例存活就一直订阅(建窗时后端已在 open_log_window 里把订阅标志
  // 置真,见 src-tauri/ipc/log_commands.rs);本 store 全生命周期监听,不随组件挂/卸载反复订阅。
  useTauriListen<unknown>(EVENTS.LOG_BATCH, (event) => onBatch(event.payload))

  function setPaused(next: boolean) {
    paused.value = next
    if (!next && pendingEntries.length > 0) {
      appendLive(pendingEntries)
      pendingEntries = []
      pendingCount.value = 0
    }
  }

  function clearView() {
    liveEntries.value = []
    pendingEntries = []
    pendingCount.value = 0
  }

  function setRenderCap(n: number) {
    const clamped = Math.min(RENDER_CAP_MAX, Math.max(RENDER_CAP_MIN, Math.round(n)))
    renderCap.value = clamped
    if (liveEntries.value.length > clamped) {
      liveEntries.value = liveEntries.value.slice(liveEntries.value.length - clamped)
    }
  }

  // ── 过滤(方案 §5 MVP:级别多选 + target 前缀 + 文本子串,大小写不敏感;
  //    S5 P1 补正则搜索模式)────────────────────────────────────────────
  const levelFilter = ref<Set<LogLevelFilter>>(new Set(LOG_LEVELS))
  const targetFilter = ref('')
  const textFilter = ref('')
  const textFilterMode = ref<'substring' | 'regex'>('substring')

  function toggleLevel(level: LogLevelFilter) {
    const next = new Set(levelFilter.value)
    if (next.has(level)) next.delete(level)
    else next.add(level)
    levelFilter.value = next
  }

  // 正则非法(用户还在输入途中很常见,如漏闭合括号)时降级为 null——matchesFilter 据此不施加文本
  // 过滤(不静默清空视图误导用户「日志没了」),textFilterInvalid 供 UI 显示错误态边框/提示。
  const compiledTextRegex = computed<RegExp | null>(() => {
    if (textFilterMode.value !== 'regex' || !textFilter.value) return null
    try {
      return new RegExp(textFilter.value, 'i')
    } catch {
      return null
    }
  })
  const textFilterInvalid = computed(
    () => textFilterMode.value === 'regex' && textFilter.value !== '' && !compiledTextRegex.value,
  )

  function toggleTextFilterMode() {
    textFilterMode.value = textFilterMode.value === 'substring' ? 'regex' : 'substring'
  }

  function matchesFilter(e: LogEntry): boolean {
    const level = e.level.toLowerCase() as LogLevelFilter
    if (!levelFilter.value.has(level)) return false
    if (targetFilter.value && !e.target.toLowerCase().startsWith(targetFilter.value.toLowerCase())) {
      return false
    }
    if (textFilter.value) {
      if (textFilterMode.value === 'regex') {
        const re = compiledTextRegex.value
        if (re && !re.test(e.msg)) return false
      } else if (!e.msg.toLowerCase().includes(textFilter.value.toLowerCase())) {
        return false
      }
    }
    return true
  }

  const filteredLive = computed(() => liveEntries.value.filter(matchesFilter))

  // ── 历史(按日 JSONL 分页,方案 §5 MVP)────────────────────────────────
  const historyFiles = ref<Array<{ name: string; sizeBytes: number; modifiedMs: number }>>([])
  const selectedHistoryFile = ref<string | null>(null)
  const historyEntries = shallowRef<LogEntry[]>([])
  const historyLoading = ref(false)
  const historyHasMoreOlder = ref(false)
  // 分页锚点(方案 §5,reviewer 深审 2026-07-20 修复):后端 read_log_file_page 返回的
  // oldest_loaded_line 原样带回下一次请求的 beforeLine。null = 从文件当前末尾取(仅首页)。
  // 用「绝对行号锚点」而非「距末尾的相对偏移」,防止浏览仍在写入的当日日志文件时,两次分页
  // 调用之间新追加的行让下一页窗口整体前移、与上一页产生重叠。
  let historyBeforeLine: number | null = null

  async function loadHistoryFiles() {
    historyFiles.value = await invokeIpc<Array<{ name: string; sizeBytes: number; modifiedMs: number }>>(
      IPC.LIST_LOG_FILES,
    )
  }

  async function openHistoryFile(name: string) {
    selectedHistoryFile.value = name
    historyEntries.value = []
    historyBeforeLine = null
    historyHasMoreOlder.value = false
    await loadOlderHistory()
  }

  async function loadOlderHistory() {
    if (!selectedHistoryFile.value || historyLoading.value) return
    historyLoading.value = true
    try {
      const page = await invokeIpc<{
        lines: unknown[]
        totalLines: number
        hasMoreOlder: boolean
        oldestLoadedLine: number
      }>(IPC.READ_LOG_FILE_PAGE, {
        fileName: selectedHistoryFile.value,
        beforeLine: historyBeforeLine,
        limit: HISTORY_PAGE_SIZE,
      })
      historyBeforeLine = page.oldestLoadedLine
      historyHasMoreOlder.value = page.hasMoreOlder
      historyEntries.value = tagEntries(page.lines).concat(historyEntries.value)
    } finally {
      historyLoading.value = false
    }
  }

  const filteredHistory = computed(() => historyEntries.value.filter(matchesFilter))

  // ── 过滤 preset(方案 §5 P1「过滤条件 preset(Docker 范式)」)────────────────
  // 存中央设置键 log_filter_presets(原生表数组):全局用户偏好,与其它设置同库同理。
  interface LogFilterPreset {
    name: string
    levels: LogLevelFilter[]
    target: string
    text: string
    textMode: 'substring' | 'regex'
  }
  const PRESETS_KEY = 'log_filter_presets'

  /** 读 preset 列表:缺键/非法文本/非数组一律空表。 */
  function readPresets(): LogFilterPreset[] {
    const parsed = parseSettingJson<unknown>(readSetting(PRESETS_KEY), [])
    return Array.isArray(parsed) ? (parsed as LogFilterPreset[]) : []
  }

  const presets = ref<LogFilterPreset[]>(readPresets())
  // 后端只应用:权威快照变化(启动水合 / 恢复默认 / 外部改文件)→ 重读列表,不写回。
  watch(
    () => readSetting(PRESETS_KEY),
    () => {
      presets.value = readPresets()
    },
  )

  /** 提交 preset 列表:写盘失败由中央服务统一提示,此处 catch 只为收掉 promise。 */
  function persistPresets() {
    writeSettings({ [PRESETS_KEY]: JSON.stringify(presets.value) }).catch(() => {})
  }

  function saveCurrentAsPreset(name: string) {
    const trimmed = name.trim()
    if (!trimmed) return
    const next = presets.value.filter((p) => p.name !== trimmed)
    next.push({
      name: trimmed,
      levels: Array.from(levelFilter.value),
      target: targetFilter.value,
      text: textFilter.value,
      textMode: textFilterMode.value,
    })
    presets.value = next
    persistPresets()
  }

  function applyPreset(name: string) {
    const p = presets.value.find((x) => x.name === name)
    if (!p) return
    levelFilter.value = new Set(p.levels)
    targetFilter.value = p.target
    textFilter.value = p.text
    textFilterMode.value = p.textMode
  }

  function deletePreset(name: string) {
    presets.value = presets.value.filter((p) => p.name !== name)
    persistPresets()
  }

  // ── 诊断(方案 §5 P1「丢弃计数/压缩计数可见」)────────────────────────────
  const diagnostics = ref<{
    droppedLines: number
    dedupActive: Array<{
      target: string
      code: string
      swallowedInWindow: number
      windowAgeSecs: number
    }>
  } | null>(null)

  async function refreshDiagnostics() {
    diagnostics.value = await invokeIpc(IPC.GET_LOG_DIAGNOSTICS)
  }

  // ── 直方图(方案 §5 P2「时间线直方图+级别分布」)──────────────────────────
  const histogram = ref<{
    buckets: Array<{ bucket: string; level: string; count: number }>
    totalLines: number
    parsedLines: number
  } | null>(null)
  const histogramLoading = ref(false)
  // 生成当前 histogram 数据时实际使用的粒度(而非"用户下拉框此刻选的值"——用户可能在结果显示后
  // 又改了下拉框但未点重新生成,LogHistogramChart 的 X 轴标签格式必须跟着数据走,不能跟着控件走,
  // 否则 reviewer 深审指出的"靠猜字符串形状判断粒度"问题换个位置又出现)。
  const histogramBucketUsed = ref<'hour' | 'day'>('hour')

  async function loadHistogram(fileName: string, bucket: 'hour' | 'day') {
    histogramLoading.value = true
    try {
      histogram.value = await invokeIpc(IPC.COMPUTE_LOG_HISTOGRAM, { fileName, bucket })
      histogramBucketUsed.value = bucket
    } finally {
      histogramLoading.value = false
    }
  }

  // ── 诊断包导出(方案 §5 P2)────────────────────────────────────────────
  async function exportDiagnosticsPackage() {
    return invokeIpc<{ dir: string; zipPath: string; sizeBytes: number; redactedMatches: number }>(
      IPC.EXPORT_DIAGNOSTICS_PACKAGE,
    )
  }

  // ── 双窗格上下文(方案 §5 P2「过滤命中↔原文上下文」,klogg 范式)──────────────
  // 选中一条(过滤后可见的)日志行,在同一来源的**未过滤**数组里找到它,展开前后各 CONTEXT_RADIUS
  // 条作为上下文——用已加载在内存里的数据,不追加 IPC 往返(命中行若刚好在已加载页边界外则该侧
  // 上下文不完整,MVP 已知取舍)。liveEntries/historyEntries 共享全局单调 _seq,故不需要调用方
  // 说明「这是哪个 tab」,直接两个数组依次找即可。
  const CONTEXT_RADIUS = 15
  const selectedSeq = ref<number | null>(null)

  function selectEntry(seq: number | null) {
    selectedSeq.value = selectedSeq.value === seq ? null : seq
  }

  const contextView = computed<{ entries: LogEntry[]; selectedIndex: number } | null>(() => {
    if (selectedSeq.value === null) return null
    for (const source of [liveEntries.value, historyEntries.value]) {
      const idx = source.findIndex((e) => e._seq === selectedSeq.value)
      if (idx !== -1) {
        const start = Math.max(0, idx - CONTEXT_RADIUS)
        const end = Math.min(source.length, idx + CONTEXT_RADIUS + 1)
        return { entries: source.slice(start, end), selectedIndex: idx - start }
      }
    }
    return null
  })

  return {
    liveEntries,
    filteredLive,
    paused,
    pendingCount,
    renderCap,
    setPaused,
    clearView,
    setRenderCap,
    levelFilter,
    targetFilter,
    textFilter,
    textFilterMode,
    textFilterInvalid,
    toggleLevel,
    toggleTextFilterMode,
    historyFiles,
    selectedHistoryFile,
    historyEntries,
    filteredHistory,
    historyLoading,
    historyHasMoreOlder,
    loadHistoryFiles,
    openHistoryFile,
    loadOlderHistory,
    presets,
    saveCurrentAsPreset,
    applyPreset,
    deletePreset,
    diagnostics,
    refreshDiagnostics,
    histogram,
    histogramLoading,
    histogramBucketUsed,
    loadHistogram,
    exportDiagnosticsPackage,
    selectedSeq,
    selectEntry,
    contextView,
  }
})
