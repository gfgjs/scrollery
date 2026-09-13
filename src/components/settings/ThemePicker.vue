<template>
  <div class="theme-picker">
    <!-- 外观模式:亮 / 暗 / 跟随系统。 -->
    <div class="theme-picker__mode" role="group" :aria-label="$t('settings.themeMode')">
      <button
        v-for="option in modeOptions"
        :key="option.value"
        type="button"
        class="theme-picker__mode-btn"
        :class="{ active: ui.appearance === option.value }"
        :aria-pressed="ui.appearance === option.value"
        @click="ui.setAppearance(option.value)"
      >
        <component :is="option.icon" :size="14" aria-hidden="true" />
        <span>{{ $t(option.labelKey) }}</span>
      </button>
    </div>

    <!-- 风格卡片同时展示明暗配对，选择一次即可更新两个槽位。 -->
    <div class="theme-picker__styles">
      <div class="theme-picker__heading">
        <span class="theme-picker__title">{{ $t('settings.themeStyle') }}</span>
        <span class="theme-picker__hint">{{ $t('settings.themeStyleDesc') }}</span>
      </div>

      <div class="theme-picker__grid">
        <button
          v-for="style in styles"
          :key="style.id"
          type="button"
          class="theme-card"
          :class="{ selected: ui.themeStyle === style.id }"
          :aria-pressed="ui.themeStyle === style.id"
          @click="pickStyle(style.id)"
        >
          <span class="theme-card__previews">
            <span
              v-for="kind in previewKinds"
              :key="kind"
              class="theme-card__preview"
              :aria-label="$t(kind === 'light' ? 'settings.themeLight' : 'settings.themeDark')"
            >
              <span
                class="theme-card__preview-canvas"
                :style="{ backgroundColor: style.themes[kind].preview.bg }"
              >
                <span
                  class="theme-card__preview-surface"
                  :style="{ backgroundColor: style.themes[kind].preview.surface }"
                >
                  <span
                    class="theme-card__preview-text"
                    :style="{ backgroundColor: style.themes[kind].preview.text }"
                  />
                  <span
                    class="theme-card__preview-accent"
                    :style="{ backgroundColor: style.themes[kind].preview.accent }"
                  />
                </span>
              </span>
              <span class="theme-card__preview-label">
                {{ $t(kind === 'light' ? 'settings.themeLight' : 'settings.themeDark') }}
              </span>
            </span>
          </span>

          <span class="theme-card__name">
            <span>{{ $t(style.nameKey) }}</span>
            <Check v-if="ui.themeStyle === style.id" :size="14" aria-hidden="true" />
          </span>
          <span class="theme-card__description">{{ $t(style.descriptionKey) }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Check, Monitor, Moon, Sun } from '@lucide/vue'
import { useUiStore } from '../../stores/uiStore'
import { THEME_STYLES } from '../../themes/registry'
import type { ThemeKind, ThemeStyle } from '../../themes/registry'
import type { AppearanceMode } from '../../types/ui'

const ui = useUiStore()
const styles = THEME_STYLES
const previewKinds: readonly ThemeKind[] = ['light', 'dark']

const modeOptions: { value: AppearanceMode; icon: typeof Sun; labelKey: string }[] = [
  { value: 'light', icon: Sun, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: Moon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: Monitor, labelKey: 'settings.themeSystem' },
]

function pickStyle(style: ThemeStyle): void {
  ui.setThemeStyle(style)
}
</script>

<style scoped>
.theme-picker {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
  padding: var(--spacing-sm) var(--spacing-md) var(--spacing-md);
}

.theme-picker__mode {
  display: inline-flex;
  align-self: flex-start;
  gap: 2px;
  padding: 2px;
  background: var(--color-bg-inset);
  border-radius: var(--radius-md);
}

.theme-picker__mode-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast);
}

.theme-picker__mode-btn:hover {
  color: var(--color-text-primary);
}

.theme-picker__mode-btn.active {
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
  box-shadow: var(--shadow-sm);
}

.theme-picker__styles {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.theme-picker__heading {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 6px var(--spacing-sm);
}

.theme-picker__title {
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  font-weight: 600;
}

.theme-picker__hint {
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}

.theme-picker__grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
  gap: var(--spacing-sm);
}

.theme-card {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: var(--spacing-xs);
  padding: var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-surface);
  color: var(--color-text-primary);
  text-align: left;
  cursor: pointer;
  transition:
    border-color var(--transition-fast),
    background var(--transition-fast),
    box-shadow var(--transition-fast);
}

.theme-card:hover {
  border-color: var(--color-border-strong);
  background: var(--color-bg-hover);
}

.theme-card:active {
  background: var(--color-bg-active);
}

.theme-card.selected {
  border-color: var(--color-accent);
  background: var(--color-accent-subtle);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.theme-card__previews {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
}

.theme-card__preview {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}

.theme-card__preview-canvas {
  position: relative;
  display: block;
  height: 52px;
  padding: 6px;
  overflow: hidden;
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--radius-sm);
}

.theme-card__preview-surface {
  position: relative;
  display: block;
  width: 78%;
  height: 100%;
  border-radius: 3px;
}

.theme-card__preview-text,
.theme-card__preview-accent {
  position: absolute;
  display: block;
  height: 4px;
  border-radius: 2px;
}

.theme-card__preview-text {
  top: 10px;
  left: 8px;
  width: 58%;
}

.theme-card__preview-accent {
  bottom: 8px;
  left: 8px;
  width: 30%;
}

.theme-card__preview-label {
  overflow: hidden;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-2xs);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.theme-card__name {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-xs);
  margin-top: 2px;
  font-size: var(--font-size-sm);
  font-weight: 600;
}

.theme-card__name svg {
  flex-shrink: 0;
  color: var(--color-accent);
}

.theme-card__description {
  min-height: 2.6em;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  line-height: 1.35;
}
</style>
