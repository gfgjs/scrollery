// src/types/ipc.ts
// Tauri 事件和通道消息的 IPC 负载类型

import type { MediaFilter } from './media'

// ── 扫描通道负载 ──────────────────────────────────────────────────────────

export interface ScanProgressPayload {
  rootId: number
  runId: string
  scanned: number
  total: number
  processedBytes: number
  totalBytes: number | null
  currentDir: string
  status: 'discovering' | 'scanning' | 'enriching'
}

export interface ScanCompletedPayload {
  rootId: number
  runId: string
  totalItems: number
  totalBytes: number
  elapsedMs: number
  /** 本次缺失检测标记为「缺失」(availability='missing') 的项数（四道闸通过才 >0）。 */
  markedMissing: number
}

export interface ScanErrorPayload {
  rootId: number
  runId: string
  error: string
}

// 🔴 第 8 轮核验 P1-5：对齐 Rust wire format——后端 ScanChannelPayload 为
// `#[serde(tag = "type")]` 的 internally-tagged enum，newtype variant 把内层 struct 字段
// **扁平铺开**到 `type` 旁（非嵌在 progress/completed/error 键下）。故此处用「扁平联合」
// （`{ type } & Payload`），消除运行时 `as unknown as` 强制 cast。
export type ScanChannelPayload =
  | ({ type: 'progress' } & ScanProgressPayload)
  | ({ type: 'completed' } & ScanCompletedPayload)
  | ({ type: 'error' } & ScanErrorPayload)

// ── 丰富化事件 ──────────────────────────────────────────────────────────────

export interface MediaEnrichedPayload {
  rootId: number
  /** rootId=0 的画廊刷新哨兵没有 runId；真实扫描事件必须携带。 */
  runId?: string | null
  enrichedCount: number
  total: number
  processedBytes: number
  totalBytes: number
}

export interface EnrichmentCompletedPayload {
  rootId: number
  runId: string
  elapsedMs: number
  /** 终态错误码：缺省/null = 正常完成；'enrich_failed' | 'enrich_panicked' = 后台补全异常终止
   *  （前端据此弹 warning，提示部分元数据可能缺失）。携带稳定码而非原始错误串。 */
  errorCode?: string | null
}

// ── 数据库更新事件 ──────────────────────────────────────────────────────────

export interface MediaUpdatedPayload {
  action: 'update' | 'delete' | 'restore'
  itemIds: number[]
}

// ── 命令参数 ───────────────────────────────────────────────────────────────

export interface ComputeLayoutParams {
  directoryId?: number | null
  filters?: MediaFilter | null
  containerWidth: number
  rowHeight: number
  gap: number
}

export interface FullThumbProgressPayload {
  generated: number
  total: number
  status: 'running' | 'completed' | 'cancelled' | 'error'
  currentItem?: string
  phase?: string
}

// ── 精确内容去重 ─────────────────────────────────────────────────────────

export type DedupRunStatus =
  | 'idle'
  | 'running'
  | 'stopping'
  | 'completed'
  | 'failed'
  | 'cancelled'

export interface DedupErrorSummary {
  code: string
  count: number
}

export interface DedupStatusSnapshot {
  runId: string | null
  status: DedupRunStatus
  phase: 'idle' | 'quick' | 'exact' | 'unit' | string
  itemsDone: number
  itemsTotal: number
  bytesDone: number
  bytesTotal: number
  groupsFound: number
  potentialLogicalBytes: number
  errors: DedupErrorSummary[]
  waitingOn: string[]
}
// ── 目录移动的半完成恢复 ────────────────────────────────────────────────

/** move_directory 的结果（file_ops_commands.rs 的 MoveDirResult）。 */
export interface MoveDirResult {
  dirId: number
  rootId: number
  newRelPath: string
  affectedDirs: number
  affectedMedia: number
  /** 文件真实落点（绝对路径）：物理搬运成功但索引段失败时，仍能如实告知用户文件在哪。 */
  targetAbsPath: string
  /** 源目录残留（跨卷删源未完成）：索引已更新，旧路径还有一份副本待清理。 */
  sourceLeftover: string | null
  /** 仍需收尾的阶段日志 id（null = 本次移动已完全收尾）。 */
  recoveryId: number | null
}

/** 目录复制的累计进度；publishing 后以命令返回值判定最终结果。 */
export interface DirectoryCopyProgress {
  generation: number
  copiedFiles: number
  copiedBytes: number
  publishing: boolean
}

/** copy_directory 的结果（file_ops_commands.rs 的 CopyDirResult）。 */
export interface CopyDirResult {
  createdRootId: number
  createdRelPath: string
  createdAbsPath: string
  /** 本次落盘的普通文件数。 */
  copiedFiles: number
  /** 需要调用方触发目标根重扫把新文件入库（入库失败不代表复制失败）。 */
  needsRescan: boolean
}

/** 单项实际完成的媒体移动，保留原目录以构造逆操作。 */
export interface MediaRelocationResult {
  id: number
  fromDirId: number
  targetDirId: number
}

/** 批量移动遇首个失败即停止；同目录无操作不计入 completed。 */
export interface MediaRelocationBatchResult {
  completed: MediaRelocationResult[]
  /** 失败项及稳定 IPC 错误；它之后的请求尚未执行。 */
  failure: { id: number; error: { code: string; message: string } } | null
}

/** 批量复制的实际成功项与首个失败项；失败后的请求尚未执行。 */
export interface MediaCopyBatchResult {
  completed: { srcId: number; newId: number }[]
  failure: { id: number; error: { code: string; message: string } } | null
}

/** 未完成目录移动的报告（dir_move.rs 的 MoveRecoveryReport）。 */
export interface DirectoryMoveRecovery {
  recoveryId: number
  /** 收尾后的阶段：intent / published / source_leftover。 */
  stage: string
  sourceName: string
  sourceAbsPath: string
  /** 文件真实落点（绝对路径）。 */
  targetAbsPath: string
  targetRelPath: string
  targetRootId: number
  /** true = 仍需重试（物理未搬完 / 目标未能证明 / 源残留未清 / 盘不在）。 */
  needsRetry: boolean
  /** 稳定短标签（dir_move.rs 的白名单）；标签→文案映射见 directoryMoveRecoveryStore。 */
  detail: string
}

/** 已提交图片结果；available 为新生成与缓存复用之和。 */
export interface ThumbResultCounts {
  available: number
  newlyGenerated: number
  cacheHit: number
  direct: number
  failed: number
  temporarilyUnavailable: number
}

/** 已发布新图片使用的实际解码后端；空值表示未提供执行信息。 */
export interface ThumbExecutionCount {
  backend: string | null
  count: number
}
