<template>
  <div class="audio-player">
    <!-- 工具栏：返回 / 标题 / 外部打开 -->
    <div class="audio-player__toolbar">
      <button class="ap-btn" @click="goBack" :title="t('common.back')">
        <ChevronLeft :size="18" /> <span>{{ t('common.back') }}</span>
      </button>
      <span class="audio-player__title" :title="title">{{ title }}</span>
      <div class="audio-player__spacer"></div>
      <button
        v-if="detail"
        class="ap-btn"
        @click="openExternal"
        :title="t('common.openExternal')"

      >
        <ExternalLink :size="16" />
      </button>
    </div>

    <div v-if="detail" class="audio-player__body">
      <!-- 左：封面 + 元数据 + 播放控件 -->
      <div class="audio-player__main">
        <div class="audio-player__cover">
          <img v-if="coverUrl" :src="coverUrl" />
          <div v-else class="audio-player__cover-fallback">
            <Music :size="96" />
          </div>
        </div>

        <div class="audio-player__info">
          <h1 class="audio-player__track">{{ meta.trackTitle || stripExt(detail.fileName) }}</h1>
          <p v-if="meta.artist" class="audio-player__artist">{{ meta.artist }}</p>
          <p v-if="meta.albumTitle" class="audio-player__album">
            {{ meta.albumTitle }}<span v-if="meta.year"> · {{ meta.year }}</span>
          </p>
        </div>

        <!-- 控件：进度条 + 播放/暂停 + 时间 + 音量 -->
        <div class="audio-player__controls">
          <div class="audio-player__seek">
            <span class="audio-player__time">{{ fmt(currentTime) }}</span>
            <input
              class="audio-player__range"
              type="range"
              min="0"
              :max="duration || 0"
              step="0.1"
              :value="currentTime"
              @input="onSeek"
            />
            <span class="audio-player__time">{{ fmt(duration) }}</span>
          </div>
          <div class="audio-player__buttons">
            <button
              class="ap-icon"
              @click="seekBy(-10)"
              :title="t('audio.rewind10')"

            >
              <SkipBack :size="20" />
            </button>
            <button
              class="ap-icon ap-icon--play"
              @click="togglePlay"
              :title="playing ? t('common.pause') : t('audio.play')"

            >
              <component :is="playing ? Pause : Play" :size="26" :fill="'currentColor'" />
            </button>
            <button
              class="ap-icon"
              @click="seekBy(10)"
              :title="t('audio.forward10')"

            >
              <SkipForward :size="20" />
            </button>
            <div class="audio-player__volume">
              <Volume2 :size="16" />
              <input
                class="audio-player__range audio-player__range--vol"
                type="range"
                min="0"
                max="1"
                step="0.01"
                :value="volume"
                @input="onVolume"
              />
            </div>
          </div>
        </div>

        <!-- 元数据细节 -->
        <dl class="audio-player__meta">
          <template v-if="meta.genre"
            ><dt>{{ t('audio.genre') }}</dt>
            <dd>{{ meta.genre }}</dd></template
          >
          <template v-if="meta.trackNo"
            ><dt>{{ t('audio.trackNo') }}</dt>
            <dd>{{ meta.trackNo }}</dd></template
          >
          <template v-if="meta.audioCodec"
            ><dt>{{ t('audio.codec') }}</dt>
            <dd>{{ meta.audioCodec }}</dd></template
          >
          <template v-if="detail.fileFormat"
            ><dt>{{ t('detail.format') }}</dt>
            <dd>{{ detail.fileFormat.toUpperCase() }}</dd></template
          >
        </dl>
      </div>

      <!-- 右：歌词（同步高亮 / 纯文本） -->
      <div class="audio-player__lyrics" ref="lyricsBox">
        <template v-if="lyricsSynced && syncedLines.length">
          <p
            v-for="(line, i) in syncedLines"
            :key="i"
            :ref="(el) => setLineRef(el as HTMLElement | null, i)"
            class="audio-player__lyric-line"
            :class="{ 'is-active': i === activeLine }"
            @click="seekTo(line.time)"
          >
            <template v-if="line.text">{{ line.text }}</template>
            <Music v-else :size="12" />
          </p>
        </template>
        <pre v-else-if="detail.lyrics" class="audio-player__lyric-plain">{{ detail.lyrics }}</pre>
        <div v-else class="audio-player__no-lyrics">{{ t('audio.noLyrics') }}</div>
      </div>
    </div>

    <div v-else-if="error" class="audio-player__error">{{ error }}</div>

    <!-- 共享音频元素（隐藏，控件自绘） -->
    <audio
      ref="audioEl"
      :src="url"
      preload="metadata"
      @timeupdate="onTimeUpdate"
      @loadedmetadata="onLoadedMeta"
      @play="playing = true"
      @pause="playing = false"
      @ended="playing = false"
    ></audio>
  </div>
</template>

<script setup lang="ts">
// 音频播放器（需求6, §3.6）：路由 /audio/:id。封面 + 控件 + 同步歌词 + 元数据面板。
// 标签/歌词由后端 get_audio_detail 懒加载（既有库无需重扫）；封面为后端按需抽取的全分辨率内嵌图。
import { ref, computed, watch, onBeforeUnmount, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { convertFileSrc } from '@tauri-apps/api/core'
import { invokeIpc } from '../utils/ipc'
import { open as shellOpen } from '@tauri-apps/plugin-shell'
import {
  ChevronLeft,
  ExternalLink,
  Music,
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
} from '@lucide/vue'
import { IPC } from '../constants/ipc'
import { formatDuration } from '../utils/format'
import { parseLrc, activeLineIndex, type LrcLine } from '../utils/lrc'
import {
  useViewerStore,
  toViewerFileInfo,
  type ViewerApi,
  type ActiveViewer,
  type ViewerFileInfo,
} from '../stores/viewerStore'
import { resolveViewerKind } from '../utils/viewerKind'
import type { MediaDetail } from '../types/media'

interface AudioMeta {
  audioCodec?: string | null
  artist?: string | null
  albumTitle?: string | null
  trackTitle?: string | null
  trackNo?: number | null
  year?: number | null
  genre?: string | null
}
interface AudioDetail {
  fileName: string
  fileFormat: string
  absPath: string
  meta: AudioMeta
  coverPath?: string | null
  lyrics?: string | null
  lyricsSynced: boolean
}

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const id = computed(() => Number(route.params.id))

const detail = ref<AudioDetail | null>(null)
const error = ref('')

const audioEl = ref<HTMLAudioElement | null>(null)
const playing = ref(false)
const currentTime = ref(0)
const duration = ref(0)
const volume = ref(1)

const url = computed(() => (detail.value ? convertFileSrc(detail.value.absPath) : ''))
const coverUrl = computed(() =>
  detail.value?.coverPath ? convertFileSrc(detail.value.coverPath) : '',
)
const title = computed(() => detail.value?.fileName ?? t('routes.audio'))
const meta = computed<AudioMeta>(() => detail.value?.meta ?? {})
const lyricsSynced = computed(() => detail.value?.lyricsSynced ?? false)

// 同步歌词：解析为带时间轴的行。
const syncedLines = ref<LrcLine[]>([])
const activeLine = ref(-1)
const lineEls: (HTMLElement | null)[] = []
const lyricsBox = ref<HTMLElement | null>(null)

function setLineRef(el: HTMLElement | null, i: number) {
  lineEls[i] = el
}

function fmt(sec: number): string {
  if (!Number.isFinite(sec) || sec <= 0) return '0:00'
  return formatDuration(sec * 1000)
}
function stripExt(name: string): string {
  const i = name.lastIndexOf('.')
  return i > 0 ? name.slice(0, i) : name
}

// ── 播放控制 ──────────────────────────────────────────────────────────────────
function togglePlay() {
  const el = audioEl.value
  if (!el) return
  if (el.paused) el.play().catch(() => {})
  else el.pause()
}
function onSeek(e: Event) {
  const el = audioEl.value
  if (!el) return
  el.currentTime = Number((e.target as HTMLInputElement).value)
}
function seekTo(sec: number) {
  const el = audioEl.value
  if (!el) return
  el.currentTime = sec
  if (el.paused) el.play().catch(() => {})
}
function seekBy(delta: number) {
  const el = audioEl.value
  if (!el) return
  el.currentTime = Math.max(0, Math.min(duration.value, el.currentTime + delta))
}
function onVolume(e: Event) {
  const el = audioEl.value
  if (!el) return
  const v = Number((e.target as HTMLInputElement).value)
  el.volume = v
  volume.value = v
}

function onLoadedMeta() {
  const el = audioEl.value
  if (el) duration.value = el.duration || 0
}
function onTimeUpdate() {
  const el = audioEl.value
  if (!el) return
  currentTime.value = el.currentTime
  if (lyricsSynced.value && syncedLines.value.length) {
    const idx = activeLineIndex(syncedLines.value, el.currentTime)
    if (idx !== activeLine.value) {
      activeLine.value = idx
      scrollActiveIntoView()
    }
  }
}

// 高亮行居中滚动（仅在切换时触发，避免抖动）。
function scrollActiveIntoView() {
  const el = lineEls[activeLine.value]
  if (!el) return
  el.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

async function load() {
  detail.value = null
  fileInfo.value = null
  error.value = ''
  syncedLines.value = []
  activeLine.value = -1
  lineEls.length = 0
  currentTime.value = 0
  duration.value = 0
  // 底栏文件信息:AudioDetail 载荷不含 rating/isFavorited/colorLabel 等库内标量,并行补一次
  // get_media_detail(同 items 表单行,doc/图视页同源 IPC)。换歌竞态以 id 快照守卫;失败不阻塞
  // 播放(fileInfo 留 null,底栏仅少标量段)。
  const reqId = id.value
  invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id: reqId })
    .then((d) => {
      if (id.value === reqId) fileInfo.value = toViewerFileInfo(d)
    })
    .catch(() => {})
  try {
    const d = await invokeIpc<AudioDetail>(IPC.GET_AUDIO_DETAIL, { id: id.value })
    detail.value = d
    if (d.lyricsSynced && d.lyrics) {
      syncedLines.value = parseLrc(d.lyrics)
    }
    // 等 <audio> 重新绑定 src 后自动尝试播放（静音策略不影响音频）。
    await nextTick()
    audioEl.value?.play().catch(() => {})
  } catch (e) {
    error.value = t('audio.openFailed', { error: (e as Error)?.message ?? e })
  }
}

function goBack() {
  // 与 ContentViewer.close 同判据(深链直开时 window.history.length 不可靠,LOW-1 统一)。
  if (router.options.history.state.back != null) router.back()
  else void router.push('/')
}
async function openExternal() {
  if (detail.value) await shellOpen(detail.value.absPath).catch(() => {})
}

// ── activeViewer 单源 populate(顶栏重构 P5 余项)─────────────────────────────
// 让音频也进 viewerStore, 点亮标题栏上下文工具栏(播放/±10s 命令); ViewerApi 映射到本地播放函数。
// 与本组件内的播放控件并存(双入口, 承 P5-5「保留局部控件」决策)。populate 后 ContextualToolbar
// 靠 when=kind 'audio' 渲染音频命令, 其 /audio/ 前缀兜底抑制随 hasActiveViewer 转真自然失效。
const viewer = useViewerStore()
const viewerApi: ViewerApi = {
  togglePlay,
  seekBy,
  close: goBack,
}
let viewerToken: number | null = null
// 底栏文件信息标量(get_media_detail 异步补齐,load() 换歌复位;见 load 内注释)。
const fileInfo = ref<ViewerFileInfo | null>(null)
function viewerSnapshot(): Omit<ActiveViewer, 'immersive'> {
  const d = detail.value
  return {
    kind: resolveViewerKind('audio', d?.fileFormat ?? ''),
    mediaType: 'audio',
    fileFormat: d?.fileFormat ?? '',
    id: Number.isFinite(id.value) ? id.value : null,
    path: d?.absPath ?? null,
    title: d?.fileName ?? title.value,
    api: viewerApi,
    fileInfo: fileInfo.value,
  }
}
// detail 加载完成(每首一次)→ 首次 populate、后续换歌 patch。token 时序防御见 viewerStore。
watch(detail, (d) => {
  if (!d) return
  if (viewerToken === null) {
    viewerToken = viewer.populate({ ...viewerSnapshot(), immersive: false })
  } else {
    viewer.patch(viewerToken, viewerSnapshot())
  }
})
// 标量补齐落地(两种到达顺序都覆盖:先于 populate → snapshot 直接带上;晚于 populate → 此处 patch)。
watch(fileInfo, (fi) => {
  if (fi && viewerToken !== null) viewer.patch(viewerToken, { fileInfo: fi })
})

watch(id, load, { immediate: true })

onBeforeUnmount(() => {
  audioEl.value?.pause()
  // 离开音频页:清 activeViewer 上下文(token 时序防御, 迟到 clear 不误清新查看器)。
  if (viewerToken !== null) viewer.clear(viewerToken)
})
</script>

<style scoped>
.audio-player {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  /* 原 --color-bg-base 为不存在的幽灵 token 且无 fallback——实际渲染透明(S5 修) */
  background: var(--color-bg-primary);
  z-index: 5;
}
.audio-player__toolbar {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  min-height: var(--toolbar-height);
  padding: 0 var(--spacing-md);
  border-bottom: 1px solid var(--color-divider);
  background: var(--color-bg-primary);
}
.ap-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  background: transparent;
  border: 1px solid transparent;
  color: var(--color-text-secondary);
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--font-size-sm);
}
.ap-btn:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}
.audio-player__title {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 50vw;
}
.audio-player__spacer {
  flex: 1;
}

.audio-player__body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.audio-player__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-md);
  padding: var(--spacing-xl);
  overflow-y: auto;
}
.audio-player__cover {
  width: min(42vh, 360px);
  height: min(42vh, 360px);
  border-radius: var(--radius-xl);
  overflow: hidden;
  background: var(--color-bg-surface);
  box-shadow: var(--shadow-sm);
  flex: 0 0 auto;
}
.audio-player__cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.audio-player__cover-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary);
}
.audio-player__info {
  text-align: center;
}
.audio-player__track {
  font-size: var(--font-size-xl);
  font-weight: 600;
  margin: 0;
  color: var(--color-text-primary);
}
.audio-player__artist {
  margin: var(--spacing-xs) 0 0;
  color: var(--color-text-primary);
}
.audio-player__album {
  margin: var(--spacing-2xs) 0 0;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.audio-player__controls {
  width: min(100%, 460px);
}
.audio-player__seek {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}
.audio-player__time {
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
  min-width: 42px;
  text-align: center;
}
.audio-player__range {
  flex: 1;
  accent-color: var(--color-accent);
  cursor: pointer;
}
.audio-player__range--vol {
  flex: 0 0 90px;
}
.audio-player__buttons {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-sm);
}
.ap-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--color-text-primary);
  cursor: pointer;
  width: var(--control-size-default);
  height: var(--control-size-default);
  padding: 0;
  border-radius: 50%;
}
.ap-icon:hover {
  background: var(--color-bg-hover);
}
.ap-icon--play {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  width: var(--control-size-touch);
  height: var(--control-size-touch);
}
.ap-icon--play:hover {
  background: var(--color-accent-hover);
}
.audio-player__volume {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  color: var(--color-text-secondary);
  margin-left: var(--spacing-xs);
}

.audio-player__meta {
  display: grid;
  grid-template-columns: auto auto;
  gap: var(--spacing-xs) var(--spacing-md);
  margin: 0;
  font-size: var(--font-size-sm);
}
.audio-player__meta dt {
  color: var(--color-text-secondary);
  text-align: right;
}
.audio-player__meta dd {
  margin: 0;
  color: var(--color-text-primary);
}

.audio-player__lyrics {
  flex: 0 0 38%;
  max-width: 460px;
  min-width: 280px;
  border-left: 1px solid var(--color-divider);
  overflow-y: auto;
  padding: var(--spacing-2xl) var(--spacing-xl);
  background: var(--color-bg-secondary);
}
.audio-player__lyric-line {
  margin: 0;
  padding: var(--spacing-sm) 0;
  text-align: center;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition:
    color var(--transition-fast),
    background-color var(--transition-fast);
}
.audio-player__lyric-line.is-active {
  color: var(--color-accent-text);
  font-weight: 600;
  background: var(--color-accent-subtle);
  border-radius: var(--radius-sm);
}
.audio-player__lyric-plain {
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  line-height: 1.8;
  margin: 0;
  font-family: inherit;
}
.audio-player__no-lyrics {
  color: var(--color-text-secondary);
  text-align: center;
  margin-top: var(--spacing-2xl);
  font-size: var(--font-size-sm);
}
.audio-player__error {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary);
}
</style>
