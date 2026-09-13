// src/composables/useOcr.ts
// OCR 文字提取(B′ 路线,2026-07-23,T9):门控三分支 + 图片/视频帧提取 + 结果面板状态。
//
// 模块级单例状态(先例 useExoticGate.ts:17-40 的模块缓存):busy/panelOpen/result/sourceLabel
// 定义在模块作用域,ContentViewer(图片入口)与 VideoPlayer(视频帧入口)各自调用 useOcr() 时
// 共享同一份状态——两态触发的结果自然落在同一个 OcrResultPanel 单例上,不需跨组件搭桥。
//
// 门控:先查 ocr_status(60s TTL 缓存,resetOcrStatusCache 供下载/激活成功后失效)——
// 未授权/过期 → toast + 跳插件商店;模型未装 → toast + 跳设置页;通过才发起提取命令。
// 提取命令若仍返回门控类错误码(status 与实际操作之间的竞态,如授权刚过期),按 code 兜底分流,
// 不假设 ensureGate 通过后命令必然成功。

import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { IPC } from '../constants/ipc'
import { invokeIpc, ipcErrorMessage, IpcError } from '../utils/ipc'
import { useToastStore } from '../stores/toastStore'
import { logger } from '../utils/logger'
import { blobToBase64 } from './player/useVideoFrameCapture'
import type { OcrResult, OcrStatus } from '../types/ocr'

const STATUS_TTL_MS = 60_000

let statusCache: OcrStatus | null = null
let statusCacheAt = 0
let statusInflight: Promise<OcrStatus> | null = null
/**
 * 缓存代际计数:resetOcrStatusCache 自增,令在途 loadOcrStatus 请求的回填/清理作废。
 * 无此计数则「reset 时已有旧请求在飞」的场景下,旧请求 resolve 后会把 statusCache 重新
 * 写回旧值(下载/激活成功后 60s 内门控仍误判缺失/未授权),且其 finally 会清掉此后新发起
 * 请求的 statusInflight 引用,致重复 IPC。
 */
let statusGen = 0

/** 清空 ocr_status 缓存(模型下载完成 / 插件激活成功后调用,供下次门控判定拿到新态)。 */
export function resetOcrStatusCache(): void {
  statusCache = null
  statusCacheAt = 0
  statusInflight = null
  statusGen++
}

async function loadOcrStatus(): Promise<OcrStatus> {
  const now = Date.now()
  if (statusCache && now - statusCacheAt < STATUS_TTL_MS) return statusCache
  if (!statusInflight) {
    const myGen = statusGen
    statusInflight = invokeIpc<OcrStatus>(IPC.OCR_STATUS)
      .then((status) => {
        if (myGen === statusGen) {
          statusCache = status
          statusCacheAt = Date.now()
        }
        return status
      })
      .finally(() => {
        if (myGen === statusGen) {
          statusInflight = null
        }
      })
  }
  return statusInflight
}

// 模块级单例状态
const busy = ref(false)
const panelOpen = ref(false)
const result = ref<OcrResult | null>(null)
/** 面板标题用:图片文件名 / 视频帧来源文件名。 */
const sourceLabel = ref('')
/**
 * 单调请求序号:每次 extract 起始自增取号,closePanel 也自增(令在途请求过期)。
 * invoke 返回后若取号已不等于当前值,说明面板已被关闭/被更晚一次提取取代,结果作废
 * (不开面板、不 toast)——防迟到响应把已关闭的面板重新弹开,或把旧结果盖过新触发。
 */
let reqSeq = 0

export function useOcr() {
  const { t } = useI18n()
  const router = useRouter()
  const toast = useToastStore()

  /** 门控三分支之「未授权/未装模型」两支;返回 true 表示可继续发起提取。 */
  async function ensureGate(): Promise<boolean> {
    let status: OcrStatus
    try {
      status = await loadOcrStatus()
    } catch (e) {
      toast.addToast('error', ipcErrorMessage(e))
      logger.error('[useOcr] ocr_status failed', { error: e })
      return false
    }
    if (status.availability !== 'authorized') {
      toast.addToast(
        'info',
        status.availability === 'licenseExpired' ? t('ocr.licenseExpired') : t('ocr.unlicensed'),
      )
      void router.push('/plugins')
      return false
    }
    const tier = status.tiers.find((tr) => tr.id === status.activeTier)
    if (!tier?.manifestReady || !tier?.installed) {
      toast.addToast('info', t('ocr.modelMissing'))
      void router.push('/settings')
      return false
    }
    return true
  }

  /** 提取命令失败按稳定 code 分流(ipcErrorMessage 惯例,aiStore.ts 同款用法);兜底走通用错误 toast。 */
  function handleExtractError(e: unknown): void {
    const code = e instanceof IpcError ? e.code : null
    switch (code) {
      case 'ocr_unlicensed':
      case 'ocr_unavailable':
        toast.addToast('info', t('ocr.unlicensed'))
        void router.push('/plugins')
        break
      case 'ocr_license_expired':
        toast.addToast('info', t('ocr.licenseExpired'))
        void router.push('/plugins')
        break
      case 'ocr_model_missing':
      case 'ocr_manifest_unready':
        toast.addToast('info', t('ocr.modelMissing'))
        void router.push('/settings')
        break
      default:
        toast.addToast('error', ipcErrorMessage(e))
    }
    logger.error('[useOcr] extract failed', { error: e, code })
  }

  function applyResult(res: OcrResult, label: string): void {
    if (res.lines.length === 0) {
      toast.addToast('info', t('ocr.empty'))
      return
    }
    result.value = res
    sourceLabel.value = label
    panelOpen.value = true
  }

  /**
   * 画廊图片 OCR 提取。busy 在门控判定前**同步**置位(而非等 ensureGate resolve 后才置),
   * 保证连续两次点击(第二次发生在第一次的 await 让出之后、状态更新之前)也被互斥拦下。
   */
  async function extractFromImage(itemId: number, fileName: string): Promise<void> {
    if (busy.value) return
    busy.value = true
    const myReq = ++reqSeq
    try {
      if (!(await ensureGate())) return
      // 边界8:AI worker 严格串行,OCR 最坏排在 EmbedBatch(120s 上限)后——按钮禁用不够反馈,补 toast。
      // 挪到门控通过之后:门控失败已有专属 toast,提前弹会双弹。
      toast.addToast('info', t('ocr.extracting'))
      const res = await invokeIpc<OcrResult>(IPC.OCR_EXTRACT_IMAGE, { itemId })
      if (myReq !== reqSeq) return // 迟到结果:面板已关闭/已被更晚一次提取取代,作废
      applyResult(res, fileName)
    } catch (e) {
      handleExtractError(e)
    } finally {
      busy.value = false
    }
  }

  /**
   * 视频当前帧 OCR 提取。canvas 截帧段逐行复用 useVideoFrameCapture 的 drawImage/toBlob/
   * SecurityError 防御(同一前提:`<video crossorigin="anonymous">` 随 src 渲染,正常不会污染画布)。
   * busy 置位时机同 extractFromImage(门控判定前同步置位)。
   */
  async function extractFromVideoFrame(videoEl: HTMLVideoElement, fileName: string): Promise<void> {
    if (busy.value || !videoEl) return
    const width = videoEl.videoWidth
    const height = videoEl.videoHeight
    if (!width || !height) return
    busy.value = true
    const myReq = ++reqSeq
    try {
      if (!(await ensureGate())) return
      // 边界8:门控通过之后再弹排队提醒,门控失败已有专属 toast,提前弹会双弹。
      toast.addToast('info', t('ocr.extracting'))
      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height
      const ctx = canvas.getContext('2d')
      if (!ctx) {
        toast.addToast('error', t('player.captureFailed'))
        return
      }

      let blob: Blob | null
      try {
        ctx.drawImage(videoEl, 0, 0, width, height)
        blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
      } catch (e) {
        // 跨源画布污染:drawImage/toBlob 均可能同步抛 SecurityError(见 useVideoFrameCapture 头注释)。
        if (e instanceof DOMException && e.name === 'SecurityError') {
          toast.addToast('error', t('player.captureTainted'))
          logger.error('[useOcr] tainted canvas (SecurityError)', { error: e })
          return
        }
        throw e
      }
      if (!blob) {
        toast.addToast('error', t('player.captureFailed'))
        return
      }

      const dataBase64 = await blobToBase64(blob)
      const res = await invokeIpc<OcrResult>(IPC.OCR_EXTRACT_FRAME, { dataBase64 })
      if (myReq !== reqSeq) return // 迟到结果:面板已关闭/已被更晚一次提取取代,作废
      applyResult(res, fileName)
    } catch (e) {
      handleExtractError(e)
    } finally {
      busy.value = false
    }
  }

  /** 复制全部识别文本到剪贴板。**仅用户点击才写剪贴板**(契约:不自动覆写)。 */
  async function copyAll(): Promise<void> {
    if (!result.value) return
    try {
      await navigator.clipboard.writeText(result.value.lines.map((line) => line.text).join('\n'))
      toast.addToast('success', t('ocr.copied'))
    } catch (e) {
      toast.addToast('error', t('ocr.failed'))
      logger.error('[useOcr] copyAll failed', { error: e })
    }
  }

  /**
   * 关闭面板并清空结果(深审裁决:防止切项/卸载后旧结果闪现——不只翻 panelOpen)。
   * 同时使在途请求过期(reqSeq++)——关闭后才返回的提取结果不得把面板重新弹开。
   */
  function closePanel(): void {
    reqSeq++
    panelOpen.value = false
    result.value = null
    sourceLabel.value = ''
  }

  return {
    busy,
    panelOpen,
    result,
    sourceLabel,
    extractFromImage,
    extractFromVideoFrame,
    copyAll,
    closePanel,
  }
}
