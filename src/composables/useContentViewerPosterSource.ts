// src/composables/useContentViewerPosterSource.ts
// 从 ContentViewer.vue 下沉(超长文件拆分方案 2.1):图片加载占位 + 视频海报候选(含 404 自愈)。
// 纯逻辑搬迁,不改变原调用点/参数/时序——见 docs/planning/2026-07-25-超长文件拆分方案/analysis/ContentViewer-vue.md。

import { ref, computed, watch, onMounted } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { getThumbCacheDir } from '../utils/thumbCacheDir'
import { buildThumbUrl } from './useThumbLoader'
import { IPC } from '../constants/ipc'
import type { MediaDetail } from '../types/media'

export function useContentViewerPosterSource(options: { detail: () => MediaDetail | null }) {
  const { detail } = options

  // 缓存缩略图同时服务图片完整解码前的目标占位与视频首帧海报。只消费 thumb_status=1 的
  // 独立缓存文件；status=3 指向原文件本身，坏图时仍会失败，不能拿来做加载占位。
  const thumbCacheDir = ref('')
  const imagePlaceholderCandidate = computed<string | null>(() => {
    const item = detail()
    if (
      item?.mediaType !== 'image' ||
      item.thumbStatus !== 1 ||
      !item.thumbPath ||
      !thumbCacheDir.value
    ) {
      return null
    }
    return buildThumbUrl(item.thumbStatus, item.thumbPath, thumbCacheDir.value)
  })
  const imagePlaceholderBroken = ref(false)
  watch(imagePlaceholderCandidate, () => {
    imagePlaceholderBroken.value = false
  })
  const imagePlaceholder = computed(() =>
    imagePlaceholderBroken.value ? null : imagePlaceholderCandidate.value,
  )

  function onImagePlaceholderError(event: Event) {
    const image = event.currentTarget as HTMLImageElement
    if (image.getAttribute('src') === imagePlaceholderCandidate.value) {
      imagePlaceholderBroken.value = true
    }
  }

  // 视频派生流水线会把首帧封面镜像到 media_items.thumb_status=1 / thumb_path；原生 <video>
  // 此前从未接过 poster。补上后:
  //   ① 打开视频到首帧解码呈现前不再黑屏一瞬,先显封面;
  //   ② autoplay 被浏览器策略拦截(如未交互)时仍有可辨画面而非全黑。
  // 走 buildThumbUrl 单源(全项目缩略图 URL 唯一解析路径),与网格像素一致,不引第二套逻辑。
  // cacheDir 经 IPC 取一次(所有缩略图消费者统一惯例);未就绪 / 无封面(status≠1,如尚在生成或后端
  // 缺失)时返回 undefined → Vue 省略 poster 特性,不会绑成空串。
  const videoPosterCandidate = computed<string | null>(() => {
    const item = detail()
    if (item?.mediaType !== 'video' || !thumbCacheDir.value) return null
    return buildThumbUrl(item.thumbStatus, item.thumbPath, thumbCacheDir.value)
  })
  // 封面文件可能被缓存 LRU 驱逐而 DB 仍标 status=1(网格侧同款问题由 useThumbLoader 自愈)。
  // 绑死 URL 会让 <video poster> 静默 404;<video> 无 poster 失败事件,故先用 Image 探测:
  // 失败 → 本次隐藏 poster(回退旧「无封面」行为)+ 走网格同一自愈通道重生成(每项每会话至多
  // 一次,防后端持续失败打转)。重生成完成不回头刷新本次——下次打开该视频即恢复。
  const posterBroken = ref(false)
  const healedPosterIds = new Set<number>()
  watch(
    videoPosterCandidate,
    (url) => {
      posterBroken.value = false
      if (!url) return
      const img = new Image()
      img.onerror = () => {
        if (url !== videoPosterCandidate.value) return // 已切换查看项,过期探测丢弃
        posterBroken.value = true
        const id = detail()?.id
        if (id != null && !healedPosterIds.has(id)) {
          healedPosterIds.add(id)
          void invokeIpc(IPC.REGENERATE_MISSING_THUMB, { id }).catch(() => {})
        }
      }
      img.src = url
    },
    { immediate: true },
  )
  const videoPoster = computed<string | undefined>(() =>
    posterBroken.value ? undefined : (videoPosterCandidate.value ?? undefined),
  )

  // 缩略图缓存根:图片加载占位与视频 poster 共用(status=1 走 cacheDir/thumbnails/path)。
  // 失败静默降级为中性图片加载态 / 无 poster，主媒体加载不受影响。
  onMounted(() => {
    void getThumbCacheDir()
      .then((dir) => {
        thumbCacheDir.value = dir
      })
      .catch(() => {})
  })

  return {
    thumbCacheDir,
    imagePlaceholder,
    onImagePlaceholderError,
    videoPoster,
  }
}
