// src/composables/useContentViewerQuickActions.ts
// 从 ContentViewer.vue 下沉(超长文件拆分方案 2.1):收藏/评分/色标/资源管理器定位/Live Photo。
// 纯逻辑搬迁,不改变原调用点/参数/时序——见 docs/planning/2026-07-25-超长文件拆分方案/analysis/ContentViewer-vue.md。

import { invokeIpc, type IpcError } from '../utils/ipc'
import { logger } from '../utils/logger'
import { isMobilePlatform } from '../utils/platform'
import { resolveAssetUrl } from '../utils/assetUrl'
import { IPC } from '../constants/ipc'
import type { MediaDetail } from '../types/media'
import type { useMediaStore } from '../stores/mediaStore'
import type { useToastStore } from '../stores/toastStore'
import type { useMediaDetail } from './useMediaDetail'

export function useContentViewerQuickActions(options: {
  detail: () => MediaDetail | null
  media: ReturnType<typeof useMediaStore>
  toast: ReturnType<typeof useToastStore>
  t: (key: string, params?: Record<string, unknown>) => string
  state: ReturnType<typeof useMediaDetail>
}) {
  const { detail, media, toast, t, state } = options

  async function toggleFav() {
    const target = detail()
    if (!target) return
    const newVal = await media.toggleFavorite(target.id)
    // IPC 返回前用户可能已翻到另一项；旧结果不得写进新详情。
    if (detail()?.id === target.id) detail()!.isFavorited = newVal
  }

  async function setRating(n: number) {
    const target = detail()
    if (!target) return
    // 点当前星 = 清零(toggle-off)。next 算一次供 DB 写入与本地回写共用——
    // 此前本地恒写 n,点当前星清零后 UI 仍显 n(与 DB 不一致),一并修正。
    const next = n === target.rating ? 0 : n
    await media.setRating(target.id, next)
    if (detail()?.id === target.id) detail()!.rating = next
  }

  async function setColorLabel(value: number) {
    const target = detail()
    if (!target) return
    // ColorLabelPicker 的 @change 已吐出 toggle 后的值(点当前色→0),直接持久化即可,
    // 与工具栏批量设色 / 按色筛选同一语义。乐观回写 store 响应对象即时刷新色块。
    await media.setColorLabel(target.id, value)
    if (detail()?.id === target.id) detail()!.colorLabel = value
  }

  async function showInExplorer() {
    const target = detail()
    if (!target) return
    // 移动端 opener 不支持 reveal(D-001/R-09):不发起;两处入口按钮已 v-if 隐藏,此为兜底。
    if (isMobilePlatform) return
    // reveal 统一到 opener 后(F-015)后端会校验路径存在并同步等结果,「文件已被外部删除」
    // 成了常见真实失败;裸 await 会变成 unhandled rejection——用户只看到「点了没反应」。
    try {
      await invokeIpc(IPC.SHOW_IN_EXPLORER, { itemId: target.id })
    } catch (err) {
      if (detail()?.id !== target.id) return
      logger.error('show_in_explorer failed', { error: err })
      // 按稳定 code 分流(R-08):unsupported_platform 给「平台不支持」而非「失败请重试」。
      const code = (err as { code?: string } | null)?.code
      toast.addToast(
        'error',
        t(
          code === 'unsupported_platform'
            ? 'contextMenu.revealUnsupported'
            : 'contextMenu.showInExplorerFailed',
        ),
      )
    }
  }

  async function toggleLive() {
    const target = detail()
    if (!target) return
    if (state.isPlayingLive.value) {
      state.isPlayingLive.value = false
      state.liveVideoSrc.value = null
    } else {
      try {
        // 走 invokeIpc(而非裸 invoke)以拿到结构化 IpcError.code——据此分流卷离线。
        const path = await invokeIpc<string>(IPC.GET_COMPANION_VIDEO_URL, { itemId: target.id })
        if (detail()?.id !== target.id) return
        state.liveVideoSrc.value = resolveAssetUrl(path)
        state.isPlayingLive.value = true
      } catch (e) {
        if (detail()?.id !== target.id) return
        // 卷离线(T13):后端返 VolumeOffline{message=卷标签} → 提示「请插入设备 <label>」而非泛化错误。
        const err = e as IpcError
        if (err?.code === 'VolumeOffline') {
          toast.addToast('error', t('detail.volumeOffline', { label: err.message }))
        } else {
          toast.addToast('error', t('detail.livePhotoError'))
        }
      }
    }
  }

  return { toggleFav, setRating, setColorLabel, showInExplorer, toggleLive }
}
