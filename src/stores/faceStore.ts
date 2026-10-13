// 人脸识别分析状态与流水线控制；分析管理委托 useAnalysisController。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { invokeIpc, ipcErrorMessage, parseAppError } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'
import { useToastStore } from './toastStore'
import { useAnalysisController } from '../composables/useAnalysisController'
import { guideToModelDownload } from '../composables/useModelDownloadGuide'
import type { FaceStatusSummary, FaceModelInfo, FaceModelDownloadProgress } from '../types/face'

export const useFaceStore = defineStore('face', () => {
  // ── State ─────────────────────────────────────────────────────────────────
  const status = ref<FaceStatusSummary>({
    provider: '',
    gpuName: '',
    faceLoaded: false,
    totalItems: 0,
    processedItems: 0,
    pendingItems: 0,
    personCount: 0,
    faceCount: 0,
    errorItems: 0,
    isAnalyzing: false,
    analysisActive: false,
    waitingOn: [],
  })

  // 与 aiStore 共用分析控制器。face 专属:
  // start/restart 缺模型时定位下载处，其他错误走 toast；pause/stop/自动续传只记日志。
  // analyzedCount 取 processedItems（含失败项，故进度条仍到 100%）；无 onStarted。
  const analysis = useAnalysisController<FaceStatusSummary>({
    status,
    commands: {
      getStatus: IPC.GET_FACE_STATUS,
      start: IPC.START_FACE_ANALYSIS,
      pause: IPC.PAUSE_FACE_ANALYSIS,
      restart: IPC.RESTART_FACE_ANALYSIS,
      stop: IPC.STOP_FACE_ANALYSIS,
    },
    analyzedCount: () => status.value.processedItems,
    logTag: '[Face]',
    onError: (action, e) => {
      if (action === 'start' || action === 'restart') {
        // 与 AI 共用错误码，但人脸入口必须引导到独立的人脸模型卡片。
        if (parseAppError(e).code === 'AiModelNotLoaded') {
          void guideToModelDownload('face')
          return
        }
        useToastStore().addToast('error', ipcErrorMessage(e))
      } else {
        logger.error(`[Face] ${action} 分析出错 | analysis error`, { error: e })
      }
    },
  })
  const {
    fetchStatus,
    analyzeProgress,
    providerLabel,
    isWaitingBlocked,
    waitingForSession,
    startAnalysis,
    pauseAnalysis,
    restartAnalysis,
    stopAnalysis,
    maybeAutoResume,
  } = analysis

  /** 非破坏重试失败项（审查 F9）：Error→Pending 复位后走既有 start 复跑。
   *  与 restart（销毁本模型全部命名/确认）语义严格区分——这是零破坏的补跑通道。 */
  async function retryFailedItems() {
    try {
      const n = await invokeIpc<number>(IPC.RETRY_FAILED_FACE_ITEMS)
      if (n > 0) await startAnalysis()
      await fetchStatus()
    } catch (e) {
      useToastStore().addToast('error', ipcErrorMessage(e))
    }
  }

  /** 列出内置人脸模型轨+安装状态（F7 只读）。 */
  async function listFaceModels(): Promise<FaceModelInfo[]> {
    try {
      return await invokeIpc<FaceModelInfo[]>(IPC.LIST_FACE_MODEL_REGISTRY)
    } catch (e) {
      logger.error('[Face] listFaceModels failed', { error: e })
      return []
    }
  }

  /** 下载某人脸模型轨的 onnx（size+sha256 校验、断点续传），进度经 Channel 回传。
   *  仅可下载轨（默认 YuNet+SFace）成功；SCRFD/ArcFace 仅手动导入。 */
  function downloadFaceModel(
    profileId: string,
    onProgress: (p: FaceModelDownloadProgress) => void,
  ): Promise<void> {
    const ch = new Channel<FaceModelDownloadProgress>()
    ch.onmessage = onProgress
    return invokeIpc(IPC.DOWNLOAD_FACE_MODEL, { profileId, onProgress: ch })
  }

  return {
    // state
    status,
    // computed
    analyzeProgress,
    providerLabel,
    isWaitingBlocked,
    waitingForSession,
    // actions
    fetchStatus,
    startAnalysis,
    pauseAnalysis,
    restartAnalysis,
    stopAnalysis,
    maybeAutoResume,
    retryFailedItems,
    listFaceModels,
    downloadFaceModel,
  }
})
