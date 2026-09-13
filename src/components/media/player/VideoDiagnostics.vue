<script setup lang="ts">
// VideoDiagnostics — 视频播放失败诊断面板,替换原生 <video> 失败时的黑屏/broken 观感。
// 仅在 <video> 触发 error 事件后由 VideoPlayer 渲染(边界 15:可播 mkv 不误伤)。
// 挂载时跑一次 canPlayType / MediaSource.isTypeSupported 探针矩阵并 logger.info 落盘——mkv 争议的
// 真机裁决数据源(计划 §裁决)。据探针 + MediaError.code + 扩展名调 diagnoseVideoError 推断成因,
// 给出本地化的标题/提示 + 可展开的技术详情。
import { ref, computed, onMounted } from 'vue'
import { AlertTriangle } from '@lucide/vue'
import { logger } from '../../../utils/logger'
import { diagnoseVideoError, type VideoDiagnosisResult } from '../../../utils/videoDiagnostics'

const props = defineProps<{
  /** 文件扩展名(可含点,util 内部归一)。 */
  extension: string
  /** <video>.error?.code;无则 null。 */
  mediaErrorCode: number | null
  /** 已探测到的视频编码(video_meta),无则 null。 */
  videoCodec?: string | null
  /** resolve_video_playback 原始判定分支(V7 项2,如 `needs_hevc_ext`);无则 null。 */
  resolveVerdict?: string | null
}>()

/** verdict(host 判定标签,snake_case)→ diagnoseVideoError 的 resolveMode 入参(V7 项2):
 * mediaCapabilities 前端实测「可解」而乐观直播(mode=direct),但真实播放仍失败的场景——此时
 * 唯一还留着的归因线索是 host 原始 verdict,须映射回来才能给出「需装 HEVC 扩展」而非笼统诊断。 */
const resolveMode = computed<'needsComponent' | 'needsHevcExt' | undefined>(() => {
  return props.resolveVerdict === 'needs_hevc_ext' ? 'needsHevcExt' : undefined
})

const result = ref<VideoDiagnosisResult | null>(null)

// HEVC/AV1 的探测 MIME:选带完整 codecs 参数的字符串,让浏览器给出确切的 ''/'maybe'/'probably'。
const PROBE_HEVC = 'video/mp4; codecs="hev1.1.6.L93.B0"'
const PROBE_AV1 = 'video/mp4; codecs="av01.0.05M.08"'
const PROBE_MKV = 'video/x-matroska'
const PROBE_WEBM_VP9 = 'video/webm; codecs="vp9"'

onMounted(() => {
  const probeEl = document.createElement('video')
  const probes = {
    canPlayHevc: probeEl.canPlayType(PROBE_HEVC),
    canPlayAv1: probeEl.canPlayType(PROBE_AV1),
    canPlayMkv: probeEl.canPlayType(PROBE_MKV),
  }
  // MSE 支持矩阵:与 canPlayType 分列,直播/流式解码能力独立于渐进式播放。
  const mseAvailable = typeof MediaSource !== 'undefined'
  const isTypeSupported = {
    hevc: mseAvailable ? MediaSource.isTypeSupported(PROBE_HEVC) : null,
    av1: mseAvailable ? MediaSource.isTypeSupported(PROBE_AV1) : null,
    mkv: mseAvailable ? MediaSource.isTypeSupported(PROBE_MKV) : null,
    webmVp9: mseAvailable ? MediaSource.isTypeSupported(PROBE_WEBM_VP9) : null,
  }
  // mkv 争议裁决数据源:每次失败都落一条,真机日志比对系统编码支持。
  logger.info('[VideoDiagnostics] codec support probe', {
    extension: props.extension,
    mediaErrorCode: props.mediaErrorCode,
    videoCodec: props.videoCodec ?? null,
    canPlayType: probes,
    isTypeSupported,
  })
  result.value = diagnoseVideoError({
    extension: props.extension,
    mediaErrorCode: props.mediaErrorCode,
    probes,
    videoCodec: props.videoCodec ?? null,
    resolveMode: resolveMode.value,
  })
})
</script>

<template>
  <div v-if="result" class="video-diag">
    <AlertTriangle :size="48" class="video-diag__icon" />
    <p class="video-diag__title">{{ $t(result.titleKey) }}</p>
    <p class="video-diag__hint">{{ $t(result.hintKey) }}</p>
    <details class="video-diag__tech">
      <summary>{{ $t('player.diag.technical') }}</summary>
      <code>{{ result.technical }}</code>
    </details>
  </div>
</template>

<style scoped>
/* 诊断面板浮于看图台黑底之上:承看图台恒黑观感(硬编码豁免同 ContentViewer),文字用半透明白,
   与不可用占位(.detail-viewer__unavailable)风格对齐但语义不同——此为「格式/解码」诊断。 */
.video-diag {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-xl);
  text-align: center;
  color: rgba(255, 255, 255, 0.75);
  user-select: text;
  -webkit-user-select: text;
}
.video-diag__icon {
  color: rgba(255, 200, 80, 0.9);
  margin-bottom: var(--spacing-xs);
}
.video-diag__title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: rgba(255, 255, 255, 0.92);
}
.video-diag__hint {
  font-size: var(--font-size-sm);
  color: rgba(255, 255, 255, 0.6);
  max-width: 420px;
  line-height: 1.5;
}
.video-diag__tech {
  margin-top: var(--spacing-sm);
  font-size: var(--font-size-xs);
  color: rgba(255, 255, 255, 0.4);
  max-width: 90%;
}
.video-diag__tech summary {
  cursor: pointer;
  user-select: none;
}
.video-diag__tech code {
  display: block;
  margin-top: 4px;
  word-break: break-all;
  font-family: var(--font-mono, monospace);
  text-align: left;
}
</style>
