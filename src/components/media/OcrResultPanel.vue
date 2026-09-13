<script setup lang="ts">
// OcrResultPanel — OCR 文字提取结果面板(T9)。消费 useOcr 单例状态,图片/视频两态共用同一实例
// (ContentViewer 挂载点固定,视频态触发也呈现在此)。
//
// 行文本区 `user-select: text` 允许手动选中复制;底部「复制全部」按钮触发 useOcr.copyAll()
// (仅用户点击才写剪贴板)。行数已在后端按 max_lines_per_image cap(1000),故这里是天然有界列表,
// 显式豁免虚拟化——普通滚动即可,不需要 virtual scroller 的额外复杂度。
import { Copy, X } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import { useOcr } from '../../composables/useOcr'
import UiIconButton from '../ui/UiIconButton.vue'

const { t } = useI18n()
const ocr = useOcr()
</script>

<template>
  <div v-if="ocr.panelOpen.value" class="ocr-panel" @click.stop @mousedown.stop>
    <div class="ocr-panel__header">
      <div class="ocr-panel__title">
        <span>{{ t('ocr.resultTitle') }}</span>
        <span class="ocr-panel__source" :title="ocr.sourceLabel.value">{{ ocr.sourceLabel.value }}</span>
      </div>
      <UiIconButton :label="t('ocr.close')" @click="ocr.closePanel">
        <X :size="16" />
      </UiIconButton>
    </div>
    <!-- 有界列表显式豁免虚拟化:后端已按 max_lines_per_image(1000)cap,普通滚动足够。 -->
    <div class="ocr-panel__body">
      <p v-for="(line, idx) in ocr.result.value?.lines ?? []" :key="idx" class="ocr-panel__line">
        {{ line.text }}
      </p>
    </div>
    <div class="ocr-panel__footer">
      <button type="button" class="ocr-panel__copy" @click="ocr.copyAll">
        <Copy :size="14" />
        {{ t('ocr.copy') }}
      </button>
      <button type="button" class="ocr-panel__close" @click="ocr.closePanel">
        {{ t('ocr.close') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
/* OCR 面板是查看器上的普通浮层，统一消费 material float recipe，避免和查看器黑画布脱节。 */
.ocr-panel {
  position: absolute;
  top: var(--spacing-md);
  right: var(--spacing-md);
  width: min(420px, calc(100% - var(--spacing-md) * 2));
  max-height: 60vh;
  display: flex;
  flex-direction: column;
  background: var(--material-recipe-float-background-color);
  border: 1px solid var(--material-recipe-float-border-color);
  border-radius: var(--radius-xl);
  box-shadow: var(--material-recipe-float-box-shadow);
  backdrop-filter: var(--material-recipe-float-backdrop-filter);
  -webkit-backdrop-filter: var(--material-recipe-float-backdrop-filter);
  color: var(--color-text-primary);
  z-index: 20;
  overflow: hidden;
}
.ocr-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--color-divider);
  flex-shrink: 0;
}
.ocr-panel__title {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  font-size: var(--font-size-sm);
  font-weight: 600;
}
.ocr-panel__source {
  font-size: var(--font-size-2xs);
  font-weight: 400;
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ocr-panel__body {
  overflow-y: auto;
  padding: var(--spacing-sm) var(--spacing-md);
  user-select: text;
}
/* 选中文本沿用主题 accent 角色，亮/暗主题都保持可读。 */
.ocr-panel__body ::selection {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}
.ocr-panel__line {
  margin: 0 0 6px;
  font-size: var(--font-size-sm);
  line-height: 1.5;
  word-break: break-word;
}
.ocr-panel__line:last-child {
  margin-bottom: 0;
}
.ocr-panel__footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  border-top: 1px solid var(--color-divider);
  flex-shrink: 0;
}
.ocr-panel__copy,
.ocr-panel__close {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
  font-size: var(--font-size-xs);
  cursor: pointer;
}
.ocr-panel__copy:hover,
.ocr-panel__close:hover {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
}
</style>
