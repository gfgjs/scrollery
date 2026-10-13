// 设置页缓存占用统计(缩略图/日志目录 + 缩略图缓存统计 + 视频可播产物独立池统计),从
// SettingsView.vue 下沉(超长文件拆分方案 tierB-2 §SettingsView.vue ②)。返回值名与模板
// 现有绑定逐一同名、ref/computed 本体不解包不改名(§③ 最大风险红线)。
import { ref, computed, watch, onScopeDispose } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'

// ── 缓存占用统计(镜像后端 CacheStats 的 serde camelCase 形状)────────────────
interface CacheCategoryStat {
  bytes: number
  files: number
}
export interface CacheStatsPayload {
  thumbnails: CacheCategoryStat
  aiThumbs: CacheCategoryStat
  sprites: CacheCategoryStat
  motionVideos: CacheCategoryStat
  audioCovers: CacheCategoryStat
  // 查看器渲染色域派生文件(方案 B §0②):并入 10GB 单源 LRU 预算,统计行随其余类目同源展示。
  viewerColor: CacheCategoryStat
  total: CacheCategoryStat
  limitMb: number
}

export interface VideoCacheStatsPayload {
  bytes: number
  files: number
  limitMb: number
}

export function useSettingsCacheStats() {
  const thumbCacheDir = ref('')
  const logDir = ref('')

  const cacheStats = ref<CacheStatsPayload | null>(null)
  const cacheStatsLoading = ref(false)
  let cacheDirectoryRevision = 0
  let disposed = false
  onScopeDispose(() => { disposed = true })

  watch(thumbCacheDir, (_directory, previous) => {
    // 首次路径水合后由设置页主动取数；热切换须立即撤销旧路径统计及在途回执。
    if (!previous) return
    cacheDirectoryRevision++
    cacheStats.value = null
    void refreshCacheStats()
  }, { flush: 'sync' })

  // 视频可播产物独立池默认上限(镜像 `src-tauri/src/video/playable_cache.rs::DEFAULT_VIDEO_CACHE_MAX_MB`
  // = 20 * 1024 MB):仅作 videoCacheStats 取回前/失败时的降级静态展示兜底,不代表实时占用。
  const DEFAULT_VIDEO_CACHE_MAX_BYTES = 20 * 1024 * 1024 * 1024

  // 视频可播产物独立池实时占用统计(VIDEO_CACHE_STATS,design §5.4,V7 补遗)。
  const videoCacheStats = ref<VideoCacheStatsPayload | null>(null)

  async function refreshVideoCacheStats() {
    try {
      videoCacheStats.value = await invokeIpc<VideoCacheStatsPayload>(IPC.VIDEO_CACHE_STATS)
    } catch (e) {
      logger.warn('Failed to fetch video cache stats', { error: e })
    }
  }

  // 遍历缓存目录是秒级阻塞 IO(后端 spawn_blocking):进设置页拉一次 + 手动刷新,不轮询。
  async function refreshCacheStats() {
    if (disposed || cacheStatsLoading.value) return
    const revision = cacheDirectoryRevision
    cacheStatsLoading.value = true
    try {
      const result = await invokeIpc<CacheStatsPayload>(IPC.GET_CACHE_STATS)
      if (!disposed && revision === cacheDirectoryRevision) cacheStats.value = result
    } catch (e) {
      if (!disposed && revision === cacheDirectoryRevision) {
        logger.warn('Failed to fetch cache stats', { error: e })
      }
    } finally {
      cacheStatsLoading.value = false
      // 不并行遍历大缓存；换目录发生在旧遍历期间时，收尾后只补查最新路径。
      if (!disposed && revision !== cacheDirectoryRevision) void refreshCacheStats()
    }
  }

  // 明细行序=后端类目序;label 走 i18n。
  const cacheStatRows = computed(() => {
    const s = cacheStats.value
    if (!s) return []
    return [
      { labelKey: 'settings.cacheStatsThumbnails', stat: s.thumbnails },
      { labelKey: 'settings.cacheStatsAi', stat: s.aiThumbs },
      { labelKey: 'settings.cacheStatsSprites', stat: s.sprites },
      { labelKey: 'settings.cacheStatsMotion', stat: s.motionVideos },
      { labelKey: 'settings.cacheStatsAudio', stat: s.audioCovers },
      { labelKey: 'settings.cacheStatsViewerColor', stat: s.viewerColor },
    ]
  })

  return {
    thumbCacheDir,
    logDir,
    cacheStats,
    cacheStatsLoading,
    videoCacheStats,
    DEFAULT_VIDEO_CACHE_MAX_BYTES,
    refreshCacheStats,
    refreshVideoCacheStats,
    cacheStatRows,
  }
}

export type SettingsCacheStats = ReturnType<typeof useSettingsCacheStats>
