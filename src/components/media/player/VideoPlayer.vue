<script setup lang="ts">
// VideoPlayer — 视频播放器编排层。持有 <video crossorigin="anonymous">,装配控制条/进度条/诊断,
// 接线全部 composables(useVideoPlayback 状态、usePlayerPrefs 偏好持久化、useVideoFullscreen 全屏对),
// 并 expose PlayerApi 供 ContentViewer 的 viewerApi 透传给命令层(GD 批)。
//
// 交互契约(计划 §裁决):
//  · crossorigin 随 :src 一同渲染(截帧 taint 前提,不可事后追加)。
//  · 根上点击 = 播放/暂停,带 4px 拖拽守卫;mousedown **不** stopPropagation——保 ContentViewer
//    的 .detail-viewer startDrag 平移链(缩放后可拖视频)。控制层区 @mousedown.stop/@click.stop,
//    避免操作控件时误触平移/播放切换。
//  · transform 施加在内部 <video>(缩放/平移/旋转),控制条不随缩放旋转(独立浮层)。
//  · 字幕/截帧/进度记忆/seekbar sprite 预览为 GE 批接线(useVideoResume/useVideoSubtitles/
//    useVideoFrameCapture/useSeekbarPreview)。
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { Play } from '@lucide/vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import type { VideoMeta } from '../../../types/media'
import { useVideoPlayback } from '../../../composables/player/useVideoPlayback'
import { usePlayerPrefs } from '../../../composables/player/usePlayerPrefs'
import { useVideoFullscreen } from '../../../composables/player/useVideoFullscreen'
import { useVideoResume } from '../../../composables/player/useVideoResume'
import { useVideoSubtitles } from '../../../composables/player/useVideoSubtitles'
import { useVideoFrameCapture } from '../../../composables/player/useVideoFrameCapture'
import { useSeekbarPreview } from '../../../composables/player/useSeekbarPreview'
import { useOcr } from '../../../composables/useOcr'
import VideoSeekBar from './VideoSeekBar.vue'
import VideoControlBar from './VideoControlBar.vue'
import VideoDiagnostics from './VideoDiagnostics.vue'

const props = defineProps<{
  /** 已解析的 asset URL。 */
  src: string
  /** 视频封面(首帧)URL;无则浏览器空白。 */
  poster?: string
  /** 缩放/平移/旋转的 CSS transform 串(施加到 <video>)。 */
  transform: string
  /** 平移拖拽中(禁 transform 过渡,与图像一致)。 */
  dragging: boolean
  /** 库内资产 id(进度续播 / 截帧命名用,GE 批消费)。 */
  itemId: number | null
  /** 文件扩展名(诊断用)。 */
  fileFormat: string
  /** 原始文件名(截帧默认命名取 basename 用,GE 批消费)。 */
  fileName: string
  /** 该条目上次退出时的播放位置记忆(ms;GE①消费)。 */
  playbackPositionMs: number
  /** 视频元数据(hasAudio / codec);null=非视频或未探测。 */
  videoMeta: VideoMeta | null
  /** ContentViewer 底部操作栏隐藏中(控制条内切换钮图标据此翻转;状态归 ContentViewer)。 */
  toolbarHidden: boolean
  /**
   * 库内记录的视频像素宽高(DB 扫描产出;0=未知)。用于换源瞬间预设元素 aspect-ratio+尺寸,
   * 消除「metadata 未到时元素塌回替换元素默认 300×150、到了再跳成大画面」的展开闪动;
   * loadedmetadata 后以真实 videoWidth/Height 校正(DB 偶有占位尺寸,不可尽信)。
   */
  intrinsicWidth: number
  intrinsicHeight: number
  /** 视频格式扩展子系统(design.md §5.3,V7 项2,additive 可选):resolve_video_playback 原始判定
   * 分支(如 needs_hevc_ext)。仅透传给 VideoDiagnostics 作诊断输入,本体播放逻辑不消费。 */
  resolveVerdict?: string | null
}>()

const emit = defineEmits<{
  /** 元数据就绪(视频宽高可读)→ ContentViewer 驱动 updateZoomRatio。 */
  (e: 'loadedmetadata'): void
  /** 控制条内「底部操作栏显隐」切换钮 → ContentViewer toggleControls(状态不下沉)。 */
  (e: 'toggle-toolbar'): void
}>()

const videoEl = ref<HTMLVideoElement | null>(null)

const {
  currentTime,
  duration,
  bufferedEnd,
  paused,
  waiting,
  seeking,
  rate,
  volume,
  muted,
  loop,
  pipActive,
  playPause,
  seekTo,
  seekBy,
  setVolume,
  volumeBy,
  toggleMute,
  setRate,
  rateStep,
  toggleLoop,
  setLoop,
  togglePip,
  exitPipIfActive,
} = useVideoPlayback(videoEl)

const { prefs, setPrefs } = usePlayerPrefs()
const fullscreen = useVideoFullscreen()

// ── GE 批接线:进度记忆 / 字幕 / 截帧 / seekbar sprite 预览 ───────────────────
const itemIdRef = computed(() => props.itemId)
const playbackPositionMsRef = computed(() => props.playbackPositionMs)
const fileNameRef = computed(() => props.fileName)
useVideoResume(videoEl, itemIdRef, playbackPositionMsRef)
const subtitles = useVideoSubtitles(videoEl, itemIdRef)
const frameCapture = useVideoFrameCapture(videoEl, fileNameRef)
const seekPreview = useSeekbarPreview(itemIdRef)
// OCR 文字提取(T9):模块级单例,与 ContentViewer 内的 useOcr() 调用共享同一份 busy/panelOpen/result。
const ocr = useOcr()

// ── 偏好施加 / 回写 ─────────────────────────────────────────────────────────
// 元素挂载即把持久化偏好施加到 <video>(loop 无 media 事件,经 setLoop 同步镜像)。
// 时序门控(采纳 opus 深审):useVideoPlayback 的 attach() 在同一 videoEl watch 批次里先跑,
// 会把镜像 ref(volume/muted/rate)同步成元素的**默认值**(1/false/1)。若下面的回写 watch
// 无门控,这个默认值会在 applyPrefs() 真正施加已存偏好之前抢先写回 prefs.value,
// 用默认值错写覆盖已存偏好。门控:首次 applyPrefs() 完成前,回写 watch 一律早退。
let prefsApplied = false
function applyPrefs(): void {
  const el = videoEl.value
  if (!el) return
  el.volume = prefs.value.volume
  el.muted = prefs.value.muted
  el.playbackRate = prefs.value.rate
  el.loop = prefs.value.loop
  setLoop(prefs.value.loop)
  prefsApplied = true
}
watch(videoEl, (el) => el && applyPrefs(), { immediate: true })
// 后端只应用:设置快照变化(启动水合 / 恢复默认 / 外部改文件)→ 施加到元素,不发保存请求。
watch(
  prefs,
  () => {
    if (prefsApplied) applyPrefs()
  },
  { deep: true },
)
// 用户交互改动镜像态 → 显式提交(与权威值同值时早退,applyPrefs 自身触发的事件不产生写盘)。
// 音量/倍速是连续操作,提交走中央防抖合并;静音/循环是离散开关,即时提交。
watch(volume, (v) => {
  if (!prefsApplied || v === prefs.value.volume) return
  setPrefs({ volume: v }, { debounce: true })
})
watch(muted, (v) => {
  if (!prefsApplied || v === prefs.value.muted) return
  setPrefs({ muted: v })
})
watch(rate, (v) => {
  if (!prefsApplied || v === prefs.value.rate) return
  setPrefs({ rate: v }, { debounce: true })
})
watch(loop, (v) => {
  if (!prefsApplied || v === prefs.value.loop) return
  setPrefs({ loop: v })
})

// ── 播放失败 → 诊断面板 ─────────────────────────────────────────────────────
const errored = ref(false)
const errorCode = ref<number | null>(null)
function onError(): void {
  errorCode.value = videoEl.value?.error?.code ?? null
  errored.value = true
}
// ── 元素尺寸预设(消除切换展开闪动)──────────────────────────────────────────
// 换源即按 DB 宽高定形;loadedmetadata 后用元素真实 videoWidth/Height 覆盖(含旋转已折算)。
// 尺寸未知(DB 无记录且 metadata 未到)返回空样式,退回纯 max 约束的自适应。
const displayDims = ref<{ w: number; h: number } | null>(null)
watch(
  () => [props.src, props.intrinsicWidth, props.intrinsicHeight] as const,
  () => {
    displayDims.value =
      props.intrinsicWidth > 0 && props.intrinsicHeight > 0
        ? { w: props.intrinsicWidth, h: props.intrinsicHeight }
        : null
  },
  { immediate: true },
)
// contain 且不超原生:width:100% 铺容器,aspect-ratio 定高,max 双约束按比例回缩——与
// useMediaDetail 缩放数学的 base_w/h 口径(min(原生, 容器, 容器×比例))逐项一致,勿改一侧。
const sizeStyle = computed(() => {
  const d = displayDims.value
  if (!d) return {}
  return {
    width: '100%',
    aspectRatio: `${d.w} / ${d.h}`,
    maxWidth: `min(100%, ${d.w}px)`,
    maxHeight: `min(100%, ${d.h}px)`,
  }
})

function onLoadedMeta(): void {
  const el = videoEl.value
  if (el && el.videoWidth > 0 && el.videoHeight > 0) {
    displayDims.value = { w: el.videoWidth, h: el.videoHeight }
  }
  emit('loadedmetadata')
  // 视频元素+条目均已就绪的自然汇合点(首次挂载与每次切条目都会触发一次):字幕自动探测
  // 挂 <track> 需要真实 DOM 元素,故不在更早的 itemId/src watch 里做(那时 videoEl 可能未就绪)。
  void subtitles.autoDetectForItem()
}
// 切换视频源:复位诊断态 + 退出可能残留的 PiP(边界 7)。
watch(
  () => props.src,
  () => {
    errored.value = false
    errorCode.value = null
    // 切条目重新亮控制条(闲置计时随 revealControls 重排;上一条目隐藏态不带入下一条)。
    revealControls()
    void exitPipIfActive()
  },
)

// ── 派生态 ─────────────────────────────────────────────────────────────────
// 无音轨判定(边界 1):缺 video_meta 行按有音轨处理,不误禁音量。
const hasAudio = computed(() => props.videoMeta?.hasAudio ?? true)
const pipSupported = typeof document !== 'undefined' && !!document.pictureInPictureEnabled
// 缓冲/seek 等待时显 spinner(仅播放中,暂停态由大播放键表达)。
const showSpinner = computed(() => (waiting.value || seeking.value) && !paused.value && !errored.value)
// autoplay 被拦截 / 用户暂停:中央大播放键覆层(边界 5)。
const showBigPlay = computed(() => paused.value && !errored.value)

// ── 控制层自动隐藏(闲置 3s;暂停态恒显;悬停控制条钉住)────────────────────────
const controlsVisible = ref(true)
let hideTimer: ReturnType<typeof setTimeout> | null = null
// 光标悬在控制条上时钉住不隐:操作中途被藏走是反直觉;同时切断「隐藏→光标下元素变化→
// Chromium 补发合成 pointermove→又唤回」的死循环(该循环即"永远隐不掉"的根因之一)。
let chromePinned = false
function revealControls(): void {
  controlsVisible.value = true
  if (hideTimer) clearTimeout(hideTimer)
  hideTimer = null
  if (!paused.value && !chromePinned) {
    hideTimer = setTimeout(() => {
      controlsVisible.value = false
    }, 3000)
  }
}
function pinControls(): void {
  chromePinned = true
  revealControls()
}
function unpinControls(): void {
  chromePinned = false
  revealControls()
}
// 合成 pointermove 守卫:Chromium 在光标下元素显隐/重排时会补发**坐标不变**的 pointermove
// (非用户动作)。若不过滤,控制条 v-show 隐藏那一帧就会被这类事件立刻唤回,自动隐藏失效。
// 只有坐标真变(用户真动了鼠标)才算活动。
let lastMoveX = Number.NaN
let lastMoveY = Number.NaN
function onRootPointerMove(e: PointerEvent): void {
  if (e.clientX === lastMoveX && e.clientY === lastMoveY) return
  lastMoveX = e.clientX
  lastMoveY = e.clientY
  revealControls()
}
watch(paused, (p) => {
  if (p) {
    if (hideTimer) clearTimeout(hideTimer)
    hideTimer = null
    controlsVisible.value = true
  } else {
    revealControls()
  }
})

// ── 根手势:click=播放/暂停(4px 拖拽守卫),mousedown 不阻断冒泡(保平移链)──────
let downX = 0
let downY = 0
function onRootMouseDown(e: MouseEvent): void {
  downX = e.clientX
  downY = e.clientY
  // 刻意不 stopPropagation:让 ContentViewer 的 startDrag 接管平移。
}
function onRootClick(e: MouseEvent): void {
  if (errored.value) return
  // 拖拽(平移)不算点击:位移超阈值则不切换播放。
  if (Math.hypot(e.clientX - downX, e.clientY - downY) > 4) return
  playPause()
}

// ── 全屏对 / GE 批存根 ───────────────────────────────────────────────────────
function toggleFullscreenPair(): void {
  void fullscreen.togglePair()
}
function isFullscreenPair(): boolean {
  return fullscreen.pairActive.value
}
/** drawImage(原生尺寸)→ toBlob → dialog save → IPC 写盘;busy 态防抖内聚在 composable 里。 */
function captureFrame(): void {
  void frameCapture.captureFrame()
}
/** 当前帧 OCR 提取(T9);videoEl 空守卫(未挂载/已卸载态点击无效)。 */
function ocrFrame(): void {
  if (!videoEl.value) return
  void ocr.extractFromVideoFrame(videoEl.value, fileNameRef.value)
}

// ── 字幕菜单接线(VideoControlBar → VideoSubtitleMenu 事件透传)─────────────
function onSubtitleSelect(key: string): void {
  subtitles.selectTrack(key)
}
function onSubtitleTurnOff(): void {
  subtitles.turnOff()
}
/** 「加载字幕文件…」:plugin-dialog 选路径 → useVideoSubtitles 走 IPC 读取转换。 */
async function onSubtitleLoadFile(): Promise<void> {
  const path = await openDialog({
    multiple: false,
    filters: [{ name: 'Subtitles', extensions: ['vtt', 'srt'] }],
  })
  if (typeof path === 'string') await subtitles.loadFromPath(path)
}

onBeforeUnmount(() => {
  if (hideTimer) clearTimeout(hideTimer)
  void exitPipIfActive()
})

// ── PlayerApi:供 ContentViewer viewerApi 透传给命令层(GD 批)────────────────
defineExpose({
  /** 原生 <video> 元素(ContentViewer getMediaDimensions 用)。 */
  videoEl,
  playPause,
  seekBy,
  volumeBy,
  toggleMute,
  setRate,
  rateStep,
  toggleLoop,
  togglePip,
  toggleFullscreenPair,
  captureFrame,
  isFullscreenPair,
})
</script>

<template>
  <div class="video-player" @mousedown="onRootMouseDown" @click="onRootClick" @pointermove="onRootPointerMove">
    <!-- crossorigin 随 :src 一同渲染(截帧 taint 前提,计划 §裁决 0.3)。 -->
    <video
      ref="videoEl"
      class="video-player__video"
      :class="{ 'is-dragging': dragging }"
      :src="src"
      :poster="poster"
      :style="[sizeStyle, { transform }]"
      crossorigin="anonymous"
      autoplay
      playsinline
      draggable="false"
      @loadedmetadata="onLoadedMeta"
      @error="onError"
    />

    <!-- 缓冲/seek 等待 spinner -->
    <div v-if="showSpinner" class="video-player__spinner" />

    <!-- autoplay 拦截 / 暂停:中央大播放键 -->
    <button
      v-if="showBigPlay"
      type="button"
      class="video-player__bigplay"

      @click.stop="playPause"
    >
      <Play :size="40" />
    </button>

    <!-- 播放失败诊断(替换黑屏);仅 error 后渲染(边界 15)。 -->
    <VideoDiagnostics
      v-if="errored"
      :extension="fileFormat"
      :media-error-code="errorCode"
      :video-codec="videoMeta?.videoCodec ?? null"
      :resolve-verdict="resolveVerdict ?? null"
      @click.stop
    />

    <!-- 控制层:进度条 + 控制条,闲置自动隐藏;控件区阻断冒泡(不误触平移/播放切换)。 -->
    <Transition name="video-chrome">
      <div
        v-show="controlsVisible && !errored"
        class="video-player__chrome"
        @click.stop
        @mousedown.stop
        @wheel.stop
        @pointerenter="pinControls"
        @pointerleave="unpinControls"
      >
        <VideoSeekBar
          :current-time="currentTime"
          :duration="duration"
          :buffered-end="bufferedEnd"
          @seek="seekTo"
        >
          <!-- sprite 缩略帧预览(GE④):无 sprite 时 styleForRatio 返回 null,只留 VideoSeekBar
               自带的时间 tooltip(边界 12)。 -->
          <template #preview="{ ratio }">
            <div
              v-if="seekPreview.styleForRatio(ratio)"
              class="video-player__seek-thumb"
              :style="seekPreview.styleForRatio(ratio) ?? {}"
            />
          </template>
        </VideoSeekBar>
        <VideoControlBar
          :paused="paused"
          :current-time="currentTime"
          :duration="duration"
          :volume="volume"
          :muted="muted"
          :has-audio="hasAudio"
          :rate="rate"
          :loop="loop"
          :pip-active="pipActive"
          :pip-supported="pipSupported"
          :fullscreen="fullscreen.pairActive.value"
          :subtitle-embedded-tracks="subtitles.embeddedTracks.value"
          :subtitle-external-label="subtitles.externalLabel.value"
          :subtitle-active-key="subtitles.activeKey.value"
          :capture-busy="frameCapture.busy.value"
          :ocr-busy="ocr.busy.value"
          :toolbar-hidden="toolbarHidden"
          @play-pause="playPause"
          @toggle-mute="toggleMute"
          @set-volume="setVolume"
          @set-rate="setRate"
          @toggle-loop="toggleLoop"
          @toggle-pip="togglePip"
          @subtitle-select="onSubtitleSelect"
          @subtitle-turn-off="onSubtitleTurnOff"
          @subtitle-load-file="onSubtitleLoadFile"
          @capture-frame="captureFrame"
          @ocr-frame="ocrFrame"
          @toggle-toolbar="emit('toggle-toolbar')"
          @toggle-fullscreen="toggleFullscreenPair"
        />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.video-player {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
/* 内部 <video> 承接 transform(缩放/平移/旋转),与图像同款过渡曲线;拖拽中去过渡即时跟手。
   max 100%(2026-07-23,原 90%):顶满宽或高,与 .detail-viewer__img 及 useMediaDetail 缩放
   数学的 contain-不放大口径对齐(90% 时 zoom% 读数已系统性偏差 1/0.9)。 */
.video-player__video {
  max-width: 100%;
  max-height: 100%;
  transform-origin: center;
  transition: transform var(--duration-normal) cubic-bezier(0.25, 0.46, 0.45, 0.94);
}
.video-player__video.is-dragging {
  transition: none;
}

/* 控制层浮层:恒深色渐变底(硬编码白系,不随主题——同 ContentViewer 看图台豁免 S5)。 */
.video-player__chrome {
  position: absolute;
  left: 0;
  right: 0;
  /* 恒贴底:ContentViewer 底部操作栏可见时经 --viewer-bottom-inset 下发其高度,用 padding-bottom
     抬升**内容**让位(而非整体 bottom 抬升)——渐变底衬保持连续覆盖到视口底缘,与 .detail-controls
     的渐变无缝叠接,消除两条渐变各自起步时接缝透出的亮带;沉浸/隐藏底栏时回落 4px。 */
  bottom: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 var(--spacing-sm) calc(var(--viewer-bottom-inset, 0px) + 4px);
  background: linear-gradient(transparent, rgba(0, 0, 0, 0.72));
  z-index: 10;
}
.video-chrome-enter-active,
.video-chrome-leave-active {
  transition:
    opacity var(--transition-normal),
    transform var(--transition-normal);
}
.video-chrome-enter-from,
.video-chrome-leave-to {
  opacity: 0;
  transform: translateY(8px);
}

/* 中央大播放键:autoplay 拦截 / 暂停时的显式播放入口。恒深色玻璃圆钮(硬编码豁免同上)。 */
.video-player__bigplay {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: center;
  justify-content: center;
  width: 72px;
  height: 72px;
  border: none;
  border-radius: 50%;
  background: rgba(20, 20, 20, 0.55);
  color: rgba(255, 255, 255, 0.9);
  cursor: pointer;
  z-index: 11;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}
.video-player__bigplay:hover {
  background: rgba(20, 20, 20, 0.8);
  color: #fff;
}

/* 缓冲 spinner:居中,承看图台深色语义(白描边环)。 */
.video-player__spinner {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 44px;
  height: 44px;
  transform: translate(-50%, -50%);
  border: 3px solid rgba(255, 255, 255, 0.25);
  border-top-color: rgba(255, 255, 255, 0.9);
  border-radius: 50%;
  animation: video-player-spin 0.8s linear infinite;
  z-index: 11;
  pointer-events: none;
}
@keyframes video-player-spin {
  to {
    transform: translate(-50%, -50%) rotate(360deg);
  }
}
/* seekbar sprite 缩略帧预览(GE④):固定尺寸缩略框,背景经 useSeekbarPreview.styleForRatio 计算。 */
.video-player__seek-thumb {
  width: 120px;
  height: 68px;
  border-radius: var(--radius-sm);
  border: 1px solid rgba(255, 255, 255, 0.3);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.45);
  background-color: rgba(0, 0, 0, 0.6);
}
</style>
