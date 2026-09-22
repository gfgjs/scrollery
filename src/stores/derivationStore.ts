// src/stores/derivationStore.ts
// 视频封面/关键帧手动提取控制(2026-07-18):对派生流水线 IPC 的薄封装,只关注 video 两 kind。
// 流水线全局唯一(start/stop 作用于整条流水线),卡片「运行中」= 全局在跑 ∧ 视频侧仍有在途任务;
// 进度无推送事件,沿用 aiStore 模式——运行中定时轮询 derivation_status(kinds 限定)。
// 「提取关键帧」勾选的单一事实源 = configStore.enableVideoKeyframes(同时 gate 自动流水线,D-003):
// 勾选 → 手动提取带 video_keyframes;不勾 → 只提 video_cover。

import { defineStore } from 'pinia'
import { ref, computed, watch, onScopeDispose } from 'vue'
import { invokeIpc, generateOperationId, ipcErrorMessage } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { useConfigStore } from './configStore'
import { useToastStore } from './toastStore'

interface DerivationStatus {
  pending: number
  processing: number
  done: number
  error: number
  isRunning: boolean
  active: boolean
}

const VIDEO_COVER = 'video_cover'
const VIDEO_KEYFRAMES = 'video_keyframes'

const POLL_INTERVAL_MS = 1000

export const useDerivationStore = defineStore('derivation', () => {
  const videoStatus = ref<DerivationStatus>({
    pending: 0,
    processing: 0,
    done: 0,
    error: 0,
    isRunning: false,
    active: false,
  })

  // 视频提取运行态:全局流水线在跑,且视频侧还有未消化任务(pending/processing)。
  // 纯全局 isRunning 会把「流水线在跑别的 kind」也算成视频在跑。
  const isVideoRunning = computed(
    () =>
      videoStatus.value.isRunning && videoStatus.value.pending + videoStatus.value.processing > 0,
  )
  const videoTotal = computed(() => {
    const s = videoStatus.value
    return s.pending + s.processing + s.done + s.error
  })
  const videoFinished = computed(() => videoStatus.value.done + videoStatus.value.error)

  // 勾选决定 kind 集(需求:勾了关键帧才提关键帧,不勾只提封面)。
  function videoKinds(): string[] {
    const config = useConfigStore()
    return config.enableVideoKeyframes ? [VIDEO_COVER, VIDEO_KEYFRAMES] : [VIDEO_COVER]
  }

  let statusGeneration = 0
  let statusInFlight: Promise<void> | null = null

  function invalidateStatus() {
    statusGeneration++
    statusInFlight = null
    return statusGeneration
  }

  function fetchVideoStatus(): Promise<void> {
    if (statusInFlight) return statusInFlight
    const generation = statusGeneration
    // 计数范围跟随勾选:不勾关键帧时,keyframes 的存量 pending 不应把进度分母撑大。
    const request: Promise<void> = invokeIpc<DerivationStatus>(IPC.DERIVATION_STATUS, {
      kinds: videoKinds(),
    })
      .then((s) => {
        // 控制动作或 kind 变化后的旧快照不得覆盖当前状态,也不得启停当前轮询。
        if (generation !== statusGeneration) return
        videoStatus.value = s
        if (isVideoRunning.value) ensurePolling()
        else stopPolling()
      })
      .catch(() => {}) // 保留最后已知状态,下一轮仍可重试。
      .finally(() => {
        if (statusInFlight === request) statusInFlight = null
      })
    statusInFlight = request
    return request
  }

  let pollTimer: ReturnType<typeof setInterval> | null = null
  function ensurePolling() {
    if (pollTimer) return
    pollTimer = setInterval(() => {
      void fetchVideoStatus()
    }, POLL_INTERVAL_MS)
  }
  function stopPolling() {
    if (pollTimer) {
      clearInterval(pollTimer)
      pollTimer = null
    }
  }

  watch(
    () => useConfigStore().enableVideoKeyframes,
    () => {
      invalidateStatus()
      void fetchVideoStatus()
    },
    { flush: 'sync' },
  )
  onScopeDispose(() => {
    invalidateStatus()
    stopPolling()
  })

  /// 增量提取:backfill 补缺行 + 续跑 pending/中断项,已完成跳过。
  async function startVideoIncremental() {
    await startVideo(false)
  }
  /// 全量提取:先把所选 kind 的 done/error 行退回 pending(后端 reset)再跑 = 全部重做。
  async function startVideoFull() {
    await startVideo(true)
  }
  async function startVideo(reset: boolean) {
    const operationId = generateOperationId()
    try {
      await invokeIpc(IPC.START_DERIVATION, { kinds: videoKinds(), reset, operationId })
    } catch (e) {
      // 后端拒绝须用户可见(2026-07-23 裁决 J13):调用方模板 @click 直调不接 catch,
      // 此处不 toast 则失败只剩 unhandledrejection 日志、按钮点了没反应。
      // 姿态对齐 aiStore start/restart 的 toast + ipcErrorMessage。
      useToastStore().addToast('error', ipcErrorMessage(e))
      return
    }
    const generation = invalidateStatus()
    await fetchVideoStatus()
    if (generation === statusGeneration) ensurePolling()
  }

  // 停止 = 全局 stop_derivation(清续传标志)。派生流水线全局唯一,音频/文档 kind 的在途任务
  // 会一并停下——它们由 useDerivationAutoStart 在下次 media_enriched/启动时自动补跑,无进度损失。
  async function stopVideoExtraction() {
    try {
      await invokeIpc(IPC.STOP_DERIVATION)
    } catch (e) {
      // 同 startVideo:失败须 toast(J13)。停失败=流水线可能仍在跑,不动轮询,
      // 由 fetchVideoStatus/isVideoRunning 决定表停不停。
      useToastStore().addToast('error', ipcErrorMessage(e))
      return
    }
    stopPolling()
    invalidateStatus()
    await fetchVideoStatus()
  }

  return {
    videoStatus,
    isVideoRunning,
    videoTotal,
    videoFinished,
    fetchVideoStatus,
    startVideoIncremental,
    startVideoFull,
    stopVideoExtraction,
  }
})
