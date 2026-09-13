// src/composables/player/useVideoSubtitles.ts
// 字幕(播放器线 GE②):内挂(video.textTracks 枚举,单选 showing/disabled)+ 外挂(LOAD_VIDEO_SUBTITLE
// IPC → looksLikeVtt/srtToVtt → Blob URL → 动态 <track>)。视频打开时自动探测同名 sidecar,命中即
// 自动启用(桌面播放器惯例);not_found 静默,其余错误 toast。ass/ssa/mkv 内嵌轨的「不支持」文案
// 由纯 UI 层(VideoSubtitleMenu)固定展示,本 composable 不做格式判定。
//
// 竞态(边界 8):切条目/重复加载均递增 loadToken,迟到的旧 IPC 应答按 token 丢弃(镜像
// ContentViewer.vue:916 faceToken 模式)。

import { ref, watch, onBeforeUnmount, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { invokeIpc, IpcError } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { useToastStore } from '../../stores/toastStore'
import { looksLikeVtt, srtToVtt } from '../../utils/srtToVtt'
import { logger } from '../../utils/logger'

/** 字幕自动探测的稳定错误码(镜像后端 player_commands.rs 的 CODE_SUBTITLE_NOT_FOUND)。 */
const CODE_SUBTITLE_NOT_FOUND = 'player_subtitle_not_found'

export interface SubtitleOption {
  /** 稳定 key:`embedded-${textTracks 索引}`。 */
  key: string
  label: string
}

interface SubtitleFileResponse {
  fileName: string
  content: string
}

export function useVideoSubtitles(videoEl: Ref<HTMLVideoElement | null>, itemId: Ref<number | null>) {
  const { t } = useI18n()
  const toast = useToastStore()

  /** 内挂轨选项(不含本 composable 动态挂的外挂 <track>)。 */
  const embeddedTracks = ref<SubtitleOption[]>([])
  /** 已加载的外挂字幕文件名;null=未加载。 */
  const externalLabel = ref<string | null>(null)
  /** 当前选中项:'off' | `embedded-${idx}` | 'external'。 */
  const activeKey = ref<string>('off')

  let externalTrackEl: HTMLTrackElement | null = null
  let externalBlobUrl: string | null = null
  let loadToken = 0

  function cleanupExternal(): void {
    if (externalBlobUrl) {
      URL.revokeObjectURL(externalBlobUrl)
      externalBlobUrl = null
    }
    if (externalTrackEl) {
      externalTrackEl.remove()
      externalTrackEl = null
    }
    externalLabel.value = null
  }

  /** 重新枚举 `<video>` 的内挂 textTracks(排除本 composable 动态挂的外挂轨)。 */
  function refreshEmbeddedTracks(): void {
    const el = videoEl.value
    if (!el) {
      embeddedTracks.value = []
      return
    }
    const opts: SubtitleOption[] = []
    for (let i = 0; i < el.textTracks.length; i++) {
      const tt = el.textTracks[i]
      if (externalTrackEl && tt === externalTrackEl.track) continue
      opts.push({
        key: `embedded-${i}`,
        label: tt.label || tt.language || t('player.subtitleTrack', { n: i + 1 }),
      })
    }
    embeddedTracks.value = opts
  }

  /** 单选施加:目标轨 mode='showing',其余(含外挂)'disabled'。 */
  function applySelection(key: string): void {
    const el = videoEl.value
    if (!el) return
    activeKey.value = key
    for (let i = 0; i < el.textTracks.length; i++) {
      const tt = el.textTracks[i]
      const isExternalTrack = !!externalTrackEl && tt === externalTrackEl.track
      const matches = isExternalTrack ? key === 'external' : key === `embedded-${i}`
      tt.mode = matches ? 'showing' : 'disabled'
    }
  }

  /** 用户在菜单选中某内挂/外挂轨。 */
  function selectTrack(key: string): void {
    applySelection(key)
  }

  /** 关闭字幕(单选「关」项)。 */
  function turnOff(): void {
    applySelection('off')
  }

  /** 加载外挂字幕(`path=null`=同目录同名 sidecar 自动探测;`path` 非空=用户对话框选中的路径)。 */
  async function loadExternal(path: string | null): Promise<void> {
    const id = itemId.value
    if (id == null) return
    const myToken = ++loadToken
    try {
      const file = await invokeIpc<SubtitleFileResponse>(IPC.LOAD_VIDEO_SUBTITLE, {
        itemId: id,
        path: path ?? undefined,
      })
      if (myToken !== loadToken) return // 迟到应答(已切条目/已发起新加载),丢弃
      const el = videoEl.value
      if (!el) return
      const vtt = looksLikeVtt(file.content) ? file.content : srtToVtt(file.content)
      const url = URL.createObjectURL(new Blob([vtt], { type: 'text/vtt' }))
      cleanupExternal()
      const track = document.createElement('track')
      track.kind = 'subtitles'
      track.label = file.fileName
      track.src = url
      el.appendChild(track)
      externalTrackEl = track
      externalBlobUrl = url
      externalLabel.value = file.fileName
      refreshEmbeddedTracks()
      // 桌面播放器惯例:加载成功即自动启用展示。
      applySelection('external')
    } catch (e) {
      if (myToken !== loadToken) return
      const code = e instanceof IpcError ? e.code : 'Unknown'
      if (code === CODE_SUBTITLE_NOT_FOUND) return // 自动探测未命中是常态,静默
      toast.addToast('error', t('player.subtitleLoadFailed'))
      logger.error('[useVideoSubtitles] load failed', { error: e, path })
    }
  }

  /** 打开视频/切条目:清残留外挂轨态 + 自动探测同名 sidecar。 */
  async function autoDetectForItem(): Promise<void> {
    loadToken++ // 作废在途的旧条目请求
    cleanupExternal()
    activeKey.value = 'off'
    refreshEmbeddedTracks()
    await loadExternal(null)
  }

  /** 「加载字幕文件…」:用户经 plugin-dialog 选中路径后调用。 */
  async function loadFromPath(path: string): Promise<void> {
    await loadExternal(path)
  }

  watch(videoEl, (el, prev) => {
    if (prev) prev.textTracks.removeEventListener('addtrack', refreshEmbeddedTracks)
    if (el) el.textTracks.addEventListener('addtrack', refreshEmbeddedTracks)
    refreshEmbeddedTracks()
  })

  watch(itemId, () => {
    // 条目切换时立即清理旧条目的外挂轨,不依赖 loadedmetadata 事件
    // (新条目加载可能在 loadedmetadata 前报错,导致旧外挂轨滞留)
    cleanupExternal()
    activeKey.value = 'off'
  })

  onBeforeUnmount(() => {
    loadToken++
    if (videoEl.value) videoEl.value.textTracks.removeEventListener('addtrack', refreshEmbeddedTracks)
    cleanupExternal()
  })

  return {
    embeddedTracks,
    externalLabel,
    activeKey,
    selectTrack,
    turnOff,
    loadFromPath,
    autoDetectForItem,
  }
}
