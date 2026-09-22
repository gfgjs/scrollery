// src/stores/enhanceStore.ts
// 影像增强 store（降噪/超分子系统 P0 批 5）——照 aiStore 形态（defineStore + 常规 ref，无大数组）。
//
// 职责：门控/模型态查询、模型下载（Channel 进度）+ 删除、队列拉取 + `enhance:queue-changed`
// 事件订阅刷新、enhance_start/enhance_cancel、参数记忆（本批内存态）、错误码→用户可读消息映射。
// 授权真相全在后端（enhance_start 有后端真门）；本 store 只做薄封装 + 门控 UX 引导所需的状态镜像。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC, EVENTS } from '../constants/ipc'
import i18n from '../i18n'
import { useToastStore } from './toastStore'
import type {
  EnhanceStatus,
  EnhanceJob,
  EnhanceJobStatus,
  EnhanceParams,
  EnhanceModelTask,
  EnhanceStepTask,
  EnhancePreviewResult,
  ModelDownloadProgress,
} from '../types/enhance'

/** 每任务默认档（design.md §F：降噪=scunet、超分=realesrgan-x4plus、去伪影=fbcnn）。 */
export const ENHANCE_DEFAULT_MODEL: Record<EnhanceModelTask, string> = {
  denoise: 'scunet',
  dejpegArtifact: 'fbcnn',
  upscale: 'realesrgan-x4plus',
}

/** 模型列表口径 task（camelCase）→ 协议 step 口径 task（snake_case）。见 types/enhance.ts 头注差异 1。 */
export function modelTaskToStepTask(task: EnhanceModelTask): EnhanceStepTask {
  return task === 'dejpegArtifact' ? 'dejpeg_artifact' : task
}

/** 稳定错误码 → 用户可读消息的 i18n key（未知码回退通用）。纯函数，供 store/组件/单测共用。 */
export function enhanceErrorMessageKey(code: string | null | undefined): string {
  switch (code) {
    case 'enhance_input_too_large':
      return 'enhance.errInputTooLarge'
    case 'enhance_unlicensed':
      return 'enhance.errUnlicensed'
    case 'enhance_input_unsupported':
      return 'enhance.errInputUnsupported'
    case 'enhance_model_missing':
      return 'enhance.errModelMissing'
    case 'enhance_worker_missing':
      return 'enhance.errWorkerMissing'
    case 'enhance_manifest_unready':
      return 'settings.enhanceManifestUnready'
    case 'enhance_status_unavailable':
      return 'enhance.errStatusUnavailable'
    case 'enhance_busy':
      return 'enhance.errBusy'
    case 'enhance_download_failed':
      return 'enhance.errDownloadFailed'
    case 'enhance_io':
      return 'enhance.errIo'
    case 'enhance_invalid_params':
      return 'enhance.errInvalidParams'
    case 'enhance_not_implemented':
      return 'enhance.errNotImplemented'
    default:
      return 'enhance.errUnknown'
  }
}

export const useEnhanceStore = defineStore('enhance', () => {
  // ── State ─────────────────────────────────────────────────────────────────
  const status = ref<EnhanceStatus | null>(null)
  const queue = ref<EnhanceJob[]>([])
  /** 参数记忆（本批内存态；settings 持久化 enhance_last_params 归 4.5 批，后端 keys 未注册）。 */
  const lastParams = ref<EnhanceParams | null>(null)

  // ── Computed ──────────────────────────────────────────────────────────────
  const isAuthorized = computed(() => status.value?.availability === 'authorized')
  /** 存在运行中/排队中的 job（入口按钮/面板据此提示）。 */
  const hasActiveJob = computed(() =>
    queue.value.some((j) => j.status === 'queued' || j.status === 'running'),
  )

  /** 某任务可选的模型档（按 task 过滤 status.models）。 */
  function modelsForTask(task: EnhanceModelTask) {
    return status.value?.models.filter((m) => m.task === task) ?? []
  }

  /** 按 id 查模型档（预计输出尺寸/安装态判定用）。 */
  function modelById(id: string) {
    return status.value?.models.find((m) => m.id === id) ?? null
  }

  // ── Actions ───────────────────────────────────────────────────────────────

  let statusGeneration = 0
  /** 授权变更后的新查询优先，旧响应不能恢复已被移除的授权展示。 */
  async function fetchStatus(): Promise<EnhanceStatus | null> {
    const current = ++statusGeneration
    status.value = null
    try {
      const result = await invokeIpc<EnhanceStatus>(IPC.ENHANCE_STATUS)
      if (current === statusGeneration) status.value = result
    } catch (e) {
      if (current === statusGeneration) {
        status.value = null
        logger.error('[Enhance] 拉取状态失败 | fetch status', { error: e })
      }
    }
    return status.value
  }

  /** 下载某增强模型档（Channel 流式进度；复用既有 DownloadProgress 形态）。 */
  function downloadModel(
    modelId: string,
    onProgress: (p: ModelDownloadProgress) => void,
  ): Promise<void> {
    const ch = new Channel<ModelDownloadProgress>()
    ch.onmessage = onProgress
    return invokeIpc(IPC.DOWNLOAD_ENHANCE_MODEL, { modelId, onProgress: ch })
  }

  /** 删除某增强模型档（两文件；幂等）。 */
  async function deleteModel(modelId: string): Promise<void> {
    await invokeIpc(IPC.DELETE_ENHANCE_MODEL, { modelId })
  }

  /**
   * job 上次已知状态（jobId → status）：`fetchQueue` 据此检测「非终态→终态」跃迁以弹 toast
   * （批 5.5）。只在有活跃订阅者时弹（`subscriberCount` 守卫）——否则设置页/对话框都未挂载时
   * 静默拉取（如 `start()` 后立即回拉一次）不该弹重复/滞后的 toast。
   */
  const lastKnownStatus = new Map<number, EnhanceJobStatus>()

  /** 检测本次快照相对上次的「非终态→终态」跃迁并弹 toast；随后更新已知状态表。 */
  function notifyTerminalTransitions(next: EnhanceJob[]): void {
    for (const job of next) {
      const prev = lastKnownStatus.get(job.id)
      const wasActive = prev === 'queued' || prev === 'running'
      const isTerminal = job.status === 'done' || job.status === 'error'
      if (wasActive && isTerminal && subscriberCount > 0) {
        const toast = useToastStore()
        if (job.status === 'done') {
          toast.addToast('success', i18n.global.t('enhance.toastJobDone', { id: job.id, n: job.total }))
        } else {
          toast.addToast('error', i18n.global.t(enhanceErrorMessageKey(job.errorCode)))
        }
      }
      lastKnownStatus.set(job.id, job.status)
    }
  }

  /** 队列全量快照拉取（事件触发或进面板时调）。 */
  async function fetchQueue(): Promise<void> {
    try {
      const next = await invokeIpc<EnhanceJob[]>(IPC.GET_ENHANCE_QUEUE)
      notifyTerminalTransitions(next)
      queue.value = next
    } catch (e) {
      logger.error('[Enhance] 拉取队列失败 | fetch queue', { error: e })
    }
  }

  /** 前后对比预览；错误上抛，由调用方展示占位态。 */
  async function preview(itemId: number, params: EnhanceParams): Promise<EnhancePreviewResult> {
    return await invokeIpc<EnhancePreviewResult>(IPC.ENHANCE_PREVIEW, { itemId, params })
  }

  /** 发起增强（多选入队，顺序执行）：返回 job_id，并记住参数供下次预选。错误上抛供调用方分流。 */
  async function start(itemIds: number[], params: EnhanceParams): Promise<number> {
    const jobId = await invokeIpc<number>(IPC.ENHANCE_START, { itemIds, params })
    lastParams.value = params
    await fetchQueue()
    return jobId
  }

  /** 取消某 job（后端置 token → 在途请求醒来即中止）。 */
  async function cancel(jobId: number): Promise<void> {
    await invokeIpc(IPC.ENHANCE_CANCEL, { jobId })
    await fetchQueue()
  }

  // ── 队列事件订阅（进程内长驻；订阅即拉一次全量对齐初值）───────────────────────
  // 计数守卫（批 5.5）：多消费方（设置分节面板 + 对话框内面板）可能同时挂载；`unsubscribeQueue`
  // 只在最后一个订阅者退出时才真正 unlisten，防止某一方卸载（如关 Dialog）打断另一方仍在用的
  // 监听——同时也是终态 toast「仅在有订阅者时活跃」的判据（`subscriberCount`）。
  let unlisten: UnlistenFn | null = null
  let subscribing = false
  let subscriberCount = 0

  async function subscribeQueue(): Promise<void> {
    subscriberCount += 1
    if (unlisten || subscribing) {
      await fetchQueue()
      return
    }
    subscribing = true
    try {
      unlisten = await listen(EVENTS.ENHANCE_QUEUE_CHANGED, () => void fetchQueue())
    } finally {
      subscribing = false
    }
    await fetchQueue()
  }

  function unsubscribeQueue(): void {
    subscriberCount = Math.max(0, subscriberCount - 1)
    if (subscriberCount === 0) {
      unlisten?.()
      unlisten = null
      // 裁决「不滞后补报」：0 订阅期间完成的 job 不该在下次重挂载订阅时补爆终态 toast——
      // toast 只报「在场期间」发生的跃迁，离场后的终态变化交给队列面板终态行本身呈现。
      lastKnownStatus.clear()
    }
  }

  return {
    // state
    status,
    queue,
    lastParams,
    // computed
    isAuthorized,
    hasActiveJob,
    // helpers
    modelsForTask,
    modelById,
    // actions
    fetchStatus,
    downloadModel,
    deleteModel,
    fetchQueue,
    preview,
    start,
    cancel,
    subscribeQueue,
    unsubscribeQueue,
  }
})
