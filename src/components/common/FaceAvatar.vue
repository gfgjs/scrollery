<template>
  <!-- 可复用裁脸头像（T10 审批 / 人物墙）：整图缩略图 cover + 按 bbox 中心定位（v1 近似裁剪）。
       thumbPath/thumbStatus 约定同 PersonSummary（status=1 相对缓存 / 3 绝对源路径）。 -->
  <div class="face-avatar" :style="style">
    <ScanFace v-if="!src" :size="Math.round(size * 0.42)" class="face-avatar__fallback" />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { ScanFace } from '@lucide/vue'
import { buildThumbUrl } from '../../composables/useThumbLoader'

const props = withDefaults(
  defineProps<{
    /** 缩略图路径（相对缓存名 或 绝对源路径，依 thumbStatus）。 */
    thumbPath: string | null
    /** 1=相对缓存（拼 cacheDir/thumbnails/）/ 3=绝对源路径 / 其他=无图回退占位。 */
    thumbStatus: number | null
    /** 归一化 [x, y, w, h]，定位脸中心；null 时居中。 */
    bbox: [number, number, number, number] | null
    /** 应用缓存目录（解析 status=1 相对路径用）。 */
    cacheDir: string
    /** 头像直径（px）。 */
    size?: number
  }>(),
  { size: 72 },
)

// 路径 → Tauri asset:// URL(status=1 相对缓存 / 3 绝对源)。
// 构造逻辑收敛到 buildThumbUrl 单源(useThumbLoader),防内联版漂移。
const src = computed<string | null>(() => {
  if (props.thumbStatus === 1 && !props.cacheDir) return null
  try {
    return buildThumbUrl(props.thumbStatus ?? 0, props.thumbPath, props.cacheDir)
  } catch {
    return null
  }
})

const style = computed<Record<string, string>>(() => {
  const dim = `${props.size}px`
  const base: Record<string, string> = { width: dim, height: dim }
  if (!src.value) return base
  const bb = props.bbox
  const cx = bb ? (bb[0] + bb[2] / 2) * 100 : 50
  const cy = bb ? (bb[1] + bb[3] / 2) * 100 : 50
  return {
    ...base,
    backgroundImage: `url("${src.value}")`,
    backgroundPosition: `${cx}% ${cy}%`,
  }
})
</script>

<style scoped>
.face-avatar {
  border-radius: 50%;
  background-color: var(--color-bg-primary);
  background-size: cover;
  background-repeat: no-repeat;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  flex-shrink: 0;
}
.face-avatar__fallback {
  color: var(--color-text-tertiary);
}
</style>
