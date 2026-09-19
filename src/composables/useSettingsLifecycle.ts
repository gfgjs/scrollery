// src/composables/useSettingsLifecycle.ts
// 退出前设置 flush 的前端一侧(设置集中保存,批次C)。
//
// 为什么需要这一层:正常退出必须等前端在途提交落盘,而浏览器 unload 里的异步 IPC 不可靠
// (页面销毁竞态),方案 §5.4 因此定下「后端请求 → 前端落盘 → 前端回执 → 后端再退出」的协议,
// 由退出事件驱动,不依赖任何 unload 钩子。本文件是该协议在前端的唯一实现。
//
// 协议(带 request id,避免重入;不搭通用 RPC 框架):
//   1. 后端在退出/关窗前发 IPC.SETTINGS_FLUSH_REQUESTED,载荷 { requestId }。
//   2. 本模块 await flushSettings()(中央保存集合的强制落盘),再经 IPC.SETTINGS_FLUSH_DONE
//      回执 { requestId, ok }。
//   3. 同一时刻只跑一次 flush(单飞):重叠到达的请求复用同一个在途 Promise,不会并行写盘。
//   4. 同一个 requestId 只回执一次:重复投递只重跑 flush、不重复回执(后端按 id 记账,重复回执
//      虽被它忽略,但没必要自造流量)。
//   5. flush 失败即回执 ok=false,后端据此保留窗口不退出,由 CloseConfirmDialog 给出
//      「重试 / 放弃未保存修改并退出」;本模块不吞错、不把失败当成功。

import { ref } from 'vue'
import { listenAppEvent } from '../utils/appEvents'
import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { logger } from '../utils/logger'
import { flushSettings } from '../stores/settingsPersistence'

/** 后端 flush 请求载荷(与 config/lifecycle 侧的 FlushRequest 同型)。 */
interface FlushRequestPayload {
  requestId: string
}

/** 单飞闸:同一时刻只跑一次 flush;重入调用复用同一个在途 Promise。 */
let inflight: Promise<void> | null = null

/** 已回执过的 requestId。上限只为防后端异常时无界增长,不承担长期记账职责。 */
const ackedRequestIds = new Set<string>()
const ACKED_IDS_CAP = 32

/** 卸载句柄(幂等装配用)。 */
let unlisten: (() => void) | null = null

/** 最近一次收到的 requestId,供测试与诊断观察。 */
export const lastFlushRequestId = ref<string | null>(null)

/**
 * 强制把在途/待保存设置落盘,并在同一时刻只跑一次。
 *
 * 失败原样抛出中央保存集合的错误(写盘失败、generation 过期等),由调用方决定重试或放弃 ——
 * 本模块不吞错,也不把失败当成功回报后端。
 */
export function flushPendingSettings(): Promise<void> {
  if (!inflight) {
    inflight = flushSettings().finally(() => {
      inflight = null
    })
  }
  return inflight
}

function rememberAck(requestId: string): void {
  ackedRequestIds.add(requestId)
  if (ackedRequestIds.size > ACKED_IDS_CAP) {
    const oldest = ackedRequestIds.values().next().value
    if (oldest !== undefined) ackedRequestIds.delete(oldest)
  }
}

/** 向前端所在窗口回执一次 flush 结果。回执自身失败只记日志:后端有超时兜底,不会永久悬住。 */
async function ack(requestId: string, ok: boolean): Promise<void> {
  lastFlushRequestId.value = requestId
  rememberAck(requestId)
  try {
    await invokeIpc(IPC.SETTINGS_FLUSH_DONE, { requestId, ok })
  } catch (e) {
    logger.warn('[settingsLifecycle] flush ack failed', { error: e, requestId, ok })
  }
}

/**
 * 处理一次后端 flush 请求:落盘 → 回执。已在途/已回执的 id 按上述协议去重。
 */
async function handleFlushRequest(requestId: string): Promise<void> {
  lastFlushRequestId.value = requestId
  let ok = false
  try {
    await flushPendingSettings()
    ok = true
  } catch (e) {
    // 不吞错:失败如实回执 ok=false,后端据此保留窗口并让用户裁决重试或明确放弃。
    logger.warn('[settingsLifecycle] settings flush failed before exit', { error: e, requestId })
  }
  // 同一个 requestId 只回执一次 —— **成功与失败两条路径同规则**:重复投递只重跑 flush,
  // 不重复回执(失败路径若各报一次,后端会收到同一 id 的多份结论)。
  if (ackedRequestIds.has(requestId)) return
  await ack(requestId, ok)
}

/**
 * 装配退出 flush 协议。全应用在各自的根组件 onMounted 调一次(主窗口 App.vue 与独立日志窗口
 * 的入口都要挂:两者都可能持有待保存设置)。幂等 —— 重复调用返回同一解绑函数,不重复注册监听。
 */
export async function installSettingsLifecycle(): Promise<() => void> {
  if (unlisten) return unlisten
  let disposed = false
  const off = await listenAppEvent<FlushRequestPayload>(
    IPC.SETTINGS_FLUSH_REQUESTED,
    (event) => {
      const requestId = event.payload?.requestId
      if (!requestId) {
        logger.warn('[settingsLifecycle] flush request without requestId, ignored')
        return
      }
      void (async () => {
        if (disposed) return
        await handleFlushRequest(requestId)
      })()
    },
  )
  unlisten = () => {
    disposed = true
    off()
    unlisten = null
  }
  return unlisten
}

/** 仅供测试与卸载:解绑监听并清空单飞与已回执状态。 */
export function disposeSettingsLifecycle(): void {
  unlisten?.()
  unlisten = null
  inflight = null
  ackedRequestIds.clear()
  lastFlushRequestId.value = null
}
