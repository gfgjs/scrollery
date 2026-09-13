// src/composables/player/useVideoFrameCapture.ts
// 截帧(播放器线 GE③,计划 §裁决「截帧:不 taint(有条件)」):drawImage(videoWidth×videoHeight
// 原生尺寸,不含 viewRotation——截帧取「拍摄内容」像素,非查看器展示朝向)→ canvas.toBlob('image/png')
// → plugin-dialog save() 选定落盘位置 → blob→base64 → SAVE_FRAME_PNG IPC(后端 `*.tmp` 同卷 rename)。
//
// 前提:`<video crossorigin="anonymous">` 随 src 一同渲染(VideoPlayer.vue),asset protocol 对
// window origin 发 ACAO(实证见计划 §裁决),故正常情况下画布不被污染。SecurityError 仍防御性
// 兜底(macOS WKWebView 未实证)。

import { ref, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { save } from '@tauri-apps/plugin-dialog'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { useToastStore } from '../../stores/toastStore'
import { logger } from '../../utils/logger'

/** 秒数 → "hh-mm-ss"(截帧默认文件名用;恒三段两位,不因 <1h 省略小时段)。 */
function hhmmss(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(h)}-${pad(m)}-${pad(s)}`
}

/** 去掉文件名的扩展名(截帧默认名取原视频 basename 用)。 */
function stripExtension(fileName: string): string {
  return fileName.replace(/\.[^./\\]+$/, '')
}

/** Blob → 标准 base64(不含 `data:...;base64,` 前缀)。导出供 useOcr(视频帧提取)复用。 */
export function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onloadend = () => {
      const result = reader.result
      if (typeof result !== 'string') {
        reject(new Error('FileReader did not produce a data URL'))
        return
      }
      const idx = result.indexOf(',')
      resolve(idx >= 0 ? result.slice(idx + 1) : result)
    }
    reader.onerror = () => reject(reader.error ?? new Error('FileReader failed'))
    reader.readAsDataURL(blob)
  })
}

/**
 * @param videoEl 播放器内部 `<video>` 元素
 * @param fileName 当前条目的原始文件名(取 basename 拼截帧默认文件名用)
 */
export function useVideoFrameCapture(videoEl: Ref<HTMLVideoElement | null>, fileName: Ref<string>) {
  const { t } = useI18n()
  const toast = useToastStore()
  /** 截帧进行中(防抖;VideoControlBar 据此置按钮 disabled,边界 13)。 */
  const busy = ref(false)

  async function captureFrame(): Promise<void> {
    const el = videoEl.value
    if (!el || busy.value) return
    const width = el.videoWidth
    const height = el.videoHeight
    if (!width || !height) return
    busy.value = true
    try {
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
        ctx.drawImage(el, 0, 0, width, height)
        blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
      } catch (e) {
        // 跨源画布污染:toBlob/drawImage 均可能同步抛 SecurityError(理论不可达,见文件头注释,
        // 仍作纵深防御)。
        if (e instanceof DOMException && e.name === 'SecurityError') {
          toast.addToast('error', t('player.captureTainted'))
          logger.error('[useVideoFrameCapture] tainted canvas (SecurityError)', { error: e })
          return
        }
        throw e
      }
      if (!blob) {
        toast.addToast('error', t('player.captureFailed'))
        return
      }

      const defaultPath = `${stripExtension(fileName.value)}_${hhmmss(el.currentTime)}.png`
      const target = await save({
        defaultPath,
        filters: [{ name: 'PNG', extensions: ['png'] }],
      })
      if (!target) return // 用户取消对话框:静默(边界 13)

      const dataBase64 = await blobToBase64(blob)
      await invokeIpc<void>(IPC.SAVE_FRAME_PNG, { targetPath: target, dataBase64 })
      toast.addToast('success', t('player.captureSaved'))
    } catch (e) {
      toast.addToast('error', t('player.captureFailed'))
      logger.error('[useVideoFrameCapture] capture failed', { error: e })
    } finally {
      busy.value = false
    }
  }

  return { busy, captureFrame }
}
