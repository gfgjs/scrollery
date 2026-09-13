// src/composables/useContentViewerPreload.ts
// 大图查看器前后邻近项预加载与极速飞掠保护：
// 1. 邻近元数据预取：空闲时预查 N+1 与 N-1 的 MediaDetail 缓存在内存中，消除翻页 IPC 往返延迟。
// 2. 邻近缩略图预热：空闲时将前后缩略图预热入浏览器缓存，翻页瞬间 0ms 全幅秒现。
// 3. 高清原图空闲预解码：当前图就绪后，沿浏览方向对前方 1 张原图执行低优先级 Image.decode()。
// 4. 极速飞掠保护（Burst Mode）：鼠标滚轮猛滚或按住方向键连翻时，自动进入极速模式，跳过重型原图解码，
//    仅流水线渲染轻量缩略图保持满帧，停稳后（settle）再触发最终停留项的高清解码。

import { ref, watch, onScopeDispose, type Ref } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { resolveAssetUrl } from '../utils/assetUrl'
import { buildThumbUrl } from './useThumbLoader'
import type { LensAdjacentMedia, MediaDetail } from '../types/media'
import type { useMediaStore } from '../stores/mediaStore'

export interface ViewerPreloadOptions {
  detail: () => MediaDetail | null
  media: ReturnType<typeof useMediaStore>
  thumbCacheDir: () => string
  isHighResReady: () => boolean
  isBursting?: Ref<boolean>
  /** 镜头路由下禁止退回普通数据库邻接查询。 */
  isLensActive?: () => boolean
}

export function useContentViewerPreload(options: ViewerPreloadOptions) {
  const isBursting = options.isBursting ?? ref(false)
  let lastNavTimestamp = 0
  let settleTimeout: ReturnType<typeof setTimeout> | null = null

  // 浏览方向记忆：1 为向后浏览，-1 为向前浏览
  let lastDirection: 1 | -1 = 1

  // 邻近元数据缓存按 ordinary/lens 与镜头布局版本隔离；上下文变化时直接清空。
  const adjacentMetaCache = new Map<string, MediaDetail>()
  type LensNavigationContext = {
    type: 'lens'
    layoutVersion: number
    totalCount: number
    currentIndex: number | null
  }
  type AdjacentCacheScope =
    | { kind: 'ordinary' }
    | { kind: 'lens'; context: LensNavigationContext | null; layoutVersion: number }
  let adjacentCacheScope: AdjacentCacheScope | null = null

  function syncAdjacentCacheScope(): AdjacentCacheScope {
    const navCtx = options.media.navContext
    const lensActive = options.isLensActive?.() === true || navCtx?.type === 'lens'
    const next: AdjacentCacheScope = lensActive
      ? {
          kind: 'lens',
          context: navCtx?.type === 'lens' ? navCtx : null,
          layoutVersion:
            navCtx?.type === 'lens' ? navCtx.layoutVersion : options.media.layoutVersion,
        }
      : { kind: 'ordinary' }

    const changed =
      adjacentCacheScope?.kind !== next.kind ||
      (next.kind === 'lens' &&
        adjacentCacheScope?.kind === 'lens' &&
        (adjacentCacheScope.context !== next.context ||
          adjacentCacheScope.layoutVersion !== next.layoutVersion))
    if (changed) {
      adjacentMetaCache.clear()
      adjacentCacheScope = next
    } else if (adjacentCacheScope === null) {
      adjacentCacheScope = next
    }
    return adjacentCacheScope
  }

  function adjacentCacheKey(scope: AdjacentCacheScope, currentId: number, offset: number): string {
    return scope.kind === 'lens'
      ? `lens:${scope.layoutVersion}:${currentId}:${offset}`
      : `ordinary:${currentId}:${offset}`
  }

  function isCurrentLensContext(
    context: LensNavigationContext | null,
    layoutVersion: number,
  ): boolean {
    const current = options.media.navContext
    return (
      context !== null &&
      current?.type === 'lens' &&
      current === context &&
      current.layoutVersion === layoutVersion &&
      (options.isLensActive?.() ?? true)
    )
  }

  function setAdjacentMetaCache(key: string, detail: MediaDetail) {
    adjacentMetaCache.set(key, detail)
    if (adjacentMetaCache.size > 50) {
      const first = adjacentMetaCache.keys().next().value
      if (first !== undefined) adjacentMetaCache.delete(first)
    }
  }

  // 已预热缩略图集合，避免重复创建 Image
  const preloadedThumbs = new Set<string>()

  // 当前正在预解码的高清原图句柄（至多保留 1 个）
  let activeHighResImage: HTMLImageElement | null = null
  let activeHighResUrl: string | null = null

  function markNavigation(offset: number) {
    const now = Date.now()
    const delta = lastNavTimestamp > 0 ? now - lastNavTimestamp : 999
    lastNavTimestamp = now
    if (offset > 0) lastDirection = 1
    else if (offset < 0) lastDirection = -1

    // 翻页间隔小于 180ms 且大于 5ms（排除同一事件内的重复触发）判定为极速连翻/飞掠状态
    if (delta > 5 && delta < 180) {
      isBursting.value = true
      cancelHighResPreload()
    }

    if (settleTimeout) clearTimeout(settleTimeout)
    settleTimeout = setTimeout(() => {
      isBursting.value = false
      scheduleIdlePreload()
    }, 180)
  }

  function cancelHighResPreload() {
    if (activeHighResImage) {
      activeHighResImage.onload = null
      activeHighResImage.onerror = null
      activeHighResImage.src = ''
      activeHighResImage = null
      activeHighResUrl = null
    }
  }

  // 预拉取前后邻近项元数据与缩略图
  async function prefetchAdjacentMetadata(currentId: number) {
    const navCtx = options.media.navContext
    const offsets: (1 | -1)[] = lastDirection === 1 ? [1, -1] : [-1, 1]
    const scope = syncAdjacentCacheScope()

    if (scope.kind === 'lens') {
      // 镜头序只在后端布局缓存中解析；即使上下文尚未由深链补齐，也不能退回普通邻接序。
      const { layoutVersion, context } = scope
      if (!layoutVersion) return
      for (const offset of offsets) {
        const cacheKey = adjacentCacheKey(scope, currentId, offset)
        if (adjacentMetaCache.has(cacheKey)) {
          preloadThumbnail(adjacentMetaCache.get(cacheKey)!)
          continue
        }
        try {
          const result = await invokeIpc<LensAdjacentMedia | null>(IPC.GET_LENS_ADJACENT_MEDIA, {
            currentId,
            offset,
            layoutVersion,
          })
          const nextDetail = result?.detail
          if (
            nextDetail &&
            options.detail()?.id === currentId &&
            isCurrentLensContext(context, layoutVersion)
          ) {
            setAdjacentMetaCache(cacheKey, nextDetail)
            preloadThumbnail(nextDetail)
          }
        } catch {}
      }
      return
    }

    for (const offset of offsets) {
      const cacheKey = adjacentCacheKey(scope, currentId, offset)
      if (adjacentMetaCache.has(cacheKey)) {
        const cached = adjacentMetaCache.get(cacheKey)!
        preloadThumbnail(cached)
        continue
      }

      if (navCtx && navCtx.type !== 'lens') {
        const nextIdx = navCtx.currentIndex + offset
        if (nextIdx >= 0 && nextIdx < navCtx.itemIds.length) {
          const nextId = navCtx.itemIds[nextIdx]
          try {
            const nextDetail = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id: nextId })
            if (options.detail()?.id === currentId) {
              setAdjacentMetaCache(cacheKey, nextDetail)
              preloadThumbnail(nextDetail)
            }
          } catch {}
        }
      } else if (!navCtx) {
        try {
          const nextDetail = await invokeIpc<MediaDetail | null>(IPC.GET_ADJACENT_MEDIA, {
            currentId,
            offset,
          })
          if (nextDetail && options.detail()?.id === currentId) {
            setAdjacentMetaCache(cacheKey, nextDetail)
            preloadThumbnail(nextDetail)
          }
        } catch {}
      }
    }
  }

  function preloadThumbnail(item: MediaDetail) {
    if (item.mediaType !== 'image' || item.thumbStatus !== 1 || !item.thumbPath) return
    const cacheDir = options.thumbCacheDir()
    if (!cacheDir) return
    const url = buildThumbUrl(item.thumbStatus, item.thumbPath, cacheDir)
    if (!url || preloadedThumbs.has(url)) return

    preloadedThumbs.add(url)
    const img = new Image()
    img.src = url
    // 限制预热集合大小，防止无限增长
    if (preloadedThumbs.size > 50) {
      const first = preloadedThumbs.values().next().value
      if (first) preloadedThumbs.delete(first)
    }
  }

  function preloadHighRes(item: MediaDetail) {
    if (item.mediaType !== 'image' || item.availability !== 'online' || !item.absPath) return
    const url = resolveAssetUrl(item.absPath)
    if (!url || activeHighResUrl === url) return

    cancelHighResPreload()
    activeHighResUrl = url
    const img = new Image()
    activeHighResImage = img
    img.decoding = 'async'
    img.src = url
    if (typeof img.decode === 'function') {
      img.decode().catch(() => {})
    }
  }

  function scheduleIdlePreload() {
    const cur = options.detail()
    if (!cur || isBursting.value) return

    const run = () => {
      if (isBursting.value || options.detail()?.id !== cur.id) return
      void prefetchAdjacentMetadata(cur.id).then(() => {
        if (isBursting.value || options.detail()?.id !== cur.id) return
        const nextDetail = adjacentMetaCache.get(`${cur.id}:${lastDirection}`)
        if (nextDetail) preloadHighRes(nextDetail)
      })
    }

    if (typeof window !== 'undefined' && 'requestIdleCallback' in window) {
      window.requestIdleCallback(run, { timeout: 300 })
    } else {
      setTimeout(run, 60)
    }
  }

  // 当前大图就绪且非连翻时触发空闲预加载
  watch(
    () => [options.detail()?.id, options.isHighResReady()],
    ([id, ready]) => {
      if (id && ready && !isBursting.value) {
        scheduleIdlePreload()
      }
    },
    { immediate: true },
  )

  onScopeDispose(() => {
    if (settleTimeout) clearTimeout(settleTimeout)
    cancelHighResPreload()
    adjacentMetaCache.clear()
    preloadedThumbs.clear()
  })

  function getCachedAdjacent(currentId: number, offset: number): MediaDetail | undefined {
    const scope = syncAdjacentCacheScope()
    return adjacentMetaCache.get(adjacentCacheKey(scope, currentId, offset))
  }

  return {
    isBursting,
    markNavigation,
    getCachedAdjacent,
    scheduleIdlePreload,
  }
}
