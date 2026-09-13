<script setup lang="ts">
// VideoControlBar — 纯 UI 控制条:播放/暂停、时间读数、音量、倍速、循环、字幕、PiP、截帧、全屏。
// props 入、emits 出,零业务逻辑(状态与副作用全在 VideoPlayer/composables)。时间读数点击在
// 「总长 ↔ 剩余」间切换(纯展示偏好,本地 ref)。字幕经 VideoSubtitleMenu 子组件承接(GE 批接线),
// 截帧按钮 busy 态由 captureBusy prop 驱动禁用防抖。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Play,
  Pause,
  Repeat,
  PictureInPicture2,
  Camera,
  Maximize,
  Minimize2,
  ScanText,
  PanelBottomClose,
  PanelBottomOpen,
} from '@lucide/vue'
import UiIconButton from '../../ui/UiIconButton.vue'
import VideoVolumeControl from './VideoVolumeControl.vue'
import VideoRateMenu from './VideoRateMenu.vue'
import VideoSubtitleMenu from './VideoSubtitleMenu.vue'
import { formatPlayerTime } from '../../../utils/format'
import type { SubtitleOption } from '../../../composables/player/useVideoSubtitles'

const props = defineProps<{
  paused: boolean
  currentTime: number
  duration: number
  volume: number
  muted: boolean
  hasAudio: boolean
  rate: number
  loop: boolean
  pipActive: boolean
  /** document.pictureInPictureEnabled;false 时隐 PiP 按钮(边界 7)。 */
  pipSupported: boolean
  /** 全屏对生效中(F 键)——切图标为退出。 */
  fullscreen: boolean
  /** 字幕菜单态(GE 批接线:useVideoSubtitles 直传)。 */
  subtitleEmbeddedTracks: SubtitleOption[]
  subtitleExternalLabel: string | null
  subtitleActiveKey: string
  /** 截帧进行中(GE 批;busy 时按钮禁用防抖,边界 13)。 */
  captureBusy: boolean
  /** OCR 提取进行中(T9;busy 时按钮禁用防抖,同截帧姿态)。 */
  ocrBusy: boolean
  /** ContentViewer 底部操作栏(缩放/旋转等)当前隐藏中——切换钮图标/文案据此翻转。 */
  toolbarHidden: boolean
}>()

const emit = defineEmits<{
  (e: 'play-pause'): void
  (e: 'toggle-mute'): void
  (e: 'set-volume', value: number): void
  (e: 'set-rate', value: number): void
  (e: 'toggle-loop'): void
  (e: 'toggle-pip'): void
  (e: 'subtitle-select', key: string): void
  (e: 'subtitle-turn-off'): void
  (e: 'subtitle-load-file'): void
  (e: 'capture-frame'): void
  (e: 'ocr-frame'): void
  (e: 'toggle-fullscreen'): void
  (e: 'toggle-toolbar'): void
}>()

const { t } = useI18n()

// 时间读数第二段:总长 ↔ 剩余(点击切)。纯展示偏好,故本地持有(不违「零业务」)。
const showRemaining = ref(false)
const remaining = computed(() =>
  Number.isFinite(props.duration) ? props.duration - props.currentTime : Number.NaN,
)
const timeReadout = computed(() => {
  const played = formatPlayerTime(props.currentTime)
  const tail = showRemaining.value
    ? formatPlayerTime(remaining.value, { negative: true })
    : formatPlayerTime(props.duration)
  return `${played} / ${tail}`
})
</script>

<template>
  <div class="video-controls">
    <!-- 左:播放/暂停 + 时间 -->
    <div class="video-controls__group">
      <UiIconButton
        :label="paused ? t('player.play') : t('player.pause')"
        @click="emit('play-pause')"
      >
        <Play v-if="paused" :size="20" />
        <Pause v-else :size="20" />
      </UiIconButton>
      <button
        type="button"
        class="video-controls__time"
        :title="t('player.toggleTimeDisplay')"
        @click="showRemaining = !showRemaining"
      >
        {{ timeReadout }}
      </button>
    </div>

    <!-- 右:音量 / 倍速 / 循环 / 字幕 / PiP / 截帧 / 全屏 -->
    <div class="video-controls__group">
      <VideoVolumeControl
        :volume="volume"
        :muted="muted"
        :has-audio="hasAudio"
        @toggle-mute="emit('toggle-mute')"
        @set-volume="emit('set-volume', $event)"
      />
      <VideoRateMenu :rate="rate" @set-rate="emit('set-rate', $event)" />
      <UiIconButton
        :label="t('player.loop')"
        toggle
        :active="loop"
        @click="emit('toggle-loop')"
      >
        <Repeat :size="18" />
      </UiIconButton>
      <VideoSubtitleMenu
        :embedded-tracks="subtitleEmbeddedTracks"
        :external-label="subtitleExternalLabel"
        :active-key="subtitleActiveKey"
        @select="emit('subtitle-select', $event)"
        @turn-off="emit('subtitle-turn-off')"
        @load-file="emit('subtitle-load-file')"
      />
      <UiIconButton
        v-if="pipSupported"
        :label="t('player.pip')"
        toggle
        :active="pipActive"
        @click="emit('toggle-pip')"
      >
        <PictureInPicture2 :size="18" />
      </UiIconButton>
      <UiIconButton :label="t('player.captureFrame')" :disabled="captureBusy" @click="emit('capture-frame')">
        <Camera :size="18" />
      </UiIconButton>
      <UiIconButton :label="t('ocr.entryVideo')" :disabled="ocrBusy" @click="emit('ocr-frame')">
        <ScanText :size="18" />
      </UiIconButton>
      <!-- 底部操作栏(缩放/旋转等)显隐切换:2026-07-23 自右上玻璃圆钮迁入本条——视频态操作
           重心在控制条,右上角反操作直觉;图像态另有点大图切换路径,不经此钮。 -->
      <UiIconButton
        :label="toolbarHidden ? t('detail.showControls') : t('detail.hideControls')"
        @click="emit('toggle-toolbar')"
      >
        <PanelBottomOpen v-if="toolbarHidden" :size="18" />
        <PanelBottomClose v-else :size="18" />
      </UiIconButton>
      <UiIconButton
        :label="fullscreen ? t('player.exitFullscreen') : t('player.fullscreen')"
        toggle
        :active="fullscreen"
        @click="emit('toggle-fullscreen')"
      >
        <Minimize2 v-if="fullscreen" :size="18" />
        <Maximize v-else :size="18" />
      </UiIconButton>
    </div>
  </div>
</template>

<style scoped>
/* 控制条:承看图台深色浮层(硬编码白系,不随主题——同 ContentViewer .detail-controls 豁免)。
   按钮色/悬停/激活沿用 .detail-controls 的既有规则(它们在全局作用域,本组件 btn-icon 亦命中)。 */
.video-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
  padding: 4px var(--spacing-sm);
  color: #fff;
}
.video-controls__group {
  display: flex;
  align-items: center;
  gap: 4px;
}
.video-controls :deep(.btn-icon) {
  color: rgba(255, 255, 255, 0.82);
}
.video-controls :deep(.btn-icon:hover) {
  color: #fff;
  background: rgba(255, 255, 255, 0.14);
}
.video-controls :deep(.btn-icon.active) {
  color: var(--color-accent);
}
.video-controls__time {
  padding: 2px 6px;
  border: none;
  background: transparent;
  color: rgba(255, 255, 255, 0.85);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  white-space: nowrap;
  user-select: none;
}
.video-controls__time:hover {
  color: #fff;
}
</style>
