// src/composables/player/useSeekbarPreview.ts
// seekbar hover 关键帧预览(播放器线 GE④,计划 §裁决「复用 sprite 产物,纳入正编」):复用
// GET_KEYFRAME_SPRITE IPC + 帧映射数学(逻辑抄 useHoverPreview.ts:116-125 scrubStyle 与
// :151-167 resolveSprite/30s 负缓存 TTL 的模式)。**只抄逻辑,不 import 该模块的共享状态**——
// useHoverPreview 的模块级 activeId 单例池是网格悬停互斥语义(同一时刻仅一格预览),与本 seekbar
// 场景(播放器内单条视频的进度条 hover)混用会互踢,故本文件自持独立、不共享的会话态(计划 §裁决 0.4)。
//
// sprite 缺失/驱逐(边界 12):IPC 返回 null 记 30s 负缓存;拿到路径后再用 Image() 预加载校验一次
// ——404/损坏文件会在此 onerror 落空,styleForRatio 相应返回 null,上层(VideoSeekBar)据此撤预览、
// 只留时间 tooltip(镜像 onerror 接住语义)。

import { ref, computed, watch, type Ref } from 'vue'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { resolveAssetUrl } from '../../utils/assetUrl'
import { useUiStore } from '../../stores/uiStore'

/** sprite 负结果重试间隔(镜像 useHoverPreview.SPRITE_MISS_TTL_MS)。 */
const SPRITE_MISS_TTL_MS = 30_000
/** 关键帧数兜底默认值(镜像 useHoverPreview.KEYFRAME_COUNT_FALLBACK)。 */
const KEYFRAME_COUNT_FALLBACK = 10

/** 用 Image() 预加载校验 URL 可解码(404/损坏文件会触发 onerror)。 */
function preloadImage(url: string): Promise<boolean> {
  return new Promise((resolve) => {
    const img = new Image()
    img.onload = () => resolve(true)
    img.onerror = () => resolve(false)
    img.src = url
  })
}

export function useSeekbarPreview(itemId: Ref<number | null>) {
  const ui = useUiStore()
  /** 已解析且预加载校验通过的 sprite URL;''=当前条目无可用 sprite。 */
  const spriteSrc = ref('')

  // 本 composable 实例私有的会话级缓存(非模块级——见文件头「只抄逻辑不共享状态」)。
  const spriteCache = new Map<number, string>()
  const spriteMissAt = new Map<number, number>()
  let resolveToken = 0

  async function resolveSprite(id: number): Promise<string> {
    const cached = spriteCache.get(id)
    if (cached) return cached
    const missAt = spriteMissAt.get(id)
    if (missAt !== undefined && Date.now() - missAt < SPRITE_MISS_TTL_MS) return ''
    try {
      const path = await invokeIpc<string | null>(IPC.GET_KEYFRAME_SPRITE, { itemId: id })
      if (!path) {
        spriteMissAt.set(id, Date.now())
        return ''
      }
      const url = resolveAssetUrl(path)
      const ok = await preloadImage(url)
      if (!ok) {
        spriteMissAt.set(id, Date.now())
        return ''
      }
      spriteCache.set(id, url)
      spriteMissAt.delete(id)
      return url
    } catch {
      spriteMissAt.set(id, Date.now())
      return ''
    }
  }

  // 切条目(边界 8 token 防御):迟到的旧条目解析结果按 token 丢弃。
  watch(
    itemId,
    (id) => {
      const myToken = ++resolveToken
      spriteSrc.value = ''
      if (id == null) return
      resolveSprite(id).then((url) => {
        if (myToken !== resolveToken) return
        spriteSrc.value = url
      })
    },
    { immediate: true },
  )

  /** 有可用 sprite(供上层决定是否渲染预览浮层的缩略帧区)。 */
  const hasSprite = computed(() => !!spriteSrc.value)

  /**
   * 按 hover 比例 [0,1] 算 sprite 背景样式(帧映射数学镜像 useHoverPreview.scrubStyle);
   * 无 sprite 时返回 null——上层(VideoSeekBar)据此只留时间 tooltip。
   */
  function styleForRatio(ratio: number): Record<string, string> | null {
    if (!spriteSrc.value) return null
    const cols = ui.videoKeyframeCount || KEYFRAME_COUNT_FALLBACK
    const clamped = Math.min(1, Math.max(0, ratio))
    const frame = cols > 1 ? Math.min(cols - 1, Math.floor(clamped * cols)) : 0
    const posX = cols > 1 ? (frame / (cols - 1)) * 100 : 0
    return {
      backgroundImage: `url("${spriteSrc.value}")`,
      backgroundSize: `${cols * 100}% 100%`,
      backgroundPosition: `${posX}% 0%`,
      backgroundRepeat: 'no-repeat',
    }
  }

  return { hasSprite, styleForRatio }
}
