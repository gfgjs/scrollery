// 去重分析运行态 store：只承载后端分析任务的「运行态」（状态快照/启停/完成时刻），
// 供主画廊重复镜头状态条（components/media/useDuplicateLensStatus.ts）消费。
// 重复内容的浏览/布局不在本 store——镜头状态与 URL 同步见 duplicateLensStore，
// 成员/文件夹图由后端布局缓存承载。

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
   * 后端快照不带完成时刻，重启后无法恢复该文案。
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
    const versionBeforeRequest = eventVersion
    const started = await invokeIpc<DedupStatusSnapshot>(IPC.START_DEDUP_ANALYSIS, {
      reset,
    })
    // 后台任务可能先于启动响应完成；与恢复读取相同，优先保留在途收到的事件。
    if (eventVersion === versionBeforeRequest) status.value = started
  }

  async function stop() {
    if (!isRunning.value) return
    const versionBeforeRequest = eventVersion
    const stopped = await invokeIpc<DedupStatusSnapshot>(IPC.STOP_DEDUP_ANALYSIS)
    // 停止事件已允许重新开始，迟到的旧响应不能覆盖新一轮进度。
    if (eventVersion === versionBeforeRequest) status.value = stopped
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
