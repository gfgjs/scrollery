// 去重分析运行态 store：只承载后端分析任务的「运行态」（状态快照/启停/完成时刻），
// 供主画廊重复镜头状态条（components/media/useDuplicateLensStatus.ts）消费。
// 重复内容的浏览/布局不在本 store——镜头状态与 URL 同步见 duplicateLensStore，
// 成员/文件夹图由后端布局缓存承载（docs/designs/2026-09-02-主画廊重复项浏览方案.md §10.1）。
// 旧组浏览/清理 IPC（list_duplicate_groups 等）后端暂保留，但前端已无绑定消费方（P4 退场）。

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { listen } from '@tauri-apps/api/event'
import { EVENTS, IPC } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'
import type { DedupStatusSnapshot } from '../types/ipc'

export const useDedupStore = defineStore('dedup', () => {
  const status = ref<DedupStatusSnapshot>({
    runId: null,
    status: 'idle',
    phase: 'idle',
    itemsDone: 0,
    itemsTotal: 0,
    bytesDone: 0,
    bytesTotal: 0,
    groupsFound: 0,
    potentialLogicalBytes: 0,
    errors: [],
    waitingOn: [],
  })
  /**
   * 最近一次分析完成（status → completed）的本地时间戳（主画廊重复镜头状态条「最近完成于」）。
   * 仅存在于本会话内存：restoreStatus 首次恢复（含应用重启后）时置 null——
   * 后端快照不带完成时刻,重启后丢失该文案属 MVP 接受范围（方案 §9 状态表）。
   */
  const completedAt = ref<number | null>(null)
  /** restoreStatus 是否已在本会话执行过（幂等恢复 + completedAt 仅首次清空的依据）。 */
  const statusRestored = ref(false)
  const isRunning = computed(() => status.value.status === 'running')

  let listenerPromise: Promise<unknown> | null = null
  let eventVersion = 0
  function ensureListener(): Promise<unknown> {
    listenerPromise ??= listen<DedupStatusSnapshot>(EVENTS.DEDUP_PROGRESS, (event) => {
      // 进入 completed 的边沿记录本地完成时刻（旧结果重跑再次完成时刷新）；非 completed
      // 事件不清它——running/failed 期间 UI 不消费该值,成功后由新的边沿重写。
      const previous = status.value.status
      eventVersion += 1
      status.value = event.payload
      if (event.payload.status === 'completed' && previous !== 'completed') {
        completedAt.value = Date.now()
      }
    }).catch((reason: unknown) => {
      listenerPromise = null
      throw reason
    })
    return listenerPromise
  }

  async function restoreStatus() {
    await ensureListener()
    const versionBeforeRequest = eventVersion
    const restored = await invokeIpc<DedupStatusSnapshot>(IPC.DEDUP_STATUS)
    // 监听先建立；请求在途若已有新事件，旧快照不得覆盖事件状态。
    if (eventVersion === versionBeforeRequest) status.value = restored
    if (!statusRestored.value) {
      statusRestored.value = true
      completedAt.value = null
    }
  }

  /** 幂等恢复：重复镜头状态条多次挂载只做一次 IPC,此后状态由事件流维持新鲜。 */
  async function restoreStatusOnce() {
    if (statusRestored.value) return
    await restoreStatus()
  }

  async function start(reset = false) {
    await ensureListener()
    // 新一轮开始即作废旧完成时刻：完成后事件会重新记录,失败/停止期间不显示陈旧时间。
    completedAt.value = null
    status.value = await invokeIpc<DedupStatusSnapshot>(IPC.START_DEDUP_ANALYSIS, {
      reset,
    })
  }

  async function stop() {
    if (!isRunning.value) return
    status.value = await invokeIpc<DedupStatusSnapshot>(IPC.STOP_DEDUP_ANALYSIS)
  }

  return {
    status,
    completedAt,
    isRunning,
    restoreStatus,
    restoreStatusOnce,
    start,
    stop,
  }
})
