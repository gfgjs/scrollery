<script setup lang="ts">
// VideoRateMenu — 倍速菜单。当前倍速显为按钮文字(如 1×),点击开 UiPopover 列出档位
// 0.25/0.5/0.75/1/1.25/1.5/2/3(PLAYBACK_RATES 单源)。选择即 emit set-rate(已 clamp)。
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import UiIconButton from '../../ui/UiIconButton.vue'
import UiPopover from '../../ui/UiPopover.vue'
import { PLAYBACK_RATES, clampRate } from '../../../composables/player/useVideoPlayback'

defineProps<{
  /** 当前倍速(模板中直接引用 rate)。 */
  rate: number
}>()

const emit = defineEmits<{
  (e: 'set-rate', value: number): void
}>()

const { t } = useI18n()

const open = ref(false)
// UiIconButton 经 defineExpose 暴露根 button;锚点在开菜单时抓取(镜像 AppToolbar 弹层惯例)。
const btnRef = ref<InstanceType<typeof UiIconButton>>()
const anchor = ref<HTMLElement | null>(null)

function toggle(): void {
  if (!open.value) anchor.value = btnRef.value?.el ?? null
  open.value = !open.value
}

/** 倍速文字:档位本就是简洁小数(1 / 0.25 / 1.5),直接拼即可。 */
function formatRate(r: number): string {
  return `${r}×`
}

function select(r: number): void {
  emit('set-rate', clampRate(r))
  open.value = false
}
</script>

<template>
  <div class="video-rate">
    <UiIconButton
      ref="btnRef"
      :label="t('player.playbackRate')"
      :active="open || rate !== 1"
      @click="toggle"
    >
      <span class="video-rate__label">{{ formatRate(rate) }}</span>
    </UiIconButton>
    <UiPopover v-model:open="open" :anchor="anchor" placement="top">
      <div class="video-rate__menu">
        <button
          v-for="r in PLAYBACK_RATES"
          :key="r"
          type="button"
          class="video-rate__option"
          :class="{ 'is-current': r === rate }"
          @click="select(r)"
        >
          {{ formatRate(r) }}
        </button>
      </div>
    </UiPopover>
  </div>
</template>

<style scoped>
.video-rate__label {
  font-size: var(--font-size-xs);
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
/* 弹层表面视觉(UiPopover 只管定位/dismiss):用语义 token,六主题自动成立。 */
.video-rate__menu {
  display: flex;
  flex-direction: column;
  min-width: 88px;
  padding: var(--spacing-xs);
}
.video-rate__option {
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
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  transition: background var(--transition-fast);
}
.video-rate__option:hover {
  background: var(--color-bg-hover);
}
.video-rate__option.is-current {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  font-weight: 500;
}
</style>
