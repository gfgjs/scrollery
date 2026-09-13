// src/utils/ipc.ts
// Part5 T8/T9 · 统一 IPC 封装 + 结构化错误解析。
//
// 目的:
// 1. **常量强制**——invokeIpc 只接受 `IpcCommand`(IPC 常量的值类型),类型层禁止裸字符串命令名,
//    消除「改后端命令名而前端漏改」的隐患(裸字符串改名零保障)。
// 2. **结构化错误**——后端 AppError 经 IPC 序列化为 `{ code, message }`(error.rs)。invokeIpc 捕获
//    invoke 的 reject,统一解析为 IpcError(带稳定 code),调用方据 code **按类型分流**(而非匹配文案)。
//    仍返回裸字符串的旧命令(如 ai_commands 尚未迁移)被宽容降级为 code='Unknown',不致解析崩溃。

import { invoke } from '@tauri-apps/api/core'
import { IPC } from '../constants/ipc'
import { isUiHarness } from '../harness/runtime'
import { invokeHarness } from '../harness/ipcFixtures'

/** 所有已登记 IPC 命令名的联合类型(IPC 常量的值)。invokeIpc 仅接受此类型 → 杜绝裸字符串。 */
export type IpcCommand = (typeof IPC)[keyof typeof IPC]

/**
 * 后端 AppError 的已知稳定 code(error.rs 的 Serialize 实现)。**非穷尽**——code 为开放字符串,
 * exotic 子系统会透出底层码(如 'rollback'/'http');这里仅列前端会按类型分流的常用码,便于 IDE 补全。
 */
export type AppErrorCode =
  | 'Io'
  | 'Db'
  | 'Pool'
  | 'UnsupportedFormat'
  | 'PathResolution'
  | 'LayoutNotReady'
  | 'ViewStale'
  | 'ScanRootNotFound'
  | 'MediaNotFound'
  | 'Cancelled'
  | 'Ai'
  | 'AiModelNotLoaded'
  | 'System'
  | 'Internal'
  | 'VolumeOffline' // 前向声明:T13 离线 UX 落地后由后端打开原图/视频命令返回(见 §3.7)
  | 'move_db_pending' // 目录移动半完成:物理已落盘、索引未更新,可重试收尾(P0-1)
  | 'Unknown' // 前端兜底:无法解析为结构化 AppError 时(如旧命令裸字符串)
  | (string & {}) // 开放:保留任意后端/exotic 自定义 code,同时不丢上面字面量的补全

/** 「物理已落盘、索引未更新」的稳定码(error.rs 的 AppError::MoveRecovery 用这个 code)。 */
export const MOVE_DB_PENDING_CODE = 'move_db_pending'

/**
 * 目录移动半完成时后端**额外**序列化的恢复定位(error.rs 的 MoveRecovery 分支,见其 Serialize 实现)。
 * 只要这两个字段:够前端如实显示「文件到底在哪」并按日志 id 重试,不多带内部错误串。
 */
export interface MoveRecoveryDetails {
  /** 阶段日志 id:重试收尾(retry_directory_move)按它定位。 */
  recoveryId: number
  /** 文件真实落点(绝对路径)。 */
  targetAbsPath: string
}

/** 结构化 IPC 错误。`code` 供按类型分流;`message` 仅作展示/日志,不承担分流职责。 */
export class IpcError extends Error {
  readonly code: AppErrorCode
  /** 仅 move_db_pending 携带(其余变体与旧命令恒为 null);调用方经 [moveRecoveryOf] 取用。 */
  readonly recovery: MoveRecoveryDetails | null
  constructor(code: AppErrorCode, message: string, recovery: MoveRecoveryDetails | null = null) {
    super(message)
    this.name = 'IpcError'
    this.code = code
    // 恢复定位只对 move_db_pending 有意义:别的 code 带着它,只会让「有重试入口」看起来像个假象。
    this.recovery = code === MOVE_DB_PENDING_CODE ? recovery : null
  }
}

/**
 * 把 invoke 的 reject 值解析为 IpcError。
 * - 结构化 `{ code, message }`(后端 AppError) → 原样取 code/message。
 * - 裸字符串(尚未迁移为 AppError 的旧命令) → code='Unknown',message 即该串。
 * - 其它(Error / 未知) → code='Unknown',尽力取 message。
 */
export function parseAppError(e: unknown): IpcError {
  // 已解析过的错误原样返回:否则恢复定位会在二次解析里被丢掉(IpcError 实例同样有 code/message 字段)。
  if (e instanceof IpcError) return e
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
    const o = e as {
      code: unknown
      message: unknown
      recoveryId?: unknown
      targetAbsPath?: unknown
    }
    return new IpcError(String(o.code), String(o.message), readMoveRecovery(o))
  }
  if (typeof e === 'string') return new IpcError('Unknown', e)
  if (e instanceof Error) return new IpcError('Unknown', e.message)
  return new IpcError('Unknown', String(e))
}

/**
 * 只接受类型正确的恢复定位:字段缺失或类型不符一律当作「没有」(不猜、不补默认值)。
 * 半成品的日志 id 与路径是给用户看并据此点重试的,猜出来的值比没有更糟。
 */
function readMoveRecovery(o: {
  recoveryId?: unknown
  targetAbsPath?: unknown
}): MoveRecoveryDetails | null {
  if (
    typeof o.recoveryId === 'number' &&
    Number.isInteger(o.recoveryId) &&
    typeof o.targetAbsPath === 'string'
  ) {
    return { recoveryId: o.recoveryId, targetAbsPath: o.targetAbsPath }
  }
  return null
}

/**
 * 从任意捕获值取目录移动的半完成定位(仅 move_db_pending 有)。
 * 调用方据此显示文件真实位置并登记重试入口;其余错误返回 null,原有分流不变。
 */
export function moveRecoveryOf(e: unknown): MoveRecoveryDetails | null {
  const err = parseAppError(e)
  return err.code === MOVE_DB_PENDING_CODE ? err.recovery : null
}

/**
 * 生成一次用户操作的关联 id(方案 §3.3/S2 operation_id):贯穿「前端发起 → IPC command →
 * spawn_blocking/后台流水线」的显式字符串传参(tracing span 不跨 spawn_blocking,D-304)。
 * 目前仅流水线/扫描相关命令的调用点显式生成并透传,不强求全量(方案 §9.4 S2 step 1)。
 */
export function generateOperationId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `op-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
}

/**
 * 统一 IPC 调用入口:常量强制 + 错误结构化。
 * 调用方 `try { await invokeIpc(IPC.X, args) } catch (e) { if ((e as IpcError).code === 'Cancelled') … }`。
 * @param cmd IPC 命令(必须取自 IPC 常量;裸字符串被类型层拒绝)
 * @param args 命令参数(snake_case 由 Tauri 自动转;前端传 camelCase 键)
 */
export async function invokeIpc<T>(cmd: IpcCommand, args?: Record<string, unknown>): Promise<T> {
  if (isUiHarness) return invokeHarness<T>(cmd, args)
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    throw parseAppError(e)
  }
}

/**
 * raw body IPC 调用:字节负载直传(零 JSON 膨胀),元数据走 request headers。
 * 与 invokeIpc 同保「常量强制 + 错误结构化」两个契约。用于大二进制载荷命令
 * (如 store_doc_thumbnail:PNG 经 JSON 数字数组每字节膨胀 ~4 字符)。
 * @param headers 自定义头(小写命名,值须为字符串;后端经 request.headers() 读取)
 */
export async function invokeIpcRaw<T>(
  cmd: IpcCommand,
  body: Uint8Array,
  headers: Record<string, string>,
): Promise<T> {
  if (isUiHarness) return invokeHarness<T>(cmd, { body, headers })
  try {
    return await invoke<T>(cmd, body, { headers })
  } catch (e) {
    throw parseAppError(e)
  }
}

/** 从任意捕获值提取可展示的错误文案(优先 IpcError.message)。toast 等展示用。 */
export function ipcErrorMessage(e: unknown): string {
  if (e instanceof IpcError || e instanceof Error) return e.message
  return parseAppError(e).message
}
