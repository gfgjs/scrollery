// src/composables/useContentViewerPreload.ts
// 大图查看器前后邻近项预加载与极速飞掠保护：
// 1. 邻近元数据预取：空闲时预查 N+1 与 N-1 的 MediaDetail 缓存在内存中，消除翻页 IPC 往返延迟。
// 2. 邻近缩略图预热：空闲时将前后缩略图预热入浏览器缓存，减少翻页等待。
// 3. 高清原图空闲预解码：当前图就绪后，沿浏览方向对前方 1 张原图执行低优先级 Image.decode()。
// 4. 极速飞掠保护（Burst Mode）：鼠标滚轮猛滚或按住方向键连翻时，自动进入极速模式，跳过重型原图解码，
//    仅流水线渲染轻量缩略图保持满帧，停稳后（settle）再触发最终停留项的高清解码。

import { ref, watch, onScopeDispose, type Ref } from 'vue'
import { resolveAssetUrl } from '../utils/assetUrl'
import { buildThumbUrl } from './useThumbLoader'
import type { MediaDetail } from '../types/media'
import type { AdjacentDetail, NavigationContext, useMediaStore } from '../stores/mediaStore'

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

  // 上下文对象代表一次导航会话；普通顺序、独立搜索列表与两种镜头不共用邻居缓存。
  const adjacentMetaCache = new Map<string, AdjacentDetail>()
  const adjacentInFlight = new Map<string, Promise<void>>()
  let adjacentCacheScope: NavigationContext | null = null
  let disposed = false
  let idleTimer: ReturnType<typeof setTimeout> | null = null
  let idleHandle: number | null = null

  function syncAdjacentCacheScope(): NavigationContext | null {
    const context = options.media.navContext
    const next = options.media.isNavContextCurrent(context) &&
      (!options.isLensActive?.() || context.type === 'lens') ? context : null
    if (adjacentCacheScope !== next) {
      adjacentMetaCache.clear()
      adjacentInFlight.clear()
      cancelHighResPreload()
      adjacentCacheScope = next
    }
    return next
  }

  function isCurrent(context: NavigationContext, currentId: number): boolean {
    return !disposed && !isBursting.value && options.detail()?.id === currentId &&
      syncAdjacentCacheScope() === context
  }

  function setAdjacentMetaCache(key: string, result: AdjacentDetail) {
    adjacentMetaCache.set(key, result)
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

  // 同一上下文的重复空闲回调共用在途查询；换上下文后旧响应不能写入新缓存或预热。
  async function prefetchAdjacentMetadata(context: NavigationContext, currentId: number) {
    const offsets = lastDirection === 1 ? [1, -1] : [-1, 1]
    for (const offset of offsets) {
      if (!isCurrent(context, currentId)) return
      const key = currentId + ':' + offset
      const cached = adjacentMetaCache.get(key)
      if (cached) {
        preloadThumbnail(cached.detail)
        continue
      }
      const running = adjacentInFlight.get(key)
      if (running) { await running; continue }
      const request = (async () => {
        try {
          const result = await options.media.fetchAdjacentDetail(context, currentId, offset)
          if (result && isCurrent(context, currentId)) {
            setAdjacentMetaCache(key, result)
            preloadThumbnail(result.detail)
          }
        } catch { /* 预取失败由下次显式翻页处理,不自动重试。 */ }
      })()
      adjacentInFlight.set(key, request)
      try { await request } finally {
        if (adjacentInFlight.get(key) === request) adjacentInFlight.delete(key)
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

  function cancelIdlePreload() {
    if (idleTimer !== null) clearTimeout(idleTimer)
    if (idleHandle !== null && typeof window.cancelIdleCallback === 'function') window.cancelIdleCallback(idleHandle)
    idleTimer = null
    idleHandle = null
  }

  function scheduleIdlePreload() {
    cancelIdlePreload()
    const cur = options.detail()
    const context = syncAdjacentCacheScope()
    if (!cur || !context || !isCurrent(context, cur.id)) return
    const run = () => {
      idleTimer = null
      idleHandle = null
      if (!isCurrent(context, cur.id)) return
      void prefetchAdjacentMetadata(context, cur.id).then(() => {
        if (!isCurrent(context, cur.id)) return
        const next = adjacentMetaCache.get(cur.id + ':' + lastDirection)
        if (next) preloadHighRes(next.detail)
      })
    }
    if (typeof window !== 'undefined' && 'requestIdleCallback' in window) {
      idleHandle = window.requestIdleCallback(run, { timeout: 300 })
    } else {
      idleTimer = setTimeout(run, 60)
    }
  }

  // 当前大图就绪且非连翻时触发空闲预加载
  watch(
    () => [options.detail()?.id, options.isHighResReady(), syncAdjacentCacheScope()],
    ([id, ready]) => {
      if (id && ready && !isBursting.value) {
        scheduleIdlePreload()
      }
    },
    { immediate: true },
  )

  onScopeDispose(() => {
    disposed = true
    cancelIdlePreload()
    adjacentInFlight.clear()
    if (settleTimeout) clearTimeout(settleTimeout)
    cancelHighResPreload()
    adjacentMetaCache.clear()
    preloadedThumbs.clear()
  })

  function getCachedAdjacent(currentId: number, offset: number): AdjacentDetail | undefined {
    if (disposed || !syncAdjacentCacheScope()) return undefined
    return adjacentMetaCache.get(currentId + ':' + offset)
  }

  return {
    isBursting,
    markNavigation,
    getCachedAdjacent,
    scheduleIdlePreload,
  }
}
