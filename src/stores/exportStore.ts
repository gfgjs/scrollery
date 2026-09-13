// src/stores/exportStore.ts
// 导出整理成果(方案 A):进度事件订阅 + 快照恢复(同 scanStore 的缩略图生成进度姿态——
// Channel 随发起它的 webview 一起死,故用 app 级事件 + export_status 快照恢复),
// 以及供任意入口(选区工具条/右键菜单/相册视图工具栏)统一触发的对话框开关态。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invokeIpc } from '../utils/ipc'
import { IPC, EVENTS } from '../constants/ipc'
import type { BackendSelectionDescriptor } from '../composables/useSelection'

export type ExportNamingScheme = 'original' | 'sequence' | 'date'
export type ExportConflictPolicy = 'rename' | 'skip'

/** 镜像后端 `export::manifest::ExportSource`(serde tag="kind")。 */
export type ExportSource =
  | { kind: 'selection' }
  | { kind: 'album'; id: number; name: string }
  | { kind: 'view'; name: string }

export interface ExportItemResultDto {
  fileName: string
  code: string
}

export type ExportJobStatus = 'idle' | 'running' | 'completed' | 'failed' | 'cancelled'

export interface ExportProgressPayload {
  jobId: string
  status: ExportJobStatus
  processed?: number
  total?: number
  finalDir?: string
  succeeded?: number
  items?: ExportItemResultDto[]
  itemsTotal?: number
  code?: string
}

export interface ExportRequest {
  selection: BackendSelectionDescriptor
  targetParent: string
  naming: ExportNamingScheme
  conflict: ExportConflictPolicy
  includeManifest: boolean
  source: ExportSource
  allowInsideLibrary: boolean
}

export interface ExportPreflight {
  count: number
  estimatedBytes: number
  offlineOrMissingCount: number
  nonZeroRotationCount: number
  targetWritable: boolean
  insideLibraryWarning: boolean
}

const IDLE_PROGRESS: ExportProgressPayload = { jobId: '', status: 'idle' }

export const useExportStore = defineStore('export', () => {
  const progress = ref<ExportProgressPayload>(IDLE_PROGRESS)
  const isRunning = computed(() => progress.value.status === 'running')

  // 对话框触发态(全局单例):任意入口只需 openExportDialog(selection, source, rebuild),
  // 对话框本体只挂载一份(App.vue),不必每个入口各自持有一份对话框实例状态。
  const dialogOpen = ref(false)
  const dialogSelection = ref<BackendSelectionDescriptor | null>(null)
  const dialogSource = ref<ExportSource>({ kind: 'selection' })
  // ViewStale 重试回调(外部审查【严重】):此前 ExportDialog 的重试无条件用当前选区
  // (selection.toBackendDescriptor())重建描述符,会把相册/视图导出静默换成当前选区——三入口
  // 语义不同,重建方式必须各自持有。缺失(null)= 该入口未接线或重建不可行,重试必须失败,
  // 严禁回退借用选区。
  const dialogSelectionRebuild = ref<(() => BackendSelectionDescriptor | null) | null>(null)

  function openExportDialog(
    selection: BackendSelectionDescriptor,
    source: ExportSource,
    rebuild: (() => BackendSelectionDescriptor | null) | null,
  ) {
    dialogSelection.value = selection
    dialogSource.value = source
    dialogSelectionRebuild.value = rebuild
    dialogOpen.value = true
  }
  function closeExportDialog() {
    dialogOpen.value = false
    // 防御性清空(复审建议):dialogSelection/dialogSource **不**清(见下方 spec 注释——对话框可能
    // 被同一轮再次打开复用最近一次上下文,这两者的"关闭不清空"是既定策略)。dialogSelectionRebuild
    // 不同——它是"当次打开注册的那个入口"的重建闭包,复用价值为负:若下次以不同入口重新打开却漏
    // 传第三参(如未来某条调用路径疏漏),留着旧回调会让 ViewStale 重试悄悄用上一入口的重建逻辑,
    // 而不是「回调缺失→重试失败」的安全默认。清空后,任何忘记注册的入口都会显式失败而非误用旧值。
    dialogSelectionRebuild.value = null
  }

  // ViewStale 重试的描述符解析(P3 特征化测试种子,从 ExportDialog.start() 的 catch 分支抽出):
  // 唯一输入是「当次打开时该入口是否注册了 rebuild、rebuild() 本身返回什么」——**不读取、不接触
  // 任何选区状态**,这本身就是「不得回退借用选区」的结构性保证(没有选区引用可借)。
  // 输出:回调存在且返回非 null → 该描述符;回调缺失或返回 null → null(调用方按失败处理)。
  function resolveViewStaleRetry(): BackendSelectionDescriptor | null {
    return dialogSelectionRebuild.value ? dialogSelectionRebuild.value() : null
  }

  // 终态(completed/failed/cancelled)不可被同一 job 的迟到 running 快照回退覆盖:快照回填是
  // "先订阅再拉快照"两步异步操作,若 completed 事件先到、running 快照后到(如导出秒级完成、
  // 快照查询恰好卡在网络/调度延迟),旧快照会把已应用的终态盖回 running,且导出无后续事件
  // 自愈(cancel 对已空 token 是空操作)——指示器会永久转圈。
  const TERMINAL_STATUSES: ExportJobStatus[] = ['completed', 'failed', 'cancelled']
  function applyProgress(p: ExportProgressPayload) {
    const cur = progress.value
    if (cur.jobId === p.jobId && TERMINAL_STATUSES.includes(cur.status) && p.status === 'running') {
      return
    }
    progress.value = p
  }

  // 共享注册 Promise(而非布尔标记,同 scanStore 姿态):并发调用都等同一次注册完成。
  let listenerPromise: Promise<unknown> | null = null
  function ensureListener(): Promise<unknown> {
    listenerPromise ??= listen<ExportProgressPayload>(EVENTS.EXPORT_PROGRESS, (e) =>
      applyProgress(e.payload),
    ).catch((e) => {
      // 审查 P3:`??=` 会把失败的 Promise 永久缓存——listen 一次失败后,startExport 的
      // `await ensureListener()` 会永远抛错,导出功能整机死锁到重启。失败时清空缓存,
      // 允许下次调用重试。
      listenerPromise = null
      throw e
    })
    return listenerPromise
  }

  // 启动/刷新恢复:先订阅事件流,再查后端快照回填——导出仍在跑时进度条立即续上。
  async function restoreExportProgress() {
    await ensureListener()
    const snap = await invokeIpc<ExportProgressPayload>(IPC.EXPORT_STATUS).catch(() => null)
    if (snap && snap.status !== 'idle') applyProgress(snap)
  }

  async function preflightExport(req: ExportRequest): Promise<ExportPreflight> {
    return invokeIpc<ExportPreflight>(IPC.PREFLIGHT_EXPORT, { req })
  }

  async function startExport(req: ExportRequest): Promise<string> {
    await ensureListener() // 先订阅再启动,避免早期进度事件在监听挂上前丢失
    return invokeIpc<string>(IPC.START_EXPORT, { req })
  }

  async function stopExport() {
    if (!progress.value.jobId) return
    await invokeIpc(IPC.STOP_EXPORT, { jobId: progress.value.jobId })
  }

  return {
    progress,
    isRunning,
    dialogOpen,
    dialogSelection,
    dialogSource,
    dialogSelectionRebuild,
    openExportDialog,
    closeExportDialog,
    resolveViewStaleRetry,
    restoreExportProgress,
    preflightExport,
    startExport,
    stopExport,
  }
})
