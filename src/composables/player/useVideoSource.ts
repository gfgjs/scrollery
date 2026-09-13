// src/composables/player/useVideoSource.ts
// 视频格式扩展子系统 · 播放链路前端状态机(design.md §5.3)。
//
// 用途:据 itemId 调 `resolve_video_playback` 拿到 mode(direct/derived/preparing/
// needsComponent/needsHevcExt/needsConfirm),据 mode 驱动 ContentViewer 该给 VideoPlayer
// 喂什么 src、要不要挂 VideoPreparingOverlay。preparing 态订阅 VIDEO_PLAYBACK_PROGRESS 事件
// (Channel→事件+快照的缩略图进度先例,useTauriListen 免「await listen 卸载竞态」);挂载时先取
// 一次快照,防 webview 刷新丢进度。
//
// needsHevcExt 前端修正(design §5.1):host 只按容器判定,真能不能解码要前端用
// `navigator.mediaCapabilities.decodingInfo` 实测——能解就直接当 direct 播原文件,免去一次
// 不必要的转码等待;探测失败/API 缺失一律保守走引导态。

import { ref, computed, watch, type Ref } from 'vue'
import { useTauriListen } from '../useTauriListen'
import { invokeIpc, type IpcError } from '../../utils/ipc'
import { resolveAssetUrl } from '../../utils/assetUrl'
import { IPC, EVENTS } from '../../constants/ipc'

/** `resolve_video_playback` / `confirm_video_playback` 后端响应形状(camelCase,video_commands.rs)。 */
interface VideoPlaybackResolution {
  mode: 'direct' | 'derived' | 'preparing' | 'needsComponent' | 'needsHevcExt' | 'needsConfirm'
  src?: string
  estimateBytes?: number
  verdict?: string
  /** needsConfirm 态回显(V7 项6):本次 needsConfirm 是否由 force_transcode 覆写触发——前端据此
   * 决定下次 confirmProceed() 要不要继续携带 forceTranscode(粘性,不打回 needsHevcExt)。 */
  forceTranscode?: boolean
}

/** `video_playback_progress_snapshot` 响应形状(有在途 job 才非 null)。 */
interface VideoPlaybackProgressSnapshot {
  percent?: number
  stage: string
}

/** `VIDEO_PLAYBACK_PROGRESS` 事件载荷(video_commands.rs `PlaybackProgressPayload`)。 */
interface VideoPlaybackProgressPayload {
  itemId: number
  status: 'preparing' | 'ready' | 'error' | 'cancelled'
  src?: string
  code?: string
  percent?: number
  stage?: string
}

/** 前端消费态:在后端六态基础上补 `idle`(尚未解析/已取消待重触发)与 `error`(IPC 异常/终态失败)。 */
export type VideoSourceMode =
  | 'idle'
  | 'direct'
  | 'derived'
  | 'preparing'
  | 'needsComponent'
  | 'needsHevcExt'
  | 'needsConfirm'
  | 'error'
  /** 取消后的可重新准备态(V7 项5):与 idle 区分开,使覆盖层不留白、给出显式重触发入口。 */
  | 'cancelled'

/** 探测系统是否能硬解 HEVC(design §5.1 前端修正)。API 缺失/探测异常一律回退「不可解」(保守)。 */
async function probeHevcDecodable(): Promise<boolean> {
  try {
    const mc = (navigator as Navigator & { mediaCapabilities?: MediaCapabilities }).mediaCapabilities
    if (!mc) return false
    // 无法拿到源文件真实 profile/level/尺寸(host 只给了容器判定),用主流 HEVC Main 档一般配置探测——
    // 探测目的只是「系统装没装 HEVC 解码器」这一粗粒度问题,细档差异不影响该结论的可用性。
    const info = await mc.decodingInfo({
      type: 'file',
      video: {
        contentType: 'video/mp4; codecs="hvc1.1.6.L93.B0"',
        width: 1920,
        height: 1080,
        bitrate: 8_000_000,
        framerate: 30,
      },
    })
    return info.supported === true
  } catch {
    return false
  }
}

/**
 * 视频播放源解析状态机。`itemId` 为 getter(随查看项切换),内部 watch 自动重新解析并复位状态。
 * 必须在组件 `setup` 的同步上下文中调用(useTauriListen 的 onScopeDispose 要求)。
 */
export function useVideoSource(itemId: Ref<number | null> | (() => number | null)) {
  const getId = typeof itemId === 'function' ? itemId : () => itemId.value

  const mode = ref<VideoSourceMode>('idle')
  const resolvedSrc = ref<string | null>(null)
  const percent = ref<number | null>(null)
  const stage = ref<string | null>(null)
  const estimateBytes = ref<number | null>(null)
  const errorCode = ref<string | null>(null)
  const verdict = ref<string | null>(null)
  const downloadingComponent = ref(false)
  // needsHevcExt「改用本地转码」→needsConfirm 的粘性标记(V7 项6):记住本次流程是否携带
  // force_transcode,使 needsConfirm 态「继续生成」二次确认时不丢失 forceTranscode 意图。
  const pendingForceTranscode = ref(false)

  function resetState() {
    mode.value = 'idle'
    resolvedSrc.value = null
    percent.value = null
    stage.value = null
    estimateBytes.value = null
    errorCode.value = null
    verdict.value = null
    pendingForceTranscode.value = false
  }

  async function loadProgressSnapshot(id: number) {
    try {
      const snap = await invokeIpc<VideoPlaybackProgressSnapshot | null>(
        IPC.VIDEO_PLAYBACK_PROGRESS_SNAPSHOT,
        { itemId: id },
      )
      // 快照只是辅助恢复进度条展示;取回时查看项可能已切换,过期结果丢弃。
      if (getId() !== id || !snap) return
      // 仅在仍处于 preparing 且尚无进度时应用(V7 项3):防事件驱动已把 mode/percent 推进到
      // 更新状态后,迟到的快照又把它们回退,或覆盖掉已存在的更新值。
      if (mode.value !== 'preparing' || percent.value !== null) return
      percent.value = snap.percent ?? null
      stage.value = snap.stage
    } catch {
      // 快照失败不阻断播放准备(进度条只是缺失,preparing 态本身仍成立)。
    }
  }

  async function applyResolution(id: number, res: VideoPlaybackResolution) {
    if (getId() !== id) return // 查看项已切换,过期响应丢弃
    verdict.value = res.verdict ?? null
    // pendingForceTranscode 清零纪律:先按「非 needsConfirm 分支不残留」的默认假设清零,
    // 仅 needsConfirm 分支按 server 回显重新赋值——避免 force 流放弃/error 重试回到
    // needsHevcExt 后,上一轮 needsConfirm 遗留的 true 在下一轮被 confirmProceed 误读为粘性。
    pendingForceTranscode.value = false
    switch (res.mode) {
      case 'direct':
        mode.value = 'direct'
        resolvedSrc.value = res.src ? resolveAssetUrl(res.src) : null
        break
      case 'derived':
        mode.value = 'derived'
        resolvedSrc.value = res.src ? resolveAssetUrl(res.src) : null
        break
      case 'preparing':
        mode.value = 'preparing'
        resolvedSrc.value = null
        await loadProgressSnapshot(id)
        break
      case 'needsComponent':
        mode.value = 'needsComponent'
        resolvedSrc.value = null
        break
      case 'needsHevcExt': {
        // 前端实测修正(design §5.1):可硬解 → 直接当 direct 播原文件,不必等引导/转码。
        const decodable = res.src ? await probeHevcDecodable() : false
        if (getId() !== id) return // 探测耗时窗口内查看项可能已切换
        if (decodable && res.src) {
          mode.value = 'direct'
          resolvedSrc.value = resolveAssetUrl(res.src)
        } else {
          mode.value = 'needsHevcExt'
          resolvedSrc.value = null
        }
        break
      }
      case 'needsConfirm':
        mode.value = 'needsConfirm'
        estimateBytes.value = res.estimateBytes ?? null
        // server 回显本次是否由 force_transcode 触发(V7 项6):记下来,供 confirmProceed 粘性判断。
        pendingForceTranscode.value = res.forceTranscode ?? false
        resolvedSrc.value = null
        break
    }
  }

  async function doResolve(
    confirmed: boolean,
    forceTranscode = false,
    forceTranscodeConfirmed = false,
  ) {
    const id = getId()
    if (id == null) return
    try {
      const res = await invokeIpc<VideoPlaybackResolution>(
        confirmed ? IPC.CONFIRM_VIDEO_PLAYBACK : IPC.RESOLVE_VIDEO_PLAYBACK,
        confirmed ? { itemId: id, forceTranscode, forceTranscodeConfirmed } : { itemId: id },
      )
      await applyResolution(id, res)
    } catch (e) {
      if (getId() !== id) return
      mode.value = 'error'
      errorCode.value = (e as IpcError)?.code ?? 'Unknown'
    }
  }

  /** 事件驱动的 preparing → ready/error/cancelled 迁移(app 级广播,对重建后的 webview 照送)。 */
  useTauriListen<VideoPlaybackProgressPayload>(EVENTS.VIDEO_PLAYBACK_PROGRESS, (event) => {
    const payload = event.payload
    if (payload.itemId !== getId()) return // 非当前查看项的进度,忽略
    switch (payload.status) {
      case 'preparing':
        // 终态守卫(V7 项4):mode 已推进到 derived/direct(如已切到缓存命中的直接播放)后,
        // 迟到的 preparing 帧不得把 mode 拽回去、也不该覆盖已展示的正片。
        if (mode.value === 'derived' || mode.value === 'direct') break
        mode.value = 'preparing'
        if (payload.percent !== undefined) percent.value = payload.percent
        if (payload.stage !== undefined) stage.value = payload.stage
        break
      case 'ready':
        mode.value = 'derived'
        resolvedSrc.value = payload.src ? resolveAssetUrl(payload.src) : null
        percent.value = null
        stage.value = null
        break
      case 'error':
        mode.value = 'error'
        errorCode.value = payload.code ?? 'video_worker_failed'
        percent.value = null
        stage.value = null
        break
      case 'cancelled':
        // 用户主动取消(或另一处取消,V7 项5):不留白——转「可重新准备」态而非 idle,
        // VideoPreparingOverlay 据此显式给出「重新准备」入口(idle 在 ContentViewer 侧不挂
        // 覆盖层,会让视频区彻底空白)。
        mode.value = 'cancelled'
        percent.value = null
        stage.value = null
        break
    }
  })

  /**
   * 复调 confirm(=true)。两个调用场景共用(ContentViewer 的 `@confirm`/`@transcode-instead`
   * 均绑定本函数,按当前 mode 分流,免前端另接线):
   * - needsConfirm 且非 force_transcode 触发:单文件超池 50% 护栏放行,`forceTranscode` 恒 false。
   * - needsHevcExt:「改用本地转码」(D-444 ④)首次点击,携带 `forceTranscode=true`、
   *   `forceTranscodeConfirmed=false`——host 侧把这次 resolve 结果覆写为 Transcode,且仍按
   *   真实护栏判定一次(V7 项6:force_transcode 不得因走 confirm 命令而跳过超池 50% 判定)。
   * - needsConfirm 且由 force_transcode 触发(`pendingForceTranscode` 粘性,来自 host 回显):
   *   「继续生成」二次确认时继续携带 `forceTranscode=true` 且 `forceTranscodeConfirmed=true`——
   *   不能因为此刻 mode 已是 needsConfirm(不再是 needsHevcExt)而丢失 forceTranscode 意图,
   *   否则 host 会重新按原判定表得出 needsHevcExt,把用户打回引导页(而非放行转码)。
   */
  function confirmProceed() {
    const forceTranscode = mode.value === 'needsHevcExt' || pendingForceTranscode.value
    return doResolve(true, forceTranscode, pendingForceTranscode.value)
  }

  /** 硬取消在途/待产出的播放准备。 */
  async function cancel() {
    const id = getId()
    if (id == null) return
    try {
      await invokeIpc(IPC.CANCEL_VIDEO_PLAYBACK, { itemId: id })
    } catch {
      // 取消失败静默:cancelled 事件到达前 UI 仍显 preparing,用户可再次点取消。
    }
  }

  /** 触发下载 + 安装 FFmpeg 视频扩展组件;成功后自动重新解析。 */
  async function downloadComponent() {
    if (downloadingComponent.value) return
    // 进入时快照 id(项1):await 期间查看项可能已切换,成功/失败路径都不得砸新项的状态
    // (新项自己的 watch 早已接管解析,本次收尾只该管「本次点击对应的那个 id」)。
    const id = getId()
    downloadingComponent.value = true
    try {
      await invokeIpc(IPC.DOWNLOAD_VIDEO_COMPONENT)
      if (getId() !== id) return // 查看项已切换,不替新项重复触发 resolve
      await doResolve(false)
    } catch (e) {
      if (getId() !== id) return // 查看项已切换,error 不得覆盖新项当前状态
      mode.value = 'error'
      errorCode.value = (e as IpcError)?.code ?? 'Unknown'
    } finally {
      downloadingComponent.value = false
    }
  }

  /** 重新解析(error 态「重试」按钮专用;语义按后端 resolve 契约)。 */
  function retry() {
    return doResolve(false)
  }

  watch(
    getId,
    (id) => {
      resetState()
      if (id != null) void doResolve(false)
    },
    { immediate: true },
  )

  return {
    mode: computed(() => mode.value),
    src: computed(() => resolvedSrc.value),
    percent: computed(() => percent.value),
    stage: computed(() => stage.value),
    estimateBytes: computed(() => estimateBytes.value),
    errorCode: computed(() => errorCode.value),
    verdict: computed(() => verdict.value),
    downloadingComponent: computed(() => downloadingComponent.value),
    confirm: confirmProceed,
    cancel,
    downloadComponent,
    retry,
  }
}

export type UseVideoSourceReturn = ReturnType<typeof useVideoSource>
