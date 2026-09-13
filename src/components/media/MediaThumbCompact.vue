<template>
  <!-- 极密网格专用轻量卡片(T1 §6):w/h<100px 时替代 MediaThumb。只渲染 60px 尺度下真正可见的东西——
       占位 + <img> + 颜色色条 + 可用态角标 + 选中态(纯 class)。刻意不含:useHoverPreview / 徽章 /
       评分 / 收藏 / info-overlay / thumbInfoLines(7 依赖) —— 这些在 60px 下看不清/点不动,却 ×数千格
       构成快滚 churn 与常驻内存主源(维度 B)。加载逻辑复用 useThumbLoader 单源,与 MediaThumb 零漂移。 -->
  <div
    class="media-thumb media-thumb--compact"
    :style="thumbStyle"
    :class="{
      loaded: isLoaded,
      'media-thumb--placeholder': !isLoaded,
      'media-thumb--selected': isSelected,
      'media-thumb--missing': availability === 'missing',
      'media-thumb--offline': availability === 'offline',
    }"
  >
    <!-- 可用态角标(缺失检测 Part2 §3.2):missing/offline 始终可见,一眼可辨。 -->
    <div
      v-if="availability !== 'online'"
      class="media-thumb__avail"
      :class="'avail--' + availability"
      :title="
        availability === 'missing' ? $t('media.availMissingTitle') : $t('media.availOfflineTitle')
      "
    >
      {{ availability === 'missing' ? $t('media.availMissing') : $t('settings.volOffline') }}
    </div>
    <!-- 颜色标签色条(T16):缩略图顶缘细条(Lightroom 式)。 -->
    <div v-if="colorStrip" class="media-thumb__color-strip" :style="{ background: colorStrip }"></div>
    <!-- 平均色占位(后端 hydrate 算好)+ 文件格式文本(未加载时)。 -->
    <div
      v-if="!isLoaded"
      class="media-thumb__placeholder"
      :style="{ backgroundColor: placeholderBgColor }"
    >
      <span v-if="fileFormat" class="media-thumb__ext">{{ fileFormat.toUpperCase() }}</span>
    </div>
    <!-- 实际图片。 -->
    <img
      v-if="displaySrc"
      class="media-thumb__img thumb-loaded"
      :src="displaySrc"
      :width="w"
      :height="h"
      loading="lazy"
      @error="onError"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { colorLabelHex } from '../../constants/colorLabels'
import { useThumbLoader } from '../../composables/useThumbLoader'

interface Props {
  id: number
  w: number
  h: number
  mediaType: string
  thumbStatus: number
  thumbPath?: string | null
  /** 占位平均色 CSS `#rrggbb`（后端 hydrate 算好；null → 回退 CSS 变量）。 */
  placeholderColor?: string | null
  fileFormat?: string
  /** 用户颜色标签 0-7(0=未标)。 */
  colorLabel?: number
  /** 系统可用态 'online'|'offline'|'missing'。 */
  availability?: string
  isSelected?: boolean
  cacheDir: string
}

const props = withDefaults(defineProps<Props>(), {
  thumbPath: null,
  placeholderColor: null,
  colorLabel: 0,
  availability: 'online',
  isSelected: false,
})

const emit = defineEmits<{
  (e: 'request-thumb', id: number): void
  (e: 'cancel-thumb', id: number): void
  (e: 'regenerate-thumb', id: number): void
}>()

// 加载单源(与 MediaThumb 共享)：status 1/3/0 解码 + 懒自愈。
const { isLoaded, displaySrc, onError } = useThumbLoader(
  {
    id: () => props.id,
    thumbStatus: () => props.thumbStatus,
    thumbPath: () => props.thumbPath,
    cacheDir: () => props.cacheDir,
  },
  {
    requestThumb: (id) => emit('request-thumb', id),
    cancelThumb: (id) => emit('cancel-thumb', id),
    regenerateThumb: (id) => emit('regenerate-thumb', id),
  },
)

const thumbStyle = computed(() => ({
  width: `${props.w}px`,
  height: `${props.h}px`,
}))

// 颜色标签色条颜色：0/未标 → null(不渲染色条)。
const colorStrip = computed<string | null>(() => colorLabelHex(props.colorLabel))

// 回退 bg-elevated:与 MediaThumb 同步(compact 永不进悬停卡,无需画格同源覆盖)。
const placeholderBgColor = computed(() => props.placeholderColor ?? 'var(--color-bg-elevated)')
</script>

<style scoped>
/* 样式镜像 MediaThumb 的 compact 可见子集(单源在 MediaThumb;此处为 compact 独立组件的等价副本,
   仅含 60px 下真正会显示的规则)。改视觉须两处同步——但本组件永不含 hover/徽章/评分,面积极小。 */
.media-thumb {
  position: relative;
  overflow: hidden;
  border-radius: 2px;
  background: var(--color-bg-elevated);
}

/* 格内 1px 内描边(--color-thumb-outline):与 MediaThumb / canvas strokeRect 同 token 同源,
   极密网格下白底图贴亮底同样需要图界。零模糊 box-shadow,×数千格成本仍可忽略。 */
.media-thumb::before {
  content: '';
  position: absolute;
  inset: 0;
  z-index: 6;
  box-shadow: inset 0 0 0 1px var(--color-thumb-outline);
  border-radius: inherit;
  pointer-events: none;
}

.media-thumb__placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.media-thumb__ext {
  font-family: var(--font-mono);
  font-size: 14px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.4);
  letter-spacing: 1px;
}

.media-thumb__img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

/* 颜色标签色条(T16)：顶缘 4px 细条。 */
.media-thumb__color-strip {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 4px;
  z-index: 11;
  pointer-events: none;
}

/* ── 选中态 ── */
.media-thumb--selected {
  border-radius: var(--radius-lg);
  /* 与标准缩略图同源：只加 2px 内描边，不改变格子尺寸或内容亮度。 */
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

/* ── 可用态：missing/offline 置灰 + 角标 ── */
.media-thumb--missing {
  filter: grayscale(1) brightness(0.85);
  opacity: 0.5;
}
.media-thumb--offline {
  filter: grayscale(0.7);
  opacity: 0.72;
}
.media-thumb__avail {
  position: absolute;
  bottom: var(--spacing-2xs); /* 左下角:让出左上给拖拽手柄(与 MediaThumb 对齐;compact 无评分星,不冲突) */
  left: var(--spacing-2xs);
  z-index: 11;
  font-size: var(--font-size-2xs);
  font-weight: 700;
  line-height: 1;
  padding: var(--spacing-2xs) var(--spacing-xs);
  border-radius: var(--radius-xs);
  color: #fff;
  letter-spacing: 0.04em;
  pointer-events: auto;
}
.media-thumb__avail.avail--missing {
  background: var(--color-error);
  color: var(--color-text-on-error);
}
.media-thumb__avail.avail--offline {
  background: var(--color-bg-elevated);
  color: var(--color-text-primary);
}

/* ── Compact 极限优化：禁用 transition/额外合成层,contain:strict 隔离内部变化。 ── */
.media-thumb--compact {
  transition: none;
  contain: strict;
}
</style>
