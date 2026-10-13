import i18n from '../i18n'
import { useToastStore } from '../stores/toastStore'
import { logger } from '../utils/logger'

const MODEL_DOWNLOAD_TARGETS = {
  ai: { card: 'modelLibrary', message: 'settings.modelDownloadGuideAi' },
  face: { card: 'faceModels', message: 'settings.modelDownloadGuideFace' },
  ocr: { card: 'ocrModels', message: 'settings.modelDownloadGuideOcr' },
} as const

/** 主动使用功能但缺少模型时，定位下载卡片并说明下一步，由用户选择下载。 */
export async function guideToModelDownload(
  kind: keyof typeof MODEL_DOWNLOAD_TARGETS,
): Promise<void> {
  const target = MODEL_DOWNLOAD_TARGETS[kind]
  const toast = useToastStore()
  try {
    // Store 也会调用此引导；按需取得路由，避免在 Store 加载时创建浏览器历史。
    const { default: router } = await import('../router')
    await router.push({ path: '/settings/ai', hash: `#${target.card}` })
    toast.addToast('info', i18n.global.t(target.message), 8000)
  } catch (error) {
    logger.error('[Models] 跳转模型下载失败 | Navigate to model download failed', { error })
  }
}
