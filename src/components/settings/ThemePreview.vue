<template>
  <div class="theme-preview" :style="{ background: palette.canvas }">
    <div class="theme-preview__sidebar" :style="{ background: palette.background }">
      <span
        v-for="row in 3"
        :key="row"
        class="theme-preview__row"
        :style="{
          background: row === 2 ? palette.selection : 'transparent',
          borderColor: palette.borderSubtle,
        }"
      >
        <span class="theme-preview__dot" :style="{ background: row === 2 ? palette.accent : palette.textTertiary }" />
        <span
          class="theme-preview__line"
          :style="{ background: row === 2 ? palette.accentText : palette.textSecondary }"
        />
      </span>
    </div>

    <div class="theme-preview__content">
      <div class="theme-preview__grid">
        <span
          v-for="cell in 4"
          :key="cell"
          class="theme-preview__cell"
          :style="{
            background: cell === 3 ? palette.selection : palette.canvasGap,
            borderColor: palette.thumbOutline,
          }"
        >
          <span
            v-if="cell === 1"
            class="theme-preview__badge"
            :style="{ background: palette.accent, color: palette.textOnAccent }"
          />
        </span>
      </div>
      <span class="theme-preview__button" :style="{ background: palette.accent, color: palette.textOnAccent }">
        {{ t('settings.themePreviewButton') }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
// 配色卡缩略预览(方案 §2 第 1 条):用**真实生成色板**渲染侧栏、文件夹行、缩略图与按钮,
// 预览与实装同源(generateTheme 纯函数),不是另一套手写色值。
// 拖动期间种子的每次变化都经 rAF 合流后再生成,与 store 的发布节奏一致(方案 §2「最多每动画帧一次」)。
import { onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { generateTheme } from '../../themes/generate'
import type { ThemeSeed, ThemeMode } from '../../themes/types'

const props = defineProps<{
  seed: ThemeSeed
  mode: ThemeMode
}>()

const { t } = useI18n()

// 首值立即生成(SSR 与首帧无 rAF 也直接可用),后续变化按帧合流。
const palette = ref(generateTheme(props.seed, props.mode))
let frame: number | null = null

function regenerate(): void {
  palette.value = generateTheme(props.seed, props.mode)
}

watch([() => props.seed, () => props.mode], () => {
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
  gap: 4px;
  height: 96px;
  padding: 4px;
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.theme-preview__sidebar {
  display: flex;
  width: 42px;
  flex-direction: column;
  justify-content: center;
  gap: 3px;
  padding: 4px;
  border-radius: 3px;
}

.theme-preview__row {
  display: flex;
  align-items: center;
  gap: 3px;
  height: 12px;
  padding: 0 3px;
  border-radius: 2px;
}

.theme-preview__dot {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  flex-shrink: 0;
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
}

.theme-preview__grid {
  display: grid;
  flex: 1;
  grid-template-columns: repeat(2, 1fr);
  gap: 4px;
}

.theme-preview__cell {
  position: relative;
  border: 1px solid;
  border-radius: 3px;
}

.theme-preview__badge {
  position: absolute;
  right: 3px;
  bottom: 3px;
  width: 14px;
  height: 6px;
  border-radius: 2px;
}

.theme-preview__button {
  align-self: flex-start;
  padding: 2px 8px;
  border-radius: var(--radius-xs);
  font-size: var(--font-size-2xs);
  line-height: 1.4;
}
</style>
