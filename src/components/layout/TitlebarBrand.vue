<template>
  <!-- 顶栏品牌标识(顶栏重构 Phase G):logo 常驻自绘标题栏左端,兼作拖拽锚点。移窗拖拽由 WindowChrome 的
       useWindowDrag 统一委托(见其顶注),故本区无需 data-tauri-drag-region,也无需 pointer-events:none 穿透。
       data-window-drag-surface:命中 padding 即 pointerdown 即时移窗(路径 B / VSCode 级);
       内部 icon/text 走 matches 判定的阈值路径,均无点击语义。 -->
  <div class="titlebar-brand" data-window-drag-surface>
    <span class="titlebar-brand__icon"><Aperture :size="18" /></span>
    <span class="titlebar-brand__text">Scrollery</span>
  </div>
</template>

<script setup lang="ts">
import { Aperture } from '@lucide/vue'
</script>

<style scoped>
.titlebar-brand {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding-inline: var(--spacing-xs);
  flex-shrink: 0;
  user-select: none;
}
.titlebar-brand__icon {
  display: inline-flex;
  color: var(--color-accent);
}
.titlebar-brand__text {
  font-size: var(--font-size-sm);
  font-weight: 700;
  color: var(--color-text-primary);
  letter-spacing: -0.3px;
  white-space: nowrap;
}
</style>
