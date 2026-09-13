// src/composables/useThumbLoader.ts
// 缩略图加载单源（T1 §6.3）：MediaThumb / MediaThumbCompact / MediaGridCanvas 三方共用同一套
// status 1/3/0 解码 + 懒自愈逻辑，杜绝「两份 loadThumb 漂移」(方案 §11 首号风险)。
//
// 设计要点（保持 MediaThumb 原行为逐字不变）：
//  - status 1(已生成 WebP)→ 从缓存目录加载；decode() 拒绝 + img.onerror=404 → 触发一次懒自愈
//    (regenerate-thumb)，因缓存命中判定只查存在性，被 LRU 驱逐的封面须复位重生成。
//  - status 3(小文件直显)→ thumbPath 即绝对路径；无 path 时向队列请求解析。
//  - status 0(待生成)→ 请求父层生成；每挂载生命周期仅请求一次(hasRequested 守卫防环)。
//  - 竞态守卫 decodingImg：仅最后一次 loadThumb 的 img 可落 displaySrc(快滚换代不串图)。

import { ref, watch, onMounted, onBeforeUnmount } from 'vue'
import { resolveAssetUrl } from '../utils/assetUrl'
import { useThumbLoadGate } from './useThumbLoadGate'

/** 缩略图源字段（getter 形式，随 props 实时求值）。 */
export interface ThumbLoaderSource {
  id: () => number
  thumbStatus: () => number
  thumbPath: () => string | null | undefined
  cacheDir: () => string
}

/** 加载器需上抛的三类父层事件（由宿主组件的 emit 适配）。 */
export interface ThumbLoaderEmit {
  requestThumb: (id: number) => void
  cancelThumb: (id: number) => void
  regenerateThumb: (id: number) => void
}

/**
 * 由 thumb_status 1/3 构造缩略图 URL（纯函数，单源）。
 * status 1 → `${cacheDir}/thumbnails/${thumbPath}`（生成的 WebP）；
 * status 3 → thumbPath 即原文件绝对路径（小文件直显）；其余 → null。
 * @param cacheBuster 可选 `?t=xxx` 后缀（`?clear=` 强刷时附加）。
 */
export function buildThumbUrl(
  thumbStatus: number,
  thumbPath: string | null | undefined,
  cacheDir: string,
  cacheBuster = '',
): string | null {
  if (thumbStatus === 1 && thumbPath) {
    const abs = `${cacheDir}/thumbnails/${thumbPath}`.replace(/\\/g, '/')
    return resolveAssetUrl(abs) + cacheBuster
  }
  if (thumbStatus === 3 && thumbPath) {
    return resolveAssetUrl(thumbPath) + cacheBuster
  }
  return null
}

// `?clear=` 强刷缓存串：模块级快照一次（全组件共享，值恒定）。
const urlParams =
  typeof window !== 'undefined' && window.location
    ? new URLSearchParams(window.location.search)
    : null
const cacheBuster = urlParams?.get('clear') ? `?t=${urlParams.get('clear')}` : ''

/**
 * 缩略图加载 composable（挂载即加载，status 0→1/3 迁移时重载）。
 * @returns isLoaded / displaySrc（模板绑定）+ onError（<img> @error 兜底自愈）。
 */
export function useThumbLoader(src: ThumbLoaderSource, emit: ThumbLoaderEmit) {
  const isLoaded = ref(false)
  const displaySrc = ref('')
  const hasRequested = ref(false) // 守卫：每次挂载仅请求一次生成（防后端持续 status=2 时死循环）
  const hasRequestedHeal = ref(false) // 守卫：被 LRU 驱逐的封面每挂载至多自愈一次（防抖/防环）
  let decodingImg: HTMLImageElement | null = null
  // B(快滚甩滚低保真):飞掠中被闸门推迟启动的加载在此挂标，闸门放行时补起。
  const loadGate = useThumbLoadGate()
  let deferredByGate = false

  // 仅对 status=1（DB 声称已生成）触发自愈：0/2/3 由既有 pending/失败流程管，勿插手。
  function requestHealOnce() {
    if (src.thumbStatus() === 1 && !hasRequestedHeal.value) {
      hasRequestedHeal.value = true
      emit.regenerateThumb(src.id())
    }
  }

  async function loadThumb() {
    // B:快速飞掠期间推迟启动新加载(解码/生成请求),把段落地爆发里的解码工作挪到降速/
    // 停稳帧。仅拦「尚未加载」的项——已 decode 出图者不受影响;被推迟者挂标,由下方 watch
    // 在闸门放行时补起。
    if (loadGate.value && !isLoaded.value) {
      deferredByGate = true
      return
    }
    deferredByGate = false
    const status = src.thumbStatus()
    const path = src.thumbPath()

    if (status === 1 && path) {
      try {
        const url = buildThumbUrl(1, path, src.cacheDir(), cacheBuster)!
        const img = new Image()
        decodingImg = img
        // onerror 是「真·加载失败(404)」的唯一可靠信号——img.decode() 的 reject 无法区分
        // 「文件缺失」与「有效图的偶发解码拒绝」。用它把两类失败分流。
        let loadErrored = false
        img.onerror = () => {
          loadErrored = true
        }
        img.src = url
        try {
          await img.decode()
        } catch {
          // decode() 失败：可能是 404(loadErrored=true)，也可能是有效图的偶发解码拒绝
        }
        if (decodingImg !== img) return
        if (loadErrored) {
          // 文件确实缺失（多为封面被 LRU 驱逐）→ 触发一次懒自愈；不再挂 DOM <img>，省掉对同一坏
          // URL 的第 2 次 404 请求，保持占位。
          requestHealOnce()
          return
        }
        displaySrc.value = url
        isLoaded.value = true
      } catch {
        // status 1 外层吞错（无需错误对象）
      }
      return
    }

    if (status === 3) {
      if (path) {
        try {
          const url = buildThumbUrl(3, path, src.cacheDir(), cacheBuster)!
          const img = new Image()
          decodingImg = img
          img.src = url
          try {
            await img.decode()
          } catch {
            // decode() 失败回退到 DOM 加载（吞错，无需错误对象）
          }
          if (decodingImg !== img) return
          displaySrc.value = url
          isLoaded.value = true
        } catch {
          // status 3 外层吞错（无需错误对象）
        }
        return
      } else {
        // 知道是 status 3 但布局行无 absPath → 向队列请求解析（batch_request_thumbnails 快路径回填）。
        if (!hasRequested.value) {
          hasRequested.value = true
          emit.requestThumb(src.id())
        }
        return
      }
    }

    if (status === 0) {
      // 尚未生成 — 请求父层生成（每挂载仅一次，防后端持续失败时死循环）。
      if (!hasRequested.value) {
        hasRequested.value = true
        emit.requestThumb(src.id())
      }
    }
  }

  // <img> 真加载失败（如 decode 偶发拒绝路径下文件其实也缺失）→ 兜底触发一次自愈。
  function onError() {
    displaySrc.value = ''
    isLoaded.value = false
    requestHealOnce()
  }

  // 仅当 thumbPath/thumbStatus 确实迁移到可用值(0→1 / 0→3)时才重载（父层批处理回填后）。
  watch(
    () => [src.thumbPath(), src.thumbStatus()] as const,
    ([, newStatus]) => {
      if (newStatus === 1 || newStatus === 3) loadThumb()
    },
  )

  // B:闸门放行(true→false)→ 补起飞掠期间被推迟的加载。仅对本卡确有待载且尚未加载时触发，
  // 故停稳一刻只有可见窗口内真正缺图的卡各自发起一次加载(面积有界)。
  watch(loadGate, (deferred) => {
    if (!deferred && deferredByGate && !isLoaded.value) loadThumb()
  })

  onMounted(() => loadThumb())

  onBeforeUnmount(() => {
    if (decodingImg) {
      decodingImg.src = ''
      decodingImg = null
    }
    if (hasRequested.value && !isLoaded.value) {
      emit.cancelThumb(src.id())
    }
  })

  return { isLoaded, displaySrc, onError }
}
