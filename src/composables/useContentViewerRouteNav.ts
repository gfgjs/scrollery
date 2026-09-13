// src/composables/useContentViewerRouteNav.ts
// 从 ContentViewer.vue 下沉(超长文件拆分方案 2.1):路由驱动翻页。不含 close/closeViewer——
// 那两个跨多个功能域被引用,留在编排层(ContentViewer.vue)。
// 纯逻辑搬迁,不改变原调用点/参数/时序——见 docs/planning/2026-07-25-超长文件拆分方案/analysis/ContentViewer-vue.md。

import { computed } from 'vue'
import type { RouteLocationNormalizedLoaded, Router } from 'vue-router'
import type { useMediaStore } from '../stores/mediaStore'
import type { useImageEditor } from './useImageEditor'
import type { MediaDetail } from '../types/media'

export function useContentViewerRouteNav(options: {
  media: ReturnType<typeof useMediaStore>
  editor: ReturnType<typeof useImageEditor>
  router: Router
  route: RouteLocationNormalizedLoaded
  onNavigate?: (offset: number) => void
  getCachedAdjacent?: (id: number, offset: number) => MediaDetail | undefined
  /** 由画廊镜头状态提供；镜头上下文缺失时也必须禁止普通邻接降级。 */
  isLensActive?: () => boolean
}) {
  const { media, editor, router, route, onNavigate, getCachedAdjacent, isLensActive } = options

  // /view/:id 的 :id 是「当前查看项」的单一事实源。路由变化(深链 / 前进后退 / 翻页后同步)→
  // 加载该项;翻页经 navigate() 走 mediaStore 邻接后 replace 路由,loadFromRoute 守卫防重复加载。
  const routeId = computed(() => Number(route.params.id))

  function ensureLensContext() {
    if (!isLensActive?.() || media.navContext?.type === 'lens') return
    media.setLensNavContext(media.layoutVersion, media.viewTotalItems)
  }

  async function loadFromRoute(id: number) {
    if (!Number.isFinite(id)) return
    // 深链/刷新没有经过 MediaGrid 点击路径时，仍建立无数组的镜头上下文，
    // 以便后续方向键只走带 layoutVersion 的镜头 IPC。
    ensureLensContext()
    // 翻页已把 detailItem 换到目标(随后 replace 路由触发本加载)→ 免重复 IPC。
    if (media.detailItem?.id === id) return
    await media.openDetail(id)
  }

  // 统一翻页:镜头上下文经带 layoutVersion 的镜头 IPC；普通上下文保持缓存/普通邻接路径。
  // replace 同步 URL(loadFromRoute 守卫跳过重载)。onKeydown/onWheel/移动后跳转共用。
  async function navigate(offset: number) {
    if (editor.status.value !== 'idle') return // 编辑中禁翻页(方案 §7);顶栏工具栏按钮同经此函数
    onNavigate?.(offset)
    const curId = media.detailItem?.id
    const lensContext = media.navContext?.type === 'lens' || isLensActive?.() === true
    if (lensContext && media.navContext?.type !== 'lens') {
      ensureLensContext()
    }
    // 镜头中不读旧的普通缓存，也不在 miss 时回退 GET_ADJACENT_MEDIA；
    // mediaStore.navigateDetail 会把 layoutVersion 传给后端镜头缓存。
    const cached = !lensContext && curId != null ? getCachedAdjacent?.(curId, offset) : undefined
    if (cached) {
      if (media.navContext && media.navContext.type !== 'lens') {
        const nextIdx = media.navContext.itemIds.indexOf(cached.id)
        if (nextIdx >= 0) {
          media.navContext.currentIndex = nextIdx
        } else {
          const fallbackIdx = media.navContext.currentIndex + offset
          if (fallbackIdx >= 0 && fallbackIdx < media.navContext.itemIds.length) {
            media.navContext.currentIndex = fallbackIdx
          }
        }
      }
      media.detailItem = cached
      if (cached.id !== routeId.value) void router.replace(`/view/${cached.id}`)
      return
    }
    await media.navigateDetail(offset)
    const id = media.detailItem?.id
    if (id != null && id !== routeId.value) void router.replace(`/view/${id}`)
  }

  return { routeId, loadFromRoute, navigate }
}
