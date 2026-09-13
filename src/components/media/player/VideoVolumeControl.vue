<script setup lang="ts">
// VideoVolumeControl — 静音钮 + 音量滑杆。纯 UI:props 入、emits 出,零业务。
// hasAudio===false(无音轨)→ 整组禁用 + 「无音轨」tooltip(边界 1)。缺 video_meta 行时上层按
// 有音轨传入,不误禁。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Volume2, Volume1, VolumeX, VolumeOff } from '@lucide/vue'
import UiIconButton from '../../ui/UiIconButton.vue'

const props = defineProps<{
  /** 音量 [0,1]。 */
  volume: number
  muted: boolean
  /** 是否有音轨;false 时禁用整组。 */
  hasAudio: boolean
}>()

const emit = defineEmits<{
  (e: 'toggle-mute'): void
  (e: 'set-volume', value: number): void
}>()

const { t } = useI18n()

/** 有效静音:显式静音或音量为 0。 */
const effectivelyMuted = computed(() => props.muted || props.volume <= 0)

const muteLabel = computed(() => {
  if (!props.hasAudio) return t('player.noAudioTrack')
  return effectivelyMuted.value ? t('player.unmute') : t('player.mute')
})

function onSliderInput(e: Event): void {
  const value = Number((e.target as HTMLInputElement).value)
  emit('set-volume', value)
}
</script>

<template>
  <div class="video-volume" :class="{ 'is-disabled': !hasAudio }">
    <UiIconButton
      :label="muteLabel"
      :disabled="!hasAudio"
      toggle
      :active="effectivelyMuted"
      @click="emit('toggle-mute')"
    >
      <VolumeOff v-if="!hasAudio" :size="18" />
      <VolumeX v-else-if="effectivelyMuted" :size="18" />
      <Volume1 v-else-if="volume < 0.5" :size="18" />
      <Volume2 v-else :size="18" />
    </UiIconButton>
    <input
      class="video-volume__slider"
      type="range"
      min="0"
      max="1"
      step="0.01"
      :value="effectivelyMuted ? 0 : volume"
      :disabled="!hasAudio"

      @input="onSliderInput"
    />
  </div>
</template>

<style scoped>
.video-volume {
  display: flex;
  align-items: center;
  gap: 4px;
}
.video-volume.is-disabled {
  opacity: 0.45;
}
/* 音量滑杆:横向 range,轨道/拇指用控制条恒白语义(承 VideoControlBar 深色浮层,硬编码豁免)。
   宽度收窄避免占据控制条;悬停时 accent 拇指提示可拖。 */
.video-volume__slider {
  width: 72px;
  height: 4px;
  cursor: pointer;
  accent-color: var(--color-accent);
  background: transparent;
}
.video-volume__slider:disabled {
  cursor: not-allowed;
}
</style>
