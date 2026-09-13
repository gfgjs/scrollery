<script setup lang="ts">
// VideoSubtitleMenu — 字幕菜单(UiPopover 形态,仿 VideoRateMenu.vue):关闭 / 内挂轨 / 外挂字幕
// 单选,「加载字幕文件…」入口;ass/ssa/mkv 内嵌轨暂不支持——固定展示提示文案(纯 UI,不做格式
// 判定,业务态全在上层 useVideoSubtitles)。
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Captions } from '@lucide/vue'
import UiIconButton from '../../ui/UiIconButton.vue'
import UiPopover from '../../ui/UiPopover.vue'
import type { SubtitleOption } from '../../../composables/player/useVideoSubtitles'

const props = defineProps<{
  /** 内挂轨选项(不含外挂)。 */
  embeddedTracks: SubtitleOption[]
  /** 已加载的外挂字幕文件名;null=未加载外挂轨(菜单不显示该选项)。 */
  externalLabel: string | null
  /** 当前选中项:'off' | `embedded-${idx}` | 'external'。 */
  activeKey: string
}>()

const emit = defineEmits<{
  (e: 'select', key: string): void
  (e: 'turn-off'): void
  (e: 'load-file'): void
}>()

const { t } = useI18n()

const open = ref(false)
const btnRef = ref<InstanceType<typeof UiIconButton>>()
const anchor = ref<HTMLElement | null>(null)

function toggle(): void {
  if (!open.value) anchor.value = btnRef.value?.el ?? null
  open.value = !open.value
}
function select(key: string): void {
  emit('select', key)
  open.value = false
}
function turnOff(): void {
  emit('turn-off')
  open.value = false
}
function loadFile(): void {
  emit('load-file')
  open.value = false
}
</script>

<template>
  <div class="video-subtitle-menu">
    <UiIconButton
      ref="btnRef"
      :label="t('player.subtitles')"
      toggle
      :active="open || props.activeKey !== 'off'"
      @click="toggle"
    >
      <Captions :size="18" />
    </UiIconButton>
    <UiPopover v-model:open="open" :anchor="anchor" placement="top">
      <div class="video-subtitle-menu__list">
        <button
          type="button"
          class="video-subtitle-menu__option"
          :class="{ 'is-current': props.activeKey === 'off' }"
          @click="turnOff"
        >
          {{ t('player.subtitleOff') }}
        </button>
        <button
          v-for="opt in props.embeddedTracks"
          :key="opt.key"
          type="button"
          class="video-subtitle-menu__option"
          :class="{ 'is-current': props.activeKey === opt.key }"
          @click="select(opt.key)"
        >
          {{ opt.label }}
        </button>
        <button
          v-if="props.externalLabel"
          type="button"
          class="video-subtitle-menu__option"
          :class="{ 'is-current': props.activeKey === 'external' }"
          @click="select('external')"
        >
          {{ t('player.subtitleExternal') }}: {{ props.externalLabel }}
        </button>
        <div class="video-subtitle-menu__divider" />
        <button type="button" class="video-subtitle-menu__option" @click="loadFile">
          {{ t('player.loadSubtitleFile') }}
        </button>
        <p class="video-subtitle-menu__hint">{{ t('player.subtitleUnsupported') }}</p>
      </div>
    </UiPopover>
  </div>
</template>

<style scoped>
/* 承 VideoRateMenu 弹层视觉(既有 token,六主题自动成立)。 */
.video-subtitle-menu__list {
  display: flex;
  flex-direction: column;
  min-width: 200px;
  padding: var(--spacing-xs);
}
.video-subtitle-menu__option {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  text-align: left;
  cursor: pointer;
  transition: background var(--transition-fast);
}
.video-subtitle-menu__option:hover {
  background: var(--color-bg-hover);
}
.video-subtitle-menu__option.is-current {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  font-weight: 500;
}
.video-subtitle-menu__divider {
  height: 1px;
  margin: var(--spacing-xs) 0;
  background: var(--color-divider);
}
.video-subtitle-menu__hint {
  margin: var(--spacing-xs) var(--spacing-sm) var(--spacing-2xs);
  color: var(--color-text-tertiary, var(--color-text-secondary));
  font-size: var(--font-size-2xs);
  line-height: 1.4;
}
</style>
