// src/composables/useRequestQueue.ts
// 批量缩略图请求队列 (§8.3)。收集项目 ID 并以 THUMB_BATCH_SIZE 为批次进行刷新。

import { Channel } from '@tauri-apps/api/core'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import type { ThumbResult } from '../types/media'
import { IPC } from '../constants/ipc'
import { DEFAULTS, THUMB_SIZE_TIERS } from '../constants/defaults'
import { useScanStore } from '../stores/scanStore'
import { useUiStore } from '../stores/uiStore'

/// 视口缩略图批处理的「无进展」超时。若后端在此时长内无任何结果，判定为 worker 卡住
/// （损坏/不支持的文件）并释放该批，使状态指示不会永久卡住（问题9）。取值宽松，
/// 避免仍在推进的慢盘/大 RAW 批次误触发。
const STALL_MS = 30000

/// 可重试失败的有界退避(2026-08-16 审查阶段3):整批 IPC 失败、停滞释放或批内缺结果时,
/// 同一逻辑请求在队列内按 0.5s→1s→2s 最多重试 3 次,而非把拒绝直接抛给调用方——
/// 调用方(MediaGrid.onRequestThumb)的 catch 只留占位,而 canvas 侧 requestThumbOnce
/// 对同签名只上抛一次,拒绝后再无重试路径(冷库静默失败格的根因)。上限防后端持续故障时
/// 的请求风暴;cancelled 是调用方主动放弃,永不重试。后端对在途项 single-flight(ipc.ts
/// CANCEL 注),停滞重试不会造成同项重复生成。
const THUMB_RETRY_MAX = 3
const THUMB_RETRY_BASE_MS = 500

function getOptimalThumbTier(rowHeight: number): number {
  for (const tier of THUMB_SIZE_TIERS) {
    if (tier >= rowHeight) return tier
  }
  return THUMB_SIZE_TIERS[THUMB_SIZE_TIERS.length - 1]
}

type Resolver = (result: ThumbResult) => void

interface RequestWaiter {
  resolve: Resolver
  reject: (err: unknown) => void
}

interface RequestSlot {
  id: number
  state: 'queued' | 'inFlight'
  waiters: RequestWaiter[]
}

/** 拒绝原因分类:cancelled=调用方主动放弃,不重试;stalled/incomplete=可按上限退避重试。 */
type RejectReason = 'cancelled' | 'stalled' | 'incomplete'

class ThumbRequestError extends Error {
  constructor(readonly reason: RejectReason, message: string) {
    super(message)
  }
}

function isRetryable(err: unknown): boolean {
  return err instanceof ThumbRequestError && err.reason !== 'cancelled'
}

export function useRequestQueue() {
  const queue: RequestSlot[] = []
  let flushTimer: ReturnType<typeof setTimeout> | null = null
  const inFlight = new Set<RequestSlot>()
  const activeSlots = new Map<number, RequestSlot>()
  // 退避等待中的重试链:id → 定时器 + 外层 Promise 的 reject(cancel 需能掐断等待期)。
  const retryPending = new Map<number, { timer: ReturnType<typeof setTimeout>; reject: (e: unknown) => void }>()

  let isFlushing = false

  function syncStats() {
    const scan = useScanStore()
    scan.autoThumbQueueSize = queue.length
    scan.autoThumbInFlight = inFlight.size
  }

  function detachSlot(slot: RequestSlot) {
    inFlight.delete(slot)
    if (activeSlots.get(slot.id) === slot) {
      activeSlots.delete(slot.id)
    }
  }

  function resolveSlot(slot: RequestSlot, result: ThumbResult) {
    const waiters = slot.waiters.splice(0)
    waiters.forEach((cb) => cb.resolve(result))
    detachSlot(slot)
  }

  function rejectSlot(slot: RequestSlot, err: unknown) {
    const waiters = slot.waiters.splice(0)
    waiters.forEach((cb) => cb.reject(err))
    detachSlot(slot)
  }

  function flush() {
    flushTimer = null
    if (queue.length === 0) return

    isFlushing = true
    const batch = queue.splice(0, DEFAULTS.THUMB_BATCH_SIZE)
    const batchIds = batch.map((slot) => slot.id)
    const batchSlots = new Map<number, RequestSlot>(batch.map((slot) => [slot.id, slot] as const))
    const pending = new Set(batch)
    batch.forEach((slot) => {
      slot.state = 'inFlight'
      inFlight.add(slot)
    })

    syncStats()

    // 停滞看门狗（问题9）：
    // 若后端 worker 卡在损坏/不支持的文件上，批处理 invoke 永不 resolve、`.finally` 永不
    // 执行，「处理中 N 项」会永久卡住。装一个「无进展」计时器（每送达一个结果就重置）；
    // 若 STALL_MS 内无任何结果，则释放本批仍在途的 id，使 UI 不依赖后端即可恢复。
    // `released` 让 {停滞, finally} 中先到者生效，避免迟到的卡死 invoke 冲掉后续新批。
    let released = false
    let stallTimer: ReturnType<typeof setTimeout> | null = null

    const releaseBatch = (reason: string, rejectErr?: unknown) => {
      if (released) return
      released = true
      if (stallTimer !== null) {
        clearTimeout(stallTimer)
        stallTimer = null
      }
      if (rejectErr !== undefined) {
        for (const slot of Array.from(pending)) {
          pending.delete(slot)
          rejectSlot(slot, rejectErr)
        }
        logger.warn(`[useRequestQueue] ${reason}`)
      } else {
        logger.debug(`[useRequestQueue] ${reason}`)
      }
      isFlushing = false
      syncStats()
      if (queue.length > 0) scheduleFlush()
    }

    const armStall = () => {
      if (released) return
      if (stallTimer !== null) clearTimeout(stallTimer)
      stallTimer = setTimeout(() => {
        releaseBatch(
          `batch stalled ${STALL_MS}ms with no progress — releasing ${batch.length} id(s)`,
          new ThumbRequestError('stalled', 'thumb batch stalled'),
        )
      }, STALL_MS)
    }

    const onResult = new Channel<ThumbResult>()
    onResult.onmessage = (r) => {
      const slot = batchSlots.get(r.itemId)
      if (!slot || !pending.has(slot)) return
      armStall() // 有进展 — 重置无进展计时器

      // 关键点：按本批次的 slot 收尾，而不是按 id 清全局 Map。
      // 这样同一 id 在旧批次结果返回后重新排队时，新 Promise 不会被旧批次误删或误拒绝。
      pending.delete(slot)
      resolveSlot(slot, r)
      syncStats()
      if (pending.size === 0) {
        releaseBatch('batch drained by item results')
      }
    }

    const ui = useUiStore()
    // 生成档位与服务选档对齐(2026-08-16 阶段 2):后端出口按「格尺寸×DPR」选最小满足档,
    // 生成请求也须按设备像素取档,否则 DPR>1 机器上生成的小档永远不满足服务需求
    // (本机 150% 缩放实测:64×1.5=96 → 应生成 128 档而非 64 档)。node 测试环境无 window → 1。
    const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1
    const targetSize = getOptimalThumbTier(ui.gridRowHeight * dpr)
    armStall()

    invokeIpc(IPC.BATCH_REQUEST_THUMBNAILS, { itemIds: batchIds, targetSize, onResult })
      .catch((err) => {
        logger.error(`[useRequestQueue] batch failed: ${err}`, { batchIds })
      })
      .finally(() => {
        // 正常完成：关掉看门狗并释放仍挂起的项（如后端跳过的 id）。若停滞看门狗已触发则空操作。
        releaseBatch('batch finished', new ThumbRequestError('incomplete', 'Batch finished without result'))
      })
  }

  function scheduleFlush() {
    if (flushTimer !== null) return
    if (isFlushing) return
    flushTimer = setTimeout(flush, 50)
  }

  function request(id: number): Promise<ThumbResult> {
    return new Promise((resolve, reject) => {
      // 有界退避重试:槽位拒绝先看可重试性与次数上限,可重试则等退避后重排同一 id
      // (走正常 enqueue 路径,复用 50ms 合批与 single-flight 去重);不可重试或超限才
      // 把拒绝交给调用方。attemptNo 从 0 起,>= THUMB_RETRY_MAX 即终拒。
      const run = (attemptNo: number): void => {
        enqueue(id, resolve, (err) => {
          if (!isRetryable(err) || attemptNo >= THUMB_RETRY_MAX) {
            reject(err)
            return
          }
          const timer = setTimeout(() => {
            retryPending.delete(id)
            run(attemptNo + 1)
          }, THUMB_RETRY_BASE_MS * 2 ** attemptNo)
          retryPending.set(id, { timer, reject })
        })
      }
      run(0)
    })
  }

  function enqueue(id: number, resolve: Resolver, reject: (err: unknown) => void): void {
    const existing = activeSlots.get(id)
    if (existing) {
      existing.waiters.push({ resolve, reject })
      // 已排队或已在后端处理中：复用同一个 slot，确保重复 Promise 一起收尾。
      return
    }

    const slot: RequestSlot = {
      id,
      state: 'queued',
      waiters: [{ resolve, reject }],
    }
    activeSlots.set(id, slot)
    queue.push(slot)
    syncStats()
    scheduleFlush()
  }

  function cancel(id: number) {
    // 退避等待中的重试链:先掐断(否则 cancel 后定时器仍会把该 id 重新排进队列)。
    // 与下方 slot 路径不互斥——等待期与新建 slot 可并存(他人在退避期重新 request 同 id)。
    const pending = retryPending.get(id)
    if (pending) {
      clearTimeout(pending.timer)
      retryPending.delete(id)
      pending.reject(new ThumbRequestError('cancelled', 'cancelled'))
    }

    const slot = activeSlots.get(id)
    if (!slot) return

    if (slot.state === 'queued') {
      const idx = queue.indexOf(slot)
      if (idx >= 0) {
        queue.splice(idx, 1)
      }
      rejectSlot(slot, new ThumbRequestError('cancelled', 'cancelled'))
      syncStats()
    } else {
      // in-flight 请求不能安全地按 id 取消：同一 id 很可能马上重新进入视口。
      // 这里只取消当前前端等待者，保留后端 single-flight；新 Promise 会挂回同一 slot 并随结果 resolve。
      slot.waiters.splice(0).forEach((cb) => cb.reject(new ThumbRequestError('cancelled', 'cancelled')))
    }
  }

  return { request, cancel }
}
