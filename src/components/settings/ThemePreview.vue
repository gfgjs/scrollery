<template>
  <div
    class="theme-preview"
    data-theme-scope
    :data-color-scheme="mode"
    :data-visual-style="definition.visualStyle === 'standard' ? undefined : definition.visualStyle"
    :style="paletteVars"
  >
    <div class="theme-preview__titlebar" />
    <div class="theme-preview__body">
      <div class="theme-preview__sidebar">
        <span
          v-for="row in 3"
          :key="row"
          class="theme-preview__row"
          :style="{ background: row === 2 ? palette.shellSelection : 'transparent' }"
        >
          <span class="theme-preview__dot" :style="{ background: row === 2 ? palette.accent : palette.shellTextTertiary }" />
          <span class="theme-preview__line" :style="{ background: row === 2 ? palette.shellAccentText : palette.shellTextSecondary }" />
        </span>
      </div>
      <div class="theme-preview__content">
        <div class="theme-preview__toolbar" />
        <div class="theme-preview__grid">
          <span v-for="cell in 4" :key="cell" class="theme-preview__cell" />
        </div>
        <span class="theme-preview__button">{{ t('settings.themePreviewButton') }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { generateTheme, paletteToCssVars } from '../../themes/generate'
import type { ThemeDefinition, ThemeMode } from '../../themes/types'

const props = defineProps<{ definition: ThemeDefinition; mode: ThemeMode }>()
const { t } = useI18n()
const palette = ref(generateTheme(props.definition[props.mode], props.mode, props.definition.visualStyle))
const paletteVars = computed(() => paletteToCssVars(palette.value))
let frame: number | null = null

function regenerate(): void {
  palette.value = generateTheme(props.definition[props.mode], props.mode, props.definition.visualStyle)
}

watch([() => props.definition, () => props.mode], () => {
  if (typeof requestAnimationFrame !== 'function') {
    regenerate()
    return
  }
  if (frame !== null) return
  frame = requestAnimationFrame(() => {
    frame = null
    regenerate()
  })
})

onUnmounted(() => {
  if (frame !== null && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(frame)
})
</script>

<style scoped>
.theme-preview {
  display: flex;
  flex-direction: column;
  height: 96px;
  overflow: hidden;
  border: 1px solid var(--color-shell-border);
  border-radius: calc(var(--radius-md) * 0.5);
  background: var(--color-shell-bg-primary);
}
.theme-preview__titlebar {
  height: 10px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-shell-border);
  background: var(--color-shell-bg-secondary);
}
.theme-preview__body {
  display: flex;
  min-height: 0;
  flex: 1;
}
.theme-preview__sidebar {
  position: relative;
  display: flex;
  width: 42px;
  flex-shrink: 0;
  flex-direction: column;
  justify-content: center;
  gap: 3px;
  padding: 4px;
  border-right: 1px solid var(--color-shell-border);
  background: var(--color-shell-bg-secondary);
}
.theme-preview__row {
  display: flex;
  align-items: center;
  gap: 3px;
  height: 12px;
  padding: 0 3px;
  border-radius: calc(var(--radius-sm) * 0.5);
}
.theme-preview__dot {
  width: 4px;
  height: 4px;
  flex-shrink: 0;
  border-radius: 50%;
}
.theme-preview__line {
  height: 3px;
  flex: 1;
  border-radius: 2px;
}
.theme-preview__content {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 4px;
  margin: calc(var(--theme-main-inset-top) * 0.5) calc(var(--theme-main-inset-right) * 0.5)
    calc(var(--theme-main-inset-bottom) * 0.5) calc(var(--theme-main-inset-left) * 0.5);
  padding: 4px;
  overflow: hidden;
  border: var(--theme-main-border-width) solid var(--theme-main-border-color);
  border-radius: calc(var(--theme-main-radius) * 0.5);
  background: var(--color-bg-primary);
}
.theme-preview__toolbar {
  height: 5px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border);
}
.theme-preview__grid {
  display: grid;
  min-height: 0;
  flex: 1;
  grid-template-columns: repeat(2, 1fr);
  gap: 4px;
}
.theme-preview__cell {
  border: 1px solid var(--color-thumb-outline);
  border-radius: 2px;
  background: var(--color-bg-canvas-placeholder);
}
.theme-preview__button {
  align-self: flex-start;
  padding: 2px 8px;
  border-radius: calc(var(--radius-sm) * 0.5);
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  font-size: var(--font-size-2xs);
  line-height: 1.4;
  box-shadow: 0 calc(var(--theme-button-depth) * 0.5) 0 var(--color-accent-hover);
}
</style>
