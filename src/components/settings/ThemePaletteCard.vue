<template>
  <section class="palette-card" :class="{ 'palette-card--previewing': previewing }">
    <header class="palette-card__header">
      <span class="palette-card__title">
        {{ mode === 'light' ? t('settings.themeLight') : t('settings.themeDark') }}
      </span>
      <span class="palette-card__actions">
        <UiButton variant="ghost" size="sm" @click="emit('preview')">
          {{ previewing ? t('settings.themePreviewing') : t('settings.themePreviewThisMode') }}
        </UiButton>
        <UiButton variant="ghost" size="sm" @click="emit('reset')">
          {{ t('settings.themeResetMode') }}
        </UiButton>
      </span>
    </header>

    <ThemePreview :seed="seed" :mode="mode" />

    <div class="palette-card__colors">
      <ThemeColorPicker
        :model-value="seed.background"
        :label="t('settings.themeBackground')"
        @update:model-value="emit('update', { background: $event })"
      />
      <ThemeColorPicker
        :model-value="seed.foreground"
        :label="t('settings.themeForeground')"
        @update:model-value="emit('update', { foreground: $event })"
      />
      <ThemeColorPicker
        :model-value="seed.accent"
        :label="t('settings.themeAccent')"
        @update:model-value="emit('update', { accent: $event })"
      />
    </div>

    <label class="palette-card__contrast">
      <span>{{ t('settings.themeContrast') }}</span>
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        :value="seed.contrast"
        @input="emit('update', { contrast: Number(($event.target as HTMLInputElement).value) })"
      />
      <span class="palette-card__contrast-value">{{ seed.contrast }}</span>
    </label>

    <div class="palette-card__gallery">
      <label class="palette-card__gallery-toggle">
        <input
          type="checkbox"
          :checked="seed.gallery === GALLERY_AUTO"
          @change="onGalleryAutoChange"
        />
        {{ t('settings.themeGalleryAuto') }}
      </label>
      <ThemeColorPicker
        v-if="seed.gallery !== GALLERY_AUTO"
        :model-value="seed.gallery"
        :label="t('settings.themeGallery')"
        @update:model-value="emit('update', { gallery: $event })"
      />
      <span v-else class="palette-card__hint">{{ t('settings.themeGalleryAutoHint') }}</span>
    </div>
  </section>
</template>

<script setup lang="ts">
// 单套配色卡(方案 §2):真实色板预览 + 三个色块与 HEX + 层次对比度滑条 + 画廊背景。
// 只呈现与上抛改动,草稿由 themeStore 持有(浅／深两卡共用同一份草稿)。
import { useI18n } from 'vue-i18n'
import UiButton from '../ui/UiButton.vue'
import ThemeColorPicker from './ThemeColorPicker.vue'
import ThemePreview from './ThemePreview.vue'
import { GALLERY_AUTO, type ThemeMode, type ThemeSeed } from '../../themes/types'

const props = defineProps<{
  mode: ThemeMode
  seed: ThemeSeed
  /** 该模式是否正被临时预览(界面外观偏好不因此改变)。 */
  previewing: boolean
}>()

const emit = defineEmits<{
  update: [patch: Partial<ThemeSeed>]
  preview: []
  reset: []
}>()

const { t } = useI18n()

/** 勾选「跟随界面」写回 auto;取消勾选落到当前界面底色作为起点,避免出现空值。 */
function onGalleryAutoChange(event: Event): void {
  const auto = (event.target as HTMLInputElement).checked
  emit('update', { gallery: auto ? GALLERY_AUTO : props.seed.background })
}
</script>

<style scoped>
.palette-card {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-surface);
}

.palette-card--previewing {
  border-color: var(--color-accent);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.palette-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
}

.palette-card__title {
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  font-weight: 600;
}

.palette-card__actions {
  display: flex;
  gap: var(--spacing-2xs);
}

.palette-card__colors {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.palette-card__contrast {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}

.palette-card__contrast input {
  flex: 1;
  min-width: 0;
}

.palette-card__contrast-value {
  width: 2.5em;
  color: var(--color-text-tertiary);
  font-family: var(--font-mono);
  text-align: right;
}

.palette-card__gallery {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding-top: var(--spacing-xs);
  border-top: 1px solid var(--color-divider);
}

.palette-card__gallery-toggle {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  user-select: none;
}

.palette-card__hint {
  color: var(--color-text-tertiary);
  font-size: var(--font-size-xs);
}
</style>
